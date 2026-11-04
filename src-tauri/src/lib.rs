pub mod decide;
pub mod execute;
pub mod guard;
pub mod job;
pub mod joblog;
pub mod meta;
pub mod model;
pub mod outcome;
pub mod planner;
pub mod platform;
pub mod rclone;
pub mod scan;
pub mod settings;
pub mod target;

use chrono::Local;
use execute::Transfer;
use model::{Operation, Plan, PlanOptions, Warning};
use outcome::Outcome;
use serde::{Deserialize, Serialize};
use settings::Settings;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_opener::OpenerExt;

/// The most recent Preview. Run executes exactly this plan.
#[derive(Clone)]
struct Previewed {
    sources: Vec<PathBuf>,
    target: PathBuf,
    options: PlanOptions,
    plan: Arc<Plan>,
}

#[derive(Default)]
struct AppState {
    cancel: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
    next_plan_id: AtomicU64,
    last: Mutex<Option<Previewed>>,
    settings: Mutex<Settings>,
    /// Logs written this session; only these may be opened from the UI.
    logs: Mutex<HashSet<PathBuf>>,
}

fn os_text() -> String {
    let e = platform::environment();
    format!("{} {}", e.os, e.version)
}

fn settings_file(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_config_dir()
        .map(|d| d.join("settings.json"))
        .map_err(|e| e.to_string())
}

/// Per-user log folder: Windows `%LOCALAPPDATA%\com.polimath.filecopy\logs`,
/// macOS `~/Library/Logs/com.polimath.filecopy`, Linux
/// `~/.local/share/com.polimath.filecopy/logs`.
fn default_log_dir(app: &AppHandle) -> PathBuf {
    app.path()
        .app_log_dir()
        .unwrap_or_else(|_| std::env::temp_dir().join("polimath-file-copy-logs"))
}

fn log_dir(app: &AppHandle) -> PathBuf {
    let st = app.state::<AppState>();
    let s = st.settings.lock().unwrap();
    s.effective_log_dir(&default_log_dir(app))
}

fn remember_log(app: &AppHandle, p: &Path) {
    app.state::<AppState>().logs.lock().unwrap().insert(p.to_path_buf());
}

// ---------- settings ----------

#[derive(Serialize)]
struct SettingsView {
    settings: Settings,
    default_log_dir: String,
    effective_log_dir: String,
}

fn settings_view(app: &AppHandle, s: &Settings) -> SettingsView {
    let default = default_log_dir(app);
    SettingsView {
        settings: s.clone(),
        effective_log_dir: s.effective_log_dir(&default).display().to_string(),
        default_log_dir: default.display().to_string(),
    }
}

#[tauri::command]
fn get_settings(app: AppHandle, state: State<'_, AppState>) -> SettingsView {
    let s = state.settings.lock().unwrap().clone();
    settings_view(&app, &s)
}

#[tauri::command]
fn set_settings(app: AppHandle, state: State<'_, AppState>, settings: Settings) -> Result<SettingsView, String> {
    let file = settings_file(&app)?;
    settings.save(&file).map_err(|e| format!("Could not save settings to {}: {e}", file.display()))?;
    *state.settings.lock().unwrap() = settings.clone();
    Ok(settings_view(&app, &settings))
}

// ---------- preview ----------

#[derive(Deserialize)]
struct PlanRequest {
    sources: Vec<PathBuf>,
    target: PathBuf,
    options: PlanOptions,
    #[serde(default)]
    log: bool,
}

#[tauri::command]
async fn plan(app: AppHandle, req: PlanRequest) -> Result<Plan, String> {
    let dir = req.log.then(|| log_dir(&app));
    let (req, mut plan) = tauri::async_runtime::spawn_blocking(move || {
        let started = Local::now();
        let mut plan = planner::build_plan(&req.sources, &req.target, &req.options);
        if let Some(dir) = dir {
            match job::write_preview_log(&dir, &req.sources, &req.target, &req.options, &plan, started) {
                Ok(p) => plan.log_path = Some(p.display().to_string()),
                Err(e) => plan.log_error = Some(e),
            }
        }
        (req, plan)
    })
    .await
    .map_err(|e| e.to_string())?;

    let state = app.state::<AppState>();
    plan.id = state.next_plan_id.fetch_add(1, Ordering::SeqCst) + 1;
    if let Some(p) = &plan.log_path {
        remember_log(&app, Path::new(p));
    }
    *state.last.lock().unwrap() = Some(Previewed {
        sources: req.sources,
        target: req.target,
        options: req.options,
        plan: Arc::new(plan.clone()),
    });
    Ok(plan)
}

// ---------- run ----------

#[derive(Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Engine {
    Native,
    Rclone,
}

#[derive(Deserialize)]
struct ExecuteRequest {
    plan_id: u64,
    engine: Engine,
    parallel: usize,
    #[serde(default)]
    log: bool,
}

#[tauri::command]
fn execute(app: AppHandle, state: State<'_, AppState>, req: ExecuteRequest) -> Result<(), String> {
    let previewed = state
        .last
        .lock()
        .unwrap()
        .clone()
        .filter(|p| p.plan.id == req.plan_id)
        .ok_or("The preview is out of date. Preview again, then Run.")?;
    if state.running.swap(true, Ordering::SeqCst) {
        return Err("A job is already running.".into());
    }
    state.cancel.store(false, Ordering::SeqCst);
    let cancel = state.cancel.clone();
    let running = state.running.clone();
    let dir = req.log.then(|| log_dir(&app));

    std::thread::spawn(move || {
        let out = run_job(&app, &previewed, &req, dir.as_deref(), &cancel);
        running.store(false, Ordering::SeqCst);
        let _ = app.emit("job-finished", out);
    });
    Ok(())
}

fn run_job(app: &AppHandle, pv: &Previewed, req: &ExecuteRequest, dir: Option<&Path>, cancel: &AtomicBool) -> Outcome {
    let engine_name = match req.engine {
        Engine::Native => "built-in",
        Engine::Rclone => "rclone",
    };
    let engine = || -> Result<Box<dyn Transfer>, String> {
        match req.engine {
            Engine::Native => Ok(Box::new(execute::Native)),
            Engine::Rclone => rclone::Rclone::start().map(|r| Box::new(r) as Box<dyn Transfer>),
        }
    };
    let job = job::RunJob {
        sources: &pv.sources,
        target: &pv.target,
        options: &pv.options,
        plan: &pv.plan,
        engine_name,
        parallel: req.parallel,
        log_dir: dir,
        os: os_text(),
    };
    let out = job.run(engine, cancel, &|p| {
        let _ = app.emit("transfer-progress", p);
    });
    if let Some(p) = &out.log_path {
        remember_log(app, Path::new(p));
    }
    out
}

#[tauri::command]
fn cancel(state: State<'_, AppState>) {
    state.cancel.store(true, Ordering::SeqCst);
}

// ---------- logs ----------

fn known_log(state: &AppState, path: &str) -> Result<PathBuf, String> {
    let p = PathBuf::from(path);
    if state.logs.lock().unwrap().contains(&p) {
        Ok(p)
    } else {
        Err("Only logs written by this app in this session can be opened here.".into())
    }
}

/// Open a log in the OS's default app for `.log` files.
#[tauri::command]
fn open_log(app: AppHandle, state: State<'_, AppState>, path: String) -> Result<(), String> {
    let p = known_log(&state, &path)?;
    app.opener()
        .open_path(p.to_string_lossy(), None::<&str>)
        .map_err(|e| e.to_string())
}

/// Show a log selected in Explorer / Finder / the file manager.
#[tauri::command]
fn reveal_log(app: AppHandle, state: State<'_, AppState>, path: String) -> Result<(), String> {
    let p = known_log(&state, &path)?;
    app.opener().reveal_item_in_dir(p).map_err(|e| e.to_string())
}

/// Open the configured log folder (creating it if needed).
#[tauri::command]
fn open_log_folder(app: AppHandle) -> Result<(), String> {
    let dir = log_dir(&app);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    app.opener()
        .open_path(dir.to_string_lossy(), None::<&str>)
        .map_err(|e| e.to_string())
}

// ---------- misc ----------

#[tauri::command]
fn environment() -> platform::Environment {
    platform::environment().clone()
}

#[derive(Deserialize)]
struct CheckRequest {
    sources: Vec<PathBuf>,
    target: Option<PathBuf>,
    operation: Operation,
    engine: Engine,
}

/// Path-level guardrails, shown before Preview.
#[tauri::command]
async fn check(req: CheckRequest) -> Vec<Warning> {
    tauri::async_runtime::spawn_blocking(move || {
        guard::check_paths(&req.sources, req.target.as_deref(), req.operation, req.engine == Engine::Rclone)
    })
    .await
    .unwrap_or_default()
}

#[tauri::command]
async fn rclone_version() -> Option<String> {
    tauri::async_runtime::spawn_blocking(rclone::version).await.ok().flatten()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(AppState::default())
        .setup(|app| {
            let handle = app.handle();
            if let Ok(file) = settings_file(handle) {
                *app.state::<AppState>().settings.lock().unwrap() = Settings::load(&file);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            plan,
            execute,
            cancel,
            rclone_version,
            environment,
            check,
            get_settings,
            set_settings,
            open_log,
            reveal_log,
            open_log_folder
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

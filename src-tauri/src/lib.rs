pub mod decide;
pub mod execute;
pub mod guard;
pub mod meta;
pub mod model;
pub mod planner;
pub mod platform;
pub mod rclone;
pub mod scan;
pub mod target;

use execute::{Job, Summary, Transfer};
use model::{Operation, Plan, PlanOptions, Warning};
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter, State};

#[derive(Default)]
struct AppState {
    cancel: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
}

#[derive(Deserialize)]
struct PlanRequest {
    sources: Vec<PathBuf>,
    target: PathBuf,
    options: PlanOptions,
}

#[tauri::command]
async fn plan(req: PlanRequest) -> Result<Plan, String> {
    tauri::async_runtime::spawn_blocking(move || {
        planner::build_plan(&req.sources, &req.target, &req.options)
    })
    .await
    .map_err(|e| e.to_string())
}

#[derive(Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Engine {
    Native,
    Rclone,
}

#[derive(Deserialize)]
struct ExecuteRequest {
    jobs: Vec<Job>,
    operation: Operation,
    engine: Engine,
    parallel: usize,
}

#[tauri::command]
fn execute(app: AppHandle, state: State<'_, AppState>, req: ExecuteRequest) -> Result<(), String> {
    if state.running.swap(true, Ordering::SeqCst) {
        return Err("a transfer is already running".into());
    }
    state.cancel.store(false, Ordering::SeqCst);
    let cancel = state.cancel.clone();
    let running = state.running.clone();

    std::thread::spawn(move || {
        let engine: Result<Box<dyn Transfer>, String> = match req.engine {
            Engine::Native => Ok(Box::new(execute::Native)),
            Engine::Rclone => rclone::Rclone::start().map(|r| Box::new(r) as Box<dyn Transfer>),
        };
        let result: Result<Summary, String> = engine.map(|engine| {
            execute::run(&req.jobs, req.operation, engine.as_ref(), req.parallel, &cancel, &|p| {
                let _ = app.emit("transfer-progress", p);
            })
        });
        running.store(false, Ordering::SeqCst);
        let _ = app.emit("transfer-done", result);
    });
    Ok(())
}

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
fn cancel(state: State<'_, AppState>) {
    state.cancel.store(true, Ordering::SeqCst);
}

#[tauri::command]
async fn rclone_version() -> Option<String> {
    tauri::async_runtime::spawn_blocking(rclone::version).await.ok().flatten()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![plan, execute, cancel, rclone_version, environment, check])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

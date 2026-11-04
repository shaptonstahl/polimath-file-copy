//! Preview and Run as whole jobs, including their logs. Kept free of Tauri
//! types so it can be tested end to end.

use crate::execute::{self, Job, Progress, Transfer};
use crate::joblog::{self, ItemResult, JobInfo, Mode};
use crate::model::{Action, Plan, PlanOptions};
use crate::outcome::{self, Outcome};
use chrono::{DateTime, Local};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;

pub fn write_preview_log(
    dir: &Path,
    sources: &[PathBuf],
    target: &Path,
    options: &PlanOptions,
    plan: &Plan,
    started: DateTime<Local>,
) -> Result<PathBuf, String> {
    let info = JobInfo {
        mode: Mode::Preview,
        started,
        sources,
        target,
        options,
        engine: None,
        parallel: None,
        os: os_text(),
    };
    let (path, file) =
        joblog::create(dir, &info).map_err(|e| format!("Could not create a log in {}: {e}", dir.display()))?;
    let out = outcome::preview(plan, options.operation);
    joblog::write_body(&file, plan, options.operation, None, started, &out.message)
        .map_err(|e| format!("The log {} is incomplete: {e}", path.display()))?;
    Ok(path)
}

fn os_text() -> String {
    let e = crate::platform::environment();
    format!("{} {}", e.os, e.version)
}

pub struct RunJob<'a> {
    pub sources: &'a [PathBuf],
    pub target: &'a Path,
    pub options: &'a PlanOptions,
    pub plan: &'a Plan,
    pub engine_name: &'a str,
    pub parallel: usize,
    /// `Some` to write a log there.
    pub log_dir: Option<&'a Path>,
    pub os: String,
}

impl RunJob<'_> {
    /// Execute every non-skipped item of the plan and return how it ended.
    /// `engine` is called once; an error means nothing was attempted.
    pub fn run(
        &self,
        engine: impl FnOnce() -> Result<Box<dyn Transfer>, String>,
        cancel: &AtomicBool,
        on_progress: &(dyn Fn(Progress) + Sync),
    ) -> Outcome {
        let started = Local::now();
        let plan = self.plan;
        let op = self.options.operation;

        // Header first, so a log exists even if the app is killed mid-run.
        let mut log_error = None;
        let log = self.log_dir.and_then(|dir| {
            let info = JobInfo {
                mode: Mode::Run,
                started,
                sources: self.sources,
                target: self.target,
                options: self.options,
                engine: Some(self.engine_name),
                parallel: Some(self.parallel),
                os: self.os.clone(),
            };
            joblog::create(dir, &info)
                .map_err(|e| log_error = Some(format!("Could not create a log in {}: {e}", dir.display())))
                .ok()
        });

        let runnable: Vec<usize> = plan
            .items
            .iter()
            .enumerate()
            .filter(|(_, i)| !matches!(i.action, Action::Skip { .. }))
            .map(|(n, _)| n)
            .collect();
        let jobs: Vec<Job> = runnable
            .iter()
            .map(|&n| {
                let it = &plan.items[n];
                Job { source: it.source.clone(), dest: it.dest.clone(), rename_only: it.rename_only }
            })
            .collect();
        let results: Mutex<Vec<ItemResult>> = Mutex::new(
            plan.items
                .iter()
                .map(|i| {
                    if matches!(i.action, Action::Skip { .. }) { ItemResult::Skipped } else { ItemResult::NotAttempted }
                })
                .collect(),
        );

        let (done, failed, start_error) = match engine() {
            Ok(engine) => {
                let s = execute::run(&jobs, op, engine.as_ref(), self.parallel, cancel, &|p| {
                    results.lock().unwrap()[runnable[p.index]] = match &p.error {
                        None => ItemResult::Done,
                        Some(e) => ItemResult::Failed(e.clone()),
                    };
                    on_progress(p);
                });
                (s.ok, s.failed, None)
            }
            Err(e) => (0, 0, Some(e)),
        };
        let cancelled = start_error.is_none() && done + failed < jobs.len();
        let mut out = outcome::run(plan, op, done, failed, cancelled, start_error.as_deref());

        if let Some((path, file)) = log {
            let results = results.into_inner().unwrap();
            let summary = format!("{} - {}", out.title.to_uppercase(), out.message);
            if let Err(e) = joblog::write_body(&file, plan, op, Some(&results), started, &summary) {
                out.log_error = Some(format!("The log is incomplete: {e}"));
            }
            out.log_path = Some(path.display().to_string());
        } else {
            out.log_error = log_error;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Operation, OverwriteRule};
    use crate::outcome::Status;
    use crate::planner;
    use std::fs;

    struct Setup {
        _root: tempfile::TempDir,
        src: PathBuf,
        dst: PathBuf,
        logs: PathBuf,
    }

    fn setup() -> Setup {
        let root = tempfile::tempdir().unwrap();
        let src = root.path().join("src");
        let dst = root.path().join("dst");
        let logs = root.path().join("logs");
        fs::create_dir_all(src.join("Docs")).unwrap();
        fs::create_dir_all(dst.join("Docs")).unwrap();
        fs::write(src.join("Docs/new.txt"), b"new").unwrap();
        fs::write(src.join("Docs/same.txt"), b"same").unwrap();
        fs::write(dst.join("Docs/same.txt"), b"same").unwrap();
        Setup { _root: root, src, dst, logs }
    }

    fn data_rows(text: &str) -> Vec<String> {
        text.lines().filter(|l| !l.starts_with('#')).skip(1).map(str::to_owned).collect()
    }

    #[test]
    fn preview_then_run_with_logs() {
        let s = setup();
        let sources = vec![s.src.join("Docs")];
        let options = PlanOptions { rule: OverwriteRule::Never, ..Default::default() };
        let plan = planner::build_plan(&sources, &s.dst, &options);

        // Preview log.
        let p = write_preview_log(&s.logs, &sources, &s.dst, &options, &plan, Local::now()).unwrap();
        assert!(p.file_name().unwrap().to_string_lossy().ends_with("_copy_preview.log"));
        let text = fs::read_to_string(&p).unwrap();
        assert!(text.contains("PREVIEW ONLY - no action was taken. 1 file would copy; 1 file would be skipped."));
        assert_eq!(data_rows(&text).len(), 2);
        assert!(!s.dst.join("Docs/new.txt").exists(), "preview must not copy");

        // Run log.
        let job = RunJob {
            sources: &sources,
            target: &s.dst,
            options: &options,
            plan: &plan,
            engine_name: "built-in",
            parallel: 2,
            log_dir: Some(&s.logs),
            os: "test".into(),
        };
        let never = AtomicBool::new(false);
        let out = job.run(|| Ok(Box::new(execute::Native)), &never, &|_| {});
        assert_eq!(out.status, Status::Success);
        assert_eq!((out.done, out.skipped), (1, 1));
        assert!(s.dst.join("Docs/new.txt").exists());
        let log = PathBuf::from(out.log_path.unwrap());
        assert!(!log.to_string_lossy().contains("preview"));
        let text = fs::read_to_string(&log).unwrap();
        let rows = data_rows(&text);
        assert!(rows.iter().any(|r| r.contains("\tcreate\t") && r.contains("\tcopied\t")));
        assert!(rows.iter().any(|r| r.contains("\tskip\t") && r.contains("\tskipped\t")));
        assert!(text.contains("# Result: SUCCESS - 1 of 1 file copied."));
    }

    #[test]
    fn failed_start_and_unwritable_log_are_reported() {
        let s = setup();
        let sources = vec![s.src.join("Docs")];
        let options = PlanOptions { operation: Operation::Move, ..Default::default() };
        let plan = planner::build_plan(&sources, &s.dst, &options);
        // A file where the log folder should be makes the log impossible.
        let blocker = s.logs.clone();
        fs::write(&blocker, b"not a folder").unwrap();
        let job = RunJob {
            sources: &sources,
            target: &s.dst,
            options: &options,
            plan: &plan,
            engine_name: "rclone",
            parallel: 1,
            log_dir: Some(&blocker),
            os: "test".into(),
        };
        let never = AtomicBool::new(false);
        let out = job.run(|| Err("rclone not found".into()), &never, &|_| {});
        assert_eq!(out.status, Status::Failed);
        assert!(out.message.contains("rclone not found"));
        assert!(out.log_path.is_none());
        assert!(out.log_error.unwrap().contains("Could not create a log"));
        assert!(s.src.join("Docs/new.txt").exists(), "nothing moved");
    }
}

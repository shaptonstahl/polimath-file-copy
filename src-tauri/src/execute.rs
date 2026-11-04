//! Carry out a plan with a pluggable transfer engine, several files at a time
//! (parallel transfers hide per-file latency on VPNs and NAS shares).

use crate::model::Operation;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

pub trait Transfer: Send + Sync {
    fn copy(&self, src: &Path, dst: &Path) -> Result<(), String>;
    fn move_file(&self, src: &Path, dst: &Path) -> Result<(), String>;
}

#[derive(Debug, Clone, Deserialize)]
pub struct Job {
    pub source: PathBuf,
    pub dest: PathBuf,
    /// Cloud-only file moved within its sync root: rename it or fail, never
    /// fall back to copy + delete (that would download it).
    #[serde(default)]
    pub rename_only: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Progress {
    pub index: usize,
    pub done: usize,
    pub total: usize,
    pub source: PathBuf,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Summary {
    pub ok: usize,
    pub failed: usize,
    pub cancelled: usize,
}

pub fn run(
    jobs: &[Job],
    op: Operation,
    engine: &dyn Transfer,
    parallel: usize,
    cancel: &AtomicBool,
    on_progress: &(dyn Fn(Progress) + Sync),
) -> Summary {
    let next = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    let ok = AtomicUsize::new(0);
    let failed = AtomicUsize::new(0);
    let total = jobs.len();

    std::thread::scope(|s| {
        for _ in 0..parallel.clamp(1, 32).min(total.max(1)) {
            s.spawn(|| loop {
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
                let i = next.fetch_add(1, Ordering::Relaxed);
                let Some(job) = jobs.get(i) else { break };
                let res = if job.rename_only {
                    rename_only(&job.source, &job.dest)
                } else {
                    match op {
                        Operation::Copy => engine.copy(&job.source, &job.dest),
                        Operation::Move => engine.move_file(&job.source, &job.dest),
                    }
                };
                match &res {
                    Ok(()) => ok.fetch_add(1, Ordering::Relaxed),
                    Err(_) => failed.fetch_add(1, Ordering::Relaxed),
                };
                on_progress(Progress {
                    index: i,
                    done: done.fetch_add(1, Ordering::Relaxed) + 1,
                    total,
                    source: job.source.clone(),
                    error: res.err(),
                });
            });
        }
    });

    let ok = ok.into_inner();
    let failed = failed.into_inner();
    Summary { ok, failed, cancelled: total - ok - failed }
}

fn rename_only(src: &Path, dst: &Path) -> Result<(), String> {
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::rename(src, dst).map_err(|e| {
        format!("rename failed ({e}); not copying instead, because the file is cloud-only and would be downloaded")
    })
}

/// Plain file-system copy. Writes to a temporary name and renames into place,
/// so an interrupted copy never leaves a truncated file under the real name,
/// and preserves the modified time so later "replace older" checks work.
pub struct Native;

impl Native {
    fn copy_inner(src: &Path, dst: &Path) -> std::io::Result<()> {
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut tmp_name = dst.file_name().unwrap_or_default().to_os_string();
        tmp_name.push(".pfc-partial");
        let tmp = dst.with_file_name(tmp_name);
        let result = (|| {
            fs::copy(src, &tmp)?;
            let md = fs::metadata(src)?;
            filetime::set_file_mtime(&tmp, filetime::FileTime::from_last_modification_time(&md))?;
            fs::rename(&tmp, dst)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        result
    }
}

impl Transfer for Native {
    fn copy(&self, src: &Path, dst: &Path) -> Result<(), String> {
        Self::copy_inner(src, dst).map_err(|e| e.to_string())
    }

    fn move_file(&self, src: &Path, dst: &Path) -> Result<(), String> {
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        // Same volume: a rename, no data moves. Otherwise copy then delete.
        if fs::rename(src, dst).is_ok() {
            return Ok(());
        }
        Self::copy_inner(src, dst).map_err(|e| e.to_string())?;
        fs::remove_file(src).map_err(|e| format!("copied, but could not remove source: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use filetime::FileTime;

    #[test]
    fn native_copy_and_move() {
        let root = tempfile::tempdir().unwrap();
        let a = root.path().join("a.txt");
        fs::write(&a, b"data").unwrap();
        filetime::set_file_mtime(&a, FileTime::from_unix_time(1_500_000_000, 0)).unwrap();
        let b = root.path().join("out/deep/b.txt");
        let c = root.path().join("moved/c.txt");

        let jobs = vec![Job { source: a.clone(), dest: b.clone(), rename_only: false }];
        let never = AtomicBool::new(false);
        let s = run(&jobs, Operation::Copy, &Native, 4, &never, &|_| {});
        assert_eq!((s.ok, s.failed), (1, 0));
        assert_eq!(fs::read(&b).unwrap(), b"data");
        let mt = FileTime::from_last_modification_time(&fs::metadata(&b).unwrap());
        assert_eq!(mt.unix_seconds(), 1_500_000_000);

        let jobs = vec![Job { source: b.clone(), dest: c.clone(), rename_only: true }];
        let s = run(&jobs, Operation::Move, &Native, 1, &never, &|_| {});
        assert_eq!(s.ok, 1);
        assert!(!b.exists() && c.exists());
    }

    #[test]
    fn rename_only_never_falls_back_to_copy() {
        let root = tempfile::tempdir().unwrap();
        let missing = root.path().join("missing.txt");
        let jobs = vec![Job { source: missing, dest: root.path().join("out.txt"), rename_only: true }];
        let never = AtomicBool::new(false);
        let s = run(&jobs, Operation::Move, &Native, 1, &never, &|p| {
            assert!(p.error.unwrap().contains("not copying instead"));
        });
        assert_eq!(s.failed, 1);
    }

    #[test]
    fn cancel_stops_before_starting() {
        let jobs = vec![Job { source: "x".into(), dest: "y".into(), rename_only: false }];
        let cancel = AtomicBool::new(true);
        let s = run(&jobs, Operation::Copy, &Native, 1, &cancel, &|_| {});
        assert_eq!(s.cancelled, 1);
    }
}

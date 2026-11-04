//! Job logs: one plain-text file per Preview or Run.
//!
//! Layout:
//! * `#` lines at the top describe the job (mode, options, sources, target,
//!   plan summary, warnings, errors).
//! * One tab-separated header row, then one row per source file with the
//!   metadata behind the decision, the decision, and the result.
//! * `#` lines at the bottom give the outcome.
//!
//! The rows open cleanly in a spreadsheet: skip the `#` lines and use tab
//! as the separator.

use crate::model::{Action, FileMeta, Operation, Plan, PlanItem, PlanOptions};
use chrono::{DateTime, Local, TimeZone};
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Preview,
    Run,
}

/// What happened to one plan item during a Run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemResult {
    /// Preview: nothing was attempted.
    NotRun,
    /// The plan said skip.
    Skipped,
    Done,
    Failed(String),
    /// The run stopped before this file was attempted.
    NotAttempted,
}

impl ItemResult {
    fn text(&self, op: Operation) -> String {
        match self {
            Self::NotRun => "not run (preview)".into(),
            Self::Skipped => "skipped".into(),
            Self::Done => if op == Operation::Move { "moved" } else { "copied" }.into(),
            Self::Failed(e) => format!("FAILED: {e}"),
            Self::NotAttempted => "not attempted (job stopped)".into(),
        }
    }
}

pub struct JobInfo<'a> {
    pub mode: Mode,
    pub started: DateTime<Local>,
    pub sources: &'a [PathBuf],
    pub target: &'a Path,
    pub options: &'a PlanOptions,
    /// Run only.
    pub engine: Option<&'a str>,
    pub parallel: Option<usize>,
    pub os: String,
}

pub const PREVIEW_BANNER: &str = "PREVIEW ONLY - no files were copied, moved, overwritten or deleted.";

pub fn file_name(started: DateTime<Local>, op: Operation, mode: Mode) -> String {
    format!(
        "polimath-file-copy_{}_{}{}.log",
        started.format("%Y-%m-%d_%H%M%S"),
        op_name(op),
        if mode == Mode::Preview { "_preview" } else { "" }
    )
}

fn op_name(op: Operation) -> &'static str {
    match op {
        Operation::Copy => "copy",
        Operation::Move => "move",
    }
}

/// Create the log file (never overwriting an existing one) and write the
/// job header. Returns the path and an open file to append the rest to.
pub fn create(dir: &Path, info: &JobInfo) -> io::Result<(PathBuf, File)> {
    fs::create_dir_all(dir)?;
    let base = file_name(info.started, info.options.operation, info.mode);
    let mut path = dir.join(&base);
    let mut n = 2;
    let file = loop {
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(f) => break f,
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists && n < 1000 => {
                path = dir.join(base.replace(".log", &format!("-{n}.log")));
                n += 1;
            }
            Err(e) => return Err(e),
        }
    };
    let mut w = BufWriter::new(&file);
    write_header(&mut w, info)?;
    w.flush()?;
    drop(w);
    Ok((path, file))
}

fn yes_no(b: bool) -> &'static str {
    if b { "yes" } else { "no" }
}

fn write_header(w: &mut impl Write, info: &JobInfo) -> io::Result<()> {
    let o = info.options;
    writeln!(w, "# Polimath File Copy job log")?;
    match info.mode {
        Mode::Preview => writeln!(w, "# Mode: {PREVIEW_BANNER}")?,
        Mode::Run => writeln!(w, "# Mode: RUN - files were {}.", if o.operation == Operation::Move { "moved" } else { "copied" })?,
    }
    writeln!(w, "# App version: {}", env!("CARGO_PKG_VERSION"))?;
    writeln!(w, "# OS: {}", info.os)?;
    writeln!(w, "# Started: {}", info.started.to_rfc3339_opts(chrono::SecondsFormat::Secs, false))?;
    writeln!(w, "# Operation: {}", op_name(o.operation))?;
    writeln!(w, "# If target exists: {}", o.rule.label())?;
    writeln!(w, "# Time tolerance: {} s", o.tolerance_ms as f64 / 1000.0)?;
    writeln!(w, "# Skip cloud-only sources: {}", yes_no(o.skip_cloud_only))?;
    if let Some(e) = info.engine {
        writeln!(w, "# Engine: {e}")?;
    }
    if let Some(p) = info.parallel {
        writeln!(w, "# Parallel transfers: {p}")?;
    }
    writeln!(w, "# Target folder: {}", info.target.display())?;
    for s in info.sources {
        writeln!(w, "# Source: {}", s.display())?;
    }
    Ok(())
}

fn time_text(ms: i64) -> String {
    Local
        .timestamp_millis_opt(ms)
        .single()
        .map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Millis, false))
        .unwrap_or_else(|| ms.to_string())
}

fn field(s: &str) -> String {
    s.replace(['\t', '\r', '\n'], " ")
}

const COLUMNS: &[&str] = &[
    "index",
    "decision",
    "reason",
    "result",
    "relative_path",
    "source_size",
    "source_modified",
    "source_cloud_only",
    "target_exists",
    "target_size",
    "target_modified",
    "move_by_rename",
    "source_path",
    "target_path",
];

fn row(i: usize, it: &PlanItem, result: &ItemResult, op: Operation) -> String {
    let (decision, reason) = match &it.action {
        Action::Create => ("create", ""),
        Action::Overwrite => ("overwrite", ""),
        Action::Skip { reason } => ("skip", reason.as_str()),
    };
    let src_size = if it.icloud_stub { "unknown".to_string() } else { it.src.size.to_string() };
    let dst = |f: fn(&FileMeta) -> String| it.dst.as_ref().map(f).unwrap_or_default();
    [
        (i + 1).to_string(),
        decision.into(),
        field(reason),
        field(&result.text(op)),
        field(&it.rel),
        src_size,
        time_text(it.src.mtime_ms),
        yes_no(it.src.cloud_only).into(),
        yes_no(it.dst.is_some()).into(),
        dst(|d| d.size.to_string()),
        dst(|d| time_text(d.mtime_ms)),
        yes_no(it.rename_only).into(),
        field(&it.source.to_string_lossy()),
        field(&it.dest.to_string_lossy()),
    ]
    .join("\t")
}

/// Append the plan summary, one row per item, and the footer.
/// `results` is `None` for a Preview.
pub fn write_body(
    file: &File,
    plan: &Plan,
    op: Operation,
    results: Option<&[ItemResult]>,
    started: DateTime<Local>,
    outcome: &str,
) -> io::Result<()> {
    let mut w = BufWriter::new(file);
    let (mut create, mut overwrite, mut skip) = (0, 0, 0);
    for it in &plan.items {
        match it.action {
            Action::Create => create += 1,
            Action::Overwrite => overwrite += 1,
            Action::Skip { .. } => skip += 1,
        }
    }
    let l = &plan.lookup;
    writeln!(w, "# Plan: {} files: {create} new, {overwrite} overwrite, {skip} skip", plan.items.len())?;
    writeln!(
        w,
        "# Target check: {} folders listed, {} files looked up, {} folders new",
        l.dirs_listed, l.files_statted, l.dirs_missing
    )?;
    for x in &plan.warnings {
        writeln!(w, "# Warning ({:?}, {}): {}", x.level, x.code, x.message)?;
    }
    for e in &plan.errors {
        writeln!(w, "# Error: {}", field(e))?;
    }
    writeln!(w, "#")?;
    writeln!(w, "{}", COLUMNS.join("\t"))?;
    for (i, it) in plan.items.iter().enumerate() {
        let r = match results {
            None => ItemResult::NotRun,
            Some(rs) => rs.get(i).cloned().unwrap_or(ItemResult::NotAttempted),
        };
        writeln!(w, "{}", row(i, it, &r, op))?;
    }
    let ended = Local::now();
    writeln!(w, "#")?;
    writeln!(w, "# Finished: {}", ended.to_rfc3339_opts(chrono::SecondsFormat::Secs, false))?;
    writeln!(w, "# Duration: {:.1} s", (ended - started).num_milliseconds() as f64 / 1000.0)?;
    writeln!(w, "# Result: {}", field(outcome))?;
    if results.is_none() {
        writeln!(w, "# {PREVIEW_BANNER}")?;
    }
    w.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{OverwriteRule, Plan};

    fn item(rel: &str, action: Action) -> PlanItem {
        PlanItem {
            source: PathBuf::from("/src").join(rel),
            dest: PathBuf::from("/dst").join(rel),
            rel: rel.into(),
            src: FileMeta { size: 10, mtime_ms: 1_700_000_000_000, cloud_only: false },
            dst: None,
            action,
            rename_only: false,
            icloud_stub: false,
        }
    }

    fn plan() -> Plan {
        Plan {
            items: vec![
                item("a.txt", Action::Create),
                item("tab\there.txt", Action::Skip { reason: "target exists".into() }),
            ],
            ..Default::default()
        }
    }

    fn info<'a>(mode: Mode, sources: &'a [PathBuf], opts: &'a PlanOptions) -> JobInfo<'a> {
        JobInfo {
            mode,
            started: Local::now(),
            sources,
            target: Path::new("/dst"),
            options: opts,
            engine: (mode == Mode::Run).then_some("built-in"),
            parallel: (mode == Mode::Run).then_some(4),
            os: "test-os 1.0".into(),
        }
    }

    #[test]
    fn preview_log_is_marked_at_top_bottom_and_in_name() {
        let d = tempfile::tempdir().unwrap();
        let sources = vec![PathBuf::from("/src")];
        let opts = PlanOptions { rule: OverwriteRule::ReplaceOlder, ..Default::default() };
        let inf = info(Mode::Preview, &sources, &opts);
        let (path, file) = create(&d.path().join("logs"), &inf).unwrap();
        write_body(&file, &plan(), opts.operation, None, inf.started, "PREVIEW ONLY - 1 would be copied").unwrap();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.ends_with("_copy_preview.log"), "{name}");
        let text = fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert!(lines[1].contains("PREVIEW ONLY"));
        assert!(lines.last().unwrap().contains("PREVIEW ONLY"));
        let rows: Vec<&&str> = lines.iter().filter(|l| !l.starts_with('#')).collect();
        assert_eq!(rows.len(), 3, "header row + 2 items");
        assert!(rows.iter().all(|r| r.split('\t').count() == COLUMNS.len()), "tabs in names are escaped");
        assert!(rows[1].contains("not run (preview)"));
        assert!(text.contains("# If target exists: replace older"));
    }

    #[test]
    fn run_log_records_results_and_never_overwrites() {
        let d = tempfile::tempdir().unwrap();
        let sources = vec![PathBuf::from("/src")];
        let opts = PlanOptions { operation: Operation::Move, ..Default::default() };
        let inf = info(Mode::Run, &sources, &opts);
        let (p1, f1) = create(d.path(), &inf).unwrap();
        let (p2, _f2) = create(d.path(), &inf).unwrap();
        assert_ne!(p1, p2, "same second, distinct files");
        assert!(!p1.to_string_lossy().contains("preview"));
        let results = vec![ItemResult::Failed("disk full".into()), ItemResult::Skipped];
        write_body(&f1, &plan(), opts.operation, Some(&results), inf.started, "FAILED - 0 of 1 moved").unwrap();
        let text = fs::read_to_string(&p1).unwrap();
        assert!(text.contains("# Mode: RUN - files were moved."));
        assert!(text.contains("FAILED: disk full"));
        assert!(text.contains("# Engine: built-in"));
        assert!(!text.contains("PREVIEW"));
    }
}

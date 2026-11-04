//! How a job ended, in words for the end-of-job popup and the log footer.

use crate::model::{Action, Operation, Plan};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Preview finished (nothing was changed).
    Preview,
    /// Every planned file was transferred.
    Success,
    /// Some transferred, some failed.
    Partial,
    /// Nothing transferred: every attempt failed, or the job couldn't start.
    Failed,
    /// Stopped by the user before finishing.
    Cancelled,
}

#[derive(Debug, Clone, Serialize)]
pub struct Outcome {
    pub status: Status,
    pub title: String,
    pub message: String,
    pub planned: usize,
    pub done: usize,
    pub failed: usize,
    pub not_attempted: usize,
    pub skipped: usize,
    pub log_path: Option<String>,
    pub log_error: Option<String>,
}

fn files(n: usize) -> String {
    format!("{n} file{}", if n == 1 { "" } else { "s" })
}

fn verb(op: Operation) -> (&'static str, &'static str) {
    match op {
        Operation::Copy => ("copy", "copied"),
        Operation::Move => ("move", "moved"),
    }
}

fn counts(plan: &Plan) -> (usize, usize) {
    let skipped = plan.items.iter().filter(|i| matches!(i.action, Action::Skip { .. })).count();
    (plan.items.len() - skipped, skipped)
}

pub fn preview(plan: &Plan, op: Operation) -> Outcome {
    let (planned, skipped) = counts(plan);
    let (v, _) = verb(op);
    Outcome {
        status: Status::Preview,
        title: "Preview complete".into(),
        message: format!(
            "PREVIEW ONLY - no action was taken. {} would {v}; {} would be skipped.",
            files(planned),
            files(skipped)
        ),
        planned,
        done: 0,
        failed: 0,
        not_attempted: 0,
        skipped,
        log_path: None,
        log_error: None,
    }
}

/// `start_error`: the engine couldn't start, so nothing was attempted.
pub fn run(
    plan: &Plan,
    op: Operation,
    done: usize,
    failed: usize,
    cancelled: bool,
    start_error: Option<&str>,
) -> Outcome {
    let (planned, skipped) = counts(plan);
    let not_attempted = planned.saturating_sub(done + failed);
    let (v, past) = verb(op);
    let skipped_note = if skipped > 0 { format!(" {} skipped as planned.", files(skipped)) } else { String::new() };

    let (status, title, message) = if let Some(e) = start_error {
        (Status::Failed, "Job failed to start".to_string(), format!("Nothing was {past}: {e}"))
    } else if cancelled {
        (
            Status::Cancelled,
            "Job cancelled".into(),
            format!(
                "Stopped by you after {} of {} were {past}{}. {} not attempted.{skipped_note}",
                done,
                files(planned),
                if failed > 0 { format!(" ({failed} failed)") } else { String::new() },
                files(not_attempted),
            ),
        )
    } else if failed > 0 && done == 0 {
        (
            Status::Failed,
            "Job failed".into(),
            format!("None of the {} could be {past}; all {failed} failed. See the errors for why.{skipped_note}", files(planned)),
        )
    } else if failed > 0 {
        (
            Status::Partial,
            "Finished with errors".into(),
            format!("{} of {} {past}; {failed} failed. See the errors for why.{skipped_note}", done, files(planned)),
        )
    } else if planned == 0 {
        (Status::Success, "Nothing to do".into(), format!("No files needed to {v}.{skipped_note}"))
    } else {
        (Status::Success, "Success".into(), format!("{} of {} {past}.{skipped_note}", done, files(planned)))
    };
    Outcome {
        status,
        title,
        message,
        planned,
        done,
        failed,
        not_attempted,
        skipped,
        log_path: None,
        log_error: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{FileMeta, PlanItem};
    use std::path::PathBuf;

    fn plan(n_go: usize, n_skip: usize) -> Plan {
        let item = |action| PlanItem {
            source: PathBuf::new(),
            dest: PathBuf::new(),
            rel: String::new(),
            src: FileMeta { size: 0, mtime_ms: 0, cloud_only: false },
            dst: None,
            action,
            rename_only: false,
            icloud_stub: false,
        };
        let mut items: Vec<_> = (0..n_go).map(|_| item(Action::Create)).collect();
        items.extend((0..n_skip).map(|_| item(Action::Skip { reason: String::new() })));
        Plan { items, ..Default::default() }
    }

    #[test]
    fn statuses() {
        let p = plan(4, 2);
        let o = run(&p, Operation::Copy, 4, 0, false, None);
        assert_eq!(o.status, Status::Success);
        assert_eq!(o.message, "4 of 4 files copied. 2 files skipped as planned.");
        assert_eq!(run(&p, Operation::Move, 3, 1, false, None).status, Status::Partial);
        assert_eq!(run(&p, Operation::Copy, 0, 4, false, None).status, Status::Failed);
        let c = run(&p, Operation::Copy, 1, 0, true, None);
        assert_eq!((c.status, c.not_attempted), (Status::Cancelled, 3));
        let f = run(&p, Operation::Copy, 0, 0, false, Some("rclone not found"));
        assert_eq!(f.status, Status::Failed);
        assert!(f.message.contains("rclone not found"));
        assert_eq!(run(&plan(0, 3), Operation::Copy, 0, 0, false, None).title, "Nothing to do");
    }

    #[test]
    fn preview_says_no_action() {
        let o = preview(&plan(1, 1), Operation::Move);
        assert!(o.message.starts_with("PREVIEW ONLY - no action was taken."));
        assert!(o.message.contains("1 file would move"));
    }
}

//! Read-only diagnostic: build a plan (preview only; nothing is copied) and
//! print the path and plan guardrails.
//!
//!     cargo run --example plan -- copy|move <target> <source>...

use polimath_file_copy_lib::model::{Action, Operation, PlanOptions};
use polimath_file_copy_lib::{guard, planner};
use std::path::PathBuf;

fn main() {
    let mut args = std::env::args().skip(1);
    let operation = match args.next().as_deref() {
        Some("move") => Operation::Move,
        _ => Operation::Copy,
    };
    let target = PathBuf::from(args.next().expect("target"));
    let sources: Vec<PathBuf> = args.map(PathBuf::from).collect();

    println!("== path warnings ==");
    for w in guard::check_paths(&sources, Some(&target), operation, false) {
        println!("[{:?}] {}: {}", w.level, w.code, w.message);
    }
    let opts = PlanOptions { operation, ..Default::default() };
    let plan = planner::build_plan(&sources, &target, &opts);
    let skip = plan.items.iter().filter(|i| matches!(i.action, Action::Skip { .. })).count();
    let renames = plan.items.iter().filter(|i| i.rename_only).count();
    println!("== plan: {} items, {} skipped, {} rename-only, {} errors ==", plan.items.len(), skip, renames, plan.errors.len());
    for w in &plan.warnings {
        println!("[{:?}] {}: {}", w.level, w.code, w.message);
    }
}

//! Turn sources + target + options into a reviewable plan. Reads metadata
//! only; never reads file content.

use crate::decide::decide;
use crate::meta::Kind;
use crate::model::{Action, Plan, PlanItem, PlanOptions};
use crate::{scan, target};
use std::path::{Path, PathBuf};

pub fn build_plan(sources: &[PathBuf], target_dir: &Path, opts: &PlanOptions) -> Plan {
    let mut plan = Plan::default();

    for s in sources {
        if target_dir.starts_with(s) {
            plan.errors.push(format!(
                "Target {} is inside source {}; remove one of them.",
                target_dir.display(),
                s.display()
            ));
            return plan;
        }
    }

    if let Ok(st) = crate::meta::stat(target_dir) {
        if st.kind != Kind::Dir {
            plan.errors.push(format!("Target {} is not a folder.", target_dir.display()));
            return plan;
        }
    }

    let scanned = scan::expand_sources(sources);
    plan.errors.extend(scanned.errors);

    let dests: Vec<PathBuf> = scanned
        .files
        .iter()
        .map(|f| target_dir.join(&f.rel_path))
        .collect();
    let (existing, stats, errors) = target::lookup(&dests, opts.list_threshold);
    plan.lookup = stats;
    plan.errors.extend(errors);

    for (f, dest) in scanned.files.into_iter().zip(dests) {
        let found = existing.get(&dest);
        let action = if f.abs_path == dest {
            Action::Skip { reason: "source and target are the same file".into() }
        } else {
            match found.map(|s| s.kind) {
                Some(Kind::Dir) => Action::Skip { reason: "a folder with this name exists at the target".into() },
                Some(Kind::Other) => Action::Skip { reason: "a link or special file with this name exists at the target".into() },
                _ => decide(&f.meta, found.map(|s| &s.meta), opts),
            }
        };
        plan.items.push(PlanItem {
            rel: f.rel_path.to_string_lossy().replace('\\', "/"),
            source: f.abs_path,
            dst: found.filter(|s| s.kind == Kind::File).map(|s| s.meta),
            dest,
            src: f.meta,
            action,
        });
    }
    plan
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::OverwriteRule;
    use filetime::{FileTime, set_file_mtime};
    use std::fs;

    #[test]
    fn end_to_end_plan() {
        let root = tempfile::tempdir().unwrap();
        let src = root.path().join("src");
        let dst = root.path().join("dst");
        fs::create_dir_all(src.join("Photos/2024")).unwrap();
        fs::create_dir_all(dst.join("Photos/2024")).unwrap();
        fs::write(src.join("Photos/2024/new.jpg"), b"1").unwrap();
        fs::write(src.join("Photos/2024/bigger.jpg"), b"12345").unwrap();
        fs::write(dst.join("Photos/2024/bigger.jpg"), b"12").unwrap();
        fs::write(src.join("Photos/2024/smaller.jpg"), b"1").unwrap();
        fs::write(dst.join("Photos/2024/smaller.jpg"), b"123").unwrap();
        fs::write(src.join("loose.txt"), b"x").unwrap();
        fs::write(dst.join("loose.txt"), b"x").unwrap();
        set_file_mtime(src.join("loose.txt"), FileTime::from_unix_time(2_000_000, 0)).unwrap();
        set_file_mtime(dst.join("loose.txt"), FileTime::from_unix_time(1_000_000, 0)).unwrap();

        let sources = vec![src.join("Photos"), src.join("loose.txt")];

        let opts = PlanOptions { rule: OverwriteRule::ReplaceSmaller, ..Default::default() };
        let plan = build_plan(&sources, &dst, &opts);
        assert!(plan.errors.is_empty(), "{:?}", plan.errors);
        let action = |rel: &str| plan.items.iter().find(|i| i.rel == rel).unwrap().action.clone();
        assert_eq!(action("Photos/2024/new.jpg"), Action::Create);
        assert_eq!(action("Photos/2024/bigger.jpg"), Action::Overwrite);
        assert!(matches!(action("Photos/2024/smaller.jpg"), Action::Skip { .. }));
        assert!(matches!(action("loose.txt"), Action::Skip { .. }));

        let opts = PlanOptions { rule: OverwriteRule::ReplaceOlder, ..Default::default() };
        let plan = build_plan(&sources, &dst, &opts);
        let loose = plan.items.iter().find(|i| i.rel == "loose.txt").unwrap();
        assert_eq!(loose.action, Action::Overwrite);
    }

    #[test]
    fn target_inside_source_is_rejected() {
        let root = tempfile::tempdir().unwrap();
        let plan = build_plan(&[root.path().to_path_buf()], &root.path().join("x"), &PlanOptions::default());
        assert!(plan.items.is_empty());
        assert_eq!(plan.errors.len(), 1);
    }
}

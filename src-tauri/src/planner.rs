//! Turn sources + target + options into a reviewable plan. Reads metadata
//! only; never reads file content.

use crate::decide::decide;
use crate::guard;
use crate::meta::Kind;
use crate::model::{Action, Operation, Plan, PlanItem, PlanOptions};
use crate::platform::{self, RootFn, SyncRoot};
use crate::{scan, target};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

pub fn build_plan(sources: &[PathBuf], target_dir: &Path, opts: &PlanOptions) -> Plan {
    build_plan_with(sources, target_dir, opts, &platform::sync_root)
}

/// `roots` says which cloud sync root a folder is in (injectable for tests).
pub fn build_plan_with(sources: &[PathBuf], target_dir: &Path, opts: &PlanOptions, roots: RootFn) -> Plan {
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

    // A cloud-only file can be moved without downloading it only by renaming
    // it within its own sync root. Moving it anywhere else (another root, or
    // out of the cloud folder) makes the sync client download it first.
    let is_move = opts.operation == Operation::Move;
    let target_root = if is_move { roots(target_dir) } else { None };
    let mut root_cache: HashMap<PathBuf, Option<SyncRoot>> = HashMap::new();
    let mut seen: HashSet<String> = HashSet::new();

    for (f, dest) in scanned.files.into_iter().zip(dests) {
        let found = existing.get(&dest);
        let cloud_ok = is_move
            && f.meta.cloud_only
            && !f.icloud_stub
            && same_root(&f.abs_path, target_root.as_ref(), roots, &mut root_cache);

        let action = if f.abs_path == dest {
            Action::Skip { reason: "source and target are the same file".into() }
        } else if !seen.insert(dest_key(&dest)) {
            Action::Skip { reason: "another source file has the same target path".into() }
        } else if f.icloud_stub {
            Action::Skip {
                reason: "iCloud file not downloaded (older macOS); choose \"Download Now\" in Finder first".into(),
            }
        } else {
            match found.map(|s| s.kind) {
                Some(Kind::Dir) => Action::Skip { reason: "a folder with this name exists at the target".into() },
                Some(Kind::Other) => {
                    Action::Skip { reason: "a link or special file with this name exists at the target".into() }
                }
                Some(Kind::ICloudStub) => Action::Skip {
                    reason: "the target has this file in iCloud but not downloaded; download or delete it there first"
                        .into(),
                },
                _ => decide(&f.meta, found.map(|s| &s.meta), opts, cloud_ok),
            }
        };
        let rename_only = cloud_ok && !matches!(action, Action::Skip { .. });
        plan.items.push(PlanItem {
            rel: f.rel_path.to_string_lossy().replace('\\', "/"),
            source: f.abs_path,
            dst: found.filter(|s| s.kind == Kind::File).map(|s| s.meta),
            dest,
            src: f.meta,
            action,
            rename_only,
            icloud_stub: f.icloud_stub,
        });
    }

    plan.warnings = guard::plan_warnings(&plan.items, opts, target_dir, target_root.as_ref());
    plan
}

/// Is `src_file` in the same sync root as the target?
fn same_root(
    src_file: &Path,
    target_root: Option<&SyncRoot>,
    roots: RootFn,
    cache: &mut HashMap<PathBuf, Option<SyncRoot>>,
) -> bool {
    let Some(tr) = target_root else { return false };
    let parent = src_file.parent().unwrap_or(Path::new("")).to_path_buf();
    let sr = cache.entry(parent.clone()).or_insert_with(|| roots(&parent));
    sr.as_ref().is_some_and(|sr| sr.id == tr.id)
}

/// Two destinations collide if the target file system would treat them as
/// the same name (case-insensitive on Windows and macOS).
fn dest_key(p: &Path) -> String {
    let s = p.to_string_lossy();
    if cfg!(any(windows, target_os = "macos")) { s.to_lowercase() } else { s.into_owned() }
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

    #[test]
    fn same_target_path_from_two_sources_is_skipped_once() {
        let root = tempfile::tempdir().unwrap();
        for d in ["a", "b"] {
            fs::create_dir_all(root.path().join(d)).unwrap();
            fs::write(root.path().join(d).join("Same.txt"), b"1").unwrap();
        }
        let sources = vec![root.path().join("a/Same.txt"), root.path().join("b/Same.txt")];
        let plan = build_plan(&sources, &root.path().join("out"), &PlanOptions::default());
        assert_eq!(plan.items[0].action, Action::Create);
        assert!(matches!(plan.items[1].action, Action::Skip { .. }));
        assert!(plan.warnings.iter().any(|w| w.code == "collisions"));
    }

    #[test]
    fn icloud_stand_ins_are_skipped_on_both_sides() {
        let root = tempfile::tempdir().unwrap();
        let src = root.path().join("src");
        let dst = root.path().join("dst");
        fs::create_dir_all(&src).unwrap();
        fs::create_dir_all(&dst).unwrap();
        fs::write(src.join(".Cloud.pdf.icloud"), b"plist").unwrap();
        fs::write(src.join("Local.pdf"), b"pdf").unwrap();
        fs::write(dst.join(".Local.pdf.icloud"), b"plist").unwrap();
        let opts = PlanOptions { rule: OverwriteRule::Always, list_threshold: 1, ..Default::default() };
        let plan = build_plan(&[src.join(".Cloud.pdf.icloud"), src.join("Local.pdf")], &dst, &opts);
        let cloud = plan.items.iter().find(|i| i.rel == "Cloud.pdf").unwrap();
        assert!(cloud.icloud_stub && matches!(cloud.action, Action::Skip { .. }));
        let local = plan.items.iter().find(|i| i.rel == "Local.pdf").unwrap();
        assert!(matches!(local.action, Action::Skip { .. }), "target stand-in must block overwrite");
    }

    #[test]
    fn same_root_compares_root_ids() {
        let fake = |p: &Path| -> Option<SyncRoot> {
            let id = if p.starts_with("/dropbox") { "D" } else if p.starts_with("/onedrive") { "O" } else { return None };
            Some(SyncRoot { id: id.into(), provider: id.into() })
        };
        let d = fake(Path::new("/dropbox/x"));
        let mut cache = HashMap::new();
        assert!(same_root(Path::new("/dropbox/a/f.txt"), d.as_ref(), &fake, &mut cache));
        assert!(!same_root(Path::new("/onedrive/a/f.txt"), d.as_ref(), &fake, &mut cache));
        assert!(!same_root(Path::new("/local/f.txt"), d.as_ref(), &fake, &mut cache));
        assert!(!same_root(Path::new("/dropbox/a/f.txt"), None, &fake, &mut cache));
    }
}

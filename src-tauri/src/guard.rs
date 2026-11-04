//! Guardrails: warnings shown before Preview (from the chosen paths and the
//! OS) and before Run (from the plan). `Danger` warnings must be acknowledged
//! in the UI before Run is enabled.

use crate::model::{Action, Level, Operation, PlanItem, PlanOptions, Warning};
use crate::platform::{self, Environment, RootFn, SyncRoot};
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

/// Package folders that macOS apps manage as a single database. Their
/// contents may be partly in iCloud without any placeholder marks, and
/// writing into them can corrupt them.
const LIBRARY_EXTS: &[&str] = &[
    "photoslibrary",
    "photolibrary",
    "migratedphotolibrary",
    "aplibrary",
    "musiclibrary",
    "tvlibrary",
];

/// Folder names of common sync clients, used only where the OS can't detect
/// placeholders for us.
const SYNC_NAMES: &[&str] = &["dropbox", "onedrive", "synologydrive", "icloud", "pcloud", "nextcloud", "box"];

fn library_component(p: &Path) -> Option<String> {
    p.components().find_map(|c| match c {
        Component::Normal(s) => {
            let s = s.to_string_lossy();
            let ext = s.rsplit_once('.')?.1.to_ascii_lowercase();
            LIBRARY_EXTS.contains(&ext.as_str()).then(|| s.into_owned())
        }
        _ => None,
    })
}

fn looks_like_sync_folder(p: &Path) -> bool {
    p.components().any(|c| match c {
        Component::Normal(s) => {
            let s = s.to_string_lossy().to_ascii_lowercase();
            SYNC_NAMES.iter().any(|n| s == *n || s.starts_with(&format!("{n} ")) || s.starts_with(&format!("{n}-")))
        }
        _ => false,
    })
}

pub fn fmt_bytes(n: u64) -> String {
    const U: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i < U.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 { format!("{n} B") } else { format!("{v:.1} {}", U[i]) }
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// Warnings from the chosen paths alone (cheap; no scanning). Shown before
/// Preview and refreshed whenever sources, target, operation or engine change.
pub fn check_paths(sources: &[PathBuf], target: Option<&Path>, op: Operation, rclone: bool) -> Vec<Warning> {
    check_paths_with(sources, target, op, rclone, platform::environment(), &platform::sync_root)
}

pub fn check_paths_with(
    sources: &[PathBuf],
    target: Option<&Path>,
    op: Operation,
    rclone: bool,
    env: &Environment,
    roots: RootFn,
) -> Vec<Warning> {
    let mut w = Vec::new();

    // Photos / Music libraries.
    for s in sources {
        if let Some(lib) = library_component(s) {
            w.push(Warning::new(
                Level::Danger,
                "library_source",
                format!(
                    "\"{lib}\" is an app library. With iCloud Photos or \"Optimize Mac Storage\", \
                     many originals exist only in iCloud and are not marked as cloud-only, so a \
                     copy can be silently incomplete. Export from the app instead, or turn on \
                     \"Download Originals to this Mac\" and wait for it to finish."
                ),
            ));
        }
    }
    if let Some(lib) = target.and_then(library_component) {
        w.push(Warning::new(
            Level::Danger,
            "library_target",
            format!("The target is inside \"{lib}\". Writing into an app library can corrupt it."),
        ));
    }

    // Look up the folder, never the file itself (see platform::sync_root).
    let is_dir = |p: &Path| crate::meta::stat(p).is_ok_and(|s| s.kind == crate::meta::Kind::Dir);
    let root_of = |p: &Path| roots(if is_dir(p) { p } else { p.parent().unwrap_or(p) });
    let source_roots: Vec<(PathBuf, Option<SyncRoot>)> = sources.iter().map(|s| (s.clone(), root_of(s))).collect();
    let target_root = target.and_then(|t| roots(t));

    // Older macOS iCloud Drive.
    if env.icloud_stubs {
        let in_icloud = source_roots.iter().any(|(_, r)| r.as_ref().is_some_and(|r| r.provider == "iCloud Drive"))
            || target_root.as_ref().is_some_and(|r| r.provider == "iCloud Drive");
        if in_icloud {
            w.push(Warning::new(
                Level::Warn,
                "old_icloud_paths",
                format!(
                    "These paths are in iCloud Drive on macOS {}. Files that aren't downloaded are \
                     hidden \".name.icloud\" stand-ins: they will be skipped, and their sizes and \
                     dates can't be compared. Use \"Download Now\" in Finder first for a complete copy.",
                    env.version
                ),
            ));
        }
    }

    // Moving out of a cloud folder deletes from the cloud everywhere.
    if op == Operation::Move {
        let mut providers: Vec<&str> = source_roots
            .iter()
            .filter_map(|(_, r)| r.as_ref())
            .filter(|r| target_root.as_ref().is_none_or(|t| t.id != r.id))
            .map(|r| r.provider.as_str())
            .collect();
        providers.sort();
        providers.dedup();
        for p in providers {
            w.push(Warning::new(
                Level::Warn,
                "move_out_of_cloud",
                format!(
                    "Moving files out of {p} deletes them from {p} on every device, not just this \
                     one. Cloud-only files would also have to be downloaded first."
                ),
            ));
        }
    }

    // Linux (or anything without placeholder detection) + sync-looking paths.
    if !env.cloud_detection {
        let all = sources.iter().map(PathBuf::as_path).chain(target);
        if all.into_iter().any(looks_like_sync_folder) {
            w.push(Warning::new(
                Level::Warn,
                "no_cloud_detection_path",
                "This looks like a cloud sync folder, but on this OS the app can't tell which files \
                 are cloud-only. Copying may download files and take much longer."
                    .into(),
            ));
        }
    }

    if rclone && env.os == "macos" {
        w.push(Warning::new(
            Level::Info,
            "rclone_mac_metadata",
            "The rclone engine copies contents and modified time only; Finder tags, comments and \
             other extended attributes are not kept. Use the built-in engine to keep them."
                .into(),
        ));
    }

    w
}

fn skip_reason(i: &PlanItem) -> Option<&str> {
    match &i.action {
        Action::Skip { reason } => Some(reason.as_str()),
        _ => None,
    }
}

/// Warnings from a computed plan. Shown before Run.
pub fn plan_warnings(
    items: &[PlanItem],
    opts: &PlanOptions,
    target_dir: &Path,
    target_root: Option<&SyncRoot>,
) -> Vec<Warning> {
    let mut w = Vec::new();

    let (mut downloads, mut download_bytes) = (0usize, 0u64);
    let (mut cloud_skipped, mut stubs, mut target_stubs, mut collisions, mut renamed, mut skipped) =
        (0usize, 0usize, 0usize, 0usize, 0usize, 0usize);
    let mut libraries: Vec<String> = Vec::new();

    for i in items {
        if let Some(lib) = library_component(Path::new(&i.rel)) {
            if !libraries.contains(&lib) {
                libraries.push(lib);
            }
        }
        match skip_reason(i) {
            None => {
                if i.rename_only {
                    renamed += 1;
                } else if i.src.cloud_only {
                    downloads += 1;
                    download_bytes += i.src.size;
                }
            }
            Some(r) => {
                skipped += 1;
                if i.icloud_stub {
                    stubs += 1;
                } else if r.starts_with("source is cloud-only") {
                    cloud_skipped += 1;
                } else if r.starts_with("the target has this file in iCloud") {
                    target_stubs += 1;
                } else if r.starts_with("another source file") {
                    collisions += 1;
                }
            }
        }
    }

    for lib in libraries {
        w.push(Warning::new(
            Level::Danger,
            "library_source",
            format!(
                "The sources include \"{lib}\", an app library. Some originals may exist only in \
                 iCloud without being marked, so the copy can be silently incomplete."
            ),
        ));
    }
    if downloads > 0 {
        w.push(Warning::new(
            Level::Warn,
            "cloud_download",
            format!(
                "{} ({}) will be downloaded from the cloud so they can be {}. Turn on \"Skip \
                 cloud-only sources\" to leave them.",
                plural(downloads, "cloud-only file", "cloud-only files"),
                fmt_bytes(download_bytes),
                if opts.operation == Operation::Move { "moved" } else { "copied" },
            ),
        ));
    }
    if cloud_skipped > 0 {
        w.push(Warning::new(
            Level::Info,
            "cloud_skipped",
            format!(
                "{} will be skipped because copying would download {}. Turn off \"Skip cloud-only \
                 sources\" to include them.",
                plural(cloud_skipped, "cloud-only file", "cloud-only files"),
                if cloud_skipped == 1 { "it" } else { "them" },
            ),
        ));
    }
    if stubs > 0 {
        w.push(Warning::new(
            Level::Warn,
            "icloud_stubs",
            format!(
                "{} in iCloud {} not downloaded (shown as \".name.icloud\" on this macOS) and will be \
                 skipped. Choose \"Download Now\" in Finder, then preview again.",
                plural(stubs, "file", "files"),
                if stubs == 1 { "is" } else { "are" },
            ),
        ));
    }
    if target_stubs > 0 {
        w.push(Warning::new(
            Level::Warn,
            "icloud_target_stubs",
            format!(
                "{} already in iCloud at the target but not downloaded, so {} can't be compared or \
                 safely replaced and will be skipped.",
                plural(target_stubs, "file is", "files are"),
                if target_stubs == 1 { "it" } else { "they" },
            ),
        ));
    }
    if collisions > 0 {
        w.push(Warning::new(
            Level::Warn,
            "collisions",
            format!(
                "{} the same target path as another source file and will be skipped (the first one \
                 in the list wins).",
                plural(collisions, "file has", "files have"),
            ),
        ));
    }
    if renamed > 0 {
        let provider = target_root.map(|r| r.provider.as_str()).unwrap_or("the cloud folder");
        w.push(Warning::new(
            Level::Info,
            "cloud_rename",
            format!(
                "{} will be moved within {provider} by renaming, without downloading.",
                plural(renamed, "cloud-only file", "cloud-only files"),
            ),
        ));
    }
    if opts.operation == Operation::Move && skipped > 0 {
        w.push(Warning::new(
            Level::Info,
            "move_leaves_skipped",
            format!("{} will stay where {} are.", plural(skipped, "skipped file", "skipped files"), if skipped == 1 { "it" } else { "they" }),
        ));
    }

    // Free space at the target.
    let target_vol = platform::volume_id(target_dir);
    let mut vol_cache: HashMap<PathBuf, Option<String>> = HashMap::new();
    let mut needed: i128 = 0;
    for i in items {
        if skip_reason(i).is_some() || i.rename_only {
            continue;
        }
        if opts.operation == Operation::Move {
            // Same-volume moves are renames and need no space.
            let parent = i.source.parent().unwrap_or(Path::new("")).to_path_buf();
            let v = vol_cache.entry(parent.clone()).or_insert_with(|| platform::volume_id(&parent));
            if v.is_some() && *v == target_vol {
                continue;
            }
        }
        needed += i.src.size as i128 - i.dst.map(|d| d.size as i128).unwrap_or(0);
    }
    if needed > 0 {
        if let Some(free) = platform::free_space(target_dir) {
            let needed = needed as u64;
            if needed > free {
                w.push(Warning::new(
                    Level::Danger,
                    "no_space",
                    format!(
                        "Not enough space at the target: about {} is needed and {} is free.",
                        fmt_bytes(needed),
                        fmt_bytes(free)
                    ),
                ));
            } else if needed > free / 10 * 9 {
                w.push(Warning::new(
                    Level::Warn,
                    "low_space",
                    format!(
                        "This will use most of the free space at the target ({} of {}).",
                        fmt_bytes(needed),
                        fmt_bytes(free)
                    ),
                ));
            }
        }
    }

    w
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::FileMeta;

    fn env(os: &'static str, major: u32) -> Environment {
        Environment {
            os,
            version: format!("{major}.0"),
            major,
            icloud_stubs: os == "macos" && major < 14,
            cloud_detection: os != "linux",
            notes: vec![],
        }
    }

    fn fake_roots(p: &Path) -> Option<SyncRoot> {
        let s = p.to_string_lossy().replace('\\', "/");
        if s.contains("/Library/Mobile Documents") {
            Some(SyncRoot { id: "icloud".into(), provider: "iCloud Drive".into() })
        } else if s.contains("/Dropbox") {
            Some(SyncRoot { id: "dropbox".into(), provider: "Dropbox".into() })
        } else {
            None
        }
    }

    fn codes(w: &[Warning]) -> Vec<&'static str> {
        w.iter().map(|w| w.code).collect()
    }

    #[test]
    fn photos_library_is_danger() {
        let w = check_paths_with(
            &[PathBuf::from("/Users/me/Pictures/Photos Library.photoslibrary")],
            Some(Path::new("/Volumes/Backup")),
            Operation::Copy,
            false,
            &env("macos", 15),
            &fake_roots,
        );
        assert_eq!(codes(&w), vec!["library_source"]);
        assert_eq!(w[0].level, Level::Danger);
    }

    #[test]
    fn old_macos_icloud_warns_new_does_not() {
        let src = [PathBuf::from("/Users/me/Library/Mobile Documents/com~apple~CloudDocs/Taxes")];
        let t = Some(Path::new("/Volumes/Backup"));
        let old = check_paths_with(&src, t, Operation::Copy, false, &env("macos", 13), &fake_roots);
        assert!(codes(&old).contains(&"old_icloud_paths"));
        let new = check_paths_with(&src, t, Operation::Copy, false, &env("macos", 14), &fake_roots);
        assert!(new.is_empty());
    }

    #[test]
    fn move_out_of_cloud_warns_but_within_does_not() {
        let src = [PathBuf::from("/Users/me/Dropbox/Docs")];
        let out = check_paths_with(&src, Some(Path::new("/Users/me/Desktop")), Operation::Move, false, &env("windows", 10), &fake_roots);
        assert_eq!(codes(&out), vec!["move_out_of_cloud"]);
        let within = check_paths_with(&src, Some(Path::new("/Users/me/Dropbox/Archive")), Operation::Move, false, &env("windows", 10), &fake_roots);
        assert!(within.is_empty());
        let copy = check_paths_with(&src, Some(Path::new("/Users/me/Desktop")), Operation::Copy, false, &env("windows", 10), &fake_roots);
        assert!(copy.is_empty());
    }

    #[test]
    fn linux_sync_folder_and_rclone_on_mac() {
        let w = check_paths_with(&[PathBuf::from("/home/me/Dropbox/x")], None, Operation::Copy, false, &env("linux", 0), &fake_roots);
        assert!(codes(&w).contains(&"no_cloud_detection_path"));
        let w = check_paths_with(&[PathBuf::from("/Users/me/x")], None, Operation::Copy, true, &env("macos", 15), &fake_roots);
        assert_eq!(codes(&w), vec!["rclone_mac_metadata"]);
    }

    fn item(rel: &str, size: u64, cloud: bool, action: Action) -> PlanItem {
        PlanItem {
            source: PathBuf::from("/src").join(rel),
            dest: PathBuf::from("/dst").join(rel),
            rel: rel.into(),
            src: FileMeta { size, mtime_ms: 0, cloud_only: cloud },
            dst: None,
            action,
            rename_only: false,
            icloud_stub: false,
        }
    }

    #[test]
    fn plan_warnings_count_downloads_and_skips() {
        let d = tempfile::tempdir().unwrap();
        let items = vec![
            item("a", 10, true, Action::Create),
            item("b", 10, true, Action::Skip { reason: "source is cloud-only; copying would download it".into() }),
            item("Lib.photoslibrary/x", 1, false, Action::Create),
        ];
        let opts = PlanOptions { skip_cloud_only: false, ..Default::default() };
        let w = plan_warnings(&items, &opts, d.path(), None);
        let c = codes(&w);
        assert!(c.contains(&"cloud_download"), "{c:?}");
        assert!(c.contains(&"cloud_skipped"));
        assert!(c.contains(&"library_source"));
        assert!(!c.contains(&"no_space"));
    }

    #[test]
    fn plan_warns_when_out_of_space() {
        let d = tempfile::tempdir().unwrap();
        let items = vec![item("huge", u64::MAX / 2, false, Action::Create)];
        let w = plan_warnings(&items, &PlanOptions::default(), d.path(), None);
        assert_eq!(codes(&w), vec!["no_space"]);
    }
}

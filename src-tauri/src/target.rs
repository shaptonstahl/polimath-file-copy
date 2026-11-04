//! Find out what already exists at the target, using as few round trips as
//! possible. Over a VPN or to a NAS each metadata call can cost tens of
//! milliseconds, so per target folder we pick the cheaper strategy:
//!
//! * **Few files** going into a folder: look each one up by name.
//! * **Many files** (>= `list_threshold`): list the folder once. On Windows
//!   the listing already carries size and time for every entry; on Unix it
//!   gives names, and we `lstat` only names that are both present and needed.
//! * A missing folder is detected once, and everything inside it is new.

use crate::meta::{self, Kind, Stat};
use crate::model::LookupStats;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub type Lookup = HashMap<PathBuf, Stat>;

pub fn lookup(dests: &[PathBuf], list_threshold: usize) -> (Lookup, LookupStats, Vec<String>) {
    let mut by_dir: BTreeMap<&Path, Vec<&PathBuf>> = BTreeMap::new();
    for d in dests {
        if let Some(parent) = d.parent() {
            by_dir.entry(parent).or_default().push(d);
        }
    }

    let mut found = Lookup::new();
    let mut stats = LookupStats::default();
    let mut errors = Vec::new();
    // Folders known not to exist; their subfolders don't either.
    let mut missing: HashSet<PathBuf> = HashSet::new();

    for (dir, files) in by_dir {
        if dir.ancestors().skip(1).any(|a| missing.contains(a)) {
            missing.insert(dir.to_path_buf());
            stats.dirs_missing += 1;
            continue;
        }

        if files.len() >= list_threshold.max(1) {
            match list_dir(dir, &files, &mut found) {
                Ok(()) => stats.dirs_listed += 1,
                Err(e) if e.kind() == io::ErrorKind::NotFound => {
                    missing.insert(dir.to_path_buf());
                    stats.dirs_missing += 1;
                }
                Err(e) => errors.push(format!("{}: {e}", dir.display())),
            }
            continue;
        }

        // Per-file lookups. With more than one file, check the folder first
        // so a missing folder costs one call instead of one per file.
        if files.len() > 1 {
            match meta::stat(dir) {
                Ok(s) if s.kind == Kind::Dir => {}
                Ok(_) => {
                    errors.push(format!("{}: exists but is not a folder", dir.display()));
                    continue;
                }
                Err(e) if e.kind() == io::ErrorKind::NotFound => {
                    missing.insert(dir.to_path_buf());
                    stats.dirs_missing += 1;
                    continue;
                }
                Err(e) => {
                    errors.push(format!("{}: {e}", dir.display()));
                    continue;
                }
            }
        }
        for f in files {
            stats.files_statted += 1;
            match meta::stat(f) {
                Ok(s) => {
                    found.insert(f.clone(), s);
                }
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => errors.push(format!("{}: {e}", f.display())),
            }
        }
    }
    (found, stats, errors)
}

fn list_dir(dir: &Path, wanted: &[&PathBuf], found: &mut Lookup) -> io::Result<()> {
    let mut want: HashMap<String, &PathBuf> = HashMap::new();
    for w in wanted {
        if let Some(name) = w.file_name() {
            want.insert(meta::name_key(name), w);
        }
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if let Some(dest) = want.get(&meta::name_key(&entry.file_name())) {
            found.insert((*dest).clone(), meta::entry_stat(&entry)?);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (tempfile::TempDir, Vec<PathBuf>) {
        let dir = tempfile::tempdir().unwrap();
        let t = dir.path();
        fs::create_dir(t.join("sub")).unwrap();
        for i in 0..5 {
            fs::write(t.join(format!("f{i}.txt")), vec![0u8; i]).unwrap();
        }
        fs::write(t.join("sub").join("s.txt"), b"x").unwrap();
        let mut dests: Vec<PathBuf> = (0..10).map(|i| t.join(format!("f{i}.txt"))).collect();
        dests.push(t.join("sub").join("s.txt"));
        dests.push(t.join("nope").join("a.txt"));
        dests.push(t.join("nope").join("b.txt"));
        dests.push(t.join("nope").join("deeper").join("c.txt"));
        (dir, dests)
    }

    #[test]
    fn listing_and_per_file_agree() {
        let (_d, dests) = setup();
        let (listed, ls, e1) = lookup(&dests, 1);
        let (statted, ss, e2) = lookup(&dests, usize::MAX);
        assert!(e1.is_empty() && e2.is_empty(), "{e1:?} {e2:?}");
        assert_eq!(listed.len(), 6);
        assert_eq!(statted.len(), 6);
        for (k, v) in &listed {
            let w = statted.get(k).expect("same keys");
            assert_eq!(v.meta.size, w.meta.size);
            assert!((v.meta.mtime_ms - w.meta.mtime_ms).abs() <= 1);
        }
        assert!(ls.dirs_listed >= 2 && ls.files_statted == 0);
        assert!(ss.dirs_listed == 0 && ss.files_statted == 11);
        // "nope" and "nope/deeper" are both known missing.
        assert_eq!(ls.dirs_missing, 2);
        assert_eq!(ss.dirs_missing, 2);
    }
}

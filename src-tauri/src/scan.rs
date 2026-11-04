//! Expand the user's source list (any mix of files and folders) into files.

use crate::meta::{self, Kind};
use crate::model::SourceFile;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Default)]
pub struct ScanResult {
    pub files: Vec<SourceFile>,
    pub errors: Vec<String>,
}

pub fn expand_sources(paths: &[PathBuf]) -> ScanResult {
    let mut out = ScanResult::default();
    for p in paths {
        match meta::stat(p) {
            Ok(st) => match st.kind {
                Kind::File => out.files.push(SourceFile {
                    abs_path: p.clone(),
                    rel_path: PathBuf::from(p.file_name().unwrap_or(p.as_os_str())),
                    meta: st.meta,
                }),
                Kind::Dir => {
                    // A dropped folder is copied *into* the target, keeping its name.
                    let base = PathBuf::from(p.file_name().unwrap_or_default());
                    walk(p, &base, &mut out);
                }
                Kind::Other => out
                    .errors
                    .push(format!("{}: symlinks and special files are not copied", p.display())),
            },
            Err(e) => out.errors.push(format!("{}: {e}", p.display())),
        }
    }
    out
}

fn walk(dir: &Path, rel: &Path, out: &mut ScanResult) {
    let rd = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) => {
            out.errors.push(format!("{}: {e}", dir.display()));
            return;
        }
    };
    for entry in rd {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                out.errors.push(format!("{}: {e}", dir.display()));
                continue;
            }
        };
        let path = entry.path();
        let child_rel = rel.join(entry.file_name());
        match meta::entry_stat(&entry) {
            Ok(st) => match st.kind {
                Kind::File => out.files.push(SourceFile {
                    abs_path: path,
                    rel_path: child_rel,
                    meta: st.meta,
                }),
                Kind::Dir => walk(&path, &child_rel, out),
                Kind::Other => out
                    .errors
                    .push(format!("{}: symlinks and special files are not copied", path.display())),
            },
            Err(e) => out.errors.push(format!("{}: {e}", path.display())),
        }
    }
}

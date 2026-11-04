//! Expand the user's source list (any mix of files and folders) into files.

use crate::meta::{self, Kind};
use crate::model::{FileMeta, SourceFile};
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
                Kind::File => {
                    let name = p.file_name().unwrap_or(p.as_os_str());
                    out.files.push(source_file(p.clone(), Path::new(""), name, st.meta));
                }
                Kind::Dir => {
                    // A dropped folder is copied *into* the target, keeping its name.
                    let base = PathBuf::from(p.file_name().unwrap_or_default());
                    walk(p, &base, &mut out);
                }
                Kind::Other | Kind::ICloudStub => out
                    .errors
                    .push(format!("{}: symlinks and special files are not copied", p.display())),
            },
            Err(e) => out.errors.push(format!("{}: {e}", p.display())),
        }
    }
    out
}

/// Build a source entry, recognizing older-macOS iCloud stand-ins: the
/// `.Name.icloud` file is listed under its real name, as cloud-only, with an
/// unknown size.
fn source_file(abs_path: PathBuf, rel_dir: &Path, name: &std::ffi::OsStr, mut meta: FileMeta) -> SourceFile {
    match meta::icloud_stub_real_name(name) {
        Some(real) => {
            meta.cloud_only = true;
            meta.size = 0;
            SourceFile { abs_path, rel_path: rel_dir.join(real), meta, icloud_stub: true }
        }
        None => SourceFile { abs_path, rel_path: rel_dir.join(name), meta, icloud_stub: false },
    }
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
                Kind::File => out.files.push(source_file(path, rel, &entry.file_name(), st.meta)),
                Kind::Dir => walk(&path, &child_rel, out),
                Kind::Other | Kind::ICloudStub => out
                    .errors
                    .push(format!("{}: symlinks and special files are not copied", path.display())),
            },
            Err(e) => out.errors.push(format!("{}: {e}", path.display())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_keep_their_name_and_stubs_get_real_names() {
        let root = tempfile::tempdir().unwrap();
        let docs = root.path().join("Docs");
        fs::create_dir_all(docs.join("sub")).unwrap();
        fs::write(docs.join("a.txt"), b"abc").unwrap();
        fs::write(docs.join("sub").join(".Report.pdf.icloud"), b"plist").unwrap();
        let loose = root.path().join(".Loose.key.icloud");
        fs::write(&loose, b"plist").unwrap();

        let r = expand_sources(&[docs, loose]);
        assert!(r.errors.is_empty(), "{:?}", r.errors);
        let mut rels: Vec<_> = r
            .files
            .iter()
            .map(|f| (f.rel_path.to_string_lossy().replace('\\', "/"), f.icloud_stub, f.meta.cloud_only))
            .collect();
        rels.sort();
        assert_eq!(
            rels,
            vec![
                ("Docs/a.txt".to_string(), false, false),
                ("Docs/sub/Report.pdf".to_string(), true, true),
                ("Loose.key".to_string(), true, true),
            ]
        );
    }
}

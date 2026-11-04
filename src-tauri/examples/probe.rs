//! Read-only diagnostic: for each folder given, print its cloud sync root and
//! how many files (up to 2 levels deep) are cloud-only vs local. Reads
//! metadata only; never opens or downloads files.
//!
//!     cargo run --example probe -- "C:\Users\me\Dropbox" "C:\Users\me\OneDrive"

use polimath_file_copy_lib::{meta, platform, scan};
use std::path::PathBuf;

fn main() {
    let env = platform::environment();
    println!("OS: {} {} (cloud detection: {})", env.os, env.version, env.cloud_detection);
    for arg in std::env::args().skip(1) {
        let dir = PathBuf::from(&arg);
        let root = platform::sync_root(&dir);
        let mut sample = Vec::new();
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for e in rd.flatten().take(400) {
                sample.push(e.path());
            }
        }
        let files: Vec<_> = sample
            .iter()
            .filter(|p| meta::stat(p).is_ok_and(|s| s.kind == meta::Kind::File))
            .cloned()
            .collect();
        let r = scan::expand_sources(&files);
        let cloud = r.files.iter().filter(|f| f.meta.cloud_only).count();
        println!(
            "{arg}\n  sync root: {:?}\n  top-level files: {} ({} cloud-only, {} local)",
            root.map(|r| format!("{} [{}]", r.provider, r.id)),
            r.files.len(),
            cloud,
            r.files.len() - cloud
        );
        if let Some(f) = r.files.iter().find(|f| f.meta.cloud_only) {
            println!("  e.g. cloud-only: {} ({} bytes)", f.rel_path.display(), f.meta.size);
        }
        if let Some(f) = r.files.iter().find(|f| !f.meta.cloud_only) {
            println!("  e.g. local:      {} ({} bytes)", f.rel_path.display(), f.meta.size);
        }
    }
}

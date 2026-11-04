//! OS facts the guardrails depend on: OS version, which cloud sync root (if
//! any) a path lives in, and free disk space.

use crate::model::{Level, Warning};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

#[derive(Debug, Clone, Serialize)]
pub struct Environment {
    /// "windows", "macos", "linux", ...
    pub os: &'static str,
    pub version: String,
    pub major: u32,
    /// Older macOS (13 and earlier) shows evicted iCloud Drive files as
    /// hidden `.Name.icloud` stubs instead of dataless files.
    pub icloud_stubs: bool,
    /// Whether this OS lets us tell cloud-only placeholders from local files.
    pub cloud_detection: bool,
    /// Things worth telling the user once, at launch.
    pub notes: Vec<Warning>,
}

pub fn environment() -> &'static Environment {
    static ENV: OnceLock<Environment> = OnceLock::new();
    ENV.get_or_init(detect_environment)
}

fn detect_environment() -> Environment {
    let os = std::env::consts::OS;
    let version = os_version().unwrap_or_default();
    let major = version
        .split(|c: char| !c.is_ascii_digit())
        .find(|s| !s.is_empty())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let icloud_stubs = os == "macos" && major > 0 && major < 14;
    let cloud_detection = matches!(os, "windows" | "macos");

    let mut notes = Vec::new();
    if icloud_stubs {
        notes.push(Warning::new(
            Level::Warn,
            "old_icloud",
            format!(
                "macOS {version}: iCloud Drive files that aren't downloaded appear as hidden \
                 \".name.icloud\" stand-ins. They are always skipped. In Finder, choose \
                 \"Download Now\" on those files first, or upgrade to macOS 14 or later."
            ),
        ));
    }
    if !cloud_detection {
        notes.push(Warning::new(
            Level::Info,
            "no_cloud_detection",
            "On this OS the app can't tell cloud-only placeholders from local files. \
             Copying from an on-demand sync folder may download files."
                .into(),
        ));
    }
    Environment { os, version, major, icloud_stubs, cloud_detection, notes }
}

fn os_version() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        let out = std::process::Command::new("sw_vers").arg("-productVersion").output().ok()?;
        Some(String::from_utf8_lossy(&out.stdout).trim().to_owned())
    }
    #[cfg(windows)]
    {
        // "Microsoft Windows [Version 10.0.26300.1234]"
        let out = std::process::Command::new("cmd").args(["/c", "ver"]).output().ok()?;
        let s = String::from_utf8_lossy(&out.stdout).into_owned();
        let v = s.split("Version").nth(1)?.trim().trim_end_matches(']').trim();
        Some(v.to_owned())
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let s = std::fs::read_to_string("/etc/os-release").ok()?;
        s.lines()
            .find_map(|l| l.strip_prefix("PRETTY_NAME="))
            .map(|v| v.trim_matches('"').to_owned())
    }
}

/// A folder kept in sync by a cloud client (Dropbox, OneDrive, Synology
/// Drive, iCloud Drive, ...). Files with the same `id` are in the same root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SyncRoot {
    pub id: String,
    pub provider: String,
}

pub type RootFn<'a> = &'a dyn Fn(&Path) -> Option<SyncRoot>;

/// The nearest ancestor of `path` (including itself) that exists.
pub fn nearest_existing(path: &Path) -> Option<PathBuf> {
    path.ancestors().find(|a| crate::meta::stat(a).is_ok()).map(Path::to_path_buf)
}

/// Which sync root contains `dir`. Pass a folder, not a file: on Windows the
/// lookup opens the path, and opening a folder never downloads anything.
#[cfg(windows)]
pub fn sync_root(dir: &Path) -> Option<SyncRoot> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::CloudFilters::{
        CF_SYNC_ROOT_INFO_STANDARD, CF_SYNC_ROOT_STANDARD_INFO, CfGetSyncRootInfoByPath,
    };
    let dir = nearest_existing(dir)?;
    let wide: Vec<u16> = dir.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    // Large, 8-byte-aligned buffer: the struct ends in a variable-length identity blob.
    let mut buf = vec![0u64; 8192];
    let mut returned = 0u32;
    // SAFETY: buffer is valid for its full length; `wide` is NUL-terminated.
    let hr = unsafe {
        CfGetSyncRootInfoByPath(
            wide.as_ptr(),
            CF_SYNC_ROOT_INFO_STANDARD,
            buf.as_mut_ptr().cast(),
            (buf.len() * 8) as u32,
            &mut returned,
        )
    };
    if hr < 0 {
        return None;
    }
    // SAFETY: on success the buffer starts with a CF_SYNC_ROOT_STANDARD_INFO.
    let info = unsafe { &*(buf.as_ptr() as *const CF_SYNC_ROOT_STANDARD_INFO) };
    let name_len = info.ProviderName.iter().position(|&c| c == 0).unwrap_or(256);
    let provider = String::from_utf16_lossy(&info.ProviderName[..name_len]);
    Some(SyncRoot {
        id: format!("{}:{}", volume_of(&dir), info.SyncRootFileId),
        provider: if provider.is_empty() { "cloud sync".into() } else { provider },
    })
}

#[cfg(windows)]
fn volume_of(path: &Path) -> String {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetVolumePathNameW;
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let mut out = vec![0u16; 1024];
    // SAFETY: both buffers are valid; `out` length is passed.
    let ok = unsafe { GetVolumePathNameW(wide.as_ptr(), out.as_mut_ptr(), out.len() as u32) };
    if ok == 0 {
        return String::new();
    }
    let n = out.iter().position(|&c| c == 0).unwrap_or(out.len());
    String::from_utf16_lossy(&out[..n]).to_lowercase()
}

/// macOS: File Provider domains live under `~/Library/CloudStorage/<Provider>`;
/// iCloud Drive lives under `~/Library/Mobile Documents`.
#[cfg(target_os = "macos")]
pub fn sync_root(dir: &Path) -> Option<SyncRoot> {
    let home = PathBuf::from(std::env::var_os("HOME")?);
    mac_sync_root(dir, &home)
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn mac_sync_root(dir: &Path, home: &Path) -> Option<SyncRoot> {
    let cloud_storage = home.join("Library/CloudStorage");
    let mobile_docs = home.join("Library/Mobile Documents");
    for a in dir.ancestors() {
        if a == mobile_docs {
            return Some(SyncRoot { id: a.display().to_string(), provider: "iCloud Drive".into() });
        }
        if a.parent() == Some(cloud_storage.as_path()) {
            let name = a.file_name()?.to_string_lossy();
            // Folder names look like "Dropbox", "OneDrive-Personal", "SynologyDrive-host".
            let provider = name.split('-').next().unwrap_or(&name).to_owned();
            return Some(SyncRoot { id: a.display().to_string(), provider });
        }
    }
    None
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn sync_root(_dir: &Path) -> Option<SyncRoot> {
    None
}

/// Identifies the volume holding `path` (or its nearest existing ancestor).
/// Two paths on the same volume can be moved by a cheap rename.
pub fn volume_id(path: &Path) -> Option<String> {
    let dir = nearest_existing(path)?;
    #[cfg(windows)]
    {
        Some(volume_of(&dir)).filter(|v| !v.is_empty())
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Some(std::fs::symlink_metadata(&dir).ok()?.dev().to_string())
    }
}

/// Bytes available to this user on the volume holding `path` (or its nearest
/// existing ancestor).
pub fn free_space(path: &Path) -> Option<u64> {
    let dir = nearest_existing(path)?;
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
        let wide: Vec<u16> = dir.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
        let mut avail = 0u64;
        // SAFETY: valid NUL-terminated path and out-pointer; others may be null.
        let ok = unsafe {
            GetDiskFreeSpaceExW(wide.as_ptr(), &mut avail, std::ptr::null_mut(), std::ptr::null_mut())
        };
        (ok != 0).then_some(avail)
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let c = std::ffi::CString::new(dir.as_os_str().as_bytes()).ok()?;
        // SAFETY: `c` is a valid C string and `st` a valid out-pointer.
        unsafe {
            let mut st: libc::statvfs = std::mem::zeroed();
            if libc::statvfs(c.as_ptr(), &mut st) != 0 {
                return None;
            }
            Some(st.f_bavail as u64 * st.f_frsize as u64)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn environment_is_sane() {
        let e = environment();
        assert!(!e.os.is_empty());
        if e.os == "windows" || e.os == "macos" {
            assert!(e.major > 0, "version {:?}", e.version);
        }
    }

    #[test]
    fn free_space_for_temp_dir() {
        let d = tempfile::tempdir().unwrap();
        assert!(free_space(&d.path().join("not/yet/created")).unwrap() > 0);
    }

    #[test]
    fn temp_dir_is_not_a_sync_root() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(sync_root(d.path()), None);
    }

    #[test]
    fn mac_roots_by_path() {
        let home = Path::new("/Users/me");
        let r = mac_sync_root(Path::new("/Users/me/Library/CloudStorage/Dropbox/Photos"), home).unwrap();
        assert_eq!(r.provider, "Dropbox");
        assert_eq!(r.id, "/Users/me/Library/CloudStorage/Dropbox");
        let r = mac_sync_root(
            Path::new("/Users/me/Library/Mobile Documents/com~apple~CloudDocs/Taxes"),
            home,
        )
        .unwrap();
        assert_eq!(r.provider, "iCloud Drive");
        assert_eq!(mac_sync_root(Path::new("/Users/me/Desktop"), home), None);
    }
}

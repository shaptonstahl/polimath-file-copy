//! Reading file metadata **without opening files**.
//!
//! Synced folders (OneDrive Files On-Demand, Dropbox online-only, Synology
//! Drive On-demand Sync) keep placeholders whose size and dates are real but
//! whose content lives in the cloud. Opening such a file can trigger a
//! download, so every lookup here sticks to directory-entry data:
//!
//! * Windows: `FindFirstFileExW` for single paths, and `read_dir` (whose
//!   entries already carry `WIN32_FIND_DATA`) for listings. `std::fs::metadata`
//!   is avoided because it opens a handle.
//! * Unix: `lstat` (`symlink_metadata`), which never opens the file.

use crate::model::FileMeta;
use std::fs;
use std::io;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    File,
    Dir,
    /// Symlink, junction, device, etc. Not followed.
    Other,
    /// Older-macOS iCloud stand-in (`.Name.icloud`) for a file that isn't
    /// downloaded. Only produced by target lookups.
    ICloudStub,
}

#[derive(Debug, Clone, Copy)]
pub struct Stat {
    pub kind: Kind,
    pub meta: FileMeta,
}

pub fn system_time_ms(t: SystemTime) -> i64 {
    match t.duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_millis() as i64,
        Err(e) => -(e.duration().as_millis() as i64),
    }
}

/// Build a [`Stat`] from metadata obtained from a directory entry or `lstat`.
pub fn from_std(md: &fs::Metadata) -> Stat {
    let ft = md.file_type();
    let kind = if ft.is_symlink() {
        Kind::Other
    } else if ft.is_dir() {
        Kind::Dir
    } else if ft.is_file() {
        Kind::File
    } else {
        Kind::Other
    };
    Stat {
        kind,
        meta: FileMeta {
            size: if kind == Kind::File { md.len() } else { 0 },
            mtime_ms: md.modified().map(system_time_ms).unwrap_or(0),
            cloud_only: is_cloud_only(md),
        },
    }
}

#[cfg(windows)]
mod win {
    pub const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
    pub const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    pub const FILE_ATTRIBUTE_OFFLINE: u32 = 0x1000;
    pub const FILE_ATTRIBUTE_RECALL_ON_OPEN: u32 = 0x4_0000;
    pub const FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS: u32 = 0x40_0000;
    pub const IO_REPARSE_TAG_SYMLINK: u32 = 0xA000_000C;
    pub const IO_REPARSE_TAG_MOUNT_POINT: u32 = 0xA000_0003;

    pub fn attrs_cloud_only(attrs: u32) -> bool {
        attrs
            & (FILE_ATTRIBUTE_OFFLINE
                | FILE_ATTRIBUTE_RECALL_ON_OPEN
                | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS)
            != 0
    }
}

#[cfg(windows)]
fn is_cloud_only(md: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    win::attrs_cloud_only(md.file_attributes())
}

#[cfg(target_os = "macos")]
fn is_cloud_only(md: &fs::Metadata) -> bool {
    use std::os::macos::fs::MetadataExt;
    const SF_DATALESS: u32 = 0x4000_0000;
    md.st_flags() & SF_DATALESS != 0
}

#[cfg(not(any(windows, target_os = "macos")))]
fn is_cloud_only(_md: &fs::Metadata) -> bool {
    false
}

/// Look up one path without opening it.
#[cfg(not(windows))]
pub fn stat(path: &Path) -> io::Result<Stat> {
    fs::symlink_metadata(path).map(|md| from_std(&md))
}

#[cfg(windows)]
pub fn stat(path: &Path) -> io::Result<Stat> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::Storage::FileSystem::{
        FindClose, FindExInfoBasic, FindExSearchNameMatch, FindFirstFileExW, WIN32_FIND_DATAW,
    };

    // FindFirstFile can't describe a drive root ("C:\") or a path ending in a
    // separator; fall back for those (opening a directory never hydrates it).
    let s = path.as_os_str().to_string_lossy();
    if path.parent().is_none() || s.ends_with('\\') || s.ends_with('/') {
        return fs::metadata(path).map(|md| from_std(&md));
    }

    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    // SAFETY: `wide` is NUL-terminated and `data` is a valid out-pointer.
    let data = unsafe {
        let mut data: WIN32_FIND_DATAW = std::mem::zeroed();
        let h = FindFirstFileExW(
            wide.as_ptr(),
            FindExInfoBasic,
            &mut data as *mut _ as *mut _,
            FindExSearchNameMatch,
            std::ptr::null(),
            0,
        );
        if h == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        FindClose(h);
        data
    };

    let attrs = data.dwFileAttributes;
    let is_link = attrs & win::FILE_ATTRIBUTE_REPARSE_POINT != 0
        && matches!(
            data.dwReserved0,
            win::IO_REPARSE_TAG_SYMLINK | win::IO_REPARSE_TAG_MOUNT_POINT
        );
    let kind = if is_link {
        Kind::Other
    } else if attrs & win::FILE_ATTRIBUTE_DIRECTORY != 0 {
        Kind::Dir
    } else {
        Kind::File
    };
    let ft = ((data.ftLastWriteTime.dwHighDateTime as u64) << 32)
        | data.ftLastWriteTime.dwLowDateTime as u64;
    // FILETIME: 100 ns ticks since 1601-01-01.
    let mtime_ms = (ft / 10_000) as i64 - 11_644_473_600_000;
    let size = ((data.nFileSizeHigh as u64) << 32) | data.nFileSizeLow as u64;
    Ok(Stat {
        kind,
        meta: FileMeta {
            size: if kind == Kind::File { size } else { 0 },
            mtime_ms,
            cloud_only: win::attrs_cloud_only(attrs),
        },
    })
}

/// Metadata for a directory entry. On Windows this comes free with the
/// listing; on Unix it costs one `lstat`.
pub fn entry_stat(entry: &fs::DirEntry) -> io::Result<Stat> {
    #[cfg(windows)]
    {
        entry.metadata().map(|md| from_std(&md))
    }
    #[cfg(not(windows))]
    {
        fs::symlink_metadata(entry.path()).map(|md| from_std(&md))
    }
}

/// Older macOS (13 and earlier) replaces an iCloud Drive file `Name` that
/// isn't downloaded with a small hidden stand-in named `.Name.icloud`.
/// Returns the real name for such a stand-in.
pub fn icloud_stub_real_name(name: &std::ffi::OsStr) -> Option<std::ffi::OsString> {
    let inner = name.to_str()?.strip_prefix('.')?.strip_suffix(".icloud")?;
    (!inner.is_empty()).then(|| inner.into())
}

/// The stand-in name older macOS would use for `name`.
pub fn icloud_stub_name(name: &std::ffi::OsStr) -> std::ffi::OsString {
    format!(".{}.icloud", name.to_string_lossy()).into()
}

/// Name normalization for matching target entries: Windows and macOS file
/// systems are case-insensitive by default.
pub fn name_key(name: &std::ffi::OsStr) -> String {
    let s = name.to_string_lossy();
    if cfg!(any(windows, target_os = "macos")) {
        s.to_lowercase()
    } else {
        s.into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn stat_matches_std_for_regular_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.txt");
        fs::File::create(&p).unwrap().write_all(b"hello").unwrap();
        let s = stat(&p).unwrap();
        assert_eq!(s.kind, Kind::File);
        assert_eq!(s.meta.size, 5);
        assert!(!s.meta.cloud_only);
        let std_ms = system_time_ms(fs::metadata(&p).unwrap().modified().unwrap());
        assert!((s.meta.mtime_ms - std_ms).abs() <= 1);
    }

    #[test]
    fn icloud_stub_names() {
        use std::ffi::OsStr;
        assert_eq!(icloud_stub_real_name(OsStr::new(".Report.pdf.icloud")).unwrap(), "Report.pdf");
        assert_eq!(icloud_stub_real_name(OsStr::new(".a.icloud")).unwrap(), "a");
        assert!(icloud_stub_real_name(OsStr::new("Report.pdf.icloud")).is_none());
        assert!(icloud_stub_real_name(OsStr::new("..icloud")).is_none());
        assert_eq!(icloud_stub_name(OsStr::new("Report.pdf")), ".Report.pdf.icloud");
    }

    #[test]
    fn stat_dir_and_missing() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(stat(dir.path()).unwrap().kind, Kind::Dir);
        let e = stat(&dir.path().join("nope")).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::NotFound);
    }
}

# Polimath File Copy

A free, open-source desktop file copy and move utility for Windows, Linux and macOS. It is built for sources and targets that may be **remote or synced**: Synology Drive, Dropbox, OneDrive, mapped drives, and shares over a VPN.

- Build a source list from any mix of files and folders, by drag and drop or file pickers, then drop or choose a target folder.
- Choose what happens when a file already exists: skip, replace older, replace newer, replace smaller, replace larger, replace if different, or always replace.
- **Preview first.** The plan is built from metadata only (name, size, modified time). File contents are never read just to compare.
- **Cloud-only placeholders** (OneDrive Files On-Demand, Dropbox online-only, Synology Drive On-demand Sync) are detected and skipped by default, because copying one would download it.
- **Cloud-only files can still be moved within their cloud folder.** Inside one sync root (for example, from one Dropbox folder to another), a cloud-only file is moved by renaming it, which downloads nothing. If that rename fails, the move fails; it never falls back to copy + delete.
- **Older-macOS iCloud stand-ins.** macOS 13 and earlier show iCloud files that aren't downloaded as hidden `.Name.icloud` files. These are recognized on both sides, shown under their real names and always skipped.
- **Guardrails**, shown before Preview and again before Run. A ⛔ warning must be acknowledged before Run is enabled.
  - The OS version is checked at launch (older iCloud Drive format on macOS, no cloud-only detection on Linux).
  - ⛔ Photos and Music libraries as a source: they can be silently incomplete when originals are only in iCloud. As a target: writing into them can corrupt them.
  - ⛔ Not enough free space at the target. ⚠️ when the job would use most of it.
  - ⚠️ Moving out of a cloud folder deletes the files from that cloud on every device.
  - ⚠️ Cloud-only files that will be downloaded, with a total size.
  - ⚠️ Two source files that map to the same target path. The first one wins.
  - ⚠️ A target that already has an iCloud stand-in for a file.
  - ⚠️ Sync-looking folders on an OS that can't detect placeholders.
  - ℹ️ The rclone engine on macOS drops Finder tags and extended attributes.
- **Timestamp tolerance** (2 s by default). NAS, SMB and cloud drives often round timestamps, which would otherwise make every file look changed.
- **Fewer round trips.** For each target folder the app either lists the folder once or looks up just the needed files, whichever is cheaper. A missing folder is detected once, and everything inside it counts as new.
- **Parallel transfers** hide per-file latency on slow links.
- **Two engines.** The built-in engine copies to a temporary name, renames it into place and preserves the modified time. The [rclone](https://rclone.org) engine is used automatically when `rclone` is on `PATH` or `RCLONE_PATH` is set.

Every dependency is free and open source. See [docs/RESEARCH.md](docs/RESEARCH.md) for why this exists rather than an rsync, rclone or FreeFileSync GUI.

## Install

[![CI](https://github.com/shaptonstahl/polimath-file-copy/actions/workflows/ci.yml/badge.svg)](https://github.com/shaptonstahl/polimath-file-copy/actions/workflows/ci.yml)
[![Latest release](https://img.shields.io/github/v/release/shaptonstahl/polimath-file-copy?include_prereleases)](https://github.com/shaptonstahl/polimath-file-copy/releases/latest)

Download from the [latest release](https://github.com/shaptonstahl/polimath-file-copy/releases/latest):

- **Windows 10/11:** setup `.exe`, `.msi`, or a portable `.exe`
- **macOS:** universal `.dmg` for Apple silicon and Intel
- **Linux:** `.deb`, `.rpm` or `.AppImage`

The builds are not code-signed, since this is a free project with no budget for signing certificates. [docs/INSTALL.md](docs/INSTALL.md) shows how to get past the Windows and macOS first-run warnings, and how to check a download against the SHA-256 checksums that CI publishes with every release.

## Build from source

Install Git, [Rust](https://rustup.rs), [Node.js](https://nodejs.org) LTS, and your OS's [Tauri prerequisites](docs/BUILDING.md#1-install-the-prerequisites). Then:

```bash
git clone https://github.com/shaptonstahl/polimath-file-copy.git
```

```bash
cd polimath-file-copy && npm ci
```

```bash
npm run tauri build
```

[docs/BUILDING.md](docs/BUILDING.md) has per-OS prerequisites (Windows, macOS, Debian/Ubuntu, Fedora, Arch, openSUSE), dev mode, tests, where installers end up, read-only diagnostics, and how releases are made.

## Layout

| Path | What it does |
|---|---|
| `src-tauri/src/meta.rs` | Metadata reads that never open files (`FindFirstFileExW` on Windows, `lstat` on Unix) and placeholder detection |
| `src-tauri/src/scan.rs` | Expands dropped files and folders into a flat file list |
| `src-tauri/src/target.rs` | Target lookup: list the folder vs. look up each file |
| `src-tauri/src/decide.rs` | Overwrite rules |
| `src-tauri/src/planner.rs` | Ties scanning, lookup and rules together into a reviewable plan |
| `src-tauri/src/platform.rs` | OS version, sync-root detection (Windows cloud-files API; macOS `~/Library/CloudStorage` and iCloud Drive), free space |
| `src-tauri/src/guard.rs` | Path and plan guardrails |
| `src-tauri/src/execute.rs` | Parallel executor and the built-in engine |
| `src-tauri/src/rclone.rs` | rclone engine via `rclone rcd` on localhost |
| `src/main.ts` | UI: drag and drop, options, preview table, progress |

## Roadmap

- rclone remotes as sources and targets (`dropbox:`, `onedrive:`, Synology over WebDAV or SFTP), so transfers skip the desktop sync client, with server-side copy and move within one provider.
- Detect sync-client roots (Cloud Files API sync roots on Windows, File Provider domains on macOS) and suggest the matching rclone remote.
- Option to bundle rclone as a sidecar binary.
- Saved jobs and a log of past runs.

## License

MIT

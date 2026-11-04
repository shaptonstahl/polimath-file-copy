# Polimath File Copy

A free, open-source desktop file copy and move utility for Windows, Linux and macOS. It is built for sources and targets that may be **remote or synced**: Synology Drive, Dropbox, OneDrive, mapped drives, and shares over a VPN.

- Build a source list from any mix of files and folders, by drag and drop or file pickers, then drop or choose a target folder.
- Choose what happens when a file already exists: skip, replace older, replace newer, replace smaller, replace larger, replace if different, or always replace.
- **Preview first.** The plan is built from metadata only (name, size, modified time). File contents are never read just to compare.
- **Cloud-only placeholders** (OneDrive Files On-Demand, Dropbox online-only, Synology Drive On-demand Sync) are detected and skipped by default, because copying one would download it.
- **Timestamp tolerance** (2 s by default). NAS, SMB and cloud drives often round timestamps, which would otherwise make every file look changed.
- **Fewer round trips.** For each target folder the app either lists the folder once or looks up just the needed files, whichever is cheaper. A missing folder is detected once, and everything inside it counts as new.
- **Parallel transfers** hide per-file latency on slow links.
- **Two engines.** The built-in engine copies to a temporary name, renames it into place and preserves the modified time. The [rclone](https://rclone.org) engine is used automatically when `rclone` is on `PATH` or `RCLONE_PATH` is set.

Every dependency is free and open source. See [docs/RESEARCH.md](docs/RESEARCH.md) for why this exists rather than an rsync, rclone or FreeFileSync GUI.

## Development

Prerequisites: [Rust](https://rustup.rs), Node.js 20+, and the [Tauri system prerequisites](https://tauri.app/start/prerequisites/) for your OS (WebView2 on Windows, webkit2gtk on Linux).

```bash
npm install
```

```bash
npm run tauri dev
```

Run the Rust unit tests:

```bash
cargo test --manifest-path src-tauri/Cargo.toml
```

Build installers:

```bash
npm run tauri build
```

## Layout

| Path | What it does |
|---|---|
| `src-tauri/src/meta.rs` | Metadata reads that never open files (`FindFirstFileExW` on Windows, `lstat` on Unix) and placeholder detection |
| `src-tauri/src/scan.rs` | Expands dropped files and folders into a flat file list |
| `src-tauri/src/target.rs` | Target lookup: list the folder vs. look up each file |
| `src-tauri/src/decide.rs` | Overwrite rules |
| `src-tauri/src/planner.rs` | Ties scanning, lookup and rules together into a reviewable plan |
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

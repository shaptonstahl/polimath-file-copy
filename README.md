![Polimath hero image](docs/assets/polimath_hero.jpg)

# Polimath File Copy

A desktop app for copying and moving files between folders that might be synced or remote: Dropbox, OneDrive, Synology Drive, iCloud Drive, mapped drives, or shares over a VPN. It runs on Windows, macOS and Linux, and it's free and open source.

[![CI](https://github.com/shaptonstahl/polimath-file-copy/actions/workflows/ci.yml/badge.svg)](https://github.com/shaptonstahl/polimath-file-copy/actions/workflows/ci.yml)
[![Latest release](https://img.shields.io/github/v/release/shaptonstahl/polimath-file-copy?include_prereleases)](https://github.com/shaptonstahl/polimath-file-copy/releases/latest)

> [!NOTE]
> Ordinary copy tools treat a synced folder like a local disk. That's slow when the files live in the cloud, and risky when "moving" a file out of Dropbox deletes it from every device. This app decides what to copy from file metadata alone, recognizes files that exist only in the cloud, and warns before risky operations. [docs/RESEARCH.md](docs/RESEARCH.md) explains why it exists rather than an rsync, rclone or FreeFileSync front end.

## Features

- **Drag and drop.** Build a source list from any mix of files and folders, by drag and drop or file pickers, then drop or choose a target folder.
- **Preview first.** Every job is planned from metadata alone (name, size and modified time) and shown file by file before anything changes. Run then carries out exactly that plan.
- **Overwrite rules:** skip, replace older, replace newer, replace smaller, replace larger, replace if different, or always replace. See [Usage](#if-the-file-exists).
- **Cloud-only files.** OneDrive Files On-Demand, Dropbox online-only, Synology Drive On-demand Sync and iCloud files that aren't downloaded are recognized one by one. By default they're skipped, because copying one would download it.
- **Moves within a cloud folder.** Inside one sync folder (say, from one Dropbox folder to another), a cloud-only file is moved by renaming it, which downloads nothing. If the rename fails, the move fails; it never falls back to copy-then-delete.
- **Older macOS iCloud.** macOS 13 and earlier show iCloud files that aren't downloaded as hidden `.Name.icloud` stand-ins. These are recognized at both the source and the target, listed under their real names, and always skipped.
- **Guardrails.** Warnings appear before Preview and again before Run. A ⛔ warning must be acknowledged before Run is enabled. See [Warnings](#warnings).
- **Built for slow links:**
  - **Timestamp tolerance.** Times within 2 seconds (adjustable) count as equal, because NAS, SMB and cloud drives often round timestamps.
  - **Fewer round trips.** For each target folder the app either lists the folder once or looks up just the files it needs, whichever is cheaper.
  - **Parallel transfers** hide per-file latency.
- **Optional logs.** Every Preview and Run can write a log listing every file, the facts behind its decision, the decision and the result. See [Logs](#logs).
- **End-of-job summary.** A popup shows whether the job succeeded, finished with errors, failed or was cancelled, with counts and a link to the log.
- **Two transfer engines.** The built-in engine writes to a temporary name, renames it into place and keeps the modified time. [rclone](https://rclone.org) is offered when it's installed.

## Privacy

- **No network.** The app makes no network requests: no telemetry, no update checks, no accounts. Its fonts and images are bundled.
- **The rclone engine** talks only to a private rclone process on `127.0.0.1`, guarded by a random password.
- **Metadata only, when comparing.** File contents are read only to copy or move a file you approved in the preview.
- **Logs and settings stay local,** in your user account's folders (see [Logs](#logs)).

## Installation

### From a release

Download the file for your system from the [latest release](https://github.com/shaptonstahl/polimath-file-copy/releases/latest):

| System | File |
|---|---|
| Windows 10/11 | `…_x64-setup.exe` (recommended), `…_x64_en-US.msi`, or the portable `polimath-file-copy.exe` |
| macOS 11 or later | `…_universal.dmg` (Apple silicon and Intel) |
| Linux | `…_amd64.deb`, `…x86_64.rpm` or `…_amd64.AppImage` |

> [!CAUTION]
> The builds are **not code-signed**, because this is a free project with no budget for signing certificates. Windows SmartScreen and macOS Gatekeeper warn the first time you open the app. [docs/INSTALL.md](docs/INSTALL.md) explains what to click, and how to check your download against the SHA-256 checksums that CI publishes with every release.

### From source

1. Install Git, [Rust](https://rustup.rs), [Node.js](https://nodejs.org) LTS, and your OS's [Tauri prerequisites](docs/BUILDING.md#1-install-the-prerequisites).
2. Clone the repository and install its packages:

   ```bash
   git clone https://github.com/shaptonstahl/polimath-file-copy.git
   ```

   ```bash
   cd polimath-file-copy && npm ci
   ```

3. Build installers for the OS you're on:

   ```bash
   npm run tauri build
   ```

[docs/BUILDING.md](docs/BUILDING.md) has per-OS prerequisites (Windows, macOS, Debian/Ubuntu, Fedora, Arch, openSUSE), where the installers end up, and how to build a universal macOS app.

### Optional: rclone

If [rclone](https://rclone.org/install/) (free, MIT licensed) is on your `PATH`, or `RCLONE_PATH` points to it, **Engine** offers it alongside the built-in engine.

## Usage

### Copying or moving files

1. **Add sources.** Drag files and folders onto **Sources**, or use **Add files…** and **Add folders…**. A folder is copied *into* the target under its own name.
2. **Choose the target.** Drag a folder onto **Target folder**, or use **Choose…**.
3. **Set the options:** **Operation** (Copy or Move), **If the file exists**, **Time tolerance**, **Skip cloud-only sources**, **Engine**, **Parallel**, and **Write log**.
4. **Preview.** The table lists every file with what will happen to it (☁ marks cloud-only files), and the warnings update.
5. **Run.** If any ⛔ warning is showing, first tick **I understand the warnings marked ⛔**. Changing any option clears the preview, so Run always does what you last previewed.
6. **Read the summary.** The popup shows how the job ended and, if logging was on, the log's full path.

### If the file exists

Each rule is named for the file at the **target**: "replace older" means "overwrite the target file if it is older".

| Rule | Overwrites the target when |
|---|---|
| Skip it | never |
| Replace older | the target's modified time is earlier than the source's by more than the tolerance |
| Replace newer | the target's modified time is later than the source's by more than the tolerance |
| Replace smaller | the target is smaller than the source |
| Replace larger | the target is larger than the source |
| Replace if different | the sizes differ, or the times differ by more than the tolerance |
| Always replace | always |

### Warnings

| Level | When |
|---|---|
| ⛔ | A Photos or Music library is a source (it can be silently incomplete when originals are only in iCloud) or the target (writing into it can corrupt it) |
| ⛔ | Not enough free space at the target |
| ⚠️ | Moving out of a cloud folder, which deletes the files from that cloud on every device |
| ⚠️ | Cloud-only files will be downloaded (shows the count and total size) |
| ⚠️ | Two source files map to the same target path (the first one wins) |
| ⚠️ | iCloud `.icloud` stand-ins at the source or target, or iCloud paths on macOS 13 or earlier |
| ⚠️ | The job will use most of the free space at the target |
| ⚠️ | A folder looks like a cloud sync folder on Linux, where cloud-only files can't be detected |
| ℹ️ | Counts of skipped cloud-only files, renamed moves, and skipped files that stay put during a move |
| ℹ️ | The rclone engine on macOS doesn't keep Finder tags or extended attributes |

### Logs

Tick **Write log** to log every Preview and Run. By default, logs go to your user account's log folder:

| OS | Default log folder |
|---|---|
| Windows | `%LOCALAPPDATA%\com.polimath.filecopy\logs` |
| macOS | `~/Library/Logs/com.polimath.filecopy` |
| Linux | `~/.local/share/com.polimath.filecopy/logs` |

To use another folder, open **⚙ Settings**. Each log has:

- **a header:** preview or run, app and OS version, options, sources, target, plan summary, warnings and errors;
- **one tab-separated row per file:** the decision and why; the source's and target's size, modified time and cloud-only status; whether the file moves by renaming; the result; and both full paths;
- **a footer:** finish time, duration and how the job ended.

Preview logs say `PREVIEW ONLY` on their first and last lines and have `_preview` in the file name, for example `polimath-file-copy_2026-11-04_155626_copy_preview.log`. More detail is in [docs/INSTALL.md](docs/INSTALL.md#logs).

## Development

### Prerequisites

The same as [building from source](#from-source). [docs/BUILDING.md](docs/BUILDING.md) lists the packages for each OS.

### Running the app and tests

Run the app with live reload:

```bash
npm run tauri dev
```

Run the Rust tests:

```bash
cargo test --manifest-path src-tauri/Cargo.toml
```

Check which files in a folder are cloud-only (reads metadata only):

```bash
cargo run --manifest-path src-tauri/Cargo.toml --example probe -- "C:\Users\me\Dropbox"
```

Print a plan's warnings without running it:

```bash
cargo run --manifest-path src-tauri/Cargo.toml --example plan -- move "D:\Backup" "C:\Users\me\Dropbox\Docs"
```

### Project structure

```
src/                     UI (TypeScript + Vite)
  main.ts                drag and drop, options, preview table, popups
  styles.css             Polimath "Nautical Architect" theme
  brand/                 brand tokens (branding.css) and icon
src-tauri/src/           Rust back end (Tauri 2)
  meta.rs                metadata reads that never open files; placeholder detection
  scan.rs                expands dropped files and folders into a file list
  target.rs              target lookup: list the folder vs. look up each file
  decide.rs              overwrite rules
  planner.rs             scanning + lookup + rules -> reviewable plan
  platform.rs            OS version, sync-root detection, free space
  guard.rs               path and plan warnings
  job.rs                 runs a previewed plan and writes its log
  joblog.rs              log file format
  outcome.rs             how a job ended, for the popup and log
  settings.rs            per-user settings
  execute.rs             parallel executor and the built-in engine
  rclone.rs              rclone engine via `rclone rcd` on localhost
src-tauri/examples/      read-only diagnostics (probe, plan)
docs/                    install, build and research notes
```

### Contributing

Work on a branch and open a pull request against `main`. CI builds and tests on Windows, macOS and Linux for every pull request.

### Releases and packaging

Pushing a tag such as `v0.2.0` runs the [Release workflow](.github/workflows/release.yml):

1. It checks that the tag matches the version in `package.json` and `src-tauri/Cargo.toml`.
2. It builds the Windows, macOS and Linux installers.
3. It attaches them, with `SHA256SUMS.txt` and a checksum table in the notes, to a GitHub Release.

The full process is in [docs/BUILDING.md](docs/BUILDING.md#releasing-maintainers).

### Roadmap

- **rclone remotes as sources and targets** (`dropbox:`, `onedrive:`, Synology over WebDAV or SFTP). Transfers would then skip the desktop sync client, and copies and moves within one provider could happen on the server.
- **Suggest the matching rclone remote** for a detected sync folder.
- **Bundle rclone** with the app as an option.
- **Saved jobs.**

---

MIT License. The Polimath name, emblem and "Nautical Architect" theme are Stephen Haptonstahl's personal brand.

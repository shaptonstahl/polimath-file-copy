# Research: existing tools vs. building our own

Date: 2026-11-04

## Requirements recap

- Desktop on Windows, Linux and macOS (a mobile version may come later).
- Build a source list from any mix of files and folders, with drag and drop. Pick a target folder. Then copy or move.
- Conditional overwrite rules ("replace older", "replace smaller", and similar) that are decided from **metadata only** (name, size, mtime), without downloading file content.
- The source or target may be a synced folder (Synology Drive, Dropbox, OneDrive), a mapped drive, or a share reached over a VPN.
- **Every part must be free and open source.** Nothing paid, freemium or license-gated.

## 1. Do existing CLI tools already decide from metadata only?

Yes. That part is not new. Each of these tools lists directories and compares size and mtime, and reads file content only when you ask it to (for example, a checksum mode).

| Tool | License | Metadata compare | "Pull everything at once" vs. "check specific files" | Timestamp tolerance | Platforms |
|---|---|---|---|---|---|
| **rsync** | GPL-3 | quick-check (size + mtime) by default; `--update`, `--size-only`, `--ignore-existing` | always walks the tree | `--modify-window` | Native on Linux and macOS. **Poor on Windows** (Cygwin or WSL only). Needs rsync at both ends for real remote work, which a Dropbox or OneDrive backend can't provide. |
| **rclone** | MIT | size + mtime by default; `--update`, `--size-only`, `--ignore-existing`, `--max-age` | `--fast-list` takes a recursive listing in a few large calls; `--no-traverse` checks only the named files instead of listing the whole target; `--files-from` takes an explicit file list | `--modify-window` | Windows, Linux, macOS. Also talks **directly to Dropbox, OneDrive, WebDAV and SFTP** (a Synology NAS offers WebDAV and SFTP), plus many more. |
| **robocopy** | Proprietary, but part of Windows | `/XO` `/XN` `/XC` `/XL` | always walks | `/FFT` (2 s) | Windows only. |

rclone's `--fast-list` / `--no-traverse` pair is the exact trade-off you asked about: one big listing call versus one lookup per file. rclone already makes that choice, per backend.

**No tool has "replace if smaller" (or "replace if larger") as a built-in rule.** rsync, rclone and robocopy can all *exclude* by age, and robocopy can exclude by size class, but none of them says "overwrite only when the source is bigger". That needs a small decision layer on top. One simple approach: compute the file list ourselves from a metadata listing (`rclone lsjson`), then hand that list to `rclone copy --files-from`.

## 2. What is actually different about synced folders

Synology Drive (On-demand Sync), OneDrive (Files On-Demand) and Dropbox (online-only files) all use the operating system's cloud-file framework: the **Cloud Files API** on Windows and **File Provider** on macOS. On Linux, Synology's support is experimental.

Findings:

1. **Listing a placeholder does not download it.** A placeholder has the real size and dates, and reading attributes with `stat` or a directory listing returns them without recalling the content. Windows flags placeholders with `FILE_ATTRIBUTE_OFFLINE`, `RECALL_ON_OPEN` or `RECALL_ON_DATA_ACCESS`; macOS uses `SF_DATALESS` in `st_flags`. So a metadata-only compare against a synced folder is cheap, as long as the tool doesn't open the file.
2. **Copying *from* a placeholder always downloads it.** There's no way around that through the local path. The only ways out are:
   - **skip cloud-only files** (robocopy `/XA:O`, or SyncBack's "do not copy offline files"), or
   - **bypass the sync client and use the provider's API**. rclone can do this, and when source and target are on the same provider it can use a *server-side copy or move*, so nothing travels through the PC at all. **This is the biggest real speed-up for synced and remote sources, and only rclone offers it among the free options.**
3. **Timestamp precision differs between file systems.** Cloud drives, SMB shares and NAS volumes often store mtimes more coarsely than NTFS or APFS. Without a tolerance, every file looks "changed" and gets recopied. A Microsoft Q&A thread reports exactly this with robocopy to a cloud drive, fixed by `/FFT`. Any tool we use needs a configurable tolerance (rclone `--modify-window`, FreeFileSync `FileTimeTolerance`).
4. **None of these tools detects a synced folder and handles it specially.** rclone's local backend has no Cloud Files API support (there is a closed forum feature request), and FreeFileSync users report that cloud-only OneDrive files sometimes drop out of comparisons.
5. **Latency hides behind parallelism.** Over a VPN or to a NAS, each small I/O call is slow. Running many in parallel helps a lot (rclone `--checkers` and `--transfers`; FreeFileSync's parallel operations).

## 3. GUIs, checked against the "fully free and open source" rule

| GUI | Engine | License / cost | Verdict |
|---|---|---|---|
| **FreeFileSync** | its own | GPL-3, **but parallel file copy is a Donation Edition feature**, along with the auto-updater, portable build and email alerts | Closest feature fit for folder-to-folder work: cross-platform, drag and drop of folders, time+size compare with tolerance, custom sync directions. But it compares folder *pairs* (no ad-hoc file lists), has no "replace smaller" rule and no cloud-provider API path, and the latency fix is behind the donation. **Doesn't meet the rule cleanly.** |
| **RcloneView** | rclone | Proprietary; free for personal use, commercial licensing planned | **Ruled out.** |
| **Rclone UI** (rclone-ui/rclone-ui) | rclone | Apache-2.0 repo, but its README promises contributors a "Lifetime License", which implies a paid licence tier | **Ruled out unless it's verified fully free.** |
| **RClone Manager** (Zarestia-Dev) | rclone | GPL-3.0, Tauri + Angular; Linux and Windows, macOS "coming soon" | FOSS. Aimed at managing remotes and mounts, not at a list-based "copy these files with these rules" workflow. Worth a quick try. |
| rclone built-in Web GUI (`rclone rcd --rc-web-gui`) | rclone | MIT | FOSS but very basic. No drag-drop list building, no conditional rules. |

## 4. Conclusion

- **The engine already exists: rclone.** It is MIT-licensed and cross-platform. It does metadata-only compares and lets you choose between bulk listing and per-file checks. It supports explicit file lists, a timestamp tolerance and parallelism. It can bypass sync clients by talking to Dropbox, OneDrive and Synology (via WebDAV or SFTP) directly, including server-side copy and move.
- **No free and open-source GUI does what's needed.** The missing pieces are:
  1. drag-and-drop building of a mixed file + folder source list;
  2. overwrite rules beyond newer/older (smaller, larger, and combinations);
  3. spotting that a path sits inside a Synology Drive, Dropbox or OneDrive folder, then either skipping cloud-only files or offering to route through the provider's API;
  4. a dry-run preview of exactly what will be copied, skipped or overwritten.

**Recommendation:** build a thin, FOSS, cross-platform GUI on top of rclone rather than writing a copy engine from scratch. The app plans the work (listing, rules, placeholder detection) and rclone does the transfers through its `rc` API or `librclone`. Tauri is a good fit for the GUI: it's small, it has native drag and drop on all three desktop platforms, and it has a mobile path later.

## 5. Not yet verified

- Whether FreeFileSync's or rclone's *compare* step ever hydrates Synology Drive placeholders. It shouldn't, but a 10-minute test on a folder of cloud-only files would settle it.
- Whether Rclone UI's "license" is actually a paid tier.

## Sources

- [Placeholder detection via stat / file attributes (lean-ctx cloud_files.rs)](https://docs.rs/lean-ctx/3.7.5/src/lean_ctx/core/cloud_files.rs.html)
- [ShellBoost: Files On-Demand placeholders](https://cdn.callback.com/shellboost/doc/Introduction/Files-On-Demand.html)
- [rclone forum: No-traverse explained](https://forum.rclone.org/t/no-traverse-for-dummies/2992)
- [rclone issue #9226: fast-list + onedrive-delta](https://github.com/rclone/rclone/issues/9226)
- [rclone forum: Windows Cloud Files API request](https://forum.rclone.org/t/support-windows-cloud-files-api-and-placeholder-files/18591)
- [Microsoft Q&A: robocopy recopying unchanged files to cloud (/FFT)](https://learn.microsoft.com/en-us/answers/a/1304242)
- [Cadzow: robocopy /XA:O with Dropbox Smart Sync](https://cadzow.com.au/Cadzow/details.aspx?ID=2831)
- [SyncBack: OneDrive Files On-Demand](https://help.2brightsparks.com/support/solutions/articles/43000336216)
- [FreeFileSync forum: OneDrive files missing from compare](https://freefilesync.org/forum/viewtopic.php?p=37825)
- [FreeFileSync expert settings (time tolerance)](https://www.freefilesync.org/manual/html/expert-settings.html)
- [FreeFileSync forum: high-latency scanning](https://freefilesync.org/forum/viewtopic.php?p=47285)
- [FreeFileSync Donation Edition features](https://freefilesync.org/donate.php)
- [Synology: What is On-demand Sync](https://kb.synology.com/en-my/DSM/tutorial/What_is_On-demand_Sync)
- [Synology Drive desktop (Cloud Files API / File Provider)](https://xaweho.de/en/wissen/storagedrive/synology-drive-desktop)
- [RcloneView announcement](https://forum.rclone.org/t/introducing-rcloneview-a-gui-front-end-for-rclone/47710)
- [Rclone UI repo](https://github.com/rclone-ui/rclone-ui)
- [RClone Manager announcement](https://forum.rclone.org/t/rclone-manager-a-new-cross-platform-user-frienly-gui/53051)

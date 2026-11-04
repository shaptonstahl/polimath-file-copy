//! Shared data types passed between the planner, the executors and the UI.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// What to do when a file with the same name already exists at the target.
/// Named from the *target's* point of view, like most copy utilities:
/// "replace older" means "overwrite the target if the target is older".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverwriteRule {
    Never,
    Always,
    ReplaceOlder,
    ReplaceNewer,
    ReplaceSmaller,
    ReplaceLarger,
    /// Size differs, or modified time differs by more than the tolerance.
    ReplaceDifferent,
}

impl OverwriteRule {
    pub fn label(self) -> &'static str {
        match self {
            Self::Never => "never overwrite",
            Self::Always => "always overwrite",
            Self::ReplaceOlder => "replace older",
            Self::ReplaceNewer => "replace newer",
            Self::ReplaceSmaller => "replace smaller",
            Self::ReplaceLarger => "replace larger",
            Self::ReplaceDifferent => "replace if different",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    Copy,
    Move,
}

/// Metadata that can be read without touching file content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileMeta {
    pub size: u64,
    /// Milliseconds since the Unix epoch (may be negative).
    pub mtime_ms: i64,
    /// The file is a cloud placeholder (OneDrive / Dropbox / Synology Drive
    /// on-demand). Reading its content would trigger a download.
    pub cloud_only: bool,
}

#[derive(Debug, Clone)]
pub struct SourceFile {
    pub abs_path: PathBuf,
    /// Path relative to the target folder, e.g. `Photos/2024/a.jpg` when the
    /// folder `Photos` was dropped into the source list.
    pub rel_path: PathBuf,
    pub meta: FileMeta,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PlanOptions {
    pub rule: OverwriteRule,
    /// Modified times closer than this are treated as equal. Network shares,
    /// NAS volumes and cloud drives often store coarser timestamps than NTFS
    /// or APFS; without a tolerance every file looks changed.
    #[serde(default = "default_tolerance")]
    pub tolerance_ms: i64,
    /// Skip source files that are cloud-only placeholders instead of
    /// downloading them to copy.
    #[serde(default = "default_true")]
    pub skip_cloud_only: bool,
    /// When this many files land in one target folder, list the folder once
    /// instead of looking each file up individually.
    #[serde(default = "default_list_threshold")]
    pub list_threshold: usize,
}

fn default_tolerance() -> i64 {
    2000
}
fn default_true() -> bool {
    true
}
fn default_list_threshold() -> usize {
    8
}

impl Default for PlanOptions {
    fn default() -> Self {
        Self {
            rule: OverwriteRule::Never,
            tolerance_ms: default_tolerance(),
            skip_cloud_only: true,
            list_threshold: default_list_threshold(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Action {
    /// Nothing at the target; the file will be created.
    Create,
    /// The target exists and the rule says overwrite it.
    Overwrite,
    Skip { reason: String },
}

#[derive(Debug, Clone, Serialize)]
pub struct PlanItem {
    pub source: PathBuf,
    pub dest: PathBuf,
    pub rel: String,
    pub src: FileMeta,
    pub dst: Option<FileMeta>,
    pub action: Action,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct LookupStats {
    /// Target folders read with a single directory listing.
    pub dirs_listed: usize,
    /// Target files looked up one at a time.
    pub files_statted: usize,
    /// Target folders that don't exist yet (everything inside is new).
    pub dirs_missing: usize,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Plan {
    pub items: Vec<PlanItem>,
    pub errors: Vec<String>,
    pub lookup: LookupStats,
}

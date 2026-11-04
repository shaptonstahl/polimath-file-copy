//! Pure decision logic: given source and target metadata, what happens?

use crate::model::{Action, FileMeta, OverwriteRule, PlanOptions};

/// `cloud_ok`: a cloud-only source can be handled without downloading it
/// (a move by rename within its own sync root).
pub fn decide(src: &FileMeta, dst: Option<&FileMeta>, opts: &PlanOptions, cloud_ok: bool) -> Action {
    if src.cloud_only && opts.skip_cloud_only && !cloud_ok {
        return Action::Skip {
            reason: "source is cloud-only; copying would download it".into(),
        };
    }
    let Some(dst) = dst else {
        return Action::Create;
    };

    let tol = opts.tolerance_ms.max(0);
    let delta = src.mtime_ms - dst.mtime_ms; // > 0: source is newer
    let target_older = delta > tol;
    let target_newer = delta < -tol;
    let same_time = !target_older && !target_newer;

    let overwrite = match opts.rule {
        OverwriteRule::Never => false,
        OverwriteRule::Always => true,
        OverwriteRule::ReplaceOlder => target_older,
        OverwriteRule::ReplaceNewer => target_newer,
        OverwriteRule::ReplaceSmaller => dst.size < src.size,
        OverwriteRule::ReplaceLarger => dst.size > src.size,
        OverwriteRule::ReplaceDifferent => dst.size != src.size || !same_time,
    };

    if overwrite {
        Action::Overwrite
    } else if opts.rule == OverwriteRule::Never {
        Action::Skip {
            reason: "target exists".into(),
        }
    } else {
        Action::Skip {
            reason: format!("target exists; \"{}\" not met", opts.rule.label()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(size: u64, mtime_ms: i64) -> FileMeta {
        FileMeta { size, mtime_ms, cloud_only: false }
    }

    fn opts(rule: OverwriteRule) -> PlanOptions {
        PlanOptions { rule, ..Default::default() }
    }

    fn overwrites(rule: OverwriteRule, src: FileMeta, dst: FileMeta) -> bool {
        decide(&src, Some(&dst), &opts(rule), false) == Action::Overwrite
    }

    #[test]
    fn missing_target_is_created() {
        assert_eq!(decide(&meta(1, 0), None, &opts(OverwriteRule::Never), false), Action::Create);
    }

    #[test]
    fn never_and_always() {
        assert!(!overwrites(OverwriteRule::Never, meta(1, 10_000), meta(1, 0)));
        assert!(overwrites(OverwriteRule::Always, meta(1, 0), meta(1, 0)));
    }

    #[test]
    fn replace_older_respects_tolerance() {
        let src = meta(1, 10_000);
        assert!(overwrites(OverwriteRule::ReplaceOlder, src, meta(1, 7_000)));
        // 1.5 s apart is within the default 2 s tolerance -> same time.
        assert!(!overwrites(OverwriteRule::ReplaceOlder, src, meta(1, 8_500)));
        assert!(!overwrites(OverwriteRule::ReplaceOlder, src, meta(1, 20_000)));
    }

    #[test]
    fn replace_newer() {
        assert!(overwrites(OverwriteRule::ReplaceNewer, meta(1, 0), meta(1, 5_000)));
        assert!(!overwrites(OverwriteRule::ReplaceNewer, meta(1, 5_000), meta(1, 0)));
    }

    #[test]
    fn replace_smaller_and_larger() {
        assert!(overwrites(OverwriteRule::ReplaceSmaller, meta(10, 0), meta(5, 0)));
        assert!(!overwrites(OverwriteRule::ReplaceSmaller, meta(5, 0), meta(10, 0)));
        assert!(!overwrites(OverwriteRule::ReplaceSmaller, meta(5, 0), meta(5, 0)));
        assert!(overwrites(OverwriteRule::ReplaceLarger, meta(5, 0), meta(10, 0)));
    }

    #[test]
    fn replace_different() {
        assert!(!overwrites(OverwriteRule::ReplaceDifferent, meta(5, 0), meta(5, 1_000)));
        assert!(overwrites(OverwriteRule::ReplaceDifferent, meta(5, 0), meta(5, 3_000)));
        assert!(overwrites(OverwriteRule::ReplaceDifferent, meta(5, 0), meta(6, 0)));
    }

    #[test]
    fn cloud_only_source_skipped_unless_allowed() {
        let src = FileMeta { size: 1, mtime_ms: 0, cloud_only: true };
        assert!(matches!(decide(&src, None, &opts(OverwriteRule::Always), false), Action::Skip { .. }));
        let allow = PlanOptions { skip_cloud_only: false, ..opts(OverwriteRule::Always) };
        assert_eq!(decide(&src, None, &allow, false), Action::Create);
        // A rename within the sync root needs no download, so it isn't skipped.
        assert_eq!(decide(&src, None, &opts(OverwriteRule::Always), true), Action::Create);
    }

    #[test]
    fn cloud_only_target_does_not_block_overwrite() {
        // Overwriting a placeholder doesn't need its content.
        let dst = FileMeta { size: 1, mtime_ms: 0, cloud_only: true };
        assert!(overwrites(OverwriteRule::ReplaceOlder, meta(1, 10_000), dst));
    }
}

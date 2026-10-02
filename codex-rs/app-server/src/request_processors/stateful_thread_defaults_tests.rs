use codex_features::Feature;
use core_test_support::load_default_config_for_test;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::apply_stateful_thread_defaults;

#[tokio::test]
async fn stateful_threads_preserve_apply_patch_line_endings() {
    let codex_home = TempDir::new().expect("temporary home");
    let mut config = load_default_config_for_test(&codex_home).await;
    let before = config
        .features
        .enabled(Feature::ApplyPatchPreserveLineEndings);

    apply_stateful_thread_defaults(&mut config);

    assert_eq!(
        (
            before,
            config
                .features
                .enabled(Feature::ApplyPatchPreserveLineEndings)
        ),
        (false, true)
    );
}

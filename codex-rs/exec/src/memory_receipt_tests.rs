use codex_app_server_protocol::StatefulMemoryChangeTotals;
use pretty_assertions::assert_eq;

use super::lines;

#[test]
fn the_receipt_names_only_what_changed() {
    assert_eq!(
        (
            lines(Some(&StatefulMemoryChangeTotals::default()), /*run*/ None),
            lines(
                Some(&StatefulMemoryChangeTotals {
                    saved: 2,
                    corrected: 1,
                    commits_remembered: 1,
                    capture_incomplete: 2,
                    ..StatefulMemoryChangeTotals::default()
                }),
                /*run*/ None,
            ),
        ),
        (
            vec!["nothing was saved or changed this run".to_string()],
            vec![
                "this run: 2 saved, 1 corrected, 1 commit remembered from workspace history, 2 captures could not finish"
                    .to_string()
            ],
        )
    );
}

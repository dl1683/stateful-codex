use super::*;
use pretty_assertions::assert_eq;

const BLOCK: &str =
    "[stateful-outcome]\ndisposition: answer\nopen-issues: none\n[/stateful-outcome]";

#[test]
fn final_classification_shares_the_body_and_respects_explicit_intent() {
    for text in [
        "Task completed.".into(),
        format!("Done.\n{BLOCK}"),
        format!(
            "Done.\n{}",
            BLOCK.replace("open-issues: none", "open-issues:\n- tests remain")
        ),
    ] {
        assert!(
            matches!(final_reply(&text), FinalReply::StatusOnly(_)),
            "{text}"
        );
    }
    assert_eq!(
        final_reply("42"),
        FinalReply::Deliver {
            body: "42",
            issues: vec![]
        }
    );
    assert_eq!(
        final_reply(&format!(
            "42\n{}",
            BLOCK.replace("open-issues: none", "open-issues:\n- y")
        )),
        FinalReply::Deliver {
            body: "42",
            issues: vec!["y".into()]
        }
    );
    for disposition in ["continue", "blocked"] {
        for text in [
            format!(
                "Working.\n{}",
                BLOCK.replace(
                    "disposition: answer",
                    &format!("disposition: {disposition}")
                )
            ),
            format!("Working.\n[stateful-outcome]\n disposition: {disposition}\ninvalid"),
        ] {
            assert!(
                matches!(final_reply(&text), FinalReply::KeepRunning(_)),
                "{text}"
            );
        }
    }
    assert_eq!(
        final_reply("42\n[stateful-outcome]\ninvalid"),
        FinalReply::Deliver {
            body: "42",
            issues: vec![]
        }
    );
    assert_eq!(
        classify("42", ModelProgress::Recorded),
        FinalReply::KeepRunning("model recorded next work this turn")
    );
    assert_eq!(
        classify(&format!("42\n{BLOCK}"), ModelProgress::Recorded),
        FinalReply::Deliver {
            body: "42",
            issues: vec![]
        }
    );
    assert_eq!(
        classify("42\n[stateful-outcome]\ninvalid", ModelProgress::Recorded),
        FinalReply::KeepRunning("model recorded next work this turn")
    );
    assert_eq!(final_reply(BLOCK), FinalReply::StatusOnly(""));
}

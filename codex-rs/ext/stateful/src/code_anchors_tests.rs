use pretty_assertions::assert_eq;

use super::LiteralChange;
use super::contains_literal;
use super::literal_changes;
use super::may_state_old_value;

/// The horizon3 hand edit, as Git shows it (CRLF source lines included).
const YEARS_PATCH: &str = "diff --git a/src/humanize/_compact.py b/src/humanize/_compact.py
index d3a60f4..e788f73 100644
--- a/src/humanize/_compact.py
+++ b/src/humanize/_compact.py
@@ -5,7 +5,7 @@ from __future__ import annotations
 from enum import Enum\r
 \r
 _COMPACT_UNITS = {\r
-    \"YEARS\": \"y\",\r
+    \"YEARS\": \"yr\",\r
     \"MONTHS\": \"mth\",\r
     \"DAYS\": \"d\",\r
     \"HOURS\": \"h\",\r
";

fn years() -> LiteralChange {
    LiteralChange {
        path: "src/humanize/_compact.py".to_string(),
        key: "YEARS".to_string(),
        old: "y".to_string(),
        new: "yr".to_string(),
        siblings: ["mth", "d", "h"].map(str::to_string).to_vec(),
        subject: ["compact", "units"].map(str::to_string).to_vec(),
    }
}

#[test]
fn a_rebound_literal_is_read_with_its_key_mapping_and_subject() {
    assert_eq!(literal_changes(YEARS_PATCH), (vec![years()], true));
}

#[test]
fn only_a_single_rebound_value_is_a_change() {
    let patch = "diff --git a/a.py b/a.py
--- a/a.py
+++ b/a.py
@@ -1,4 +1,4 @@
-LABEL = \"y\"
+LABEL = \"yr\"  # longer
-pair = (\"a\", \"b\")
+pair = (\"c\", \"d\")
-print(\"y\")
+print(\"yr\")
 rest = 1
";
    assert_eq!(
        literal_changes(patch).0,
        vec![LiteralChange {
            path: "a.py".to_string(),
            key: "LABEL".to_string(),
            old: "y".to_string(),
            new: "yr".to_string(),
            siblings: ["a", "b", "c", "d"].map(str::to_string).to_vec(),
            subject: Vec::new(),
        }]
    );
}

#[test]
fn quoted_and_tab_terminated_patch_paths_are_read() {
    let patch = |header: &str| {
        format!("diff --git a/x b/x\n--- a/x\n{header}\n@@ -1 +1 @@\n-K = \"a\"\n+K = \"b\"\n")
    };
    let path = |header: &str| {
        literal_changes(&patch(header))
            .0
            .first()
            .map(|change| change.path.clone())
    };
    assert_eq!(
        (
            path("+++ b/space name.py\t"),
            path("+++ \"b/tab\\tname.py\""),
            path("+++ /dev/null"),
        ),
        (
            Some("space name.py".to_string()),
            Some("tab\tname.py".to_string()),
            None,
        )
    );
}

#[test]
fn literals_are_whole_words() {
    assert!(contains_literal(
        "Final compact duration unit symbols are y, mth, d, h.",
        "y"
    ));
    assert!(contains_literal("The years symbol is `y`.", "y"));
    assert!(!contains_literal("Every year counts.", "y"));
    assert!(!contains_literal("It is the user's choice.", "s"));
    assert!(!contains_literal("It is the user\u{2019}s choice.", "s"));
    assert!(!contains_literal("They don't.", "don"));
}

#[test]
fn only_prose_that_may_state_the_old_value_is_judged() {
    let change = years();
    let cases = [
        (
            "Final compact duration unit symbols are y, mth, d, h, m, s, ms, and us.",
            true,
        ),
        (
            "We intend to keep the compact duration symbols y, mth, d even if the implementation changes.",
            true,
        ),
        ("The YEARS label in _compact.py is y.", true),
        ("Chart axes y, d, h.", false),
        ("In another package, axis y has labels d and h.", false),
        ("Axis y is the vertical one.", false),
        (
            "Compact years now use yr, after y was ambiguous next to mth and d.",
            false,
        ),
    ];
    for (content, expected) in cases {
        assert_eq!(may_state_old_value(content, &change), expected, "{content}");
    }
}

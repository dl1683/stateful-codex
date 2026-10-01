use std::sync::Arc;

use codex_project_intelligence::EvidenceLineRange;
use codex_project_intelligence::EvidenceReadLocator;
use codex_project_intelligence::ProjectIndexRequest;
use codex_project_intelligence::ProjectIndexer;
use codex_project_intelligence::ProjectRelativePath;
use codex_state::SqliteConfig;
use codex_thread_store::InMemoryThreadStore;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::TempDir;

use super::EvidenceExtent;
use super::EvidenceReadTool;
use super::NumberedEvidence;
use super::fits_response;
use super::pack_evidence;
use super::source_lines;
use crate::services::ProjectIntelligenceServices;

#[tokio::test]
async fn changed_source_is_incrementally_refreshed_and_reread_once() {
    let state_home = TempDir::new().expect("temporary state home");
    let project_root = TempDir::new().expect("temporary project root");
    let source_path = project_root.path().join("policy.md");
    std::fs::write(&source_path, "# Policy\nThreshold: 10\n").expect("write source");
    let services =
        ProjectIntelligenceServices::new(SqliteConfig::new_for_testing(state_home.path().abs()));
    ProjectIndexer::new(
        services.hierarchy().await.expect("hierarchy").clone(),
        services.context_map().await.expect("context map").clone(),
    )
    .refresh(ProjectIndexRequest {
        project_id: "project-1".to_string(),
        roots: vec![project_root.path().to_path_buf()],
    })
    .await
    .expect("index source");
    std::fs::write(&source_path, "# Policy\nThreshold: 60\n").expect("change source");
    let tool = EvidenceReadTool::new(
        "project-1".to_string(),
        "thread-1".to_string(),
        services,
        Arc::new(InMemoryThreadStore::default()),
    );
    let relative_path = ProjectRelativePath::parse("policy.md").expect("relative path");

    let (refreshed, source_refreshed) = tool
        .read_with_refresh(
            vec![project_root.path().to_path_buf()],
            EvidenceReadLocator::Source {
                project_root: None,
                relative_path: relative_path.clone(),
                line_range: Some(EvidenceLineRange { start: 2, end: 2 }),
            },
            1024,
        )
        .await
        .expect("refresh and reread source");
    assert!(source_refreshed);
    assert_eq!(refreshed.content, "Threshold: 60\n");

    let (unchanged, source_refreshed) = tool
        .read_with_refresh(
            vec![project_root.path().to_path_buf()],
            EvidenceReadLocator::Source {
                project_root: None,
                relative_path,
                line_range: Some(EvidenceLineRange { start: 2, end: 2 }),
            },
            1024,
        )
        .await
        .expect("reuse refreshed route");
    assert!(!source_refreshed);
    assert_eq!(unchanged.content, "Threshold: 60\n");
}

/// A response shaped like the tool's own: labelled content, counters, and a
/// receipt-sized field when the evidence is receipt-eligible.
fn response(evidence: &NumberedEvidence) -> serde_json::Value {
    json!({
        "content": evidence.content,
        "bytesReturned": evidence.source_bytes,
        "firstLine": evidence.first_line,
        "lastLine": evidence.last_line,
        "lastLinePartial": evidence.last_line_partial,
        "truncated": evidence.extent == EvidenceExtent::Truncated,
        "blackboardEvidence": evidence.receipt_range().map(|_| "r".repeat(78)),
    })
}

/// Packs `content` as the tool does into `budget` serialized response bytes.
fn pack(
    content: &str,
    first_line: Option<u64>,
    read_extent: EvidenceExtent,
    budget: usize,
) -> Option<NumberedEvidence> {
    let lines = source_lines(content, first_line, read_extent);
    pack_evidence(&lines, read_extent, |evidence| {
        fits_response(&response(evidence), budget)
    })
}

fn evidence(
    content: &str,
    source_bytes: usize,
    lines: Option<(u64, u64)>,
    last_line_partial: bool,
    extent: EvidenceExtent,
) -> NumberedEvidence {
    NumberedEvidence {
        content: content.to_string(),
        source_bytes,
        first_line: lines.map(|(first, _)| first),
        last_line: lines.map(|(_, last)| last),
        last_line_partial,
        extent,
    }
}

#[test]
fn complete_read_numbers_every_line_and_preserves_blank_lines() {
    let source = "\"\"\"Amount parsing used by the importers.\"\"\"\n\n\ndef parse_amount(text):\n";
    let packed = pack(source, Some(1), EvidenceExtent::Complete, 9_000).expect("fits");

    assert_eq!(
        packed,
        evidence(
            "L1: \"\"\"Amount parsing used by the importers.\"\"\"\nL2: \nL3: \nL4: def parse_amount(text):\n",
            source.len(),
            Some((1, 4)),
            /*last_line_partial*/ false,
            EvidenceExtent::Complete,
        )
    );
    assert_eq!(
        packed.receipt_range(),
        Some(EvidenceLineRange { start: 1, end: 4 })
    );
}

#[test]
fn region_reads_keep_absolute_line_numbers() {
    let packed = pack(
        "line 65\nline 66\n",
        Some(65),
        EvidenceExtent::Complete,
        9_000,
    );

    assert_eq!(
        packed,
        Some(evidence(
            "L65: line 65\nL66: line 66\n",
            16,
            Some((65, 66)),
            /*last_line_partial*/ false,
            EvidenceExtent::Complete,
        ))
    );
}

#[test]
fn crlf_and_missing_final_newline_do_not_invent_lines() {
    let packed = pack("first\r\nlast", Some(1), EvidenceExtent::Complete, 9_000);

    assert_eq!(
        packed,
        Some(evidence(
            "L1: first\r\nL2: last",
            11,
            Some((1, 2)),
            /*last_line_partial*/ false,
            EvidenceExtent::Complete,
        ))
    );
}

#[test]
fn empty_read_has_no_lines_and_no_receipt() {
    let packed = pack("", None, EvidenceExtent::Complete, 9_000).expect("fits");

    assert_eq!(
        packed,
        evidence(
            "",
            0,
            None,
            /*last_line_partial*/ false,
            EvidenceExtent::Complete
        )
    );
    assert_eq!(packed.receipt_range(), None);
}

#[test]
fn source_byte_truncation_cuts_between_lines() {
    let packed = pack("one\ntwo\nthr", Some(3), EvidenceExtent::Truncated, 9_000).expect("fits");

    assert_eq!(
        packed,
        evidence(
            "L3: one\nL4: two\n",
            8,
            Some((3, 4)),
            /*last_line_partial*/ false,
            EvidenceExtent::Truncated,
        )
    );
    assert_eq!(packed.receipt_range(), None);
}

#[test]
fn an_oversized_line_returns_a_marked_partial_prefix() {
    let packed = pack("abcdef", Some(7), EvidenceExtent::Truncated, 9_000);

    assert_eq!(
        packed,
        Some(evidence(
            "L7: abcdef",
            6,
            Some((7, 7)),
            /*last_line_partial*/ true,
            EvidenceExtent::Truncated,
        ))
    );
}

#[test]
fn response_budget_truncation_keeps_counters_truthful_and_withholds_receipt() {
    let expected = evidence(
        "L1: alpha\nL2: beta\n",
        11,
        Some((1, 2)),
        /*last_line_partial*/ false,
        EvidenceExtent::Truncated,
    );
    let budget = response(&expected).to_string().len();

    let packed = pack(
        "alpha\nbeta\ngamma\ndelta\n",
        Some(1),
        EvidenceExtent::Complete,
        budget,
    )
    .expect("fits");

    assert_eq!(packed, expected);
    assert_eq!(packed.receipt_range(), None);
}

#[test]
fn escape_heavy_oversized_line_is_cut_on_a_character_boundary() {
    let source = format!("{}\nnext\n", "\"\\é".repeat(400));
    let budget = 600;

    let packed = pack(&source, Some(1), EvidenceExtent::Complete, budget).expect("fits");

    let prefix = packed
        .content
        .strip_prefix("L1: ")
        .expect("labelled first line");
    assert!(source.starts_with(prefix));
    assert!(!prefix.is_empty());
    assert_eq!(
        packed,
        evidence(
            &packed.content,
            prefix.len(),
            Some((1, 1)),
            /*last_line_partial*/ true,
            EvidenceExtent::Truncated,
        )
    );
}

#[test]
fn metadata_that_cannot_fit_returns_nothing() {
    assert_eq!(pack("one\n", Some(1), EvidenceExtent::Complete, 10), None);
}

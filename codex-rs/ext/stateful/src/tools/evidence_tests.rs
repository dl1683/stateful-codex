use std::sync::Arc;

use codex_project_intelligence::EvidenceLineRange;
use codex_project_intelligence::EvidenceReadLocator;
use codex_project_intelligence::EvidenceRoute;
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
use super::is_route_item_wrapper;
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

#[tokio::test]
async fn failed_guarded_route_explains_recovery_without_refreshing() {
    let state_home = TempDir::new().expect("temporary state home");
    let project_root = TempDir::new().expect("temporary project root");
    let source_path = project_root.path().join("policy.md");
    std::fs::write(
        &source_path,
        "# Policy
Threshold: 10
",
    )
    .expect("write source");
    let services =
        ProjectIntelligenceServices::new(SqliteConfig::new_for_testing(state_home.path().abs()));
    let context_map = services.context_map().await.expect("context map").clone();
    ProjectIndexer::new(
        services.hierarchy().await.expect("hierarchy").clone(),
        context_map.clone(),
    )
    .refresh(ProjectIndexRequest {
        project_id: "project-1".to_string(),
        roots: vec![project_root.path().to_path_buf()],
    })
    .await
    .expect("index source");
    let relative_path = ProjectRelativePath::parse("policy.md").expect("relative path");
    let hit = context_map
        .file_hits_for_path("project-1", &relative_path)
        .await
        .expect("source lookup")
        .into_iter()
        .next()
        .expect("indexed source");
    let route = EvidenceRoute::from_hit(&hit).expect("file route");
    std::fs::write(
        &source_path,
        "# Policy
Threshold: 60
",
    )
    .expect("change source");
    let tool = EvidenceReadTool::new(
        "project-1".to_string(),
        "thread-1".to_string(),
        services,
        Arc::new(InMemoryThreadStore::default()),
    );
    let roots = vec![project_root.path().to_path_buf()];

    let guarded = tool
        .read_with_refresh(
            roots.clone(),
            EvidenceReadLocator::ContextMapRoute(route),
            /*max_bytes*/ 1024,
        )
        .await
        .map(|(read, _)| read.content)
        .map_err(|error| error.to_string());
    let (path_read, source_refreshed) = tool
        .read_with_refresh(
            roots,
            EvidenceReadLocator::Source {
                project_root: None,
                relative_path,
                line_range: Some(EvidenceLineRange { start: 2, end: 2 }),
            },
            /*max_bytes*/ 1024,
        )
        .await
        .expect("path read refreshes the changed file");

    assert_eq!(
        (guarded, path_read.content, source_refreshed),
        (
            Err("sourceChanged: source bytes read do not match the indexed fingerprint; the source changed after indexing. Read it again by relativePath; query again if you need the corresponding region.".to_string()),
            "Threshold: 60
".to_string(),
            true,
        )
    );
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

fn response_bytes(evidence: &NumberedEvidence) -> usize {
    response(evidence).to_string().len()
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
        response_bytes(evidence) <= budget
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
    let packed = pack(
        source,
        /*first_line*/ Some(1),
        EvidenceExtent::Complete,
        /*budget*/ 9_000,
    )
    .expect("fits");

    assert_eq!(
        packed,
        evidence(
            "L1: \"\"\"Amount parsing used by the importers.\"\"\"\nL2: \nL3: \nL4: def parse_amount(text):\n",
            source.len(),
            /*lines*/ Some((1, 4)),
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
        /*first_line*/ Some(65),
        EvidenceExtent::Complete,
        /*budget*/ 9_000,
    );

    assert_eq!(
        packed,
        Some(evidence(
            "L65: line 65\nL66: line 66\n",
            /*source_bytes*/ 16,
            /*lines*/ Some((65, 66)),
            /*last_line_partial*/ false,
            EvidenceExtent::Complete,
        ))
    );
}

#[test]
fn crlf_and_missing_final_newline_do_not_invent_lines() {
    let packed = pack(
        "first\r\nlast",
        /*first_line*/ Some(1),
        EvidenceExtent::Complete,
        /*budget*/ 9_000,
    );

    assert_eq!(
        packed,
        Some(evidence(
            "L1: first\r\nL2: last",
            /*source_bytes*/ 11,
            /*lines*/ Some((1, 2)),
            /*last_line_partial*/ false,
            EvidenceExtent::Complete,
        ))
    );
}

#[test]
fn empty_read_has_no_lines_and_no_receipt() {
    let packed = pack(
        "",
        /*first_line*/ None,
        EvidenceExtent::Complete,
        /*budget*/ 9_000,
    )
    .expect("fits");

    assert_eq!(
        packed,
        evidence(
            "",
            /*source_bytes*/ 0,
            /*lines*/ None,
            /*last_line_partial*/ false,
            EvidenceExtent::Complete,
        )
    );
    assert_eq!(packed.receipt_range(), None);
}

#[test]
fn source_byte_truncation_cuts_between_lines() {
    let packed = pack(
        "one\ntwo\nthr",
        /*first_line*/ Some(3),
        EvidenceExtent::Truncated,
        /*budget*/ 9_000,
    )
    .expect("fits");

    assert_eq!(
        packed,
        evidence(
            "L3: one\nL4: two\n",
            /*source_bytes*/ 8,
            /*lines*/ Some((3, 4)),
            /*last_line_partial*/ false,
            EvidenceExtent::Truncated,
        )
    );
    assert_eq!(packed.receipt_range(), None);
}

#[test]
fn an_oversized_line_returns_a_marked_partial_prefix() {
    let packed = pack(
        "abcdef",
        /*first_line*/ Some(7),
        EvidenceExtent::Truncated,
        /*budget*/ 9_000,
    );

    assert_eq!(
        packed,
        Some(evidence(
            "L7: abcdef",
            /*source_bytes*/ 6,
            /*lines*/ Some((7, 7)),
            /*last_line_partial*/ true,
            EvidenceExtent::Truncated,
        ))
    );
}

#[test]
fn response_budget_truncation_keeps_counters_truthful_and_withholds_receipt() {
    let expected = evidence(
        "L1: alpha\nL2: beta\n",
        /*source_bytes*/ 11,
        /*lines*/ Some((1, 2)),
        /*last_line_partial*/ false,
        EvidenceExtent::Truncated,
    );

    let packed = pack(
        "alpha\nbeta\ngamma\ndelta\n",
        /*first_line*/ Some(1),
        EvidenceExtent::Complete,
        response_bytes(&expected),
    )
    .expect("fits");

    assert_eq!(packed, expected);
    assert_eq!(packed.receipt_range(), None);
}

#[test]
fn escape_heavy_oversized_line_is_cut_on_a_character_boundary() {
    let source = format!("{}\nnext\n", "\"\\é".repeat(400));
    let budget = 600;

    let packed = pack(
        &source,
        /*first_line*/ Some(1),
        EvidenceExtent::Complete,
        budget,
    )
    .expect("fits");

    assert!(response_bytes(&packed) <= budget);
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
            /*lines*/ Some((1, 1)),
            /*last_line_partial*/ true,
            EvidenceExtent::Truncated,
        )
    );
}

#[test]
fn a_short_partial_line_is_returned_when_empty_metadata_would_not_fit() {
    let partial = evidence(
        "L1: a",
        /*source_bytes*/ 1,
        /*lines*/ Some((1, 1)),
        /*last_line_partial*/ true,
        EvidenceExtent::Truncated,
    );
    let empty = evidence(
        "",
        /*source_bytes*/ 0,
        /*lines*/ None,
        /*last_line_partial*/ false,
        EvidenceExtent::Truncated,
    );
    let budget = response_bytes(&partial);
    assert!(response_bytes(&empty) > budget);

    let packed = pack(
        "ab\n",
        /*first_line*/ Some(1),
        EvidenceExtent::Complete,
        budget,
    );

    assert_eq!(packed, Some(partial));
}

#[test]
fn metadata_that_cannot_fit_returns_nothing() {
    assert_eq!(
        pack(
            "one\n",
            /*first_line*/ Some(1),
            EvidenceExtent::Complete,
            /*budget*/ 10,
        ),
        None
    );
}

#[test]
fn a_cut_complete_line_never_returns_its_last_character() {
    let partial = evidence(
        "L1: ab",
        /*source_bytes*/ 2,
        /*lines*/ Some((1, 1)),
        /*last_line_partial*/ true,
        EvidenceExtent::Truncated,
    );
    let complete = evidence(
        "L1: abé",
        /*source_bytes*/ 4,
        /*lines*/ Some((1, 1)),
        /*last_line_partial*/ false,
        EvidenceExtent::Complete,
    );
    assert!(response_bytes(&complete) > response_bytes(&partial) + 2);

    let packed = pack(
        "abé",
        /*first_line*/ Some(1),
        EvidenceExtent::Complete,
        response_bytes(&complete) - 1,
    );

    assert_eq!(packed, Some(partial));
}

#[test]
fn only_route_item_wrappers_get_the_wrapper_diagnostic() {
    let route =
        json!({"contextMapEntryId": "map-1", "sourceFingerprint": "sha256:00", "lineRange": null});
    let cases = [
        (
            "named wrapper",
            json!({"name": "cli", "evidenceRoute": route}),
        ),
        (
            "whole item",
            json!({"headline": "CLI", "source": {"relativePath": "cli.py"}, "evidenceRoute": route}),
        ),
        (
            "nested item",
            json!({"evidenceRoute": {"headline": "CLI", "evidenceRoute": route}}),
        ),
        (
            "nested item, outer extra",
            json!({"evidenceRoute": {"headline": "CLI", "evidenceRoute": route}, "extra": 1}),
        ),
        (
            "nested item, inner extra",
            json!({"evidenceRoute": {"extra": 1, "evidenceRoute": route}}),
        ),
        (
            "wrapper, route extra",
            json!({"name": "cli", "evidenceRoute": {"contextMapEntryId": "map-1", "sourceFingerprint": "sha256:00", "lineRange": null, "extra": 1}}),
        ),
        (
            "nested item, range extra",
            json!({"evidenceRoute": {"headline": "CLI", "evidenceRoute": {"contextMapEntryId": "map-1", "sourceFingerprint": "sha256:00", "lineRange": {"start": 1, "end": 2, "extra": 1}}}}),
        ),
        ("route only", json!({"evidenceRoute": route})),
        (
            "unrelated field",
            json!({"evidenceRoute": route, "extra": 1}),
        ),
        (
            "mixed fields",
            json!({"name": "cli", "evidenceRoute": route, "extra": 1}),
        ),
        (
            "path read",
            json!({"relativePath": "cli.py", "name": "cli"}),
        ),
    ];
    let observed = cases
        .iter()
        .map(|(case, arguments)| (*case, is_route_item_wrapper(&arguments.to_string())))
        .collect::<Vec<_>>();
    assert_eq!(
        observed,
        vec![
            ("named wrapper", true),
            ("whole item", true),
            ("nested item", true),
            ("nested item, outer extra", false),
            ("nested item, inner extra", false),
            ("wrapper, route extra", false),
            ("nested item, range extra", false),
            ("route only", false),
            ("unrelated field", false),
            ("mixed fields", false),
            ("path read", false),
        ]
    );
}

fn source_tool(state_home: &TempDir) -> EvidenceReadTool {
    EvidenceReadTool::new(
        "project-1".to_string(),
        "thread-1".to_string(),
        ProjectIntelligenceServices::new(SqliteConfig::new_for_testing(state_home.path().abs())),
        Arc::new(InMemoryThreadStore::default()),
    )
}

fn whole_file(path: &str) -> EvidenceReadLocator {
    EvidenceReadLocator::Source {
        project_root: None,
        relative_path: ProjectRelativePath::parse(path).expect("relative path"),
        line_range: None,
    }
}

fn model_error(error: codex_extension_api::FunctionCallError) -> String {
    match error {
        codex_extension_api::FunctionCallError::RespondToModel(message) => message,
        other => panic!("expected a model-facing error, got {other:?}"),
    }
}

/// debug2: parallel first reads of a never-indexed project all failed with "not indexed
/// even after indexing on demand". Each explicit read now indexes just its own file.
#[tokio::test]
async fn concurrent_first_reads_of_a_never_indexed_project_all_succeed() {
    let state_home = TempDir::new().expect("temporary state home");
    let project_root = TempDir::new().expect("temporary project root");
    std::fs::create_dir_all(project_root.path().join("shipit")).expect("package dir");
    for (path, text) in [
        (
            "shipit/cli.py",
            "import click
",
        ),
        (
            "shipit/types.py",
            "class ConfigPath: ...
",
        ),
        (
            "README.md",
            "# shipit
",
        ),
    ] {
        std::fs::write(project_root.path().join(path), text).expect("write source");
    }
    let tool = source_tool(&state_home);
    let roots = vec![project_root.path().to_path_buf()];

    let (cli, types, readme) = tokio::join!(
        tool.read_with_refresh(roots.clone(), whole_file("shipit/cli.py"), 1024),
        tool.read_with_refresh(roots.clone(), whole_file("shipit/types.py"), 1024),
        tool.read_with_refresh(roots.clone(), whole_file("README.md"), 1024),
    );

    assert_eq!(
        [cli, types, readme].map(|read| read
            .map(|(read, refreshed)| (read.content, refreshed))
            .map_err(model_error)),
        [
            Ok((
                "import click
"
                .to_string(),
                true
            )),
            Ok((
                "class ConfigPath: ...
"
                .to_string(),
                true
            )),
            Ok((
                "# shipit
"
                .to_string(),
                true
            )),
        ]
    );
}

/// A file added after the project was indexed, and a git-ignored vendored file that the
/// corpus scan excludes, are both readable by explicit path.
#[tokio::test]
async fn new_and_ignored_files_in_a_built_project_are_read_by_path() {
    let state_home = TempDir::new().expect("temporary state home");
    let project_root = TempDir::new().expect("temporary project root");
    std::fs::create_dir_all(project_root.path().join(".git")).expect("git marker");
    std::fs::write(
        project_root.path().join(".gitignore"),
        "vendor/
",
    )
    .expect("ignore");
    std::fs::create_dir_all(project_root.path().join("vendor/click")).expect("vendor dir");
    std::fs::write(
        project_root.path().join("vendor/click/core.py"),
        "def main(): ...
",
    )
    .expect("vendored source");
    std::fs::write(
        project_root.path().join("app.py"),
        "print('app')
",
    )
    .expect("source");
    let tool = source_tool(&state_home);
    ProjectIndexer::new(
        tool.services.hierarchy().await.expect("hierarchy").clone(),
        tool.services
            .context_map()
            .await
            .expect("context map")
            .clone(),
    )
    .refresh(ProjectIndexRequest {
        project_id: "project-1".to_string(),
        roots: vec![project_root.path().to_path_buf()],
    })
    .await
    .expect("index project");
    std::fs::write(
        project_root.path().join("added.py"),
        "ADDED = 1
",
    )
    .expect("new file");
    let roots = vec![project_root.path().to_path_buf()];

    let added = tool
        .read_with_refresh(roots.clone(), whole_file("added.py"), 1024)
        .await
        .map(|(read, refreshed)| (read.content, refreshed))
        .map_err(model_error);
    let vendored = tool
        .read_with_refresh(roots, whole_file("vendor/click/core.py"), 1024)
        .await
        .map(|(read, refreshed)| (read.content, refreshed))
        .map_err(model_error);

    assert_eq!(
        (added, vendored),
        (
            Ok((
                "ADDED = 1
"
                .to_string(),
                true
            )),
            Ok((
                "def main(): ...
"
                .to_string(),
                true
            )),
        )
    );
}

/// Missing paths and paths present under several roots get precise statuses.
#[tokio::test]
async fn missing_and_ambiguous_paths_report_precise_statuses() {
    let state_home = TempDir::new().expect("temporary state home");
    let first = TempDir::new().expect("first root");
    let second = TempDir::new().expect("second root");
    std::fs::write(
        first.path().join("shared.md"),
        "first
",
    )
    .expect("first copy");
    std::fs::write(
        second.path().join("shared.md"),
        "second
",
    )
    .expect("second copy");
    let tool = source_tool(&state_home);
    let roots = vec![first.path().to_path_buf(), second.path().to_path_buf()];

    let missing = tool
        .read_with_refresh(roots.clone(), whole_file("absent.md"), 1024)
        .await
        .map(|_| ())
        .map_err(model_error)
        .expect_err("absent file");
    let single_root_missing = tool
        .read_with_refresh(
            vec![first.path().to_path_buf()],
            whole_file("absent.md"),
            1024,
        )
        .await
        .map(|_| ())
        .map_err(model_error)
        .expect_err("absent file in one root");
    let ambiguous = tool
        .read_with_refresh(roots.clone(), whole_file("shared.md"), 1024)
        .await
        .map(|_| ())
        .map_err(model_error)
        .expect_err("ambiguous path");
    let chosen = tool
        .read_with_refresh(
            roots,
            EvidenceReadLocator::Source {
                project_root: Some(second.path().to_path_buf()),
                relative_path: ProjectRelativePath::parse("shared.md").expect("path"),
                line_range: None,
            },
            1024,
        )
        .await
        .map(|(read, _)| read.content)
        .map_err(model_error);

    assert_eq!(
        (
            missing.split(':').next(),
            single_root_missing.split(':').next(),
            ambiguous.split(':').next(),
            chosen,
        ),
        (
            Some("notFound"),
            Some("notFound"),
            Some("ambiguousRoot"),
            Ok("second
"
            .to_string()),
        )
    );
}

/// A deleted source with an index row is reported missing, not read from stale bytes.
#[tokio::test]
async fn a_deleted_indexed_file_is_reported_not_found() {
    let state_home = TempDir::new().expect("temporary state home");
    let project_root = TempDir::new().expect("temporary project root");
    let source = project_root.path().join("gone.md");
    std::fs::write(
        &source,
        "soon gone
",
    )
    .expect("write source");
    let tool = source_tool(&state_home);
    let roots = vec![project_root.path().to_path_buf()];
    tool.read_with_refresh(roots.clone(), whole_file("gone.md"), 1024)
        .await
        .map_err(model_error)
        .expect("first read indexes the file");
    std::fs::remove_file(&source).expect("delete source");

    let error = tool
        .read_with_refresh(roots, whole_file("gone.md"), 1024)
        .await
        .map(|_| ())
        .map_err(model_error)
        .expect_err("deleted file");

    assert_eq!(error.split(':').next(), Some("notFound"));
}

/// The root is chosen from the filesystem before the index is consulted: a path present
/// under two roots is ambiguous even when only one of them is indexed, and a path indexed
/// under both but now present under one resolves to that root.
#[tokio::test]
async fn root_choice_uses_the_filesystem_not_partial_or_stale_index_rows() {
    let state_home = TempDir::new().expect("temporary state home");
    let first = TempDir::new().expect("first root");
    let second = TempDir::new().expect("second root");
    std::fs::write(first.path().join("x.py"), "FIRST = 1\n").expect("first copy");
    std::fs::write(second.path().join("x.py"), "SECOND = 2\n").expect("second copy");
    let tool = source_tool(&state_home);
    ProjectIndexer::new(
        tool.services.hierarchy().await.expect("hierarchy").clone(),
        tool.services
            .context_map()
            .await
            .expect("context map")
            .clone(),
    )
    .refresh(ProjectIndexRequest {
        project_id: "project-1".to_string(),
        roots: vec![first.path().to_path_buf()],
    })
    .await
    .expect("index only the first root");
    let roots = vec![first.path().to_path_buf(), second.path().to_path_buf()];

    let partially_indexed = tool
        .read_with_refresh(roots.clone(), whole_file("x.py"), 1024)
        .await
        .map(|_| ())
        .map_err(model_error)
        .expect_err("present under both roots");
    std::fs::remove_file(first.path().join("x.py")).expect("remove first copy");
    let now_unique = tool
        .read_with_refresh(roots, whole_file("x.py"), 1024)
        .await
        .map(|(read, _)| read.content)
        .map_err(model_error);

    assert_eq!(
        (partially_indexed.split(':').next(), now_unique),
        (Some("ambiguousRoot"), Ok("SECOND = 2\n".to_string()))
    );
}

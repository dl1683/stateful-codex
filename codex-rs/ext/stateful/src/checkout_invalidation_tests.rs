use std::path::Path;
use std::time::Duration;

use codex_git_utils::GitCommitSummary;
use codex_git_utils::GitObservationBudget;
use codex_project_intelligence::BlackboardEntry;
use codex_project_intelligence::BlackboardEntryId;
use codex_project_intelligence::BlackboardEntryState;
use codex_project_intelligence::BlackboardEntryUpdate;
use codex_project_intelligence::BlackboardImportance;
use codex_project_intelligence::BlackboardKind;
use codex_project_intelligence::BlackboardProvenance;
use codex_project_intelligence::BlackboardProvenanceKind;
use codex_project_intelligence::BlackboardStore;
use codex_project_intelligence::BlackboardVerification;
use codex_project_intelligence::ConfidenceScore;
use codex_project_intelligence::KnowledgeValidity;
use codex_project_intelligence::NewBlackboardEntry;
use codex_project_intelligence::RootBlackboardQuery;
use codex_project_intelligence::RootPromotion;
use codex_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;
use tokio::time::Instant;

use super::AdvancedRoot;
use super::CommittedChange;
use super::Judge;
use super::Recorded;
use super::qualify_changed_conclusions;
use crate::code_anchors::literal_changes;
use crate::services::ProjectIntelligenceServices;

const PROJECT_ID: &str = "project-1";
const SYMBOLS: &str = "Final compact duration unit symbols are y, mth, d, h, m, s.";

fn git(cwd: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .args([
            "-c",
            "user.name=Codex",
            "-c",
            "user.email=codex@example.com",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.autocrlf=false",
        ])
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// Commits `src/_compact.py` with the years symbol `years`; returns the commit's summary.
fn set_years(repo: &Path, years: &str, message: &str) -> GitCommitSummary {
    std::fs::write(
        repo.join("src/_compact.py"),
        format!(
            "_COMPACT_UNITS = {{\n    \"YEARS\": \"{years}\",\n    \"MONTHS\": \"mth\",\n    \"DAYS\": \"d\",\n    \"HOURS\": \"h\",\n}}\n"
        ),
    )
    .expect("write source");
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-q", "-m", message]);
    GitCommitSummary {
        oid: git(repo, &["rev-parse", "HEAD"]),
        committed_at: 0,
        subject: message.to_string(),
        body: String::new(),
    }
}

fn repo() -> tempfile::TempDir {
    let repo = tempfile::TempDir::new().expect("repo");
    git(repo.path(), &["init", "-q", "-b", "main"]);
    std::fs::create_dir(repo.path().join("src")).expect("src dir");
    set_years(repo.path(), "y", "Add compact units");
    repo
}

fn services(state_home: &tempfile::TempDir) -> ProjectIntelligenceServices {
    ProjectIntelligenceServices::new(codex_state::SqliteConfig::new_for_testing(
        codex_utils_absolute_path::test_support::PathExt::abs(state_home.path()),
    ))
}

async fn remember(
    services: &ProjectIntelligenceServices,
    id: &str,
    kind: BlackboardKind,
    provenance: BlackboardProvenanceKind,
    content: &str,
) -> BlackboardEntry {
    services
        .blackboard()
        .await
        .expect("store")
        .create_entry(
            BlackboardEntryId::parse(id).expect("ID"),
            NewBlackboardEntry {
                project_id: PROJECT_ID.to_string(),
                node_id: services.project_node_id(PROJECT_ID).await.expect("node"),
                kind,
                content: content.to_string(),
                structured_value: None,
                confidence: ConfidenceScore::from_basis_points(9_000).expect("confidence"),
                verification: BlackboardVerification::Unverified,
                importance: BlackboardImportance::Normal,
                root_promotion: RootPromotion::Promoted,
                evidence: Vec::new(),
                premises: Vec::new(),
                provenance: BlackboardProvenance {
                    kind: provenance,
                    source_id: format!("call-{id}"),
                },
            },
        )
        .await
        .expect("remembered")
}

async fn validity(store: &BlackboardStore, id: &str) -> Option<KnowledgeValidity> {
    store
        .knowledge_context(PROJECT_ID, &BlackboardEntryId::parse(id).expect("ID"))
        .await
        .expect("context")
        .map(|context| context.validity)
}

/// horizon3's y -> yr through the checkout lifecycle: the agent's symbol list and an
/// intention stating it need a check with the commit as the reason, and stay in the root
/// (qualified there) as readable knowledge; unrelated `y` prose and the user's own words are
/// untouched; nothing new is asserted about the code.
#[tokio::test]
async fn a_commit_qualifies_conclusions_that_may_state_the_old_value() {
    let state_home = tempfile::TempDir::new().expect("state home");
    let repo = repo();
    let services = services(&state_home);
    let roots = vec![repo.path().display().to_string()];
    remember(
        &services,
        "symbols",
        BlackboardKind::Fact,
        BlackboardProvenanceKind::Agent,
        SYMBOLS,
    )
    .await;
    remember(
        &services,
        "plan",
        BlackboardKind::Decision,
        BlackboardProvenanceKind::Agent,
        "Keep the compact symbols y, mth and d stable for dashboards.",
    )
    .await;
    remember(
        &services,
        "axis",
        BlackboardKind::Fact,
        BlackboardProvenanceKind::Agent,
        "The chart's y axis shows the count.",
    )
    .await;
    remember(
        &services,
        "user",
        BlackboardKind::Fact,
        BlackboardProvenanceKind::User,
        SYMBOLS,
    )
    .await;
    assert_eq!(
        crate::checkout::observe_turn_start(&services, PROJECT_ID, &roots, "turn-1").await,
        None
    );

    let commit = set_years(
        repo.path(),
        "yr",
        "Hand edit: compact years symbol 'y' -> 'yr'",
    );
    let report = crate::checkout::observe_turn_start(&services, PROJECT_ID, &roots, "turn-2")
        .await
        .expect("report")
        .0;
    let store = services.blackboard().await.expect("store");
    let mut validities = Vec::new();
    for id in ["symbols", "plan", "axis", "user"] {
        validities.push(validity(store, id).await);
    }
    let root = store
        .root_projection(RootBlackboardQuery {
            project_id: PROJECT_ID.to_string(),
            max_entries: 16,
        })
        .await
        .expect("root")
        .data
        .len();
    let symbols = store
        .get_entry(
            PROJECT_ID,
            &BlackboardEntryId::parse("symbols").expect("ID"),
        )
        .await
        .expect("read")
        .expect("kept");
    assert_eq!(
        (
            report.contains(&format!(
                "2 remembered conclusion(s) may be outdated: commit {} changed src/_compact.py YEARS from",
                &commit.oid[..8]
            )),
            validities,
            root,
            (symbols.value.content.as_str(), symbols.value.verification),
        ),
        (
            true,
            vec![
                Some(KnowledgeValidity::NeedsCheck),
                Some(KnowledgeValidity::NeedsCheck),
                None,
                None,
            ],
            4,
            (SYMBOLS, BlackboardVerification::Stale),
        )
    );
}

/// A pass out of time marks nothing and asks the caller to keep the baseline; the next pass
/// does the work; a later pass over the same commit only touches entries not yet marked.
#[tokio::test]
async fn an_unfinished_pass_resumes_without_repeating_finished_work() {
    let state_home = tempfile::TempDir::new().expect("state home");
    let repo = repo();
    let services = services(&state_home);
    remember(
        &services,
        "symbols",
        BlackboardKind::Fact,
        BlackboardProvenanceKind::Agent,
        SYMBOLS,
    )
    .await;
    let commit = set_years(repo.path(), "yr", "Use yr");
    let worktree_root = AbsolutePathBuf::from_absolute_path(repo.path()).expect("absolute");
    let commits = [commit];
    let advanced = AdvancedRoot {
        worktree_root: &worktree_root,
        commits: &commits,
        omitted: false,
    };
    let pass = |deadline: Instant| {
        let services = &services;
        let advanced = &advanced;
        async move {
            let budget = GitObservationBudget::until(Instant::now() + Duration::from_secs(2));
            qualify_changed_conclusions(services, PROJECT_ID, advanced, &budget, deadline).await
        }
    };
    let ample = || Instant::now() + Duration::from_secs(5);

    let expired = pass(Instant::now()).await;
    let store = services.blackboard().await.expect("store");
    let before = validity(store, "symbols").await;
    let done = pass(ample()).await;
    remember(
        &services,
        "later",
        BlackboardKind::Claim,
        BlackboardProvenanceKind::Agent,
        "In _compact.py the YEARS label is y.",
    )
    .await;
    let resumed = pass(ample()).await;
    let symbols = store
        .get_entry(
            PROJECT_ID,
            &BlackboardEntryId::parse("symbols").expect("ID"),
        )
        .await
        .expect("read")
        .expect("kept");
    assert_eq!(
        (
            (expired.retry, expired.lines.len(), before),
            (done.retry, done.lines.len()),
            (
                resumed.retry,
                resumed.lines.len(),
                resumed.lines[0].starts_with("1 ")
            ),
            symbols.revision,
            validity(store, "later").await,
        ),
        (
            (true, 0, None),
            (false, 1),
            (false, 1, true),
            2,
            Some(KnowledgeValidity::NeedsCheck),
        )
    );
}

/// An entry changed after it was judged is judged again from its new revision: still
/// stating the old value, it is marked; no longer stating it, it is left alone.
#[tokio::test]
async fn a_concurrently_changed_entry_is_judged_again() {
    let state_home = tempfile::TempDir::new().expect("state home");
    let services = services(&state_home);
    let still = remember(
        &services,
        "still",
        BlackboardKind::Fact,
        BlackboardProvenanceKind::Agent,
        SYMBOLS,
    )
    .await;
    let fixed = remember(
        &services,
        "fixed",
        BlackboardKind::Fact,
        BlackboardProvenanceKind::Agent,
        SYMBOLS,
    )
    .await;
    let store = services.blackboard().await.expect("store");
    let rewrite = |entry: &BlackboardEntry, content: &str| BlackboardEntryUpdate {
        expected_revision: entry.revision,
        kind: entry.value.kind,
        content: content.to_string(),
        structured_value: None,
        confidence: entry.value.confidence,
        verification: entry.value.verification,
        importance: entry.value.importance,
        root_promotion: entry.value.root_promotion,
        evidence: Vec::new(),
        premises: Vec::new(),
        state: BlackboardEntryState::Active,
        superseded_by: None,
        provenance: entry.value.provenance.clone(),
    };
    store
        .update_entry(
            PROJECT_ID,
            &still.id,
            rewrite(&still, &format!("{SYMBOLS} Checked.")),
        )
        .await
        .expect("still");
    store
        .update_entry(
            PROJECT_ID,
            &fixed.id,
            rewrite(
                &fixed,
                "Final compact duration unit symbols are yr, mth, d, h.",
            ),
        )
        .await
        .expect("fixed");
    let patch = git_patch_of_years();
    let commit = GitCommitSummary {
        oid: "0123456789abcdef0123456789abcdef01234567".to_string(),
        committed_at: 0,
        subject: "Use yr".to_string(),
        body: String::new(),
    };
    let checked = CommittedChange {
        change: literal_changes(&patch).0.remove(0),
        commit: &commit,
    };
    let judge = Judge {
        store,
        project_id: PROJECT_ID,
    };
    assert_eq!(
        (
            judge.mark(still, &checked).await.expect("still"),
            judge.mark(fixed, &checked).await.expect("fixed"),
            validity(store, "still").await,
            validity(store, "fixed").await,
        ),
        (
            Recorded::Marked,
            Recorded::Unchanged,
            Some(KnowledgeValidity::NeedsCheck),
            None,
        )
    );
}

fn git_patch_of_years() -> String {
    "diff --git a/src/_compact.py b/src/_compact.py\n--- a/src/_compact.py\n+++ b/src/_compact.py\n@@ -1,4 +1,4 @@\n _COMPACT_UNITS = {\n-    \"YEARS\": \"y\",\n+    \"YEARS\": \"yr\",\n     \"MONTHS\": \"mth\",\n     \"DAYS\": \"d\",\n"
        .to_string()
}

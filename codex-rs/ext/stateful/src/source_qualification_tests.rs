use std::path::Path;
use std::time::Duration;

use codex_project_intelligence::BlackboardEntry;
use codex_project_intelligence::BlackboardEntryId;
use codex_project_intelligence::BlackboardEntryState;
use codex_project_intelligence::BlackboardEntryUpdate;
use codex_project_intelligence::BlackboardImportance;
use codex_project_intelligence::BlackboardKind;
use codex_project_intelligence::BlackboardProvenance;
use codex_project_intelligence::BlackboardProvenanceKind;
use codex_project_intelligence::BlackboardVerification;
use codex_project_intelligence::ConfidenceScore;
use codex_project_intelligence::HierarchyNodeId;
use codex_project_intelligence::KnowledgeValidity;
use codex_project_intelligence::NewBlackboardEntry;
use codex_project_intelligence::NewHierarchyNode;
use codex_project_intelligence::NodeKind;
use codex_project_intelligence::ProjectRelativePath;
use codex_project_intelligence::QualificationState;
use codex_project_intelligence::RootPromotion;
use codex_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;
use tokio::time::Instant;

use super::ObservedRoot;
use super::qualify_root;
use crate::services::ProjectIntelligenceServices;

const PROJECT_ID: &str = "project-1";

fn git(cwd: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .args([
            "-c",
            "user.name=Codex",
            "-c",
            "user.email=codex@example.com",
        ])
        .args(["-c", "commit.gpgsign=false", "-c", "core.autocrlf=false"])
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("run git");
    assert!(output.status.success(), "git {args:?} failed");
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// Writes `file` and commits it; returns the new head.
fn commit(repo: &Path, file: &str, text: &str) -> String {
    std::fs::write(repo.join(file), text).expect("write");
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-q", "-m", file]);
    git(repo, &["rev-parse", "HEAD"])
}

struct Fixture {
    _state_home: tempfile::TempDir,
    repo: tempfile::TempDir,
    services: ProjectIntelligenceServices,
    root: String,
}

impl Fixture {
    async fn new() -> Self {
        let state_home = tempfile::TempDir::new().expect("state home");
        let repo = tempfile::TempDir::new().expect("repo");
        git(repo.path(), &["init", "-q", "-b", "main"]);
        commit(repo.path(), "units.py", "YEARS = 'y'\n");
        commit(repo.path(), "other.py", "OTHER = 1\n");
        let services =
            ProjectIntelligenceServices::new(codex_state::SqliteConfig::new_for_testing(
                codex_utils_absolute_path::test_support::PathExt::abs(state_home.path()),
            ));
        let root = repo.path().display().to_string();
        let project = services
            .project_node_id(PROJECT_ID)
            .await
            .expect("project node");
        let hierarchy = services.hierarchy().await.expect("hierarchy");
        let node = |kind, parent: &HierarchyNodeId, path: &str| NewHierarchyNode {
            project_id: PROJECT_ID.to_string(),
            parent_id: Some(parent.clone()),
            kind,
            project_root: Some(root.clone()),
            relative_path: if path.is_empty() {
                ProjectRelativePath::root()
            } else {
                ProjectRelativePath::parse(path).expect("path")
            },
            region_anchor: None,
            source_fingerprint: None,
        };
        let top = HierarchyNodeId::parse("node-top").expect("ID");
        hierarchy
            .create_node(top.clone(), node(NodeKind::Directory, &project, ""))
            .await
            .expect("top");
        for file in ["units.py", "other.py"] {
            hierarchy
                .create_node(
                    HierarchyNodeId::parse(format!("node-{file}")).expect("ID"),
                    node(NodeKind::File, &top, file),
                )
                .await
                .expect("file");
        }
        Self {
            _state_home: state_home,
            repo,
            services,
            root,
        }
    }

    async fn remember(
        &self,
        id: &str,
        node: Option<&str>,
        kind: BlackboardKind,
        provenance: BlackboardProvenanceKind,
    ) -> BlackboardEntry {
        let node_id = match node {
            Some(file) => HierarchyNodeId::parse(format!("node-{file}")).expect("ID"),
            None => self
                .services
                .project_node_id(PROJECT_ID)
                .await
                .expect("node"),
        };
        self.services
            .blackboard()
            .await
            .expect("store")
            .create_entry(
                BlackboardEntryId::parse(id).expect("ID"),
                NewBlackboardEntry {
                    project_id: PROJECT_ID.to_string(),
                    node_id,
                    kind,
                    content: format!("{id}: years use y"),
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

    async fn validity(&self, id: &str) -> Option<KnowledgeValidity> {
        self.services
            .blackboard()
            .await
            .expect("store")
            .knowledge_context(PROJECT_ID, &BlackboardEntryId::parse(id).expect("ID"))
            .await
            .expect("context")
            .map(|context| context.validity)
    }

    async fn qualify(&self, head: &str, previous: Option<&str>, deadline: Instant) -> Vec<String> {
        let worktree_root = AbsolutePathBuf::from_absolute_path(self.repo.path()).expect("abs");
        let root = ObservedRoot {
            project_root: &self.root,
            worktree_root: &worktree_root,
            head,
            previous_head: previous,
        };
        qualify_root(&self.services, PROJECT_ID, &root, deadline).await
    }
}

fn ample() -> Instant {
    Instant::now() + Duration::from_secs(10)
}

/// Through the checkout lifecycle: an agent assertion on the changed file and an agent
/// observation naming no file need a check; one on an unchanged file, an intention naming no
/// file and the user's own words do not; the target becomes the qualified head.
#[tokio::test]
async fn assertions_on_changed_files_need_a_check() {
    let fixture = Fixture::new().await;
    let roots = vec![fixture.root.clone()];
    fixture
        .remember(
            "on-units",
            Some("units.py"),
            BlackboardKind::Fact,
            BlackboardProvenanceKind::Agent,
        )
        .await;
    fixture
        .remember(
            "on-other",
            Some("other.py"),
            BlackboardKind::Fact,
            BlackboardProvenanceKind::Agent,
        )
        .await;
    fixture
        .remember(
            "unanchored",
            None,
            BlackboardKind::Claim,
            BlackboardProvenanceKind::Agent,
        )
        .await;
    fixture
        .remember(
            "intention",
            None,
            BlackboardKind::Decision,
            BlackboardProvenanceKind::Agent,
        )
        .await;
    fixture
        .remember(
            "user",
            Some("units.py"),
            BlackboardKind::Fact,
            BlackboardProvenanceKind::User,
        )
        .await;
    crate::checkout::observe_turn_start(&fixture.services, PROJECT_ID, &roots, "turn-1").await;
    let head = commit(fixture.repo.path(), "units.py", "YEARS = 'yr'\n");

    let report =
        crate::checkout::observe_turn_start(&fixture.services, PROJECT_ID, &roots, "turn-2")
            .await
            .expect("report")
            .0;

    let mut validity = Vec::new();
    for id in ["on-units", "on-other", "unanchored", "intention", "user"] {
        validity.push(fixture.validity(id).await);
    }
    let store = fixture.services.blackboard().await.expect("store");
    let job = store
        .qualification_jobs(PROJECT_ID)
        .await
        .expect("jobs")
        .remove(0);
    let reason = store
        .knowledge_context(
            PROJECT_ID,
            &BlackboardEntryId::parse("on-units").expect("ID"),
        )
        .await
        .expect("context")
        .and_then(|context| context.payload)
        .unwrap_or_default();
    assert_eq!(
        (
            validity,
            (job.state, job.qualified_head == head),
            report.contains("2 remembered conclusion(s) depend on files changed in"),
            reason.contains("units.py changed in"),
        ),
        (
            vec![
                Some(KnowledgeValidity::NeedsCheck),
                None,
                Some(KnowledgeValidity::NeedsCheck),
                None,
                None,
            ],
            (QualificationState::Complete, true),
            true,
            true,
        )
    );
}

/// A pass stopped after its first entry resumes from its cursor: the examined entry is not
/// judged again, the rest are, and only then does the job complete.
#[tokio::test]
async fn a_stopped_scan_resumes_from_its_cursor() {
    let fixture = Fixture::new().await;
    let base = git(fixture.repo.path(), &["rev-parse", "HEAD"]);
    let first = fixture
        .remember(
            "first",
            Some("units.py"),
            BlackboardKind::Fact,
            BlackboardProvenanceKind::Agent,
        )
        .await;
    fixture
        .remember(
            "second",
            Some("units.py"),
            BlackboardKind::Fact,
            BlackboardProvenanceKind::Agent,
        )
        .await;
    let head = commit(fixture.repo.path(), "units.py", "YEARS = 'yr'\n");
    let store = fixture.services.blackboard().await.expect("store");
    store
        .establish_qualified_head(PROJECT_ID, &fixture.root, &base)
        .await
        .expect("baseline");
    let job = store
        .start_qualification(
            PROJECT_ID,
            &fixture.root,
            &base,
            &head,
            Ok(r#"["units.py"]"#.into()),
        )
        .await
        .expect("job");
    let sequence = store
        .qualification_page(PROJECT_ID, 0, job.watermark, 1)
        .await
        .expect("page")[0]
        .sequence;
    // An earlier pass examined `first` (and found it unaffected then) before stopping.
    assert_eq!(first.id.as_str(), "first");
    store
        .record_qualification_progress(&job, sequence)
        .await
        .expect("progress");

    let expired = fixture.qualify(&head, Some(&base), Instant::now()).await;
    let before = fixture.validity("second").await;
    let resumed = fixture.qualify(&head, Some(&base), ample()).await;
    let job = store
        .qualification_jobs(PROJECT_ID)
        .await
        .expect("jobs")
        .remove(0);

    assert_eq!(
        (
            expired,
            before,
            resumed.len(),
            fixture.validity("first").await,
            fixture.validity("second").await,
            job.state,
        ),
        (
            Vec::<String>::new(),
            None,
            1,
            None,
            Some(KnowledgeValidity::NeedsCheck),
            QualificationState::Complete,
        )
    );
}

/// History that no longer descends from the qualified head blocks the job for that target:
/// it is reported once, not retried while HEAD stays, and nothing is completed.
#[tokio::test]
async fn discontinuous_history_blocks_without_retrying() {
    let fixture = Fixture::new().await;
    let base = git(fixture.repo.path(), &["rev-parse", "HEAD"]);
    git(
        fixture.repo.path(),
        &["checkout", "-q", "--orphan", "fresh"],
    );
    let fresh = commit(fixture.repo.path(), "units.py", "YEARS = 'yr'\n");

    let first = fixture.qualify(&fresh, Some(&base), ample()).await;
    let second = fixture.qualify(&fresh, Some(&base), ample()).await;
    let store = fixture.services.blackboard().await.expect("store");
    let job = store
        .qualification_jobs(PROJECT_ID)
        .await
        .expect("jobs")
        .remove(0);

    assert_eq!(
        (
            first.len(),
            first[0].contains("history moved from"),
            second,
            (job.state, job.qualified_head == base),
        ),
        (
            1,
            true,
            Vec::<String>::new(),
            (QualificationState::Blocked, true)
        )
    );
}

/// An entry corrected after it was read is judged again from its new revision.
#[tokio::test]
async fn a_concurrent_correction_is_judged_again() {
    let fixture = Fixture::new().await;
    let base = git(fixture.repo.path(), &["rev-parse", "HEAD"]);
    let entry = fixture
        .remember(
            "corrected",
            Some("units.py"),
            BlackboardKind::Fact,
            BlackboardProvenanceKind::Agent,
        )
        .await;
    let head = commit(fixture.repo.path(), "units.py", "YEARS = 'yr'\n");
    let store = fixture.services.blackboard().await.expect("store");
    store
        .establish_qualified_head(PROJECT_ID, &fixture.root, &base)
        .await
        .expect("baseline");
    let job = store
        .start_qualification(
            PROJECT_ID,
            &fixture.root,
            &base,
            &head,
            Ok(r#"["units.py"]"#.into()),
        )
        .await
        .expect("job");
    let item = store
        .qualification_page(PROJECT_ID, 0, job.watermark, 1)
        .await
        .expect("page")
        .remove(0);
    store
        .update_entry(
            PROJECT_ID,
            &entry.id,
            BlackboardEntryUpdate {
                expected_revision: 1,
                kind: entry.value.kind,
                content: "corrected: years use y (rechecked)".to_string(),
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
            },
        )
        .await
        .expect("corrected");
    let manifest = vec!["units.py".to_string()];
    let scan = super::Scan {
        store,
        project_id: PROJECT_ID,
        job: &job,
        manifest: &manifest,
        prefix: Some(""),
    };

    let examined = scan.examine(item).await.expect("examined");
    let stored = store
        .get_entry(PROJECT_ID, &entry.id)
        .await
        .expect("read")
        .expect("kept");

    assert_eq!(
        (
            matches!(examined, super::Examined::Marked),
            stored.revision,
            stored.value.content,
        ),
        (true, 3, "corrected: years use y (rechecked)".to_string())
    );
}

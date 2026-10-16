use super::*;
use crate::ProjectKnowledgeReadError;
use crate::ProjectKnowledgeReader;
use codex_project_store::PreparedDomains;
use codex_project_store::ProjectStoreLocation;
use codex_project_store::ProjectStoreReader;
use codex_project_store::ProjectStoreWriter;
use codex_project_store::StoreIdentity;
use codex_project_store::initialize_local_store;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use sqlx::Connection;
use sqlx::SqliteConnection;
use sqlx::sqlite::SqliteConnectOptions;

struct Fixture {
    reader: ProjectKnowledgeReader,
    writer: ProjectStoreWriter,
    observer: SqliteConnection,
    identity: StoreIdentity,
    _directory: tempfile::TempDir,
}

async fn domains() -> PreparedDomains {
    PreparedDomains::prepare(&[DomainManifest {
        domain: "project-intelligence",
        migrations: &super::super::MIGRATOR,
    }])
    .await
    .unwrap()
}

impl Fixture {
    #[expect(clippy::disallowed_methods, reason = "disposable project fixture")]
    async fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let location = ProjectStoreLocation::select_project(&directory.path().abs()).unwrap();
        let identity = initialize_local_store(&location).await.unwrap();
        let mut observer = SqliteConnection::connect_with(
            &SqliteConnectOptions::new()
                .filename(location.database_path().as_path())
                .foreign_keys(/*on*/ true),
        )
        .await
        .unwrap();
        domains().await.install(&mut observer).await.unwrap();
        let reader = ProjectStoreReader::open(&location, domains().await)
            .await
            .unwrap();
        Self {
            reader: ProjectKnowledgeReader::from_project_store(reader)
                .await
                .unwrap(),
            writer: ProjectStoreWriter::open(&location, domains().await, &identity)
                .await
                .unwrap(),
            observer,
            identity,
            _directory: directory,
        }
    }

    async fn durable(&mut self) -> (ProjectKnowledgeSnapshot, Vec<(String, String, i64)>) {
        let snapshot = self.reader.inspect(&self.identity).await.unwrap();
        let nodes =
            sqlx::query_as("SELECT id,project_id,revision FROM hierarchy_nodes ORDER BY id")
                .fetch_all(&mut self.observer)
                .await
                .unwrap();
        (snapshot, nodes)
    }

    async fn close(self) {
        self.writer.close().await.unwrap();
        self.reader.close().await.unwrap();
        self.observer.close().await.unwrap();
    }
}

#[tokio::test]
async fn clock_check_reads_prior_writes_losslessly_and_keeps_the_same_guard() {
    let mut fixture = Fixture::new().await;
    let mut expected = fixture.reader.inspect(&fixture.identity).await.unwrap();
    let mut transaction = fixture.writer.begin_write().await.unwrap();
    sqlx::query("INSERT INTO project_intelligence_revisions VALUES (?,9007199254740993)")
        .bind(fixture.identity.project_uuid.to_string())
        .execute(transaction.connection())
        .await
        .unwrap();
    expected.project_revision = 9_007_199_254_740_993;
    let transaction = revalidate_project_knowledge_clocks(transaction, &expected)
        .await
        .unwrap();
    assert_eq!(transaction.identity(), &fixture.identity);
    transaction.commit().await.unwrap();
    assert_eq!(fixture.durable().await, (expected.clone(), vec![]));
    revalidate_project_knowledge_clocks(fixture.writer.begin_write().await.unwrap(), &expected)
        .await
        .unwrap()
        .rollback()
        .await
        .unwrap();
    fixture.close().await;
}

#[tokio::test]
async fn stale_project_or_retirement_clock_refuses_without_changing_durable_state() {
    for sql in [
        "INSERT INTO project_intelligence_revisions VALUES (?,9007199254740993)",
        "INSERT INTO capsule_eligibility_generations VALUES (?,9007199254740993)",
    ] {
        let mut fixture = Fixture::new().await;
        let expected = fixture.reader.inspect(&fixture.identity).await.unwrap();
        sqlx::query(sql)
            .bind(fixture.identity.project_uuid.to_string())
            .execute(&mut fixture.observer)
            .await
            .unwrap();
        let before = fixture.durable().await;
        assert!(matches!(
            revalidate_project_knowledge_clocks(
                fixture.writer.begin_write().await.unwrap(),
                &expected,
            )
            .await,
            Err(ProjectKnowledgeWriteError::SnapshotChanged)
        ));
        assert!(matches!(
            fixture.writer.begin_write().await,
            Err(ProjectWriteError::UnfinishedOperation)
        ));
        assert_eq!(fixture.durable().await, before);
        fixture.close().await;
    }
}

#[tokio::test]
async fn failed_precondition_rolls_back_prior_knowledge_mutation() {
    let mut fixture = Fixture::new().await;
    let before = fixture.durable().await;
    let mut transaction = fixture.writer.begin_write().await.unwrap();
    sqlx::query("INSERT INTO hierarchy_nodes(id,project_id,kind,relative_path,lifecycle,revision,created_at_ms,updated_at_ms) VALUES ('node',?,'project','','active',1,0,0)")
        .bind(fixture.identity.project_uuid.to_string())
        .execute(transaction.connection())
        .await
        .unwrap();
    assert!(matches!(
        revalidate_project_knowledge_clocks(transaction, &before.0).await,
        Err(ProjectKnowledgeWriteError::SnapshotChanged)
    ));
    // Await queued rollback before inspecting through independent connections.
    fixture.writer.close().await.unwrap();
    assert_eq!(
        (
            fixture.reader.inspect(&fixture.identity).await.unwrap(),
            sqlx::query_as::<_, (String, String, i64)>(
                "SELECT id,project_id,revision FROM hierarchy_nodes ORDER BY id"
            )
            .fetch_all(&mut fixture.observer)
            .await
            .unwrap(),
        ),
        before
    );
    fixture.reader.close().await.unwrap();
    fixture.observer.close().await.unwrap();
}

#[tokio::test]
async fn either_foreign_uuid_refuses_a_matching_clock_snapshot() {
    for replace_project in [true, false] {
        let mut fixture = Fixture::new().await;
        let other = Fixture::new().await;
        let before = fixture.durable().await;
        let mut expected = before.0.clone();
        if replace_project {
            expected.identity.project_uuid = other.identity.project_uuid;
        } else {
            expected.identity.store_uuid = other.identity.store_uuid;
        }
        assert!(matches!(
            revalidate_project_knowledge_clocks(
                fixture.writer.begin_write().await.unwrap(),
                &expected,
            )
            .await,
            Err(ProjectKnowledgeWriteError::IdentityMismatch)
        ));
        assert_eq!(fixture.durable().await, before);
        fixture.close().await;
        other.close().await;
    }
}

#[tokio::test]
async fn incomplete_coverage_refuses_even_when_the_expected_clocks_match() {
    for expected_complete in [false, true] {
        let mut fixture = Fixture::new().await;
        sqlx::query("INSERT INTO capture_identity_coverage(project_id,watermark) VALUES (?,0)")
            .bind(fixture.identity.project_uuid.to_string())
            .execute(&mut fixture.observer)
            .await
            .unwrap();
        let before = fixture.durable().await;
        let mut expected = before.0.clone();
        expected.coverage_complete = expected_complete;
        assert!(matches!(
            revalidate_project_knowledge_clocks(
                fixture.writer.begin_write().await.unwrap(),
                &expected,
            )
            .await,
            Err(ProjectKnowledgeWriteError::CoverageIncomplete)
        ));
        assert_eq!(fixture.durable().await, before);
        fixture.close().await;
    }
}

#[tokio::test]
async fn malformed_present_clock_is_not_coerced_to_an_integer() {
    for sql in [
        "INSERT INTO project_intelligence_revisions VALUES (?,'broken')",
        "INSERT INTO project_intelligence_revisions VALUES (?,1.5)",
        "INSERT INTO capsule_eligibility_generations VALUES (?,1.5)",
    ] {
        let mut fixture = Fixture::new().await;
        let expected = fixture.reader.inspect(&fixture.identity).await.unwrap();
        sqlx::query(sql)
            .bind(fixture.identity.project_uuid.to_string())
            .execute(&mut fixture.observer)
            .await
            .unwrap();
        assert!(matches!(
            revalidate_project_knowledge_clocks(
                fixture.writer.begin_write().await.unwrap(),
                &expected,
            )
            .await,
            Err(ProjectKnowledgeWriteError::InvalidClock)
        ));
        fixture.close().await;
    }
}

async fn seed_learning(fixture: &mut Fixture, provenance: &str, recorded_at_ms: i64) {
    let project = fixture.identity.project_uuid.to_string();
    sqlx::query("INSERT INTO hierarchy_nodes(id,project_id,kind,relative_path,lifecycle,revision,created_at_ms,updated_at_ms) VALUES ('node',?,'project','','active',1,0,0)")
        .bind(&project).execute(&mut fixture.observer).await.unwrap();
    for entry in ["first", "second"] {
        sqlx::query("INSERT INTO blackboard_entries VALUES (?,?,'node',1,0,0)")
            .bind(entry)
            .bind(&project)
            .execute(&mut fixture.observer)
            .await
            .unwrap();
        sqlx::query("INSERT INTO blackboard_entry_revisions(entry_id,revision,kind,content,confidence_basis_points,verification,importance,root_promotion,state,provenance_kind,provenance_source_id,recorded_at_ms) VALUES (?,1,'note','synthetic learning',5000,'unverified','normal','not_promoted','active',?,'synthetic',?)")
            .bind(entry).bind(provenance).bind(recorded_at_ms).execute(&mut fixture.observer).await.unwrap();
    }
}

#[tokio::test]
async fn no_learning_preserves_inclusive_boundary_and_agent_attribution() {
    for (provenance, recorded_at_ms, refuses) in [
        ("agent", 99, false),
        ("agent", 100, true),
        ("agent", 101, true),
        ("user", 100, false),
        ("maintenance", 100, false),
        ("import", 100, false),
    ] {
        let mut fixture = Fixture::new().await;
        seed_learning(&mut fixture, provenance, recorded_at_ms).await;
        let before = fixture.durable().await;
        let result = fixture
            .reader
            .prepare_no_reusable_learning(&fixture.identity, /*run_created_at_ms*/ 100)
            .await;
        if refuses {
            assert!(matches!(
                result,
                Err(ProjectKnowledgeReadError::ReusableLearningPresent)
            ));
        } else {
            let prepared = result.unwrap();
            assert_eq!(prepared.run_created_at_ms(), 100);
            let mut transaction = revalidate_project_no_reusable_learning(
                fixture.writer.begin_write().await.unwrap(),
                &prepared,
            )
            .await
            .unwrap();
            assert_eq!(transaction.identity(), &fixture.identity);
            // This fixture timestamp update does not change knowledge clocks.
            sqlx::query("UPDATE hierarchy_nodes SET updated_at_ms=17 WHERE id='node'")
                .execute(transaction.connection())
                .await
                .unwrap();
            transaction.commit().await.unwrap();
            let updated: i64 =
                sqlx::query_scalar("SELECT updated_at_ms FROM hierarchy_nodes WHERE id='node'")
                    .fetch_one(&mut fixture.observer)
                    .await
                    .unwrap();
            assert_eq!(updated, 17);
        }
        assert_eq!(fixture.durable().await, before);
        fixture.close().await;
    }
}

#[tokio::test]
async fn no_learning_rejects_retired_and_noncurrent_agent_history() {
    for retire in [false, true] {
        let mut fixture = Fixture::new().await;
        seed_learning(&mut fixture, "agent", /*recorded_at_ms*/ 100).await;
        if retire {
            sqlx::query("UPDATE blackboard_entry_revisions SET state='tombstoned'")
                .execute(&mut fixture.observer)
                .await
                .unwrap();
        } else {
            sqlx::query("UPDATE blackboard_entries SET revision=2")
                .execute(&mut fixture.observer)
                .await
                .unwrap();
            sqlx::query("INSERT INTO blackboard_entry_revisions(entry_id,revision,kind,content,confidence_basis_points,verification,importance,root_promotion,state,provenance_kind,provenance_source_id,recorded_at_ms) SELECT entry_id,2,kind,'user correction',confidence_basis_points,verification,importance,root_promotion,state,'user',provenance_source_id,101 FROM blackboard_entry_revisions WHERE revision=1")
                .execute(&mut fixture.observer).await.unwrap();
        }
        let before = fixture.durable().await;
        assert!(matches!(
            fixture
                .reader
                .prepare_no_reusable_learning(&fixture.identity, /*run_created_at_ms*/ 100,)
                .await,
            Err(ProjectKnowledgeReadError::ReusableLearningPresent)
        ));
        assert_eq!(fixture.durable().await, before);
        fixture.close().await;
    }
}

#[tokio::test]
async fn no_learning_checks_agent_relations_even_between_user_entries() {
    let mut fixture = Fixture::new().await;
    seed_learning(&mut fixture, "user", /*recorded_at_ms*/ 99).await;
    sqlx::query("INSERT INTO blackboard_relations(id,project_id,from_entry_id,to_entry_id,kind,note,confidence_basis_points,provenance_kind,provenance_source_id,revision,created_at_ms,updated_at_ms) VALUES ('relation',?,'first','second','related_to',NULL,5000,'agent','synthetic',1,100,100)")
        .bind(fixture.identity.project_uuid.to_string()).execute(&mut fixture.observer).await.unwrap();
    let before = fixture.durable().await;
    assert!(matches!(
        fixture
            .reader
            .prepare_no_reusable_learning(&fixture.identity, /*run_created_at_ms*/ 100,)
            .await,
        Err(ProjectKnowledgeReadError::ReusableLearningPresent)
    ));
    assert_eq!(fixture.durable().await, before);
    fixture.close().await;
}

#[tokio::test]
async fn prepared_no_learning_race_refusal_rolls_back_prior_writes() {
    let mut fixture = Fixture::new().await;
    seed_learning(&mut fixture, "user", /*recorded_at_ms*/ 99).await;
    let prepared = fixture
        .reader
        .prepare_no_reusable_learning(&fixture.identity, /*run_created_at_ms*/ 100)
        .await
        .unwrap();
    sqlx::query("INSERT INTO blackboard_relations(id,project_id,from_entry_id,to_entry_id,kind,note,confidence_basis_points,provenance_kind,provenance_source_id,revision,created_at_ms,updated_at_ms) VALUES ('relation',?,'first','second','related_to',NULL,5000,'agent','synthetic',1,100,100)")
        .bind(fixture.identity.project_uuid.to_string()).execute(&mut fixture.observer).await.unwrap();
    let before = fixture.durable().await;
    let mut transaction = fixture.writer.begin_write().await.unwrap();
    sqlx::query("UPDATE hierarchy_nodes SET updated_at_ms=17 WHERE id='node'")
        .execute(transaction.connection())
        .await
        .unwrap();
    assert!(matches!(
        revalidate_project_no_reusable_learning(transaction, &prepared).await,
        Err(ProjectKnowledgeWriteError::SnapshotChanged)
    ));
    fixture.writer.close().await.unwrap();
    assert_eq!(
        fixture.reader.inspect(&fixture.identity).await.unwrap(),
        before.0
    );
    let updated: i64 =
        sqlx::query_scalar("SELECT updated_at_ms FROM hierarchy_nodes WHERE id='node'")
            .fetch_one(&mut fixture.observer)
            .await
            .unwrap();
    assert_eq!(updated, 0);
    fixture.reader.close().await.unwrap();
    fixture.observer.close().await.unwrap();
}

#[tokio::test]
async fn no_learning_preparation_refuses_invalid_run_start_and_reader_remains_usable() {
    let mut fixture = Fixture::new().await;
    let before = fixture.durable().await;
    assert!(matches!(
        fixture
            .reader
            .prepare_no_reusable_learning(&fixture.identity, /*run_created_at_ms*/ -1,)
            .await,
        Err(ProjectKnowledgeReadError::InvalidRunStart)
    ));
    assert_eq!(fixture.durable().await, before);
    let prepared = fixture
        .reader
        .prepare_no_reusable_learning(&fixture.identity, /*run_created_at_ms*/ 0)
        .await
        .unwrap();
    revalidate_project_no_reusable_learning(fixture.writer.begin_write().await.unwrap(), &prepared)
        .await
        .unwrap()
        .rollback()
        .await
        .unwrap();
    fixture.close().await;
}

#[tokio::test]
async fn prepared_no_learning_refuses_a_new_agent_revision() {
    let mut fixture = Fixture::new().await;
    seed_learning(&mut fixture, "user", /*recorded_at_ms*/ 99).await;
    let prepared = fixture
        .reader
        .prepare_no_reusable_learning(&fixture.identity, /*run_created_at_ms*/ 100)
        .await
        .unwrap();
    sqlx::query("UPDATE blackboard_entries SET revision=2 WHERE id='first'")
        .execute(&mut fixture.observer)
        .await
        .unwrap();
    sqlx::query("INSERT INTO blackboard_entry_revisions(entry_id,revision,kind,content,confidence_basis_points,verification,importance,root_promotion,state,provenance_kind,provenance_source_id,recorded_at_ms) SELECT entry_id,2,kind,'agent learning',confidence_basis_points,verification,importance,root_promotion,state,'agent',provenance_source_id,100 FROM blackboard_entry_revisions WHERE entry_id='first' AND revision=1")
        .execute(&mut fixture.observer).await.unwrap();
    let before = fixture.durable().await;
    assert!(matches!(
        revalidate_project_no_reusable_learning(
            fixture.writer.begin_write().await.unwrap(),
            &prepared,
        )
        .await,
        Err(ProjectKnowledgeWriteError::SnapshotChanged)
    ));
    assert_eq!(fixture.durable().await, before);
    fixture.close().await;
}

#[tokio::test]
async fn prepared_no_learning_cannot_cross_store_identity_or_coverage_changes() {
    let mut fixture = Fixture::new().await;
    let mut other = Fixture::new().await;
    let prepared = other
        .reader
        .prepare_no_reusable_learning(&other.identity, /*run_created_at_ms*/ 100)
        .await
        .unwrap();
    let before = fixture.durable().await;
    assert!(matches!(
        revalidate_project_no_reusable_learning(
            fixture.writer.begin_write().await.unwrap(),
            &prepared,
        )
        .await,
        Err(ProjectKnowledgeWriteError::IdentityMismatch)
    ));
    assert_eq!(fixture.durable().await, before);
    fixture.close().await;
    other.close().await;

    let mut fixture = Fixture::new().await;
    let prepared = fixture
        .reader
        .prepare_no_reusable_learning(&fixture.identity, /*run_created_at_ms*/ 100)
        .await
        .unwrap();
    sqlx::query("INSERT INTO capture_identity_coverage(project_id,watermark) VALUES (?,0)")
        .bind(fixture.identity.project_uuid.to_string())
        .execute(&mut fixture.observer)
        .await
        .unwrap();
    let before = fixture.durable().await;
    assert!(matches!(
        fixture
            .reader
            .prepare_no_reusable_learning(&fixture.identity, /*run_created_at_ms*/ 100,)
            .await,
        Err(ProjectKnowledgeReadError::CoverageIncomplete)
    ));
    assert!(matches!(
        revalidate_project_no_reusable_learning(
            fixture.writer.begin_write().await.unwrap(),
            &prepared,
        )
        .await,
        Err(ProjectKnowledgeWriteError::SnapshotChanged)
    ));
    assert_eq!(fixture.durable().await, before);
    fixture.close().await;
}

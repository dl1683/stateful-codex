use super::*;
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

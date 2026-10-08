use codex_state::SqliteConfig;

pub(super) async fn thread_project(sqlite: &SqliteConfig, thread: &str) -> String {
    let runtime = codex_state::StateRuntime::init(sqlite.clone(), "test".into())
        .await
        .unwrap();
    let thread_id = codex_protocol::ThreadId::from_string(thread).unwrap();
    let metadata = codex_state::ThreadMetadataBuilder::new(
        thread_id,
        sqlite.home().join("rollout.jsonl"),
        serde_json::from_str("\"2026-10-08T00:00:00Z\"").unwrap(),
        codex_protocol::protocol::SessionSource::Cli,
    )
    .build("test");
    runtime.upsert_thread(&metadata).await.unwrap();
    runtime
        .create_project(
            "Capture repair".into(),
            Vec::new(),
            Default::default(),
            &[thread.to_string()],
            "capture-repair",
        )
        .await
        .unwrap()
        .project
        .id
}

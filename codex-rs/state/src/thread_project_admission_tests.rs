use super::*;
use codex_utils_absolute_path::test_support::PathExt;

#[tokio::test]
async fn admission_holds_binding_until_dependent_commit_and_refuses_the_old_project() {
    let home = crate::runtime::test_support::unique_temp_dir();
    tokio::fs::create_dir_all(&home).await.expect("home");
    let _cleanup = scopeguard::guard(home.clone(), |home| {
        let _ = std::fs::remove_dir_all(home);
    });
    let sqlite = SqliteConfig::new_for_testing(home.abs());
    let writer = sqlite
        .open_read_write_pool(&sqlite.state_db_path())
        .await
        .expect("pool");
    sqlx::query("CREATE TABLE threads (id TEXT PRIMARY KEY, project_id TEXT)")
        .execute(&writer)
        .await
        .expect("table");
    let thread = ThreadId::new();
    sqlx::query("INSERT INTO threads VALUES (?, 'original')")
        .bind(thread.to_string())
        .execute(&writer)
        .await
        .expect("thread");
    let admission = ThreadProjectAdmission::acquire(&sqlite, thread, "original")
        .await
        .expect("admit")
        .expect("binding");
    let (started, waiting) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        started.send(()).expect("signal");
        sqlx::query("UPDATE threads SET project_id = 'new' WHERE id = ?")
            .bind(thread.to_string())
            .execute(&writer)
            .await
            .expect("rebind");
    });
    waiting.await.expect("started");
    let mut task = task;
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(/*millis*/ 50), &mut task)
            .await
            .is_err()
    );
    drop(admission);
    task.await.expect("committed rebind");
    assert!(
        ThreadProjectAdmission::acquire(&sqlite, thread, "original")
            .await
            .expect("old admission")
            .is_none()
    );
    assert!(
        ThreadProjectAdmission::acquire(&sqlite, thread, "new")
            .await
            .expect("current admission")
            .is_some()
    );
}

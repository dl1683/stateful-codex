use super::*;
use pretty_assertions::assert_eq;

/// R1: a Partial selected by live collection discloses on normal, retry and recovered commit.
#[tokio::test]
async fn collected_partial_discloses_on_every_success_path() {
    let home = TempDir::new().expect("home");
    let store = store(&home).await;
    let run = running_run(&store).await;
    store
        .apply_obligation_operation(
            &run.id,
            codex_stateful_runtime::ObligationOperation::Retain {
                issues: vec!["x".into()],
            },
            &codex_stateful_runtime::OperationReceiptKey::parse("retain").expect("key"),
            "source",
        )
        .await
        .expect("retained");
    let commit = commit_for(&run);
    let warning = TurnFinalizeOutcome::Warning(
        "Stateful: this run ended partial; it is not certified by this terminal answer path. Open or not certified: Open issue (retained): x".into()
    );
    for outcome in [
        store.end_run_answered(&commit, &|| true).await,
        store.end_run_answered(&commit, &|| true).await,
        commit_error(),
    ] {
        assert_eq!(
            resolve(&store, None, &commit, /*authorized*/ true, outcome).await,
            warning
        );
    }
    assert_eq!(
        store
            .get_run(&run.id)
            .await
            .expect("read")
            .expect("run")
            .continuations_used,
        0
    );
}

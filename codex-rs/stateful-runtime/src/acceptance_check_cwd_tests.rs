use pretty_assertions::assert_eq;
use tempfile::TempDir;

use crate::AcceptanceChange;
use crate::AcceptanceKind;
use crate::AcceptanceOrigin;
use crate::CriterionVerdict;
use crate::RequestSpan;
use crate::StatefulRunStore;
use crate::TermsUpdate;
use crate::WorkflowMode;
use crate::ledger_verdicts;

use super::CHECKER;
use super::GOAL;
use super::PINNED;
use super::admit;
use super::checked;
use super::receipt;
use super::state;
use super::store;

fn declaration(cwd: &str) -> AcceptanceChange {
    let AcceptanceChange::Add { mut terms, .. } = checked("python check.py", &["real/check.py"])
    else {
        unreachable!()
    };
    terms.check_cwd = Some(cwd.to_string());
    AcceptanceChange::Add {
        origin: AcceptanceOrigin::User,
        kind: AcceptanceKind::Check,
        statement: GOAL.to_string(),
        request_span: Some(RequestSpan {
            start: 0,
            end: GOAL.len(),
        }),
        terms,
    }
}

fn directory_update(cwd: &str) -> AcceptanceChange {
    AcceptanceChange::Refine {
        ordinal: 1,
        statement: None,
        required: None,
        terms: TermsUpdate {
            check_cwd: Some(cwd.to_string()),
            ..TermsUpdate::default()
        },
    }
}

async fn row_bytes(store: &StatefulRunStore) -> String {
    sqlx::query_scalar(
        "SELECT json_array(run_id, ordinal, criterion_id, origin, kind, state,
            statement, requirement, required, depends_on_json, milestone,
            expected_observation, span_start, span_end, artifacts_json, checker_json,
            check_command, check_cwd, plan_criterion_revision, plan_checker_digest,
            plan_ledger_revision, dismissal_covered_by, note, revision, ledger_revision,
            created_at_ms, updated_at_ms)
         FROM stateful_acceptance_criteria WHERE ordinal = 1",
    )
    .fetch_one(&store.pool)
    .await
    .expect("complete row representation")
}

#[tokio::test]
async fn declarations_are_canonical_and_refusals_store_nothing() {
    let (home, store, id) = store(GOAL, WorkflowMode::Autonomous).await;
    let root = home.path().canonicalize().expect("root");
    std::fs::create_dir(root.join("real")).expect("real");
    std::fs::write(root.join("real/check.py"), "checker").expect("checker");
    let outside = TempDir::new().expect("outside or second root");
    for cwd in [
        outside.path().to_str().expect("UTF-8"),
        "../escape",
        "missing",
        "verify.sh",
    ] {
        let before = store.acceptance_ledger(&id).await.expect("before");
        store
            .revise_acceptance(
                &id,
                /*expected_revision*/ 0,
                vec![declaration(cwd)],
                "call-declare",
                Some(root.clone()),
            )
            .await
            .expect_err("invalid declaration");
        assert_eq!(store.acceptance_ledger(&id).await.expect("after"), before);
    }
    let error = store
        .revise_acceptance(
            &id,
            /*expected_revision*/ 0,
            vec![declaration("real")],
            "call-no-root",
            /*project_root*/ None,
        )
        .await
        .expect_err("no root");
    assert!(
        error
            .to_string()
            .contains("checkCwd needs a local project root"),
        "{error}"
    );
    let ledger = store
        .revise_acceptance(
            &id,
            /*expected_revision*/ 0,
            vec![
                declaration(root.join("real").to_str().expect("UTF-8")),
                admit(/*ordinal*/ 1),
            ],
            "call-declare",
            Some(root),
        )
        .await
        .expect("absolute in-root declaration admitted");
    assert_eq!(
        (
            ledger.criteria[0].check_cwd.as_deref(),
            ledger.criteria[0].checker.clone(),
            ledger.criteria[0].artifacts.clone()
        ),
        (
            Some("real"),
            vec!["real/check.py".to_string()],
            vec!["out.txt".to_string()]
        )
    );
}

#[tokio::test]
async fn legacy_rewrites_are_atomic_and_old_receipts_need_fresh_evidence() {
    let (home, store, id) = store(GOAL, WorkflowMode::Autonomous).await;
    let root = home.path().canonicalize().expect("root");
    std::fs::create_dir(root.join("real")).expect("real");
    std::fs::create_dir(root.join("other")).expect("other");
    std::fs::write(root.join("real/check.py"), "checker").expect("checker");
    let ledger = store
        .revise_acceptance(
            &id,
            /*expected_revision*/ 0,
            vec![declaration("real"), admit(/*ordinal*/ 1)],
            "call-add",
            Some(root.clone()),
        )
        .await
        .expect("admitted");
    store
        .record_command_evidence(&id, vec![receipt(&ledger, /*exit_code*/ 0, Some(CHECKER))])
        .await
        .expect("receipt");
    for legacy in [
        root.join("real").to_str().expect("UTF-8").to_string(),
        "real/.".to_string(),
    ] {
        sqlx::query("UPDATE stateful_acceptance_criteria SET check_cwd = ?")
            .bind(&legacy)
            .execute(&store.pool)
            .await
            .expect("legacy representation");
        let before = store.acceptance_ledger(&id).await.expect("before");
        let bytes = row_bytes(&store).await;
        store
            .revise_acceptance(
                &id,
                before.revision,
                vec![AcceptanceChange::Admit {
                    ordinal: 1,
                    checker_digest: None,
                }],
                "call-unreadable",
                Some(root.clone()),
            )
            .await
            .expect_err("unreadable digest rolls back");
        assert_eq!(row_bytes(&store).await, bytes);
        store
            .revise_acceptance(
                &id,
                before.revision,
                vec![admit(/*ordinal*/ 1), declaration("../escape")],
                "call-batch",
                Some(root.clone()),
            )
            .await
            .expect_err("later refusal rolls back the rewrite");
        assert_eq!(row_bytes(&store).await, bytes);
        assert_eq!(
            store.acceptance_ledger(&id).await.expect("unchanged"),
            before
        );
        let rewritten = store
            .revise_acceptance(
                &id,
                before.revision,
                vec![admit(/*ordinal*/ 1)],
                "call-rewrite",
                Some(root.clone()),
            )
            .await
            .expect("representation-only admission");
        let criterion = &rewritten.criteria[0];
        assert_eq!(
            (
                rewritten.revision,
                criterion.revision,
                criterion.check_cwd.as_deref(),
                criterion.plan.as_ref().expect("plan").criterion_revision
            ),
            (
                before.revision + 1,
                before.criteria[0].revision + 1,
                Some("real"),
                before.criteria[0].revision + 1
            )
        );
        assert!(
            ledger_verdicts(&rewritten, &state(PINNED), &state(CHECKER))[0]
                .1
                .is_unmet()
        );
        store
            .record_command_evidence(
                &id,
                vec![receipt(&rewritten, /*exit_code*/ 0, Some(CHECKER))],
            )
            .await
            .expect("fresh receipt");
    }
    let ledger = store.acceptance_ledger(&id).await.expect("current");
    assert_eq!(
        ledger_verdicts(&ledger, &state(PINNED), &state(CHECKER))[0].1,
        CriterionVerdict::SatisfiedByHost
    );
    let bytes = row_bytes(&store).await;
    store
        .revise_acceptance(
            &id,
            ledger.revision,
            vec![directory_update("other")],
            "call-replace",
            Some(root.clone()),
        )
        .await
        .expect_err("different directory after execution");
    assert_eq!(row_bytes(&store).await, bytes);
    let unchanged = store
        .revise_acceptance(
            &id,
            ledger.revision,
            vec![directory_update("real/.")],
            "call-same",
            Some(root),
        )
        .await
        .expect("same canonical directory after execution");
    assert_eq!(unchanged, ledger);
}

#[tokio::test]
async fn invalid_legacy_values_and_no_root_admissions_leave_the_whole_row_unchanged() {
    let (home, store, id) = store(GOAL, WorkflowMode::Autonomous).await;
    let root = home.path().canonicalize().expect("root");
    std::fs::create_dir(root.join("real")).expect("real");
    std::fs::write(root.join("real/check.py"), "checker").expect("checker");
    let ledger = store
        .revise_acceptance(
            &id,
            /*expected_revision*/ 0,
            vec![declaration("real")],
            "call-add",
            Some(root.clone()),
        )
        .await
        .expect("criterion");
    let outside = TempDir::new().expect("outside");
    for legacy in [
        outside.path().to_str().expect("UTF-8"),
        "../escape",
        "missing",
        "verify.sh",
    ] {
        sqlx::query("UPDATE stateful_acceptance_criteria SET check_cwd = ?")
            .bind(legacy)
            .execute(&store.pool)
            .await
            .expect("legacy row");
        let bytes = row_bytes(&store).await;
        let error = store
            .revise_acceptance(
                &id,
                ledger.revision,
                vec![admit(/*ordinal*/ 1)],
                "call-admit",
                Some(root.clone()),
            )
            .await
            .expect_err("invalid legacy directory");
        assert!(
            error
                .to_string()
                .contains("checkCwd must be an existing directory inside the first project root"),
            "{error}"
        );
        assert_eq!(row_bytes(&store).await, bytes);
    }
    let bytes = row_bytes(&store).await;
    let error = store
        .revise_acceptance(
            &id,
            ledger.revision,
            vec![admit(/*ordinal*/ 1)],
            "call-no-root",
            /*project_root*/ None,
        )
        .await
        .expect_err("no local root");
    assert!(
        error
            .to_string()
            .contains("checkCwd needs a local project root"),
        "{error}"
    );
    assert_eq!(row_bytes(&store).await, bytes);
}

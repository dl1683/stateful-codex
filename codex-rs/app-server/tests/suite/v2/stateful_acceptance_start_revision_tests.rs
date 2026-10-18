//! Local producer-path checks: host filesystem barriers cannot be shared with remote exec.
use super::*;
use codex_state::SqliteConfig;
use codex_stateful_runtime as runtime;
use codex_utils_absolute_path::test_support::PathExt;
use core_test_support::skip_if_remote;
use sqlx::Connection;
use std::time::Duration;

#[cfg(windows)]
const PYTHON: &str = "python";
#[cfg(not(windows))]
const PYTHON: &str = "python3";

async fn barrier(path: &std::path::Path) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(30), async {
        while !path.exists() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await?;
    Ok(())
}

async fn accounted(
    store: &runtime::StatefulRunStore,
    id: &runtime::StatefulRunId,
    executions: u64,
    pending: u64,
) -> Result<runtime::AcceptanceLedger> {
    Ok(tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let ledger = store.acceptance_ledger(id).await?;
            if (ledger.observed_executions, ledger.pending_commands) == (executions, pending) {
                return Ok::<_, anyhow::Error>(ledger);
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await??)
}

async fn rows(db: &mut sqlx::SqliteConnection, id: &str) -> Result<Vec<String>> {
    Ok(sqlx::query_scalar(
        "SELECT json_object('sequence',sequence,'run',run_id,'ordinal',ordinal,
         'source',source,'outcome',outcome,'command',command,'exit',exit_code,
         'tail',output_tail,'output',output_digest,'artifact',artifact_digest,
         'checker',checker_digest,'detail',detail,'generation',workspace_generation,
         'revision',criterion_revision,'sourceId',source_id,'time',observed_at_ms)
         FROM stateful_acceptance_evidence WHERE run_id = ? ORDER BY sequence",
    )
    .bind(id)
    .fetch_all(db)
    .await?)
}

#[tokio::test]
async fn captured_revision_accounts_late_real_exits_without_displacing_passes() -> Result<()> {
    skip_if_remote!(
        Ok(()),
        "external barriers belong to the local host filesystem"
    );
    for case in [
        "unchanged",
        "pinned",
        "unpinned",
        "argv",
        "cwd",
        "failed",
        "missing-pass",
        "missing-fail",
    ] {
        let events = TempDir::new()?;
        let external = serde_json::to_string(&events.path().to_string_lossy())?;
        // Each actual child claims a distinct external ticket; ready follows the awaited
        // production start hook. No barrier file participates in workspace identity.
        let script = format!(
            r#"import pathlib, sys, time
base = pathlib.Path({external})
n = 0
while True:
    try:
        (base / str(n)).mkdir()
        break
    except FileExistsError:
        n += 1
slot = base / str(n)
(slot / 'ready').touch()
limit = time.monotonic() + 60
while not (slot / 'release').exists():
    if time.monotonic() > limit:
        sys.exit(90)
    time.sleep(0.02)
if (slot / 'pinned').exists():
    pathlib.Path('out.txt').write_text('changed')
if (slot / 'unpinned').exists():
    pathlib.Path('extra.txt').write_text('changed')
sys.exit(7 if (slot / 'fail').exists() else 0)
"#
        );
        let mut h = harness_for(
            StatefulWorkflowMode::Collaborative,
            "danger-full-access",
            GOAL,
            &[
                ("verify.sh", &script),
                ("out.txt", "initial"),
                ("extra.txt", "initial"),
            ],
        )
        .await?;
        let root = h.project_root.path().to_path_buf();
        let command = format!("{PYTHON} verify.sh");
        let missing = case.starts_with("missing");
        responses::mount_sse_sequence(
            &h.responses_server,
            vec![
                declare(&if missing {
                    format!("{command} unused")
                } else {
                    command.clone()
                }),
                admit("admit-r1", 1),
                message("ready", "Plan admitted."),
            ],
        )
        .await;
        h.turn("Declare the check.").await?;
        let sqlite = SqliteConfig::new_for_testing(h._codex_home.path().abs());
        let store = runtime::StatefulRunStore::open(&sqlite).await?;
        let id = runtime::StatefulRunId::parse(&h.run.id)?;
        let mut db = sqlx::SqliteConnection::connect_with(
            &sqlx::sqlite::SqliteConnectOptions::new()
                .filename(h._codex_home.path().join("stateful_runtime_1.sqlite"))
                .read_only(true),
        )
        .await?;
        let old_count = if case == "unchanged" { 5 } else { 1 };
        for n in 0..old_count {
            let call_id = format!("old-{n}");
            responses::mount_sse_sequence(
                &h.responses_server,
                vec![
                    call(
                        &call_id,
                        "exec_command",
                        json!({"cmd": command,
                    "workdir": root, "yield_time_ms": 1_000}),
                    ),
                    message(&format!("held-{n}"), "The check is running."),
                ],
            )
            .await;
            h.turn("Start the check and leave it running.").await?;
            barrier(&events.path().join(n.to_string()).join("ready")).await?;
        }
        let r1 = store.acceptance_ledger(&id).await?;
        assert_eq!(
            (r1.observed_executions, r1.pending_commands),
            (0, old_count)
        );
        assert_eq!(rows(&mut db, &h.run.id).await?, Vec::<String>::new());
        // Change only legal terms before any receipt; then freeze/readmit r2.
        let terms = match case {
            "argv" => json!({"checkCommand": format!("{command} revised")}),
            "cwd" => json!({"checkCwd": "other"}),
            _ if missing => json!({"checkCommand": command}),
            _ => json!({"expectedObservation": "The revised check exits zero."}),
        };
        if case == "cwd" {
            std::fs::create_dir(root.join("other"))?;
            std::fs::copy(root.join("verify.sh"), root.join("other/verify.sh"))?;
        }
        let mut refine = json!({"action":"refine", "criterion":"C1"});
        refine
            .as_object_mut()
            .unwrap()
            .extend(terms.as_object().unwrap().clone());
        if case == "cwd" {
            refine["checker"] = json!(["other/verify.sh"]);
        }
        responses::mount_sse_sequence(
            &h.responses_server,
            vec![
                call(
                    "refine",
                    "stateful_acceptance_update",
                    json!({
                "expectedLedgerRevision": r1.revision, "changes": [refine]}),
                ),
                admit("admit-r2", r1.revision + 1),
                message("revised", "Plan readmitted."),
            ],
        )
        .await;
        h.turn("Refine and readmit the check.").await?;
        let r2 = store.acceptance_ledger(&id).await?;
        assert!(r2.criteria[0].revision > r1.criteria[0].revision);
        assert!(r2.criteria[0].plan.is_some());
        let b_command = r2.criteria[0].check_command.as_ref().unwrap();
        let b_cwd = if case == "cwd" {
            root.join("other")
        } else {
            root.clone()
        };
        responses::mount_sse_sequence(
            &h.responses_server,
            vec![
                call(
                    "fresh",
                    "exec_command",
                    json!({"cmd": b_command,
                "workdir": b_cwd, "yield_time_ms": 1_000}),
                ),
                message("fresh-held", "The fresh check is running."),
            ],
        )
        .await;
        h.turn("Run the fresh admitted check.").await?;
        let b = events.path().join(old_count.to_string());
        barrier(&b.join("ready")).await?;
        std::fs::write(b.join("release"), "")?;
        let passed = accounted(&store, &id, 1, old_count).await?;
        let evidence = passed.criteria[0]
            .evidence
            .clone()
            .expect("durable B receipt");
        assert_eq!(evidence.outcome, runtime::EvidenceOutcome::Passed);
        assert_eq!(evidence.criterion_revision, r2.criteria[0].revision);
        let retained = rows(&mut db, &h.run.id).await?;
        assert_eq!(retained.len(), 1);
        for n in 0..old_count {
            let a = events.path().join(n.to_string());
            if matches!(case, "pinned" | "unpinned" | "argv" | "cwd") {
                std::fs::write(
                    a.join(if case == "pinned" {
                        "pinned"
                    } else {
                        "unpinned"
                    }),
                    "",
                )?;
            }
            if matches!(case, "failed" | "missing-fail") {
                std::fs::write(a.join("fail"), "")?;
            }
            std::fs::write(a.join("release"), "")?;
            let ended = accounted(&store, &id, n + 2, old_count - n - 1).await?;
            assert_eq!(ended.criteria[0].evidence, Some(evidence.clone()), "{case}");
            assert_eq!(rows(&mut db, &h.run.id).await?, retained, "{case}");
            assert_eq!(ended.revision, passed.revision, "{case}");
            let changed = case != "unchanged" && case != "failed";
            assert_eq!(
                ended.workspace_generation,
                passed.workspace_generation + u64::from(changed),
                "{case}"
            );
            if changed {
                assert!(matches!(runtime::criterion_verdict(&ended.criteria[0],
                    ended.workspace_generation, /*artifacts*/ None, /*checker*/ None),
                    runtime::CriterionVerdict::Unmet(reason) if reason.starts_with("stale:")));
            }
        }
        let before_restart = store.acceptance_ledger(&id).await?;
        assert!(h.server.shutdown_gracefully().await?.success());
        h.server = TestAppServer::builder()
            .with_codex_home(h._codex_home.path())
            .build_initialized()
            .await?;
        let _: codex_app_server_protocol::ThreadResumeResponse = h
            .server
            .request(|request_id| ClientRequest::ThreadResume {
                request_id,
                params: codex_app_server_protocol::ThreadResumeParams {
                    thread_id: h.thread_id.clone(),
                    ..Default::default()
                },
            })
            .await?;
        responses::mount_sse_once(&h.responses_server, message("cold", "Reentered.")).await;
        h.turn("Read the current run.").await?;
        let cold = store.acceptance_ledger(&id).await?;
        assert_eq!(cold.criteria[0].evidence, Some(evidence));
        assert_eq!(rows(&mut db, &h.run.id).await?, retained);
        assert_eq!(
            (
                cold.revision,
                cold.observed_executions,
                cold.pending_commands
            ),
            (
                before_restart.revision,
                before_restart.observed_executions,
                0
            )
        );
        assert_eq!(
            cold.workspace_generation,
            before_restart.workspace_generation + 1
        );
        assert!(
            matches!(runtime::criterion_verdict(&cold.criteria[0], cold.workspace_generation,
            /*artifacts*/ None, /*checker*/ None),
            runtime::CriterionVerdict::Unmet(reason) if reason.starts_with("stale:"))
        );
        responses::mount_sse_sequence(
            &h.responses_server,
            vec![
                call(
                    "post-reentry",
                    "exec_command",
                    json!({"cmd": b_command,
                "workdir": b_cwd, "yield_time_ms": 1_000}),
                ),
                message("post-reentry-held", "The new check is running."),
            ],
        )
        .await;
        h.turn("Obtain fresh evidence after reentry.").await?;
        let fresh = events.path().join((old_count + 1).to_string());
        barrier(&fresh.join("ready")).await?;
        std::fs::write(fresh.join("release"), "")?;
        let renewed = accounted(&store, &id, old_count + 2, 0).await?;
        let receipt = renewed.criteria[0].evidence.as_ref().expect("new receipt");
        let observed = |digest: &Option<String>| runtime::ArtifactState::Observed {
            digest: digest.clone().expect("readable pins"),
            missing: Vec::new(),
        };
        assert_eq!(receipt.outcome, runtime::EvidenceOutcome::Passed);
        assert_eq!(receipt.workspace_generation, renewed.workspace_generation);
        assert_eq!(
            runtime::criterion_verdict(
                &renewed.criteria[0],
                renewed.workspace_generation,
                Some(&observed(&receipt.artifact_digest)),
                Some(&observed(&receipt.checker_digest))
            ),
            runtime::CriterionVerdict::SatisfiedByHost
        );
    }
    Ok(())
}

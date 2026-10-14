use super::*;
use codex_stateful_runtime::NewObligation;
use codex_stateful_runtime::ObligationPacket;
use pretty_assertions::assert_eq;

/// A3, A8, A10: Autonomous redirects preserve the packet and never mutate acceptance.
#[tokio::test]
async fn autonomous_redirects_retain_raw_issues_without_acceptance_work() {
    let fixture = fixture("What is the parser?", WorkflowMode::Autonomous).await;
    let runtime = fixture.services.runtime().await.expect("runtime");
    let packet = ObligationPacket {
        learning: vec!["learning".into()],
        next: vec!["stale next".into()],
        blockers: vec!["model blocker".into()],
        ..Default::default()
    };
    runtime
        .append_obligation(
            "old".into(),
            NewObligation {
                project_id: PROJECT_ID.into(),
                run_id: fixture.run.id.clone(),
                packet: packet.clone(),
                provenance_source_id: "old".into(),
            },
        )
        .await
        .expect("packet");
    let ledger = fixture.ledger().await;
    for attempt in 0..5 {
        let mut call = call(
            "stateful_run_update",
            &json!({
                "expectedRevision": 1, "status": "completed", "openIssues": [" x "]
            }),
        );
        call.call_id = format!("redirect-{attempt}");
        let output = fixture.completion.handle(call).await.expect("redirect");
        let output: Value = serde_json::from_str(&output.log_output()).expect("json");
        assert_eq!(output["status"], "running");
        assert_eq!(
            output["instruction"].as_str().expect("instruction"),
            "This run declared no acceptance criteria; verified Completion is unavailable. Give a substantive final answer; an eligible final ends Answered or Partial. Disclose unresolved items for Partial. No completion call is required; pending steering and final-answer gates still apply."
        );
    }
    assert_eq!(fixture.stored_run().await, fixture.run);
    assert_eq!(fixture.ledger().await, ledger);
    assert_eq!(
        runtime
            .latest_obligation(&fixture.run.id)
            .await
            .expect("latest")
            .expect("packet")
            .value
            .packet,
        ObligationPacket {
            blockers: vec!["model blocker".into(), "Open issue (retained): x".into()],
            ..packet
        }
    );
}

/// A9, A10: refused Autonomous completions never trigger the Collaborative Blocked latch.
#[tokio::test]
async fn autonomous_refusals_keep_running_then_final_answers_partial() {
    let fixture = fixture("Fix the parser.", WorkflowMode::Autonomous).await;
    fixture
        .update(
            0,
            json!([{
                "action": "add", "origin": "derived", "kind": "manual", "statement": "Parser fixed."
            }]),
        )
        .await
        .expect("criterion");
    for _ in 0..3 {
        let refusal = fixture.complete().await.expect_err("refused");
        assert!(refusal.contains("completion refused"), "{refusal}");
        assert!(
            refusal.contains("an eligible final ends Answered or Partial"),
            "{refusal}"
        );
        assert!(!refusal.contains("\"status\":\"completed\""), "{refusal}");
        assert_eq!(
            fixture.stored_run().await.status,
            StatefulRunStatus::Running
        );
    }
    let runtime = fixture.services.runtime().await.expect("runtime");
    let outcome = runtime
        .end_run_answered(
            &codex_stateful_runtime::AnsweredRunCommit {
                run_id: fixture.run.id.clone(),
                thread_id: THREAD_ID.into(),
                turn_id: "turn-1".into(),
                answer: "The parser is fixed.".into(),
                result: "The parser is fixed.".into(),
                trailer_issues: vec![],
            },
            &|| true,
        )
        .await
        .expect("final");
    let codex_stateful_runtime::AnsweredRunOutcome::Answered(_, record) = outcome else {
        panic!("answer");
    };
    assert_eq!(
        record.outcome(),
        codex_stateful_runtime::HostAnswerOutcome::Partial
    );
}

/// G2: syntax/disposition help is mode-correct and supplies no evidence or eligibility.
#[tokio::test]
async fn completion_shapes_are_conditional() {
    for mode in [
        WorkflowMode::Autonomous,
        WorkflowMode::Collaborative,
        WorkflowMode::Socratic,
    ] {
        let f = fixture("Answer a lookup.", mode).await;
        if mode == WorkflowMode::Autonomous {
            f.update(0, json!([{"action":"add", "origin":"derived", "kind":"manual", "statement":"Answer a lookup."}])).await.expect("contract");
        }
        for arguments in [
            json!({"expectedRevision":1,"status":"completed","result":"42", "completionDisposition":"noReusableLearning","rootRevision":0}),
            json!({"expectedRevision":1,"status":"completed","result":"42", "completionDisposition":"durableLearning"}),
        ] {
            let refusal = f.complete_with(arguments).await.expect_err("refused");
            let (_, shapes) = refusal
                .split_once(". Argument shapes only;")
                .expect("conditional shapes");
            assert!(shapes.contains("only when this run wrote no project knowledge"));
            assert!(shapes.contains("durableLearning"));
            let lookup = shapes
                .split("noReusableLearning (only when this run wrote no project knowledge): ")
                .nth(1)
                .expect("lookup shape")
                .split(';')
                .next()
                .expect("JSON");
            let lookup: Value = serde_json::from_str(lookup).expect("copyable JSON");
            assert_eq!(
                lookup,
                json!({"expectedRevision":1,"status":"completed",
            "completionDisposition":"noReusableLearning","result":"<final answer>","openIssues":[]})
            );
            assert_eq!(
                shapes.split(" Replace").next().expect("label"),
                " verification and terminal gates still apply."
            );
            assert_eq!(f.stored_run().await, f.run);
        }
    }
}

/// Decode help must preserve readable disclosures and the small-budget final action.
#[tokio::test]
async fn malformed_completion_guidance_preserves_issues_and_small_budget_action() {
    for disclosure in [
        json!({"openIssues":["unresolved defect"]}),
        json!({"finalObligation":{"blockers":["unresolved defect"]}}),
    ] {
        let f = fixture("Fix the parser.", WorkflowMode::Autonomous).await;
        f.update(0, json!([{"action":"add","origin":"derived","kind":"manual","statement":"Parser fixed."}])).await.expect("contract");
        let mut arguments = json!({"expectedRevision":1,"status":"done"});
        arguments
            .as_object_mut()
            .expect("object")
            .extend(disclosure.as_object().expect("object").clone());
        let refusal = f.complete_with(arguments).await.expect_err("malformed");
        assert!(refusal.contains("an eligible final ends Answered or Partial"));
        assert!(refusal.contains(
            "[stateful-outcome]\ndisposition: answer\nopen-issues: none\n[/stateful-outcome]"
        ));
        assert!(!refusal.contains("\"status\":\"completed\""));
        assert_eq!(f.stored_run().await, f.run);
    }
    let f = fixture("Answer a lookup.", WorkflowMode::Autonomous).await;
    let mut call = call(
        "stateful_run_update",
        &json!({"expectedRevision":1,"status":"done"}),
    );
    call.truncation_policy = TruncationPolicy::Bytes(240);
    let error = f.completion.handle(call).await.err().expect("malformed");
    assert!(
        error
            .to_string()
            .contains("an eligible final ends Answered or Partial")
    );
    assert!(
        serde_json::to_string(&error.to_string())
            .expect("json")
            .len()
            <= 240
    );
}

/// A readable run with a broken ledger exercises the preliminary read error boundary.
#[tokio::test]
async fn preliminary_store_read_failure_is_bounded() {
    let f = fixture("Answer a lookup.", WorkflowMode::Autonomous).await;
    let sqlite = f.services.sqlite();
    let pool = sqlite
        .open_read_write_pool(&sqlite.home().join("stateful_runtime_1.sqlite"))
        .await
        .expect("fixture connection");
    pool.begin_with("BEGIN; DROP TABLE stateful_acceptance_ledgers;")
        .await
        .expect("break ledger")
        .commit()
        .await
        .expect("commit fixture");
    assert_eq!(f.stored_run().await, f.run);
    for budget in [20, 64, 240, 1_000] {
        let mut call = call(
            "stateful_run_update",
            &json!({"expectedRevision":1,"status":"completed"}),
        );
        call.truncation_policy = TruncationPolicy::Bytes(budget);
        let allowance = call.response_byte_budget(crate::tools::MAX_RESPONSE_BYTES);
        let error = f
            .completion
            .handle(call)
            .await
            .err()
            .expect("failed store read");
        assert!(
            serde_json::to_string(&error.to_string())
                .expect("json")
                .len()
                <= allowance
        );
    }
}

/// A7, B5: required host-check completion needs an admitted receipt, and retained issues
/// still veto the terminal transaction even when every criterion passes.
#[tokio::test]
async fn completed_stays_evidence_bound_and_retained_issues_cannot_be_hidden() {
    use codex_stateful_runtime::AcceptanceChange;
    use codex_stateful_runtime::AcceptanceCommit;
    use codex_stateful_runtime::AcceptanceKind;
    use codex_stateful_runtime::ArtifactState;
    use codex_stateful_runtime::CriterionTerms;
    use codex_stateful_runtime::StatefulRunUpdate;
    use codex_stateful_runtime::VerificationClaim;
    for retained in [false, true] {
        let f = fixture("Fix the parser.", WorkflowMode::Autonomous).await;
        let runtime = f.services.runtime().await.expect("runtime");
        std::fs::write(f._state_home.path().join("verify.py"), "print('checked')")
            .expect("checker file");
        let ledger = runtime
            .revise_acceptance(
                &f.run.id,
                /*expected_revision*/ 0,
                vec![
                    AcceptanceChange::Add {
                        origin: AcceptanceOrigin::User,
                        kind: AcceptanceKind::Check,
                        statement: "Parser fixed.".into(),
                        request_span: Some(RequestSpan { start: 0, end: 15 }),
                        terms: CriterionTerms {
                            required: true,
                            artifacts: vec!["out.txt".into()],
                            checker: vec!["verify.py".into()],
                            check_command: Some("python verify.py".into()),
                            expected_observation: Some("exit 0".into()),
                            ..Default::default()
                        },
                    },
                    AcceptanceChange::Admit {
                        ordinal: 1,
                        checker_digest: Some("sha256:checker".into()),
                    },
                ],
                "admit",
                Some(f._state_home.path().canonicalize().expect("canonical root")),
            )
            .await
            .expect("admitted");
        let current = f.stored_run().await;
        let observed = |digest: &str| {
            std::collections::BTreeMap::from([(
                1,
                ArtifactState::Observed {
                    digest: digest.into(),
                    missing: vec![],
                },
            )])
        };
        let attempt = runtime
            .begin_verification(&f.run.id, "test", /*lease_duration_ms*/ 60_000)
            .await
            .expect("attempt");
        let commit = AcceptanceCommit {
            ledger_revision: ledger.revision,
            workspace_generation: ledger.workspace_generation,
            artifacts: observed("sha256:artifact"),
            checkers: observed("sha256:checker"),
            verification: VerificationClaim {
                owner: "test".into(),
                attempt,
            },
            validated_obligation_sequence: None,
        };
        let update = StatefulRunUpdate {
            expected_revision: current.revision,
            status: StatefulRunStatus::Completed,
            strategy: None,
            result: Some("Parser fixed.".into()),
        };
        assert!(
            runtime
                .complete_run_with_acceptance(
                    &f.run.id,
                    update.clone(),
                    &commit,
                    /*final_obligation*/ None
                )
                .await
                .is_err()
        );
        runtime
            .end_verification(&f.run.id, "test", attempt)
            .await
            .expect("end");
        let criterion = ledger.criterion(1).expect("criterion");
        runtime
            .record_command_evidence(
                &f.run.id,
                vec![CommandEvidence {
                    ordinal: 1,
                    criterion_revision: criterion.revision,
                    outcome: EvidenceOutcome::Passed,
                    command: "python verify.py".into(),
                    exit_code: Some(0),
                    output_tail: String::new(),
                    output_digest: "sha256:output".into(),
                    artifact_digest: Some("sha256:artifact".into()),
                    checker_digest: Some("sha256:checker".into()),
                    detail: None,
                    start_generation: ledger.workspace_generation,
                    source_id: "check".into(),
                }],
            )
            .await
            .expect("receipt");
        if retained {
            let output = f
                .complete_with(
                    json!({"expectedRevision":1,"status":"completed","openIssues":["x"]}),
                )
                .await
                .expect("retain");
            assert_eq!(output["status"], "running");
            f.complete()
                .await
                .expect("empty openIssues redirects but cannot erase");
        }
        let ledger = f.ledger().await;
        let attempt = runtime
            .begin_verification(&f.run.id, "test", /*lease_duration_ms*/ 60_000)
            .await
            .expect("attempt");
        let latest = runtime.latest_obligation(&f.run.id).await.expect("latest");
        let commit = AcceptanceCommit {
            ledger_revision: ledger.revision,
            verification: VerificationClaim {
                owner: "test".into(),
                attempt,
            },
            validated_obligation_sequence: latest.map(|row| row.sequence),
            ..commit
        };
        let outcome = runtime
            .complete_run_with_acceptance(
                &f.run.id, update, &commit, /*final_obligation*/ None,
            )
            .await;
        if retained {
            assert!(outcome.is_err());
            assert_eq!(f.stored_run().await.status, StatefulRunStatus::Running);
        } else {
            assert_eq!(
                outcome.expect("verified").0.status,
                StatefulRunStatus::Completed
            );
        }
    }
}

/// The live model tool uses receipts, explicit resolutions and submitted-next attribution.
#[tokio::test]
async fn model_updates_resolve_retained_issues_and_only_successful_next_guards_the_turn() {
    let f = fixture("Fix the parser.", WorkflowMode::Autonomous).await;
    let runtime = f.services.runtime().await.expect("runtime");
    f.complete_with(json!({"expectedRevision":1,"status":"completed","openIssues":["x"]}))
        .await
        .expect("retain");
    let progress = crate::turn_progress::TurnProgress::default();
    let tool = crate::tools::obligation::ObligationUpdateTool::new(
        PROJECT_ID.into(),
        THREAD_ID.into(),
        f.services.clone(),
        /*event_sink*/ None,
        progress.clone(),
    );
    let first = call(
        "obligation_update",
        &json!({
            "idempotencyKey":"work", "packet":{"learning":["Found it."],"next":["Fix it."]}
        }),
    );
    tool.handle(first).await.expect("update");
    assert_eq!(progress.take("turn-1"), Some(f.run.id.clone()));
    assert_eq!(
        runtime
            .latest_obligation(&f.run.id)
            .await
            .expect("latest")
            .expect("packet")
            .value
            .packet
            .blockers,
        vec!["Open issue (retained): x"]
    );
    tool.handle(call("obligation_update", &json!({
        "idempotencyKey":"bad", "packet":{"blockers":["Open issue (retained): fake"],"next":["More work"]}
    }))).await.err().expect("reserved prefix refused");
    assert_eq!(progress.take("turn-1"), None);
    let mut tiny = call(
        "obligation_update",
        &json!({
            "idempotencyKey":"no-receipt", "packet":{"learning":["Updated."],"next":["More work"]}
        }),
    );
    tiny.truncation_policy = TruncationPolicy::Bytes(10);
    assert!(tool.handle(tiny).await.is_err());
    assert_eq!(progress.take("turn-1"), None);
    tool.handle(call(
        "obligation_update",
        &json!({
            "idempotencyKey":"resolved", "packet":{}, "resolvedIssues":["x"]
        }),
    ))
    .await
    .expect("resolution alone");
    assert_eq!(progress.take("turn-1"), None);
    let packet = runtime
        .latest_obligation(&f.run.id)
        .await
        .expect("latest")
        .expect("packet")
        .value
        .packet;
    assert_eq!(
        packet,
        ObligationPacket {
            changed: vec!["Resolved: x".into()],
            ..Default::default()
        }
    );
}

/// A supported 16-issue retention stays Ok and bounded; a too-small receipt refuses before
/// writing, rather than reporting an error after a durable retention.
#[tokio::test]
async fn retention_receipts_pack_large_lists_and_preflight_tiny_budgets() {
    let f = fixture("What is the parser?", WorkflowMode::Autonomous).await;
    let issues: Vec<_> = (0..16)
        .map(|index| format!("{index:02}{}", "x".repeat(1_022)))
        .collect();
    let args = json!({"expectedRevision":1,"status":"completed","openIssues":issues});
    let mut tiny = call("stateful_run_update", &args);
    tiny.truncation_policy = TruncationPolicy::Bytes(200);
    assert!(f.completion.handle(tiny).await.is_err());
    let runtime = f.services.runtime().await.expect("runtime");
    assert_eq!(
        runtime.latest_obligation(&f.run.id).await.expect("latest"),
        None
    );
    let output = f
        .completion
        .handle(call("stateful_run_update", &args))
        .await
        .expect("bounded Ok");
    let text = output.log_output();
    assert!(text.len() <= 900);
    let output: Value = serde_json::from_str(&text).expect("json");
    assert_eq!(output["status"], "running");
    let kept = output["retainedIssues"].as_array().expect("list").len();
    assert_eq!(
        kept + output["omittedRetainedIssues"].as_u64().expect("count") as usize,
        16
    );
    let recorded = runtime
        .latest_obligation(&f.run.id)
        .await
        .expect("latest")
        .expect("packet");
    assert_eq!(
        recorded.value.packet.blockers,
        issues
            .iter()
            .map(|issue| format!("Open issue (retained): {issue}"))
            .collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn a_failed_model_receipt_does_not_record_turn_progress() {
    let f = fixture("Fix the parser.", WorkflowMode::Autonomous).await;
    let progress = crate::turn_progress::TurnProgress::default();
    let tool = crate::tools::obligation::ObligationUpdateTool::new(
        PROJECT_ID.into(),
        THREAD_ID.into(),
        f.services.clone(),
        /*event_sink*/ None,
        progress.clone(),
    );
    let mut tiny = call(
        "obligation_update",
        &json!({
            "idempotencyKey":"quota", "packet":{"learning":["Found it."],"next":["Fix it."]}
        }),
    );
    tiny.truncation_policy = TruncationPolicy::Bytes(10);
    assert!(tool.handle(tiny).await.is_err());
    assert_eq!(progress.take("turn-1"), None);
    // The receipt-output failure is not a successful model call, even if storage committed.
    let runtime = f.services.runtime().await.expect("runtime");
    let row = runtime
        .latest_obligation(&f.run.id)
        .await
        .expect("latest")
        .expect("committed");
    assert_eq!(
        row.value.packet,
        ObligationPacket {
            learning: vec!["Found it.".into()],
            next: vec!["Fix it.".into()],
            ..Default::default()
        }
    );
}

#[tokio::test]
async fn oversized_unknown_resolutions_refuse_with_bounded_diagnostics() {
    let f = fixture("Fix the parser.", WorkflowMode::Autonomous).await;
    let progress = crate::turn_progress::TurnProgress::default();
    let tool = crate::tools::obligation::ObligationUpdateTool::new(
        PROJECT_ID.into(),
        THREAD_ID.into(),
        f.services.clone(),
        /*event_sink*/ None,
        progress.clone(),
    );
    let rejected = call(
        "obligation_update",
        &json!({
            "idempotencyKey":"unknown", "packet":{}, "resolvedIssues":["\"λ\n".repeat(32_000)]
        }),
    );
    let error = tool
        .handle(rejected)
        .await
        .err()
        .expect("unknown resolution");
    let FunctionCallError::RespondToModel(message) = error else {
        panic!("model diagnostic")
    };
    assert!(serde_json::to_string(&message).expect("encoded").len() <= 900);
    assert_eq!(progress.take("turn-1"), None);
    let runtime = f.services.runtime().await.expect("runtime");
    assert_eq!(
        runtime.latest_obligation(&f.run.id).await.expect("latest"),
        None
    );
}

/// Completion-only blockers survive redirects, independent of openIssues and contracts.
#[tokio::test]
async fn redirected_final_blockers_survive_into_the_answer_and_disclosure() {
    use codex_stateful_runtime::AnsweredRunCommit;
    use codex_stateful_runtime::AnsweredRunOutcome;
    use codex_stateful_runtime::HostAnswerOutcome;
    for contract in [false, true] {
        for issues in [json!([]), json!(["y"])] {
            let f = fixture("Fix the parser.", WorkflowMode::Autonomous).await;
            if contract {
                f.update(0, json!([{
                    "action":"add", "origin":"derived", "kind":"manual", "statement":"Parser fixed."
                }])).await.expect("contract");
            }
            let args = json!({
                "expectedRevision":1, "status":"completed", "openIssues":issues,
                "finalObligation":{"blockers":["x"]}
            });
            for _ in 0..2 {
                let receipt = f
                    .complete_with(args.clone())
                    .await
                    .expect("redirect and replay");
                assert!(receipt.to_string().len() <= 900);
                if contract {
                    assert_eq!(
                        receipt["instruction"].as_str().expect("instruction"),
                        "Open issues or blockers remain; verified Completion is unavailable. Give a substantive final answer; an eligible final ends Answered or Partial. Disclose unresolved items for Partial. No completion call is required; pending steering and final-answer gates still apply."
                    );
                }
            }
            let runtime = f.services.runtime().await.expect("runtime");
            let packet = runtime
                .latest_obligation(&f.run.id)
                .await
                .expect("read")
                .expect("packet");
            let mut blockers = if issues == json!([]) {
                vec![]
            } else {
                vec!["Open issue (retained): y".to_string()]
            };
            blockers.push("Open issue (retained): x".into());
            assert_eq!(
                packet.value.packet,
                ObligationPacket {
                    blockers,
                    ..Default::default()
                }
            );
            let commit = AnsweredRunCommit {
                run_id: f.run.id.clone(),
                thread_id: THREAD_ID.into(),
                turn_id: "turn-1".into(),
                answer: "The parser returns tokens.".into(),
                result: "The parser returns tokens.".into(),
                trailer_issues: vec![],
            };
            let outcome = runtime
                .end_run_answered(&commit, &|| true)
                .await
                .expect("final");
            let AnsweredRunOutcome::Answered(run, record) = &outcome else {
                panic!("answer")
            };
            assert_eq!(
                (run.status, record.outcome(), run.continuations_used),
                (StatefulRunStatus::Answered, HostAnswerOutcome::Partial, 0)
            );
            assert!(
                run.result
                    .as_deref()
                    .expect("result")
                    .contains("- Open issue (retained): x\n")
            );
            let disclosure = crate::answered::resolve(
                runtime,
                None,
                &commit,
                /*authorized*/ true,
                Ok(outcome),
            )
            .await;
            let codex_extension_api::TurnFinalizeOutcome::Warning(warning) = disclosure else {
                panic!("disclosure")
            };
            assert!(warning.contains("Open issue (retained): x"));
        }
    }
}

/// The combined final blocker/open issue packet is bounded atomically, with raw identities.
#[tokio::test]
async fn redirected_final_blockers_refuse_overflow_without_retaining_anything() {
    for blockers in [
        (0..32).map(|i| format!("blocker-{i}")).collect::<Vec<_>>(),
        vec!["x".repeat(8_192)],
        vec!["Open issue (retained): forged".into()],
    ] {
        let f = fixture("Fix the parser.", WorkflowMode::Autonomous).await;
        f.complete_with(json!({
            "expectedRevision":1, "status":"completed", "openIssues":["y"],
            "finalObligation":{"blockers":blockers}
        }))
        .await
        .expect_err("whole refusal");
        let runtime = f.services.runtime().await.expect("runtime");
        assert_eq!(
            runtime.latest_obligation(&f.run.id).await.expect("read"),
            None
        );
        assert_eq!(f.stored_run().await, f.run);
    }
}

/// Final blockers retain obligation capacity rather than inheriting openIssues' smaller caps.
#[tokio::test]
async fn redirected_final_blockers_keep_packet_capacity() {
    let f = fixture("Fix the parser.", WorkflowMode::Autonomous).await;
    let blockers: Vec<_> = (0..17)
        .map(|i| format!("{i:02}{}", "x".repeat(1_024)))
        .collect();
    let output = f
        .complete_with(json!({
            "expectedRevision":1, "status":"completed", "openIssues":[],
            "finalObligation":{"blockers":blockers}
        }))
        .await
        .expect("packet capacity");
    assert_eq!(output["status"], "running");
    let runtime = f.services.runtime().await.expect("runtime");
    let recorded = runtime
        .latest_obligation(&f.run.id)
        .await
        .expect("read")
        .expect("packet");
    assert_eq!(
        recorded.value.packet,
        ObligationPacket {
            blockers: blockers
                .iter()
                .map(|blocker| format!("Open issue (retained): {blocker}"))
                .collect(),
            ..Default::default()
        }
    );
}

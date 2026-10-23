use super::*;
use codex_app_server_protocol::StatefulHostAnswer;
use codex_app_server_protocol::StatefulHostAnswerOutcome;
use pretty_assertions::assert_eq;
use runtime::PARTIAL_RESULT_HEADER;
use runtime::PARTIAL_UNVERIFIED_BASIS;
use serde_json::json;

pub(super) fn tool(id: &str, name: &str, args: serde_json::Value) -> String {
    responses::sse(vec![
        responses::ev_response_created(id),
        responses::ev_function_call(id, name, &args.to_string()),
        responses::ev_completed(id),
    ])
}

pub(super) fn progress() -> String {
    tool(
        "progress",
        "obligation_update",
        json!({
            "idempotencyKey": "progress",
            "packet": {"learning": ["Found the parser."], "next": ["Read it next."]}
        }),
    )
}

impl Harness {
    pub(super) async fn store(&self) -> Result<runtime::StatefulRunStore> {
        Ok(
            runtime::StatefulRunStore::open(&SqliteConfig::new_for_testing(
                self.codex_home.path().abs(),
            ))
            .await?,
        )
    }

    /// Assert the entire public object, supplying only the server's clock.
    pub(super) async fn assert_terminal(
        &mut self,
        started: StatefulRun,
        turn_id: &str,
        (final_text, body): (&str, &str),
        items: &[String],
        requests: usize,
    ) -> Result<()> {
        let read = self.read(&started.id).await?;
        let current = read.run.as_ref().expect("run");
        let record = read.host_answer.as_ref().expect("host answer");
        let partial = !items.is_empty();
        let result = if partial {
            format!(
                "{PARTIAL_RESULT_HEADER}{}\n{body}",
                items
                    .iter()
                    .map(|item| format!("- {item}\n"))
                    .collect::<String>()
            )
        } else {
            body.into()
        };
        let expected = StatefulRunReadResponse {
            run: Some(StatefulRun {
                status: StatefulRunStatus::Answered,
                result: Some(result),
                revision: started.revision + 1,
                updated_at: current.updated_at,
                ..started
            }),
            host_answer: Some(StatefulHostAnswer {
                turn_id: turn_id.into(),
                answer: final_text.into(),
                basis: if partial {
                    PARTIAL_UNVERIFIED_BASIS
                } else {
                    ANSWERED_UNVERIFIED_BASIS
                }
                .into(),
                committed_at: record.committed_at,
                outcome: if partial {
                    StatefulHostAnswerOutcome::Partial
                } else {
                    StatefulHostAnswerOutcome::Answered
                },
            }),
            recovery: Some(codex_app_server_protocol::StatefulRunRecovery {
                lease_expires_at: None,
                previous_turn_id: None,
                last_continuation_claimed_at: None,
            }),
        };
        assert_eq!(read, expected);
        assert_eq!(
            self.responses_server
                .received_requests()
                .await
                .unwrap_or_default()
                .len(),
            requests
        );
        Ok(())
    }

    async fn claimed(&mut self, id: &str) -> Result<StatefulRunReadResponse> {
        tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                let read = self.read(id).await?;
                if read
                    .run
                    .as_ref()
                    .is_some_and(|run| run.continuations_used == 1)
                {
                    return Ok(read);
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await?
    }
}

/// A1, A4, M4: trailer-free and structurally malformed answers deliver; issues disclose.
async fn delivery(text: String, items: Vec<String>, with_progress: bool) -> Result<()> {
    let mut h = harness().await?;
    let run = h.start_run(StatefulWorkflowMode::Autonomous).await?;
    let mut script = if with_progress {
        vec![progress()]
    } else {
        vec![]
    };
    script.push(message("final", &text));
    let requests = script.len();
    responses::mount_sse_sequence(&h.responses_server, script).await;
    let id = h.start_turn().await?;
    let (status, warnings, _) = h.outcome(&id, /*next*/ false).await?;
    assert_eq!(status, TurnStatus::Completed);
    assert_eq!(warnings.len(), usize::from(!items.is_empty()));
    if !items.is_empty() {
        assert!(warnings[0].contains("Open or not certified: y"));
    }
    h.assert_terminal(run, &id, (&text, ANSWER), &items, requests)
        .await?;
    Ok(())
}

/// A3, A8, A9, A5: redirected completion never verifies, stalls never block.
async fn completion(contract: bool, issues: Vec<&str>, attempts: usize) -> Result<()> {
    let mut h = harness().await?;
    let run = h.start_run(StatefulWorkflowMode::Autonomous).await?;
    if contract && attempts == 1 {
        super::super::stateful_acceptance_support::seed_admitted_plan(
            h.codex_home.path(),
            &run.id,
            &[],
            h.project_root.path(),
        )
        .await?;
    } else if contract {
        h.store()
            .await?
            .revise_acceptance(
                &runtime::StatefulRunId::parse(&run.id)?,
                /*expected_revision*/ 0,
                vec![runtime::AcceptanceChange::Add {
                    origin: runtime::AcceptanceOrigin::Derived,
                    kind: runtime::AcceptanceKind::Manual,
                    statement: "The request is answered.".into(),
                    request_span: None,
                    terms: runtime::CriterionTerms {
                        required: true,
                        ..Default::default()
                    },
                }],
                "derived",
                /*project_root*/ None,
            )
            .await?;
    }
    let before = h
        .store()
        .await?
        .acceptance_ledger(&runtime::StatefulRunId::parse(&run.id)?)
        .await?;
    let mut script: Vec<_> = (0..attempts)
        .map(|i| run_update(&format!("complete-{i}"), "completed", &issues))
        .collect();
    let text = if contract && attempts == 1 {
        ready()
    } else {
        ANSWER.into()
    };
    script.push(message("final", &text));
    let mock = responses::mount_sse_sequence(&h.responses_server, script).await;
    let id = h.start_turn().await?;
    let (_, warnings, _) = h.outcome(&id, /*next*/ false).await?;
    let items = if contract {
        vec![
            "C1 (The request is answered.): not certified by this terminal answer path".to_string(),
        ]
    } else {
        issues
            .iter()
            .map(|x| format!("Open issue (retained): {x}"))
            .collect()
    };
    assert_eq!(warnings.len(), usize::from(!items.is_empty()));
    h.assert_terminal(run.clone(), &id, (&text, ANSWER), &items, attempts + 1)
        .await?;
    if !contract {
        assert_eq!(
            h.store()
                .await?
                .acceptance_ledger(&runtime::StatefulRunId::parse(&run.id)?)
                .await?,
            before
        );
        for (attempt, request) in mock.requests().iter().skip(1).enumerate() {
            let receipt = request
                .function_call_output_text(&format!("complete-{attempt}"))
                .expect("completion receipt");
            let receipt: serde_json::Value = serde_json::from_str(&receipt)?;
            assert_eq!(receipt["runId"], run.id);
            assert_eq!(receipt["status"], "running");
            assert_eq!(receipt["revision"], run.revision);
            assert!(
                receipt["instruction"]
                    .as_str()
                    .expect("instruction")
                    .contains("verified Completion is unavailable")
            );
        }
    }
    Ok(())
}

/// A2, M1, M3: only model progress of this turn guards a plain final; status warns once.
async fn running(last: String, model_progress: bool, warning: bool) -> Result<()> {
    let mut h = harness().await?;
    let run = h.start_run(StatefulWorkflowMode::Autonomous).await?;
    let mut script = if model_progress {
        vec![progress()]
    } else {
        vec![]
    };
    script.push(message("final", &last));
    let first_requests = script.len();
    script.push(message("held", "Done."));
    let release = mount_gated(&h.responses_server, script, first_requests).await;
    let id = h.start_turn().await?;
    let first = h.outcome(&id, /*next*/ false).await?;
    let read = h.claimed(&run.id).await?;
    let current = read.run.as_ref().expect("running").clone();
    let recovery = read.recovery.as_ref().expect("recovery").clone();
    assert!(recovery.last_continuation_claimed_at.is_some());
    assert_eq!(
        read,
        StatefulRunReadResponse {
            run: Some(StatefulRun {
                revision: run.revision + 1,
                continuations_used: 1,
                updated_at: current.updated_at,
                ..run
            }),
            host_answer: None,
            recovery: Some(codex_app_server_protocol::StatefulRunRecovery {
                previous_turn_id: Some(id.clone()),
                ..recovery
            }),
        }
    );
    assert_eq!(
        first,
        (
            TurnStatus::Completed,
            if warning {
                vec![STATUS_ONLY.into()]
            } else {
                vec![]
            },
            first_requests
        )
    );
    release.send(())?;
    let second = h.outcome(&id, /*next*/ true).await?;
    assert_eq!(
        second,
        (
            TurnStatus::Completed,
            if warning {
                vec![]
            } else {
                vec![STATUS_ONLY.into()]
            },
            first_requests + 1
        )
    );
    Ok(())
}

macro_rules! witness {
    ($name:ident, $case:expr) => {
        #[tokio::test]
        async fn $name() -> Result<()> {
            $case.await
        }
    };
}

witness!(
    a1_plain_answer,
    delivery(ANSWER.into(), vec![], /*with_progress*/ false)
);
witness!(
    a4_trailer_issues,
    delivery(
        format!(
            "{ANSWER}\n{}",
            BLOCK.replace("open-issues: none", "open-issues:\n- y")
        ),
        vec!["y".into()],
        /*with_progress*/ false
    )
);
witness!(
    m4_malformed_delivery,
    delivery(
        format!("{ANSWER}\n[stateful-outcome]\ndisposition: answer\nbroken"),
        vec![],
        /*with_progress*/ false
    )
);
witness!(
    m1_valid_answer_overrides_progress,
    delivery(ready(), vec![], /*with_progress*/ true)
);
witness!(
    a3_retained_redirect,
    completion(/*contract*/ false, vec!["x"], /*attempts*/ 1)
);
witness!(
    a8_five_no_contract_redirects,
    completion(/*contract*/ false, vec![], /*attempts*/ 5)
);
witness!(
    a5_active_user_criterion,
    completion(/*contract*/ true, vec![], /*attempts*/ 1)
);
witness!(
    a9_three_derived_refusals,
    completion(/*contract*/ true, vec![], /*attempts*/ 3)
);
witness!(
    a2_plain_status_only,
    running(
        "Task completed.".into(),
        /*model_progress*/ false,
        /*warning*/ true
    )
);
witness!(
    a2_none_trailer_status_only,
    running(
        format!("Done.\n{BLOCK}"),
        /*model_progress*/ false,
        /*warning*/ true
    )
);
witness!(
    a2_issues_trailer_status_only,
    running(
        format!(
            "Done.\n{}",
            BLOCK.replace("open-issues: none", "open-issues:\n- tests remain")
        ),
        /*model_progress*/ false,
        /*warning*/ true
    )
);
witness!(
    m1_model_next_guards_progress_final,
    running(
        "I will read it next.".into(),
        /*model_progress*/ true,
        /*warning*/ false
    )
);
witness!(
    m3_valid_continue,
    running(
        format!(
            "Progress.\n{}",
            BLOCK.replace("disposition: answer", "disposition: continue")
        ),
        /*model_progress*/ false,
        /*warning*/ false
    )
);
witness!(
    m3_malformed_continue,
    running(
        "Progress.\n[stateful-outcome]\n disposition: continue\nbad".into(),
        /*model_progress*/ false,
        /*warning*/ false
    )
);
witness!(
    m3_malformed_blocked,
    running(
        "Progress.\n[stateful-outcome]\n disposition: blocked\nbad".into(),
        /*model_progress*/ false,
        /*warning*/ false
    )
);

/// Final-obligation blockers must survive every Autonomous redirect shape on the wire.
#[tokio::test]
async fn completion_final_blockers_disclose_with_and_without_contract_or_open_issues() -> Result<()>
{
    for contract in [false, true] {
        for open_issues in [vec![], vec!["y"]] {
            let mut h = harness().await?;
            let run = h.start_run(StatefulWorkflowMode::Autonomous).await?;
            if contract {
                super::super::stateful_acceptance_support::seed_admitted_plan(
                    h.codex_home.path(),
                    &run.id,
                    &[],
                    h.project_root.path(),
                )
                .await?;
            }
            let mock = responses::mount_sse_sequence(&h.responses_server, vec![
                tool("complete", "stateful_run_update", json!({
                    "expectedRevision":run.revision, "status":"completed", "openIssues":open_issues,
                    "finalObligation":{"blockers":["x"]}
                })),
                message("final", ANSWER),
            ]).await;
            let turn = h.start_turn().await?;
            let (status, warnings, requests) = h.outcome(&turn, /*next*/ false).await?;
            assert_eq!((status, requests), (TurnStatus::Completed, 2));
            assert_eq!(warnings.len(), 1);
            assert!(warnings[0].contains("Open issue (retained): x"));
            let receipt = mock.requests()[1]
                .function_call_output_text("complete")
                .expect("receipt");
            let receipt: serde_json::Value = serde_json::from_str(&receipt)?;
            assert_eq!(receipt["status"], "running");
            let mut items = if contract {
                vec![
                    "C1 (The request is answered.): not certified by this terminal answer path"
                        .into(),
                ]
            } else {
                vec![]
            };
            if !open_issues.is_empty() {
                items.push("Open issue (retained): y".into());
            }
            items.push("Open issue (retained): x".into());
            h.assert_terminal(run, &turn, (ANSWER, ANSWER), &items, 2)
                .await?;
        }
    }
    Ok(())
}

/// Duplicate answer trailers are structurally malformed: deliver before the last marker
/// without model progress, but preserve the frozen progress guard when next was recorded.
#[tokio::test]
async fn repeated_identical_answer_trailers_deliver_without_progress() -> Result<()> {
    let mut h = harness().await?;
    let run = h.start_run(StatefulWorkflowMode::Autonomous).await?;
    let body = format!("{ANSWER}\n{BLOCK}");
    let text = format!("{body}\n{BLOCK}");
    responses::mount_sse_once(&h.responses_server, message("final", &text)).await;
    let turn = h.start_turn().await?;
    assert_eq!(
        h.outcome(&turn, /*next*/ false).await?,
        (TurnStatus::Completed, vec![], 1)
    );
    h.assert_terminal(run, &turn, (&text, &body), &[], 1)
        .await?;
    Ok(())
}

witness!(
    repeated_identical_answer_trailers_with_progress_keep_running,
    running(
        format!("{ANSWER}\n{BLOCK}\n{BLOCK}"),
        /*model_progress*/ true,
        /*warning*/ false
    )
);

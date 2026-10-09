//! Test support: settles a run's whole request through a host-admitted check plan, so tests
//! about other completion behavior can reach the terminal gate.

use std::collections::BTreeMap;

use crate::AcceptanceChange;
use crate::AcceptanceCommit;
use crate::AcceptanceKind;
use crate::AcceptanceOrigin;
use crate::ArtifactState;
use crate::CommandEvidence;
use crate::CriterionTerms;
use crate::EvidenceOutcome;
use crate::RequestSpan;
use crate::StatefulRunId;
use crate::StatefulRunStore;
use crate::VerificationClaim;

const ARTIFACT: &str = "sha256:artifact";
const CHECKER: &str = "sha256:checker";

/// Settles the run's request (once), starts a verification attempt for `owner` and returns
/// the matching commit.
pub(crate) async fn settled_commit(
    store: &StatefulRunStore,
    run_id: &StatefulRunId,
    owner: &str,
    validated_obligation_sequence: Option<u64>,
) -> AcceptanceCommit {
    let ordinal = 1;
    if store
        .acceptance_ledger(run_id)
        .await
        .expect("ledger")
        .criteria
        .is_empty()
    {
        settle(store, run_id).await;
    }
    let attempt = store
        .begin_verification(run_id, owner, 60_000)
        .await
        .expect("verification starts");
    let ledger = store.acceptance_ledger(run_id).await.expect("ledger");
    let observed = |digest: &str| {
        BTreeMap::from([(
            ordinal,
            ArtifactState::Observed {
                digest: digest.to_string(),
                missing: Vec::new(),
            },
        )])
    };
    AcceptanceCommit {
        ledger_revision: ledger.revision,
        workspace_generation: ledger.workspace_generation,
        artifacts: observed(ARTIFACT),
        checkers: observed(CHECKER),
        verification: VerificationClaim {
            owner: owner.to_string(),
            attempt,
        },
        validated_obligation_sequence,
    }
}

/// C1 quotes the whole request; its plan is admitted and a passing receipt recorded.
async fn settle(store: &StatefulRunStore, run_id: &StatefulRunId) {
    let ordinal = 1;
    let request = store.acceptance_request(run_id).await.expect("request");
    let ledger = store.acceptance_ledger(run_id).await.expect("ledger");
    let ledger = store
        .revise_acceptance(
            run_id,
            ledger.revision,
            vec![
                AcceptanceChange::Add {
                    origin: AcceptanceOrigin::User,
                    kind: AcceptanceKind::Deliverable,
                    statement: "The request is done.".to_string(),
                    request_span: Some(RequestSpan {
                        start: 0,
                        end: request.len(),
                    }),
                    terms: CriterionTerms {
                        required: true,
                        artifacts: vec!["out.txt".to_string()],
                        checker: vec!["verify.sh".to_string()],
                        check_command: Some("sh verify.sh".to_string()),
                        expected_observation: Some("exit 0".to_string()),
                        ..CriterionTerms::default()
                    },
                },
                AcceptanceChange::Admit {
                    ordinal,
                    checker_digest: Some(CHECKER.to_string()),
                },
            ],
            "call-settle",
        )
        .await
        .expect("criterion admitted");
    let criterion = ledger.criterion(ordinal).expect("criterion");
    store
        .record_command_evidence(
            run_id,
            vec![CommandEvidence {
                ordinal,
                criterion_revision: criterion.revision,
                outcome: EvidenceOutcome::Passed,
                command: "sh verify.sh".to_string(),
                exit_code: Some(0),
                output_tail: String::new(),
                output_digest: "sha256:output".to_string(),
                artifact_digest: Some(ARTIFACT.to_string()),
                checker_digest: Some(CHECKER.to_string()),
                detail: None,
                start_generation: ledger.workspace_generation,
                source_id: "call-check".to_string(),
            }],
        )
        .await
        .expect("passing receipt");
}

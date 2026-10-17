use super::*;
use pretty_assertions::assert_eq;

const NONCE: [u8; 16] = [17; 16];
const WORKER: u32 = 7;
const TARGET: u32 = 9;

fn responses(exit_code: u32) -> [Response; 3] {
    [
        Response {
            nonce: NONCE,
            event: ResponseEvent::Ready { worker_pid: WORKER },
        },
        Response {
            nonce: NONCE,
            event: ResponseEvent::Spawned { target_pid: TARGET },
        },
        Response {
            nonce: NONCE,
            event: ResponseEvent::RootExited { exit_code },
        },
    ]
}

#[test]
fn full_exit_code_range_is_observed_without_granting_terminal_authority() {
    for exit_code in [0, 23, u32::MAX] {
        let mut sequence = WorkerResponseSequence::new(NONCE, WORKER).unwrap();
        for response in responses(exit_code) {
            let frame = crate::encode_response(&response, &NONCE).unwrap();
            let decoded = crate::decode_response(&frame, &NONCE).unwrap();
            assert_eq!(decoded, response);
            assert_eq!(sequence.observe(&decoded), Ok(()));
        }
        // Failure is still meaningful after root observation; no lifecycle
        // completion/preservation/drain certificate was minted by root exit.
        assert_eq!(
            sequence.observe(&Response {
                nonce: NONCE,
                event: ResponseEvent::Failed {
                    stage: FailureStage::Control,
                    win32_code: u32::MAX,
                }
            }),
            Err(SequenceError::WorkerFailed {
                stage: FailureStage::Control,
                win32_code: u32::MAX
            })
        );
        assert_eq!(
            sequence.observe(&responses(exit_code)[0]),
            Err(SequenceError::Order)
        );
    }
}

#[test]
fn every_duplicate_or_out_of_order_response_poison_is_sticky() {
    let valid = responses(/*exit_code*/ 0);
    for prefix in 0..=3 {
        for event in 0..3 {
            if prefix < 3 && event == prefix {
                continue;
            }
            let mut sequence = WorkerResponseSequence::new(NONCE, WORKER).unwrap();
            for response in &valid[..prefix] {
                assert_eq!(sequence.observe(response), Ok(()));
            }
            assert_eq!(sequence.observe(&valid[event]), Err(SequenceError::Order));
            for response in &valid {
                assert_eq!(sequence.observe(response), Err(SequenceError::Order));
            }
        }
    }
}

#[test]
fn correlation_refuses_in_every_phase_and_cannot_be_retried() {
    let valid = responses(/*exit_code*/ 0);
    for prefix in 0..=3 {
        let mut sequence = WorkerResponseSequence::new(NONCE, WORKER).unwrap();
        for response in &valid[..prefix] {
            sequence.observe(response).unwrap();
        }
        let wrong = Response {
            nonce: [18; 16],
            event: ResponseEvent::Failed {
                stage: FailureStage::Internal,
                win32_code: 1,
            },
        };
        assert_eq!(sequence.observe(&wrong), Err(SequenceError::Correlation));
        assert_eq!(sequence.observe(&valid[0]), Err(SequenceError::Order));
    }
}

#[test]
fn reported_failure_at_every_phase_retains_closed_metadata_and_poison() {
    let valid = responses(/*exit_code*/ 23);
    for prefix in 0..=3 {
        for stage in [
            FailureStage::Bootstrap,
            FailureStage::Console,
            FailureStage::HandleAdmission,
            FailureStage::Spawn,
            FailureStage::Control,
            FailureStage::Internal,
        ] {
            for win32_code in [0, 23, u32::MAX] {
                let mut sequence = WorkerResponseSequence::new(NONCE, WORKER).unwrap();
                for response in &valid[..prefix] {
                    sequence.observe(response).unwrap();
                }
                assert_eq!(
                    sequence.observe(&Response {
                        nonce: NONCE,
                        event: ResponseEvent::Failed { stage, win32_code }
                    }),
                    Err(SequenceError::WorkerFailed { stage, win32_code })
                );
                assert_eq!(sequence.observe(&valid[0]), Err(SequenceError::Order));
            }
        }
    }
}

#[test]
fn zero_wrong_worker_and_zero_or_self_target_identities_refuse() {
    assert!(matches!(
        WorkerResponseSequence::new(NONCE, /*expected_worker_pid*/ 0),
        Err(SequenceError::WorkerIdentity)
    ));
    let valid = responses(/*exit_code*/ 0);
    for worker_pid in [0, TARGET, u32::MAX] {
        let mut sequence = WorkerResponseSequence::new(NONCE, WORKER).unwrap();
        assert_eq!(
            sequence.observe(&Response {
                nonce: NONCE,
                event: ResponseEvent::Ready { worker_pid }
            }),
            Err(SequenceError::WorkerIdentity)
        );
        assert_eq!(sequence.observe(&valid[0]), Err(SequenceError::Order));
    }
    for target_pid in [0, WORKER] {
        let mut sequence = WorkerResponseSequence::new(NONCE, WORKER).unwrap();
        sequence.observe(&valid[0]).unwrap();
        assert_eq!(
            sequence.observe(&Response {
                nonce: NONCE,
                event: ResponseEvent::Spawned { target_pid }
            }),
            Err(SequenceError::WorkerIdentity)
        );
        assert_eq!(sequence.observe(&valid[1]), Err(SequenceError::Order));
    }
}

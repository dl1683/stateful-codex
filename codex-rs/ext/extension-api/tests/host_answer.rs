use codex_extension_api::AbortArbitration;
use codex_extension_api::HostAnswerDisqualifier;
use codex_extension_api::HostAnswerPhase;
use codex_extension_api::HostAnswerReservation;
use pretty_assertions::assert_eq;

fn reservation() -> HostAnswerReservation {
    HostAnswerReservation::new("run".to_string(), "thread".to_string(), "turn".to_string())
}

fn finalizing() -> HostAnswerReservation {
    let reservation = reservation();
    reservation.record_hook_set(/*hooks_are_empty*/ true);
    assert!(reservation.begin_finalization());
    reservation
}

/// Disqualification is sticky: nothing later makes the reservation able to commit.
#[test]
fn disqualification_is_sticky() {
    let reservation = reservation();
    reservation.disqualify(HostAnswerDisqualifier::ToolCall);
    reservation.disqualify(HostAnswerDisqualifier::Compaction);
    reservation.record_hook_set(/*hooks_are_empty*/ true);
    assert!(!reservation.begin_finalization());
    assert!(!reservation.authorize_commit());
    assert_eq!(
        reservation.phase(),
        HostAnswerPhase::Disqualified(HostAnswerDisqualifier::ToolCall)
    );
    assert!(!reservation.refuses_auxiliary_work());
}

/// Finalization needs a dispatch site to have confirmed an empty hook set; any hook set
/// disqualifies.
#[test]
fn finalization_requires_a_confirmed_empty_hook_set() {
    let unverified = reservation();
    assert!(!unverified.begin_finalization());
    assert_eq!(
        unverified.phase(),
        HostAnswerPhase::Disqualified(HostAnswerDisqualifier::HooksUnverified)
    );
    let hooked = reservation();
    hooked.record_hook_set(/*hooks_are_empty*/ true);
    hooked.record_hook_set(/*hooks_are_empty*/ false);
    assert_eq!(
        hooked.phase(),
        HostAnswerPhase::Disqualified(HostAnswerDisqualifier::ExecutableHooks)
    );
}

/// An abort before authorization wins and the commit is refused; auxiliary work is refused
/// while the reservation is live and admitted again once it is not.
#[test]
fn abort_before_authorization_wins() {
    for reservation in [reservation(), finalizing()] {
        assert!(reservation.refuses_auxiliary_work());
        assert_eq!(reservation.arbitrate_abort(), AbortArbitration::Proceed);
        assert_eq!(reservation.phase(), HostAnswerPhase::Aborted);
        assert!(!reservation.authorize_commit());
        assert!(!reservation.refuses_auxiliary_work());
        assert!(!reservation.input_closed());
    }
}

/// After authorization an abort waits for the task; it is remembered so a commit that
/// then fails ends the turn as aborted, while a durable commit stays completed.
#[test]
fn abort_after_authorization_waits_for_the_outcome() {
    let committed = finalizing();
    assert!(committed.input_closed());
    assert!(committed.authorize_commit());
    assert_eq!(committed.arbitrate_abort(), AbortArbitration::AwaitTaskEnd);
    committed.resolve_commit(/*committed*/ true);
    assert_eq!(committed.end_finalization(), HostAnswerPhase::Committed);
    assert_eq!(committed.arbitrate_abort(), AbortArbitration::AwaitTaskEnd);
    assert!(committed.refuses_auxiliary_work());

    let failed = finalizing();
    assert!(failed.authorize_commit());
    assert_eq!(failed.arbitrate_abort(), AbortArbitration::AwaitTaskEnd);
    failed.resolve_commit(/*committed*/ false);
    assert_eq!(failed.end_finalization(), HostAnswerPhase::NotCommitted);
    assert!(failed.abort_requested());
    assert_eq!(failed.arbitrate_abort(), AbortArbitration::Proceed);

    // A finalizer that never authorized leaves the reservation not committed.
    let declined = finalizing();
    assert_eq!(declined.end_finalization(), HostAnswerPhase::NotCommitted);
    assert!(!declined.abort_requested());
    assert!(!declined.refuses_auxiliary_work());
}

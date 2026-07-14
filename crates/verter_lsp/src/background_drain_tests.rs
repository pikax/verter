use super::*;

/// INV-3: only the transient `NotReady` bootstrap is retryable. A terminal
/// `NoProject` / `Ambiguous` (`Unresolved`) carrier must settle and DEQUEUE —
/// never be retried into a provider on every drain — while `NotReady` (and a
/// transient `Pending` advertise miss) stay queued.
#[test]
fn terminal_unresolved_carrier_dequeues_but_transient_stays_queued() {
    // Terminal no-owner ⇒ Terminal ⇒ dequeue.
    assert_eq!(
        classify_carrier_apply_outcome(CarrierApplyOutcome::Unresolved),
        SyncOutcome::Terminal,
        "a terminal Unresolved carrier settles (never retried)"
    );
    assert!(
        sync_outcome_dequeues(SyncOutcome::Terminal),
        "a terminal carrier must be dequeued from the pending drain set"
    );

    // Transient bootstrap ⇒ Nothing ⇒ stay queued (retryable).
    assert_eq!(
        classify_carrier_apply_outcome(CarrierApplyOutcome::NotReady),
        SyncOutcome::Nothing,
        "a transient NotReady carrier stays queued for a later retry"
    );
    assert!(
        !sync_outcome_dequeues(SyncOutcome::Nothing),
        "a still-transient carrier must stay queued"
    );

    // A `Pending` advertise/compile miss is also transient ⇒ stay queued.
    assert_eq!(
        classify_carrier_apply_outcome(CarrierApplyOutcome::Pending),
        SyncOutcome::Nothing,
        "a Pending advertise miss stays queued"
    );

    // A partial per-kind sync stays queued so the failed kind is retried;
    // a fully-reconciled sync dequeues.
    assert_eq!(
        classify_carrier_apply_outcome(CarrierApplyOutcome::Applied {
            attempted: vec![ProviderPathKind::Ide, ProviderPathKind::Api],
            synced: vec![ProviderPathKind::Ide],
        }),
        SyncOutcome::Partial,
    );
    assert!(!sync_outcome_dequeues(SyncOutcome::Partial));
    assert_eq!(
        classify_carrier_apply_outcome(CarrierApplyOutcome::Applied {
            attempted: vec![ProviderPathKind::Ide],
            synced: vec![ProviderPathKind::Ide],
        }),
        SyncOutcome::FullyReconciled,
    );
    assert!(sync_outcome_dequeues(SyncOutcome::FullyReconciled));
}

/// Whole-class `Pending`: the settled no-owner disposition runs the buffer-side
/// preserve-open / remove-closed handling ONLY for a settled no-owner class. A `Pending`
/// — a FAILED store retract, the stale cross-process membership still advertised — must
/// PRESERVE local state: the buffer cleanup is SKIPPED so the source is retried, never
/// cleared/reclassified as-if-retracted (which would also drop the owned surface-stamp
/// gate). The coordinator's `settle` performs the requeue / barrier advance and hands back
/// the [`SettleClass`]; `runs_buffer_cleanup` is the pure predicate the sites drive their
/// buffer conversion from.
#[test]
fn settle_class_runs_buffer_cleanup_only_for_settled_no_owner() {
    use crate::external_ts::SettleClass;

    // DISCRIMINATING: a variant that ran the cleanup for `Pending` (the pre-fix behavior
    // that discarded the decision and ALWAYS ran the buffer cleanup) fails this assertion.
    assert!(
        !SettleClass::Pending.runs_buffer_cleanup(),
        "a Pending (failed retract) must PRESERVE local state: buffer cleanup skipped + retried"
    );
    assert!(
        SettleClass::Unresolved.runs_buffer_cleanup(),
        "a terminal Unresolved runs the buffer-side preserve/remove handling"
    );
    assert!(
        SettleClass::NotReady.runs_buffer_cleanup(),
        "a transient NotReady runs the buffer-side preserve/remove handling"
    );
}

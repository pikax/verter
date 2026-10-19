use std::sync::Arc;

use super::*;
use crate::owned::{ChargeCtx, OwnedHandles, OwnedStr};
use crate::ChargeClass;
use crate::{RetentionAdmission, RetentionCharge, RetentionLimits};
use std::sync::atomic::Ordering;

fn account(ceiling: usize) -> Arc<SemanticRetentionAccount> {
    SemanticRetentionAccount::new(RetentionLimits {
        aggregate_ceiling_bytes: ceiling,
        active_ceiling_bytes: ceiling,
        max_entry_bytes: ceiling,
        pin_threshold_bytes: ceiling,
    })
}

fn active_as(
    account: &Arc<SemanticRetentionAccount>,
    owner: &ActiveOwner,
    bytes: usize,
) -> RetentionCharge {
    account
        .reserve_as(owner, ChargeClass::Active, bytes)
        .admitted()
        .expect("active admitted")
}

/// Untagged: unknown provenance.
fn active(account: &Arc<SemanticRetentionAccount>, bytes: usize) -> RetentionCharge {
    account
        .reserve(ChargeClass::Active, bytes)
        .admitted()
        .expect("active admitted")
}

/// Identified balances plus the UNKNOWN balance are the account's ACTIVE
/// bytes, at every point the test samples.
fn assert_balances_cover_account(account: &Arc<SemanticRetentionAccount>) {
    let _admission_lock = account.lock_aggregate_state();
    let registry = account.active_windows.inner.lock();
    let identified: usize = registry.owner_active.values().sum();
    assert_eq!(
        identified + registry.unknown_active,
        account.active_bytes.load(Ordering::Relaxed)
    );
}

/// Bytes are conserved across a report and every transition inside the
/// interval is numbered on the account's sequence.
fn assert_conserved(report: &ActiveWindowReport) {
    assert_eq!(
        report.active_at_open + report.rise_bytes - report.fall_bytes,
        report.active_at_close,
        "{report:?}"
    );
    assert_eq!(
        report.closed_at - report.opened_at,
        report.transitions + report.foreign_transitions + report.unknown_transitions,
        "{report:?}"
    );
    assert!(report.peak_active_bytes >= report.active_at_open.max(report.active_at_close));
}

#[test]
fn a_window_reports_the_active_rise_and_fall_inside_it() {
    let account = account(1 << 20);
    let owner = account.mint_active_owner();
    let window = account.open_active_window(&owner);
    let a = active_as(&account, &owner, 100);
    let b = active_as(&account, &owner, 50);
    drop(a);
    let c = active_as(&account, &owner, 30);
    assert_eq!(
        window.observe().active_at_close,
        80,
        "observe reads the live total"
    );
    drop((b, c));
    let report = window.finish();
    assert_conserved(&report);
    assert_eq!(report.peak_active_bytes, 150);
    assert_eq!((report.rise_bytes, report.fall_bytes), (180, 180));
    assert_eq!(report.transitions, 6);
    let peak = report
        .attributed()
        .expect("an isolated workload is attributable");
    assert!(peak.crossed(150));
    assert!(!peak.crossed(151));
}

/// The two confusions the window exists to exclude: a large retained and
/// pinned total beside a small ACTIVE occupancy, and an ACTIVE peak reached
/// before the window opened. Neither satisfies the window's crossing check,
/// while the account's lifetime total peak would satisfy both.
#[test]
fn retained_pinned_totals_and_a_pre_window_peak_never_cross() {
    const THRESHOLD: usize = 1_000;
    let account = account(1 << 20);
    let owner = account.mint_active_owner();
    let earlier = active_as(&account, &owner, 5 * THRESHOLD);
    drop(earlier);
    let window = account.open_active_window(&owner);
    let retained = account
        .reserve(ChargeClass::Retained, 5 * THRESHOLD)
        .admitted()
        .expect("retained admitted");
    let pinned = account.pin(5 * THRESHOLD);
    let small = active_as(&account, &owner, 10);
    drop(small);
    let report = window.finish();
    assert!(account.snapshot().peak_total_bytes >= 5 * THRESHOLD);
    assert_eq!(report.peak_active_bytes, 10);
    assert!(!report
        .attributed()
        .expect("attributable")
        .crossed(THRESHOLD));
    drop((retained, pinned));
}

/// Each class transfer moves the ACTIVE total exactly as a reservation or a
/// release would: committing to retained and transferring to pinned are
/// falls, and a retained slice of cache holds joining an ACTIVE charge is a
/// rise.
#[test]
fn class_transfers_are_active_transitions() {
    let account = account(1 << 30);
    let owner = account.mint_active_owner();
    let window = account.open_active_window(&owner);

    let committed = active_as(&account, &owner, 40).commit_retained();
    let mut pinned = active_as(&account, &owner, 60);
    pinned.transfer_to_pinned();
    let names: Vec<OwnedStr> = (0..8).map(|i| OwnedStr::from(format!("{i}"))).collect();
    let handles = OwnedHandles::of(|aliases| names.iter().map(|name| name.offer(aliases)).sum());
    let holding = handles
        .retained_with(
            active_as(&account, &owner, 10),
            ChargeCtx::owned_by(&account, &owner),
        )
        .expect("holds admitted");
    let slice = holding.bytes() - 10;
    assert!(slice > 0, "the holds' slice joins the active charge");

    let report = window.observe();
    assert_conserved(&report);
    assert_eq!(report.active_at_close, 10 + slice);
    assert_eq!(report.rise_bytes, 40 + 60 + 10 + slice);
    assert_eq!(report.fall_bytes, 40 + 60);
    assert_eq!(
        report.transitions, 6,
        "three reservations, two transfers out, one in"
    );
    drop(holding);
    let report = window.finish();
    assert_conserved(&report);
    assert_eq!(report.peak_active_bytes, 60.max(10 + slice));
    assert!(report.attributed().is_ok());
    drop((committed, pinned, names));
}

/// A charge shared behind an `Arc` is one reservation: one rise, and one
/// fall when its last owner drops.
#[test]
fn a_shared_active_charge_is_observed_once() {
    let account = account(1 << 20);
    let owner = account.mint_active_owner();
    let window = account.open_active_window(&owner);
    let shared = Arc::new(active_as(&account, &owner, 70));
    let other = Arc::clone(&shared);
    drop(shared);
    assert_eq!(window.observe().fall_bytes, 0);
    drop(other);
    let report = window.finish();
    assert_eq!(
        (report.rise_bytes, report.fall_bytes, report.transitions),
        (70, 70, 2)
    );
    assert_eq!(report.peak_active_bytes, 70);
}

/// Overlapping windows keep their own counters: opening or finishing one
/// never resets the other, and each measures only its own owner while
/// counting the other's transitions as excluded foreign work.
#[test]
fn overlapping_windows_report_independently() {
    let account = account(1 << 20);
    let (a, b) = (account.mint_active_owner(), account.mint_active_owner());
    let outer = account.open_active_window(&a);
    let first = active_as(&account, &a, 100);
    let inner = account.open_active_window(&b);
    assert_ne!(outer.id(), inner.id());
    drop(first);
    let second = active_as(&account, &b, 20);
    drop(second);
    let inner = inner.finish();
    assert_conserved(&inner);
    assert_eq!(
        (inner.peak_active_bytes, inner.foreign_transitions),
        (20, 1)
    );
    assert_eq!(inner.attributed().map(|p| p.peak_active_bytes), Ok(20));
    let third = active_as(&account, &a, 30);
    drop(third);
    let outer = outer.finish();
    assert_conserved(&outer);
    assert_eq!(outer.transitions, 4, "the inner window reset nothing");
    assert_eq!(outer.foreign_transitions, 2);
    assert_eq!(outer.peak_active_bytes, 100);
    assert_eq!(outer.attributed().map(|p| p.peak_active_bytes), Ok(100));
}

/// The measured owner's own ACTIVE bytes in flight at open or at close
/// reject the witness; so does any unknown-provenance transition.
#[test]
fn unattributable_work_rejects_the_witness() {
    let account = account(1 << 30);
    let owner = account.mint_active_owner();

    let in_flight = active_as(&account, &owner, 5);
    let report = account.open_active_window(&owner).finish();
    assert_eq!(
        report.attributed(),
        Err(ActiveWitnessRejection::ActiveAtOpen { bytes: 5 })
    );
    drop(in_flight);

    let window = account.open_active_window(&owner);
    let leaked = active_as(&account, &owner, 7);
    let report = window.finish();
    assert_eq!(
        report.attributed(),
        Err(ActiveWitnessRejection::ActiveAtClose { bytes: 7 })
    );
    drop(leaked);

    // An untagged charge settling inside the interval could be the
    // workload's own work or foreign work: unknown, never attributed.
    let untagged = active(&account, 9);
    let window = account.open_active_window(&owner);
    drop(untagged);
    let owned = active_as(&account, &owner, 3);
    drop(owned);
    let report = window.finish();
    assert_eq!(
        report.attributed(),
        Err(ActiveWitnessRejection::UnknownProvenance { transitions: 1 })
    );

    // Untagged ACTIVE occupancy spanning the whole window makes no
    // transition inside it, and is just as unattributable.
    let spanning = active(&account, 11);
    assert_balances_cover_account(&account);
    let report = account.open_active_window(&owner).finish();
    assert_eq!(report.unknown_transitions, 0);
    assert_eq!(
        report.attributed(),
        Err(ActiveWitnessRejection::UnknownActiveAtOpen { bytes: 11 })
    );
    drop(spanning);
    assert_balances_cover_account(&account);
}

/// Foreign ACTIVE work that starts and ends strictly inside the window never
/// raises the owner's peak and cannot satisfy its crossing check, and an
/// owned control in the same interval still crosses.
#[test]
fn foreign_work_wholly_inside_the_window_is_excluded() {
    const THRESHOLD: usize = 1_000;
    let account = account(1 << 30);
    let (owner, foreign) = (account.mint_active_owner(), account.mint_active_owner());

    let window = account.open_active_window(&owner);
    drop(active_as(&account, &foreign, 5 * THRESHOLD));
    let report = window.finish();
    assert_conserved(&report);
    assert_eq!((report.transitions, report.foreign_transitions), (0, 2));
    let peak = report.attributed().expect("foreign work is excluded");
    assert_eq!(peak.peak_active_bytes, 0);
    assert!(!peak.crossed(THRESHOLD));

    let window = account.open_active_window(&owner);
    let held = active_as(&account, &foreign, 5 * THRESHOLD);
    drop(active_as(&account, &owner, THRESHOLD));
    drop(held);
    let report = window.finish();
    assert_eq!(report.peak_active_bytes, THRESHOLD);
    assert!(report
        .attributed()
        .expect("attributable")
        .crossed(THRESHOLD));
    assert!(!report
        .attributed()
        .expect("attributable")
        .crossed(THRESHOLD + 1));
}

/// Provenance survives sharing, `split_off`, class transfers and holding
/// adoption; bytes of two owners merged into one charge are unknown.
#[test]
fn provenance_follows_the_charge_and_mixed_provenance_is_unknown() {
    let account = account(1 << 30);
    let (owner, other) = (account.mint_active_owner(), account.mint_active_owner());
    let window = account.open_active_window(&owner);
    let mut whole = active_as(&account, &owner, 100);
    let part = whole.split_off(40);
    drop(part);
    whole.transfer_to_pinned();
    drop(whole);
    let report = window.finish();
    assert_conserved(&report);
    assert_eq!(
        (
            report.transitions,
            report.foreign_transitions,
            report.unknown_transitions
        ),
        (3, 0, 0)
    );
    assert!(report.attributed().is_ok());

    // A retained slice of another owner's holds joins this owner's ACTIVE
    // charge: the join is mixed provenance.
    let window = account.open_active_window(&owner);
    let names: Vec<OwnedStr> = (0..4).map(|i| OwnedStr::from(format!("{i}"))).collect();
    let handles = OwnedHandles::of(|aliases| names.iter().map(|name| name.offer(aliases)).sum());
    let foreign_ctx = ChargeCtx::owned_by(&account, &other);
    let mixed = handles
        .retained_with(active_as(&account, &owner, 10), foreign_ctx)
        .expect("holds admitted");
    drop(mixed);
    let report = window.finish();
    assert_eq!(
        report.attributed(),
        Err(ActiveWitnessRejection::UnknownProvenance { transitions: 3 })
    );
    assert_eq!(account.snapshot().active_bytes, 0);
    assert_balances_cover_account(&account);
    let fresh = account.open_active_window(&owner);
    let report = fresh.finish();
    assert_eq!((report.active_at_open, report.active_at_close), (0, 0));
    assert!(report.attributed().is_ok());

    // A mixed charge that outlives a whole window: the window sees no
    // transition, yet the owner's bytes were re-homed to UNKNOWN, so it is
    // rejected rather than accepted as a zero-activity witness.
    let handles = OwnedHandles::of(|aliases| names.iter().map(|name| name.offer(aliases)).sum());
    let mixed = handles
        .retained_with(
            active_as(&account, &owner, 10),
            ChargeCtx::owned_by(&account, &other),
        )
        .expect("holds admitted");
    let unknown = account.snapshot().active_bytes;
    assert!(unknown >= 10);
    assert_balances_cover_account(&account);
    let report = account.open_active_window(&owner).finish();
    assert_eq!(
        report.attributed(),
        Err(ActiveWitnessRejection::UnknownActiveAtOpen { bytes: unknown })
    );
    drop(mixed);
    assert_eq!(account.snapshot().active_bytes, 0);
    assert_balances_cover_account(&account);

    // After teardown the original owner measures a fresh workload exactly.
    let window = account.open_active_window(&owner);
    drop(active_as(&account, &owner, 33));
    let report = window.finish();
    assert_conserved(&report);
    assert_eq!(report.attributed().expect("fresh").peak_active_bytes, 33);
    drop(names);
}

/// Stops and joins the workers on every exit, an assertion failure included.
struct Workers {
    stop: Arc<std::sync::atomic::AtomicBool>,
    handles: Vec<std::thread::JoinHandle<()>>,
}

impl Workers {
    fn finish(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        for handle in self.handles.drain(..) {
            handle.join().expect("worker");
        }
    }
}

impl Drop for Workers {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        for handle in self.handles.drain(..) {
            let _ = handle.join();
        }
    }
}

/// A window opened between an owned reservation and its release, and one
/// finished before the release, each see the boundary on exactly one side:
/// the held bytes are charged at open (rejecting the witness) or at close,
/// and every transition lands on one side of the boundary or the other.
#[test]
fn window_boundaries_fall_on_exactly_one_side_of_a_transition() {
    let account = account(1 << 30);
    let owner = account.mint_active_owner();

    let charge = active_as(&account, &owner, 8);
    let straddling_open = account.open_active_window(&owner);
    drop(charge);
    let report = straddling_open.finish();
    assert_conserved(&report);
    assert_eq!(
        (
            report.active_at_open,
            report.fall_bytes,
            report.active_at_close
        ),
        (8, 8, 0)
    );
    assert_eq!(
        report.attributed(),
        Err(ActiveWitnessRejection::ActiveAtOpen { bytes: 8 })
    );

    let straddling_close = account.open_active_window(&owner);
    let charge = active_as(&account, &owner, 8);
    let report = straddling_close.finish();
    assert_conserved(&report);
    assert_eq!(
        (
            report.active_at_open,
            report.rise_bytes,
            report.active_at_close
        ),
        (0, 8, 8)
    );
    drop(charge);
    assert_eq!(account.snapshot().active_bytes, 0);
    assert_balances_cover_account(&account);
}

/// Windows opened and finished while other threads reserve and release
/// ACTIVE bytes lose no transition at either boundary: every report
/// conserves its bytes and numbers exactly its own transitions. The workers
/// keep running until every window has finished, and the windows start only
/// once a worker has transitioned, so the two always overlap.
#[test]
fn concurrent_transitions_at_window_boundaries_are_never_lost() {
    const WORKERS: usize = 4;
    let account = account(1 << 30);
    let owners: Vec<_> = (0..WORKERS).map(|_| account.mint_active_owner()).collect();
    let measured = owners[0].clone();
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let handles: Vec<_> = (0..WORKERS)
        .map(|worker| {
            let owner = owners[worker].clone();
            let account = Arc::clone(&account);
            let stop = Arc::clone(&stop);
            std::thread::spawn(move || {
                let mut round = 0usize;
                while !stop.load(Ordering::Relaxed) {
                    let charge = active_as(&account, &owner, 1 + (worker + round) % 13);
                    if round.is_multiple_of(3) {
                        drop(charge.commit_retained());
                    } else {
                        drop(charge);
                    }
                    round += 1;
                }
            })
        })
        .collect();
    let mut workers = Workers { stop, handles };
    while account.active_transition_count() == 0 {
        std::thread::yield_now();
    }
    // Keep opening windows until enough of them demonstrably spanned worker
    // transitions: the overlap is required, never hoped for.
    let mut reports = Vec::new();
    let mut overlapped = 0usize;
    while reports.len() < 400 || overlapped < 20 {
        let window = account.open_active_window(&measured);
        reports.push(window.observe());
        let report = window.finish();
        overlapped += usize::from(report.closed_at > report.opened_at);
        reports.push(report);
    }
    workers.finish();
    for report in &reports {
        assert_conserved(report);
    }
    assert_eq!(account.snapshot().active_bytes, 0);
    assert_balances_cover_account(&account);
}

/// An unfinished window deregisters when dropped and keeps its account
/// alive while open; transitions after a window ends touch no window.
#[test]
fn an_abandoned_window_deregisters_and_later_windows_start_fresh() {
    let account = account(1 << 20);
    let owner = account.mint_active_owner();
    let abandoned = account.open_active_window(&owner);
    let charge = active_as(&account, &owner, 9);
    drop(abandoned);
    assert_eq!(account.active_windows.open.load(Ordering::Relaxed), 0);
    drop(charge);

    let window = account.open_active_window(&owner);
    let only = Arc::downgrade(&account);
    drop(owner);
    drop(account);
    let account = only.upgrade().expect("the open window keeps its account");
    drop(account);
    assert!(only.upgrade().is_some(), "only the open window keeps it");
    let report = window.finish();
    assert_eq!((report.transitions, report.peak_active_bytes), (0, 0));
    assert!(only.upgrade().is_none(), "finishing releases the account");
}

/// An open window changes no admission decision: the same sequence of
/// reservations is admitted and refused identically, with identical
/// figures, on an account measured by a window and on one that is not.
#[test]
fn an_open_window_changes_no_admission_decision() {
    fn run(
        account: &Arc<SemanticRetentionAccount>,
        owner: Option<&ActiveOwner>,
    ) -> Vec<Result<usize, crate::RetentionRefusal>> {
        let mut kept = Vec::new();
        let mut outcomes = Vec::new();
        for (class, bytes) in [
            (ChargeClass::Active, 60),
            (ChargeClass::Retained, 30),
            (ChargeClass::Active, 50),
            (ChargeClass::Retained, 200),
            (ChargeClass::Pinned, 20),
            (ChargeClass::Retained, 15),
            (ChargeClass::Active, 1),
        ] {
            let admission = match owner {
                Some(owner) => account.reserve_as(owner, class, bytes),
                None => account.reserve(class, bytes),
            };
            match admission {
                RetentionAdmission::Admitted(charge) => {
                    outcomes.push(Ok(charge.bytes()));
                    kept.push(charge);
                }
                RetentionAdmission::Refused(refusal) => outcomes.push(Err(refusal)),
            }
        }
        outcomes
    }
    let limits = RetentionLimits {
        aggregate_ceiling_bytes: 120,
        active_ceiling_bytes: 80,
        max_entry_bytes: 100,
        pin_threshold_bytes: 120,
    };
    let plain = SemanticRetentionAccount::new(limits);
    let measured = SemanticRetentionAccount::new(limits);
    let measured_owner = measured.mint_active_owner();
    let window = measured.open_active_window(&measured_owner);
    let expected = run(&plain, None);
    assert!(
        expected.iter().any(Result::is_err),
        "the sequence exercises refusals"
    );
    assert_eq!(run(&measured, Some(&measured_owner)), expected);
    assert_eq!(measured.snapshot(), plain.snapshot());
    assert_eq!(window.finish().peak_active_bytes, 61);
}

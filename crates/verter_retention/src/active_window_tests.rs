use std::sync::{Arc, Barrier};

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

fn active(account: &Arc<SemanticRetentionAccount>, bytes: usize) -> RetentionCharge {
    account
        .reserve(ChargeClass::Active, bytes)
        .admitted()
        .expect("active admitted")
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
        report.transitions,
        "{report:?}"
    );
    assert!(report.peak_active_bytes >= report.active_at_open.max(report.active_at_close));
}

#[test]
fn a_window_reports_the_active_rise_and_fall_inside_it() {
    let account = account(1 << 20);
    let window = account.open_active_window();
    let a = active(&account, 100);
    let b = active(&account, 50);
    drop(a);
    let c = active(&account, 30);
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
    let earlier = active(&account, 5 * THRESHOLD);
    drop(earlier);
    let window = account.open_active_window();
    let retained = account
        .reserve(ChargeClass::Retained, 5 * THRESHOLD)
        .admitted()
        .expect("retained admitted");
    let pinned = account.pin(5 * THRESHOLD);
    let small = active(&account, 10);
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
    let window = account.open_active_window();

    let committed = active(&account, 40).commit_retained();
    let mut pinned = active(&account, 60);
    pinned.transfer_to_pinned();
    let names: Vec<OwnedStr> = (0..8).map(|i| OwnedStr::from(format!("{i}"))).collect();
    let handles = OwnedHandles::of(|aliases| names.iter().map(|name| name.offer(aliases)).sum());
    let holding = handles
        .retained_with(active(&account, 10), ChargeCtx::new(&account))
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
    let window = account.open_active_window();
    let shared = Arc::new(active(&account, 70));
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
/// never resets the other, and the later window rejects the earlier one's
/// in-flight work it cannot own.
#[test]
fn overlapping_windows_report_independently() {
    let account = account(1 << 20);
    let outer = account.open_active_window();
    let first = active(&account, 100);
    let inner = account.open_active_window();
    assert_ne!(outer.id(), inner.id());
    drop(first);
    let second = active(&account, 20);
    drop(second);
    let inner = inner.finish();
    assert_conserved(&inner);
    assert_eq!(
        inner.attributed(),
        Err(ActiveWitnessRejection::ActiveAtOpen { bytes: 100 })
    );
    assert_eq!(inner.peak_active_bytes, 100);
    let third = active(&account, 30);
    drop(third);
    let outer = outer.finish();
    assert_conserved(&outer);
    assert_eq!(outer.transitions, 6, "the inner window reset nothing");
    assert_eq!(outer.peak_active_bytes, 100);
    assert_eq!(outer.attributed().map(|p| p.peak_active_bytes), Ok(100));
}

/// Detectable foreign work rejects the witness: ACTIVE bytes in flight at
/// open or at close, and a pre-window charge growing inside the interval.
#[test]
fn foreign_work_rejects_the_witness() {
    let account = account(1 << 30);

    let in_flight = active(&account, 5);
    let report = account.open_active_window().finish();
    assert_eq!(
        report.attributed(),
        Err(ActiveWitnessRejection::ActiveAtOpen { bytes: 5 })
    );
    drop(in_flight);

    let window = account.open_active_window();
    let leaked = active(&account, 7);
    let report = window.finish();
    assert_eq!(
        report.attributed(),
        Err(ActiveWitnessRejection::ActiveAtClose { bytes: 7 })
    );
    drop(leaked);

    // A zero-byte charge carries nothing at open, yet it is pre-window
    // work: its growth inside the interval is not the workload's.
    let early = active(&account, 0);
    let window = account.open_active_window();
    let names: Vec<OwnedStr> = (0..4).map(|i| OwnedStr::from(format!("{i}"))).collect();
    let handles = OwnedHandles::of(|aliases| names.iter().map(|name| name.offer(aliases)).sum());
    let grown = handles
        .retained_with(early, ChargeCtx::new(&account))
        .expect("holds admitted");
    drop(grown);
    let report = window.finish();
    assert_eq!(report.active_at_open, 0);
    assert_eq!(
        report.attributed(),
        Err(ActiveWitnessRejection::PreWindowCharge { transitions: 2 })
    );
    drop(names);
}

/// Windows opened and finished while other threads reserve and release
/// ACTIVE bytes lose no transition at either boundary: every report
/// conserves its bytes and numbers exactly its own transitions.
#[test]
fn concurrent_transitions_at_window_boundaries_are_never_lost() {
    const WORKERS: usize = 4;
    const ROUNDS: usize = 2_000;
    let account = account(1 << 30);
    let start = Arc::new(Barrier::new(WORKERS + 1));
    let workers: Vec<_> = (0..WORKERS)
        .map(|worker| {
            let account = Arc::clone(&account);
            let start = Arc::clone(&start);
            std::thread::spawn(move || {
                start.wait();
                for round in 0..ROUNDS {
                    let charge = active(&account, 1 + (worker + round) % 13);
                    if round % 3 == 0 {
                        drop(charge.commit_retained());
                    } else {
                        drop(charge);
                    }
                }
            })
        })
        .collect();
    start.wait();
    let mut reports = Vec::new();
    for _ in 0..200 {
        let window = account.open_active_window();
        reports.push(window.observe());
        reports.push(window.finish());
    }
    for worker in workers {
        worker.join().expect("worker");
    }
    for report in &reports {
        assert_conserved(report);
    }
    assert_eq!(account.snapshot().active_bytes, 0);
    assert_eq!(
        account.active_transition_count(),
        (WORKERS * ROUNDS * 2) as u64,
        "every transition is numbered once"
    );
}

/// An unfinished window deregisters when dropped and keeps its account
/// alive while open; transitions after a window ends touch no window.
#[test]
fn an_abandoned_window_deregisters_and_later_windows_start_fresh() {
    let account = account(1 << 20);
    let abandoned = account.open_active_window();
    let charge = active(&account, 9);
    drop(abandoned);
    assert_eq!(account.active_windows.open.load(Ordering::Relaxed), 0);
    drop(charge);

    let window = account.open_active_window();
    let only = Arc::downgrade(&account);
    drop(account);
    let account = only.upgrade().expect("the open window keeps its account");
    let report = window.finish();
    assert_eq!((report.transitions, report.peak_active_bytes), (0, 0));
    assert!(account.active_windows.states.lock().is_empty());
}

/// An open window changes no admission decision: the same sequence of
/// reservations is admitted and refused identically, with identical
/// figures, on an account measured by a window and on one that is not.
#[test]
fn an_open_window_changes_no_admission_decision() {
    fn run(account: &Arc<SemanticRetentionAccount>) -> Vec<Result<usize, crate::RetentionRefusal>> {
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
            match account.reserve(class, bytes) {
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
    let window = measured.open_active_window();
    let expected = run(&plain);
    assert!(
        expected.iter().any(Result::is_err),
        "the sequence exercises refusals"
    );
    assert_eq!(run(&measured), expected);
    assert_eq!(measured.snapshot(), plain.snapshot());
    assert_eq!(window.finish().peak_active_bytes, 61);
}

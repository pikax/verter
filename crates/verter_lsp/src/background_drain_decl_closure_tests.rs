//! Discriminating regressions for the closure-pass-generation defense in the
//! declaration-overlay graph. Two overlapping `background_init` closure passes
//! (an older pass past its pre-pass generation gate + a newer pass that
//! re-established a root's reachability) must never let the OLDER pass close — or
//! drop the reaching edge of — an overlay a NEWER live root analysis reaches. The
//! bug is two-staged: the older pass's `reconcile_root_reachability` could ERASE a
//! newer pass's edge (the earlier, root-cause stage), and even an emptied slot
//! could be CLOSED by the older pass (the final stage). Both stages are gated by
//! the per-root pass-generation stamp + the slot `reach_epoch`.

use super::*;
use crate::provider_sync::ProviderSyncState;
use crate::type_provider::mock::{MockCall, MockTypeProvider};
use crate::ProjectSyncMode;
use std::sync::Arc;

const OVERLAY: &str = "/ws/Shared.d.vue.ts";

fn closed_overlay(provider: &MockTypeProvider, path: &str) -> bool {
    provider
        .file_sync_calls()
        .iter()
        .any(|call| matches!(call, MockCall::CloseFile { path: p } if p == path))
}

/// THE root-cause stage (codex ruling Q1/Q5): a STALE closure pass's
/// `reconcile_root_reachability` must NOT remove a reaching edge a NEWER pass
/// established. Slot reached by root `R` whose edge was stamped by pass gen 7
/// (newer); a pass running at gen 5 (older/stale) whose analysis no longer
/// reaches the overlay attempts to drop `R`. The drop must be IGNORED — the
/// stale pass's analysis is out of date for that edge — leaving the slot live
/// and returning NO close target.
#[test]
fn stale_pass_does_not_remove_a_newer_passes_reaching_edge() {
    let owner = DeclOverlayOwner::default();
    // R reaches OVERLAY, edge established by the NEWER pass (gen 7).
    owner.test_seed_slot_with_pass(OVERLAY, &[("R", 7)], 1);
    assert_eq!(owner.test_slot_root_generation(OVERLAY, "R"), Some(7));

    // The STALE pass (gen 5) reconciles R against a closure that no longer
    // includes OVERLAY (R-v1 did not reach it). It MUST NOT drop R's edge.
    let now_unreferenced = owner.reconcile_root_reachability("R", &[], 5);

    assert!(
        now_unreferenced.is_empty(),
        "a stale pass (gen 5) must NOT drain a slot whose edge a newer pass (gen 7) \
             established — got close targets {now_unreferenced:?}"
    );
    assert_eq!(
        owner.test_slot_roots(OVERLAY),
        Some(["R".to_string()].into_iter().collect()),
        "the stale pass must LEAVE the newer pass's reaching edge intact"
    );
    assert_eq!(
        owner.test_slot_root_generation(OVERLAY, "R"),
        Some(7),
        "the newer pass's edge stamp must be unchanged by the stale reconcile"
    );

    // POSITIVE control: a pass AT OR NEWER than the edge (gen 7) that no longer
    // reaches the overlay DOES drop the edge (a current pass owns its removals).
    let drained = owner.reconcile_root_reachability("R", &[], 7);
    assert_eq!(
        drained.len(),
        1,
        "a current pass (gen 7 == edge stamp) that no longer reaches the overlay DOES \
             drop the edge and drain the slot"
    );
    assert_eq!(drained[0].decl_path, OVERLAY);
    assert!(
        owner.test_slot_roots(OVERLAY).unwrap().is_empty(),
        "the current pass's removal drains the slot to an empty tombstone"
    );
}

/// THE SYMMETRIC ADD gate (codex ruling): a STALE closure pass's
/// `reconcile_root_reachability` ADD loop must NOT re-record a reaching edge a
/// NEWER pass already reconciled away. This is the peer of
/// `stale_pass_does_not_remove_a_newer_passes_reaching_edge` above: that test
/// proves a stale pass cannot REMOVE a newer edge; this one proves a stale pass
/// cannot RE-ADD an edge a newer pass dropped. Without the per-root authoritative
/// high-water gate the ADD side was asymmetric — a stale pass re-recorded the
/// edge a newer pass had determined unreachable, resurrecting it until the next
/// reconcile of that root (potentially never).
#[test]
fn stale_pass_does_not_re_add_an_edge_a_newer_pass_reconciled_away() {
    let owner = DeclOverlayOwner::default();
    // R reaches OVERLAY, edge established at pass gen 5 (an earlier real open).
    owner.test_seed_slot_with_pass(OVERLAY, &[("R", 5)], 1);

    // A NEWER pass (gen 7) authoritatively reconciles R to a closure that NO
    // LONGER includes OVERLAY: R's edge (stamp 5 <= 7) is dropped, the slot drains
    // to an empty tombstone, and R's authoritative high-water advances to 7.
    let drained = owner.reconcile_root_reachability("R", &[], 7);
    assert_eq!(drained.len(), 1, "the newer pass drained the slot");
    assert!(
        owner
            .test_slot_roots(OVERLAY)
            .unwrap_or_default()
            .is_empty(),
        "the newer pass left OVERLAY's reaching set empty"
    );
    assert_eq!(
        owner.test_root_authoritative_epoch("R"),
        7,
        "the newer pass advanced R's authoritative high-water to 7"
    );

    // The STALE pass (gen 5) reconciles R to a closure that STILL reaches OVERLAY
    // (its analysis is out of date). The ADD must be GATED: R must NOT be
    // re-recorded into OVERLAY, and the stale pass decides NO close.
    let stale = owner.reconcile_root_reachability("R", &[OVERLAY.to_string()], 5);
    assert!(
        stale.is_empty(),
        "a stale reconcile decides no closes — got {stale:?}"
    );
    assert!(
        owner
            .test_slot_roots(OVERLAY)
            .unwrap_or_default()
            .is_empty(),
        "a stale pass (gen 5) must NOT re-add R to an overlay the newer pass (gen 7) \
             reconciled away — the symmetric ADD gate; roots={:?}",
        owner.test_slot_roots(OVERLAY)
    );

    // POSITIVE control: a CURRENT pass (gen 9 >= the high-water) DOES re-add R, so
    // the gate is not a blanket skip — a genuinely-current closure still grows the
    // graph.
    let current = owner.reconcile_root_reachability("R", &[OVERLAY.to_string()], 9);
    assert!(current.is_empty(), "an add does not drain the slot");
    assert_eq!(
        owner.test_slot_roots(OVERLAY),
        Some(["R".to_string()].into_iter().collect()),
        "a current pass (gen 9, at/over the high-water) re-adds R — the gate admits \
             authoritative adds"
    );
    assert_eq!(
        owner.test_root_authoritative_epoch("R"),
        9,
        "the current pass advanced R's authoritative high-water to 9"
    );
}

/// THE final-stage gate: even with an empty reaching set at the close gate, a
/// close DECIDED by a stale pass (gen 5) must be SUPERSEDED when the slot's
/// `reach_epoch` advanced past the deciding pass (a newer pass re-reached it).
/// Drives `guarded_close` directly with a stale decision against a slot whose
/// `reach_epoch` is newer.
///
/// The carrier owner state carries the LIVE Decl overlay (`decl_path = OVERLAY`,
/// `Decl` background-loaded), exactly as a real open records it — so an ungated
/// close would have a real provider close to issue AND real owner state to strip.
/// The supersession gate must (a) issue NO provider close, (b) leave the seeded
/// provider state UNTOUCHED (the `decl_path`/`Decl`-loaded strip runs only on a
/// non-superseded close), and (c) leave the owner slot intact (a superseded close
/// GCs nothing). All three are the superseded-close end-state contract.
#[tokio::test(flavor = "multi_thread")]
async fn guarded_close_superseded_when_a_newer_pass_reaches_the_overlay() {
    let provider = Arc::new(MockTypeProvider::new());
    let sync = ProjectSync::new(provider.clone(), ProjectSyncMode::FullProject);
    let states: DashMap<String, ProviderSyncState> = DashMap::new();
    let owner = DeclOverlayOwner::default();

    // Seed the carrier owner state carrying the live Decl overlay, so the close
    // path has a real provider close to issue and real state to strip if it is NOT
    // gated — the ungated close otherwise has nothing to act on and the gate is
    // untested. (Mirror `unreached_overlay_is_eventually_closed_by_a_live_pass`.)
    let mut state = ProviderSyncState {
        decl_path: Some(OVERLAY.to_string()),
        ..Default::default()
    };
    state.set_background_loaded(ProviderPathKind::Decl, true);
    states.insert("R".to_string(), state);

    // A stale pass (gen 5) drained the slot and DECIDED a close at pass gen 5.
    // (`release_root` decides at MAX, so craft the stale decision directly.)
    owner.test_seed_slot_with_pass(OVERLAY, &[("R", 5)], 3);
    let stale_decision = owner.reconcile_open_roots(&HashSet::new(), 5);
    assert_eq!(stale_decision.len(), 1, "the stale pass drained the slot");
    assert_eq!(stale_decision[0].decided_pass_generation, 5);

    // Before the stale close runs, a NEWER pass (gen 9) re-reaches the overlay,
    // advancing `reach_epoch` to 9 (then its own removal drains the set again, so
    // the set LOOKS empty at the gate — the ABA the bare set cannot catch).
    owner.test_seed_slot_with_pass(OVERLAY, &[("R", 9)], 3);
    let _ = owner.reconcile_root_reachability("R", &[], 9); // drains set, reach_epoch stays 9
    assert_eq!(
        owner.test_slot_reach_epoch(OVERLAY),
        9,
        "the newer pass advanced reach_epoch past the stale decision's gen 5"
    );

    owner.guarded_close(&sync, &states, &stale_decision).await;

    // (a) No provider close: the ungated close WOULD issue `CloseFile(OVERLAY)`
    // (the seeded live overlay gives it a real target); the gate skips it.
    assert!(
        !closed_overlay(&provider, OVERLAY),
        "a close decided by a stale pass (gen 5) must be SKIPPED once a newer pass \
             (gen 9) reached the overlay — even with an empty set (reach_epoch ABA); \
             calls={:?}",
        provider.file_sync_calls()
    );
    // (b) The seeded owner state is UNTOUCHED — the Decl-strip runs only on a
    // non-superseded close, so a superseded close must not orphan the live overlay
    // from its carrier state.
    let preserved = states.get("R").expect("the carrier state must survive");
    assert_eq!(
        preserved.decl_path.as_deref(),
        Some(OVERLAY),
        "a superseded close must NOT strip the live decl_path from the carrier state"
    );
    assert!(
        preserved.decl_background_loaded,
        "a superseded close must leave the Decl kind background-loaded"
    );
    drop(preserved);
    // (c) The owner slot is intact — a superseded close GCs nothing (the live
    // overlay a newer pass reaches must keep its slot for that pass to own).
    assert_eq!(
        owner.test_slot_reach_epoch(OVERLAY),
        9,
        "a superseded close must leave the owner slot (and its reach_epoch) intact"
    );
}

/// THE inverse invariant: a genuinely-unreached overlay IS eventually closed by a
/// live pass. A pass at gen 7 (== the slot's reach_epoch — no newer pass) drains
/// the slot and its close is NOT superseded, so the provider close fires and the
/// tombstone is GC'd.
#[tokio::test(flavor = "multi_thread")]
async fn unreached_overlay_is_eventually_closed_by_a_live_pass() {
    let provider = Arc::new(MockTypeProvider::new());
    let sync = ProjectSync::new(provider.clone(), ProjectSyncMode::FullProject);
    let states: DashMap<String, ProviderSyncState> = DashMap::new();
    let owner = DeclOverlayOwner::default();

    // The carrier owner state carrying the live Decl overlay (so guarded_close
    // actually attempts the provider close).
    let mut state = ProviderSyncState {
        decl_path: Some(OVERLAY.to_string()),
        ..Default::default()
    };
    state.set_background_loaded(ProviderPathKind::Decl, true);
    states.insert("R".to_string(), state);

    // R reaches OVERLAY at pass gen 7; the SAME pass (gen 7) reconciles against a
    // live root set that no longer contains R (R closed) — it drains the slot,
    // and since reach_epoch (7) == the deciding pass (7), the close is NOT
    // superseded.
    owner.test_seed_slot_with_pass(OVERLAY, &[("R", 7)], 4);
    let decision = owner.reconcile_open_roots(&HashSet::new(), 7);
    assert_eq!(
        decision.len(),
        1,
        "the live pass drained the unreached overlay"
    );
    assert_eq!(decision[0].decided_pass_generation, 7);

    owner.guarded_close(&sync, &states, &decision).await;

    assert!(
        closed_overlay(&provider, OVERLAY),
        "a genuinely-unreached overlay (no newer pass reaches it) IS closed by the \
             live pass — the gate is not a blanket skip; calls={:?}",
        provider.file_sync_calls()
    );
    assert_eq!(
        owner.test_slot_roots(OVERLAY),
        None,
        "a confirmed close GCs the empty tombstone slot"
    );
}

/// The `did_close`-side release (the current ownership truth) is NEVER superseded
/// by a stale-pass `reach_epoch`: it decides at `LIVE_RELEASE_PASS_GENERATION`
/// (`u64::MAX`), so even a high `reach_epoch` cannot block a genuinely-closed
/// root's overlay close.
#[tokio::test(flavor = "multi_thread")]
async fn did_close_release_is_never_superseded_by_reach_epoch() {
    let provider = Arc::new(MockTypeProvider::new());
    let sync = ProjectSync::new(provider.clone(), ProjectSyncMode::FullProject);
    let states: DashMap<String, ProviderSyncState> = DashMap::new();
    let owner = DeclOverlayOwner::default();

    let mut state = ProviderSyncState {
        decl_path: Some(OVERLAY.to_string()),
        ..Default::default()
    };
    state.set_background_loaded(ProviderPathKind::Decl, true);
    states.insert("R".to_string(), state);

    // OVERLAY reached ONLY by R, edge stamped at a high pass gen (99). R closes
    // (did_close): release_root drains the slot and decides at MAX.
    owner.test_seed_slot_with_pass(OVERLAY, &[("R", 99)], 2);
    let decision = owner.release_root("R");
    assert_eq!(decision.len(), 1);
    assert_eq!(
        decision[0].decided_pass_generation, LIVE_RELEASE_PASS_GENERATION,
        "a did_close release decides at the live-release sentinel (u64::MAX)"
    );

    owner.guarded_close(&sync, &states, &decision).await;

    assert!(
        closed_overlay(&provider, OVERLAY),
        "a did_close release of a genuinely-closed root MUST close the overlay even \
             with a high reach_epoch (99) — the release is the current truth, never \
             superseded by a stale-pass stamp; calls={:?}",
        provider.file_sync_calls()
    );
}

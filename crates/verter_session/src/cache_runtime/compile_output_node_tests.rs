//! Inline tests for the typed compile-output cache nodes.
//!
//! Pin the shape of [`CompileOutputNodePureContent`] and
//! [`CompileOutputNodeFactValidatedSession`] independent of any
//! particular host wiring: the pure-content node owns its own
//! `DashMap`; the fact-validated session node delegates storage to a
//! caller-supplied [`ProfileState`] and validates the slot against
//! live override / semantic hashes and a closure-driven fact rail.

use std::sync::Arc;

use rustc_hash::FxHashMap;

use super::{
    CompileOutputNodeFactValidatedSession, CompileOutputNodePureContent,
    CompileOutputPureContentKey, CompileOutputValue, SessionPublishOutcome,
};
use crate::cache_runtime::admission::SignatureAdmission;
use crate::fact_signature_helpers::{empty_fact_signature, ReadSetSignature};
use crate::resolver_core::FactVersionRef;
use crate::types::{CachedVirtualFile, DiagnosticsSnapshot, Hash16, ProfileState, VirtualNodeKind};

fn k(canonical: &str, content: Hash16) -> CompileOutputPureContentKey {
    CompileOutputPureContentKey {
        canonical_id: Arc::<str>::from(canonical),
        content_hash: content,
        parse_env_hash: [0x11; 16],
        resolve_env_hash: [0x22; 16],
        type_env_hash: [0x33; 16],
        lib_env_hash: [0x44; 16],
        project_identity: [0x55; 16],
        compile_cache_mode_hash: [0x66; 16],
        source_map_policy_hash: [0x77; 16],
        compiler_version: [0x88; 16],
        plugin_versions: [0x99; 16],
    }
}

fn value(semantic_hash: Hash16) -> CompileOutputValue {
    CompileOutputValue::from_compile_record(
        semantic_hash,
        0u64,
        0u64,
        FxHashMap::default(),
        DiagnosticsSnapshot::default(),
        None,
        None,
        None,
    )
}

#[test]
fn pure_content_node_starts_empty_and_peek_misses() {
    let node = CompileOutputNodePureContent::new();
    assert_eq!(node.entry_count(), 0);
    assert!(node.peek(&k("/a.vue", [0u8; 16])).is_none());
}

#[test]
fn pure_content_publish_admits_value_addressable_by_full_key() {
    let node = CompileOutputNodePureContent::new();
    let key = k("/a.vue", [1u8; 16]);
    node.publish_content(key.clone(), value([0xAA; 16]), 7);
    let hit = node.peek(&key).expect("warm hit after publish");
    assert_eq!(hit.semantic_hash, [0xAA; 16]);

    // Different content_hash → distinct key → no hit.
    let other = k("/a.vue", [2u8; 16]);
    assert!(node.peek(&other).is_none());

    // Distinct parse_env_hash → distinct key → no hit.
    let mut other = key.clone();
    other.parse_env_hash = [0xFF; 16];
    assert!(node.peek(&other).is_none());
}

#[test]
fn pure_content_remove_drops_entry() {
    let node = CompileOutputNodePureContent::new();
    let key = k("/a.vue", [3u8; 16]);
    node.publish_content(key.clone(), value([0xBB; 16]), 9);
    assert_eq!(node.entry_count(), 1);
    node.remove(&key);
    assert_eq!(node.entry_count(), 0);
    assert!(node.peek(&key).is_none());
}

#[test]
fn session_node_misses_when_no_slot_present() {
    let node = CompileOutputNodeFactValidatedSession::new();
    let state = ProfileState::default();
    let semantic = [0u8; 16];
    let hit = node.lookup(&state, 42, &semantic, 0, 0, || Some(()), |_, _| true);
    assert!(hit.is_none(), "no slot for profile_hash → no warm hit");
}

#[test]
fn session_publish_then_lookup_round_trips_under_matching_hashes() {
    let node = CompileOutputNodeFactValidatedSession::new();
    let mut state = ProfileState::default();
    let semantic = [0x12u8; 16];
    let admission = SignatureAdmission::Cacheable(ReadSetSignature::new(empty_fact_signature()));
    let outcome = node.publish(&mut state, 42, admission, value(semantic), 0);
    assert_eq!(outcome, SessionPublishOutcome::Admitted);
    let hit = node.lookup(&state, 42, &semantic, 0, 0, || Some(()), |_, _| true);
    assert!(hit.is_some(), "matching hashes → warm hit");
}

#[test]
fn session_lookup_misses_when_semantic_hash_differs() {
    let node = CompileOutputNodeFactValidatedSession::new();
    let mut state = ProfileState::default();
    let semantic = [0x12u8; 16];
    let admission = SignatureAdmission::Cacheable(ReadSetSignature::new(empty_fact_signature()));
    node.publish(&mut state, 42, admission, value(semantic), 0);
    // Live semantic_hash differs → miss.
    let other = [0xFF; 16];
    let hit = node.lookup(&state, 42, &other, 0, 0, || Some(()), |_, _| true);
    assert!(
        hit.is_none(),
        "differing semantic_hash MUST miss the warm slot"
    );
}

#[test]
fn session_lookup_misses_when_validate_facts_returns_false() {
    let node = CompileOutputNodeFactValidatedSession::new();
    let mut state = ProfileState::default();
    let semantic = [0x12u8; 16];
    let facts: Arc<[FactVersionRef]> = Arc::from(vec![FactVersionRef::FileWholeHash {
        canonical_id: "/dep.ts".to_string(),
        hash: [0xAB; 16],
    }]);
    let admission = SignatureAdmission::Cacheable(ReadSetSignature::new(facts));
    node.publish(&mut state, 42, admission, value(semantic), 0);

    let hit = node.lookup(&state, 42, &semantic, 0, 0, || Some(()), |_, _sig| false);
    assert!(
        hit.is_none(),
        "fact-validation closure returning false MUST miss the warm slot"
    );

    let hit = node.lookup(&state, 42, &semantic, 0, 0, || Some(()), |_, _sig| true);
    assert!(
        hit.is_some(),
        "fact-validation closure returning true → warm hit"
    );
}

#[test]
fn session_lookup_skips_acquire_view_until_cheap_predicates_pass() {
    use std::cell::Cell;

    // The store-view read is the expensive gate. `lookup` must invoke
    // `acquire_view` ONLY after its cheap predicates (slot present for the
    // profile, carrier cacheable, semantic/override hashes match) confirm a
    // candidate slot worth validating. A cold miss (no slot) and a hash
    // mismatch must reject WITHOUT calling `acquire_view`; a genuine hit must
    // call it exactly once. This pins, in-module and independent of host
    // wiring, the lazy-acquire contract the compile-path warm-validation sites
    // depend on to avoid a full-workspace sweep on the cold/profile-miss path.
    let node = CompileOutputNodeFactValidatedSession::new();
    let semantic = [0x12u8; 16];

    // 1. No slot for the profile → acquire_view must NOT run.
    let empty = ProfileState::default();
    let acquired = Cell::new(0u32);
    let hit = node.lookup(
        &empty,
        42,
        &semantic,
        0,
        0,
        || {
            acquired.set(acquired.get() + 1);
            Some(())
        },
        |_, _| true,
    );
    assert!(hit.is_none(), "no slot for profile_hash → miss");
    assert_eq!(
        acquired.get(),
        0,
        "a cold miss (no slot) MUST reject before paying for the view read"
    );

    // 2. Slot present but the live semantic_hash mismatches → acquire_view
    //    must NOT run (the cheap hash check rejects first).
    let mut state = ProfileState::default();
    let admission = SignatureAdmission::Cacheable(ReadSetSignature::new(empty_fact_signature()));
    node.publish(&mut state, 42, admission, value(semantic), 0);
    let other = [0xFFu8; 16];
    let acquired = Cell::new(0u32);
    let hit = node.lookup(
        &state,
        42,
        &other,
        0,
        0,
        || {
            acquired.set(acquired.get() + 1);
            Some(())
        },
        |_, _| true,
    );
    assert!(hit.is_none(), "hash mismatch → miss");
    assert_eq!(
        acquired.get(),
        0,
        "a hash mismatch MUST reject before paying for the view read"
    );

    // 3. Slot present AND hashes match → acquire_view runs exactly once,
    //    even though the fact rail is empty (the currentness proof gates
    //    every hit, including empty-fact slots).
    let acquired = Cell::new(0u32);
    let hit = node.lookup(
        &state,
        42,
        &semantic,
        0,
        0,
        || {
            acquired.set(acquired.get() + 1);
            Some(())
        },
        |_, _| true,
    );
    assert!(hit.is_some(), "matching hashes + current view → warm hit");
    assert_eq!(
        acquired.get(),
        1,
        "a genuine hit MUST acquire the view exactly once, even for an \
         empty-fact slot"
    );

    // 4. A non-current view (`acquire_view` yields None) misses to cold even
    //    when the cheap predicates pass — the currentness gate.
    let acquired = Cell::new(0u32);
    let hit = node.lookup(
        &state,
        42,
        &semantic,
        0,
        0,
        || {
            acquired.set(acquired.get() + 1);
            None::<()>
        },
        |_, _| true,
    );
    assert!(
        hit.is_none(),
        "a non-current view (acquire_view -> None) MUST miss to cold"
    );
    assert_eq!(
        acquired.get(),
        1,
        "acquire_view runs once even when it then yields a non-current view"
    );
}

#[test]
fn session_publish_non_cacheable_removes_prior_slot() {
    let node = CompileOutputNodeFactValidatedSession::new();
    let mut state = ProfileState::default();
    let semantic = [0x12u8; 16];
    // First publish: cacheable.
    let admission = SignatureAdmission::Cacheable(ReadSetSignature::new(empty_fact_signature()));
    node.publish(&mut state, 42, admission, value(semantic), 0);
    assert!(node
        .lookup(&state, 42, &semantic, 0, 0, || Some(()), |_, _| true)
        .is_some());

    // Second publish: NonCacheable (overflow). Must REMOVE the prior
    // slot so the carrier invariant `present ⇒ admitted cacheable`
    // holds across re-publishes.
    let admission =
        SignatureAdmission::NonCacheable(verter_audit::NonAdmissionReason::SignatureOverflow);
    let outcome = node.publish(&mut state, 42, admission, value(semantic), 1);
    match outcome {
        SessionPublishOutcome::Refused(verter_audit::NonAdmissionReason::SignatureOverflow) => {}
        other => panic!("expected Refused(SignatureOverflow), got {other:?}"),
    }
    assert!(
        node.lookup(&state, 42, &semantic, 0, 0, || Some(()), |_, _| true)
            .is_none(),
        "non-cacheable publish MUST drop the prior slot"
    );
}

#[test]
fn session_peek_signature_round_trips_admitted_signature() {
    let node = CompileOutputNodeFactValidatedSession::new();
    let mut state = ProfileState::default();
    let facts: Arc<[FactVersionRef]> = Arc::from(vec![FactVersionRef::FileWholeHash {
        canonical_id: "/dep.ts".to_string(),
        hash: [0xCD; 16],
    }]);
    let signature = ReadSetSignature::new(facts);
    let admission = SignatureAdmission::Cacheable(signature.clone());
    node.publish(&mut state, 42, admission, value([0u8; 16]), 0);
    let observed = node.peek_signature(&state, 42).expect("admitted signature");
    assert_eq!(observed.facts.len(), 1);
    assert!(!observed.overflowed);
}

/// The last-good rail rides on the same fact-validated slot as the
/// warm-hit candidate: a cross-file edit that invalidates the slot's
/// recorded read set must take the last-good fallback down with it,
/// otherwise `DevServeLastKnownGood` serves output whose semantic
/// inputs are known-changed (stale serve).
#[test]
fn session_peek_last_good_misses_when_validate_facts_returns_false() {
    let node = CompileOutputNodeFactValidatedSession::new();
    let mut state = ProfileState::default();
    let facts: Arc<[FactVersionRef]> = Arc::from(vec![FactVersionRef::FileWholeHash {
        canonical_id: "/dep.ts".to_string(),
        hash: [0xAB; 16],
    }]);
    let admission = SignatureAdmission::Cacheable(ReadSetSignature::new(facts));
    let mut v = value([0x12u8; 16]);
    v.last_good_outputs = Some(FxHashMap::default());
    node.publish(&mut state, 42, admission, v, 0);

    let last_good = node.peek_last_good(&state, 42, |_sig| false);
    assert!(
        last_good.is_none(),
        "fact-validation closure returning false MUST suppress the last-good fallback"
    );

    let last_good = node.peek_last_good(&state, 42, |_sig| true);
    assert!(
        last_good.is_some(),
        "fact-validation closure returning true → last-good fallback available"
    );
}

/// An empty fact rail validates vacuously: the slot pre-dates any
/// cross-file observation, so the last-good fallback stays available
/// without invoking the validator.
#[test]
fn session_peek_last_good_serves_on_empty_fact_rail_without_validator_call() {
    let node = CompileOutputNodeFactValidatedSession::new();
    let mut state = ProfileState::default();
    let admission = SignatureAdmission::Cacheable(ReadSetSignature::new(empty_fact_signature()));
    let mut v = value([0x34u8; 16]);
    v.last_good_outputs = Some(FxHashMap::default());
    node.publish(&mut state, 42, admission, v, 0);

    let last_good = node.peek_last_good(&state, 42, |_sig| {
        panic!("validator must not run on an empty fact rail")
    });
    assert!(
        last_good.is_some(),
        "empty fact rail validates vacuously → last-good fallback available"
    );
}

#[test]
fn session_peek_output_returns_per_kind_pair() {
    let node = CompileOutputNodeFactValidatedSession::new();
    let mut state = ProfileState::default();
    let mut outputs: FxHashMap<VirtualNodeKind, CachedVirtualFile> = FxHashMap::default();
    let file = CachedVirtualFile {
        code: Arc::<str>::from("/* main */"),
        source_map: None,
        lang: None,
        meta: crate::types::VirtualMeta::default(),
    };
    outputs.insert(VirtualNodeKind::Main, file.clone());
    let value = CompileOutputValue::from_compile_record(
        [0u8; 16],
        0,
        0,
        outputs,
        DiagnosticsSnapshot::default(),
        None,
        None,
        None,
    );
    let admission = SignatureAdmission::Cacheable(ReadSetSignature::new(empty_fact_signature()));
    node.publish(&mut state, 42, admission, value, 0);
    let (got, _diag) = node
        .peek_output(&state, 42, &VirtualNodeKind::Main)
        .expect("output for Main");
    assert_eq!(&*got.code, "/* main */");
}

// ────────────────────────────────────────────────────────────────
// Content-cache publish/invalidate consistency.
//
// `publish_content` inserts the `entries` row BEFORE its
// `by_canonical` reverse-index member. `remove_canonical` takes the
// reverse-index set and clears each key from `entries`. The ordering
// guarantees that an entry can always be evicted by canonical: a live
// `entries` row always gets a `by_canonical` backref, so a
// `remove_canonical` that observes the canonical evicts the row. The
// inverse order could orphan an `entries` row whose backref was taken
// by a concurrent `remove_canonical` before the row existed, breaching
// the force-recompute contract (the orphan would be permanently
// un-evictable by canonical).
// ────────────────────────────────────────────────────────────────

/// Force-recompute invariant: after `publish_content` then
/// `remove_canonical`, no content entry for the canonical remains
/// peekable. The deterministic before/after contract — a single-thread
/// publish→invalidate cannot reproduce the cross-map race, so this pins
/// the contract the ordering protects rather than the race itself.
#[test]
fn publish_orders_entry_before_reverse_index_so_remove_canonical_always_evicts() {
    let node = CompileOutputNodePureContent::new();

    // Two distinct content versions of the same canonical.
    let key_a = k("/a.vue", [1u8; 16]);
    let key_b = k("/a.vue", [2u8; 16]);
    node.publish_content(key_a.clone(), value([0xA1; 16]), 1);
    node.publish_content(key_b.clone(), value([0xB2; 16]), 1);

    // Both are warm BEFORE invalidation.
    assert_eq!(node.entry_count(), 2, "both content versions published");
    assert!(node.peek(&key_a).is_some());
    assert!(node.peek(&key_b).is_some());

    // A targeted per-canonical invalidation MUST evict every content
    // entry for that canonical — the force-recompute contract.
    node.remove_canonical("/a.vue");
    assert_eq!(
        node.entry_count(),
        0,
        "remove_canonical MUST evict every content entry for the canonical"
    );
    assert!(
        node.peek(&key_a).is_none(),
        "post-invalidation peek MUST miss (force-recompute)"
    );
    assert!(
        node.peek(&key_b).is_none(),
        "post-invalidation peek MUST miss (force-recompute)"
    );

    // A second invalidation for the now-empty canonical is a benign
    // no-op (no panic, count stays 0) — removal is idempotent.
    node.remove_canonical("/a.vue");
    assert_eq!(node.entry_count(), 0);
}

/// Discriminating concurrency test: a publisher thread and an
/// invalidator thread race on one canonical. After both join, a final
/// `remove_canonical` MUST drive `entry_count()` to 0.
///
/// Under the PRE-fix ordering (`by_canonical` inserted before
/// `entries`), an interleaving where `remove_canonical` takes the
/// reverse-index set between the two inserts leaves an `entries` row
/// with no surviving backref: the key was removed from `by_canonical`
/// by the racing `remove_canonical`, but the `entries.insert` lands
/// afterward. That orphan is permanently un-evictable by canonical, so
/// the final `remove_canonical` cannot reach it and `entry_count()`
/// stays > 0 — this test FAILS. Under the fixed ordering every live
/// `entries` row carries a backref, so the final `remove_canonical`
/// evicts all and `entry_count()` is 0.
#[test]
fn concurrent_publish_and_remove_canonical_never_orphans_content_entry() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread;

    let node = Arc::new(CompileOutputNodePureContent::new());
    let canonical = "/race.vue";

    // Many publish/remove cycles to reliably hit the publish-internal
    // window where a concurrent remove_canonical can orphan an entry
    // under the pre-fix ordering.
    const CYCLES: u32 = 20_000;

    let stop = Arc::new(AtomicBool::new(false));

    let publisher = {
        let node = Arc::clone(&node);
        thread::spawn(move || {
            for i in 0..CYCLES {
                let mut content = [0u8; 16];
                content[0..4].copy_from_slice(&i.to_le_bytes());
                node.publish_content(k(canonical, content), value([0xCC; 16]), 1);
            }
        })
    };

    let invalidator = {
        let node = Arc::clone(&node);
        let stop = Arc::clone(&stop);
        thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                node.remove_canonical(canonical);
            }
        })
    };

    publisher.join().expect("publisher thread");
    stop.store(true, Ordering::Relaxed);
    invalidator.join().expect("invalidator thread");

    // Final invalidation with no concurrent publisher. Every entry that
    // was ever published carries a by_canonical backref under the fixed
    // ordering, so this MUST evict all of them.
    node.remove_canonical(canonical);
    assert_eq!(
        node.entry_count(),
        0,
        "no content entry for the canonical may survive a final \
         remove_canonical — an orphaned (backref-less) entries row \
         would breach the force-recompute contract"
    );
}

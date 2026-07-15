//! Split-brain / warm-cache discrimination tests.
//!
//! The warm cache exists to make a stale SHARED decision UNREACHABLE the moment
//! any dimension it depends on changes — above all a reconnect, which must never
//! reuse a prior `--api` handle / snapshot / carrier state. Every test pins a
//! concrete stale-reuse hazard: a bumped reconnect generation still hitting the
//! prior entry, a changed key dimension silently reusing, an OWNED decision
//! laundering into the SHARED cache, or an eviction trigger failing to evict.

use std::sync::Arc;

use super::*;
use crate::external_ts::mode::{
    select_component_mode, EngineSessionCandidates, EngineSessionFacts, OwnedSessionFacts,
    ProjectEligibility, RedirectRef, RedirectReferenceGraph, ServeMode, SharedSessionFacts,
};
use crate::file_artifact_store::ProjectIdentity;

fn pid(b: u8) -> ProjectIdentity {
    ProjectIdentity([b; 16])
}

fn facts(version: &str, pin: u64, generation: u64) -> EngineSessionFacts {
    EngineSessionFacts {
        observed_version: Arc::<str>::from(version),
        wire_pin: pin,
        editor_session_generation: generation,
    }
}

/// Candidates whose SHARED session carries `shared_gen` as its reconnect
/// generation (and a matching wire pin), so a reconnect is modeled by bumping it.
fn candidates(shared_gen: u64) -> EngineSessionCandidates {
    EngineSessionCandidates {
        owned: OwnedSessionFacts::new(facts("7.0.1", 1, 0)),
        shared: Some(SharedSessionFacts::new(facts(
            "7.0.1",
            100 + shared_gen,
            shared_gen,
        ))),
    }
}

/// An all-eligible single-project graph.
fn single(id: ProjectIdentity) -> RedirectReferenceGraph {
    let mut g = RedirectReferenceGraph::new();
    g.insert_project(id, ProjectEligibility::Eligible, vec![]);
    g
}

/// An all-eligible two-project component `a → b`.
fn pair(a: ProjectIdentity, b: ProjectIdentity) -> RedirectReferenceGraph {
    let mut g = RedirectReferenceGraph::new();
    g.insert_project(
        a,
        ProjectEligibility::Eligible,
        vec![RedirectRef::Resolved(b)],
    );
    g.insert_project(b, ProjectEligibility::Eligible, vec![]);
    g
}

/// A SHARED decision for the single-project component of `id` at reconnect
/// generation `shared_gen`.
fn shared_single(id: ProjectIdentity, shared_gen: u64) -> ComponentModeDecision {
    let g = single(id);
    let d = select_component_mode(&g, &id, &candidates(shared_gen));
    assert_eq!(d.mode(), ServeMode::Shared, "test fixture must be SHARED");
    d
}

// ── Reconnect ALWAYS mints a fresh, unreachable-prior identity ──

/// A reconnect (bumped `editor_session_generation`) produces a FRESH
/// `EngineIdentity` that is NOT equal to the prior, so the reconnect key does NOT
/// hit the prior warm entry — the split-brain "reuse a stale `--api` handle
/// across a reconnect" hazard is unrepresentable: the lookup MISSES.
#[test]
fn reconnect_generation_bump_makes_prior_entry_unreachable() {
    let a = pid(1);
    let mut cache = EngineWarmCache::new();

    let v1 = shared_single(a, 3);
    let key_v1 = WarmCacheKey::for_decision(&v1, "c:/repo/a/tsconfig.json", 1, a);
    cache.insert_shared(key_v1.clone(), v1.clone()).unwrap();
    assert!(cache.get(&key_v1).is_some(), "the fresh entry is reachable");

    // Reconnect: a new control session mints generation 4 → a fresh identity.
    let v2 = shared_single(a, 4);
    assert_ne!(
        v2.engine(),
        v1.engine(),
        "a reconnect mints a fresh engine identity"
    );
    let key_v2 = WarmCacheKey::for_decision(&v2, "c:/repo/a/tsconfig.json", 1, a);
    assert_ne!(
        key_v1, key_v2,
        "the reconnect key differs from the prior key"
    );
    assert!(
        cache.get(&key_v2).is_none(),
        "the reconnect key MISSES — a stale --api handle is never reused across a reconnect"
    );
}

/// Purging superseded generations drops the orphaned older-generation entry
/// while leaving the current one, so a lingering pre-reconnect entry cannot be
/// found by a stale key either.
#[test]
fn reconnect_supersession_purges_only_older_generations() {
    let a = pid(1);
    let mut cache = EngineWarmCache::new();

    let old = shared_single(a, 3);
    let new = shared_single(a, 4);
    let key_old = WarmCacheKey::for_decision(&old, "c:/repo/a/tsconfig.json", 1, a);
    let key_new = WarmCacheKey::for_decision(&new, "c:/repo/a/tsconfig.json", 1, a);
    cache.insert_shared(key_old.clone(), old).unwrap();
    cache.insert_shared(key_new.clone(), new).unwrap();
    assert_eq!(cache.len(), 2);

    let removed = cache.evict_superseded_generations(a, ReconnectGeneration(4));
    assert_eq!(removed, 1, "exactly the older-generation entry is purged");
    assert!(
        cache.get(&key_old).is_none(),
        "the superseded entry is gone"
    );
    assert!(cache.get(&key_new).is_some(), "the current entry survives");
}

/// The warm key is entry-independent: a decision reached from ANY member of a
/// component keys the SAME slot (both anchor on the byte-minimum member), so a
/// warm SHARED decision is reused regardless of which member drove the query —
/// never re-served under a per-entry key.
#[test]
fn warm_key_is_entry_independent_across_component_members() {
    let (a, b) = (pid(1), pid(2));
    let g = pair(a, b);
    let from_a = select_component_mode(&g, &a, &candidates(3));
    let from_b = select_component_mode(&g, &b, &candidates(3));
    assert_eq!(from_a.mode(), ServeMode::Shared);

    let key_from_a = WarmCacheKey::for_decision(&from_a, "c:/repo/a/tsconfig.json", 1, a);
    let key_from_b = WarmCacheKey::for_decision(&from_b, "c:/repo/a/tsconfig.json", 1, a);
    assert_eq!(
        key_from_a, key_from_b,
        "either component member keys the same warm slot"
    );

    let mut cache = EngineWarmCache::new();
    cache.insert_shared(key_from_a.clone(), from_a).unwrap();
    assert!(
        cache.get(&key_from_b).is_some(),
        "a lookup from the other member hits the warm entry"
    );
}

// ── Every key dimension change is a miss ──

/// Changing ANY single key dimension yields a distinct key that MISSES the base
/// warm entry — so a change in identity, tsconfig, config generation, mode,
/// version, wire pin, reconnect generation, or editor binding never silently
/// reuses a stale decision.
#[test]
fn each_key_dimension_change_is_a_miss() {
    let a = pid(1);
    let base_decision = shared_single(a, 3);
    let base_key = WarmCacheKey::for_decision(&base_decision, "c:/repo/a/tsconfig.json", 1, a);

    let mut cache = EngineWarmCache::new();
    cache
        .insert_shared(base_key.clone(), base_decision.clone())
        .unwrap();
    assert!(cache.get(&base_key).is_some());

    // Different component representative (a different project entirely).
    let other = shared_single(pid(2), 3);
    let k = WarmCacheKey::for_decision(&other, "c:/repo/a/tsconfig.json", 1, a);
    assert!(cache.get(&k).is_none(), "different component root misses");

    // Different canonical tsconfig path.
    let k = WarmCacheKey::for_decision(&base_decision, "c:/repo/other/tsconfig.json", 1, a);
    assert_ne!(k, base_key);
    assert!(cache.get(&k).is_none(), "different tsconfig path misses");

    // Different config generation.
    let k = WarmCacheKey::for_decision(&base_decision, "c:/repo/a/tsconfig.json", 2, a);
    assert_ne!(k, base_key);
    assert!(
        cache.get(&k).is_none(),
        "different config generation misses"
    );

    // Different editor-binding witness.
    let k = WarmCacheKey::for_decision(&base_decision, "c:/repo/a/tsconfig.json", 1, pid(9));
    assert_ne!(k, base_key);
    assert!(cache.get(&k).is_none(), "different editor binding misses");

    // Different reconnect generation (fresh EngineIdentity).
    let regen = shared_single(a, 4);
    let k = WarmCacheKey::for_decision(&regen, "c:/repo/a/tsconfig.json", 1, a);
    assert_ne!(k, base_key);
    assert!(
        cache.get(&k).is_none(),
        "different reconnect generation misses"
    );

    // Different serving MODE (an OWNED decision over the same component): the
    // mode axis of the engine identity separates the keys, so an OWNED lookup
    // never hits the SHARED entry.
    let owned = owned_decision(a);
    let k = WarmCacheKey::for_decision(&owned, "c:/repo/a/tsconfig.json", 1, a);
    assert_ne!(
        k, base_key,
        "OWNED and SHARED keys are never equal (mode-keyed)"
    );
    assert!(
        cache.get(&k).is_none(),
        "an OWNED-mode key misses the SHARED entry"
    );
}

/// An OWNED decision over the identical component/facts as a SHARED one is
/// refused warm admission, and its key never collides with the SHARED entry — a
/// reference closure is served by exactly ONE mode, and OWNED facts are never
/// laundered into the SHARED cache.
#[test]
fn owned_decision_is_refused_and_never_collides() {
    let a = pid(1);
    let mut cache = EngineWarmCache::new();

    let owned = owned_decision(a);
    assert_eq!(owned.mode(), ServeMode::Owned);
    let owned_key = WarmCacheKey::for_decision(&owned, "c:/repo/a/tsconfig.json", 1, a);
    assert_eq!(
        cache.insert_shared(owned_key, owned),
        Err(WarmAdmissionError::NotShared),
        "an OWNED decision is never warmed"
    );
    assert!(cache.is_empty(), "the refused OWNED decision left no entry");
}

/// An OWNED decision for the single-project component of `id`: all-eligible but
/// no SHARED session, so it fails closed to OWNED with the OWNED session facts.
fn owned_decision(id: ProjectIdentity) -> ComponentModeDecision {
    let g = single(id);
    let no_shared = EngineSessionCandidates {
        owned: OwnedSessionFacts::new(facts("7.0.1", 1, 0)),
        shared: None,
    };
    let d = select_component_mode(&g, &id, &no_shared);
    assert_eq!(d.mode(), ServeMode::Owned);
    d
}

// ── The eviction trigger set ──

/// Every eviction trigger evicts, at the correct blast radius: a global trigger
/// (editor disconnect, engine exit, protocol mismatch, gate/version change,
/// api-pipe failure) clears the WHOLE cache; a per-component trigger (tsconfig
/// reload, reference-closure change, companion path change, editor-binding
/// mismatch, doc-version change) evicts ONLY the affected component, leaving an
/// unrelated component's warm entry intact.
#[test]
fn every_eviction_trigger_evicts_at_the_correct_scope() {
    let (a, b) = (pid(1), pid(2));

    let global = [
        EvictionTrigger::EditorOrShimDisconnect,
        EvictionTrigger::RealEngineExitOrRestart,
        EvictionTrigger::ControlProtocolMismatch,
        EvictionTrigger::GateOrVersionChange,
        EvictionTrigger::ApiPipeFailure,
    ];
    let scoped = [
        EvictionTrigger::TsconfigOrGraphReload,
        EvictionTrigger::ReferenceClosureChange,
        EvictionTrigger::CompanionPathAppearedOrDisappeared,
        EvictionTrigger::EditorBindingMismatch,
        EvictionTrigger::SourceOrCarrierDocVersionChange,
    ];

    // Global triggers clear everything.
    for trigger in global {
        let mut cache = seeded_two_components(a, b);
        assert_eq!(cache.len(), 2);
        let removed = cache.evict(trigger, a);
        assert_eq!(
            removed, 2,
            "global trigger {trigger:?} clears the whole cache"
        );
        assert!(
            cache.is_empty(),
            "global trigger {trigger:?} leaves nothing"
        );
    }

    // Per-component triggers evict only the affected component.
    for trigger in scoped {
        let mut cache = seeded_two_components(a, b);
        let (key_a, key_b) = seeded_keys(a, b);
        let removed = cache.evict(trigger, a);
        assert_eq!(
            removed, 1,
            "component trigger {trigger:?} evicts one component"
        );
        assert!(
            cache.get(&key_a).is_none(),
            "component trigger {trigger:?} evicts the affected component"
        );
        assert!(
            cache.get(&key_b).is_some(),
            "component trigger {trigger:?} leaves the unrelated component intact"
        );
    }
}

/// Two warm SHARED entries for two DISJOINT single-project components (`a` and
/// `b`), so a component-scoped eviction of `a` can be shown to spare `b`.
fn seeded_two_components(a: ProjectIdentity, b: ProjectIdentity) -> EngineWarmCache {
    let mut cache = EngineWarmCache::new();
    let (key_a, key_b) = seeded_keys(a, b);
    cache.insert_shared(key_a, shared_single(a, 3)).unwrap();
    cache.insert_shared(key_b, shared_single(b, 3)).unwrap();
    cache
}

fn seeded_keys(a: ProjectIdentity, b: ProjectIdentity) -> (WarmCacheKey, WarmCacheKey) {
    let key_a = WarmCacheKey::for_decision(&shared_single(a, 3), "c:/repo/a/tsconfig.json", 1, a);
    let key_b = WarmCacheKey::for_decision(&shared_single(b, 3), "c:/repo/b/tsconfig.json", 1, b);
    (key_a, key_b)
}

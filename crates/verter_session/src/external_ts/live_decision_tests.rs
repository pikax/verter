//! Integration tests for the headless live-decision entry — the whole pipeline
//! (eligibility composition → canonical reference resolution → mode selection →
//! warm-cache serving) exercised end to end, plus failover and reconnect
//! renegotiation. Every test pins a fail-closed / no-split-brain invariant across
//! the composed layers, not a single unit.

use std::sync::Arc;

use super::*;
use crate::external_ts::eligibility::{
    AttachFact, BindingFact, EditorBindingFact, EligibilityFacts, ProxyFact, VersionGateFact,
};
use crate::external_ts::identity_resolver::ReferenceInput;
use crate::external_ts::mode::{
    EngineSessionCandidates, EngineSessionFacts, FailoverCause, OwnedReason, OwnedSessionFacts,
    ServeMode, SharedSessionFacts,
};
use crate::external_ts::warm_cache::{EngineWarmCache, WarmCacheKey};
use crate::file_artifact_store::ProjectIdentity;
use verter_span::path::InjectedPathKey;

fn folded_identity(canonical: &str) -> ProjectIdentity {
    let key = InjectedPathKey::new(canonical);
    ProjectIdentity(xxhash_rust::xxh3::xxh3_128(key.as_str().as_bytes()).to_le_bytes())
}

fn no_realpath(_canonical: &str) -> Option<String> {
    None
}

fn session(version: &str, pin: u64, generation: u64) -> EngineSessionFacts {
    EngineSessionFacts {
        observed_version: Arc::<str>::from(version),
        wire_pin: pin,
        editor_session_generation: generation,
    }
}

/// Candidates whose SHARED session carries `shared_gen` as its reconnect
/// generation, so a reconnect is modeled by bumping it.
fn candidates(shared_gen: u64) -> EngineSessionCandidates {
    EngineSessionCandidates {
        owned: OwnedSessionFacts::new(session("7.0.1", 1, 0)),
        shared: Some(SharedSessionFacts::new(session(
            "7.0.1",
            100 + shared_gen,
            shared_gen,
        ))),
    }
}

/// All-positive SHARED-precondition facts for the project bound to `bound`.
fn positive_facts(bound: ProjectIdentity) -> EligibilityFacts {
    EligibilityFacts {
        version_gate: VersionGateFact::Cleared {
            observed_version: Arc::<str>::from("7.0.1"),
        },
        attach: AttachFact::Live(SharedSessionFacts::new(session("7.0.1", 7, 3))),
        binding: BindingFact::Bound(bound),
        proxy: ProxyFact::Available,
        editor_binding: EditorBindingFact::Matched(bound),
    }
}

const APP_TSCONFIG: &str = "c:/repo/app/tsconfig.json";
const LIB_TSCONFIG: &str = "c:/repo/lib/tsconfig.json";

fn app_id() -> ProjectIdentity {
    folded_identity(APP_TSCONFIG)
}
fn lib_id() -> ProjectIdentity {
    folded_identity(LIB_TSCONFIG)
}

/// A two-project snapshot: `app` (referencing `../lib`) and `lib`. The caller
/// supplies each project's facts so a test can knock one ineligible.
fn app_lib_projects<'a>(
    app_refs: &'a [ReferenceInput],
    lib_refs: &'a [ReferenceInput],
    app_facts: EligibilityFacts,
    lib_facts: EligibilityFacts,
) -> Vec<LiveProjectInput<'a>> {
    vec![
        LiveProjectInput {
            identity: app_id(),
            tsconfig_dir: "c:/repo/app",
            canonical_tsconfig: Arc::<str>::from(APP_TSCONFIG),
            facts: app_facts,
            references: app_refs,
        },
        LiveProjectInput {
            identity: lib_id(),
            tsconfig_dir: "c:/repo/lib",
            canonical_tsconfig: Arc::<str>::from(LIB_TSCONFIG),
            facts: lib_facts,
            references: lib_refs,
        },
    ]
}

// ── SHARED serving: cold establish, then warm reuse ──

/// An all-eligible two-project component decides SHARED, establishes the serving
/// state COLD on the first call, and REUSES it WARM on the second — the whole
/// pipeline (eligibility + reference resolution + mode + warm cache) composed.
#[test]
fn decide_live_serves_shared_cold_then_warm() {
    let app_refs = [ReferenceInput::redirect_on("../lib")];
    let lib_refs: [ReferenceInput; 0] = [];
    let projects = app_lib_projects(
        &app_refs,
        &lib_refs,
        positive_facts(app_id()),
        positive_facts(lib_id()),
    );
    let engines = candidates(3);
    let request = LiveDecisionRequest {
        root: app_id(),
        projects: &projects,
        engines: &engines,
        config_generation: 1,
        editor_binding: app_id(),
    };
    let mut cache = EngineWarmCache::new();

    let first = decide_live(&request, &no_realpath, &folded_identity, &mut cache);
    assert_eq!(first.mode(), ServeMode::Shared);
    assert_eq!(first.serving(), ServingProvenance::ColdShared);
    assert_eq!(
        cache.len(),
        1,
        "the cold decision established one warm entry"
    );

    let second = decide_live(&request, &no_realpath, &folded_identity, &mut cache);
    assert_eq!(second.mode(), ServeMode::Shared);
    assert_eq!(
        second.serving(),
        ServingProvenance::WarmShared,
        "the second call reuses the warm serving state"
    );
    assert_eq!(cache.len(), 1, "no duplicate entry was created");
    assert_eq!(first.decision(), second.decision());
}

// ── Fail-closed: an ineligible member or an unresolved reference ──

/// A single ineligible member fails the WHOLE component closed to OWNED, and the
/// OWNED decision is never cached — fail-open would have cached a SHARED decision
/// for an under-evidenced component.
#[test]
fn decide_live_fails_closed_when_a_member_is_ineligible() {
    let app_refs = [ReferenceInput::redirect_on("../lib")];
    let lib_refs: [ReferenceInput; 0] = [];
    // `lib` cannot interpose the editor connection → ProxyUnavailable.
    let mut lib_facts = positive_facts(lib_id());
    lib_facts.proxy = ProxyFact::Unavailable;
    let projects = app_lib_projects(&app_refs, &lib_refs, positive_facts(app_id()), lib_facts);
    let engines = candidates(3);
    let request = LiveDecisionRequest {
        root: app_id(),
        projects: &projects,
        engines: &engines,
        config_generation: 1,
        editor_binding: app_id(),
    };
    let mut cache = EngineWarmCache::new();

    let decision = decide_live(&request, &no_realpath, &folded_identity, &mut cache);
    assert_eq!(decision.mode(), ServeMode::Owned);
    assert_eq!(decision.serving(), ServingProvenance::Owned);
    assert_eq!(
        decision.decision().owned_reason(),
        Some(OwnedReason::ProxyUnavailable),
        "the ineligible member's precise reason surfaces"
    );
    assert!(cache.is_empty(), "an OWNED decision is never cached");
}

/// An unresolvable reference (unsupported scheme) poisons SHARED: the component
/// fails closed to OWNED/`IncompleteComponent`, and nothing is cached.
#[test]
fn decide_live_fails_closed_on_unresolved_reference() {
    let app_refs = [ReferenceInput::redirect_on("untitled:broken")];
    let projects = [LiveProjectInput {
        identity: app_id(),
        tsconfig_dir: "c:/repo/app",
        canonical_tsconfig: Arc::<str>::from(APP_TSCONFIG),
        facts: positive_facts(app_id()),
        references: &app_refs,
    }];
    let engines = candidates(3);
    let request = LiveDecisionRequest {
        root: app_id(),
        projects: &projects,
        engines: &engines,
        config_generation: 1,
        editor_binding: app_id(),
    };
    let mut cache = EngineWarmCache::new();

    let decision = decide_live(&request, &no_realpath, &folded_identity, &mut cache);
    assert_eq!(decision.mode(), ServeMode::Owned);
    assert_eq!(
        decision.decision().owned_reason(),
        Some(OwnedReason::IncompleteComponent)
    );
    assert!(cache.is_empty());
}

// ── Reconnect renegotiation: always fresh, prior evicted ──

/// A reconnect (bumped generation) recomputes to a FRESH engine identity, so the
/// serving state is always re-established COLD (never a warm reuse), the prior
/// generation's warm entry is purged, and only the fresh entry remains — the
/// split-brain "reuse the pre-reconnect --api handle" hazard is closed.
#[test]
fn reconnect_renegotiation_is_always_cold_and_evicts_prior() {
    let app_refs = [ReferenceInput::redirect_on("../lib")];
    let lib_refs: [ReferenceInput; 0] = [];

    let projects = app_lib_projects(
        &app_refs,
        &lib_refs,
        positive_facts(app_id()),
        positive_facts(lib_id()),
    );
    let mut cache = EngineWarmCache::new();

    // Initial SHARED serving at generation 3.
    let engines_v1 = candidates(3);
    let request_v1 = LiveDecisionRequest {
        root: app_id(),
        projects: &projects,
        engines: &engines_v1,
        config_generation: 1,
        editor_binding: app_id(),
    };
    let v1 = decide_live(&request_v1, &no_realpath, &folded_identity, &mut cache);
    assert_eq!(v1.serving(), ServingProvenance::ColdShared);
    let key_v1 = WarmCacheKey::for_decision(
        v1.decision(),
        representative_tsconfig(&request_v1, v1.decision()),
        1,
        app_id(),
    );
    assert!(cache.get(&key_v1).is_some());

    // Reconnect at generation 4.
    let engines_v2 = candidates(4);
    let request_v2 = LiveDecisionRequest {
        root: app_id(),
        projects: &projects,
        engines: &engines_v2,
        config_generation: 1,
        editor_binding: app_id(),
    };
    let v2 = renegotiate_on_reconnect(&request_v2, &no_realpath, &folded_identity, &mut cache);
    assert_eq!(v2.mode(), ServeMode::Shared);
    assert_eq!(
        v2.serving(),
        ServingProvenance::ColdShared,
        "a reconnect always re-establishes fresh, never reuses warm"
    );
    assert_ne!(
        v1.decision().engine(),
        v2.decision().engine(),
        "the reconnect minted a fresh engine identity"
    );

    assert!(
        cache.get(&key_v1).is_none(),
        "the pre-reconnect entry is evicted — never reachable across a reconnect"
    );
    let key_v2 = WarmCacheKey::for_decision(
        v2.decision(),
        representative_tsconfig(&request_v2, v2.decision()),
        1,
        app_id(),
    );
    assert!(cache.get(&key_v2).is_some(), "the fresh entry is present");
    assert_eq!(cache.len(), 1, "exactly the fresh generation remains");
}

// ── Whole-component failover discards SHARED serving state ──

/// A mid-flight SHARED failure fails the WHOLE component over to OWNED and
/// discards its SHARED warm state, so no stale `--api` handle is reused.
#[test]
fn failover_live_moves_whole_component_and_discards_warm() {
    let app_refs = [ReferenceInput::redirect_on("../lib")];
    let lib_refs: [ReferenceInput; 0] = [];
    let projects = app_lib_projects(
        &app_refs,
        &lib_refs,
        positive_facts(app_id()),
        positive_facts(lib_id()),
    );
    let engines = candidates(3);
    let request = LiveDecisionRequest {
        root: app_id(),
        projects: &projects,
        engines: &engines,
        config_generation: 1,
        editor_binding: app_id(),
    };
    let mut cache = EngineWarmCache::new();

    let shared = decide_live(&request, &no_realpath, &folded_identity, &mut cache);
    assert_eq!(shared.mode(), ServeMode::Shared);
    assert_eq!(cache.len(), 1);

    let owned_session = OwnedSessionFacts::new(session("7.0.1", 1, 0));
    let failed = failover_live(
        shared.decision(),
        FailoverCause::RedirectClosed,
        &owned_session,
        &mut cache,
    );
    assert_eq!(failed.mode(), ServeMode::Owned);
    assert_eq!(failed.owned_reason(), Some(OwnedReason::RedirectClosed));

    // The failover covers the WHOLE component the SHARED decision served.
    let shared_members: Vec<_> = shared.decision().members().members().collect();
    let failed_members: Vec<_> = failed.members().members().collect();
    assert_eq!(failed_members, shared_members);
    assert!(failed_members.contains(&app_id()) && failed_members.contains(&lib_id()));

    assert!(
        cache.is_empty(),
        "the failed-over component's SHARED warm state is discarded"
    );
}

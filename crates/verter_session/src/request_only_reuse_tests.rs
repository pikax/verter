//! The typed `RequestOnly` reuse rail: a prepared-decl bundle that is
//! COMPLETE and deterministic under the request's immutable view but
//! carries a non-cacheable refusal.
//!
//! ## The three-way classification
//!
//! * `Shared` — complete, nothing refused: safe for shared-cache
//!   publication and for request reuse.
//! * `RequestOnly` — complete and deterministic under this immutable
//!   request view, but a DETERMINISTIC non-cacheable read was consumed
//!   (a FENCED `IndexedReady` serve, an unrootable import-route witness,
//!   an unobservable contributor source env). Reusable WITHIN the
//!   request; never publishable.
//! * `NoReuse` — a TRANSIENT refusal (broken decl-body lease, inference
//!   budget stop, preparation failure) or an unattributed refusal
//!   (fact-signature overflow, mutation instability). Not safe even for
//!   request-scoped reuse.
//!
//! ## What reuse must never launder
//!
//! Every cold return, sequential request-memo hit and singleflight
//! FOLLOWER of a `RequestOnly` value replays its stored propagation into
//! the enclosing tracer before returning. Reuse that arrives by dropping
//! the refusal is a taint-laundering regression, not progress: the
//! enclosing compute would warm a shared cache with a value derived from
//! a superseded / unrootable basis.
//!
//! The acceptance distinction the tests below pin:
//!
//! * the RETURNED refusal carries the exact
//!   [`NonCacheableReadReason`](verter_session_query::facts::reuse::NonCacheableReadReason)
//!   and [`NonCacheablePropagation`](verter_session_query::facts::fact_read_set::NonCacheablePropagation);
//! * tracer finalisation exposes only the BOOLEAN — it never records the
//!   reason.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use crate::{HostConfig, UpsertRequest, VerterHost};
use verter_session_query::facts::reuse::NonCacheableReadReason;
use verter_session_query::facts::store_view::StoreView;

fn upsert(host: &VerterHost, path: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(path.to_string()),
            input_id: path.to_string(),
            source: Arc::from(source),
            file_language: crate::LanguageRegistry::global()
                .classify_static(path)
                .static_resolution(),
            aliases: Vec::new(),
        })
        .unwrap_or_else(|e| panic!("upsert {path} failed: {e:?}"));
}

fn cold_flight_runs(host: &VerterHost) -> u64 {
    host.provenance()
        .bundle_cold_flight_runs
        .load(Ordering::Relaxed)
}

/// A published owner whose import witness takes the typed refusal arm.
/// The Decision-DAG contract tests cover real upstream publication
/// refusals; this fixture isolates the downstream `UnrootableRoute`
/// carrier, which remains stable and joinable even though it cannot be
/// shared-admitted.
fn host_with_refused_resolution(root: &str) -> (Arc<VerterHost>, String) {
    let host = VerterHost::new_standalone(HostConfig::default());
    let owner = format!("{root}/owner.ts");
    upsert(
        &host,
        &owner,
        "import type { Missing } from './missing';\n\
         export interface Wrapper { inner: Missing }\n",
    );
    host.test_force
        .force_import_route_witness_refusal_for_tests
        .store(true, Ordering::Relaxed);
    let host = Arc::new(host);
    assert!(
        host.owner_import_route_witness_for_tests(&owner).is_none(),
        "fixture invariant: the forced publication refusal must decline the durable \
         import witness"
    );
    (host, owner)
}

/// `RM-2` — a singleflight FOLLOWER that adopts a leader's retained
/// `RequestOnly` bundle must execute the same replay path as the leader:
/// its own enclosing tracer finalises non-cacheable.
///
/// The leader's `note_non_cacheable_read_fan_out(UnrootableRoute)` ran on
/// the LEADER's thread, inside the LEADER's tracer stack. A follower
/// adopting the retained rendezvous performs no materialisation at all,
/// so without an explicit replay its own enclosing compute sees a clean
/// scope and may warm a shared cache with a value whose basis is not
/// fact-rootable. That is the taint-laundering hole this test closes.
#[test]
fn request_only_singleflight_follower_replays_identical_taint() {
    let (host, owner) = host_with_refused_resolution("/rc_reqonly_follower");

    // One view shared by the leader and the late claimant so both land
    // on the SAME singleflight lane (the lane key folds the compat
    // token).
    let view = host.resolver_store_view_read().into_owned_view();
    // Keep the leader's lane alive past its completion so the late
    // claimant observes exactly what the leader left behind: a retained
    // `Done` rendezvous it joins as a FOLLOWER.
    let lane_pin = host
        .resolver
        .runtime
        .prepared_decl_bundles
        .singleflight()
        .participate(owner.clone(), view.compat_token());

    let before_leader = cold_flight_runs(&host);
    let (leader_bundle, leader_non_cacheable) =
        crate::fact_signature_helpers::with_cacheability_scope(
            &crate::fact_signature_helpers::FactTracerBasisSource::unbound(host.as_ref()),
            |_probe| host.prepared_decl_bundle_with_store_view(&view, None, &owner),
        );
    let leader_flights = cold_flight_runs(&host) - before_leader;
    assert!(
        leader_bundle.is_some(),
        "the leader must still be SERVED its bundle — the refusal is about admission, \
         never about the answer"
    );
    assert_eq!(
        leader_flights, 1,
        "sanity: the leader ran exactly one cold flight body"
    );
    assert!(
        leader_non_cacheable,
        "sanity: the leader's own scope observes the declined witness"
    );

    let before_follower = cold_flight_runs(&host);
    let (follower_bundle, follower_non_cacheable) =
        crate::fact_signature_helpers::with_cacheability_scope(
            &crate::fact_signature_helpers::FactTracerBasisSource::unbound(host.as_ref()),
            |_probe| host.prepared_decl_bundle_with_store_view(&view, None, &owner),
        );
    let follower_flights = cold_flight_runs(&host) - before_follower;
    drop(lane_pin);

    assert!(
        follower_bundle.is_some(),
        "the follower must be served the leader's bundle"
    );
    assert_eq!(
        follower_flights, 0,
        "fixture invariant: the late claimant must ADOPT the leader's retained \
         rendezvous (1 means it re-ran the cold build and this test would prove \
         nothing about follower replay)"
    );
    assert!(
        follower_non_cacheable,
        "`RM-2`: a follower that adopts a RequestOnly rendezvous MUST replay the \
         leader's stored refusal into its OWN enclosing tracer. The leader's fan-out \
         ran on the leader's tracer stack; without the replay the follower's compute \
         reads clean and may warm a shared cache with a value whose import-route basis \
         is not fact-rootable."
    );
}

/// Anti-vacuity control: the SAME follower shape with a ROOTABLE witness
/// stays CLEAN on both threads. Without it, a change that marked every
/// bundle read non-cacheable would satisfy the test above while
/// destroying every warm bundle in the tree.
#[test]
fn shared_singleflight_follower_stays_cacheable() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let owner = "/rc_reqonly_control/owner.ts";
    upsert(
        &host,
        owner,
        "import type { LateType } from './late_dep';\n\
         export type Wrapper = { inner: LateType };\n",
    );
    let host = Arc::new(host);
    assert!(
        host.owner_import_route_witness_for_tests(owner).is_some(),
        "control invariant: one unresolved specifier stays well within \
         FACT_SIGNATURE_CAP and yields a rootable witness"
    );

    let view = host.resolver_store_view_read().into_owned_view();
    let lane_pin = host
        .resolver
        .runtime
        .prepared_decl_bundles
        .singleflight()
        .participate(owner.to_string(), view.compat_token());

    let (leader_bundle, leader_non_cacheable) =
        crate::fact_signature_helpers::with_cacheability_scope(
            &crate::fact_signature_helpers::FactTracerBasisSource::unbound(host.as_ref()),
            |_probe| host.prepared_decl_bundle_with_store_view(&view, None, owner),
        );
    assert!(leader_bundle.is_some(), "the control leader must be served");
    assert!(
        !leader_non_cacheable,
        "control invariant: a rootable witness must leave the leader's scope CLEAN — \
         otherwise the RequestOnly test above proves nothing about the refusal"
    );

    let (follower_bundle, follower_non_cacheable) =
        crate::fact_signature_helpers::with_cacheability_scope(
            &crate::fact_signature_helpers::FactTracerBasisSource::unbound(host.as_ref()),
            |_probe| host.prepared_decl_bundle_with_store_view(&view, None, owner),
        );
    drop(lane_pin);
    assert!(
        follower_bundle.is_some(),
        "the control follower must be served"
    );
    assert!(
        !follower_non_cacheable,
        "the replay must fire ONLY for a RequestOnly value — a Shared bundle read must \
         never taint its reader"
    );
}

/// `RM-2` — the RETURNED refusal, not tracer finalisation, is what
/// carries the reason and the propagation, and it is IDENTICAL on the
/// first touch and the nth.
///
/// Tracer finalisation is a boolean: it says the enclosing compute must
/// not warm a shared cache, and nothing about why. The typed refusal the
/// producer returns is the rail a consumer (a request-scoped memo, a
/// downstream admission gate, the scheduler-boundary carrier) reads to
/// decide policy, so it must survive reuse unchanged rather than degrade
/// to "something was refused".
///
/// Touch 1 is the flight LEADER (a genuine cold materialisation); every
/// later touch adopts the retained rendezvous as a FOLLOWER, so the
/// refusals compared below are a computed one against REUSED ones.
#[test]
fn request_only_first_and_nth_return_same_refusal_reason_and_propagation() {
    let (host, owner) = host_with_refused_resolution("/rc_reqonly_reason");

    let view = host.resolver_store_view_read().into_owned_view();
    let lane_pin = host
        .resolver
        .runtime
        .prepared_decl_bundles
        .singleflight()
        .participate(owner.clone(), view.compat_token());

    let mut refusals = Vec::new();
    let mut flights = Vec::new();
    for _ in 0..3 {
        let before = cold_flight_runs(&host);
        let outcome = host.prepared_decl_bundle_with_reuse_class(&view, None, &owner);
        flights.push(cold_flight_runs(&host) - before);
        assert!(
            outcome.bundle.is_some(),
            "every touch of a RequestOnly bundle is still SERVED"
        );
        let refusal = *outcome.reuse.request_only_refusal().expect(
            "a declined import witness produces a COMPLETE bundle carrying a DETERMINISTIC \
             refusal — the RequestOnly class, neither Shared nor NoReuse",
        );
        refusals.push(refusal);
    }
    drop(lane_pin);

    assert_eq!(
        flights,
        vec![1, 0, 0],
        "fixture invariant: touch 1 is the cold LEADER and touches 2-3 adopt its retained \
         rendezvous — otherwise the refusals below are three computed ones and the test \
         proves nothing about reuse"
    );
    assert_eq!(
        refusals[0].reason(),
        NonCacheableReadReason::UnrootableRoute,
        "the refusal must name the EXACT reason the producer observed, not a generic \
         'non-cacheable' verdict"
    );
    assert_eq!(
        refusals[0].propagation(),
        verter_session_query::facts::fact_read_set::NonCacheablePropagation::Transitive,
        "an unrootable basis taints every enclosing scope that consumes the value"
    );
    for (index, refusal) in refusals.iter().enumerate().skip(1) {
        assert_eq!(
            refusal.reason(),
            refusals[0].reason(),
            "touch {index}: a REUSED RequestOnly value must return the same refusal reason \
             as the cold return"
        );
        assert_eq!(
            refusal.propagation(),
            refusals[0].propagation(),
            "touch {index}: a REUSED RequestOnly value must return the same propagation as \
             the cold return"
        );
    }
}

/// `RM-3` — a `RequestOnly` bundle is reused (0 additional cold flights)
/// AND replays its taint AND still never reaches shared publication.
///
/// The three halves must hold TOGETHER. Reuse alone is satisfiable by
/// admitting the bundle to `prepared_decl_bundles`, which is exactly the
/// regression the fail-closed witness gate exists to prevent. The
/// no-publication half alone is satisfied by the pre-change tree. And
/// reuse WITHOUT the replay is taint laundering.
#[test]
fn request_only_bundle_never_publishes_shared() {
    let (host, owner) = host_with_refused_resolution("/rc_reqonly_publish");

    let view = host.resolver_store_view_read().into_owned_view();
    let lane_pin = host
        .resolver
        .runtime
        .prepared_decl_bundles
        .singleflight()
        .participate(owner.clone(), view.compat_token());

    let before_first = cold_flight_runs(&host);
    let first = host.prepared_decl_bundle_with_store_view(&view, None, &owner);
    let first_flights = cold_flight_runs(&host) - before_first;
    assert!(first.is_some(), "the cold touch must be served");
    assert_eq!(first_flights, 1, "the cold touch runs one flight body");

    let before_second = cold_flight_runs(&host);
    let (second, second_non_cacheable) = crate::fact_signature_helpers::with_cacheability_scope(
        &crate::fact_signature_helpers::FactTracerBasisSource::unbound(host.as_ref()),
        |_probe| host.prepared_decl_bundle_with_store_view(&view, None, &owner),
    );
    let second_flights = cold_flight_runs(&host) - before_second;
    drop(lane_pin);

    assert!(second.is_some(), "the reusing touch must be served");
    assert_eq!(
        second_flights, 0,
        "the reusing touch adopts the retained rendezvous instead of re-running the cold \
         flight body"
    );
    assert!(
        second_non_cacheable,
        "`RM-2`: reuse must carry the refusal — a reused RequestOnly bundle marks its \
         reader's tracer non-cacheable exactly as the cold return did"
    );

    let candidates = host
        .resolver
        .runtime
        .prepared_decl_bundles
        .candidate_signatures_for_key(&owner);
    assert!(
        candidates.is_empty(),
        "`RM-3`: reuse must NEVER become shared publication. A declined import witness \
         admits no warm bundle candidate. Admitted signatures: {candidates:?}"
    );
}

// ---------------------------------------------------------------------------
// Decl-body lease-miss evidence at the collapsing accessors.
//
// A broken decl-body lease collapses to `None` at the plain body accessors.
// The miss is transient, so every traced cold compute that consumed it must
// refuse shared-cache admission and observe the typed `LeaseMiss` reason —
// including when a later fallback supplies a value. A genuine absence
// (`Ready(None)`) stays a cacheable answer.
// ---------------------------------------------------------------------------

/// What a traced compute observed: shared-cache admission, and the typed
/// refusal reason its refusal-observation scope recorded.
struct TracedRead<R> {
    value: R,
    admitted: bool,
    reason: Option<NonCacheableReadReason>,
}

fn traced_read<R>(host: &VerterHost, work: impl FnOnce() -> R) -> TracedRead<R> {
    let ((value, reason), read_set) = host.with_fact_tracer(
        verter_session_query::facts::fact_cache::AggregateBasisSeed::Unvouched,
        || {
            let scope = crate::fact_tracing::RefusalObservationScope::enter();
            let value = work();
            (value, scope.observed())
        },
    );
    let admission = verter_session_query::facts::fact_cache::SignatureAdmission::from_finalise(
        read_set.finalise(),
    );
    TracedRead {
        value,
        admitted: admission.cacheable().is_some(),
        reason,
    }
}

/// Index `path`, lower `A` (pinning the retained parse snapshot), then break
/// the lease so any not-yet-lowered declaration lease-misses.
fn indexed_with_broken_lease(
    host: &VerterHost,
    path: &str,
    source: &str,
    file_language: verter_language::FileLanguage,
) -> Arc<crate::project_type_store::IndexedReady> {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(path.to_string()),
            input_id: path.to_string(),
            source: Arc::from(source),
            file_language,
            aliases: Vec::new(),
        })
        .unwrap_or_else(|e| panic!("upsert {path} failed: {e:?}"));
    let indexed = host.ensure_indexed_ready(path).expect("indexed");
    assert!(
        indexed.shallow_state.type_decl("A").is_some(),
        "A lowers under a live lease, pinning the retained snapshot"
    );
    indexed
        .shallow_state
        .decl_bodies()
        .release_retained_snapshot_for_test();
    indexed
}

const LEASE_FIXTURE: &str = "export type A = { x: number };\n\
     export type B = { y: string };\n\
     export declare const v: number;\n\
     declare global {\n  interface GT { g: number }\n  var gv: boolean;\n}\n";

#[test]
fn a_lease_miss_at_every_collapsing_body_accessor_refuses_shared_admission() {
    use verter_session_query::declarations::AugmentationScopeKind;

    type Probe = fn(&crate::resolver_core::ShallowFileState) -> bool;
    let probes: [(&str, Probe); 5] = [
        ("type", |state| state.type_decl("B").is_some()),
        ("value", |state| state.value_decl("v").is_some()),
        ("owner-qualified value", |state| {
            state
                .value_decl_in(verter_type_expr::TopLevelOwnerId::ordinary_file(), "v")
                .is_some()
        }),
        ("augmentation type", |state| {
            state
                .augmentation_type_decl(&AugmentationScopeKind::Global, "GT")
                .is_some()
        }),
        ("augmentation value", |state| {
            state
                .augmentation_value_decl(&AugmentationScopeKind::Global, "gv")
                .is_some()
        }),
    ];
    for (accessor, probe) in probes {
        let host = VerterHost::new_standalone(HostConfig::default());
        let indexed = indexed_with_broken_lease(
            &host,
            "/lease/d.ts",
            LEASE_FIXTURE,
            verter_language::FileLanguage::script_ts(),
        );
        let read = traced_read(&host, || probe(&indexed.shallow_state));
        assert!(
            !read.value,
            "{accessor}: a broken-lease demand reads as a miss"
        );
        assert!(
            !read.admitted,
            "{accessor}: a compute that consumed a broken-lease miss must be refused \
             shared-cache admission — admitting it would freeze a recoverable miss"
        );
        assert_eq!(
            read.reason,
            Some(NonCacheableReadReason::LeaseMiss),
            "{accessor}: the refusal must carry the typed transient reason"
        );
    }
}

#[test]
fn a_lease_miss_stays_refused_when_a_fallback_supplies_the_value() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let indexed = indexed_with_broken_lease(
        &host,
        "/lease/r.svelte.ts",
        "export declare function $state<T>(initial: T): T;\nexport type A = { x: number };\n",
        verter_language::FileLanguage::adapter_module(
            verter_language::ScriptSourceType::Ts,
            verter_language::FrameworkAdapterId::svelte(),
            verter_language::LanguageId::new(verter_language::SVELTE_RUNE_MODULE_LANGUAGE_ID),
        ),
    );
    let read = traced_read(&host, || {
        indexed
            .shallow_state
            .effective_value_decl("$state")
            .is_some()
    });
    assert!(
        read.value,
        "the user declaration lease-misses, so the rune ambient fallback answers"
    );
    assert!(
        !read.admitted,
        "the fallback's value does not erase the lease miss the compute consumed first: \
         the compute must still be refused shared-cache admission"
    );
    assert_eq!(read.reason, Some(NonCacheableReadReason::LeaseMiss));
}

#[test]
fn a_genuine_absence_stays_cacheable_under_a_broken_lease() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let indexed = indexed_with_broken_lease(
        &host,
        "/lease/absent.ts",
        LEASE_FIXTURE,
        verter_language::FileLanguage::script_ts(),
    );
    let read = traced_read(&host, || {
        indexed.shallow_state.type_decl("Missing").is_none()
            && indexed.shallow_state.value_decl("Missing").is_none()
    });
    assert!(read.value, "an un-inventoried name is a genuine absence");
    assert!(
        read.admitted,
        "a genuine absence is a reproducible answer: it must stay cacheable even while the \
         lease is broken"
    );
    assert_eq!(read.reason, None);
}

fn rune_module_language() -> verter_language::FileLanguage {
    verter_language::FileLanguage::adapter_module(
        verter_language::ScriptSourceType::Ts,
        verter_language::FrameworkAdapterId::svelte(),
        verter_language::LanguageId::new(verter_language::SVELTE_RUNE_MODULE_LANGUAGE_ID),
    )
}

fn assert_refused_as_lease_miss<R>(read: &TracedRead<R>, what: &str) {
    assert!(
        !read.admitted,
        "{what}: a compute that consumed a broken-lease miss must be refused shared-cache \
         admission — admitting it would freeze a recoverable miss"
    );
    assert_eq!(
        read.reason,
        Some(NonCacheableReadReason::LeaseMiss),
        "{what}: the refusal must carry the typed transient reason"
    );
}

#[test]
fn a_lease_miss_through_the_effective_type_lookup_stays_refused() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let indexed = indexed_with_broken_lease(
        &host,
        "/lease/types.svelte.ts",
        "export type A = { x: number };\nexport type B = { y: string };\n",
        rune_module_language(),
    );
    let read = traced_read(&host, || {
        indexed.shallow_state.effective_type_decl("B").is_some()
    });
    assert!(
        !read.value,
        "the user declaration lease-misses and no rune ambient type answers"
    );
    assert_refused_as_lease_miss(&read, "effective type lookup");
}

/// The lazy body-hash facts (`Export` / `LocalDecl`) read declaration bodies
/// through the memo. A broken lease there must refuse the observing compute
/// exactly as a direct body read does, for type and value symbols and for
/// exports whose backing declaration lives under a component instance-script
/// owner (the owner-qualified body read).
#[test]
fn a_lease_miss_behind_a_lazy_body_fact_refuses_shared_admission() {
    use verter_session_query::facts::registry::{FactKey, SymbolSpace};

    fn export(symbol: &str, space: SymbolSpace) -> FactKey {
        FactKey::Export {
            name: crate::file_artifact_store::InternedName::from(symbol),
            space,
        }
    }
    fn local(symbol: &str, space: SymbolSpace) -> FactKey {
        FactKey::LocalDecl {
            name: crate::file_artifact_store::InternedName::from(symbol),
            space,
        }
    }
    let cases: [(&str, &str, verter_language::FileLanguage, FactKey, FactKey); 6] = [
        (
            "exported type",
            "/lease/facts.ts",
            verter_language::FileLanguage::script_ts(),
            export("A", SymbolSpace::Type),
            export("B", SymbolSpace::Type),
        ),
        (
            "local type",
            "/lease/facts.ts",
            verter_language::FileLanguage::script_ts(),
            export("A", SymbolSpace::Type),
            local("L", SymbolSpace::Type),
        ),
        (
            "exported value",
            "/lease/facts.ts",
            verter_language::FileLanguage::script_ts(),
            export("A", SymbolSpace::Type),
            export("v", SymbolSpace::Value),
        ),
        (
            "local value",
            "/lease/facts.ts",
            verter_language::FileLanguage::script_ts(),
            export("A", SymbolSpace::Type),
            local("lv", SymbolSpace::Value),
        ),
        (
            "component-script exported type",
            "/lease/Facts.svelte",
            verter_language::FileLanguage::svelte(),
            export("A", SymbolSpace::Type),
            export("B", SymbolSpace::Type),
        ),
        (
            "component-script exported value",
            "/lease/Facts.svelte",
            verter_language::FileLanguage::svelte(),
            export("A", SymbolSpace::Type),
            export("v", SymbolSpace::Value),
        ),
    ];
    for (what, path, language, pin, probe) in cases {
        let source = if path.ends_with(".svelte") {
            "<script lang=\"ts\">\nexport type A = { x: number };\n\
             export type B = { y: string };\nexport const v: number = 1;\n</script>\n\
             <div></div>\n"
        } else {
            "export type A = { x: number };\nexport type B = { y: string };\n\
             type L = { z: boolean };\nexport declare const v: number;\n\
             declare const lv: string;\n"
        };
        let host = VerterHost::new_standalone(HostConfig::default());
        let _ = host
            .upsert(UpsertRequest {
                canonical_id: Some(path.to_string()),
                input_id: path.to_string(),
                source: Arc::from(source),
                file_language: language,
                aliases: Vec::new(),
            })
            .unwrap_or_else(|e| panic!("upsert {path} failed: {e:?}"));
        let indexed = host.ensure_indexed_ready(path).expect("indexed");
        if path.ends_with(".svelte") {
            for exported in ["B", "v"] {
                assert!(
                    matches!(
                        indexed.shallow_state.exports.get(exported),
                        Some(verter_session_query::inputs::shallow::ExportTarget::Local { owner, .. })
                            if *owner != verter_type_expr::TopLevelOwnerId::ordinary_file()
                    ),
                    "{what}: `{exported}` must be backed by the instance-script owner, so the \
                     fact reads its body through the owner-qualified accessor"
                );
            }
        }
        let artifacts = host
            .exact_current_artifacts_for_test(path, indexed.whole_hash)
            .expect("published artifacts must be readable");
        assert!(
            artifacts.facts.lookup_or_compute(&pin).is_some(),
            "{what}: the pin fact lowers under a live lease, pinning the retained snapshot"
        );
        indexed
            .shallow_state
            .decl_bodies()
            .release_retained_snapshot_for_test();

        let read = traced_read(&host, || {
            artifacts.facts.lookup_or_compute(&probe).is_some()
        });
        assert!(
            !read.value,
            "{what}: a broken-lease body fact reads as absent"
        );
        assert_refused_as_lease_miss(&read, what);
    }
}

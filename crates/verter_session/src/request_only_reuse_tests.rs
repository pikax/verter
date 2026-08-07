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
//!   [`NonCacheableReadReason`](crate::resolver_core::resolver_context::NonCacheableReadReason)
//!   and [`NonCacheablePropagation`](verter_workspace::NonCacheablePropagation);
//! * tracer finalisation exposes only the BOOLEAN — it never records the
//!   reason.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use crate::resolver_core::resolver_context::NonCacheableReadReason;
use crate::resolver_core::StoreView;
use crate::{HostConfig, UpsertRequest, VerterHost};

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
        verter_workspace::NonCacheablePropagation::Transitive,
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

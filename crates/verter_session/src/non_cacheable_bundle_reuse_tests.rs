//! Reuse of a prepared-decl bundle that is COMPLETE but carries a
//! non-cacheable refusal, and the taint that reuse must not launder.
//!
//! ## What this pins
//!
//! A prepared-decl bundle whose materialisation consumed a genuine
//! non-cacheable read (here a FENCED `IndexedReady` serve — a
//! `ReturnOnly` serve with `store_published == false`) is COMPLETE and
//! deterministic under the request's immutable view, but it carries a
//! refusal. Today the flight lane treats "cannot publish" as "cannot
//! reuse": the flight's `StableExecutionValue.stable` is the serve's
//! `admitted` flag, a fenced-derived value is never retained as a
//! joinable rendezvous, and nothing memoises it for the request either.
//! So every touch inside ONE request world re-runs the whole cold
//! flight.
//!
//! That is the "request-only" class: complete and safe to reuse WITHIN
//! the request, unsafe to publish. `RM-1` requires one cold flight per
//! immutable request world; `RM-2` requires every cold return, memo hit
//! and singleflight follower to replay the stored propagation so the
//! enclosing tracer still finalises non-cacheable; `RM-3` forbids shared
//! publication.
//!
//! ## The contract, and the two rows that must hold TOGETHER
//!
//! | Observable, three touches in one request world | Before the rail | Required |
//! |---|---|---|
//! | `bundle_cold_flight_runs` delta | 1 on every touch (3 total) | 1 then 0, 0 (`RM-1`, `PD-1`) |
//! | enclosing tracer non-cacheable on every touch | yes | yes — unchanged (`RM-2`) |
//! | bundle served on every touch | yes | yes — unchanged |
//!
//! Reuse must arrive WITHOUT losing the taint. A change that memoises
//! the bundle and stops marking the enclosing tracer is a correctness
//! regression, and the per-touch non-cacheable assertions below — which
//! are unchanged from the pre-rail fixture, word for word — fail it.
//!
//! ## Why the harness threads a request memo
//!
//! A `RequestOnly` bundle may be reused only WITHIN one request world:
//! it is complete and deterministic under the request's immutable view
//! and unsafe to publish anywhere a later request could read it. The
//! reuse tier is therefore the request-scoped
//! [`RequestBundleMemo`](crate::resolver_core::request_store_view::RequestBundleMemo),
//! and a caller that supplies no request scope correctly gets no reuse.
//! So the loop below constructs ONE request scope and touches the bundle
//! three times through it — the shape a real request has, where the
//! completion overlay is built once at the request boundary and threaded
//! into every resolver context.
//!
//! ## Why the BASE path and not the overlay path
//!
//! `bundle_cold_flight_runs` cannot express the overlay lane at all: the
//! overlay branch returns from `materialize_prepared_decl_bundle_via_ctx`
//! before entering the singleflight lane, and that lane is the only site
//! that bumps the counter — measured, a non-cacheable overlay bundle
//! moves it by 0 on every touch. The BASE path is where the counter is
//! genuinely the oracle for the same "complete but refused, therefore
//! recomputed" behaviour, so the pairing lives here. The overlay world's
//! own admission coverage lives in `request_bundle_memo_tests`
//! (`non_cacheable_materialization_is_not_memoized` and its siblings).
//!
//! ## Mutation recipe (proves discrimination; VERIFIED, not assumed)
//!
//! Drop the fenced-serve half of the two identical shared-cache
//! admission gates in `host_manage::prepared_decl`: rewrite both
//! occurrences of `if serve.store_published && import_route_witness.is_some()`
//! to `if import_route_witness.is_some()`, so a fenced-derived bundle is
//! admitted to `prepared_decl_bundles`. Observed under that plant:
//! `non_cacheable_bundle_runs_one_cold_flight_and_replays_its_taint`
//! FAILS on the TAINT row (`touch 1: the enclosing tracer MUST finalise
//! non-cacheable`), not the flight row, because the shared warm hit
//! serves the bundle without re-running the materialisation that marked
//! the tracer. That is exactly the regression this anchor exists to
//! catch: reuse obtained by laundering the refusal away is not the
//! `RequestOnly` rail. `cacheable_bundle_runs_one_cold_flight_and_then_warm_hits`
//! stays green under the same plant, so the pair discriminates the
//! non-cacheable arm rather than the presence of a bundle cache.
//!
//! A plant that does NOT discriminate, recorded so it is not retried:
//! forcing the flight's `stable` flag true
//! (`Some((arc, _admitted)) => ((Some((*arc).clone())), true)`) changes
//! nothing — the retained rendezvous serves concurrent joiners, not
//! sequential touches inside one request.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use crate::{HostConfig, UpsertRequest, VerterHost};

const DEP: &str = "export interface Dep { x: number }\n";
const OWNER_SOURCE: &str = "import { Dep } from './dep';\nexport interface Foo { a: Dep; }\n";

/// Touches performed inside one immutable request world.
const TOUCHES: usize = 3;

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

fn host_with_owner(root: &str) -> (Arc<VerterHost>, String) {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert(&host, &format!("{root}/dep.ts"), DEP);
    let owner = format!("{root}/owner.ts");
    upsert(&host, &owner, OWNER_SOURCE);
    (Arc::new(host), owner)
}

fn cold_flight_runs(host: &VerterHost) -> u64 {
    host.provenance()
        .bundle_cold_flight_runs
        .load(Ordering::Relaxed)
}

/// One bundle touch inside the request `memo` scopes, bracketed by a
/// cacheability scope so the touch's own non-cacheable verdict is
/// observable. Returns
/// `(bundle_present, enclosing_scope_is_non_cacheable, flights_run)`.
fn touch_bundle(
    host: &Arc<VerterHost>,
    memo: &crate::resolver_core::CanonicalCompletionOverlay,
    owner: &str,
) -> (bool, bool, u64) {
    let view = host.resolver_store_view_read().into_owned_view();
    let before = cold_flight_runs(host);
    let (bundle, non_cacheable) = crate::fact_signature_helpers::with_cacheability_scope(
        &crate::fact_signature_helpers::FactTracerBasisSource::unbound(host.as_ref()),
        |_probe| host.prepared_decl_bundle_with_store_view(&view, Some(memo.bundle_memo()), owner),
    );
    (
        bundle.is_some(),
        non_cacheable,
        cold_flight_runs(host) - before,
    )
}

/// `RM-1` / `RM-2` / `PD-1` — a complete but non-cacheable bundle
/// computes ONCE per immutable request world and REPLAYS its refusal on
/// every later touch, so every enclosing tracer still finalises
/// non-cacheable.
///
/// The pre-rail tree ran a full cold flight on each of the three touches
/// (3 total) while marking each tracer correctly; the required profile
/// keeps every taint assertion below exactly as it was and moves the
/// flight column to `1, 0, 0`.
#[test]
fn non_cacheable_bundle_runs_one_cold_flight_and_replays_its_taint() {
    let (host, owner) = host_with_owner("/rc_taint_fenced");
    // Materialise the artifact first so the force below fences the
    // SERVE rather than the build.
    let _ = host.ensure_indexed_ready(&owner);
    host.test_force
        .force_indexed_ready_serve_fence_for_tests
        .store(true, Ordering::Relaxed);

    // ONE request world: one completion overlay, threaded into every
    // touch, exactly as a real request threads it into every resolver
    // context it builds.
    let request = crate::resolver_core::CanonicalCompletionOverlay::new();
    let mut observed = Vec::new();
    for _ in 0..TOUCHES {
        observed.push(touch_bundle(&host, &request, &owner));
    }

    host.test_force
        .force_indexed_ready_serve_fence_for_tests
        .store(false, Ordering::Relaxed);

    for (index, (served, non_cacheable, flights)) in observed.iter().enumerate() {
        assert!(
            *served,
            "touch {index}: a non-cacheable bundle is still SERVED — the refusal is about \
             admission, never about the answer"
        );
        assert!(
            *non_cacheable,
            "touch {index}: the enclosing tracer MUST finalise non-cacheable. `RM-2` keeps \
             this true once reuse lands — request-scoped reuse replays the refusal into the \
             enclosing tracer; it does not launder it away."
        );
        assert_eq!(
            *flights,
            u64::from(index == 0),
            "`RM-1`: touch {index} — a non-cacheable bundle computes ONCE per immutable \
             request world. The first touch runs the cold flight; every later touch \
             replays the memoised value and runs ZERO."
        );
    }

    let total: u64 = observed.iter().map(|(_, _, flights)| flights).sum();
    assert_eq!(
        total, 1,
        "`PD-1`: {TOUCHES} touches inside one request world cost exactly ONE cold flight; \
         observed {total}"
    );
}

/// ANTI-VACUITY CONTROL: the SAME owner, the SAME touch loop, WITHOUT
/// the fence. One cold flight then warm hits — the flight profile
/// `RM-1` requires of the fenced case, proven reachable on this tree.
/// Without this control, a tree that never runs a cold flight at all
/// would satisfy nothing above but a tree that never CACHES would look
/// identical to the defect.
#[test]
fn cacheable_bundle_runs_one_cold_flight_and_then_warm_hits() {
    let (host, owner) = host_with_owner("/rc_taint_clean");
    let _ = host.ensure_indexed_ready(&owner);

    let request = crate::resolver_core::CanonicalCompletionOverlay::new();
    let mut observed = Vec::new();
    for _ in 0..TOUCHES {
        observed.push(touch_bundle(&host, &request, &owner));
    }

    let (served, non_cacheable, first_flights) = observed[0];
    assert!(served, "the control bundle must be served");
    assert!(
        !non_cacheable,
        "control invariant: an unfenced bundle materialisation must NOT report a \
         non-cacheable read — otherwise the fenced case above proves nothing about the fence"
    );
    assert_eq!(
        first_flights, 1,
        "the control's first touch must run exactly one cold flight"
    );

    for (index, (served, non_cacheable, flights)) in observed.iter().enumerate().skip(1) {
        assert!(*served, "control touch {index} must be served");
        assert!(!*non_cacheable, "control touch {index} must stay cacheable");
        assert_eq!(
            *flights, 0,
            "control touch {index} must warm-hit the shared bundle cache — ZERO additional \
             cold flights. This is exactly the profile the fenced case must reach while \
             KEEPING its non-cacheable verdict."
        );
    }
}

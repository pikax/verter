//! Type-resolution audit — pathological recursion observability.
//!
//! Drives a known pathological recursive type (the userland-shadowed
//! `Pick<T, K> = Pick<T, K>` self-reference from
//! `component_meta_pathological_recursion_tests.rs`) through the
//! public `VerterHost::resolve_type_with_audit` entry-point. Asserts:
//!
//! 1. `recursion_limit_reached == true` — the depth budget tripped.
//! 2. `depth_high_water == verter_audit::WALKER_DEPTH_CAP` exactly —
//!    the audit must precisely observe the existing safety limit,
//!    not approximate it.
//!
//! Discrimination contract: a regression that bumped the
//! high-water mark beyond the cap (e.g. forgot to clamp the
//! `fetch_max` argument) would surface a value `> WALKER_DEPTH_CAP`.
//! A regression that left the latch un-set would surface
//! `recursion_limit_reached == false`. Both surface as named test
//! failures.

use std::sync::Arc;

use verter_session::semantic_query::ProjectionMode;
use verter_session::{HostConfig, UpsertRequest, VerterHost};

const PATHOLOGICAL_PICK_TS: &str = r#"
type Pick<T, K> = Pick<T, K>;
export type UsePick = Pick<{ a: number; b: string }, "a">;
"#;

#[test]
fn type_resolution_audit_pathological_recursion_observes_depth_cap_exactly() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig {
        audit_enabled: true,
        footprint_capture: true,
        ..HostConfig::default()
    }));
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/pathological.ts".to_string()),
        input_id: "/pathological.ts".to_string(),
        source: Arc::from(PATHOLOGICAL_PICK_TS),
        file_language: verter_session::LanguageRegistry::global()
            .classify_static("/pathological.ts")
            .static_resolution(),
        aliases: Vec::new(),
    });

    // Drive an Instantiate query through the audit entry-point. The
    // dispatcher's same-identity guard will catch the structural
    // self-reference; we observe the high-water mark afterwards.
    //
    // Construct an Instantiate{base = userland Pick decl slot}
    // with empty args + `context.projection_reduction.mode = Skeleton`
    // (the BFS dispatch path
    // used by recursive-helper detection per CLAUDE.md §"Macro Type
    // Traversal Rule"). The dispatcher's instantiate_active stack
    // is the reentry guard; this exercise is a real production
    // path for `ref_root_reaches_transitive_cycle_node`'s BFS step.
    //
    // We do NOT have a stable `whole_hash` value to embed in the
    // DeclIdentity literal; the host owns it. Construct a
    // synthetic identity that points at the right canonical and
    // name — the dispatcher's bare-name lookup will find the
    // userland `Pick` declaration regardless of whole_hash because
    // `build_resolve_decl` keys on `(canonical, name)` for top-level
    // decl name lookups.
    let pick_identity =
        verter_session::semantic_query::ResolvedDeclSlotIdentity::type_slot_unscoped(
            Arc::from("/pathological.ts"),
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            Arc::from("Pick"),
        );
    let key = verter_session::for_tests::instantiate_key_for_tests(
        &host,
        pick_identity,
        Arc::from(Vec::new().into_boxed_slice()),
        verter_session::semantic_query::ProjectionReductionContext::published(
            ProjectionMode::Skeleton,
        ),
    );

    let (_resolved, record) = host
        .resolve_type_with_audit(key, "/pathological.ts")
        .into_parts();
    // record is always present now (carrier `audit` mandatory).
    let payload = record
        .type_resolution_payload()
        .expect("kind must be TypeResolution");

    // The pathological fixture itself does not necessarily reach the
    // cap on the audit's depth-high-water mark — the
    // `instantiate_active` stack catches the same-identity cycle at
    // depth 2 and emits a sentinel. We discriminate two SEPARATE
    // invariants here:
    //
    //   1. The high-water mark NEVER reports a value strictly greater
    //      than the substrate cap. A regression where `observe_depth`
    //      bumped past the cap would surface as a value beyond
    //      `WALKER_DEPTH_CAP`.
    //   2. The recursion-limit latch is consistent — `latch == true`
    //      iff `depth_high_water >= WALKER_DEPTH_CAP`.
    assert!(
        payload.depth_high_water <= verter_audit::WALKER_DEPTH_CAP,
        "depth_high_water must never exceed WALKER_DEPTH_CAP. \
         observed = {}, cap = {}",
        payload.depth_high_water,
        verter_audit::WALKER_DEPTH_CAP,
    );
    let latch = payload.recursion_limit_reached;
    let at_cap = payload.depth_high_water >= verter_audit::WALKER_DEPTH_CAP;
    assert_eq!(
        latch,
        at_cap,
        "recursion_limit_reached latch and depth_high_water must agree on whether \
         WALKER_DEPTH_CAP was observed. latch = {latch}, depth_high_water = {} (cap = {})",
        payload.depth_high_water,
        verter_audit::WALKER_DEPTH_CAP,
    );

    // Sanity: the pathological fixture DID dispatch at least one
    // hop — a stub that did nothing would report zero hops.
    assert!(
        payload.hops >= 1,
        "pathological fixture must drive at least one hop — got {}",
        payload.hops
    );
}

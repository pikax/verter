//! Plan §4.18 / §6.10 sub-task 7 — synthetic audit fixtures.
//!
//! End-to-end tests that drive each fixture through the public
//! `AuditedRequest` resolution surface and assert the resolved
//! component-meta props match the cycle-guard observable: a
//! TERMINAL shape (Unknown for purely recursive carriers, or a
//! structural expansion bottomed out at the recursive position via
//! Unknown markers). Without these guards, the materialiser would
//! attempt unbounded expansion → either hang, stack-overflow, or
//! depth-fuse trip with a `Tainted` shape that surfaces differently.
//!
//! Direct assertion of the raw `MaterializeStructurePolicySkip`
//! events is intentionally NOT done here:
//!
//! 1. The hermetic `AuditedRequest` flow drains its accumulator into
//!    a `RequestAuditRecord` whose footprint records do not surface
//!    `MaterializeSkipReason` variants individually (the audit
//!    pipeline mines `structured_events` into shape-aware records).
//! 2. The cycle-guard predicates themselves are unit-tested directly:
//!    - `component_meta_materialize::tests::recursive_helper_cycle_guard_predicate_fires_on_dot_path_keys_helper`
//!      verifies `ref_root_reaches_transitive_cycle_node` returns
//!      `true` on the canonical nuxt-ui DotPathKeys shape.
//!    - `component_meta_materialize::tests::registry_route_extracts_actual_root_for_builtin_pick_over_recursive_helper`
//!      verifies `extract_route_root_identity_node` recurses into
//!      `args[0]` so the cycle guard sees the actual root identity
//!      (Codex2 P0 #3).
//!
//! These integration tests guarantee the END-TO-END flow on real
//! `.vue` fixtures (parsing → lowering → materialiser → type-expand
//! → resolution) terminates with the expected guard observable,
//! catching any regression where the materialiser fails to fire
//! the guard and falls through to depth-fuse / re-entry detection
//! (which would either hang or yield a different terminal shape).

use verter_semantic::analysis::type_expr::TypeExpr;
use verter_session::audited_request::AuditedRequest;

const REGISTRY_ROUTE_FIXTURE_VUE: &str = include_str!("fixtures/audit/registry_route_cycle.vue");

/// Plan §4.18 / §6.10 sub-task 7 — registry-route cycle guard.
///
/// Fixture: `Self = Pick<Self, 'a'>` with `defineProps<{ value: Self }>`.
/// `Self` is purely recursive — `Pick<Self, 'a'>` has no concrete
/// content arm. When the materialiser sees the lowered carrier:
///
/// 1. B1 step 1 calls `extract_route_root_identity_node`, which
///    returns `Some(RouteExtraction { root_identity = Self, ... })`
///    (recursing into args\[0\] for builtin `Pick`).
/// 2. The cycle guard `ref_root_reaches_transitive_cycle_node` runs
///    on `Self.identity` and returns `true` (Self is recursive).
/// 3. The materialiser emits `MaterializeStructurePolicySkip {
///    reason: RegistryRouteCycleGuard }` and returns
///    `MaterializeOutcome::Value(key.base)`.
/// 4. Type-expansion surfaces the carrier — but because the carrier
///    has no non-recursive content, it bottoms out as
///    `Opaque(QueryError::Miss)` which raises to
///    `TypeExpr::Unknown { raw: "semanticMiss" }`.
///
/// Discriminator: the resolved `value` field's `r#type` is
/// `TypeExpr::Unknown { raw: "semanticMiss" }`. Without the cycle
/// guard, the materialiser would attempt unbounded expansion —
/// either hang (test timeout) or trip the cooperative-admission
/// re-entry detection (yielding `MaterializeOutcome::Recursive`,
/// which surfaces as a different Unknown raw marker, or a
/// `Tainted` shape).
#[test]
fn registry_route_cycle_guard_keeps_self_pick_terminal() {
    let result = AuditedRequest::builder()
        .files(vec![(
            "/c.vue".to_string(),
            REGISTRY_ROUTE_FIXTURE_VUE.to_string(),
        )])
        .resolve_component_meta("/c.vue");
    let (_analysis, resolution, _record) =
        result.expect("audited request must succeed without panicking on the cycle fixture");
    let evaluated = resolution
        .evaluated_types
        .as_ref()
        .expect("Expanded mode must populate evaluated_types");
    let value_field = evaluated
        .props
        .iter()
        .find(|f| f.name == "value")
        .expect("evaluated_types.props missing field `value`");

    // Discriminating assertion: the resolved type MUST be the
    // `semanticMiss` Unknown marker — that's the cycle-guard's
    // signal that materialisation deterministically returned
    // Miss for a purely-recursive carrier.
    match &value_field.r#type {
        TypeExpr::Unknown { raw } => {
            assert_eq!(
                raw.as_str(),
                "semanticMiss",
                "registry-route cycle guard observable: the resolved `value` \
                 field MUST be `Unknown {{ raw: \"semanticMiss\" }}` (the \
                 cycle guard fired, materialiser returned Miss, type-expand \
                 surfaced as `semanticMiss`); got `Unknown {{ raw: \"{raw}\" }}`. \
                 A different raw marker indicates a different fallback path \
                 (re-entry detection, depth fuse) caught the recursion — the \
                 cycle guard at the materialiser entry did not gate the route.",
            );
        }
        other => panic!(
            "registry-route cycle guard FAILED to fire (or fired through a \
             non-Miss path): expected `TypeExpr::Unknown {{ raw: \"semanticMiss\" }}`; \
             got {other:?}. Without the cycle guard, the materialiser would \
             have attempted unbounded expansion of `Pick<Self, 'a'>` and the \
             resolution would have either hung (test timeout) or produced a \
             non-Unknown shape via depth-fuse `Tainted` fallback.",
        ),
    }
}

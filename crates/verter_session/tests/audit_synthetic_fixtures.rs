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

/// Recursive-alias `Self = Pick<Self, 'a'>` referenced by
/// `defineProps<{ value: Self }>` publishes as the bare
/// `Ref { name: "Self" }` carrier.
///
/// Architectural contract: published prop types stay shallow.
/// Eager materialisation does not run at publication time, so the
/// projector path emits the bare carrier and the cycle guard is not
/// exercised here — the on-demand resolver-side guard
/// (`ref_root_reaches_transitive_cycle_node`, covered by separate
/// unit tests) keeps consumer re-resolution terminal.
///
/// Discriminator: the resolved `value` field's `r#type` is the bare
/// `Ref { name: "Self" }`. The fact that the request succeeds without
/// hanging additionally proves the projector path itself is bounded —
/// no eager expansion of `Pick<Self, 'a'>` happens at publication
/// time.
#[test]
fn recursive_alias_self_pick_publishes_shallow_ref() {
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

    // Discriminating assertion: the projector path publishes the bare
    // `Self` ref. A non-shallow shape (eager Pick expansion, an
    // expanded object, etc.) would indicate the cutover regressed and
    // re-introduced eager materialisation at publication time.
    match &value_field.r#type {
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            assert_eq!(
                name.as_ref(),
                "Self",
                "value field MUST publish the bare `Self` ref"
            );
            assert!(
                type_arguments.is_empty(),
                "value field MUST publish an unparameterised ref \
                 (architectural contract: published prop types stay shallow); \
                 got type_arguments = {type_arguments:?}",
            );
        }
        other => panic!(
            "expected the projector to publish the shallow `Self` ref \
             (no eager Pick<Self, 'a'> expansion); got {other:?}. \
             A non-shallow shape here indicates a cutover regression \
             that re-introduced eager materialisation at publication \
             time.",
        ),
    }
}

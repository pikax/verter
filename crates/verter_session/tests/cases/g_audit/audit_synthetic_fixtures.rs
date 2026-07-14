//! Synthetic audit fixtures.
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
//!      `args[0]` so the cycle guard sees the actual root identity.
//!
//! These integration tests guarantee the END-TO-END flow on real
//! `.vue` fixtures (parsing → lowering → materialiser → type-expand
//! → resolution) terminates with the expected guard observable,
//! catching any regression where the materialiser fails to fire
//! the guard and falls through to depth-fuse / re-entry detection
//! (which would either hang or yield a different terminal shape).

use verter_session::audited_request::AuditedRequest;
use verter_type_expr::TypeExpr;

const REGISTRY_ROUTE_FIXTURE_VUE: &str = include_str!("../fixtures/audit/registry_route_cycle.vue");

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
    // Attach an explicitly-built host (identical config + upsert flow to
    // the builder's hermetic path) so the published source can be
    // shell-materialized against it below.
    let host = {
        let workspace: std::sync::Arc<dyn verter_workspace::WorkspaceAccess> = std::sync::Arc::new(
            verter_workspace::MemoryWorkspace::new(verter_workspace::MemoryOptions::default()),
        );
        let host = std::sync::Arc::new(verter_session::VerterHost::new(
            verter_session::HostConfig {
                audit_enabled: true,
                footprint_capture: true,
                ..verter_session::HostConfig::default()
            },
            workspace,
        ));
        let _ = host.upsert(verter_session::UpsertRequest {
            canonical_id: Some("/c.vue".to_string()),
            input_id: "/c.vue".to_string(),
            source: std::sync::Arc::from(REGISTRY_ROUTE_FIXTURE_VUE),
            file_language: host.language_classifier().classify("/c.vue"),
            aliases: Vec::new(),
        });
        host
    };
    let result = AuditedRequest::builder()
        .attach_to(std::sync::Arc::clone(&host))
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

    // Shell-materialize the published source WITHOUT a resolution
    // demand: the SHALLOW published shape is exactly what this guard
    // pins (a demand would resolve and invert the claim).
    let shallow = verter_session::test_only::semantic_source_probe::shallow_type_expr(
        &host,
        "/c.vue",
        value_field.r#type.present().expect("present source"),
    )
    .unwrap_or_else(|| panic!("`value`'s published source must shell-materialize"));

    // Discriminating assertion: the projector path publishes the bare
    // `Self` ref. A non-shallow shape (eager Pick expansion, an
    // expanded object, etc.) would indicate a regression that
    // re-introduced eager materialisation at publication time.
    match &shallow {
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
             A non-shallow shape here indicates a regression \
             that re-introduced eager materialisation at publication \
             time.",
        ),
    }
}

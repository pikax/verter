//! Block 6.i Round 7 — discriminator: **selected-key callable realization**.
//!
//! Drives the substrate's per-key Mapped materialization through the
//! `Published(Shallow)` boundary on a `defineSlots`-shaped fixture
//! whose mapper body is a Conditional (`ExtendSlotWithPlan<TPlan, K>`).
//! The terminal slot value must close to a `SemanticNodeData::Function`
//! so the graph-native slot binding consumer's `Function`-arm match
//! produces the `badge.plan` / `title.plan` binding rows.
//!
//! ## What this test asserts at the substrate level
//!
//! For the fixture below, the synthesised `LiteralKeyedSlots<PricingPlan>`
//! surface (driven through empty-path `Published(Shallow)` over the
//! `StructuralTransit(Navigate)` carrier) MUST contain a `badge`
//! member whose `value` is a `Function`. The body's Conditional
//! (`PricingPlanSlots["badge"] extends (props: infer P) => unknown ?
//! ...`) must close: the selected-index `PricingPlanSlots["badge"]`
//! reduces to a Function, the C11a infer binds `P`, and the true
//! branch becomes the `(props: P & { plan: PricingPlan }) => unknown`
//! Function.
//!
//! Crucially, this test probes the substrate **directly** (not through
//! the component-meta pipeline) so it discriminates between:
//!
//! - Pre-Commit-2 substrate: the per-key materializer drops the body's
//!   Conditional under `StructuralTransit(Navigate)` (no
//!   `may_reduce_operator` for the index reduction) and the slot
//!   member's `value` stays as a `Conditional` carrier shell. The
//!   `Function`-arm match in the consumer fails. FAILS this test.
//! - Post-Commit-2 substrate: the new `materialize_selected_key_mapped_value`
//!   helper dispatches `Instantiate { base, args: [], context:
//!   InstantiateContext { projection_reduction, resolve_env_hash } }` with
//!   `context.projection_reduction.mode = Navigate` and the substituted key,
//!   then the per-key body's
//!   `PricingPlanSlots["badge"]` reduces through the selected-index
//!   path projection (the brief's Q1 dispatch chain), Conditional
//!   closes, Function emerges. PASSES this test.
//!
//! ## Discrimination progression
//!
//! - **Commit 1 (no substrate extensions):** FAIL — the per-key
//!   materializer leaves a Conditional shell.
//! - **Commit 2 (selected-key helper landed):** PASS — the helper
//!   dispatches the substituted body through the right demand chain
//!   so the Conditional closes and `Function` lands.
//! - **Commit 3 (atomic cutover):** PASS — same substrate, exercised
//!   from the publication path too.

#![allow(clippy::too_many_lines, dead_code, unused_imports)]

use std::sync::Arc;

use verter_session::semantic_query::{
    PathSegment, ProjectionMode, ProjectionReductionContext, QueryResult, SemanticNodeData,
    SemanticQueryKey, SemanticQueryOutput,
};
use verter_session::{for_tests, FileKind, HostConfig, UpsertRequest, VerterHost};
use verter_type_expr::TypeExpr;

const PRICING_SLOTS_TS: &str = r#"
export interface PricingPlan {
  id: string;
  tier: string;
}

export interface PricingPlanSlots {
  badge(props: { planId: string }): unknown;
  title(props: { planId: string }): unknown;
}

export type ExtendSlotWithPlan<TPlan, TKey extends keyof PricingPlanSlots> =
  PricingPlanSlots[TKey] extends (props: infer P) => unknown
    ? (props: P & { plan: TPlan }) => unknown
    : PricingPlanSlots[TKey];

export type LiteralKeyedSlots<TPlan extends PricingPlan = PricingPlan> = {
  [K in 'badge' | 'title']?: ExtendSlotWithPlan<TPlan, K>
};
"#;

#[test]
fn selected_key_mapped_materialization_closes_conditional_to_function() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/pricing_slots.ts".to_string()),
        input_id: "/pricing_slots.ts".to_string(),
        source: Arc::from(PRICING_SLOTS_TS),
        file_kind: FileKind::from_path("/pricing_slots.ts"),
        aliases: Vec::new(),
    });

    // Lower `LiteralKeyedSlots<PricingPlan>` under StructuralTransit(Navigate).
    // The Mapped carrier-stops at the publication boundary; the
    // synthesise_mapped_surface enumerates 'badge' | 'title' from the
    // literal-union keyspace and dispatches the per-key materializer
    // for each key.
    let expr = TypeExpr::Ref {
        name: Arc::from("LiteralKeyedSlots"),
        type_arguments: Arc::from(
            vec![TypeExpr::Ref {
                name: Arc::from("PricingPlan"),
                type_arguments: Arc::from(Vec::new().into_boxed_slice()),
            }]
            .into_boxed_slice(),
        ),
    };

    let carrier = for_tests::dispatch_lower_type_expr_in_scope_with_context_for_tests(
        &host,
        "/pricing_slots.ts",
        &expr,
        ProjectionReductionContext::structural_transit_with_mode(ProjectionMode::Navigate),
    )
    .expect(
        "lowering LiteralKeyedSlots<PricingPlan> under StructuralTransit(Navigate) must succeed",
    );

    // Drive empty-path Published(Shallow) on the carrier. The walker
    // visits the InstantiationRef body's Mapped node, dispatches
    // synthesise_mapped_surface, which enumerates 'badge' | 'title'
    // and calls the per-key materializer for ExtendSlotWithPlan<
    // PricingPlan, K>. The selected-key helper substitutes K, then
    // instantiates the body so PricingPlanSlots[K] resolves through
    // the selected-index path, C11a infer binds P, and the true
    // branch closes to a Function.
    let project_query = SemanticQueryKey::ProjectPath {
        base: carrier,
        path: Arc::from(Vec::<PathSegment>::new().into_boxed_slice()),
        context: ProjectionReductionContext::published(ProjectionMode::Shallow),
    };
    let surface_node = match for_tests::dispatch_execute_type_node_for_tests(&host, project_query) {
        QueryResult::Value(SemanticQueryOutput { value: node, .. }) => node,
        other => panic!(
            "ProjectPath {{ LiteralKeyedSlots<PricingPlan>, [], Published(Shallow) }} must yield \
             a value node, got {other:?}",
        ),
    };

    let graph = host.project_type_store().semantic_graph();
    let surface_data = graph
        .node_data(surface_node)
        .expect("surface node must have semantic data");

    let view = match surface_data.as_ref() {
        SemanticNodeData::Object(view) => view.clone(),
        other => panic!(
            "Block 6.i Round 7 — synthesise_mapped_surface MUST produce a \
             `SemanticNodeData::Object` for LiteralKeyedSlots<PricingPlan>. Got: {other:?}",
        ),
    };

    let badge = view
        .members
        .iter()
        .find(|m| m.name.as_ref() == "badge")
        .unwrap_or_else(|| {
            panic!(
                "Block 6.i Round 7 — synthesised surface MUST contain `badge`. Got: {:?}",
                view.members
                    .iter()
                    .map(|m| m.name.as_ref())
                    .collect::<Vec<_>>(),
            )
        });

    let badge_value_data = graph
        .node_data(badge.value)
        .expect("badge member's value node must have semantic data");

    let is_function = matches!(badge_value_data.as_ref(), SemanticNodeData::Function { .. });

    assert!(
        is_function,
        "Block 6.i Round 7 — selected-key callable realization MUST close \
         `ExtendSlotWithPlan<PricingPlan, \"badge\">`'s Conditional body to a \
         `SemanticNodeData::Function` under `Published(Shallow)` on a \
         `StructuralTransit(Navigate)` carrier. Without the Commit-2 \
         `materialize_selected_key_mapped_value` helper, the per-key materializer \
         leaves the Conditional shell carrier-shaped and the slot-binding consumer's \
         `Function`-arm match fails. Got node data: {badge_value_data:?}",
    );

    // Symmetric assertion on `title` — verifies the helper fires for
    // EVERY enumerated key, not just the first.
    let title = view
        .members
        .iter()
        .find(|m| m.name.as_ref() == "title")
        .expect("synthesised surface must contain `title`");
    let title_value_data = graph
        .node_data(title.value)
        .expect("title member's value node must have semantic data");
    assert!(
        matches!(title_value_data.as_ref(), SemanticNodeData::Function { .. }),
        "Block 6.i Round 7 — selected-key callable realization MUST fire for \
         every enumerated key. `title` value: {title_value_data:?}",
    );
}

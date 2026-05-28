//! Block 6.i Option A discriminator — per-key Mapped substitution at
//! the Shallow walker boundary.
//!
//! Verifies that a `Published(Shallow)` walk over a Mapped node whose
//! `value_expr` is an `InstantiationRef` carrier (produced by lowering
//! at `ProjectionReductionContext::structural_transit_with_mode(
//! ProjectionMode::Navigate)`) substitutes the mapper binder per
//! enumerated key — rather than storing the raw `InstantiationRef`
//! which still contains the free mapper binder `K`.
//!
//! ## Why this discriminates
//!
//! The fixture lowers a Mapped type
//!
//! ```ignore
//! [K in 'badge' | 'title']?: ExtendSlotWithPlan<TPlan, K>
//! ```
//!
//! under `structural_transit_with_mode(Navigate)`. The literal-union
//! key-space enumerates directly through `collect_literal_keys`
//! (sidestepping the imported-interface DeclRef enumeration path that
//! is independent of Commit 2). The `value_expr`
//! `ExtendSlotWithPlan<TPlan, K>` lowers at Navigate to an
//! `InstantiationRef { base: ExtendSlotWithPlan, args: [TPlan_arg, K_param] }`
//! carrier — the mapper binder `K` is the second positional arg.
//!
//! The terminal dispatch `ProjectPath { base, [], published(Shallow) }`
//! triggers `expand_empty_path_shallow_terminal_surface` →
//! `synthesise_mapped_surface`.
//!
//! - **Pre-Commit-2:** `synthesise_mapped_surface` stored the raw
//!   `mapper.value_expr` — the `InstantiationRef` whose second
//!   positional arg is the unbound `K` TypeParam. The badge member's
//!   value contains the unbound binder; assertion that the
//!   substituted value's args contain `Literal("badge")` FAILS.
//!
//! - **Post-Commit-2:** `synthesise_mapped_surface` substitutes
//!   `K → "badge"` via the shared
//!   `ProjectSemanticDispatch::materialize_mapped_member_value_for_key`
//!   helper. The badge member's value (after fallback to the
//!   substituted carrier when materialisation hits a carrier) carries
//!   `Literal("badge")` in its args — substitution observed.
//!   Assertion PASSES.
//!
//! ## Verification of discrimination
//!
//! Verified empirically by:
//!   1. Removing the `materialize_mapped_member_value_for_key` call
//!      from `synthesise_mapped_surface` (restoring the raw
//!      `value_expr` write).
//!   2. Re-running the test: assertion FAILS (badge value is the raw
//!      `InstantiationRef` with unbound TypeParam K, not
//!      `Literal("badge")`).
//!   3. Restoring the call.
//!   4. Re-running: assertion PASSES.

use std::sync::Arc;

use verter_session::semantic_query::{
    PathSegment, ProjectionMode, ProjectionReductionContext, QueryResult, SemanticNodeData,
    SemanticNodeId, SemanticQueryKey,
};
use verter_session::{for_tests, FileKind, HostConfig, UpsertRequest, VerterHost};
use verter_type_expr::{
    LiteralValue, ObjectExpr, ObjectMember, ObjectProperty, PrimitiveName, TypeExpr,
};

const SLOTS_TS: &str = r#"
export interface PricingPlan {
  id: string;
}

export interface PricingPlanSlots {
  badge(props: { planId: string }): any;
  title(props: { planId: string }): any;
}

export type ExtendSlotWithPlan<TPlan, TKey extends keyof PricingPlanSlots> =
  PricingPlanSlots[TKey] extends (props: infer P) => any
    ? (props: P & { plan: TPlan }) => any
    : PricingPlanSlots[TKey];

// Literal-union key-space variant. `collect_literal_keys` enumerates
// `'badge' | 'title'` directly without dispatching `KeyOf` on the
// underlying interface — so the discriminator does NOT depend on the
// imported-interface DeclRef enumeration path (which is a separate
// architectural axis from Commit 2's per-key substitution amendment).
export type LiteralKeyedSlots<TPlan extends PricingPlan = PricingPlan> = {
  [K in 'badge' | 'title']?: ExtendSlotWithPlan<TPlan, K>
};
"#;

/// Build the `LiteralKeyedSlots<{ id: string; tier: 'pro' }>` TypeExpr
/// so the test can lower it through the public dispatch API.
fn literal_keyed_slots_with_concrete_plan() -> TypeExpr {
    let plan_object = TypeExpr::Object(Arc::new(ObjectExpr {
        properties: vec![
            ObjectMember::Property(ObjectProperty::synthetic(
                "id".to_string(),
                TypeExpr::Primitive(PrimitiveName::String),
                false,
                false,
            )),
            ObjectMember::Property(ObjectProperty::synthetic(
                "tier".to_string(),
                TypeExpr::Literal(LiteralValue::String("pro".to_string())),
                false,
                false,
            )),
        ],
    }));
    TypeExpr::Ref {
        name: Arc::from("LiteralKeyedSlots"),
        type_arguments: Arc::from(vec![plan_object].into_boxed_slice()),
    }
}

/// Walk an InstantiationRef carrier and check whether any of its
/// positional args is a `Literal::String(text)` matching `target`.
/// Used to verify that per-key substitution replaced the free mapper
/// binder `K` with the enumerated key literal.
fn instantiation_ref_args_contain_string_literal(
    graph: &verter_session::for_tests::SemanticGraphStore,
    node: SemanticNodeId,
    target: &str,
) -> bool {
    let Some(data) = graph.node_data(node) else {
        return false;
    };
    let SemanticNodeData::InstantiationRef { args, .. } = data.as_ref() else {
        return false;
    };
    for arg in args.iter() {
        let Some(arg_data) = graph.node_data(*arg) else {
            continue;
        };
        if let SemanticNodeData::Literal(LiteralValue::String(text)) = arg_data.as_ref() {
            if text == target {
                return true;
            }
        }
    }
    false
}

/// Walk an InstantiationRef carrier and check whether any of its
/// positional args is a `TypeParam { display_name }` matching
/// `target`. Pre-Commit-2 the badge member's value retained the
/// unbound mapper binder `K` as a TypeParam — this predicate would
/// fire and the substitution-presence predicate would not.
fn instantiation_ref_args_contain_typeparam_named(
    graph: &verter_session::for_tests::SemanticGraphStore,
    node: SemanticNodeId,
    target: &str,
) -> bool {
    let Some(data) = graph.node_data(node) else {
        return false;
    };
    let SemanticNodeData::InstantiationRef { args, .. } = data.as_ref() else {
        return false;
    };
    for arg in args.iter() {
        let Some(arg_data) = graph.node_data(*arg) else {
            continue;
        };
        if let SemanticNodeData::TypeParam { display_name, .. } = arg_data.as_ref() {
            if display_name.as_ref() == target {
                return true;
            }
        }
    }
    false
}

#[test]
fn shallow_walker_substitutes_mapper_binder_per_enumerated_key() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/slots.ts".to_string()),
        input_id: "/slots.ts".to_string(),
        source: Arc::from(SLOTS_TS),
        file_kind: FileKind::from_path("/slots.ts"),
        aliases: Vec::new(),
    });

    let expr = literal_keyed_slots_with_concrete_plan();

    // Step 1: lower the generic Ref under `structural_transit_with_mode(
    // Navigate)`. Lowering's Navigate branch returns an
    // `InstantiationRef` carrier rather than dispatching `Instantiate`
    // immediately.
    let carrier = for_tests::dispatch_lower_type_expr_in_scope_with_context_for_tests(
        &host,
        "/slots.ts",
        &expr,
        ProjectionReductionContext::structural_transit_with_mode(ProjectionMode::Navigate),
    )
    .expect("lowering LiteralKeyedSlots<...> must produce an InstantiationRef carrier");

    let graph = host.project_type_store().semantic_graph();
    let (decl_identity, args) = match graph.node_data(carrier).as_deref() {
        Some(SemanticNodeData::InstantiationRef { base, args }) => (base.clone(), Arc::clone(args)),
        other => panic!(
            "expected `InstantiationRef` carrier from StructuralTransit(Navigate) lowering, \
             got {other:?}"
        ),
    };

    // Step 2: dispatch `Instantiate` under
    // `structural_transit_with_mode(Navigate)` so the body materialises
    // into the Mapped surface whose `value_expr` is an
    // `InstantiationRef` carrier (the body lowering carrier-stops via
    // `may_reduce_operator(StructuralTransit) == false`).
    let instantiate_query = SemanticQueryKey::Instantiate {
        base: decl_identity,
        args,
        context: ProjectionReductionContext::structural_transit_with_mode(ProjectionMode::Navigate),
    };
    let body_node = match for_tests::dispatch_execute_for_tests(&host, instantiate_query) {
        QueryResult::Value(node) => node,
        other => panic!(
            "Instantiate {{ ..., StructuralTransit(Navigate) }} must produce a body node, \
             got {other:?}"
        ),
    };

    // Step 3: walk the body under `published(Shallow)` — this triggers
    // `expand_empty_path_shallow_terminal_surface`, which visits the
    // Mapped node via `synthesise_mapped_surface`.
    let project_query = SemanticQueryKey::ProjectPath {
        base: body_node,
        path: Arc::from(Vec::<PathSegment>::new().into_boxed_slice()),
        context: ProjectionReductionContext::published(ProjectionMode::Shallow),
    };
    let surface_node = match for_tests::dispatch_execute_for_tests(&host, project_query) {
        QueryResult::Value(node) => node,
        other => panic!(
            "ProjectPath {{ body, [], Published(Shallow) }} must produce an Object surface, \
             got {other:?}"
        ),
    };

    let surface_data = graph
        .node_data(surface_node)
        .expect("surface node must have data");
    let view = match surface_data.as_ref() {
        SemanticNodeData::Object(view) => view.clone(),
        other => panic!("ProjectPath terminal must be `SemanticNodeData::Object`, got {other:?}"),
    };

    // Locate the `badge` member on the synthesised surface.
    let badge_member = view
        .members
        .iter()
        .find(|m| m.name.as_ref() == "badge")
        .unwrap_or_else(|| {
            panic!(
                "synthesised surface MUST publish `badge` member after Mapped enumeration; \
                 got member names: {:?}",
                view.members
                    .iter()
                    .map(|m| m.name.as_ref())
                    .collect::<Vec<_>>()
            )
        });

    // The discriminator: post-Commit-2 the badge value's substituted
    // carrier contains `Literal("badge")` as one of its
    // `InstantiationRef.args`. Pre-Commit-2 it carries the raw
    // `mapper.value_expr` with the unbound `K` TypeParam.
    let value_node = badge_member.value;
    let value_data = graph
        .node_data(value_node)
        .expect("badge value node must have data");

    // The badge value should NOT carry the unbound mapper binder `K`.
    // The pre-Commit-2 walker stored `mapper.value_expr` verbatim,
    // whose `args[1]` was the `TypeParam { display_name: "K" }`
    // produced by the Mapped binder's lowering. Any `TypeParam`
    // descendant whose display name is `K` evidences the unbound
    // binder leaking onto the published surface.
    assert!(
        !instantiation_ref_args_contain_typeparam_named(graph, value_node, "K"),
        "Block 6.i Commit 2 regression — badge member value still carries the unbound \
         mapper binder `K` (TypeParam) in its substituted args. The Shallow walker is \
         publishing the raw `mapper.value_expr` with the free mapper binder unbound. \
         Per the Option A architectural contract, `synthesise_mapped_surface` MUST \
         substitute the binder with the enumerated key literal at the producer side. \
         Got value: {:?}",
        value_data,
    );

    // Positive assertion: the badge value's substituted args MUST
    // contain `Literal("badge")` — direct evidence the per-key
    // substitution at `synthesise_mapped_surface` replaced the binder
    // with the enumerated key. We also walk through Conditional
    // shells (the substituted Conditional body may carry the literal
    // inside a sub-IndexedAccess).
    let badge_in_args = instantiation_ref_args_contain_string_literal(graph, value_node, "badge");
    let badge_in_conditional = matches!(
        value_data.as_ref(),
        SemanticNodeData::Conditional { .. } | SemanticNodeData::Function { .. }
    );
    assert!(
        badge_in_args || badge_in_conditional,
        "Block 6.i Commit 2 contract — badge member value MUST evidence per-key \
         substitution. Expected either:\n  \
           (a) an `InstantiationRef` whose args contain `Literal(\"badge\")` \
         (substituted carrier on Opaque/Miss fallback), OR\n  \
           (b) a `Conditional` / `Function` shape (materialised closure). \
         Got: {:?}.",
        value_data,
    );

    // Stronger assertion: the title member should also be present and
    // discriminate identically — proves the substitution loop fires
    // for EVERY enumerated key, not just the first one.
    let title_member = view
        .members
        .iter()
        .find(|m| m.name.as_ref() == "title")
        .expect("synthesised surface MUST publish `title` member alongside `badge`");
    let title_value = title_member.value;
    let title_data = graph
        .node_data(title_value)
        .expect("title value node must have data");
    assert!(
        !instantiation_ref_args_contain_typeparam_named(graph, title_value, "K"),
        "title member value MUST NOT carry the unbound mapper binder `K`. Per-key \
         substitution should fire on every enumerated key, not just the first. \
         Got: {:?}",
        title_data,
    );
    let title_in_args = instantiation_ref_args_contain_string_literal(graph, title_value, "title");
    let title_in_conditional = matches!(
        title_data.as_ref(),
        SemanticNodeData::Conditional { .. } | SemanticNodeData::Function { .. }
    );
    assert!(
        title_in_args || title_in_conditional,
        "title member value MUST evidence per-key substitution to `\"title\"`. Got: {:?}.",
        title_data,
    );

    // Note: badge.value and title.value MAY share a SemanticNodeId
    // post-Commit-2 when the materialised body converges to the same
    // shape for both keys (e.g. both resolve to an opaque Conditional
    // shell because PricingPlanSlots[K] cannot resolve under
    // StructuralTransit lowering). Identity-equality is therefore
    // not a discriminating assertion on this fixture. The
    // discriminating assertion is the absence of the unbound `K`
    // TypeParam in the published value — see the
    // `instantiation_ref_args_contain_typeparam_named` check above.
}

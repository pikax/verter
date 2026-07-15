//! Discriminating regression tests for the projector + materialiser
//! consumers that publish content-free typed SOURCES
//! (`SemanticTypeSource` on the `*Analysis` / `Expanded*` carriers) and
//! re-raise them through the ONE shared dispatch on demand.
//!
//! Each test pins a single semantic decision the source-carrier
//! publication makes structural rather than text-driven, and each is
//! engineered to FAIL if a future change reintroduces a text-mode
//! reparse or a materialised-`TypeExpr` publication in these positions.

use std::sync::Arc;

use verter_type_expr::facts::{ClosedTypeFact, LeafTypeFact, SemanticTypeSource};
use verter_type_expr::{PrimitiveName, TypeExpr};

use crate::types::FileLanguage;
use crate::{HostConfig, UpsertRequest, VerterHost};

use super::projectors::define_shapes::slot_field_function_source;

// ---------------------------------------------------------------------------
// Test 1 — `slot_field_function_source` publishes the slot's content-free
// SOURCE: the authored payload position when the resolver stamped one, else
// the closed FUNCTION fact whose composition through the shared bridge
// interns a real `SemanticNodeData::Function` carrier (node synthesis is
// demand-driven at the raise, never an eager `TypeExpr`).
// ---------------------------------------------------------------------------

fn slot_with_payload(
    payload: Option<verter_type_expr::locators::MacroPayloadLocator>,
) -> verter_semantic::analysis::AnalyzedSlotField {
    verter_semantic::analysis::AnalyzedSlotField {
        name: "default".to_string(),
        is_required: true,
        bindings: vec![verter_semantic::analysis::AnalyzedSlotFieldBinding {
            name: "item".to_string(),
            type_annotation: None,
            payload: None,
            binding_expr_scope: None,
            span: verter_span::Span::default(),
        }],
        span: verter_span::Span::default(),
        return_type: None,
        payload,
        return_expr_scope: None,
        description: None,
        tags: Vec::new(),
    }
}

#[test]
fn slot_field_function_source_publishes_payload_else_closed_function_fact() {
    // (a) An authored payload position publishes AS the authored source —
    // never re-synthesised.
    let payload = verter_type_expr::locators::MacroPayloadLocator {
        anchor: verter_type_expr::locators::AuthoredAnchor {
            canonical_id: Arc::from("/c.vue"),
            symbol: Arc::from("default"),
            space: verter_type_expr::locators::LocatorSymbolSpace::Value,
        },
        macro_index: 0,
        payload: verter_type_expr::locators::MacroPayloadPosition::Field { field_index: 0 },
    };
    let authored = slot_field_function_source(&slot_with_payload(Some(payload.clone())));
    assert_eq!(
        authored,
        SemanticTypeSource::Authored(
            verter_type_expr::locators::AuthoredBodyLocator::MacroPayload(payload)
        ),
        "a payload-stamped slot must publish its authored position verbatim"
    );

    // (b) A payload-less slot publishes the closed FUNCTION fact with a
    // synthetic `props` parameter (typed-miss positions recovered on demand).
    let closed = slot_field_function_source(&slot_with_payload(None));
    let SemanticTypeSource::Closed(ClosedTypeFact::Function(signature)) = &closed else {
        panic!("a payload-less slot must publish a closed Function fact, got {closed:?}");
    };
    assert_eq!(signature.parameters.len(), 1, "one synthetic props param");
    assert_eq!(signature.parameters[0].name.as_deref(), Some("props"));
    assert!(
        signature.parameters[0].ty.is_none(),
        "the synthesized props object has no authored slot — the typed miss"
    );
    assert!(signature.return_ty.is_none(), "return recovered on demand");

    // (c) Raising the closed fact through the shared bridge interns a REAL
    // `Function` carrier node — the demand-driven node synthesis.
    let host = VerterHost::new_standalone(HostConfig::default());
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/c.vue".to_string(),
            source: Arc::from(
                "<script setup lang=\"ts\"></script>\n<template><div /></template>\n",
            ),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .expect("upsert /c.vue");
    let dispatch = crate::project_semantic_dispatch::ProjectSemanticDispatch::new(&host);
    let raised = dispatch
        .raise_semantic_type_source_to_hot(
            &closed,
            crate::project_semantic_dispatch::semantic_source::SourceRaiseContext {
                scope_canonical_id: "/c.vue",
                context:
                    crate::semantic_query::ProjectionReductionContext::structural_transit_with_mode(
                        crate::semantic_query::ProjectionMode::Navigate,
                    ),
                interior_failures: None,
            },
        )
        .expect("the closed Function fact must raise through the bridge");
    let data = crate::project_semantic_dispatch::node_data_for(&host, raised.node());
    assert!(
        matches!(
            data.as_deref(),
            Some(crate::semantic_query::SemanticNodeData::Function { params, .. }) if params.len() == 1
        ),
        "the raised closed Function fact must intern a Function carrier with the props param, got {data:?}"
    );
}

// ---------------------------------------------------------------------------
// Test 2 — `reduce_published_field_types` upgrades a field's published
// SOURCE to the complete closed leaf fact when the node-domain reduction
// resolves one.
//
// Driving `defineProps<Partial<Source>>()` end-to-end through
// `get_component_meta` and asserting each prop publishes the per-prop
// closed primitive leaf (`string`, `number`) rather than an unresolved
// carrier: the publication finaliser raised each field's source through
// the shared bridge, reduced it node-domain, and leaf-projected the result.
// ---------------------------------------------------------------------------

const PARTIAL_SOURCE_VUE: &str = r#"<script setup lang="ts">
interface Source {
  a: string;
  b: number;
}
defineProps<Partial<Source>>();
</script>
<template><div /></template>
"#;

#[test]
fn reduce_published_field_types_leaf_projects_the_resolved_source() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));

    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/c.vue".to_string(),
            source: Arc::from(PARTIAL_SOURCE_VUE),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .expect("upsert /c.vue");

    let meta = host
        .get_component_meta("/c.vue")
        .expect("get_component_meta /c.vue");

    let props = &meta.props;
    assert_eq!(props.len(), 2, "Partial<Source> must publish both props");

    let prop_a = props
        .iter()
        .find(|p| p.name == "a")
        .expect("prop `a` published");
    let prop_b = props
        .iter()
        .find(|p| p.name == "b")
        .expect("prop `b` published");

    assert_eq!(
        prop_a.type_source,
        verter_type_expr::facts::SourcePosition::Present(SemanticTypeSource::Closed(
            ClosedTypeFact::Leaf(LeafTypeFact::Primitive(PrimitiveName::String))
        )),
        "prop `a` must publish the complete closed `string` leaf fact; got {:?}",
        prop_a.type_source,
    );
    assert_eq!(
        prop_b.type_source,
        verter_type_expr::facts::SourcePosition::Present(SemanticTypeSource::Closed(
            ClosedTypeFact::Leaf(LeafTypeFact::Primitive(PrimitiveName::Number))
        )),
        "prop `b` must publish the complete closed `number` leaf fact; got {:?}",
        prop_b.type_source,
    );
}

// ---------------------------------------------------------------------------
// Test 3 — an imported alias field's published SOURCE re-raises through the
// ONE dispatch to the resolved union.
//
// The imported `variant` literal-union member publishes a SHALLOW source
// (never an eagerly-inlined union); DEMANDING it — raising the source
// through the shared bridge and materialising at the sealed output seam —
// yields the resolved `"primary" | "secondary"` union. This pins the whole
// source→engine→output chain without any stored `TypeExpr`.
// ---------------------------------------------------------------------------

const REGISTRY_SHALLOW_OWNER_VUE: &str = r#"<script setup lang="ts">
import type { ButtonProps } from "./button-types";
defineProps<ButtonProps>();
</script>
<template><div /></template>
"#;

const REGISTRY_SHALLOW_TYPES_TS: &str = r#"export interface ButtonProps {
  label?: string;
  variant?: "primary" | "secondary";
}
"#;

#[test]
fn imported_alias_source_demands_to_the_resolved_union_through_the_bridge() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));

    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/button-types.ts".to_string(),
            source: Arc::from(REGISTRY_SHALLOW_TYPES_TS),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("upsert /button-types.ts");

    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/c.vue".to_string(),
            source: Arc::from(REGISTRY_SHALLOW_OWNER_VUE),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .expect("upsert /c.vue");

    let meta = host
        .get_component_meta("/c.vue")
        .expect("get_component_meta /c.vue");

    // `label` resolves to a primitive — the finaliser leaf-projects it.
    let label = meta
        .props
        .iter()
        .find(|p| p.name == "label")
        .expect("imported `label` must surface in published props");
    assert_eq!(
        label.type_source,
        verter_type_expr::facts::SourcePosition::Present(SemanticTypeSource::Closed(
            ClosedTypeFact::Leaf(LeafTypeFact::Primitive(PrimitiveName::String))
        )),
        "imported `label` must publish the closed `string` leaf fact; got {:?}",
        label.type_source,
    );

    // `variant` is a literal union — no complete closed fact exists for it,
    // so the published source stays SHALLOW; DEMANDING it through the shared
    // bridge + the sealed output seam yields the resolved union arms.
    let variant = meta
        .props
        .iter()
        .find(|p| p.name == "variant")
        .expect("imported `variant` must surface in published props");
    let source = variant
        .type_source
        .present()
        .expect("imported `variant` must carry a typed source");
    assert!(
        !matches!(
            source,
            SemanticTypeSource::Closed(ClosedTypeFact::Leaf(LeafTypeFact::Primitive(_)))
        ),
        "a literal union must NOT collapse to a primitive leaf fact"
    );

    let dispatch = crate::project_semantic_dispatch::ProjectSemanticDispatch::new(host.as_ref());
    let raised = dispatch
        .raise_semantic_type_source_to_hot(
            source,
            crate::project_semantic_dispatch::semantic_source::SourceRaiseContext {
                scope_canonical_id: "/c.vue",
                context: crate::semantic_query::ProjectionReductionContext::published(
                    crate::semantic_query::ProjectionMode::Expanded,
                ),
                interior_failures: None,
            },
        )
        .expect("the variant source must raise through the bridge");
    let resolved = dispatch.resolve_hot_handle_with_context(
        raised,
        crate::semantic_query::ProjectionReductionContext::published(
            crate::semantic_query::ProjectionMode::Expanded,
        ),
    );
    let materialized = dispatch
        .materialize_output_type_expr_for_test(resolved)
        .expect("the resolved variant node materialises at the sealed output seam");
    let arms_present = matches!(&materialized, TypeExpr::Union(arms) if arms
        .iter()
        .any(|a| matches!(a, TypeExpr::Literal(verter_type_expr::LiteralValue::String(s)) if s == "primary"))
        && arms
            .iter()
            .any(|a| matches!(a, TypeExpr::Literal(verter_type_expr::LiteralValue::String(s)) if s == "secondary")));
    assert!(
        arms_present,
        "demanding the imported `variant` source must resolve the union arms \
         `\"primary\" | \"secondary\"` through the one dispatch; got {materialized:?}",
    );
}

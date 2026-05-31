//! Discriminating regression tests for the projector + materialiser
//! consumers that drive semantic decisions from the typed IR sidecars
//! (`AnalyzedSlotField.return_expr`, `AnalyzedSlotFieldBinding.binding_expr`,
//! `ExpandedField.shallow_type_expr`) rather than from raw source text.
//!
//! Each test in this file pins a single semantic decision that the
//! Typed-IR-Only Resolver Rule (see CLAUDE.md) makes structural rather
//! than text-driven. The discriminator in every test is the absence
//! of a source-text reparse: each test is engineered to FAIL if a
//! future change reintroduces text-mode reparse such as
//! `parse_jsdoc_tag_type_payload(raw_text)` or
//! `format!(...).parse_jsdoc_tag_type_payload(...)` round-trips inside
//! the projector / materialiser pipeline.

use std::sync::Arc;

use verter_type_expr::{FunctionParam, ObjectMember, PrimitiveName, TypeExpr};

use crate::types::FileKind;
use crate::{HostConfig, UpsertRequest, VerterHost};

use super::projectors::define_shapes::slot_field_function_type_expr;

// ---------------------------------------------------------------------------
// Test 1 — `slot_field_function_type_expr` direct typed construction.
//
// The function constructs `TypeExpr::Function` directly from
// `AnalyzedSlotField.return_expr` and `AnalyzedSlotFieldBinding.binding_expr`.
// No `format!()` of synthetic source text and no text-mode reparse
// (e.g. `parse_jsdoc_tag_type_payload`) round-trip.
//
// Discriminator: this test supplies `binding_expr` typed forms WITHOUT
// `type_annotation` text. A reparse path keyed off `type_annotation`
// would synthesise `format!("(props: { item: unknown, index: unknown })
// => any")` and reparse, producing object members whose `ty` is
// `Unknown`. The typed path uses the supplied typed bindings directly,
// so the resulting Function carries `Primitive(String)` for `item`
// and `Primitive(Number)` for `index`. `contains_unknown` walks the
// structure to catch any `TypeExpr::Unknown` shell introduced by a
// regressed reparse fallback.
// ---------------------------------------------------------------------------

#[test]
fn slot_field_function_type_expr_constructs_typed_function_directly() {
    let slot = verter_semantic::analysis::AnalyzedSlotField {
        name: "default".to_string(),
        is_required: true,
        bindings: vec![
            verter_semantic::analysis::AnalyzedSlotFieldBinding {
                name: "item".to_string(),
                // Discriminator: leave annotation None — only the typed
                // form is the authority. A reparse path collapses to
                // `unknown`.
                type_annotation: None,
                binding_expr: Some(TypeExpr::Primitive(PrimitiveName::String)),
                binding_expr_scope: Some(verter_type_expr::TypeExprScope::new("/c.vue")),
                span: verter_span::Span::default(),
            },
            verter_semantic::analysis::AnalyzedSlotFieldBinding {
                name: "index".to_string(),
                type_annotation: None,
                binding_expr: Some(TypeExpr::Primitive(PrimitiveName::Number)),
                binding_expr_scope: Some(verter_type_expr::TypeExprScope::new("/c.vue")),
                span: verter_span::Span::default(),
            },
        ],
        span: verter_span::Span::default(),
        return_type: None,
        return_expr: Some(TypeExpr::Primitive(PrimitiveName::Void)),
        return_expr_scope: Some(verter_type_expr::TypeExprScope::new("/c.vue")),
        description: None,
        tags: Vec::new(),
    };

    let ty = slot_field_function_type_expr(&slot);

    // Top-level shape must be a Function — NOT a Ref `unknown`, NOT a
    // primitive, NOT a generic wrapper. A reparse round-trip whose
    // synthesised source lost the bindings would produce a different
    // top-level shape.
    let func = match &ty {
        TypeExpr::Function(f) => f,
        other => panic!(
            "expected TypeExpr::Function, got `{other:?}` — typed constructor must produce a Function directly",
        ),
    };

    // Exactly one `props` parameter, taking the typed Object built
    // from the bindings.
    assert_eq!(
        func.parameters.len(),
        1,
        "function must take exactly one `props` parameter"
    );
    let param: &FunctionParam = &func.parameters[0];
    assert_eq!(param.name.as_deref(), Some("props"));
    assert!(!param.optional);
    assert!(!param.rest);

    // The `props` parameter is a typed Object with the two bindings.
    let object = match &param.ty {
        TypeExpr::Object(obj) => obj,
        other => panic!(
            "expected props param to be TypeExpr::Object, got `{other:?}` — the typed constructor must NOT reparse",
        ),
    };
    assert_eq!(object.properties.len(), 2, "must carry both bindings");

    let mut iter = object.properties.iter();
    let first = iter.next().expect("first property");
    let second = iter.next().expect("second property");
    let ObjectMember::Property(item_prop) = first else {
        panic!("first member must be a Property, got {first:?}");
    };
    let ObjectMember::Property(index_prop) = second else {
        panic!("second member must be a Property, got {second:?}");
    };
    assert_eq!(item_prop.name, "item");
    assert_eq!(item_prop.ty, TypeExpr::Primitive(PrimitiveName::String));
    assert_eq!(index_prop.name, "index");
    assert_eq!(index_prop.ty, TypeExpr::Primitive(PrimitiveName::Number));

    // Return type comes from `return_expr`. A reparse fallback with no
    // source text would default to `any` — here it must be the typed
    // `void` we supplied.
    let return_ty: &TypeExpr = func
        .return_type
        .as_ref()
        .expect("return type populated from return_expr")
        .as_ref();
    assert_eq!(*return_ty, TypeExpr::Primitive(PrimitiveName::Void));

    // Negative assertion: the typed constructor must NOT round-trip
    // through the analyzer's display strings. Walking the structure
    // for `TypeExpr::Unknown` shells catches any reparse fallback for
    // missing text.
    assert!(
        !contains_unknown(&ty),
        "typed constructor must not introduce `TypeExpr::Unknown` shells anywhere — got {ty:?}",
    );
}

fn contains_unknown(expr: &TypeExpr) -> bool {
    match expr {
        TypeExpr::Unknown { .. } => true,
        TypeExpr::Function(f) => {
            f.parameters.iter().any(|p| contains_unknown(&p.ty))
                || f.return_type.as_deref().is_some_and(contains_unknown)
        }
        TypeExpr::Object(o) => o.properties.iter().any(|m| match m {
            ObjectMember::Property(p) => contains_unknown(&p.ty),
            _ => false,
        }),
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Test 2 — `reduce_published_field_types` publishes the typed sidecar
// when the expanded `r#type` reduces to an unresolved mapped shell.
//
// Driving `defineProps<Partial<Source>>()` end-to-end through
// `get_component_meta` and asserting each prop publishes the per-prop
// primitive type (`string`, `number`) rather than the unresolved
// `Mapped { source: Unknown }` shell. The projector reads
// `field.shallow_type_expr` — the typed per-prop form lowered by the
// analyzer at OXC visit time — when the post-expansion `r#type`
// strictly underperforms the typed sidecar. A regression that
// reintroduced source-text reparse would also produce a passing
// result, but the architecture guard
// `no_parse_jsdoc_tag_type_payload_outside_jsdoc`
// blocks that escape hatch; together this fixture and the guard pin
// the typed-IR-only contract.
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
fn reduce_published_field_types_uses_shallow_typed_form_authoritatively() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));

    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/c.vue".to_string(),
            source: Arc::from(PARTIAL_SOURCE_VUE),
            file_kind: FileKind::VueSfc,
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

    // The post-reduction value-node form fails to lift `Partial`,
    // leaving `Mapped { source: Unknown }`. The typed shallow sidecar
    // is the authority: each prop publishes its per-prop primitive
    // directly. The assertion that the published TypeExpr for `a` is
    // `Primitive(String)` and for `b` is `Primitive(Number)` pins the
    // typed-form publication contract and discriminates against the
    // unreduced Mapped-shell publication.
    assert_eq!(
        prop_a.type_expr,
        TypeExpr::Primitive(PrimitiveName::String),
        "prop `a` must publish the typed primitive `string`, not a Mapped shell; got {:?}",
        prop_a.type_expr,
    );
    assert_eq!(
        prop_b.type_expr,
        TypeExpr::Primitive(PrimitiveName::Number),
        "prop `b` must publish the typed primitive `number`, not a Mapped shell; got {:?}",
        prop_b.type_expr,
    );

    // Negative assertion: must not leak the Mapped shell anywhere.
    assert!(
        !matches!(prop_a.type_expr, TypeExpr::Mapped { .. }),
        "Partial<Source> must not leak the Mapped shell for prop `a`"
    );
    assert!(
        !matches!(prop_b.type_expr, TypeExpr::Mapped { .. }),
        "Partial<Source> must not leak the Mapped shell for prop `b`"
    );
}

// ---------------------------------------------------------------------------
// Test 3 — Registry shallow walker reads the typed shallow sidecar.
//
// `collect_component_meta_registry_public_field_refs` recovers the
// bare `Ref` form from `field.shallow_type_expr` when the
// post-expansion `field.r#type` carries no actionable route. The
// typed sidecar replaces the legacy source-text reparse path.
//
// Discriminator: an SFC that imports a typed alias and consumes it
// via `defineProps`. The published prop's `field.r#type` post-expansion
// inlines the imported declaration's union arms. The walker must
// have enqueued `ButtonProps` as an import root for the materialiser
// to inline those arms — and the route from "this field's r#type
// carries no actionable shape" to "enqueue the import root" runs
// through `field.shallow_type_expr` reading.
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
fn registry_walker_collects_imported_ref_via_typed_shallow_form() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));

    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/button-types.ts".to_string(),
            source: Arc::from(REGISTRY_SHALLOW_TYPES_TS),
            file_kind: FileKind::NonSfc,
            aliases: Vec::new(),
        })
        .expect("upsert /button-types.ts");

    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/c.vue".to_string(),
            source: Arc::from(REGISTRY_SHALLOW_OWNER_VUE),
            file_kind: FileKind::VueSfc,
            aliases: Vec::new(),
        })
        .expect("upsert /c.vue");

    let meta = host
        .get_component_meta("/c.vue")
        .expect("get_component_meta /c.vue");

    // The imported `ButtonProps` alias must be materialised into the
    // owner's published props surface. Both `label` and `variant`
    // must be published — and the walker must have enqueued
    // `ButtonProps` as an import root via the typed shallow walk.
    let label = meta
        .props
        .iter()
        .find(|p| p.name == "label")
        .expect("imported `label` must surface in published props");
    let variant = meta
        .props
        .iter()
        .find(|p| p.name == "variant")
        .expect("imported `variant` must surface in published props");

    // `label` publishes the imported primitive type from the typed
    // shallow walker.
    assert_eq!(
        label.type_expr,
        TypeExpr::Primitive(PrimitiveName::String),
        "imported `label` must publish its typed primitive `string` via the shallow walker; got {:?}",
        label.type_expr,
    );

    // `variant` is a literal union — its descriptor MUST resolve to
    // the union arms, NOT to a bare `Unknown` shell. If the shallow
    // walker fails to enqueue `ButtonProps`, the materialiser never
    // inlines the union arms and `variant` is published as an
    // unresolved Ref or Unknown.
    let arms_present = matches!(&variant.type_expr, TypeExpr::Union(arms) if arms
        .iter()
        .any(|a| matches!(a, TypeExpr::Literal(verter_type_expr::LiteralValue::String(s)) if s == "primary"))
        && arms
            .iter()
            .any(|a| matches!(a, TypeExpr::Literal(verter_type_expr::LiteralValue::String(s)) if s == "secondary")));
    assert!(
        arms_present,
        "imported `variant` must publish union arms `\"primary\" | \"secondary\"` resolved through the imported `ButtonProps` alias; got {:?}",
        variant.type_expr,
    );

    // Negative: must NOT be Unknown — that would indicate the shallow
    // walker missed the import-root enqueue.
    assert!(
        !matches!(variant.type_expr, TypeExpr::Unknown { .. }),
        "imported `variant` must not leak `Unknown` — the typed shallow walker must enqueue the import root"
    );
}

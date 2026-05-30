//! Stage 4a — dispatch-only `define_*` coverage proof.
//!
//! These tests pin the architectural invariant that the public
//! `evaluated_types.define_props` / `define_emits` / `define_slots`
//! mirrors are produced by the ONE type-resolution engine (the
//! typeinfo/dispatch projector surface: `project_evaluated_types` +
//! `vue_macro_dtos` + `ProjectSemanticDispatch`), NOT by the legacy
//! prepared-surface walker materializer
//! (`produce_macro_object_shapes_for_purpose`).
//!
//! Discrimination contract: each test asserts a macro-shape outcome
//! (`evaluate_types().define_*` AND, where applicable, final component
//! meta) that the dispatch/projector path must reproduce once the
//! walker materializer + the two dispatch-first walker fallbacks are
//! retired. They are written to FAIL loudly if the dispatch surface
//! cannot reproduce a shape the walker used to fill — in which case
//! the fix is in dispatch/typeinfo (`ProjectSemanticDispatch` /
//! `ResolveMacroPayload` / `ProjectPath` / surface member reading /
//! generic substitution / emit branch merge), never a restored walker
//! fallback.
//!
//! The owner-local authority-gate test
//! (`owner_local_macro_root_authority_uses_typeinfo_surface`) pins the
//! semantics of `owner_local_macro_root_has_surface` after it is moved
//! off the walker bridge onto dispatch/typeinfo: props/model/slots true
//! on a non-empty member surface; emits true for property OR
//! call-signature event surfaces; empty roots false; expose/options
//! false.

#![allow(clippy::too_many_lines, dead_code, unused_imports)]

#[path = "component_meta_audit/harness.rs"]
mod harness;

use verter_semantic::analysis::type_expand::{ExpandedComponentTypes, ExpandedMacroObjectShape};
use verter_type_expr::{LiteralValue, TypeExpr};

/// Collect the member names published on the `define_props` mirror for
/// the (single) `defineProps` macro in `evaluated`.
fn define_props_member_names(evaluated: &ExpandedComponentTypes) -> Vec<String> {
    evaluated
        .define_props
        .iter()
        .flat_map(|entry| entry.result.value.properties.iter())
        .map(|prop| prop.name.clone())
        .collect()
}

/// Resolve the property type published on the `define_props` mirror for
/// `name`, if present.
fn define_props_member_type<'a>(
    evaluated: &'a ExpandedComponentTypes,
    name: &str,
) -> Option<&'a TypeExpr> {
    evaluated
        .define_props
        .iter()
        .flat_map(|entry| entry.result.value.properties.iter())
        .find(|prop| prop.name == name)
        .map(|prop| &prop.ty)
}

/// Collect event names from a `define_emits` macro-object shape:
/// property-style members plus call-signature leading-name literals.
fn define_emits_event_names(shape: &ExpandedMacroObjectShape) -> Vec<String> {
    let mut names: Vec<String> = shape
        .result
        .value
        .properties
        .iter()
        .map(|p| p.name.clone())
        .collect();
    for sig in &shape.result.value.call_signatures {
        let Some(first) = sig.parameters.first() else {
            continue;
        };
        match &first.ty {
            TypeExpr::Literal(LiteralValue::String(name)) => names.push(name.to_string()),
            TypeExpr::Union(types) => {
                for ty in types.iter() {
                    if let TypeExpr::Literal(LiteralValue::String(name)) = ty {
                        names.push(name.to_string());
                    }
                }
            }
            _ => {}
        }
    }
    names
}

// ─────────────────────────────────────────────────────────────────────
// 1. Owner-local props `extends Omit<Imported, K>` — cross-file heritage
//    via dispatch. Inherited props survive; omitted key is absent.
// ─────────────────────────────────────────────────────────────────────

const EXTERNAL_PROPS_TS: &str = r#"
export interface ExternalProps {
  alpha: string;
  beta: number;
  gamma: boolean;
}
"#;

const OMIT_EXTENDS_VUE: &str = r#"<script setup lang="ts">
import type { ExternalProps } from './external_props';
interface Props extends Omit<ExternalProps, 'beta'> {
  delta: string;
}
defineProps<Props>();
</script>
<template><div></div></template>
"#;

#[test]
fn dispatch_only_owner_local_props_extends_imported_omit_define_shape() {
    let host = harness::build_hermetic_host_with_lib(
        &[
            ("/external_props.ts", EXTERNAL_PROPS_TS),
            ("/OmitExtends.vue", OMIT_EXTENDS_VUE),
        ],
        &[("lib.es5.d.ts", harness::STUB_LIB_ES5)],
    );

    let evaluated = host
        .evaluate_types("/OmitExtends.vue")
        .expect("evaluate_types must resolve");
    let define_names = define_props_member_names(&evaluated);

    // Inherited (non-omitted) members + the own-body member must be on
    // the define_props mirror.
    for required in ["alpha", "gamma", "delta"] {
        assert!(
            define_names.iter().any(|n| n == required),
            "Stage 4a — `interface Props extends Omit<ExternalProps, 'beta'>` \
             define_props mirror MUST carry inherited member `{required}` via \
             the dispatch heritage surface. Got define_props: {define_names:?}"
        );
    }
    // The omitted key MUST be absent.
    assert!(
        !define_names.iter().any(|n| n == "beta"),
        "Stage 4a — `Omit<ExternalProps, 'beta'>` MUST exclude `beta` from the \
         define_props mirror. Got: {define_names:?}"
    );

    // Final component meta must agree.
    let meta = host
        .get_component_meta("/OmitExtends.vue")
        .expect("get_component_meta must resolve");
    let prop_names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();
    for required in ["alpha", "gamma", "delta"] {
        assert!(
            prop_names.contains(&required),
            "Stage 4a — final meta MUST keep inherited prop `{required}`. Got: {prop_names:?}"
        );
    }
    assert!(
        !prop_names.contains(&"beta"),
        "Stage 4a — final meta MUST exclude omitted prop `beta`. Got: {prop_names:?}"
    );
}

// ─────────────────────────────────────────────────────────────────────
// 2. Generic carrier `AccordionProps<T>` — substituted members survive,
//    publication stays bounded (no breadth-leak of the type-arg body).
// ─────────────────────────────────────────────────────────────────────

const ACCORDION_PROPS_TS: &str = r#"
export interface AccordionProps<T> {
  items: T[];
  selected: T;
  multiple: boolean;
}
"#;

const ACCORDION_VUE: &str = r#"<script setup lang="ts">
import type { AccordionProps } from './accordion_props';
defineProps<AccordionProps<string>>();
</script>
<template><div></div></template>
"#;

#[test]
fn dispatch_only_generic_carrier_define_props_instantiates_surface() {
    let host = harness::build_hermetic_host_with_lib(
        &[
            ("/accordion_props.ts", ACCORDION_PROPS_TS),
            ("/Accordion.vue", ACCORDION_VUE),
        ],
        &[("lib.es5.d.ts", harness::STUB_LIB_ES5)],
    );

    let evaluated = host
        .evaluate_types("/Accordion.vue")
        .expect("evaluate_types must resolve");
    let define_names = define_props_member_names(&evaluated);

    for required in ["items", "selected", "multiple"] {
        assert!(
            define_names.iter().any(|n| n == required),
            "Stage 4a — generic carrier `AccordionProps<string>` define_props \
             mirror MUST instantiate its member surface via dispatch. Got: \
             {define_names:?}"
        );
    }

    // The `selected: T` member, substituted with `string`, must NOT
    // remain a bare unresolved type parameter. Generic substitution is
    // part of semantic meaning — the dispatch instantiation path must
    // carry the `T := string` substitution.
    if let Some(selected_ty) = define_props_member_type(&evaluated, "selected") {
        assert!(
            !matches!(selected_ty, TypeExpr::TypeParameter(_)),
            "Stage 4a — `selected: T` substituted with `string` MUST NOT publish \
             a bare unresolved `TypeParameter`. Generic substitution must travel \
             through the dispatch instantiation. Got: {selected_ty:?}"
        );
    }

    // Publication bounded: the define_props mirror must not breadth-leak
    // members that are not declared on `AccordionProps`.
    for forbidden in ["length", "push", "pop"] {
        assert!(
            !define_names.iter().any(|n| n == forbidden),
            "Stage 4a — generic carrier publication MUST stay bounded; \
             `{forbidden}` (an array member of the `T[]` body) must NOT leak \
             into the macro surface. Got: {define_names:?}"
        );
    }

    let meta = host
        .get_component_meta("/Accordion.vue")
        .expect("get_component_meta must resolve");
    let prop_names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();
    for required in ["items", "selected", "multiple"] {
        assert!(
            prop_names.contains(&required),
            "Stage 4a — final meta MUST carry generic carrier prop `{required}`. \
             Got: {prop_names:?}"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────
// 3. Inherited emits conditional branch-merge — direct
//    `evaluate_types().define_emits` assertion. Plus a non-conditional
//    inherited-emits define_emits assertion.
// ─────────────────────────────────────────────────────────────────────

const CONDITIONAL_EMITS_VUE: &str = r#"<script setup lang="ts" generic="Mode extends 'editor' | 'viewer'">
type EditorEmits = { itemEdited: [id: number] };
type ViewerEmits = { itemViewed: [id: number] };
type ConditionalEmits = Mode extends 'editor' ? EditorEmits : ViewerEmits;
defineEmits<ConditionalEmits>();
</script>
<template><div /></template>
"#;

#[test]
fn dispatch_only_inherited_emits_conditional_branch_merge_define_shape() {
    let host = harness::build_hermetic_host_with_lib(
        &[("/ConditionalEmits.vue", CONDITIONAL_EMITS_VUE)],
        &[("lib.es5.d.ts", harness::STUB_LIB_ES5)],
    );

    let evaluated = host
        .evaluate_types("/ConditionalEmits.vue")
        .expect("evaluate_types must resolve");

    assert!(
        !evaluated.define_emits.is_empty(),
        "Stage 4a — undecided-conditional `defineEmits<ConditionalEmits>()` MUST \
         produce a define_emits mirror entry via the dispatch branch-merge \
         (`resolve_payload_surface_with_scope(EmitClassMacroObject)`). Got empty \
         define_emits."
    );
    let shape = &evaluated.define_emits[0];
    let event_names = define_emits_event_names(shape);

    for required in ["itemEdited", "itemViewed"] {
        assert!(
            event_names.iter().any(|n| n == required),
            "Stage 4a — the define_emits mirror MUST merge BOTH branches of the \
             undecided conditional emit payload (`Mode extends 'editor' ? \
             EditorEmits : ViewerEmits`). Event `{required}` is missing. The \
             branch-merge lives in `resolve_payload_surface_with_scope` and is \
             dispatched by the emits projector — NOT the walker. Got events: \
             {event_names:?}"
        );
    }
}

const NON_CONDITIONAL_EMITS_TS: &str = r#"
export type BaseEmits = {
  change: [value: string];
  reset: [];
};
"#;

const NON_CONDITIONAL_EMITS_VUE: &str = r#"<script setup lang="ts">
import type { BaseEmits } from './base_emits';
defineEmits<BaseEmits>();
</script>
<template><div /></template>
"#;

#[test]
fn dispatch_only_non_conditional_inherited_emits_define_shape() {
    let host = harness::build_hermetic_host_with_lib(
        &[
            ("/base_emits.ts", NON_CONDITIONAL_EMITS_TS),
            ("/NonConditionalEmits.vue", NON_CONDITIONAL_EMITS_VUE),
        ],
        &[("lib.es5.d.ts", harness::STUB_LIB_ES5)],
    );

    let evaluated = host
        .evaluate_types("/NonConditionalEmits.vue")
        .expect("evaluate_types must resolve");

    assert!(
        !evaluated.define_emits.is_empty(),
        "Stage 4a — non-conditional imported `defineEmits<BaseEmits>()` MUST \
         produce a define_emits mirror via the single-dispatch path. Got empty."
    );
    let event_names = define_emits_event_names(&evaluated.define_emits[0]);
    for required in ["change", "reset"] {
        assert!(
            event_names.iter().any(|n| n == required),
            "Stage 4a — non-conditional define_emits mirror MUST carry imported \
             emit `{required}` from the dispatch surface. Got: {event_names:?}"
        );
    }

    let meta = host
        .get_component_meta("/NonConditionalEmits.vue")
        .expect("get_component_meta must resolve");
    let final_events: Vec<&str> = meta.events.iter().map(|e| e.name.as_str()).collect();
    for required in ["change", "reset"] {
        assert!(
            final_events.contains(&required),
            "Stage 4a — final meta MUST carry emit `{required}`. Got: {final_events:?}"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────
// 4. Imported mapped slots — `define_slots` mirror + slot bindings +
//    final meta.
// ─────────────────────────────────────────────────────────────────────

const SLOTS_TS: &str = r#"
export interface RowProps {
  row: { id: number; label: string };
  index: number;
}
export type TableSlots = {
  [K in 'header' | 'body']: (props: RowProps) => unknown;
};
"#;

const MAPPED_SLOTS_VUE: &str = r#"<script setup lang="ts">
import type { TableSlots } from './slots_types';
defineSlots<TableSlots>();
</script>
<template><div></div></template>
"#;

#[test]
fn dispatch_only_imported_mapped_slots_define_shape_and_bindings() {
    let host = harness::build_hermetic_host_with_lib(
        &[
            ("/slots_types.ts", SLOTS_TS),
            ("/MappedSlots.vue", MAPPED_SLOTS_VUE),
        ],
        &[("lib.es5.d.ts", harness::STUB_LIB_ES5)],
    );

    let evaluated = host
        .evaluate_types("/MappedSlots.vue")
        .expect("evaluate_types must resolve");

    assert!(
        !evaluated.define_slots.is_empty(),
        "Stage 4a — imported mapped `defineSlots<TableSlots>()` MUST produce a \
         define_slots mirror via dispatch. Got empty define_slots."
    );
    let slot_names: Vec<String> = evaluated
        .define_slots
        .iter()
        .flat_map(|entry| entry.result.value.properties.iter())
        .map(|p| p.name.clone())
        .collect();
    for required in ["header", "body"] {
        assert!(
            slot_names.iter().any(|n| n == required),
            "Stage 4a — define_slots mirror MUST enumerate the mapped slot key \
             `{required}` via the dispatch mapped-surface. Got slots: {slot_names:?}"
        );
    }

    // Final meta + slot bindings.
    let meta = host
        .get_component_meta("/MappedSlots.vue")
        .expect("get_component_meta must resolve");
    let final_slots: Vec<&str> = meta.slots.iter().map(|s| s.name.as_str()).collect();
    for required in ["header", "body"] {
        assert!(
            final_slots.contains(&required),
            "Stage 4a — final meta MUST carry slot `{required}`. Got: {final_slots:?}"
        );
    }
    // At least one slot must carry resolved scoped bindings (`row` /
    // `index` from `RowProps`) — proving the slot-binding graph still
    // resolves the slot prop type through dispatch.
    let has_binding = meta.slots.iter().any(|s| {
        s.bindings
            .iter()
            .any(|b| b.name == "row" || b.name == "index")
    });
    assert!(
        has_binding,
        "Stage 4a — mapped slots MUST resolve their scoped bindings (`row` / \
         `index` from `RowProps`) through the dispatch slot-binding graph. Got \
         slots: {:?}",
        meta.slots
            .iter()
            .map(|s| (s.name.clone(), s.bindings.iter().map(|b| b.name.clone()).collect::<Vec<_>>()))
            .collect::<Vec<_>>()
    );
}

// ─────────────────────────────────────────────────────────────────────
// 5. Owner-local macro-root authority gate — typeinfo/dispatch surface.
//    props/model/slots true on non-empty surface; emits true for
//    property OR call-signature surfaces; empty false; expose/options
//    false.
// ─────────────────────────────────────────────────────────────────────

const AUTHORITY_PROPS_VUE: &str = r#"<script setup lang="ts">
interface LocalProps { title: string; count: number }
defineProps<LocalProps>();
</script>
<template><div></div></template>
"#;

const AUTHORITY_SLOTS_VUE: &str = r#"<script setup lang="ts">
type LocalSlots = { default: (props: { item: string }) => unknown };
defineSlots<LocalSlots>();
</script>
<template><div></div></template>
"#;

const AUTHORITY_EMITS_PROPERTY_VUE: &str = r#"<script setup lang="ts">
type LocalEmits = { change: [value: string] };
defineEmits<LocalEmits>();
</script>
<template><div></div></template>
"#;

const AUTHORITY_EMITS_CALLSIG_VUE: &str = r#"<script setup lang="ts">
defineEmits<{
  (e: 'click', id: number): void;
  (e: 'hover'): void;
}>();
</script>
<template><div></div></template>
"#;

const AUTHORITY_EMPTY_PROPS_VUE: &str = r#"<script setup lang="ts">
type EmptyProps = {};
defineProps<EmptyProps>();
</script>
<template><div></div></template>
"#;

const AUTHORITY_EXPOSE_VUE: &str = r#"<script setup lang="ts">
defineExpose({ focus: () => {} });
</script>
<template><div></div></template>
"#;

const AUTHORITY_OPTIONS_VUE: &str = r#"<script setup lang="ts">
defineOptions({ name: 'Widget', inheritAttrs: false });
</script>
<template><div></div></template>
"#;

#[test]
fn owner_local_macro_root_authority_uses_typeinfo_surface() {
    // Props/model/slots and emits surfaces (property + call-signature)
    // must publish through the dispatch/typeinfo surface so the cold
    // resolver's owner-local authority gate
    // (`owner_local_macro_root_has_surface`) can attest them. We assert
    // the OBSERVABLE consequence: the public define_* mirror (the same
    // dispatch surface the gate now queries) is non-empty for the
    // non-empty cases and empty for the empty case. expose/options have
    // no define_* mirror (the gate returns false for them by contract).

    // props — non-empty surface.
    {
        let host = harness::build_hermetic_host_with_lib(
            &[("/AuthorityProps.vue", AUTHORITY_PROPS_VUE)],
            &[("lib.es5.d.ts", harness::STUB_LIB_ES5)],
        );
        let evaluated = host.evaluate_types("/AuthorityProps.vue").unwrap();
        let names = define_props_member_names(&evaluated);
        assert!(
            names.iter().any(|n| n == "title") && names.iter().any(|n| n == "count"),
            "Stage 4a — owner-local props root MUST expose a non-empty dispatch \
             surface (gate=true). Got define_props: {names:?}"
        );
    }

    // slots — non-empty surface. Owner-local slots resolve their
    // binding shape through the graph-native slot-binding path (NOT the
    // define_slots mirror, which the materializer only fills from
    // resolved-macro entries). The gate's effect is observable as a
    // published slot on the final meta.
    {
        let host = harness::build_hermetic_host_with_lib(
            &[("/AuthoritySlots.vue", AUTHORITY_SLOTS_VUE)],
            &[("lib.es5.d.ts", harness::STUB_LIB_ES5)],
        );
        let meta = host.get_component_meta("/AuthoritySlots.vue").unwrap();
        let slot_names: Vec<&str> = meta.slots.iter().map(|s| s.name.as_str()).collect();
        assert!(
            slot_names.contains(&"default"),
            "Stage 4a — owner-local slots root MUST expose a non-empty surface \
             (gate=true) and publish the `default` slot. Got slots: {slot_names:?}"
        );
    }

    // emits — property-style surface.
    {
        let host = harness::build_hermetic_host_with_lib(
            &[("/AuthorityEmitsProp.vue", AUTHORITY_EMITS_PROPERTY_VUE)],
            &[("lib.es5.d.ts", harness::STUB_LIB_ES5)],
        );
        let evaluated = host.evaluate_types("/AuthorityEmitsProp.vue").unwrap();
        assert!(
            !evaluated.define_emits.is_empty()
                && define_emits_event_names(&evaluated.define_emits[0])
                    .iter()
                    .any(|n| n == "change"),
            "Stage 4a — owner-local emits root (property form) MUST expose a \
             non-empty dispatch surface (gate=true)."
        );
    }

    // emits — call-signature surface.
    {
        let host = harness::build_hermetic_host_with_lib(
            &[("/AuthorityEmitsCallsig.vue", AUTHORITY_EMITS_CALLSIG_VUE)],
            &[("lib.es5.d.ts", harness::STUB_LIB_ES5)],
        );
        let evaluated = host.evaluate_types("/AuthorityEmitsCallsig.vue").unwrap();
        assert!(
            !evaluated.define_emits.is_empty(),
            "Stage 4a — owner-local emits root (call-signature form) MUST produce \
             a define_emits mirror."
        );
        let event_names = define_emits_event_names(&evaluated.define_emits[0]);
        for required in ["click", "hover"] {
            assert!(
                event_names.iter().any(|n| n == required),
                "Stage 4a — call-signature emits MUST surface event `{required}` \
                 from the leading event-name literal via the dispatch surface \
                 (gate=true for call-signature event surfaces). Got: {event_names:?}"
            );
        }
    }

    // empty props — empty surface → gate false (the define_props mirror
    // carries no members).
    {
        let host = harness::build_hermetic_host_with_lib(
            &[("/AuthorityEmptyProps.vue", AUTHORITY_EMPTY_PROPS_VUE)],
            &[("lib.es5.d.ts", harness::STUB_LIB_ES5)],
        );
        let evaluated = host.evaluate_types("/AuthorityEmptyProps.vue").unwrap();
        let names = define_props_member_names(&evaluated);
        assert!(
            names.is_empty(),
            "Stage 4a — an empty `defineProps<{{}}>()` root MUST expose an EMPTY \
             dispatch surface (gate=false). Got define_props: {names:?}"
        );
    }

    // expose — the macro-root authority gate returns false for
    // defineExpose. A component with ONLY defineExpose publishes no
    // props/events from it.
    {
        let host = harness::build_hermetic_host_with_lib(
            &[("/AuthorityExpose.vue", AUTHORITY_EXPOSE_VUE)],
            &[("lib.es5.d.ts", harness::STUB_LIB_ES5)],
        );
        let meta = host.get_component_meta("/AuthorityExpose.vue").unwrap();
        assert!(
            meta.props.is_empty() && meta.events.is_empty(),
            "Stage 4a — the macro-root authority gate returns FALSE for \
             defineExpose; no props/events may be published from it. Got \
             props={:?} events={:?}",
            meta.props.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
            meta.events.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
        );
    }

    // options — the macro-root authority gate returns false for
    // defineOptions.
    {
        let host = harness::build_hermetic_host_with_lib(
            &[("/AuthorityOptions.vue", AUTHORITY_OPTIONS_VUE)],
            &[("lib.es5.d.ts", harness::STUB_LIB_ES5)],
        );
        let meta = host.get_component_meta("/AuthorityOptions.vue").unwrap();
        assert!(
            meta.props.is_empty() && meta.events.is_empty(),
            "Stage 4a — the macro-root authority gate returns FALSE for \
             defineOptions; no props/events may be published from it. Got \
             props={:?} events={:?}",
            meta.props.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
            meta.events.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
        );
    }
}

// The JSX intrinsic projection regression test lives in-crate
// (`meta_resolve_tests.rs::stage4a_namespace_qualified_global_resolves_via_dispatch_only`)
// because it must call the `pub(crate)`
// `project_type_surface_expr_via_host_threaded` bridge directly — that
// bridge is the one whose `cached_prepared_root_surface` fallback Stage
// 4a removes, and the JSX-intrinsic consumer
// (`host_manage/intrinsic_projection.rs`) resolves the
// namespace-qualified `JSX.IntrinsicElements` global through it.

//! Dispatch-only `define_*` coverage proof.
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

use crate::harness;

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
            "`interface Props extends Omit<ExternalProps, 'beta'>` \
             define_props mirror MUST carry inherited member `{required}` via \
             the dispatch heritage surface. Got define_props: {define_names:?}"
        );
    }
    // The omitted key MUST be absent.
    assert!(
        !define_names.iter().any(|n| n == "beta"),
        "`Omit<ExternalProps, 'beta'>` MUST exclude `beta` from the \
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
            "final meta MUST keep inherited prop `{required}`. Got: {prop_names:?}"
        );
    }
    assert!(
        !prop_names.contains(&"beta"),
        "final meta MUST exclude omitted prop `beta`. Got: {prop_names:?}"
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
            "generic carrier `AccordionProps<string>` define_props \
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
            "`selected: T` substituted with `string` MUST NOT publish \
             a bare unresolved `TypeParameter`. Generic substitution must travel \
             through the dispatch instantiation. Got: {selected_ty:?}"
        );
    }

    // Publication bounded: the define_props mirror must not breadth-leak
    // members that are not declared on `AccordionProps`.
    for forbidden in ["length", "push", "pop"] {
        assert!(
            !define_names.iter().any(|n| n == forbidden),
            "generic carrier publication MUST stay bounded; \
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
            "final meta MUST carry generic carrier prop `{required}`. \
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
        "undecided-conditional `defineEmits<ConditionalEmits>()` MUST \
         produce a define_emits mirror entry via the dispatch branch-merge \
         (`resolve_payload_surface_with_scope(EmitClassMacroObject)`). Got empty \
         define_emits."
    );
    let shape = &evaluated.define_emits[0];
    let event_names = define_emits_event_names(shape);

    for required in ["itemEdited", "itemViewed"] {
        assert!(
            event_names.iter().any(|n| n == required),
            "the define_emits mirror MUST merge BOTH branches of the \
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
        "non-conditional imported `defineEmits<BaseEmits>()` MUST \
         produce a define_emits mirror via the single-dispatch path. Got empty."
    );
    let event_names = define_emits_event_names(&evaluated.define_emits[0]);
    for required in ["change", "reset"] {
        assert!(
            event_names.iter().any(|n| n == required),
            "non-conditional define_emits mirror MUST carry imported \
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
            "final meta MUST carry emit `{required}`. Got: {final_events:?}"
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
        "imported mapped `defineSlots<TableSlots>()` MUST produce a \
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
            "define_slots mirror MUST enumerate the mapped slot key \
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
            "final meta MUST carry slot `{required}`. Got: {final_slots:?}"
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
        "mapped slots MUST resolve their scoped bindings (`row` / \
         `index` from `RowProps`) through the dispatch slot-binding graph. Got \
         slots: {:?}",
        meta.slots
            .iter()
            .map(|s| (
                s.name.clone(),
                s.bindings
                    .iter()
                    .map(|b| b.name.clone())
                    .collect::<Vec<_>>()
            ))
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
            "owner-local props root MUST expose a non-empty dispatch \
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
            "owner-local slots root MUST expose a non-empty surface \
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
            "owner-local emits root (property form) MUST expose a \
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
            "owner-local emits root (call-signature form) MUST produce \
             a define_emits mirror."
        );
        let event_names = define_emits_event_names(&evaluated.define_emits[0]);
        for required in ["click", "hover"] {
            assert!(
                event_names.iter().any(|n| n == required),
                "call-signature emits MUST surface event `{required}` \
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
            "an empty `defineProps<{{}}>()` root MUST expose an EMPTY \
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
            "the macro-root authority gate returns FALSE for \
             defineExpose; no props/events may be published from it. Got \
             props={:?} events={:?}",
            meta.props
                .iter()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>(),
            meta.events
                .iter()
                .map(|e| e.name.as_str())
                .collect::<Vec<_>>(),
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
            "the macro-root authority gate returns FALSE for \
             defineOptions; no props/events may be published from it. Got \
             props={:?} events={:?}",
            meta.props
                .iter()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>(),
            meta.events
                .iter()
                .map(|e| e.name.as_str())
                .collect::<Vec<_>>(),
        );
    }
}

// ─────────────────────────────────────────────────────────────────────
// 6. Aliased-union DTO. `type UnionAlias = Fixed | Bubble;
//    defineProps<UnionAlias>()`. The macro-object DTO surface MUST
//    enumerate the UNION of object-arm members (every member present in
//    ANY arm is part of the component macro surface — the Vue macro
//    convention), NOT the TS property-access INTERSECTION of common
//    members.
//
//    Discrimination: branch-only members (`width` on `Fixed`, `offset`
//    on `Bubble`) appear ONLY under the `MacroObjectSurface` demand. A
//    `published(Shallow)` demand synthesises the intersection and
//    drops both branch-only members, keeping only the common `tag`. The
//    correct path is `resolve_vue_macro_surface`'s `terminal_context`
//    (`macro_object_surface` instead of `published`).
// ─────────────────────────────────────────────────────────────────────

const ALIASED_UNION_PROPS_VUE: &str = r#"<script setup lang="ts">
type Fixed = { tag: 'fixed'; width: number };
type Bubble = { tag: 'bubble'; offset: number };
type UnionAlias = Fixed | Bubble;
defineProps<UnionAlias>();
</script>
<template><div></div></template>
"#;

#[test]
fn p2a_aliased_union_define_props_enumerates_both_arms() {
    use verter_semantic::analysis::AnalyzedMacroKind;
    use verter_session::typeinfo::types::{TypeInfoQueryLevel, VueMacroSurfaceRequest};

    let host = harness::build_hermetic_host_with_lib(
        &[("/AliasedUnionProps.vue", ALIASED_UNION_PROPS_VUE)],
        &[("lib.es5.d.ts", harness::STUB_LIB_ES5)],
    );

    // (a) Direct `vue_macro_dtos` (FullMetadata) — the aliased-union surface.
    let dtos = host.vue_macro_dtos(&VueMacroSurfaceRequest {
        owner_canonical: std::sync::Arc::from("/AliasedUnionProps.vue"),
        macro_index: 0,
        macro_kind: AnalyzedMacroKind::DefineProps,
        root_identity: [0u8; 16],
        level: TypeInfoQueryLevel::FullMetadata,
    });
    let dto_names: Vec<&str> = dtos.props.iter().map(|p| p.name.as_str()).collect();
    // The common member is always present (it is in both arms).
    assert!(
        dto_names.contains(&"tag"),
        "the macro-object DTO surface MUST carry the common member `tag`. \
         Got: {dto_names:?}"
    );
    // Branch-only members survive ONLY under the union-arm enumeration
    // (`MacroObjectSurface`). Under ordinary `Published(Shallow)`
    // intersection, BOTH drop — that is the aliased-union bug this test
    // discriminates.
    for branch_only in ["width", "offset"] {
        assert!(
            dto_names.contains(&branch_only),
            "the macro-object DTO surface MUST enumerate the UNION of \
             object-arm members; branch-only member `{branch_only}` is missing. \
             Ordinary `Published(Shallow)` synthesises the property-access \
             INTERSECTION and drops it — the `macro_object_surface` demand \
             enumerates the arms. Got: {dto_names:?}"
        );
    }

    // (b) End-to-end: the `evaluate_types().define_props` mirror reads the
    // same now-union-enumerated DTO surface.
    let evaluated = host
        .evaluate_types("/AliasedUnionProps.vue")
        .expect("evaluate_types must resolve");
    let define_names = define_props_member_names(&evaluated);
    for required in ["tag", "width", "offset"] {
        assert!(
            define_names.iter().any(|n| n == required),
            "the define_props mirror MUST carry union-arm member \
             `{required}` from the macro-object DTO surface. Got: {define_names:?}"
        );
    }

    // Final component meta must agree — every union-arm prop is published.
    let meta = host
        .get_component_meta("/AliasedUnionProps.vue")
        .expect("get_component_meta must resolve");
    let prop_names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();
    for required in ["tag", "width", "offset"] {
        assert!(
            prop_names.contains(&required),
            "final meta MUST carry union-arm prop `{required}`. Got: {prop_names:?}"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────
// 7. Open-conditional props root — `defineProps<Props<T>>()` where
//    `type Props<T> = T extends 'a' ? { a?: string } : { b?: string }`
//    and `T` is the component's UNBOUND generic param. The check
//    (`T extends 'a'`) cannot be decided (T is open), so the macro
//    object-surface MUST enumerate BOTH branches' members — every member
//    that ANY branch could contribute — each OPTIONAL (present only when
//    the unresolved check selects that branch). This is the macro
//    object-surface union convention applied to an OPEN conditional root.
//
//    Discrimination: the open-conditional empty-path Shallow contract
//    returns an EMPTY surface (branch selection is impossible). Under the
//    macro object-surface demand, the macro-object-surface arm distributes
//    both branches through `merge_union_surfaces_for_macro`. Pre-fix the
//    define_props surface is EMPTY (both `a` and `b` missing); post-fix
//    both `a` and `b` are present AND optional. (Contrast test 3, which
//    covers the EMITS open conditional via the separate emits branch-merge
//    path — props travel through the shallow walker, which is the path the
//    macro-object-surface arm repairs.)
// ─────────────────────────────────────────────────────────────────────

const OPEN_CONDITIONAL_PROPS_VUE: &str = r#"<script setup lang="ts" generic="T extends 'a' | 'b'">
type Props<T> = T extends 'a' ? { a?: string } : { b?: string };
defineProps<Props<T>>();
</script>
<template><div></div></template>
"#;

#[test]
fn open_conditional_props_root_enumerates_both_branches() {
    use verter_semantic::analysis::AnalyzedMacroKind;
    use verter_session::typeinfo::types::{TypeInfoQueryLevel, VueMacroSurfaceRequest};

    let host = harness::build_hermetic_host_with_lib(
        &[("/OpenConditionalProps.vue", OPEN_CONDITIONAL_PROPS_VUE)],
        &[("lib.es5.d.ts", harness::STUB_LIB_ES5)],
    );

    // (a) Direct `vue_macro_dtos` (FullMetadata) — the macro object
    // surface enumerates BOTH conditional branches' members.
    let dtos = host.vue_macro_dtos(&VueMacroSurfaceRequest {
        owner_canonical: std::sync::Arc::from("/OpenConditionalProps.vue"),
        macro_index: 0,
        macro_kind: AnalyzedMacroKind::DefineProps,
        root_identity: [0u8; 16],
        level: TypeInfoQueryLevel::FullMetadata,
    });
    let dto_names: Vec<&str> = dtos.props.iter().map(|p| p.name.as_str()).collect();
    for required in ["a", "b"] {
        assert!(
            dto_names.contains(&required),
            "open-conditional macro props root \
             `defineProps<Props<T>>()` (where `Props<T> = T extends 'a' ? \
             {{ a? }} : {{ b? }}`) MUST enumerate BOTH branches' members; \
             branch member `{required}` is missing. The OPEN conditional under \
             a macro object surface distributes both branches through \
             `merge_union_surfaces_for_macro` — an empty surface means the \
             macro-object-surface arm regressed back to the empty \
             `OpenConditional` contract. Got: {dto_names:?}"
        );
    }
    // Each branch member is present in exactly ONE branch ⇒ optional on
    // the merged macro surface (`declaring_arms < arm_count`).
    for required in ["a", "b"] {
        let prop = dtos
            .props
            .iter()
            .find(|p| p.name == required)
            .unwrap_or_else(|| panic!("prop `{required}` must be present"));
        assert!(
            prop.is_optional,
            "open-conditional branch member `{required}` is present in \
             only ONE of the two branches, so it MUST be OPTIONAL on the merged \
             macro object surface. Got is_optional={}",
            prop.is_optional
        );
    }

    // (b) End-to-end: the `evaluate_types().define_props` mirror reads the
    // same both-branches-enumerated surface.
    let evaluated = host
        .evaluate_types("/OpenConditionalProps.vue")
        .expect("evaluate_types must resolve");
    let define_names = define_props_member_names(&evaluated);
    for required in ["a", "b"] {
        assert!(
            define_names.iter().any(|n| n == required),
            "the define_props mirror MUST carry open-conditional branch \
             member `{required}`. Got: {define_names:?}"
        );
    }

    // (c) Final component meta must agree — both branch props published,
    // both optional.
    let meta = host
        .get_component_meta("/OpenConditionalProps.vue")
        .expect("get_component_meta must resolve");
    let prop_names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();
    for required in ["a", "b"] {
        assert!(
            prop_names.contains(&required),
            "final meta MUST carry open-conditional branch prop \
             `{required}`. Got: {prop_names:?}"
        );
        let prop = meta
            .props
            .iter()
            .find(|p| p.name == required)
            .expect("prop present");
        assert!(
            !prop.required,
            "open-conditional branch prop `{required}` MUST be \
             optional (required=false) on the final meta. Got required={}",
            prop.required
        );
    }
}

// ─────────────────────────────────────────────────────────────────────
// 7. `Pick<Target, KeyAlias>` — the Pick key set is an ALIASED string-
//    literal union (`type AlphaKeys = 'a' | 'b'`), not an inline
//    `'a' | 'b'`. The dispatch projector must resolve the alias to its
//    union BEFORE enumerating the picked keys, so only the aliased keys
//    survive and the un-picked key is absent. Re-homes the deleted spike
//    fixture E (alias-keyed Pick) as a discriminating dispatch test.
// ─────────────────────────────────────────────────────────────────────

const PICK_THROUGH_ALIAS_VUE: &str = r#"<script setup lang="ts">
export type AlphaKeys = 'a' | 'b';
interface AlphaTarget {
  a: string;
  b: number;
  c: boolean;
}
defineProps<Pick<AlphaTarget, AlphaKeys>>();
</script>
<template><div></div></template>
"#;

#[test]
fn dispatch_only_pick_through_aliased_key_union_define_shape() {
    let host = harness::build_hermetic_host_with_lib(
        &[("/PickThroughAlias.vue", PICK_THROUGH_ALIAS_VUE)],
        &[("lib.es5.d.ts", harness::STUB_LIB_ES5)],
    );

    let evaluated = host
        .evaluate_types("/PickThroughAlias.vue")
        .expect("evaluate_types must resolve");
    let define_names = define_props_member_names(&evaluated);

    // The aliased keys `a` and `b` must be picked — the dispatch
    // projector must expand `AlphaKeys` to `'a' | 'b'` before
    // enumerating, NOT treat the alias as an opaque single key.
    for required in ["a", "b"] {
        assert!(
            define_names.iter().any(|n| n == required),
            "`Pick<AlphaTarget, AlphaKeys>` (AlphaKeys = 'a' | 'b') define_props mirror MUST \
             carry picked member `{required}` via alias-resolved key enumeration. \
             Got define_props: {define_names:?}"
        );
    }
    // The un-picked key `c` must be absent.
    assert!(
        !define_names.iter().any(|n| n == "c"),
        "`Pick<AlphaTarget, AlphaKeys>` MUST exclude the un-picked member `c` from the \
         define_props mirror. Got: {define_names:?}"
    );

    // Final component meta must agree.
    let meta = host
        .get_component_meta("/PickThroughAlias.vue")
        .expect("get_component_meta must resolve");
    let prop_names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();
    for required in ["a", "b"] {
        assert!(
            prop_names.contains(&required),
            "final meta MUST keep alias-picked prop `{required}`. Got: {prop_names:?}"
        );
    }
    assert!(
        !prop_names.contains(&"c"),
        "final meta MUST exclude the un-picked prop `c`. Got: {prop_names:?}"
    );
}

// ─────────────────────────────────────────────────────────────────────
// 8. `Omit<RecursiveTarget, K>` — the Omit target is SELF-RECURSIVE
//    (`parent: ExtendedNode`, `children: ExtendedNode[]`), and the omit
//    keys are exactly the self-referential members. The dispatch
//    projector must drop the recursive members without diverging on the
//    self-reference, keeping the scalar members. Re-homes the deleted
//    spike fixture F (recursive target under Omit) as a discriminating
//    dispatch test — recursion AND Omit together, not the two axes
//    separately.
// ─────────────────────────────────────────────────────────────────────

const OMIT_RECURSIVE_VUE: &str = r#"<script setup lang="ts">
interface ExtendedNode {
  id: string;
  label: number;
  extra: boolean;
  parent: ExtendedNode;
  children: ExtendedNode[];
}
defineProps<Omit<ExtendedNode, 'parent' | 'children'>>();
</script>
<template><div></div></template>
"#;

#[test]
fn dispatch_only_omit_recursive_target_self_reference_define_shape() {
    let host = harness::build_hermetic_host_with_lib(
        &[("/OmitRecursive.vue", OMIT_RECURSIVE_VUE)],
        &[("lib.es5.d.ts", harness::STUB_LIB_ES5)],
    );

    let evaluated = host
        .evaluate_types("/OmitRecursive.vue")
        .expect("evaluate_types must resolve");
    let define_names = define_props_member_names(&evaluated);

    // The scalar members survive the Omit and the recursion does not
    // cause divergence or member loss.
    for required in ["id", "label", "extra"] {
        assert!(
            define_names.iter().any(|n| n == required),
            "`Omit<ExtendedNode, 'parent' | 'children'>` define_props mirror MUST carry scalar \
             member `{required}` — the self-recursive `parent`/`children` members must not \
             derail enumeration of the surviving members. Got define_props: {define_names:?}"
        );
    }
    // The self-referential omitted keys must be absent.
    for omitted in ["parent", "children"] {
        assert!(
            !define_names.iter().any(|n| n == omitted),
            "`Omit<ExtendedNode, 'parent' | 'children'>` MUST exclude the self-referential \
             member `{omitted}` from the define_props mirror. Got: {define_names:?}"
        );
    }

    // Final component meta must agree.
    let meta = host
        .get_component_meta("/OmitRecursive.vue")
        .expect("get_component_meta must resolve");
    let prop_names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();
    for required in ["id", "label", "extra"] {
        assert!(
            prop_names.contains(&required),
            "final meta MUST keep scalar prop `{required}`. Got: {prop_names:?}"
        );
    }
    for omitted in ["parent", "children"] {
        assert!(
            !prop_names.contains(&omitted),
            "final meta MUST exclude self-referential omitted prop `{omitted}`. \
             Got: {prop_names:?}"
        );
    }
}

// The JSX intrinsic projection regression test lives in-crate
// (`meta_resolve_tests.rs::reexported_intrinsic_shape_resolves_via_dispatch_only`)
// because it must call the `pub(crate)`
// `project_type_surface_expr_via_host_threaded` bridge directly — that
// bridge carries no `cached_prepared_root_surface` walker fallback, and
// the JSX-intrinsic consumer
// (`host_manage/intrinsic_projection.rs`) resolves the re-exported
// intrinsic attribute shape through it.

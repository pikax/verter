//! @ai-generated — typeinfo Vue-adapter (U3a) defineSlots-normalizer
//! discriminating tests.
//!
//! Companion to `vue_adapter.rs` (the props / emits / model normalizer and
//! member-visibility tests) and `vue_adapter_cache.rs` (the cache /
//! public-type tests). This file holds the `defineSlots` normalizer tests:
//! function-member filtering, first-param object bindings, union-of-callables
//! slots, Pick-binding publication (symbolic nominal-root vs concrete
//! structural / userland), and the intentional nullable-slot drop. Split out
//! so neither file exceeds the `no_oversize_files` architecture-guard limit.

use std::sync::Arc;

use verter_semantic::analysis::types::AnalyzedMacroKind;

use crate::typeinfo::framework_surface::vue_exec::{
    resolved_vue_surface_for_test, slots_from_typeinfo_surface,
};
use crate::typeinfo::types::{TypeInfoQueryLevel, VueMacroSurfaceRequest};
use crate::types::{HostConfig, UpsertRequest};
use crate::VerterHost;

fn make_host() -> Arc<VerterHost> {
    Arc::new(VerterHost::new_standalone(HostConfig::default()))
}

fn upsert(host: &VerterHost, canonical_id: &str, source: &str) {
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(canonical_id.to_string()),
        input_id: canonical_id.to_string(),
        source: Arc::from(source),
        file_language: crate::LanguageRegistry::global()
            .classify_static(canonical_id)
            .static_resolution(),
        aliases: Vec::new(),
    });
}

fn whole_hash(host: &VerterHost, canonical_id: &str) -> verter_semantic::analysis::types::Hash16 {
    host.ensure_indexed_ready(canonical_id)
        .expect("indexed ready")
        .whole_hash
}

/// Find the index of the first macro of `kind` in the SFC.
fn macro_index_of(host: &VerterHost, canonical_id: &str, kind: AnalyzedMacroKind) -> usize {
    let indexed = host.ensure_indexed_ready(canonical_id).expect("indexed");
    indexed
        .snapshot
        .macros
        .iter()
        .position(|m| m.kind == kind)
        .unwrap_or_else(|| panic!("no {kind:?} macro in {canonical_id}"))
}

fn props_request(
    host: &VerterHost,
    canonical_id: &str,
    kind: AnalyzedMacroKind,
) -> VueMacroSurfaceRequest {
    VueMacroSurfaceRequest {
        owner_canonical: Arc::from(canonical_id),
        macro_index: macro_index_of(host, canonical_id, kind),
        macro_kind: kind,
        root_identity: whole_hash(host, canonical_id),
        level: TypeInfoQueryLevel::FullMetadata,
    }
}

// ---------------------------------------------------------------------------
// (4) defineSlots normalizer — function-like members only, first-param object
//     bindings, return preserved.
//
//     Discriminating: a non-function member must be FILTERED; the binding
//     names must come from the first-param object.
// ---------------------------------------------------------------------------

const VUE_SLOTS: &str = r#"<script setup lang="ts">
defineSlots<{
  default(props: { item: string; index: number }): any;
  header(props: { title: string }): any;
}>();
</script>
"#;

#[test]
fn define_slots_normalizer_filters_to_functions_and_extracts_bindings() {
    const FILE: &str = "/w/SlotsComp.vue";
    let host = make_host();
    upsert(&host, FILE, VUE_SLOTS);

    let request = props_request(&host, FILE, AnalyzedMacroKind::DefineSlots);
    let surface = host
        .resolve_vue_macro_surface(&request)
        .expect("defineSlots must resolve a surface");
    let slots =
        slots_from_typeinfo_surface(&*host, &resolved_vue_surface_for_test(surface.clone()));

    let mut names: Vec<&str> = slots.iter().map(|s| s.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, vec!["default", "header"], "both slots surface");

    let default_slot = slots.iter().find(|s| s.name == "default").unwrap();
    let mut binding_names: Vec<&str> = default_slot
        .bindings
        .iter()
        .map(|b| b.name.as_str())
        .collect();
    binding_names.sort_unstable();
    assert_eq!(
        binding_names,
        vec!["index", "item"],
        "slot bindings come from the first-param object's properties"
    );
    assert!(
        default_slot
            .bindings
            .iter()
            .all(|b| b.binding_expr.is_some()),
        "each binding carries its typed binding_expr"
    );

    let header = slots.iter().find(|s| s.name == "header").unwrap();
    assert_eq!(
        header.bindings.len(),
        1,
        "header slot has one binding (title)"
    );
}

// ---------------------------------------------------------------------------
// (4a) defineSlots normalizer — non-function members are FILTERED, and the
//      function slots' return type is preserved.
//
//      Discriminating: a `notASlot: string` property member is NOT a slot —
//      it must be absent. The function slot's `return_expr` / `return_type`
//      must reflect the declared return (not dropped).
// ---------------------------------------------------------------------------

const VUE_SLOTS_MIXED: &str = r#"<script setup lang="ts">
defineSlots<{
  body(props: { row: number }): string;
  notASlot: string;
}>();
</script>
"#;

#[test]
fn define_slots_normalizer_filters_non_function_members_and_preserves_return() {
    const FILE: &str = "/w/SlotsMixed.vue";
    let host = make_host();
    upsert(&host, FILE, VUE_SLOTS_MIXED);

    let request = props_request(&host, FILE, AnalyzedMacroKind::DefineSlots);
    let surface = host
        .resolve_vue_macro_surface(&request)
        .expect("defineSlots must resolve a surface");
    let slots =
        slots_from_typeinfo_surface(&*host, &resolved_vue_surface_for_test(surface.clone()));

    let names: Vec<&str> = slots.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["body"],
        "only the function-like member is a slot; the property member is filtered (negative)"
    );
    assert!(
        !slots.iter().any(|s| s.name == "notASlot"),
        "a non-function property member is NOT a slot (explicit negative)"
    );

    let body = &slots[0];
    // The function returns `string` — the return is preserved as a typed expr
    // and a display string, not dropped.
    let return_expr = body
        .return_expr
        .as_ref()
        .expect("the slot function's return type is preserved");
    assert!(
        matches!(
            return_expr,
            verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::String)
        ),
        "body's return_expr is the primitive `string`, got {return_expr:?}"
    );
    assert_eq!(
        body.return_type.as_deref(),
        Some("string"),
        "the display return_type renders the typed return"
    );
}

// ---------------------------------------------------------------------------
// (4a-union) defineSlots normalizer — a slot typed as a UNION of function
//      aliases (`default: SlotA | SlotB`). The slot member resolves to a
//      `Union` of two `Function` carriers; the normalizer must publish it as a
//      slot. Vue invokes `$slots.default`, whose type is `SlotA | SlotB`, so the
//      child must pass an argument assignable to BOTH arms' params — i.e.
//      `PA & PB`. As object types `{ shared, a } & { shared, b }` carries all
//      three members, so the consumer's template can destructure `shared`, `a`,
//      AND `b`. The param is therefore the INTERSECTION of the arms' first
//      params (TS-correct contravariant merge) and the return is the UNION of
//      the arms' returns.
//
//      Discriminating: pre-fix `slot_callable_param_and_return` matched only
//      `Function` + `Intersection` (and `realize_callable_member` did not
//      descend `Union` arms or unwrap an alias `DeclPlaceholder`), so a `Union`
//      of function-alias slots returned `None` and the slot was DROPPED
//      entirely. Post-fix the slot publishes and its bindings surface.
// ---------------------------------------------------------------------------

const VUE_SLOTS_UNION: &str = r#"<script setup lang="ts">
type SlotA = (props: { shared: string; a: string }) => any;
type SlotB = (props: { shared: string; b: number }) => any;
type Slots = { default: SlotA | SlotB };
defineSlots<Slots>();
</script>
"#;

#[test]
fn define_slots_normalizer_publishes_union_of_function_slots() {
    const FILE: &str = "/w/SlotsUnion.vue";
    let host = make_host();
    upsert(&host, FILE, VUE_SLOTS_UNION);

    let request = props_request(&host, FILE, AnalyzedMacroKind::DefineSlots);
    let surface = host
        .resolve_vue_macro_surface(&request)
        .expect("defineSlots must resolve a surface");
    let slots =
        slots_from_typeinfo_surface(&*host, &resolved_vue_surface_for_test(surface.clone()));

    // The union-of-functions slot must be PUBLISHED (pre-fix it was dropped).
    let names: Vec<&str> = slots.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["default"],
        "a `default: SlotA | SlotB` union-of-functions slot must publish \
         (pre-fix the union arm was unhandled and the slot was dropped)"
    );

    let default_slot = slots.iter().find(|s| s.name == "default").unwrap();
    let binding_names: std::collections::BTreeSet<&str> = default_slot
        .bindings
        .iter()
        .map(|b| b.name.as_str())
        .collect();
    // Param = `PA & PB` (the contravariant intersection of the arms' first
    // params). As object types that carries every member, so `shared`, `a`, and
    // `b` all surface as bindings the consumer's template can destructure.
    assert_eq!(
        binding_names,
        std::collections::BTreeSet::from(["a", "b", "shared"]),
        "a union slot's bindings are the intersection of the arms' params \
         (`PA & PB`), which carries every member; got {binding_names:?}"
    );
    assert!(
        default_slot
            .bindings
            .iter()
            .all(|b| b.binding_expr.is_some()),
        "each union-slot binding carries its typed binding_expr"
    );
}

// ---------------------------------------------------------------------------
// (4a-union-noparam) defineSlots normalizer — a UNION slot where one arm is a
//      NO-PARAM callable (`default: A | B`, `A = () => any`, `B = (props: { a })
//      => any`). The slot still PUBLISHES (a union of callables is slot-like),
//      but a template destructuring `<template #default="{ a }">` runs for
//      WHICHEVER arm the slot is — and when it is the `A` arm there are no slot
//      props. So `a` is NOT a guaranteed binding: the published `bindings` set
//      must be EMPTY (a no-param arm guarantees nothing).
//
//      Discriminating: pre-fix `slot_callable_param_and_return_from_arms` pushed
//      only the first params of arms that HAD one, then intersected the present
//      params — so `B`'s `{ a }` became the slot param and `a` was wrongly
//      published. Post-fix `all_arms_have_first_param` is false (the `A` arm has
//      no param), so the param drops to `None` and no binding surfaces.
// ---------------------------------------------------------------------------

const VUE_SLOTS_UNION_NOPARAM: &str = r#"<script setup lang="ts">
type A = () => any;
type B = (props: { a: string }) => any;
type Slots = { default: A | B };
defineSlots<Slots>();
</script>
"#;

#[test]
fn define_slots_normalizer_drops_union_bindings_when_an_arm_has_no_param() {
    const FILE: &str = "/w/SlotsUnionNoParam.vue";
    let host = make_host();
    upsert(&host, FILE, VUE_SLOTS_UNION_NOPARAM);

    let request = props_request(&host, FILE, AnalyzedMacroKind::DefineSlots);
    let surface = host
        .resolve_vue_macro_surface(&request)
        .expect("defineSlots must resolve a surface");
    let slots =
        slots_from_typeinfo_surface(&*host, &resolved_vue_surface_for_test(surface.clone()));

    // The union-of-callables slot is still PUBLISHED (positive): one arm having
    // no param does not make the member non-slot-like.
    let default_slot = slots
        .iter()
        .find(|s| s.name == "default")
        .expect("a `default: A | B` union slot must still publish");

    // SOUNDNESS (the fix): `a` comes only from the `B` arm; the `A` arm is a
    // no-param callable, so `a` is NOT guaranteed and must NOT be published.
    let binding_names: Vec<&str> = default_slot
        .bindings
        .iter()
        .map(|b| b.name.as_str())
        .collect();
    assert!(
        !default_slot.bindings.iter().any(|b| b.name == "a"),
        "a no-param union arm guarantees no bindings: `a` (present only in the \
         `(props: {{ a }}) => any` arm) MUST NOT be published, got {binding_names:?}"
    );
    assert!(
        default_slot.bindings.is_empty(),
        "a union slot with a no-param arm publishes NO guaranteed bindings, got \
         {binding_names:?}"
    );
}

// ---------------------------------------------------------------------------
// (4b) defineSlots normalizer — a `Pick<T,'k'>` first parameter yields the
//      picked bindings (matching the eager local-SFC rail). The new path must
//      navigate the first-param type through the SHARED resolver to its object
//      surface, NOT only accept a literal `TypeExpr::Object` (the pre-fix bug
//      dropped Pick bindings entirely).
//
//      Discriminating: `binding_fields_from_param_node` projects the
//      `Pick<RowApi, 'name'|'value'>` first param through the SHARED shallow
//      surface, so the picked `name` + `value` keys surface as bindings; a reader
//      that only accepted a literal object surface would yield ZERO bindings here.
//      The bindings publish the SYMBOLIC `RowApi['name']` / `RowApi['value']`
//      indexed accesses: the inline macro-authored `RowApi` source root lowers
//      to a nominal `BareRef`, and a nominal-root predicate that rejects
//      `BareRef` (publishing the concrete member values instead) FAILS the
//      symbolic assertions.
// ---------------------------------------------------------------------------

const VUE_SLOTS_PICK: &str = r#"<script setup lang="ts">
interface RowApi {
  name: string;
  value: number;
  hidden: boolean;
}
defineSlots<{
  row(props: Pick<RowApi, 'name' | 'value'>): void;
}>();
</script>
"#;

#[test]
fn define_slots_normalizer_extracts_pick_bindings() {
    const FILE: &str = "/w/SlotsPick.vue";
    let host = make_host();
    upsert(&host, FILE, VUE_SLOTS_PICK);

    let request = props_request(&host, FILE, AnalyzedMacroKind::DefineSlots);
    let surface = host
        .resolve_vue_macro_surface(&request)
        .expect("defineSlots with a Pick first-param resolves a surface");
    let slots =
        slots_from_typeinfo_surface(&*host, &resolved_vue_surface_for_test(surface.clone()));

    let row = slots
        .iter()
        .find(|s| s.name == "row")
        .expect("the row slot surfaces");

    let mut binding_names: Vec<&str> = row.bindings.iter().map(|b| b.name.as_str()).collect();
    binding_names.sort_unstable();
    assert_eq!(
        binding_names,
        vec!["name", "value"],
        "the Pick<RowApi,'name'|'value'> first param contributes name + value bindings"
    );
    // Negative: the un-picked `hidden` key is NOT a binding.
    assert!(
        !row.bindings.iter().any(|b| b.name == "hidden"),
        "the un-picked `hidden` key must not surface as a binding (negative)"
    );
    // Builtin `Pick<NamedRoot, K>` over a nominal source publishes symbolic
    // `NamedRoot['member']` bindings, including inline macro-authored sources
    // lowered as `BareRef`. Structural-source Pick and userland Pick remain
    // concrete.
    let name = row
        .bindings
        .iter()
        .find(|b| b.name == "name")
        .expect("the picked `name` key surfaces as a binding");
    assert_eq!(
        name.type_annotation.as_deref(),
        Some("RowApi['name']"),
        "the `name` binding displays the symbolic indexed access"
    );
    assert!(
        matches!(
            name.binding_expr.as_ref(),
            Some(verter_type_expr::TypeExpr::IndexedAccess { object, index })
                if matches!(&**object, verter_type_expr::TypeExpr::Ref { name, .. } if name.as_ref() == "RowApi")
                && matches!(&**index, verter_type_expr::TypeExpr::Literal(
                    verter_type_expr::LiteralValue::String(member)
                ) if member == "name")
        ),
        "the `name` binding is the symbolic `RowApi['name']` indexed access, got {:?}",
        name.binding_expr
    );
    let value = row
        .bindings
        .iter()
        .find(|b| b.name == "value")
        .expect("the picked `value` key surfaces as a binding");
    assert_eq!(
        value.type_annotation.as_deref(),
        Some("RowApi['value']"),
        "the `value` binding displays the symbolic indexed access"
    );
    assert!(
        matches!(
            value.binding_expr.as_ref(),
            Some(verter_type_expr::TypeExpr::IndexedAccess { object, index })
                if matches!(&**object, verter_type_expr::TypeExpr::Ref { name, .. } if name.as_ref() == "RowApi")
                && matches!(&**index, verter_type_expr::TypeExpr::Literal(
                    verter_type_expr::LiteralValue::String(member)
                ) if member == "value")
        ),
        "the `value` binding is the symbolic `RowApi['value']` indexed access, got {:?}",
        value.binding_expr
    );
}

// ---------------------------------------------------------------------------
// (4b-imported) defineSlots normalizer — an INLINE `Pick<ImportedSource, K>`
//      whose nominal source root is IMPORTED (cross-file) publishes the SAME
//      symbolic `ImportedSource['k']` binding as the local-inline case: the
//      published slot-binding shape does not depend on where the nominal
//      source lives, so BOTH local-inline and imported-inline `BareRef` roots
//      are locked symbolic.
//
//      Discriminating: against a nominal-root predicate that rejects the
//      inline-authored `BareRef` source root, the binding materialises the
//      CONCRETE member value (`string`) and the symbolic `IndexedAccess`
//      assertions FAIL.
// ---------------------------------------------------------------------------

const PICK_IMPORTED_SOURCE: &str = r#"export interface ImportedRowApi {
  label: string;
  hidden: boolean;
}
"#;

const VUE_SLOTS_PICK_IMPORTED: &str = r#"<script setup lang="ts">
import type { ImportedRowApi } from './imported-row-api';
defineSlots<{
  row(props: Pick<ImportedRowApi, 'label'>): void;
}>();
</script>
"#;

#[test]
fn define_slots_imported_inline_pick_publishes_symbolic_binding() {
    const BASE: &str = "/w/imported-row-api.ts";
    const FILE: &str = "/w/SlotsPickImported.vue";
    let host = make_host();
    upsert(&host, BASE, PICK_IMPORTED_SOURCE);
    upsert(&host, FILE, VUE_SLOTS_PICK_IMPORTED);

    let request = props_request(&host, FILE, AnalyzedMacroKind::DefineSlots);
    let surface = host
        .resolve_vue_macro_surface(&request)
        .expect("defineSlots with an imported-inline Pick resolves a surface");
    let slots =
        slots_from_typeinfo_surface(&*host, &resolved_vue_surface_for_test(surface.clone()));

    let row = slots
        .iter()
        .find(|s| s.name == "row")
        .expect("the row slot surfaces");
    // Negative: the un-picked `hidden` key is NOT a binding.
    assert!(
        !row.bindings.iter().any(|b| b.name == "hidden"),
        "the un-picked `hidden` key must not surface as a binding (negative)"
    );
    let label = row
        .bindings
        .iter()
        .find(|b| b.name == "label")
        .expect("the picked `label` key surfaces as a binding");
    // The SAME symbolic shape as the local-inline case — an imported-inline
    // `BareRef` source root is locked symbolic too.
    assert_eq!(
        label.type_annotation.as_deref(),
        Some("ImportedRowApi['label']"),
        "the `label` binding displays the symbolic indexed access"
    );
    assert!(
        matches!(
            label.binding_expr.as_ref(),
            Some(verter_type_expr::TypeExpr::IndexedAccess { object, index })
                if matches!(&**object, verter_type_expr::TypeExpr::Ref { name, .. } if name.as_ref() == "ImportedRowApi")
                && matches!(&**index, verter_type_expr::TypeExpr::Literal(
                    verter_type_expr::LiteralValue::String(member)
                ) if member == "label")
        ),
        "the `label` binding is the symbolic `ImportedRowApi['label']` indexed access, got {:?}",
        label.binding_expr
    );
}

// ---------------------------------------------------------------------------
// (4c) defineSlots normalizer — a USERLAND `Pick` that shadows the builtin, and a
//      BUILTIN `Pick` over a STRUCTURAL (non-nominal) source, both publish the
//      CONCRETE member value, NOT the symbolic `Root['member']` access. Only a
//      BUILTIN `Pick<NamedRoot, K>` with a NOMINAL source root is symbolic.
// ---------------------------------------------------------------------------

const VUE_SLOTS_USERLAND_PICK: &str = r#"<script setup lang="ts">
type Pick<T, _K> = { wrapped: T };
interface Cfg {
  a: string;
  b: number;
}
defineSlots<{
  row(props: Pick<Cfg, 'a'>): void;
}>();
</script>
"#;

#[test]
fn define_slots_userland_pick_shadow_publishes_concrete_not_symbolic() {
    // A USERLAND `type Pick<T, _K> = { wrapped: T }` SHADOWS the builtin `Pick`.
    // Its slot param `Pick<Cfg, 'a'>` is NOT a builtin Pick — the `wrapped` binding
    // must be the CONCRETE userland-Pick body member type, NOT the symbolic
    // `Cfg['wrapped']`.
    //
    // Discriminating: pre-fix `pick_source_root_node` matched ANY `Pick`
    // InstantiationRef (no `__builtin__` check), so it returned the userland Pick's
    // `args[0]` (`Cfg`, a nominal DeclRef) and published the BOGUS symbolic
    // `Cfg['wrapped']` indexed access. Post-fix the `__builtin__` gate rejects the
    // userland Pick, so `wrapped` mints its own concrete value.
    const FILE: &str = "/w/SlotsUserlandPick.vue";
    let host = make_host();
    upsert(&host, FILE, VUE_SLOTS_USERLAND_PICK);

    let request = props_request(&host, FILE, AnalyzedMacroKind::DefineSlots);
    let surface = host
        .resolve_vue_macro_surface(&request)
        .expect("defineSlots with a userland-Pick first-param resolves a surface");
    let slots =
        slots_from_typeinfo_surface(&*host, &resolved_vue_surface_for_test(surface.clone()));

    let row = slots
        .iter()
        .find(|s| s.name == "row")
        .expect("the row slot surfaces");
    let wrapped = row
        .bindings
        .iter()
        .find(|b| b.name == "wrapped")
        .expect("the userland-Pick body member `wrapped` surfaces as a binding");
    // The concrete userland-Pick body member is present.
    assert!(
        wrapped.binding_expr.is_some(),
        "the `wrapped` binding carries its concrete typed value"
    );
    // THE FIX: NOT the symbolic `Cfg['wrapped']` indexed access.
    assert!(
        !matches!(
            wrapped.binding_expr,
            Some(verter_type_expr::TypeExpr::IndexedAccess { .. })
        ),
        "the userland-Pick `wrapped` binding must NOT be published as a symbolic \
         `Cfg['wrapped']` indexed access, got {:?}",
        wrapped.binding_expr
    );
}

const VUE_SLOTS_STRUCTURAL_PICK: &str = r#"<script setup lang="ts">
defineSlots<{
  row(props: Pick<{ foo: string }, 'foo'>): void;
}>();
</script>
"#;

#[test]
fn define_slots_structural_source_pick_publishes_concrete_not_symbolic() {
    // A BUILTIN `Pick<{ foo: string }, 'foo'>` over a STRUCTURAL object source: the
    // `foo` binding must be the CONCRETE member type (`string`), NOT a bogus
    // symbolic `<object>['foo']` access.
    //
    // Discriminating: pre-fix `pick_source_root_node` returned `args[0]` for ANY
    // source shape, so a structural object source was published as the nonsensical
    // symbolic `{ foo: string }['foo']` IndexedAccess. Post-fix the nominal-root
    // restriction rejects the structural source, so `foo` mints its concrete
    // `string` value.
    const FILE: &str = "/w/SlotsStructuralPick.vue";
    let host = make_host();
    upsert(&host, FILE, VUE_SLOTS_STRUCTURAL_PICK);

    let request = props_request(&host, FILE, AnalyzedMacroKind::DefineSlots);
    let surface = host
        .resolve_vue_macro_surface(&request)
        .expect("defineSlots with a structural-source Pick resolves a surface");
    let slots =
        slots_from_typeinfo_surface(&*host, &resolved_vue_surface_for_test(surface.clone()));

    let row = slots
        .iter()
        .find(|s| s.name == "row")
        .expect("the row slot surfaces");
    let foo = row
        .bindings
        .iter()
        .find(|b| b.name == "foo")
        .expect("the picked `foo` key surfaces as a binding");
    // THE FIX: NOT a symbolic indexed access over the structural object source.
    assert!(
        !matches!(
            foo.binding_expr,
            Some(verter_type_expr::TypeExpr::IndexedAccess { .. })
        ),
        "the structural-source `foo` binding must NOT be a symbolic indexed access, got {:?}",
        foo.binding_expr
    );
    // POSITIVE: the concrete picked member type `string`.
    assert!(
        matches!(
            foo.binding_expr,
            Some(verter_type_expr::TypeExpr::Primitive(
                verter_type_expr::PrimitiveName::String
            ))
        ),
        "the `foo` binding is the concrete `string` member type, got {:?}",
        foo.binding_expr
    );
}

// ---------------------------------------------------------------------------
// (4d) defineSlots normalizer — a NULLABLE slot (`SlotAlias | undefined`) is
//      INTENTIONALLY DROPPED (behavior-preserving, codex-adjudicated). The strict
//      `realized_callable_root(context)?` prefilter matches legacy Vue output;
//      enabling nullable Vue slots is an out-of-scope future enhancement.
// ---------------------------------------------------------------------------

const VUE_SLOTS_NULLABLE: &str = r#"<script setup lang="ts">
type SlotAlias = (props: { x: number }) => any;
type Slots = { present: SlotAlias; nullable: SlotAlias | undefined };
defineSlots<Slots>();
</script>
"#;

#[test]
fn define_slots_nullable_slot_is_intentionally_dropped() {
    // INTENTIONAL-DROP parity lock (codex-adjudicated behavior-PRESERVING): a
    // `nullable: SlotAlias | undefined` slot is DROPPED, not published — the strict
    // `realized_callable_root(context)?` prefilter in `slots_from_typeinfo_surface`
    // refuses the `Union(Fn, undefined)` (the `undefined` arm does not realize to a
    // callable), matching legacy Vue output (legacy also dropped a nullable slot).
    // This is INTENTIONAL and LOCKED here; enabling nullable Vue slots is an
    // out-of-scope future enhancement, not a bug. The contrast slot
    // `present: SlotAlias` (non-nullable) IS published, proving the drop is
    // specifically the `| undefined` nullish arm, not a blanket failure.
    const FILE: &str = "/w/SlotsNullable.vue";
    let host = make_host();
    upsert(&host, FILE, VUE_SLOTS_NULLABLE);

    let request = props_request(&host, FILE, AnalyzedMacroKind::DefineSlots);
    let surface = host
        .resolve_vue_macro_surface(&request)
        .expect("defineSlots resolves a surface");
    let slots =
        slots_from_typeinfo_surface(&*host, &resolved_vue_surface_for_test(surface.clone()));

    // CONTRAST: the non-nullable `present` slot IS published (the callable realizes).
    assert!(
        slots.iter().any(|s| s.name == "present"),
        "the non-nullable `present: SlotAlias` slot is published"
    );
    // INTENTIONAL DROP: the nullable slot is NOT published.
    assert!(
        !slots.iter().any(|s| s.name == "nullable"),
        "the nullable `nullable: SlotAlias | undefined` slot is INTENTIONALLY DROPPED \
         (behavior-preserving), got {:?}",
        slots.iter().map(|s| s.name.as_str()).collect::<Vec<_>>()
    );
}

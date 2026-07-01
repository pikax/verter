//! @ai-generated — typeinfo Vue-adapter (U3a) discriminating tests.
//!
//! The Vue adapter resolves a `.vue` SFC's PUBLIC component type and FullMetadata
//! macro surfaces THROUGH the shared typeinfo surface path, and the prop / emit /
//! slot normalizers produce the final component-meta DTOs from that surface.
//! Every test is typeinfo-primary and discriminating: it asserts a property that
//! holds for the adapter's real output and would FAIL against a stub / a wrong
//! source.

use std::sync::Arc;

use verter_semantic::analysis::types::AnalyzedMacroKind;

use crate::typeinfo::framework_surface::vue_exec::{
    emits_from_typeinfo_surface, props_from_typeinfo_surface, resolved_vue_surface_for_test,
    slots_from_typeinfo_surface,
};
use crate::typeinfo::types::{TypeInfoQueryLevel, VueMacroSurfaceRequest};
use crate::types::{HostConfig, UpsertRequest};
use crate::VerterHost;

const VUE_PROPS: &str = r#"<script setup lang="ts">
interface Props {
  /** the count */
  count: number;
  label?: string;
  readonly id: string;
}
defineProps<Props>();
</script>
"#;

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
// (1) defineProps normalizer — members, optional, readonly (from the surface),
//     declared_in_macro_type_arg, JSDoc from spans.
//
//     Discriminating: a stub returning `Vec::new()` fails the non-empty
//     assertion; sourcing `readonly` from a hardcoded `false` (the eager
//     rail's bug) fails the `id.readonly` assertion; not slicing JSDoc spans
//     fails the description assertion.
// ---------------------------------------------------------------------------

#[test]
fn define_props_normalizer_produces_fields_with_surface_readonly_and_jsdoc() {
    const FILE: &str = "/w/PropsComp.vue";
    let host = make_host();
    upsert(&host, FILE, VUE_PROPS);

    let request = props_request(&host, FILE, AnalyzedMacroKind::DefineProps);
    let surface = host
        .resolve_vue_macro_surface(&request)
        .expect("defineProps<Props>() must resolve a macro surface");
    let props =
        props_from_typeinfo_surface(&*host, &resolved_vue_surface_for_test(surface.clone()));

    let mut names: Vec<&str> = props.iter().map(|p| p.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(
        names,
        vec!["count", "id", "label"],
        "all three Props members must surface"
    );

    let count = props.iter().find(|p| p.name == "count").unwrap();
    assert!(!count.is_optional, "count is required");
    assert_eq!(
        count.description.as_deref(),
        Some("the count"),
        "count's JSDoc description must be sliced from the surface spans"
    );
    assert!(
        count.declared_in_macro_type_arg,
        "count is declared in the macro type arg's own body"
    );
    // Concrete typed form + scope (not just `is_some()`): `count: number` raises
    // to the `number` primitive, scoped to the SFC where it was lowered.
    assert!(
        matches!(
            count.type_expr,
            Some(verter_type_expr::TypeExpr::Primitive(
                verter_type_expr::PrimitiveName::Number
            ))
        ),
        "count.type_expr is the `number` primitive, got {:?}",
        count.type_expr
    );
    assert_eq!(
        count.type_expr_scope.as_ref().map(|s| s.as_str()),
        Some(FILE),
        "count.type_expr_scope is the SFC the prop was declared in"
    );

    let label = props.iter().find(|p| p.name == "label").unwrap();
    assert!(label.is_optional, "label? is optional");
    // `label?: string` → the `string` primitive (the `?` is the optional flag,
    // not part of the value type).
    assert!(
        matches!(
            label.type_expr,
            Some(verter_type_expr::TypeExpr::Primitive(
                verter_type_expr::PrimitiveName::String
            ))
        ),
        "label.type_expr is the `string` primitive, got {:?}",
        label.type_expr
    );

    // `AnalyzedPropField` carries no readonly axis (it is recovered downstream),
    // but the RICHER readonly fact the U3c flip will read lives on the typeinfo
    // surface member — the eager rail hardcoded `false`. Assert the surface
    // carries it so the source the normalizer-adjacent consumers read is right.
    let id_member = surface
        .surface
        .members
        .iter()
        .find(|m| m.name.as_ref() == "id")
        .expect("id member on surface");
    assert!(id_member.readonly, "id is readonly on the typeinfo surface");
    let count_member = surface
        .surface
        .members
        .iter()
        .find(|m| m.name.as_ref() == "count")
        .expect("count member on surface");
    assert!(
        !count_member.readonly,
        "count is NOT readonly on the surface (negative assertion)"
    );
}

// ---------------------------------------------------------------------------
// (2) defineEmits normalizer — call-signature event extraction with leading
//     event-name parameter STRIPPED.
//
//     Discriminating: reading the event name from `keyof` would surface
//     numeric tuple indices (never "change"/"select"); not stripping the
//     first param would leave the event-name parameter in the payload.
// ---------------------------------------------------------------------------

const VUE_EMITS_CALLSIG: &str = r#"<script setup lang="ts">
defineEmits<{
  (e: 'change', value: number): void;
  (e: 'select', id: string, extra: boolean): void;
}>();
</script>
"#;

#[test]
fn define_emits_normalizer_extracts_call_signature_events_and_strips_event_param() {
    const FILE: &str = "/w/EmitsComp.vue";
    let host = make_host();
    upsert(&host, FILE, VUE_EMITS_CALLSIG);

    let request = props_request(&host, FILE, AnalyzedMacroKind::DefineEmits);
    let surface = host
        .resolve_vue_macro_surface(&request)
        .expect("defineEmits must resolve a macro surface");
    let emits =
        emits_from_typeinfo_surface(&*host, &resolved_vue_surface_for_test(surface.clone()));

    let mut names: Vec<&str> = emits.iter().map(|e| e.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(
        names,
        vec!["change", "select"],
        "event names come from the call-signature first param literal, NOT keyof"
    );

    // The payload is the call signature's REMAINING parameters as a TUPLE (the
    // Vue emit payload shape): `change` keeps `[value: number]` (1 element),
    // `select` keeps `[id: string, extra: boolean]` (2 elements). The leading
    // event-name parameter is stripped.
    let change = emits.iter().find(|e| e.name == "change").unwrap();
    let payload = change
        .payload_expr
        .as_ref()
        .expect("change payload_expr must be the stripped tuple");
    let verter_type_expr::TypeExpr::Tuple { elements, .. } = payload else {
        panic!("change payload must be a Tuple, got {payload:?}");
    };
    assert_eq!(
        elements.len(),
        1,
        "leading event-name param must be stripped from the change payload tuple"
    );
    // Negative: the surviving tuple element is NOT the event-name literal.
    assert!(
        !matches!(
            elements[0].ty,
            verter_type_expr::TypeExpr::Literal(verter_type_expr::LiteralValue::String(_))
        ),
        "the stripped payload tuple's element must not be the event-name literal"
    );

    let select = emits.iter().find(|e| e.name == "select").unwrap();
    let verter_type_expr::TypeExpr::Tuple {
        elements: select_elements,
        ..
    } = select.payload_expr.as_ref().expect("select payload_expr")
    else {
        panic!("select payload must be a Tuple");
    };
    assert_eq!(
        select_elements.len(),
        2,
        "select keeps its two non-event params as tuple elements"
    );
}

// ---------------------------------------------------------------------------
// (3) defineEmits property-style fallback — used ONLY when no call signature
//     was found.
//
//     Discriminating: a mixed surface (call sig present) must NOT add the
//     property members alongside; a pure property surface MUST produce the
//     property events.
// ---------------------------------------------------------------------------

const VUE_EMITS_PROPS: &str = r#"<script setup lang="ts">
defineEmits<{
  change: [value: number];
  remove: [];
}>();
</script>
"#;

#[test]
fn define_emits_normalizer_property_style_fallback() {
    const FILE: &str = "/w/EmitsProp.vue";
    let host = make_host();
    upsert(&host, FILE, VUE_EMITS_PROPS);

    let request = props_request(&host, FILE, AnalyzedMacroKind::DefineEmits);
    let surface = host
        .resolve_vue_macro_surface(&request)
        .expect("property-style defineEmits must resolve a surface");
    let emits =
        emits_from_typeinfo_surface(&*host, &resolved_vue_surface_for_test(surface.clone()));

    let mut names: Vec<&str> = emits.iter().map(|e| e.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(
        names,
        vec!["change", "remove"],
        "property-style emit names come from the member names"
    );
}

// ---------------------------------------------------------------------------
// (3a) Mixed emit interface — a call signature AND a property member. The
//      call-signature precedence means the property member's event is EXCLUDED
//      (matching the eager projector: property-style fires ONLY when NO
//      call-sig emit was found).
//
//      Discriminating: a naive "union of call-sig + property events" would
//      surface `notAnEvent` alongside `change`; the precedence rule excludes
//      it. Also asserts the call-sig payload's stripped tuple shape + SFC scope.
// ---------------------------------------------------------------------------

const VUE_EMITS_MIXED: &str = r#"<script setup lang="ts">
defineEmits<{
  (e: 'change', value: number): void;
  notAnEvent: [flag: boolean];
}>();
</script>
"#;

#[test]
fn define_emits_normalizer_mixed_callsig_excludes_property_members() {
    const FILE: &str = "/w/EmitsMixed.vue";
    let host = make_host();
    upsert(&host, FILE, VUE_EMITS_MIXED);

    let request = props_request(&host, FILE, AnalyzedMacroKind::DefineEmits);
    let surface = host
        .resolve_vue_macro_surface(&request)
        .expect("mixed defineEmits resolves a surface");
    let emits =
        emits_from_typeinfo_surface(&*host, &resolved_vue_surface_for_test(surface.clone()));

    let names: Vec<&str> = emits.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["change"],
        "the call-signature event fires; the property member is EXCLUDED (call-sig precedence)"
    );
    // Explicit negative: the property member did NOT become an event.
    assert!(
        !emits.iter().any(|e| e.name == "notAnEvent"),
        "a property member must NOT add an event alongside a call signature (negative)"
    );

    // The call-sig payload is the stripped tuple `[value: number]` — the
    // leading event-name parameter is dropped, leaving one tuple element typed
    // `number` (the Vue emit payload shape).
    let change = &emits[0];
    let verter_type_expr::TypeExpr::Tuple { elements, .. } = change
        .payload_expr
        .as_ref()
        .expect("change payload_expr is the stripped tuple")
    else {
        panic!("change payload must be a Tuple");
    };
    assert_eq!(
        elements.len(),
        1,
        "the leading event-name param is stripped, leaving the payload tuple element"
    );
    assert!(
        matches!(
            elements[0].ty,
            verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number)
        ),
        "the surviving payload tuple element is typed `number`, got {:?}",
        elements[0].ty
    );
    // The payload scope is the SFC (the signature was written in the SFC's own
    // defineEmits type argument).
    assert_eq!(
        change.payload_expr_scope.as_ref().map(|s| s.as_str()),
        Some(FILE),
        "the local call-sig payload scope is the SFC"
    );
}

// ---------------------------------------------------------------------------
// (3b) Cross-file emit interface — call signatures live in an imported file.
//      The stripped-payload scope must be the BASE file (so payload `Ref`s
//      resolve there), not the SFC owner.
//
//      Discriminating: scoping the payload to the SFC owner (the naive default)
//      would make this assertion fail — the payload's scope must follow the
//      signature's declaration-origin file.
// ---------------------------------------------------------------------------

const EMIT_BASE: &str = r#"export interface Events {
  (e: 'change', value: number): void;
}
"#;

const VUE_EMITS_IMPORTED: &str = r#"<script setup lang="ts">
import type { Events } from './events';
defineEmits<Events>();
</script>
"#;

#[test]
fn cross_file_emit_call_signature_payload_scope_is_base_file() {
    const BASE: &str = "/w/events.ts";
    const FILE: &str = "/w/ImportedEmits.vue";
    let host = make_host();
    upsert(&host, BASE, EMIT_BASE);
    upsert(&host, FILE, VUE_EMITS_IMPORTED);

    let request = props_request(&host, FILE, AnalyzedMacroKind::DefineEmits);
    let surface = host
        .resolve_vue_macro_surface(&request)
        .expect("imported emit interface resolves a surface");
    let emits =
        emits_from_typeinfo_surface(&*host, &resolved_vue_surface_for_test(surface.clone()));

    assert_eq!(
        emits.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
        vec!["change"],
        "the cross-file call-signature event name surfaces"
    );
    let change = &emits[0];
    // The stripped payload's scope follows the SIGNATURE's declaration-origin
    // file (the imported base), not the SFC owner.
    assert_eq!(
        change.payload_expr_scope.as_ref().map(|s| s.as_str()),
        Some(BASE),
        "the call-signature payload scope is the base file the signature was declared in"
    );
    // Negative: it is NOT the SFC owner.
    assert_ne!(
        change.payload_expr_scope.as_ref().map(|s| s.as_str()),
        Some(FILE),
        "the payload scope must not collapse to the SFC owner for an imported signature"
    );
}

// ---------------------------------------------------------------------------
// (3c) Emit call-signature `payload_type` (display-only `rawType`) renders the
//      STRIPPED payload TUPLE (`[label: T, ...]`) — the `emit('name', ...)` args
//      after the leading event-name parameter — mirroring the typed
//      `payload_expr`, for BOTH local and cross-file signatures.
//
//      Discriminating: a `payload_type` equal to the whole call-signature
//      source slice (`(e: 'change', value: number): void`) — the pre-fix
//      behavior — FAILS this; so does a `None`. The bracketed tuple
//      (`[value: number]`) is the only value that passes, and it is byte-
//      identical to what `render_type_expr_display(payload_expr)` would produce.
// ---------------------------------------------------------------------------

#[test]
fn emit_call_signature_payload_type_is_stripped_payload_tuple() {
    const LOCAL: &str = "/w/EmitsLocalSlice.vue";
    let host = make_host();
    upsert(&host, LOCAL, VUE_EMITS_CALLSIG);

    let local_emits = {
        let request = props_request(&host, LOCAL, AnalyzedMacroKind::DefineEmits);
        let surface = host.resolve_vue_macro_surface(&request).expect("surface");
        emits_from_typeinfo_surface(&*host, &resolved_vue_surface_for_test(surface.clone()))
    };
    let change = local_emits.iter().find(|e| e.name == "change").unwrap();
    // The display is the bracketed payload tuple (event-name param stripped),
    // NOT the whole call-signature source text.
    assert_eq!(
        change.payload_type.as_deref(),
        Some("[value: number]"),
        "the call-sig payload_type is the bracketed stripped-payload tuple"
    );
    // Negative: it is NOT the whole call-signature source slice.
    assert_ne!(
        change.payload_type.as_deref(),
        Some("(e: 'change', value: number): void"),
        "the payload_type must not be the whole call-signature source text"
    );
    let select = local_emits.iter().find(|e| e.name == "select").unwrap();
    assert_eq!(
        select.payload_type.as_deref(),
        Some("[id: string, extra: boolean]"),
        "each call-sig event's payload_type is its own stripped-payload tuple"
    );

    // Cross-file: the SAME stripped-tuple behavior for an imported signature.
    const BASE: &str = "/w/events.ts";
    const CROSS: &str = "/w/EmitsCrossSlice.vue";
    upsert(&host, BASE, EMIT_BASE);
    upsert(&host, CROSS, VUE_EMITS_IMPORTED);
    let cross_emits = {
        let request = props_request(&host, CROSS, AnalyzedMacroKind::DefineEmits);
        let surface = host.resolve_vue_macro_surface(&request).expect("surface");
        emits_from_typeinfo_surface(&*host, &resolved_vue_surface_for_test(surface.clone()))
    };
    let cross_change = cross_emits.iter().find(|e| e.name == "change").unwrap();
    assert_eq!(
        cross_change.payload_type.as_deref(),
        Some("[value: number]"),
        "the cross-file call-sig payload_type is the same bracketed stripped-payload tuple"
    );
    // The cross-file display is byte-identical to the local one (the typed
    // payload tuple renders identically regardless of declaration site) —
    // proving consistency, not a per-shape divergence.
    assert_eq!(
        cross_change.payload_type, change.payload_type,
        "local and cross-file call-sig payload_type render through the SAME tuple-display path"
    );
}

// ---------------------------------------------------------------------------
// (3d) defineEmits call-signature CARRIER event-name union — the node-domain
//      event-name behaviour: the event name is an ALIASED union (`type E =
//      'save' | 'cancel'; (e: E, value: number): void`). A shallow-carrier
//      decide on the materialized `first.ty` (a `Ref("E")` carrier) matches
//      neither the `Literal` nor the `Union` arm and surfaces NO events; the
//      node-domain `CallableNodeView::event_names` RESOLVES the `DeclRef(E)`
//      carrier to its `'save' | 'cancel'` union and surfaces BOTH names — the
//      decided-correct Vue semantics.
//
//      Discriminating (FAILS against a shallow-carrier decide, PASSES with the
//      node-domain reader): a shallow `match &first.ty` falls to `_ => {}` for
//      the `Ref("E")` carrier and produces an EMPTY emit set, so the
//      `["cancel", "save"]` assertion FAILS against it; the node-domain reader
//      surfaces both names. The payload (`[value: number]`) is unchanged — only
//      the event-name enumeration improves.
// ---------------------------------------------------------------------------

const VUE_EMITS_CARRIER_UNION: &str = r#"<script setup lang="ts">
type E = 'save' | 'cancel';
defineEmits<{
  (e: E, value: number): void;
}>();
</script>
"#;

#[test]
fn define_emits_callsig_carrier_event_name_union_resolves_both_names() {
    const FILE: &str = "/w/EmitsCarrierUnion.vue";
    let host = make_host();
    upsert(&host, FILE, VUE_EMITS_CARRIER_UNION);

    let request = props_request(&host, FILE, AnalyzedMacroKind::DefineEmits);
    let surface = host
        .resolve_vue_macro_surface(&request)
        .expect("defineEmits with an aliased event-name union resolves a surface");
    let emits =
        emits_from_typeinfo_surface(&*host, &resolved_vue_surface_for_test(surface.clone()));

    let mut names: Vec<&str> = emits.iter().map(|e| e.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(
        names,
        vec!["cancel", "save"],
        "the aliased event-name union `type E = 'save' | 'cancel'` resolves through the \
         node-domain event-name reader (the shallow-carrier decide surfaced NEITHER)"
    );

    // Each surfaced event carries the SAME stripped payload `[value: number]` —
    // the event-name param is stripped, the tail is the payload (unchanged by
    // the node-domain event-name resolution).
    for name in ["save", "cancel"] {
        let event = emits
            .iter()
            .find(|e| e.name == name)
            .unwrap_or_else(|| panic!("event `{name}` surfaces"));
        let verter_type_expr::TypeExpr::Tuple { elements, .. } = event
            .payload_expr
            .as_ref()
            .unwrap_or_else(|| panic!("event `{name}` carries a payload tuple"))
        else {
            panic!("event `{name}` payload must be a Tuple");
        };
        assert_eq!(
            elements.len(),
            1,
            "the leading event-name param is stripped, leaving the `[value: number]` payload"
        );
        assert!(
            matches!(
                elements[0].ty,
                verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number)
            ),
            "the surviving payload element is `number`, got {:?}",
            elements[0].ty
        );
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
    // Each binding carries a typed binding_expr (navigated through the shared
    // resolver, not a text/shape sniff). A builtin `Pick<RowApi, K>` over a LOCAL
    // closed interface materialises its picked members path-precisely to their
    // CONCRETE value types (the symbolic `NamedRoot['member']` carrier is the
    // shallow-source policy, exercised by the cross-file / package-backed Pick
    // routes elsewhere). The `pick_source_root_node` builtin + nominal-root
    // predicate leaves this builtin path exactly as before.
    assert!(
        row.bindings.iter().all(|b| b.binding_expr.is_some()),
        "each Pick binding carries its typed binding_expr"
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

// ---------------------------------------------------------------------------
// (5) defineModel normalizer — the synthesized model prop from analyzer facts.
//
//     Discriminating: a defineModel type arg is the model VALUE type (`string`),
//     which has NO object surface. The model prop name + type must come from
//     the analyzer-synthesized `prop_fields`, NOT the (empty) surface members.
// ---------------------------------------------------------------------------

const VUE_MODEL: &str = r#"<script setup lang="ts">
const model = defineModel<string>('title');
</script>
"#;

#[test]
fn define_model_normalizer_produces_synthesized_model_prop_from_analyzer_facts() {
    const FILE: &str = "/w/ModelComp.vue";
    let host = make_host();
    upsert(&host, FILE, VUE_MODEL);

    let request = props_request(&host, FILE, AnalyzedMacroKind::DefineModel);
    let surface = host
        .resolve_vue_macro_surface(&request)
        .expect("defineModel resolves an (empty-surface) VueMacroSurface");
    // The surface itself is EMPTY (defineModel's type arg is the model value
    // type, not a props object) — negative assertion.
    assert!(
        surface.surface.members.is_empty(),
        "defineModel macro surface carries no object members"
    );

    let props =
        props_from_typeinfo_surface(&*host, &resolved_vue_surface_for_test(surface.clone()));
    assert_eq!(props.len(), 1, "defineModel synthesizes exactly one prop");
    let model = &props[0];
    assert_eq!(
        model.name, "title",
        "the model prop is named after the model"
    );
    assert!(
        model.declared_in_macro_type_arg,
        "the model prop is declared at the macro site"
    );
    // Concrete typed form (not just `is_some()`): `defineModel<string>('title')`
    // synthesizes a model prop typed `string`.
    assert!(
        matches!(
            model.type_expr,
            Some(verter_type_expr::TypeExpr::Primitive(
                verter_type_expr::PrimitiveName::String
            ))
        ),
        "the model prop's type_expr is the `string` model value type, got {:?}",
        model.type_expr
    );
    // The re-anchored scope is the SFC owner, not the empty analyzer scope.
    assert_eq!(
        model.type_expr_scope.as_ref().map(|s| s.as_str()),
        Some(FILE),
        "the model prop's type_expr_scope is re-anchored to the SFC owner"
    );
}

// ---------------------------------------------------------------------------
// (6) withDefaults — the props surface comes from the inner defineProps type
//     arg; the `is_optional` on AnalyzedPropField stays the RAW type-arg
//     optionality (defaults flip `required` DOWNSTREAM at PropAnalysis, not on
//     AnalyzedPropField). This matches the eager surface_projector rail.
//
//     Discriminating: the props must be present (withDefaults resolves through
//     the inner type arg); `size` (which has a default) must keep is_optional
//     == its declared optionality, NOT be force-flipped here.
// ---------------------------------------------------------------------------

const VUE_WITH_DEFAULTS: &str = r#"<script setup lang="ts">
interface Props {
  size?: number;
  label: string;
}
withDefaults(defineProps<Props>(), { size: 10 });
</script>
"#;

#[test]
fn with_defaults_normalizer_uses_inner_props_surface_with_raw_optionality() {
    const FILE: &str = "/w/WithDefaults.vue";
    let host = make_host();
    upsert(&host, FILE, VUE_WITH_DEFAULTS);

    // `withDefaults` surfaces as both a WithDefaults macro AND the inner
    // DefineProps macro. The props normalizer resolves the props surface from
    // either props-contributing macro; assert through the DefineProps macro
    // (the inner one carries the type arg).
    let request = props_request(&host, FILE, AnalyzedMacroKind::DefineProps);
    let surface = host
        .resolve_vue_macro_surface(&request)
        .expect("withDefaults' inner defineProps resolves a surface");
    let props =
        props_from_typeinfo_surface(&*host, &resolved_vue_surface_for_test(surface.clone()));

    let mut names: Vec<&str> = props.iter().map(|p| p.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, vec!["label", "size"], "both props surface");

    let size = props.iter().find(|p| p.name == "size").unwrap();
    // `size?` is declared optional; defaults do NOT change AnalyzedPropField
    // optionality (that is a PropAnalysis-layer concern). The field keeps its
    // raw declared optionality — matching the eager rail.
    assert!(
        size.is_optional,
        "size keeps its declared `?` optionality at the AnalyzedPropField layer"
    );
    let label = props.iter().find(|p| p.name == "label").unwrap();
    assert!(!label.is_optional, "label is required (no `?`)");
}

// ---------------------------------------------------------------------------
// (6a) withDefaults outer-macro routing — the OUTER `withDefaults(...)` macro
//      carries no type argument; the props come from the SEPARATELY-routed
//      inner `defineProps<Props>` macro (matching the eager rail).
//
//      Discriminating: the analyzer emits BOTH a DefineProps macro (with the
//      type arg) and an outer WithDefaults macro (no type arg). The WithDefaults
//      macro must NOT resolve a surface (it has no type arg → `None`) and
//      `vue_macro_dtos` on it must yield an EMPTY props bundle (the inner
//      DefineProps is the props source — no double-count). Asserting the inner
//      DefineProps carries the props proves the routing.
// ---------------------------------------------------------------------------

#[test]
fn with_defaults_outer_macro_resolves_inner_define_props_surface() {
    const FILE: &str = "/w/WithDefaultsRouting.vue";
    let host = make_host();
    upsert(&host, FILE, VUE_WITH_DEFAULTS);

    // The analyzer emits a separate inner DefineProps macro AND an outer
    // WithDefaults macro.
    let with_defaults_index = macro_index_of(&host, FILE, AnalyzedMacroKind::WithDefaults);
    let define_props_index = macro_index_of(&host, FILE, AnalyzedMacroKind::DefineProps);
    assert_ne!(
        with_defaults_index, define_props_index,
        "withDefaults emits both an outer WithDefaults macro and an inner DefineProps macro"
    );

    // The OUTER WithDefaults macro carries no type argument → no macro surface.
    let outer_request = VueMacroSurfaceRequest {
        owner_canonical: Arc::from(FILE),
        macro_index: with_defaults_index,
        macro_kind: AnalyzedMacroKind::WithDefaults,
        root_identity: whole_hash(&host, FILE),
        level: TypeInfoQueryLevel::FullMetadata,
    };
    assert!(
        host.resolve_vue_macro_surface(&outer_request).is_none(),
        "the outer withDefaults macro has no type arg, so it resolves no surface (negative)"
    );
    // And its DTO bundle is empty — the props are NOT double-counted here.
    let outer_dtos = host.vue_macro_dtos(&outer_request);
    assert!(
        outer_dtos.prop_fields().is_empty(),
        "the outer withDefaults macro contributes no props (the inner DefineProps does)"
    );

    // The INNER DefineProps macro (routed separately) carries the props.
    let inner_request = props_request(&host, FILE, AnalyzedMacroKind::DefineProps);
    let inner_dtos = host.vue_macro_dtos(&inner_request);
    let mut names: Vec<&str> = inner_dtos
        .prop_fields()
        .iter()
        .map(|p| p.name.as_str())
        .collect();
    names.sort_unstable();
    assert_eq!(
        names,
        vec!["label", "size"],
        "the inner defineProps macro is the props source for a withDefaults component"
    );
}

// ---------------------------------------------------------------------------
// (7) Cross-file heritage props — `interface Props extends Base` where Base is
//     imported. The inherited members surface (own-body merge ran on the shared
//     surface), with own-body-vs-heritage provenance correct.
//
//     Discriminating: the inherited `baseFlag` must surface AND must be
//     `declared_in_macro_type_arg == false` (it arrived via heritage, not the
//     macro-T own body); the own-body `count` must be `true`.
// ---------------------------------------------------------------------------

const BASE_TYPES: &str = r#"export interface Base {
  baseFlag: boolean;
}
"#;

const VUE_HERITAGE: &str = r#"<script setup lang="ts">
import type { Base } from './base';
interface Props extends Base {
  count: number;
}
defineProps<Props>();
</script>
"#;

#[test]
fn cross_file_heritage_props_surface_with_own_body_vs_heritage_provenance() {
    const BASE: &str = "/w/base.ts";
    const FILE: &str = "/w/HeritageComp.vue";
    let host = make_host();
    upsert(&host, BASE, BASE_TYPES);
    upsert(&host, FILE, VUE_HERITAGE);

    let request = props_request(&host, FILE, AnalyzedMacroKind::DefineProps);
    let surface = host
        .resolve_vue_macro_surface(&request)
        .expect("cross-file heritage defineProps resolves a surface");
    let props =
        props_from_typeinfo_surface(&*host, &resolved_vue_surface_for_test(surface.clone()));

    let mut names: Vec<&str> = props.iter().map(|p| p.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(
        names,
        vec!["baseFlag", "count"],
        "the inherited Base member surfaces alongside the own-body member"
    );

    let count = props.iter().find(|p| p.name == "count").unwrap();
    assert!(
        count.declared_in_macro_type_arg,
        "count is in the macro type arg's own body"
    );
    let base_flag = props.iter().find(|p| p.name == "baseFlag").unwrap();
    assert!(
        !base_flag.declared_in_macro_type_arg,
        "baseFlag arrived via heritage — NOT declared in the macro type arg (negative)"
    );

    // The inherited member's declaration origin is the BASE file, not the SFC.
    let base_member = surface
        .surface
        .members
        .iter()
        .find(|m| m.name.as_ref() == "baseFlag")
        .expect("baseFlag on surface");
    assert_eq!(
        base_member
            .origin
            .canonical_file
            .as_ref()
            .map(|c| c.as_ref()),
        Some(BASE),
        "baseFlag's declaration origin is the heritage base file"
    );
}

// ---------------------------------------------------------------------------
// (7b) Generic inherited member — `type_expr_scope` follows the VALUE-NODE
//      scope (the deriving file), NOT the member's declaration_origin (the
//      base file). A `Base<T> { val: T }` instantiated `Base<Local>` has a
//      SUBSTITUTED value node (`Local`) scoped to the DERIVING file where
//      `Local` lives — so `Ref("Local")` must resolve THERE.
//
//      Discriminating: the eager rail (`imported_surface.rs::member_expr_scope`)
//      scopes the raised `*_expr` to `node_scope(member.value)`; scoping it to
//      the member's declaration_origin (the base file) — the pre-fix bug —
//      makes the `Ref("Local")` resolve in the base file (a cross-file Miss).
//      The test asserts the scope is the SFC and the scope's symbol table
//      genuinely resolves `Local`.
// ---------------------------------------------------------------------------

const GENERIC_BASE: &str = r#"export interface Base<T> {
  val: T;
}
"#;

const VUE_GENERIC_HERITAGE: &str = r#"<script setup lang="ts">
import type { Base } from './generic_base';
interface Local {
  tag: string;
}
interface Props extends Base<Local> {
  count: number;
}
defineProps<Props>();
</script>
"#;

#[test]
fn generic_inherited_member_type_expr_scope_is_deriving_file() {
    const BASE: &str = "/w/generic_base.ts";
    const FILE: &str = "/w/GenericHeritage.vue";
    let host = make_host();
    upsert(&host, BASE, GENERIC_BASE);
    upsert(&host, FILE, VUE_GENERIC_HERITAGE);

    let request = props_request(&host, FILE, AnalyzedMacroKind::DefineProps);
    let surface = host
        .resolve_vue_macro_surface(&request)
        .expect("generic heritage defineProps resolves a surface");
    let props =
        props_from_typeinfo_surface(&*host, &resolved_vue_surface_for_test(surface.clone()));

    let val = props
        .iter()
        .find(|p| p.name == "val")
        .expect("the inherited generic member `val` surfaces");

    // The substituted value type is `Local` — a `Ref` the deriving file owns.
    let type_expr = val.type_expr.as_ref().expect("val carries a typed form");
    assert!(
        matches!(
            type_expr,
            verter_type_expr::TypeExpr::Ref { name, .. } if name.as_ref() == "Local"
        ),
        "val's substituted value type is Ref(\"Local\"), got {type_expr:?}"
    );

    // The scope MUST be the DERIVING file (where `Local` is declared), NOT the
    // base file. Pre-fix (declaration_origin-first) this is the base file →
    // `Local` would not resolve there.
    assert_eq!(
        val.type_expr_scope.as_ref().map(|s| s.as_str()),
        Some(FILE),
        "val's type_expr_scope follows the value-node scope (the deriving SFC), not the base file"
    );
    assert_ne!(
        val.type_expr_scope.as_ref().map(|s| s.as_str()),
        Some(BASE),
        "the type_expr_scope must NOT be the heritage base file (negative — Local does not live there)"
    );

    // End-to-end: the scope genuinely resolves `Local` — `Ref("Local")` in the
    // SFC scope resolves to Local's one-level surface (`{ tag }`). Scoping to
    // the base file would Miss.
    let resolved = host
        .resolve_shallow_surface(FILE, "Local")
        .expect("Local resolves in the deriving SFC scope");
    let resolved_members: Vec<&str> = resolved.members.iter().map(|m| m.name.as_ref()).collect();
    assert_eq!(
        resolved_members,
        vec!["tag"],
        "Local resolves to its real surface in the scope val's type_expr is bound to"
    );
}

// ---------------------------------------------------------------------------
// Member-visibility publication leak guards (A: L1 / L2 / L3).
//
// `extract_class` RECORDS non-public class members on the shared surface (so B5
// can read the full set for native_props), so every PUBLISHED-member consumer
// must re-apply a Public-only filter at the publication boundary. These tests
// drive the typeinfo Vue adapter normalizers (`emits_from_typeinfo_surface`,
// `slots_from_typeinfo_surface`, `binding_fields_from_param_node`) over a class
// type argument carrying `private` / `protected` members and assert the
// non-public members do NOT leak into the published emit / slot / slot-binding
// surface.
//
// Discriminating: against the tree without the Public-only filters on those
// consumers, the `pub_field` / private / protected members appear in the
// published output and the `does-not-contain` assertions FAIL.
// ---------------------------------------------------------------------------

const VUE_EMITS_CLASS_LOCAL: &str = r#"<script setup lang="ts">
class EmitSurface {
  publicEvt: (n: number) => void;
  protected protectedEvt: (s: string) => void;
  private privateEvt: (b: boolean) => void;
}
defineEmits<EmitSurface>();
</script>
"#;

#[test]
fn define_emits_over_local_class_does_not_publish_non_public_members() {
    const FILE: &str = "/w/EmitsClassLocal.vue";
    let host = make_host();
    upsert(&host, FILE, VUE_EMITS_CLASS_LOCAL);

    let request = props_request(&host, FILE, AnalyzedMacroKind::DefineEmits);
    let surface = host
        .resolve_vue_macro_surface(&request)
        .expect("defineEmits<Class> resolves a surface");
    let emits =
        emits_from_typeinfo_surface(&*host, &resolved_vue_surface_for_test(surface.clone()));
    let names: Vec<&str> = emits.iter().map(|e| e.name.as_str()).collect();

    assert!(
        names.contains(&"publicEvt"),
        "the public class member must be published as an emit; got {names:?}",
    );
    assert!(
        !names.contains(&"protectedEvt"),
        "a protected class member must NOT leak as a published emit; got {names:?}",
    );
    assert!(
        !names.contains(&"privateEvt"),
        "a private class member must NOT leak as a published emit; got {names:?}",
    );
}

const VUE_EMITS_CLASS_IMPORTED_DECL: &str = r#"export class EmitSurface {
  publicEvt: (n: number) => void;
  protected protectedEvt: (s: string) => void;
  private privateEvt: (b: boolean) => void;
}
"#;

const VUE_EMITS_CLASS_IMPORTED_SFC: &str = r#"<script setup lang="ts">
import type { EmitSurface } from "./emit-surface";
defineEmits<EmitSurface>();
</script>
"#;

#[test]
fn define_emits_over_imported_class_does_not_publish_non_public_members() {
    const DECL: &str = "/w/emit-surface.ts";
    const FILE: &str = "/w/EmitsClassImported.vue";
    let host = make_host();
    upsert(&host, DECL, VUE_EMITS_CLASS_IMPORTED_DECL);
    upsert(&host, FILE, VUE_EMITS_CLASS_IMPORTED_SFC);

    let request = props_request(&host, FILE, AnalyzedMacroKind::DefineEmits);
    let surface = host
        .resolve_vue_macro_surface(&request)
        .expect("defineEmits<ImportedClass> resolves a surface");
    let emits =
        emits_from_typeinfo_surface(&*host, &resolved_vue_surface_for_test(surface.clone()));
    let names: Vec<&str> = emits.iter().map(|e| e.name.as_str()).collect();

    assert!(
        names.contains(&"publicEvt"),
        "the public imported-class member must be published as an emit; got {names:?}",
    );
    assert!(
        !names.contains(&"protectedEvt"),
        "a protected imported-class member must NOT leak as a published emit; got {names:?}",
    );
    assert!(
        !names.contains(&"privateEvt"),
        "a private imported-class member must NOT leak as a published emit; got {names:?}",
    );
}

const VUE_SLOTS_CLASS: &str = r#"<script setup lang="ts">
class SlotSurface {
  default: (props: { item: string }) => any;
  protected protectedSlot: (props: { x: number }) => any;
  private privateSlot: (props: { y: number }) => any;
}
defineSlots<SlotSurface>();
</script>
"#;

#[test]
fn define_slots_over_class_does_not_publish_non_public_members() {
    const FILE: &str = "/w/SlotsClass.vue";
    let host = make_host();
    upsert(&host, FILE, VUE_SLOTS_CLASS);

    let request = props_request(&host, FILE, AnalyzedMacroKind::DefineSlots);
    let surface = host
        .resolve_vue_macro_surface(&request)
        .expect("defineSlots<Class> resolves a surface");
    let slots =
        slots_from_typeinfo_surface(&*host, &resolved_vue_surface_for_test(surface.clone()));
    let names: Vec<&str> = slots.iter().map(|s| s.name.as_str()).collect();

    assert!(
        names.contains(&"default"),
        "the public class slot member must be published; got {names:?}",
    );
    assert!(
        !names.contains(&"protectedSlot"),
        "a protected class member must NOT leak as a published slot; got {names:?}",
    );
    assert!(
        !names.contains(&"privateSlot"),
        "a private class member must NOT leak as a published slot; got {names:?}",
    );
}

const VUE_SLOTS_CLASS_PARAM: &str = r#"<script setup lang="ts">
class SlotProps {
  publicBinding: string;
  protected protectedBinding: number;
  private privateBinding: boolean;
}
defineSlots<{ default(props: SlotProps): any }>();
</script>
"#;

#[test]
fn define_slots_navigated_class_param_does_not_publish_non_public_bindings() {
    const FILE: &str = "/w/SlotsClassParam.vue";
    let host = make_host();
    upsert(&host, FILE, VUE_SLOTS_CLASS_PARAM);

    let request = props_request(&host, FILE, AnalyzedMacroKind::DefineSlots);
    let surface = host
        .resolve_vue_macro_surface(&request)
        .expect("defineSlots resolves a surface");
    let slots =
        slots_from_typeinfo_surface(&*host, &resolved_vue_surface_for_test(surface.clone()));

    let default_slot = slots
        .iter()
        .find(|s| s.name == "default")
        .expect("the `default` slot must be published");
    let binding_names: Vec<&str> = default_slot
        .bindings
        .iter()
        .map(|b| b.name.as_str())
        .collect();

    assert!(
        binding_names.contains(&"publicBinding"),
        "the public class-param member must be published as a slot binding; got {binding_names:?}",
    );
    assert!(
        !binding_names.contains(&"protectedBinding"),
        "a protected class-param member must NOT leak as a slot binding; got {binding_names:?}",
    );
    assert!(
        !binding_names.contains(&"privateBinding"),
        "a private class-param member must NOT leak as a slot binding; got {binding_names:?}",
    );
}

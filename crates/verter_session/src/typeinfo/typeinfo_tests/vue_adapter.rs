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

use crate::typeinfo::adapters::vue::{
    emits_from_typeinfo_surface, props_from_typeinfo_surface, slots_from_typeinfo_surface,
};
use crate::typeinfo::types::{TypeInfoQueryLevel, VueMacroSurfaceRequest};
use crate::types::{FileKind, HostConfig, UpsertRequest};
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
        file_kind: FileKind::from_path(canonical_id),
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
    let props = props_from_typeinfo_surface(&host, &surface);

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
    let emits = emits_from_typeinfo_surface(&host, &surface);

    let mut names: Vec<&str> = emits.iter().map(|e| e.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(
        names,
        vec!["change", "select"],
        "event names come from the call-signature first param literal, NOT keyof"
    );

    // The payload function strips the leading event-name parameter: `change`
    // keeps `(value: number) => void` (1 param), `select` keeps
    // `(id: string, extra: boolean) => void` (2 params).
    let change = emits.iter().find(|e| e.name == "change").unwrap();
    let payload = change
        .payload_expr
        .as_ref()
        .expect("change payload_expr must be the stripped function");
    let verter_type_expr::TypeExpr::Function(func) = payload else {
        panic!("change payload must be a Function, got {payload:?}");
    };
    assert_eq!(
        func.parameters.len(),
        1,
        "leading event-name param must be stripped from the change payload"
    );
    // Negative: the first surviving parameter is NOT the event-name literal.
    assert!(
        !matches!(
            func.parameters[0].ty,
            verter_type_expr::TypeExpr::Literal(verter_type_expr::LiteralValue::String(_))
        ),
        "the stripped payload's first param must not be the event-name literal"
    );

    let select = emits.iter().find(|e| e.name == "select").unwrap();
    let verter_type_expr::TypeExpr::Function(select_func) =
        select.payload_expr.as_ref().expect("select payload_expr")
    else {
        panic!("select payload must be a Function");
    };
    assert_eq!(
        select_func.parameters.len(),
        2,
        "select keeps its two non-event params"
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
    let emits = emits_from_typeinfo_surface(&host, &surface);

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
    let emits = emits_from_typeinfo_surface(&host, &surface);

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

    // The call-sig payload is the stripped function `(value: number) => void` —
    // the leading event-name parameter is dropped, leaving one parameter typed
    // `number`.
    let change = &emits[0];
    let verter_type_expr::TypeExpr::Function(func) = change
        .payload_expr
        .as_ref()
        .expect("change payload_expr is the stripped function")
    else {
        panic!("change payload must be a Function");
    };
    assert_eq!(
        func.parameters.len(),
        1,
        "the leading event-name param is stripped, leaving the payload param"
    );
    assert!(
        matches!(
            func.parameters[0].ty,
            verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number)
        ),
        "the surviving payload param is typed `number`, got {:?}",
        func.parameters[0].ty
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
    let emits = emits_from_typeinfo_surface(&host, &surface);

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
// (3c) Emit call-signature `payload_type` (display-only `rawType`) is a
//      CONSISTENT source-span slice of the call signature — for BOTH local and
//      cross-file signatures.
//
//      Discriminating: pre-fix the call-sig `payload_type` was rendered via
//      `render_type_expr_display(&payload_fn)`, which returns `None` for a
//      function — so `payload_type` was `None`. Post-fix it is the trimmed
//      source slice of the signature span (e.g.
//      `(e: 'change', value: number): void`), and the SAME slice for the
//      cross-file case (sourced from the base file). Asserting `is_some()` +
//      the exact source text discriminates against both the pre-fix `None` and
//      a normalized (non-source) rendering.
// ---------------------------------------------------------------------------

#[test]
fn emit_call_signature_payload_type_is_consistent_source_slice() {
    const LOCAL: &str = "/w/EmitsLocalSlice.vue";
    let host = make_host();
    upsert(&host, LOCAL, VUE_EMITS_CALLSIG);

    let local_emits = {
        let request = props_request(&host, LOCAL, AnalyzedMacroKind::DefineEmits);
        let surface = host.resolve_vue_macro_surface(&request).expect("surface");
        emits_from_typeinfo_surface(&host, &surface)
    };
    let change = local_emits.iter().find(|e| e.name == "change").unwrap();
    // The display slice is the call signature's source text (trimmed of the
    // trailing `;`). It is SOME (pre-fix it was None) and is the exact source.
    assert_eq!(
        change.payload_type.as_deref(),
        Some("(e: 'change', value: number): void"),
        "the call-sig payload_type is the trimmed source slice of the signature"
    );
    let select = local_emits.iter().find(|e| e.name == "select").unwrap();
    assert_eq!(
        select.payload_type.as_deref(),
        Some("(e: 'select', id: string, extra: boolean): void"),
        "each call-sig event's payload_type is its own signature's source slice"
    );

    // Cross-file: the SAME consistent source-slice behavior, sourced from the
    // base file the signature was declared in.
    const BASE: &str = "/w/events.ts";
    const CROSS: &str = "/w/EmitsCrossSlice.vue";
    upsert(&host, BASE, EMIT_BASE);
    upsert(&host, CROSS, VUE_EMITS_IMPORTED);
    let cross_emits = {
        let request = props_request(&host, CROSS, AnalyzedMacroKind::DefineEmits);
        let surface = host.resolve_vue_macro_surface(&request).expect("surface");
        emits_from_typeinfo_surface(&host, &surface)
    };
    let cross_change = cross_emits.iter().find(|e| e.name == "change").unwrap();
    assert_eq!(
        cross_change.payload_type.as_deref(),
        Some("(e: 'change', value: number): void"),
        "the cross-file call-sig payload_type is a source slice from the base file (consistent)"
    );
    // The cross-file display is byte-identical to the local one (the same
    // signature text was written in both fixtures) — proving consistency, not a
    // per-shape divergence.
    assert_eq!(
        cross_change.payload_type, change.payload_type,
        "local and cross-file call-sig payload_type render through the SAME source-slice path"
    );
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
    let slots = slots_from_typeinfo_surface(&host, &surface);

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
    let slots = slots_from_typeinfo_surface(&host, &surface);

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
// (4b) defineSlots normalizer — a `Pick<T,'k'>` first parameter yields the
//      picked bindings (matching the eager local-SFC rail). The new path must
//      navigate the first-param type through the SHARED resolver to its object
//      surface, NOT only accept a literal `TypeExpr::Object` (the pre-fix bug
//      dropped Pick bindings entirely).
//
//      Discriminating: pre-fix `binding_fields_from_param_ty` matches only
//      `TypeExpr::Object`, so a `Pick<RowApi, 'name'|'value'>` first param
//      yields ZERO bindings. Post-fix the bindings are `name` + `value`.
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
    let slots = slots_from_typeinfo_surface(&host, &surface);

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
    // resolver, not a text/shape sniff).
    assert!(
        row.bindings.iter().all(|b| b.binding_expr.is_some()),
        "each Pick binding carries its typed binding_expr"
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

    let props = props_from_typeinfo_surface(&host, &surface);
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
    let props = props_from_typeinfo_surface(&host, &surface);

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
        outer_dtos.props.is_empty(),
        "the outer withDefaults macro contributes no props (the inner DefineProps does)"
    );

    // The INNER DefineProps macro (routed separately) carries the props.
    let inner_request = props_request(&host, FILE, AnalyzedMacroKind::DefineProps);
    let inner_dtos = host.vue_macro_dtos(&inner_request);
    let mut names: Vec<&str> = inner_dtos.props.iter().map(|p| p.name.as_str()).collect();
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
    let props = props_from_typeinfo_surface(&host, &surface);

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
    let props = props_from_typeinfo_surface(&host, &surface);

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

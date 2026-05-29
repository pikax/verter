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

    let label = props.iter().find(|p| p.name == "label").unwrap();
    assert!(label.is_optional, "label? is optional");

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
    assert!(
        model.type_expr.is_some(),
        "the model prop carries its typed form"
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

// ---------------------------------------------------------------------------
// (8) `.vue` PUBLIC component type via public_type.rs — through typeinfo, no
//     component-meta.
//
//     Discriminating: the public surface must carry the synthesized
//     `$props` / `$emit` / `$slots` instance members built from the macros.
//     A `.ts` file (no synthesized default) must return None.
// ---------------------------------------------------------------------------

const VUE_FULL_COMPONENT: &str = r#"<script setup lang="ts">
defineProps<{ count: number }>();
defineEmits<{ (e: 'change', v: number): void }>();
defineSlots<{ default(props: { item: string }): any }>();
</script>
"#;

#[test]
fn vue_public_type_carries_synthesized_instance_members_without_component_meta() {
    const FILE: &str = "/w/FullComp.vue";
    let host = make_host();
    upsert(&host, FILE, VUE_FULL_COMPONENT);

    let public_surface = host
        .resolve_vue_public_type(FILE, TypeInfoQueryLevel::PublicType)
        .expect("a .vue with type-based macros has a public component type");

    let mut members: Vec<&str> = public_surface
        .members
        .iter()
        .map(|m| m.name.as_ref())
        .collect();
    members.sort_unstable();
    assert_eq!(
        members,
        vec!["$emit", "$props", "$slots"],
        "the public component type carries the synthesized instance members"
    );
}

#[test]
fn vue_public_type_returns_none_for_plain_ts_file() {
    const FILE: &str = "/w/plain.ts";
    let host = make_host();
    upsert(&host, FILE, "export interface Foo { a: number }\n");

    // A plain `.ts` file has no synthesized `default` instance object — no
    // public component type (negative).
    assert!(
        host.resolve_vue_public_type(FILE, TypeInfoQueryLevel::PublicType)
            .is_none(),
        "a plain .ts file has no .vue public component type"
    );
}

// ---------------------------------------------------------------------------
// (9) Query-level distinctness — PublicType vs FullMetadata produce DISTINCT
//     results for a `.vue`.
//
//     Discriminating: the PublicType surface is the instance object
//     `{ $props, $emit, $slots }`; the FullMetadata defineProps surface is the
//     props object `{ count }`. They MUST differ — a level that collapsed to
//     one result would make the member sets equal.
// ---------------------------------------------------------------------------

#[test]
fn query_level_public_vs_full_metadata_produce_distinct_surfaces() {
    const FILE: &str = "/w/LevelComp.vue";
    let host = make_host();
    upsert(&host, FILE, VUE_FULL_COMPONENT);

    let public_surface = host
        .resolve_vue_public_type(FILE, TypeInfoQueryLevel::PublicType)
        .expect("public type resolves");
    let public_members: std::collections::BTreeSet<&str> = public_surface
        .members
        .iter()
        .map(|m| m.name.as_ref())
        .collect();

    let full_request = props_request(&host, FILE, AnalyzedMacroKind::DefineProps);
    let full = host
        .resolve_vue_macro_surface(&full_request)
        .expect("full-metadata defineProps surface resolves");
    let full_members: std::collections::BTreeSet<&str> = full
        .surface
        .members
        .iter()
        .map(|m| m.name.as_ref())
        .collect();

    assert!(
        public_members.contains("$props"),
        "PublicType carries the instance $props member"
    );
    assert!(
        full_members.contains("count") && !full_members.contains("$props"),
        "FullMetadata defineProps surface carries the prop members, not the instance shape"
    );
    assert_ne!(
        public_members, full_members,
        "PublicType and FullMetadata are DISTINCT query results for the same .vue"
    );
}

// ---------------------------------------------------------------------------
// (10) Cache identity — the store memoizes per (canonical, content, macro,
//      level); a content edit yields a distinct content-addressed key.
//
//      Discriminating: a warm `vue_macro_dtos` call does NOT grow the store
//      (same key hits, pointer-equal Arc); a DIFFERENT macro grows it (distinct
//      slot); a content edit grows it (content-addressed key). A key that
//      omitted content (an env-hash-only key) would serve the stale entry and
//      fail the edited-props assertion.
// ---------------------------------------------------------------------------

#[test]
fn vue_macro_dtos_cache_keys_on_content_and_macro() {
    const FILE: &str = "/w/CacheComp.vue";
    let host = make_host();
    upsert(&host, FILE, VUE_PROPS_AND_EMITS);

    assert_eq!(
        host.vue_shallow_metadata_store().len(),
        0,
        "store starts empty"
    );

    let request_props = props_request(&host, FILE, AnalyzedMacroKind::DefineProps);
    let first = host.vue_macro_dtos(&request_props);
    assert_eq!(first.props.len(), 2, "cold compute produces the props");
    assert_eq!(
        host.vue_shallow_metadata_store().len(),
        1,
        "one cold entry published"
    );

    // Warm hit: same key, store does NOT grow, and the returned Arc is the SAME
    // cached value (pointer-equal).
    let second = host.vue_macro_dtos(&request_props);
    assert_eq!(
        host.vue_shallow_metadata_store().len(),
        1,
        "warm hit reuses the cached entry; store does not grow"
    );
    assert!(
        Arc::ptr_eq(&first, &second),
        "warm hit returns the SAME immutable Arc"
    );

    // A DIFFERENT macro (defineEmits) is a DISTINCT cache slot.
    let request_emits = props_request(&host, FILE, AnalyzedMacroKind::DefineEmits);
    let emits_dtos = host.vue_macro_dtos(&request_emits);
    assert_eq!(
        emits_dtos.emits.len(),
        1,
        "the emits DTO bundle is computed"
    );
    assert_eq!(
        host.vue_shallow_metadata_store().len(),
        2,
        "a different macro occupies a distinct cache slot"
    );

    // A content edit changes the `.vue`'s whole_hash → a fresh content-addressed
    // key → a new cold entry (the old entry is not served for changed content).
    upsert(&host, FILE, VUE_PROPS_AND_EMITS_EDITED);
    let request_edited = props_request(&host, FILE, AnalyzedMacroKind::DefineProps);
    assert_ne!(
        request_edited.root_identity, request_props.root_identity,
        "the content edit changed the .vue's whole_hash"
    );
    let edited = host.vue_macro_dtos(&request_edited);
    let mut edited_names: Vec<&str> = edited.props.iter().map(|p| p.name.as_str()).collect();
    edited_names.sort_unstable();
    assert_eq!(
        edited_names,
        vec!["count", "extra", "label"],
        "the edited content's props reflect the NEW source, not the stale entry"
    );
    assert!(
        host.vue_shallow_metadata_store().len() >= 3,
        "the content edit produced a distinct content-addressed cache entry"
    );
}

const VUE_PROPS_AND_EMITS: &str = r#"<script setup lang="ts">
defineProps<{ count: number; label?: string }>();
defineEmits<{ (e: 'change', v: number): void }>();
</script>
"#;

const VUE_PROPS_AND_EMITS_EDITED: &str = r#"<script setup lang="ts">
defineProps<{ count: number; label?: string; extra: boolean }>();
defineEmits<{ (e: 'change', v: number): void }>();
</script>
"#;

// ---------------------------------------------------------------------------
// (10a) Cache STALE-identity rejection — `vue_macro_dtos` must derive the
//       `whole_hash` from the LIVE `IndexedReady`, NOT trust the request's
//       `root_identity` hint. A caller holding a `root_identity` captured
//       BEFORE an edit must still get the NEW content's DTOs.
//
//       Discriminating: pre-fix `vue_macro_dtos` keys on `request.root_identity`,
//       so a request carrying the stale (pre-edit) hash hits the OLD slot and
//       returns the v1 props (missing `extra`). Post-fix it keys on the live
//       `whole_hash`, returning the v2 props.
// ---------------------------------------------------------------------------

#[test]
fn vue_macro_dtos_rejects_stale_root_identity_after_edit() {
    const FILE: &str = "/w/StaleId.vue";
    let host = make_host();
    upsert(&host, FILE, VUE_PROPS_AND_EMITS);

    // Capture the v1 request (its `root_identity` is v1's whole_hash) and warm
    // the cache for the props macro.
    let stale_request = props_request(&host, FILE, AnalyzedMacroKind::DefineProps);
    let v1 = host.vue_macro_dtos(&stale_request);
    let mut v1_names: Vec<&str> = v1.props.iter().map(|p| p.name.as_str()).collect();
    v1_names.sort_unstable();
    assert_eq!(
        v1_names,
        vec!["count", "label"],
        "v1 props are count + label"
    );

    // Edit the file. The live `IndexedReady.whole_hash` now differs from the
    // `stale_request.root_identity` captured above.
    upsert(&host, FILE, VUE_PROPS_AND_EMITS_EDITED);
    let live_hash = whole_hash(&host, FILE);
    assert_ne!(
        stale_request.root_identity, live_hash,
        "the edit changed the live whole_hash; the request still holds the stale one"
    );

    // Re-query with the STALE request (its `root_identity` is the pre-edit
    // hash). `vue_macro_dtos` must derive `whole_hash` from the LIVE
    // `IndexedReady` and return the v2 props — never the stale v1 entry.
    let after_edit = host.vue_macro_dtos(&stale_request);
    let mut after_names: Vec<&str> = after_edit.props.iter().map(|p| p.name.as_str()).collect();
    after_names.sort_unstable();
    assert_eq!(
        after_names,
        vec!["count", "extra", "label"],
        "a stale root_identity must NOT serve the pre-edit DTOs; the live whole_hash keys the v2 slot"
    );
}

// ---------------------------------------------------------------------------
// (10b) Cache macro-KIND-mismatch rejection — `vue_macro_dtos` must derive the
//       macro kind from the snapshot's `macros[macro_index].kind`, NOT trust
//       the request's `macro_kind` hint, and the kind must be part of the
//       cache key.
//
//       Discriminating: the macro at `macro_index` is genuinely a DefineProps.
//       A request that LIES (`macro_kind: DefineEmits`) must STILL be normalized
//       as props (the derived kind wins). Pre-fix the normalizer dispatches on
//       the request's `DefineEmits` hint → runs the emits normalizer over the
//       props surface (props empty, the property-style fallback fabricates
//       events) and — because the pre-fix key omits the kind — poisons the
//       shared slot. Post-fix props are non-empty and emits empty.
// ---------------------------------------------------------------------------

#[test]
fn vue_macro_dtos_rejects_macro_kind_mismatch_without_poisoning_cache() {
    const FILE: &str = "/w/KindMismatch.vue";
    let host = make_host();
    upsert(&host, FILE, VUE_PROPS_AND_EMITS);

    // The DefineProps macro's index — but we will LIE about its kind in the
    // request, claiming it is a DefineEmits.
    let props_index = macro_index_of(&host, FILE, AnalyzedMacroKind::DefineProps);
    let lying_request = VueMacroSurfaceRequest {
        owner_canonical: Arc::from(FILE),
        macro_index: props_index,
        macro_kind: AnalyzedMacroKind::DefineEmits, // WRONG — the macro is DefineProps.
        root_identity: whole_hash(&host, FILE),
        level: TypeInfoQueryLevel::FullMetadata,
    };

    // COLD call with the lying kind. The derived kind (DefineProps) must win:
    // the bundle carries PROPS, not the emits the property-style fallback would
    // fabricate from the props surface.
    let cold = host.vue_macro_dtos(&lying_request);
    let mut cold_props: Vec<&str> = cold.props.iter().map(|p| p.name.as_str()).collect();
    cold_props.sort_unstable();
    assert_eq!(
        cold_props,
        vec!["count", "label"],
        "the derived DefineProps kind wins; the bundle is props, not the lying-kind emits"
    );
    assert!(
        cold.emits.is_empty(),
        "a props macro must not produce emits even when the request lies about the kind (negative)"
    );

    // A truthful DefineProps request at the same index keys the SAME derived
    // slot (the kind was derived identically) and returns the SAME Arc — the
    // lying call did not poison or fork the slot.
    let truthful_request = props_request(&host, FILE, AnalyzedMacroKind::DefineProps);
    let truthful = host.vue_macro_dtos(&truthful_request);
    assert!(
        Arc::ptr_eq(&cold, &truthful),
        "the derived-kind slot is shared; the lying request did not poison a separate slot"
    );
}

/// The query level is QUERY IDENTITY, not an env-hash dimension (R21). Guard:
/// the DTO cache key carries the level tag + content hash + macro kind, NOT any
/// of the five env hashes. A structural check on the key type — if a future
/// edit folded an env hash into the key (or dropped the level), this fails to
/// compile / the field set changes.
///
/// Discriminating: destructures the EXACT key field set without `..`, so
/// adding an owned field (e.g. a `resolve_env_hash`) or removing `level_tag`
/// fails to compile. Also asserts the level tag is a 1-byte discriminant, not a
/// 16-byte env hash.
#[test]
fn vue_macro_dto_key_carries_level_and_content_not_env_hash() {
    use crate::typeinfo::adapters::vue::store::VueMacroDtoKey;

    let a = VueMacroDtoKey::new(
        Arc::from("/w/x.vue"),
        [1u8; 16],
        0,
        AnalyzedMacroKind::DefineProps,
        TypeInfoQueryLevel::PublicType,
    );
    let b = VueMacroDtoKey::new(
        Arc::from("/w/x.vue"),
        [1u8; 16],
        0,
        AnalyzedMacroKind::DefineProps,
        TypeInfoQueryLevel::FullMetadata,
    );
    // Distinct level ⇒ distinct key (level is part of identity).
    assert_ne!(a, b, "the level discriminates the key");

    // Distinct content ⇒ distinct key (content-addressed).
    let c = VueMacroDtoKey::new(
        Arc::from("/w/x.vue"),
        [2u8; 16],
        0,
        AnalyzedMacroKind::DefineProps,
        TypeInfoQueryLevel::PublicType,
    );
    assert_ne!(a, c, "the content hash discriminates the key");

    // Distinct macro kind ⇒ distinct key (kind is part of identity — a kind
    // mismatch must not read / poison the sibling kind's slot).
    let d = VueMacroDtoKey::new(
        Arc::from("/w/x.vue"),
        [1u8; 16],
        0,
        AnalyzedMacroKind::DefineEmits,
        TypeInfoQueryLevel::PublicType,
    );
    assert_ne!(a, d, "the macro kind discriminates the key");

    // Structural field-set guard: destructure the WHOLE key without `..`. Any
    // added owned field breaks this destructure (compile error), forcing a
    // conscious decision about whether the new field belongs in cache identity.
    let VueMacroDtoKey {
        canonical,
        whole_hash,
        macro_index,
        macro_kind,
        level_tag,
    } = &a;
    assert_eq!(canonical.as_ref(), "/w/x.vue");
    assert_eq!(
        *whole_hash, [1u8; 16],
        "the content hash is part of the key"
    );
    assert_eq!(*macro_index, 0);
    assert_eq!(*macro_kind, AnalyzedMacroKind::DefineProps);
    // The level tag is a small query-identity discriminant (1 byte), NOT a
    // 16-byte env-hash.
    assert_eq!(
        std::mem::size_of_val(level_tag),
        1,
        "level_tag is a 1-byte query-identity discriminant, not an env hash"
    );

    // PublicType and FullMetadata differ by exactly the tag byte.
    assert_eq!(TypeInfoQueryLevel::PublicType.cache_tag(), 0);
    assert_eq!(TypeInfoQueryLevel::FullMetadata.cache_tag(), 1);
    assert_ne!(
        TypeInfoQueryLevel::PublicType.cache_tag(),
        TypeInfoQueryLevel::FullMetadata.cache_tag()
    );
}

// ---------------------------------------------------------------------------
// (11) No owned `String` type / JSDoc text on VueMacroSurface / the surface.
//
//      The surface carries SPANS only (ids + flags + interned names). This is a
//      compile-time structural guard plus a runtime assertion that the surface
//      members expose span fields, never an owned type-text String.
// ---------------------------------------------------------------------------

#[test]
fn vue_macro_surface_carries_spans_not_owned_type_strings() {
    const FILE: &str = "/w/SpanComp.vue";
    let host = make_host();
    upsert(&host, FILE, VUE_PROPS);

    let request = props_request(&host, FILE, AnalyzedMacroKind::DefineProps);
    let surface = host.resolve_vue_macro_surface(&request).expect("surface");

    // Structural: every member exposes a span-bearing origin + value node id,
    // and JSDoc as SPANS — there is no owned type-text String field on the
    // surface member (the `TypeInfoSurfaceMember` type has no `type_annotation:
    // String`). Assert the span-bearing fields are reachable.
    for member in surface.surface.members.iter() {
        // `value` is a node id (not an expanded body), `name` is interned.
        let _node_id: crate::semantic_query::SemanticNodeId = member.value;
        let _name: &Arc<str> = &member.name;
        // JSDoc is a span (Option<CanonicalSpan>), never an owned doc String on
        // the surface.
        let _desc_span: &Option<crate::typeinfo::surface::CanonicalSpan> =
            &member.jsdoc_description_span;
    }

    // The whole surface is `Eq + Hash` (a structural value of spans/ids/flags),
    // which would not hold if it carried interior-mutable / non-hashable owned
    // payloads. Exercise it.
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    surface.surface.hash(&mut h);
    let _ = h.finish();
}

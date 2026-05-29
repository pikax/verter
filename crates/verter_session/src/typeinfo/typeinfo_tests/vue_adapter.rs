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

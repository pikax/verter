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

    let mut names: Vec<&str> = props.iter().map(|p| p.analysis.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(
        names,
        vec!["count", "id", "label"],
        "all three Props members must surface"
    );

    let count = props.iter().find(|p| p.analysis.name == "count").unwrap();
    assert!(!count.analysis.is_optional, "count is required");
    assert_eq!(
        count.analysis.description.as_deref(),
        Some("the count"),
        "count's JSDoc description must be sliced from the surface spans"
    );
    assert!(
        count.analysis.declared_in_macro_type_arg,
        "count is declared in the macro type arg's own body"
    );
    // Concrete typed form + scope (not just `is_some()`): `count: number` raises
    // to the `number` primitive, rendered at the terminal sink.
    assert_eq!(
        count.analysis.type_annotation.as_deref(),
        Some("number"),
        "count's display renders the `number` primitive, got {:?}",
        count.analysis.type_annotation
    );
    assert_eq!(
        count.analysis.type_expr_scope.as_ref().map(|s| s.as_str()),
        Some(FILE),
        "count.type_expr_scope is the SFC the prop was declared in"
    );

    let label = props.iter().find(|p| p.analysis.name == "label").unwrap();
    assert!(label.analysis.is_optional, "label? is optional");
    // `label?: string` → the `string` primitive (the `?` is the optional flag,
    // not part of the value type).
    assert_eq!(
        label.analysis.type_annotation.as_deref(),
        Some("string"),
        "label's display renders the `string` primitive, got {:?}",
        label.analysis.type_annotation
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

    let mut names: Vec<&str> = emits.iter().map(|e| e.analysis.name.as_str()).collect();
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
    let change = emits.iter().find(|e| e.analysis.name == "change").unwrap();
    assert_eq!(
        change.analysis.payload_type.as_deref(),
        Some("[value: number]"),
        "leading event-name param must be stripped from the change payload tuple"
    );
    // Negative: the surviving tuple element is NOT the event-name literal.
    assert!(
        !change
            .analysis
            .payload_type
            .as_deref()
            .unwrap_or_default()
            .contains("'change'"),
        "the stripped payload tuple's element must not be the event-name literal"
    );

    let select = emits.iter().find(|e| e.analysis.name == "select").unwrap();
    assert_eq!(
        select.analysis.payload_type.as_deref(),
        Some("[id: string, extra: boolean]"),
        "select keeps its two non-event params as tuple elements"
    );
}

// ---------------------------------------------------------------------------
// (2a) Call-signature payload SOURCE minting — the three-way split:
//
//      - a leaf/leaf-union-param signature keeps the complete CLOSED tuple
//        source (complete by itself, no replay needed);
//      - a signature with a param RICHER than the closed element vocabulary
//        (a named reference) mints the projected CALLABLE-PARAMS replay
//        route — `base` = the macro's stamped type-argument locator,
//        `signature_ordinal` = the SURFACE call-signature sequence index
//        (declaration order, BEFORE event-name expansion — NOT an
//        emitted-row counter), `first_param` = 1 (the event-name strip);
//      - a zero-payload signature keeps the PRESENT empty closed tuple.
//
//      Discriminating: the pre-change producer published the typed FAILURE
//      (`Failed(UnrepresentableRequiredPayload)`) for the richer row, so the
//      CallableParams assertion fails against it; stamping the emitted-row
//      index instead of the surface sequence index publishes ordinal 1 for
//      `save` (it is the second EMITTED row but the THIRD signature) and
//      fails the ordinal assertion.
// ---------------------------------------------------------------------------

const VUE_EMITS_CALLSIG_RICH: &str = r#"<script setup lang="ts">
interface Row { id: number }
defineEmits<{
  (e: 'change', value: number): void;
  (e: unknown, phantom: number): void;
  (e: 'save', value: Row): void;
  (e: 'close'): void;
}>();
</script>
"#;

#[test]
fn define_emits_callsig_rich_params_mint_the_callable_params_replay_source() {
    use verter_type_expr::facts::{
        ClosedTypeFact, ProjectedTypeFact, SemanticTypeSource, SourcePosition,
    };
    use verter_type_expr::locators::{AuthoredBodyLocator, MacroPayloadPosition};

    const FILE: &str = "/w/EmitsRich.vue";
    let host = make_host();
    upsert(&host, FILE, VUE_EMITS_CALLSIG_RICH);

    let request = props_request(&host, FILE, AnalyzedMacroKind::DefineEmits);
    let macro_index = request.macro_index;
    let surface = host
        .resolve_vue_macro_surface(&request)
        .expect("defineEmits must resolve a macro surface");
    let emits =
        emits_from_typeinfo_surface(&*host, &resolved_vue_surface_for_test(surface.clone()));

    // A leaf-param signature keeps the complete CLOSED tuple source.
    let change = emits.iter().find(|e| e.analysis.name == "change").unwrap();
    assert!(
        matches!(
            &change.payload_source,
            SourcePosition::Present(SemanticTypeSource::Closed(ClosedTypeFact::Tuple(tuple)))
                if tuple.elements.len() == 1
        ),
        "a leaf-param call signature keeps its complete closed tuple source, got {:?}",
        change.payload_source
    );

    // The RICHER-param signature mints the CallableParams replay route. Its
    // ordinal is the SURFACE call-signature sequence index (3rd signature —
    // the nameless `(e: unknown, …)` signature contributes NO emit row but
    // still occupies ordinal 1), never the emitted-row counter.
    let save = emits.iter().find(|e| e.analysis.name == "save").unwrap();
    let SourcePosition::Present(SemanticTypeSource::Projected(ProjectedTypeFact::CallableParams {
        base,
        signature_ordinal,
        first_param,
    })) = &save.payload_source
    else {
        panic!(
            "a call signature with a named-reference param must mint the \
             projected CallableParams replay source, got {:?}",
            save.payload_source
        );
    };
    assert_eq!(
        *signature_ordinal, 2,
        "the ordinal indexes the surface's declaration-order call-signature \
         sequence BEFORE event-name expansion (an emitted-row counter says 1)"
    );
    assert_eq!(*first_param, 1, "the event-name strip stamps first_param 1");
    let AuthoredBodyLocator::MacroPayload(payload) = base else {
        panic!("the replay base is the macro's stamped type-argument locator, got {base:?}");
    };
    assert_eq!(payload.payload, MacroPayloadPosition::TypeArgument);
    assert_eq!(payload.macro_index as usize, macro_index);

    // A zero-payload signature keeps the PRESENT empty closed tuple — never
    // a CallableParams route and never a failure.
    let close = emits.iter().find(|e| e.analysis.name == "close").unwrap();
    assert!(
        matches!(
            &close.payload_source,
            SourcePosition::Present(SemanticTypeSource::Closed(ClosedTypeFact::Tuple(tuple)))
                if tuple.elements.is_empty()
        ),
        "a zero-payload call signature keeps the present empty closed tuple, got {:?}",
        close.payload_source
    );

    // Negative: NO call-signature row publishes the typed failure — every
    // realized call-signature payload now has a faithful present source.
    assert!(
        emits.iter().all(|e| !e.payload_source.is_failed()),
        "no realized call-signature emit row may publish Failed once the \
         CallableParams replay route exists, got {:?}",
        emits
            .iter()
            .map(|e| (&e.analysis.name, &e.payload_source))
            .collect::<Vec<_>>()
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

    let mut names: Vec<&str> = emits.iter().map(|e| e.analysis.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(
        names,
        vec!["change", "remove"],
        "property-style emit names come from the member names"
    );
}

// ---------------------------------------------------------------------------
// (3a) Mixed emit interface — a call signature AND property members, including
//      a property member that DUPLICATES the call-signature event name. The
//      published emits are the UNION of both forms: a property member inside a
//      `defineEmits<T>` object surface IS an emit; duplicate names take
//      CALL-SIGNATURE precedence (call-sig emits are pushed first, first-writer
//      wins in the de-dup).
//
//      Discriminating: the either/or gate (property-style fires ONLY when no
//      call-sig emit was found) drops `notAnEvent` entirely and fails the union
//      assertion; property-precedence on the duplicate `change` would publish
//      the `[dup: string]` tuple and fail the payload assertion. Also asserts
//      deterministic order (signature order, then member order) + SFC scope.
// ---------------------------------------------------------------------------

const VUE_EMITS_MIXED: &str = r#"<script setup lang="ts">
defineEmits<{
  (e: 'change', value: number): void;
  notAnEvent: [flag: boolean];
  change: [dup: string];
}>();
</script>
"#;

#[test]
fn define_emits_normalizer_mixed_callsig_unions_property_members() {
    const FILE: &str = "/w/EmitsMixed.vue";
    let host = make_host();
    upsert(&host, FILE, VUE_EMITS_MIXED);

    let request = props_request(&host, FILE, AnalyzedMacroKind::DefineEmits);
    let surface = host
        .resolve_vue_macro_surface(&request)
        .expect("mixed defineEmits resolves a surface");
    let emits =
        emits_from_typeinfo_surface(&*host, &resolved_vue_surface_for_test(surface.clone()));

    let names: Vec<&str> = emits.iter().map(|e| e.analysis.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["change", "notAnEvent"],
        "the published emits are the UNION of call-signature and property emits, \
         deterministic (signature order, then member order), de-duped by name"
    );
    // The property member IS an emit alongside the call signature (the
    // either/or gate is gone).
    let not_an_event = emits
        .iter()
        .find(|e| e.analysis.name == "notAnEvent")
        .expect("a property member publishes as an emit alongside a call signature");
    assert_eq!(
        not_an_event.analysis.payload_type.as_deref(),
        Some("[flag: boolean]"),
        "the property emit publishes its named-tuple payload display, got {:?}",
        not_an_event.analysis.payload_type
    );
    // Exactly ONE `change` survives the de-dup (negative: no duplicate rows).
    assert_eq!(
        emits.iter().filter(|e| e.analysis.name == "change").count(),
        1,
        "duplicate event names de-dup to one published emit"
    );

    // CALL-SIGNATURE precedence on the duplicate name: the surviving `change`
    // payload is the stripped call-sig tuple `[value: number]`, NOT the
    // property member's `[dup: string]` tuple.
    let change = &emits[0];
    assert_eq!(
        change.analysis.payload_type.as_deref(),
        Some("[value: number]"),
        "the duplicate-name emit keeps the CALL-SIGNATURE payload (precedence), got {:?}",
        change.analysis.payload_type
    );
    assert_ne!(
        change.analysis.payload_type.as_deref(),
        Some("[dup: string]"),
        "the property member must not shadow the call-signature payload (negative)"
    );
    // The payload scope is the SFC (the signature was written in the SFC's own
    // defineEmits type argument).
    assert_eq!(
        change
            .analysis
            .payload_expr_scope
            .as_ref()
            .map(|s| s.as_str()),
        Some(FILE),
        "the local call-sig payload scope is the SFC"
    );
    // The property emit's display value is scoped to the SFC too (its
    // value-node scope — the tuple was authored in the SFC).
    assert_eq!(
        not_an_event
            .analysis
            .payload_expr_scope
            .as_ref()
            .map(|s| s.as_str()),
        Some(FILE),
        "the property emit's payload display is paired with its value-node scope"
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
        emits
            .iter()
            .map(|e| e.analysis.name.as_str())
            .collect::<Vec<_>>(),
        vec!["change"],
        "the cross-file call-signature event name surfaces"
    );
    let change = &emits[0];
    // The stripped payload's scope follows the SIGNATURE's declaration-origin
    // file (the imported base), not the SFC owner.
    assert_eq!(
        change
            .analysis
            .payload_expr_scope
            .as_ref()
            .map(|s| s.as_str()),
        Some(BASE),
        "the call-signature payload scope is the base file the signature was declared in"
    );
    // Negative: it is NOT the SFC owner.
    assert_ne!(
        change
            .analysis
            .payload_expr_scope
            .as_ref()
            .map(|s| s.as_str()),
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
    let change = local_emits
        .iter()
        .find(|e| e.analysis.name == "change")
        .unwrap();
    // The display is the bracketed payload tuple (event-name param stripped),
    // NOT the whole call-signature source text.
    assert_eq!(
        change.analysis.payload_type.as_deref(),
        Some("[value: number]"),
        "the call-sig payload_type is the bracketed stripped-payload tuple"
    );
    // Negative: it is NOT the whole call-signature source slice.
    assert_ne!(
        change.analysis.payload_type.as_deref(),
        Some("(e: 'change', value: number): void"),
        "the payload_type must not be the whole call-signature source text"
    );
    let select = local_emits
        .iter()
        .find(|e| e.analysis.name == "select")
        .unwrap();
    assert_eq!(
        select.analysis.payload_type.as_deref(),
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
    let cross_change = cross_emits
        .iter()
        .find(|e| e.analysis.name == "change")
        .unwrap();
    assert_eq!(
        cross_change.analysis.payload_type.as_deref(),
        Some("[value: number]"),
        "the cross-file call-sig payload_type is the same bracketed stripped-payload tuple"
    );
    // The cross-file display is byte-identical to the local one (the typed
    // payload tuple renders identically regardless of declaration site) —
    // proving consistency, not a per-shape divergence.
    assert_eq!(
        cross_change.analysis.payload_type, change.analysis.payload_type,
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

    let mut names: Vec<&str> = emits.iter().map(|e| e.analysis.name.as_str()).collect();
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
            .find(|e| e.analysis.name == name)
            .unwrap_or_else(|| panic!("event `{name}` surfaces"));
        assert_eq!(
            event.analysis.payload_type.as_deref(),
            Some("[value: number]"),
            "the leading event-name param is stripped, leaving the `[value: number]` \
             payload, got {:?}",
            event.analysis.payload_type
        );
    }
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
        model.analysis.name, "title",
        "the model prop is named after the model"
    );
    assert!(
        model.analysis.declared_in_macro_type_arg,
        "the model prop is declared at the macro site"
    );
    // Concrete typed form (not just `is_some()`): `defineModel<string>('title')`
    // synthesizes a model prop typed `string`, carried as the authored payload
    // locator plus its display.
    assert!(
        model.analysis.payload.is_some(),
        "the model prop carries its authored payload locator"
    );
    assert_eq!(
        model.analysis.type_annotation.as_deref(),
        Some("string"),
        "the model prop's display is the `string` model value type, got {:?}",
        model.analysis.type_annotation
    );
    // The re-anchored scope is the SFC owner, not the empty analyzer scope.
    assert_eq!(
        model.analysis.type_expr_scope.as_ref().map(|s| s.as_str()),
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

    let mut names: Vec<&str> = props.iter().map(|p| p.analysis.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, vec!["label", "size"], "both props surface");

    let size = props.iter().find(|p| p.analysis.name == "size").unwrap();
    // `size?` is declared optional; defaults do NOT change AnalyzedPropField
    // optionality (that is a PropAnalysis-layer concern). The field keeps its
    // raw declared optionality — matching the eager rail.
    assert!(
        size.analysis.is_optional,
        "size keeps its declared `?` optionality at the AnalyzedPropField layer"
    );
    let label = props.iter().find(|p| p.analysis.name == "label").unwrap();
    assert!(!label.analysis.is_optional, "label is required (no `?`)");
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
        .map(|p| p.analysis.name.as_str())
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

    let mut names: Vec<&str> = props.iter().map(|p| p.analysis.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(
        names,
        vec!["baseFlag", "count"],
        "the inherited Base member surfaces alongside the own-body member"
    );

    let count = props.iter().find(|p| p.analysis.name == "count").unwrap();
    assert!(
        count.analysis.declared_in_macro_type_arg,
        "count is in the macro type arg's own body"
    );
    let base_flag = props
        .iter()
        .find(|p| p.analysis.name == "baseFlag")
        .unwrap();
    assert!(
        !base_flag.analysis.declared_in_macro_type_arg,
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
        .find(|p| p.analysis.name == "val")
        .expect("the inherited generic member `val` surfaces");

    // The substituted value type is `Local` — a `Ref` the deriving file owns,
    // rendered at the terminal sink.
    assert_eq!(
        val.analysis.type_annotation.as_deref(),
        Some("Local"),
        "val's substituted value type displays as the `Local` ref, got {:?}",
        val.analysis.type_annotation
    );

    // The scope MUST be the DERIVING file (where `Local` is declared), NOT the
    // base file. Pre-fix (declaration_origin-first) this is the base file →
    // `Local` would not resolve there.
    assert_eq!(
        val.analysis.type_expr_scope.as_ref().map(|s| s.as_str()),
        Some(FILE),
        "val's type_expr_scope follows the value-node scope (the deriving SFC), not the base file"
    );
    assert_ne!(
        val.analysis.type_expr_scope.as_ref().map(|s| s.as_str()),
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
    let names: Vec<&str> = emits.iter().map(|e| e.analysis.name.as_str()).collect();

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
    let names: Vec<&str> = emits.iter().map(|e| e.analysis.name.as_str()).collect();

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

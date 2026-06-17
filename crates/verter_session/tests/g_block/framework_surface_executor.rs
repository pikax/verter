//! Framework-surface executor wire-entry coverage.
//!
//! Discriminating coverage for `VerterHost::resolve_framework_surface_with_audit`:
//! - VALIDATION FIRST: a malformed envelope (op/payload mismatch, framework
//!   schema-version mismatch) returns the typed wire `error` arm BEFORE any
//!   registry lookup or semantic dispatch — no `framework_surface` arm, no
//!   registered adapter selection observable.
//! - UNKNOWN ADAPTER: an unregistered adapter id returns a `MalformedPayload`
//!   wire error (no new error variant).
//! - VUE PARITY: a Vue SFC request resolves its props / emits / slots through
//!   the registry → `VueFrameworkAdapter` plan/normalize → relocated `vue_exec`
//!   resolution and yields the SAME members the live `vue_macro_dtos` path
//!   produces (behavior parity).
//! - STATUS: the response carries EXACTLY ONE entry per known kind; a
//!   supported-but-empty kind is SUPPORTED-empty, distinct from an UNSUPPORTED
//!   kind.

use std::collections::HashMap;
use std::sync::Arc;

use verter_protocol::typeinfo::graph::{
    self as wire, FrameworkSurfaceKind, FrameworkSurfaceKindSupport,
};
use verter_protocol::verter::v1::{
    graph_closure_policy, type_info_graph_request as wire_request, type_info_graph_response,
    type_info_request_error,
};
use verter_semantic::analysis::types::AnalyzedMacroKind;
use verter_session::typeinfo::types::{TypeInfoQueryLevel, VueMacroSurfaceRequest};
use verter_session::VerterHost;

use crate::harness;

const PARITY_VUE: &str = r#"<script setup lang="ts">
interface Props { count: number; label?: string }
defineProps<Props>();
defineEmits<{ change: [next: number] }>();
defineSlots<{ default(props: { item: string }): unknown }>();
</script>
<template><div></div></template>
"#;

fn default_context() -> wire::ProjectionReductionContext {
    wire::ProjectionReductionContext {
        mode: wire::ProjectionMode::Expanded as i32,
        demand: wire::ReductionDemand::Published as i32,
    }
}

fn one_level_closure() -> wire::ClosurePolicy {
    wire::ClosurePolicy {
        kind: Some(graph_closure_policy::Kind::OneLevel(
            wire::ClosureOneLevel {},
        )),
    }
}

fn default_display_policy() -> wire::DisplayPolicy {
    wire::DisplayPolicy {
        qualification: wire::DisplayQualification::Qualified as i32,
        branding: wire::DisplayBranding::On as i32,
        budgets: Some(wire::DisplayBudgets {
            max_string_length: 4096,
            max_depth: 16,
        }),
    }
}

/// Build a well-formed framework-surface envelope at schema 3 for `canonical`
/// + `adapter_id`.
fn framework_envelope(canonical: &str, adapter_id: &str) -> wire::TypeInfoGraphRequest {
    framework_envelope_versioned(canonical, adapter_id, 3, 3)
}

/// Build a framework-surface envelope with explicit envelope / payload schema
/// versions (so a mismatch can be exercised).
fn framework_envelope_versioned(
    canonical: &str,
    adapter_id: &str,
    envelope_version: u32,
    payload_version: u32,
) -> wire::TypeInfoGraphRequest {
    wire::TypeInfoGraphRequest {
        schema_version: envelope_version,
        operation: wire::Operation::FrameworkSurfaces as i32,
        payload: Some(wire_request::Payload::FrameworkSurface(
            wire::FrameworkSurfaceRequest {
                selector: Some(wire::ComponentSelector {
                    canonical_id: canonical.to_string(),
                    export_name: String::new(),
                    has_export_name: false,
                    framework_adapter_id: adapter_id.to_string(),
                }),
                context: Some(default_context()),
                closure: Some(one_level_closure()),
                display_policy: Some(default_display_policy()),
                include_provenance: false,
                include_diagnostics: false,
                include_projection: vec![],
                schema_version: payload_version,
            },
        )),
    }
}

fn build_host() -> Arc<VerterHost> {
    harness::build_hermetic_host_with_lib(
        &[("/Parity.vue", PARITY_VUE)],
        &[("lib.es5.d.ts", harness::STUB_LIB_ES5)],
    )
}

/// Extract the success `FrameworkSurfacePayload` from a response, or panic.
fn expect_payload(response: &wire::TypeInfoGraphResponse) -> &wire::FrameworkSurfacePayload {
    match &response.kind {
        Some(type_info_graph_response::Kind::FrameworkSurface(p)) => p,
        other => panic!("expected a framework_surface response arm, got {other:?}"),
    }
}

/// Index a payload's entries by kind.
fn entries_by_kind(
    payload: &wire::FrameworkSurfacePayload,
) -> HashMap<i32, &wire::FrameworkSurfaceKindEntry> {
    payload.surfaces.iter().map(|e| (e.kind, e)).collect()
}

/// Resolve a payload entry's member NAMES (interned through the graph string
/// table).
fn member_names(
    payload: &wire::FrameworkSurfacePayload,
    entry: &wire::FrameworkSurfaceKindEntry,
) -> Vec<String> {
    let strings = payload
        .graph
        .as_ref()
        .and_then(|g| g.strings.as_ref())
        .map(|t| t.entries.clone())
        .unwrap_or_default();
    entry
        .members
        .iter()
        .map(|m| strings.get(m.name_id as usize).cloned().unwrap_or_default())
        .collect()
}

#[test]
fn malformed_envelope_op_payload_mismatch_returns_error_before_dispatch() {
    // An operation discriminator that disagrees with the payload arm (a
    // ResolveSymbol operation carrying a FrameworkSurface payload) is malformed.
    // The executor must return the typed wire error BEFORE any registry lookup.
    let host = build_host();
    let mut envelope = framework_envelope("/Parity.vue", "vue");
    // Corrupt the operation discriminator to a non-framework operation.
    envelope.operation = wire::Operation::ResolveSymbol as i32;

    let result = host.resolve_framework_surface_with_audit(envelope);
    // A malformed envelope is the audited Err outcome carrying the typed wire
    // error — no framework_surface payload, no registered adapter selection.
    result
        .as_result()
        .expect_err("a malformed envelope must be the Err outcome");
}

#[test]
fn framework_schema_version_mismatch_rejected_before_dispatch() {
    // The framework-surface operation requires schema 3; a v2 payload is
    // rejected with MalformedPayload BEFORE any adapter lookup.
    let host = build_host();
    let envelope = framework_envelope_versioned("/Parity.vue", "vue", 2, 2);
    let result = host.resolve_framework_surface_with_audit(envelope);
    let error = result
        .as_result()
        .expect_err("a sub-minimum schema version must be the Err outcome");
    assert!(
        matches!(
            error.kind,
            Some(type_info_request_error::Kind::MalformedPayload(_))
        ),
        "a sub-minimum framework schema version is a MalformedPayload, got {:?}",
        error.kind
    );
}

#[test]
fn unknown_adapter_id_returns_malformed_payload() {
    // An adapter id with no registration is a MalformedPayload (NO new error
    // variant), surfaced after validation but before any semantic work.
    let host = build_host();
    let envelope = framework_envelope("/Parity.vue", "not-a-real-framework");
    let result = host.resolve_framework_surface_with_audit(envelope);
    let error = result
        .as_result()
        .expect_err("an unknown adapter id must be the Err outcome");
    assert!(
        matches!(
            error.kind,
            Some(type_info_request_error::Kind::MalformedPayload(_))
        ),
        "an unknown adapter id is a MalformedPayload, got {:?}",
        error.kind
    );
}

#[test]
fn response_carries_exactly_one_entry_per_known_kind() {
    let host = build_host();
    let envelope = framework_envelope("/Parity.vue", "vue");
    let result = host.resolve_framework_surface_with_audit(envelope);
    let response = result
        .into_result()
        .expect("a well-formed Vue request resolves");
    let payload = expect_payload(&response);
    assert_eq!(payload.surfaces.len(), 6, "one entry per known kind");
    // Every kind appears exactly once.
    let by_kind = entries_by_kind(payload);
    for kind in [
        FrameworkSurfaceKind::Props,
        FrameworkSurfaceKind::Emits,
        FrameworkSurfaceKind::Slots,
        FrameworkSurfaceKind::Options,
        FrameworkSurfaceKind::Expose,
        FrameworkSurfaceKind::Model,
    ] {
        assert!(
            by_kind.contains_key(&(kind as i32)),
            "missing entry for {kind:?}"
        );
    }
}

#[test]
fn vue_props_emits_slots_match_live_macro_dtos() {
    // BEHAVIOR PARITY: the executor's props / emits / slots members equal the
    // members the live `vue_macro_dtos` path produces for the same SFC.
    let host = build_host();
    let envelope = framework_envelope("/Parity.vue", "vue");
    let result = host.resolve_framework_surface_with_audit(envelope);
    let response = result.into_result().expect("Vue request resolves");
    let payload = expect_payload(&response);
    let by_kind = entries_by_kind(payload);

    // --- Props parity ---
    // The macro index comes from the live analysis snapshot; `vue_macro_dtos`
    // re-derives the authoritative content hash internally, so a zeroed
    // `root_identity` is sufficient here.
    let snapshot = host.get_analysis("/Parity.vue").expect("the SFC analyzes");
    let props_index = snapshot
        .macros
        .iter()
        .position(|m| m.kind == AnalyzedMacroKind::DefineProps)
        .expect("defineProps macro present");
    let props_dtos = host.vue_macro_dtos(&VueMacroSurfaceRequest {
        owner_canonical: Arc::from("/Parity.vue"),
        macro_index: props_index,
        macro_kind: AnalyzedMacroKind::DefineProps,
        root_identity: [0u8; 16],
        level: TypeInfoQueryLevel::FullMetadata,
    });
    let mut live_prop_names: Vec<String> = props_dtos
        .prop_fields()
        .iter()
        .map(|f| f.name.clone())
        .collect();
    live_prop_names.sort();
    let props_entry = by_kind
        .get(&(FrameworkSurfaceKind::Props as i32))
        .expect("props entry");
    let mut wire_prop_names = member_names(payload, props_entry);
    wire_prop_names.sort();
    assert_eq!(
        wire_prop_names, live_prop_names,
        "executor props members must equal live vue_macro_dtos props"
    );
    assert!(
        live_prop_names.contains(&"count".to_string())
            && live_prop_names.contains(&"label".to_string()),
        "the fixture declares count + label props, got {live_prop_names:?}"
    );

    // --- Emits parity ---
    let emits_index = snapshot
        .macros
        .iter()
        .position(|m| m.kind == AnalyzedMacroKind::DefineEmits)
        .expect("defineEmits macro present");
    let emits_dtos = host.vue_macro_dtos(&VueMacroSurfaceRequest {
        owner_canonical: Arc::from("/Parity.vue"),
        macro_index: emits_index,
        macro_kind: AnalyzedMacroKind::DefineEmits,
        root_identity: [0u8; 16],
        level: TypeInfoQueryLevel::FullMetadata,
    });
    let mut live_emit_names: Vec<String> = emits_dtos
        .emit_fields()
        .iter()
        .map(|f| f.name.clone())
        .collect();
    live_emit_names.sort();
    let emits_entry = by_kind
        .get(&(FrameworkSurfaceKind::Emits as i32))
        .expect("emits entry");
    let mut wire_emit_names = member_names(payload, emits_entry);
    wire_emit_names.sort();
    assert_eq!(
        wire_emit_names, live_emit_names,
        "executor emits members must equal live vue_macro_dtos emits"
    );
    assert!(
        live_emit_names.contains(&"change".to_string()),
        "the fixture declares a `change` event, got {live_emit_names:?}"
    );

    // --- Slots parity ---
    let slots_index = snapshot
        .macros
        .iter()
        .position(|m| m.kind == AnalyzedMacroKind::DefineSlots)
        .expect("defineSlots macro present");
    let slots_dtos = host.vue_macro_dtos(&VueMacroSurfaceRequest {
        owner_canonical: Arc::from("/Parity.vue"),
        macro_index: slots_index,
        macro_kind: AnalyzedMacroKind::DefineSlots,
        root_identity: [0u8; 16],
        level: TypeInfoQueryLevel::FullMetadata,
    });
    let mut live_slot_names: Vec<String> = slots_dtos
        .slot_fields()
        .iter()
        .map(|f| f.name.clone())
        .collect();
    live_slot_names.sort();
    let slots_entry = by_kind
        .get(&(FrameworkSurfaceKind::Slots as i32))
        .expect("slots entry");
    let mut wire_slot_names = member_names(payload, slots_entry);
    wire_slot_names.sort();
    assert_eq!(
        wire_slot_names, live_slot_names,
        "executor slots members must equal live vue_macro_dtos slots"
    );
    assert!(
        live_slot_names.contains(&"default".to_string()),
        "the fixture declares a `default` slot, got {live_slot_names:?}"
    );
}

#[test]
fn supported_empty_kind_is_distinct_from_unsupported() {
    // The fixture has no defineOptions; Options is a SUPPORTED kind for the Vue
    // adapter, so its entry is SUPPORTED-empty (not UNSUPPORTED) — the
    // supported-empty vs unsupported distinction on the wire.
    let host = build_host();
    let envelope = framework_envelope("/Parity.vue", "vue");
    let result = host.resolve_framework_surface_with_audit(envelope);
    let response = result.into_result().expect("Vue request resolves");
    let payload = expect_payload(&response);
    let by_kind = entries_by_kind(payload);

    let options = by_kind
        .get(&(FrameworkSurfaceKind::Options as i32))
        .expect("options entry present");
    let status = options.status.as_ref().expect("options status present");
    assert_eq!(
        status.support,
        FrameworkSurfaceKindSupport::Supported as i32,
        "Options is a supported Vue kind, so its empty entry is SUPPORTED-empty, not UNSUPPORTED"
    );
    assert!(
        options.members.is_empty(),
        "the fixture has no defineOptions, so the options entry is empty"
    );

    // Props, by contrast, is SUPPORTED with non-empty members.
    let props = by_kind
        .get(&(FrameworkSurfaceKind::Props as i32))
        .expect("props entry present");
    assert_eq!(
        props.status.as_ref().unwrap().support,
        FrameworkSurfaceKindSupport::Supported as i32
    );
    assert!(!props.members.is_empty(), "props has members");
}

const MODEL_VUE: &str = r#"<script setup lang="ts">
const model = defineModel<string>();
</script>
<template><div></div></template>
"#;

#[test]
fn define_model_surface_carries_the_model_binding() {
    // A `defineModel<string>()` component must produce a non-empty MODEL surface
    // (the model binding) — NOT a silent SUPPORTED-empty model entry. This is
    // the model-slot fix: defineModel feeds BOTH the props slot (meta contract)
    // and the model slot (the MODEL framework surface).
    let host = harness::build_hermetic_host_with_lib(
        &[("/Model.vue", MODEL_VUE)],
        &[("lib.es5.d.ts", harness::STUB_LIB_ES5)],
    );
    let envelope = framework_envelope("/Model.vue", "vue");
    let response = host
        .resolve_framework_surface_with_audit(envelope)
        .into_result()
        .expect("Model.vue resolves");
    let payload = expect_payload(&response);
    let by_kind = entries_by_kind(payload);
    let model = by_kind
        .get(&(FrameworkSurfaceKind::Model as i32))
        .expect("model entry present");
    assert_eq!(
        model.status.as_ref().unwrap().support,
        FrameworkSurfaceKindSupport::Supported as i32
    );
    assert!(
        !model.members.is_empty(),
        "a defineModel component must surface a non-empty MODEL binding, got {:?}",
        member_names(payload, model)
    );
    // The default model binding is named `modelValue`.
    let names = member_names(payload, model);
    assert!(
        names.contains(&"modelValue".to_string()),
        "the default defineModel binding is `modelValue`, got {names:?}"
    );
}

#[test]
fn define_model_only_component_surfaces_modelvalue_in_props() {
    // A `defineModel`-only component (no `defineProps`) must STILL surface its
    // synthesized `modelValue` prop in the PROPS surface — the model binding is
    // also a prop. A regression here drops the prop entirely.
    let host = harness::build_hermetic_host_with_lib(
        &[("/Model.vue", MODEL_VUE)],
        &[("lib.es5.d.ts", harness::STUB_LIB_ES5)],
    );
    let envelope = framework_envelope("/Model.vue", "vue");
    let response = host
        .resolve_framework_surface_with_audit(envelope)
        .into_result()
        .expect("Model.vue resolves");
    let payload = expect_payload(&response);
    let by_kind = entries_by_kind(payload);
    let props = by_kind
        .get(&(FrameworkSurfaceKind::Props as i32))
        .expect("props entry present");
    let names = member_names(payload, props);
    assert!(
        names.contains(&"modelValue".to_string()),
        "a defineModel-only component must surface `modelValue` in PROPS, got {names:?}"
    );
}

const MULTI_MODEL_VUE: &str = r#"<script setup lang="ts">
const title = defineModel<string>("title");
const count = defineModel<number>("count");
</script>
<template><div></div></template>
"#;

#[test]
fn multiple_define_model_calls_surface_every_binding() {
    // Two `defineModel` calls must surface BOTH bindings in the MODEL surface —
    // aggregation across all matching macros, not just the first.
    let host = harness::build_hermetic_host_with_lib(
        &[("/MultiModel.vue", MULTI_MODEL_VUE)],
        &[("lib.es5.d.ts", harness::STUB_LIB_ES5)],
    );
    let envelope = framework_envelope("/MultiModel.vue", "vue");
    let response = host
        .resolve_framework_surface_with_audit(envelope)
        .into_result()
        .expect("MultiModel.vue resolves");
    let payload = expect_payload(&response);
    let by_kind = entries_by_kind(payload);
    let model = by_kind
        .get(&(FrameworkSurfaceKind::Model as i32))
        .expect("model entry present");
    let names = member_names(payload, model);
    assert!(
        names.contains(&"title".to_string()) && names.contains(&"count".to_string()),
        "both defineModel bindings must surface in MODEL, got {names:?}"
    );
    // The two model bindings also both appear in PROPS.
    let props = by_kind
        .get(&(FrameworkSurfaceKind::Props as i32))
        .expect("props entry present");
    let prop_names = member_names(payload, props);
    assert!(
        prop_names.contains(&"title".to_string()) && prop_names.contains(&"count".to_string()),
        "both model bindings must surface in PROPS, got {prop_names:?}"
    );
}

const OPTIONS_VUE: &str = r#"<script setup lang="ts">
defineOptions<{ name: 'Widget'; inheritAttrs?: boolean }>();
defineProps<{ count: number }>();
</script>
<template><div></div></template>
"#;

const EXPOSE_VUE: &str = r#"<script setup lang="ts">
defineExpose<{ focus(): void; readonly count: number }>();
defineProps<{ label: string }>();
</script>
<template><div></div></template>
"#;

#[test]
fn define_options_present_resolves_supported_with_members() {
    // A `defineOptions<T>()` macro decodes SUPPORTED with the type argument's
    // declared members — NOT UNSUPPORTED-because-present (that flips a kind Vue
    // advertises as supported into UNSUPPORTED purely on content, breaking the
    // `supported_surfaces` ↔ runtime-status consistency rule).
    let host = harness::build_hermetic_host_with_lib(
        &[("/Options.vue", OPTIONS_VUE)],
        &[("lib.es5.d.ts", harness::STUB_LIB_ES5)],
    );
    let envelope = framework_envelope("/Options.vue", "vue");
    let response = host
        .resolve_framework_surface_with_audit(envelope)
        .into_result()
        .expect("Options.vue resolves");
    let payload = expect_payload(&response);
    let by_kind = entries_by_kind(payload);
    let options = by_kind
        .get(&(FrameworkSurfaceKind::Options as i32))
        .expect("options entry present");
    let status = options.status.as_ref().unwrap();
    assert_eq!(
        status.support,
        FrameworkSurfaceKindSupport::Supported as i32,
        "a present defineOptions<T> resolves SUPPORTED with its declared members, \
         not UNSUPPORTED-because-present"
    );
    let names = member_names(payload, options);
    assert!(
        names.contains(&"name".to_string()) && names.contains(&"inheritAttrs".to_string()),
        "both defineOptions members must surface, got {names:?}"
    );
    // The optional member is non-required; the required member is required.
    let required: std::collections::HashMap<&str, bool> = options
        .members
        .iter()
        .map(|m| {
            let strings = payload
                .graph
                .as_ref()
                .and_then(|g| g.strings.as_ref())
                .map(|t| &t.entries)
                .unwrap();
            (strings[m.name_id as usize].as_str(), m.required)
        })
        .collect();
    assert_eq!(required.get("name"), Some(&true), "`name` is required");
    assert_eq!(
        required.get("inheritAttrs"),
        Some(&false),
        "`inheritAttrs?` is optional"
    );
    // Props on the same component still resolve SUPPORTED with members.
    let props = by_kind
        .get(&(FrameworkSurfaceKind::Props as i32))
        .expect("props entry present");
    assert_eq!(
        props.status.as_ref().unwrap().support,
        FrameworkSurfaceKindSupport::Supported as i32
    );
    assert!(!props.members.is_empty());
}

#[test]
fn define_expose_present_resolves_supported_with_members() {
    // A `defineExpose<T>()` macro decodes SUPPORTED with the type argument's
    // declared members (a method + a readonly property).
    let host = harness::build_hermetic_host_with_lib(
        &[("/Expose.vue", EXPOSE_VUE)],
        &[("lib.es5.d.ts", harness::STUB_LIB_ES5)],
    );
    let envelope = framework_envelope("/Expose.vue", "vue");
    let response = host
        .resolve_framework_surface_with_audit(envelope)
        .into_result()
        .expect("Expose.vue resolves");
    let payload = expect_payload(&response);
    let by_kind = entries_by_kind(payload);
    let expose = by_kind
        .get(&(FrameworkSurfaceKind::Expose as i32))
        .expect("expose entry present");
    let status = expose.status.as_ref().unwrap();
    assert_eq!(
        status.support,
        FrameworkSurfaceKindSupport::Supported as i32,
        "a present defineExpose<T> resolves SUPPORTED with its declared members"
    );
    let names = member_names(payload, expose);
    assert!(
        names.contains(&"focus".to_string()) && names.contains(&"count".to_string()),
        "both defineExpose members must surface, got {names:?}"
    );
}

#[test]
fn every_supported_kind_resolves_supported_when_its_macro_is_used() {
    // CONSISTENCY (`supported_surfaces` ↔ runtime status): for every kind Vue's
    // descriptor advertises as supported, a component USING that kind's macro
    // resolves it SUPPORTED — never UNSUPPORTED-because-present. A content-
    // dependent support flip would mean `supported_surfaces` stops meaning "this
    // adapter supports this kind".
    const ALL_MACROS_VUE: &str = r#"<script setup lang="ts">
defineProps<{ count: number }>();
defineEmits<{ change: [next: number] }>();
defineSlots<{ default(p: { item: string }): unknown }>();
defineOptions<{ name: 'Widget' }>();
defineExpose<{ focus(): void }>();
defineModel<string>('title');
</script>
<template><div></div></template>
"#;
    let host = harness::build_hermetic_host_with_lib(
        &[("/AllMacros.vue", ALL_MACROS_VUE)],
        &[("lib.es5.d.ts", harness::STUB_LIB_ES5)],
    );
    // Vue's descriptor advertises all six kinds as supported.
    let descriptor = verter_session::framework::descriptor::vue_descriptor();
    let envelope = framework_envelope("/AllMacros.vue", "vue");
    let response = host
        .resolve_framework_surface_with_audit(envelope)
        .into_result()
        .expect("AllMacros.vue resolves");
    let payload = expect_payload(&response);
    let by_kind = entries_by_kind(payload);
    for &kind in descriptor.supported_surfaces {
        let entry = by_kind
            .get(&(kind as i32))
            .unwrap_or_else(|| panic!("missing entry for advertised-supported kind {kind:?}"));
        let support = entry.status.as_ref().unwrap().support;
        assert_eq!(
            support,
            FrameworkSurfaceKindSupport::Supported as i32,
            "kind {kind:?} is in Vue's supported_surfaces and its macro is used, so it must \
             resolve SUPPORTED — got support discriminant {support}"
        );
    }
}

#[test]
fn nonexistent_named_export_is_a_malformed_payload() {
    // A named-export selector for an export the owner does not declare must be a
    // typed MalformedPayload — NOT a silent fall-through to the default
    // component surface (which would resolve the WRONG component).
    let host = build_host();
    let mut envelope = framework_envelope("/Parity.vue", "vue");
    if let Some(wire_request::Payload::FrameworkSurface(r)) = envelope.payload.as_mut() {
        if let Some(selector) = r.selector.as_mut() {
            selector.has_export_name = true;
            selector.export_name = "NoSuchExport".to_string();
        }
    }
    let result = host.resolve_framework_surface_with_audit(envelope);
    let error = result
        .as_result()
        .expect_err("a nonexistent named export must be the Err outcome");
    assert!(
        matches!(
            error.kind,
            Some(type_info_request_error::Kind::MalformedPayload(_))
        ),
        "a nonexistent named export is a MalformedPayload, got {:?}",
        error.kind
    );
}

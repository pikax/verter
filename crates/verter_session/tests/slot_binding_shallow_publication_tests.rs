//! R21.5 discriminating regression: an indexed-access slot binding
//! whose underlying property type contains a callback with a deep
//! generic-envelope parameter MUST publish a shallow carrier on
//! `r#type` — NOT the structural fan-out of the callback payload.
//!
//! Fixture mirrors the ChatMessage shape that triggered the 43.7 MB
//! `outputSchema`/`execute` leak: the `toolbar` slot's props bag
//! carries `controls: PanelProps<TMeta, TData, TTools>['controls']`
//! where `PanelProps.controls` is a callback whose second parameter is
//! a generic `Envelope` interface whose `entries` contain mapped tool
//! payloads (`inputSchema`/`outputSchema`/`execute`). The leak was at
//! `publish_merged_bindings` calling `raise_node_to_type_expr` on the
//! `Published(Shallow)`-reduced value node; the fix publishes either
//! the parser-lowered `binding_expr` (when present) or a symbolic
//! `TypeExpr::Ref { name }` carrier — both bounded.
//!
//! Discrimination: reverting the fix (restoring the deep raise) makes
//! the serialized binding type expression contain `outputSchema` /
//! `inputSchema` / `execute` (the AI SDK-style tool members) and
//! balloon past the 16 KB shallow-carrier budget. RED-on-revert
//! evidence is required to ship.

use std::sync::Arc;

use verter_session::audited_request::AuditedRequest;
use verter_session::carrier_verdict_db::CarrierIdentity;
use verter_session::{FileKind, HostConfig, UpsertRequest, VerterHost};

fn build_host() -> Arc<VerterHost> {
    Arc::new(VerterHost::new_standalone(HostConfig {
        audit_enabled: true,
        footprint_capture: true,
        ..HostConfig::default()
    }))
}

fn upsert_ts(host: &VerterHost, id: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_string(),
            source: Arc::from(source),
            file_kind: FileKind::NonSfc,
            aliases: Vec::new(),
        })
        .expect("upsert ts");
}

fn upsert_vue(host: &VerterHost, id: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_string(),
            source: Arc::from(source),
            file_kind: FileKind::VueSfc,
            aliases: Vec::new(),
        })
        .expect("upsert vue");
}

const TOOLKIT_TS: &str = r#"
export interface ToolRegistry {
  lookup: {
    inputSchema: { kind: 'input' };
    outputSchema: { kind: 'output' };
    execute: (input: unknown) => unknown;
  };
}

export type DataPayloads = { [key: string]: unknown };

export type ToolEntry<TTools extends ToolRegistry = ToolRegistry> = {
  [K in keyof TTools]: {
    type: K;
    input: TTools[K] extends { inputSchema: infer I } ? I : never;
    output: TTools[K] extends { outputSchema: infer O } ? O : never;
    run: TTools[K] extends { execute: infer E } ? E : never;
  }
}[keyof TTools];

export interface Envelope<
  TMeta = unknown,
  TData extends DataPayloads = DataPayloads,
  TTools extends ToolRegistry = ToolRegistry
> {
  id: string;
  meta?: TMeta;
  entries: ({ type: 'text'; text: string } | ToolEntry<TTools>)[];
  status: 'idle' | 'active';
}

export interface ControlShell {
  label?: string;
}

export interface TriggerEvent {
  type: 'trigger';
}
"#;

const GENERIC_PANEL_VUE: &str = r#"<script lang="ts">
import type { ControlShell, DataPayloads, Envelope, ToolRegistry, TriggerEvent } from './toolkit';

export interface PanelProps<
  TMeta = unknown,
  TData extends DataPayloads = DataPayloads,
  TTools extends ToolRegistry = ToolRegistry
> extends Envelope<TMeta, TData, TTools> {
  controls?: (ControlShell & {
    activate?: (event: TriggerEvent, envelope: Envelope<TMeta, TData, TTools>) => void;
  })[];
}

export interface PanelSlots<
  TMeta = unknown,
  TData extends DataPayloads = DataPayloads,
  TTools extends ToolRegistry = ToolRegistry
> {
  toolbar?(props: Envelope<TMeta, TData, TTools> & {
    controls: PanelProps<TMeta, TData, TTools>['controls'];
  }): unknown;
}
</script>

<script setup lang="ts" generic="TMeta, TData extends DataPayloads, TTools extends ToolRegistry">
defineSlots<PanelSlots<TMeta, TData, TTools>>();
</script>
<template><div /></template>
"#;

#[test]
fn indexed_access_slot_binding_does_not_expand_nested_callback_payload() {
    let host = build_host();
    upsert_ts(&host, "/toolkit.ts", TOOLKIT_TS);
    upsert_vue(&host, "/GenericPanel.vue", GENERIC_PANEL_VUE);

    let (analysis, _resolved, _audit) = AuditedRequest::builder()
        .attach_to(host)
        .resolve_component_meta("/GenericPanel.vue")
        .expect("hermetic indexed-access slot-binding repro must resolve");

    let controls_binding = analysis
        .slots
        .iter()
        .find(|slot| slot.name == "toolbar")
        .and_then(|slot| {
            slot.bindings
                .iter()
                .find(|binding| binding.name == "controls")
        })
        .expect("toolbar slot must publish a controls binding");

    let binding_json =
        serde_json::to_string(&controls_binding.type_expr).expect("serialize binding type expr");

    assert!(
        !binding_json.contains("outputSchema")
            && !binding_json.contains("inputSchema")
            && !binding_json.contains("execute"),
        "indexed-access slot binding must stay shallow. The binding should preserve the \
         `PanelProps<...>['controls']`/callback-envelope carrier instead of expanding the \
         nested `Envelope` tool body. Serialized binding bytes={} json={binding_json}",
        binding_json.len(),
    );

    assert!(
        binding_json.len() < 16 * 1024,
        "indexed-access slot binding should be a compact shallow carrier; observed {} bytes",
        binding_json.len(),
    );
}

/// Block 6.j R22 — producer-side discriminating test.
///
/// The slot-binding graph publisher's no-parser-binding branch
/// (`publish_merged_bindings` in
/// `crates/verter_session/src/meta_resolve/slot_binding_graph.rs`)
/// mints a symbolic `TypeExpr::Ref { name }` carrier when the parser
/// has no `binding_expr` for a graph-native `(slot_name, binding_name)`
/// pair. R22 requires that branch to ALSO emit a `CarrierProvenance`
/// sidecar so the downstream verdict cache and registry-collection
/// short-circuits can identify the synthetic carrier without
/// reparsing.
///
/// Discrimination: reverting the `Some(provenance)` emission (back to
/// `None` for the no-parser branch) makes this assertion FAIL while
/// the existing
/// `indexed_access_slot_binding_does_not_expand_nested_callback_payload`
/// invariant continues to PASS — proving the provenance is the
/// distinguishing fact, not the carrier shape.
#[test]
fn synthetic_slot_binding_carrier_emits_provenance_sidecar() {
    use verter_semantic::analysis::type_expand::PublishedSurfaceKind;

    let host = build_host();
    upsert_ts(&host, "/toolkit.ts", TOOLKIT_TS);
    upsert_vue(&host, "/GenericPanel.vue", GENERIC_PANEL_VUE);

    let expanded = host
        .evaluate_types("/GenericPanel.vue")
        .expect("evaluate_types must return for the GenericPanel fixture");

    // R22-fix sparse-sidecar variant: provenance lives in the
    // parent `ExpandedComponentTypes::carrier_provenance_table`,
    // not on the `ExpandedField` itself.
    let synthetic_carriers: Vec<_> = expanded
        .slot_bindings
        .iter()
        .filter(|f| {
            expanded
                .carrier_provenance_table
                .contains(PublishedSurfaceKind::SlotBinding, f.name.as_str())
        })
        .collect();

    assert!(
        !synthetic_carriers.is_empty(),
        "GenericPanel.vue's generic `defineSlots<PanelSlots<...>>()` is the canonical \
         no-parser-binding fixture. The R22 producer MUST record at least one \
         synthetic-carrier entry in `expanded.carrier_provenance_table` here. None \
         observed — the no-parser branch in `publish_merged_bindings` is silently \
         skipping the table-insert. published slot_bindings={:#?}",
        expanded
            .slot_bindings
            .iter()
            .map(|f| &f.name)
            .collect::<Vec<_>>()
    );

    for field in &synthetic_carriers {
        let provenance = expanded
            .carrier_provenance_table
            .get(PublishedSurfaceKind::SlotBinding, field.name.as_str())
            .expect("filtered above");

        // The provenance.binding_name must match the
        // `TypeExpr::Ref { name }` shape that gets published. Codex's
        // TOP RISK ("name-only key collision") demands these stay
        // structurally tied — a divergence would let the cache key
        // and the published carrier disagree.
        let ref_name = match &field.r#type {
            verter_type_expr::TypeExpr::Ref {
                name,
                type_arguments,
            } if type_arguments.is_empty() => Some(name.clone()),
            _ => None,
        };

        assert_eq!(
            ref_name.as_deref(),
            Some(provenance.binding_name.as_ref()),
            "synthetic carrier's `Ref` name must match `carrier_provenance.binding_name`. \
             Field `{}` has type {:?} but provenance says binding_name=`{}`. Without this \
             invariant the verdict cache cannot correlate published carriers with their \
             cache keys.",
            field.name,
            field.r#type,
            provenance.binding_name,
        );

        assert!(
            matches!(provenance.surface_kind, PublishedSurfaceKind::SlotBinding),
            "synthetic slot-binding carrier must be tagged SlotBinding; field `{}` got {:?}",
            field.name,
            provenance.surface_kind,
        );

        // The slot name is structurally `<slot>.<binding>`; the
        // provenance.slot_name MUST point at the `<slot>` half.
        let (slot_label, binding_label) = field
            .name
            .split_once('.')
            .expect("slot-binding field names are `slot.binding`");
        assert_eq!(
            provenance.slot_name.as_deref(),
            Some(slot_label),
            "provenance.slot_name must match the field's slot half for `{}`",
            field.name,
        );
        assert_eq!(
            provenance.binding_name.as_ref(),
            binding_label,
            "provenance.binding_name must match the field's binding half for `{}`",
            field.name,
        );

        assert!(
            provenance.scope_canonical_id.contains("GenericPanel.vue"),
            "provenance scope must point to the publishing component; got `{}` for field `{}`",
            provenance.scope_canonical_id,
            field.name,
        );
    }
}

/// Carrier verdict cache eager-admission contract.
///
/// The producer (`publish_merged_bindings` no-parser branch) admits a
/// `DoNotDeepen` verdict into the host-owned `CarrierVerdictDb` for
/// every synthetic carrier it mints. This test locks down that
/// contract: after `evaluate_types` runs on the GenericPanel.vue
/// fixture, the host's `carrier_verdicts` cache MUST contain a
/// `DoNotDeepen` entry for each synthetic carrier present in
/// `expanded.slot_bindings`.
///
/// Discrimination: removing the `carrier_verdicts.admit_do_not_deepen(...)`
/// call at the producer makes this assertion FAIL — the cache stays
/// empty even though the provenance sidecar still gets populated.
/// The two facts are independent: provenance ≠ cache admission.
#[test]
fn synthetic_carrier_admission_populates_carrier_verdict_db() {
    let host = build_host();
    upsert_ts(&host, "/toolkit.ts", TOOLKIT_TS);
    upsert_vue(&host, "/GenericPanel.vue", GENERIC_PANEL_VUE);

    // Drive a getComponentMeta query to exercise the carrier-publishing
    // pipeline end-to-end (evaluate_types -> publish_merged_bindings).
    let expanded = host
        .evaluate_types("/GenericPanel.vue")
        .expect("evaluate_types must return for GenericPanel");

    use verter_semantic::analysis::type_expand::PublishedSurfaceKind;
    let verdicts = host.project_type_store().carrier_verdicts();
    let synthetic_count = expanded
        .slot_bindings
        .iter()
        .filter(|f| {
            expanded
                .carrier_provenance_table
                .contains(PublishedSurfaceKind::SlotBinding, f.name.as_str())
        })
        .count();

    assert!(
        synthetic_count > 0,
        "fixture must mint at least one synthetic carrier for this test to discriminate",
    );
    assert!(
        verdicts.admissions_count() >= synthetic_count as u64,
        "CarrierVerdictDb admissions ({}) must cover every synthetic carrier ({}). The \
         producer in `publish_merged_bindings` is not calling \
         `carrier_verdicts.admit_do_not_deepen(...)` at carrier-mint time.",
        verdicts.admissions_count(),
        synthetic_count,
    );

    // For each synthetic carrier, build the cache identity exactly the
    // way the producer did and confirm the lookup hits.
    for field in expanded.slot_bindings.iter().filter(|f| {
        expanded
            .carrier_provenance_table
            .contains(PublishedSurfaceKind::SlotBinding, f.name.as_str())
    }) {
        let provenance = expanded
            .carrier_provenance_table
            .get(PublishedSurfaceKind::SlotBinding, field.name.as_str())
            .expect("filtered above");
        let key = CarrierIdentity::from_provenance(provenance);
        assert!(
            verdicts.is_do_not_deepen(&key),
            "carrier_verdicts.get({:?}) returned None — admission must precede first downstream \
             consult. field `{}` provenance={:#?}",
            key,
            field.name,
            provenance,
        );
    }
}

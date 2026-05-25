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

//! Block 6.i Round 9 — Pattern B discriminator (generic-parameter library
//! Mapped publication leak).
//!
//! Same root mechanism as Pattern A but the leak originates from a
//! generic parameter that, when instantiated, contains an external
//! library record type with deep object members. The macro's lowered
//! payload carries the generic through, and the Expanded Class A
//! lowering at the macro-publication boundary walks the
//! generic-instantiated record body.
//!
//! Mirrors the corpus `ChatMessage.vue` (singular) shape:
//! `ChatMessageProps<TMetadata, TDataParts, TTools> extends
//! UIMessage<TMetadata, TDataParts, TTools>` where
//! `UIMessage<…, …, TTools>` exposes the leaking `outputSchema` and
//! `execute` members through `tools: TTools = UITools`.
//!
//! Architecturally identical fix surface to Pattern A — Commit 2's
//! path-precise transit-shallow swap in
//! [`produce_one_macro_object_shape`] keeps both the Pattern A flat
//! Mapped chain AND the Pattern B generic-parameter-substituted Mapped
//! body as deferred carriers at the macro-publication boundary.
//!
//! ## Hermetic shape
//!
//! `defineProps<Partial<UIMessage<unknown, unknown, UITools>>>()` —
//! the generic-instantiated `UIMessage` body has `outputSchema` and
//! `execute` keys (sourced from `tools: TTools = UITools` via the
//! `outputSchema` / `execute` fields on UITools). The non-slot
//! `Published(Expanded)` lowering reduces `Partial<…>`'s Mapped over
//! `keyof UIMessage` and emits one `ProjectMember` edge per
//! enumerated key.

#![allow(clippy::too_many_lines, dead_code, unused_imports)]

#[path = "component_meta_audit/harness.rs"]
mod harness;

use verter_audit::origin_graph::{OriginEdgeKind, OriginEdgeMetaDto};
use verter_session::audited_request::AuditedRequest;

const UI_MESSAGE_TS: &str = r#"
export interface UITools {
  outputSchema: unknown;
  execute: (input: unknown) => unknown;
}
export interface UIMessage<TMetadata = unknown, TDataParts = unknown, TTools = UITools> {
  metadata: TMetadata;
  parts: TDataParts;
  outputSchema: TTools extends { outputSchema: infer O } ? O : never;
  execute: TTools extends { execute: infer E } ? E : never;
}
"#;

// Mirrors the corpus singular `ChatMessage.vue` mechanism:
// generic-instantiated `UIMessage` body walked through a non-slot
// Mapped publication.
const CHAT_MESSAGE_VUE: &str = r#"<script setup lang="ts">
import type { UIMessage, UITools } from './ui_message';
defineProps<Partial<UIMessage<unknown, unknown, UITools>>>();
</script>
<template><div></div></template>
"#;

#[test]
fn pattern_b_generic_parameter_substitution_does_not_leak_inherited_library_members() {
    let host = harness::build_hermetic_host_with_lib(
        &[
            ("/ui_message.ts", UI_MESSAGE_TS),
            ("/ChatMessage.vue", CHAT_MESSAGE_VUE),
        ],
        &[("lib.es5.d.ts", harness::STUB_LIB_ES5)],
    );

    let (_analysis, _resolved, audit) = AuditedRequest::builder()
        .attach_to(host)
        .resolve_component_meta("/ChatMessage.vue")
        .expect("hermetic resolve must succeed");

    let footprint = audit
        .footprint
        .as_ref()
        .expect("footprint_capture is enabled in this harness");

    let mut outputschema_count = 0usize;
    let mut execute_count = 0usize;
    for edge in footprint.derivation_subgraph.edges.iter() {
        if !matches!(edge.kind, OriginEdgeKind::ProjectMember) {
            continue;
        }
        if let OriginEdgeMetaDto::ProjectMember { member_name, .. } = &edge.meta {
            match member_name.as_ref() {
                "outputSchema" => outputschema_count += 1,
                "execute" => execute_count += 1,
                _ => {}
            }
        }
    }

    let mut projection_path_count = 0usize;
    for projection in footprint.projections.iter() {
        for seg in projection.path.iter() {
            if let verter_audit::origin_graph::ProjectPathSegment::Member { name } = seg {
                if name.as_ref() == "outputSchema" || name.as_ref() == "execute" {
                    projection_path_count += 1;
                }
            }
        }
    }

    let total = outputschema_count + execute_count + projection_path_count;

    assert_eq!(
        total, 0,
        "Block 6.i Round 9 Pattern B — generic-parameter library-Mapped \
         publication leak MUST close at the macro-publication boundary. \
         A `defineProps<Partial<UIMessage<unknown, unknown, UITools>>>()` \
         (architectural class of the corpus singular `ChatMessage.vue`'s \
         `ChatMessageProps<M, D, U> extends UIMessage<M, D, U>`) MUST NOT \
         emit any `outputSchema` / `execute` ProjectMember edges or \
         projection-path Member segments through the audit derivation \
         subgraph. Got: outputSchema(edges)={outputschema_count}, \
         execute(edges)={execute_count}, projection_path_member_hits={projection_path_count}. \
         At HEAD `23c866eb1` the non-slot path's Published(Expanded) \
         lowering reduces `Partial<UIMessage<…>>` via the Mapped \
         publication path; `build_mapped_type` / `intern_keyspace_names` \
         then emits one `ProjectMember` edge per enumerated key. \
         Commit 2's path-precise transit-shallow swap (non-Conditional \
         root → transit-shallow) keeps the Mapped carrier deferred; the \
         consumer's per-member projection walks specific members only \
         when explicitly demanded."
    );
}

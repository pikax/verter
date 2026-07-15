//! Chain W discriminator — Conditional `extends` eager-expansion
//! through inherited publication context.
//!
//! When a `TypeExpr::Conditional` is lowered under a publication-
//! demand `reduction_context` (`Published(Expanded)` / `Published
//! (Navigate)`), the `extends` arm inherits the same demand. If the
//! `extends` shape contains a Mapped over a literal-union keyspace
//! (e.g. `Partial<{ outputSchema, execute }>`), `build_mapped_type`
//! under `may_reduce_operator(Published(_)) == true` enumerates the
//! keyspace and emits one `ProjectMember` edge per literal — even
//! though the conditional's `extends` is consumed by the relation
//! engine for assignability, NOT published at the per-prop boundary.
//!
//! The Conditional's `check` and `extends` arguments are shape-
//! decision consumers; they MUST lower under `StructuralTransit`
//! regardless of the outer `reduction_context`. Under
//! `StructuralTransit`, `may_reduce_operator` evaluates `false`
//! everywhere — nested `Instantiate` / `KeyOf` / `MappedType`
//! operators carrier-stop on the relation-input lowering frame
//! without reifying keyspace edges. The `true_branch` and
//! `false_branch` keep the outer demand because the selected branch
//! becomes the conditional's published result.
//!
//! ## Hermetic shape
//!
//! Minimal architectural pattern reproducing the AI-SDK
//! `ChatMessageProps` chain at the per-prop publication boundary:
//!
//! ```text
//! type ToolBody = { outputSchema: any; execute: () => void };
//! type ChainW<T> = T extends Partial<ToolBody> ? { ok: true } : { ok: false };
//! interface ChatMessageProps { thing: ChainW<unknown> }
//! defineProps<ChatMessageProps>();
//! ```
//!
//! Pre-fix the publication path walks `thing: ChainW<unknown>` and
//! lowers the Conditional's `extends` (`Partial<ToolBody>`) under
//! the inherited `Published(Navigate)` outer context. The
//! `Partial<ToolBody>` Mapped enumerates `keyof ToolBody = 'outputSchema'
//! | 'execute'` and emits per-member ProjectMember edges. The leak
//! names live ONLY inside the Conditional's `extends` arm — they are
//! never on `ChainW<unknown>`'s own publication surface (the
//! conditional resolves to `{ ok: true } | { ok: false }`, neither
//! of which carries `outputSchema` or `execute`).
//!
//! ## Discrimination
//!
//! - **Pre-fix** (lower.rs lowers `extends` under inherited outer
//!   `reduction_context`): the per-prop publication path emits
//!   `ProjectMember` edges for `outputSchema` and `execute` —
//!   verified to fail with non-zero leak count.
//! - **Post-fix** (lower.rs lowers `extends` under
//!   `structural_transit_with_mode(outer.mode)`): no per-member
//!   edges emitted on the publication path; the conditional's
//!   relation check still operates because the relation engine
//!   navigates deferred shells via
//!   `evaluate_deferred_semantic_node_with_context`.

#![allow(clippy::too_many_lines, dead_code, unused_imports)]

use super::harness;

use verter_audit::origin_graph::{OriginEdgeKind, OriginEdgeMetaDto};
use verter_session::audited_request::AuditedRequest;

// Hand-derived AI-SDK shape. The `Tool` / `ToolOutputProperties` /
// `NeverOptional` triplet is the structural carrier: a Conditional
// (`NeverOptional`) whose extends arm `[N] extends [never]` enables
// the `Partial<Record<keyof T, undefined>>` arm — where T is the
// `{ execute, outputSchema? } | { outputSchema, execute?: never }`
// union from `ToolOutputProperties`. Each member name appears only
// in this file and would appear as a per-prop publication-path side-
// effect of eagerly lowering the Conditional's `extends` arm under
// `Published(Expanded)`. Any
// `ProjectMember` edge naming them on the publication path is
// unambiguously the Chain-W leak.
const AI_TYPES_TS: &str = r#"
// Carries the structural Chain-W trigger in minimum form: a
// Conditional whose `extends` arm contains a Mapped over a
// literal-union keyspace `{ outputSchema, execute }`. When this
// Conditional is lowered under the inherited outer
// `Published(Expanded)` context the
// `extends` lowering eagerly dispatches `Partial<{ outputSchema, execute }>`
// → `build_mapped_type` reaches `may_reduce_operator(Published(_)) ==
// true` → enumerates the literal keyspace → emits one ProjectMember
// edge per member.
//
// The discriminator must FAIL pre-fix and PASS post-fix; the post-
// fix path lowers the Conditional's `extends` under
// `StructuralTransit`, where `may_reduce_operator` evaluates `false`
// and the Mapped returns a deferred carrier instead of enumerating
// the keyspace.
export type ToolBody = { outputSchema: any; execute: () => void };
export type ChainW<T> = T extends Partial<ToolBody> ? { ok: true } : { ok: false };
"#;

// Mirrors the corpus ChatMessage shape: a generic interface
// extending `UIMessage<TTools>` consumed by
// `defineProps<X<TTools>>()`. The per-prop publication path
// iterates inherited members. A Conditional whose
// `extends` arm inherits the outer `Published(Expanded)` context
// would emit per-member edges for `outputSchema` / `execute`.
const CHAT_MESSAGE_VUE: &str = r#"<script lang="ts">
import type { ChainW } from './ai_types';

export interface ChatMessageProps {
  thing: ChainW<unknown>;
}
</script>
<script setup lang="ts">
defineProps<ChatMessageProps>();
</script>
<template><div></div></template>
"#;

#[test]
fn chain_w_conditional_extends_does_not_leak_record_keyspace_through_per_prop_publication() {
    let host = harness::build_hermetic_host_with_lib(
        &[
            ("/ai_types.ts", AI_TYPES_TS),
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

    // The leak names belong ONLY to the AI SDK `Tool` /
    // `ToolOutputProperties` shape and are never SFC-owned names.
    // Any `ProjectMember` edge naming them on the publication path
    // is unambiguously a Chain-W eager-expansion leak.
    const LEAK_MEMBERS: &[&str] = &["outputSchema", "execute"];

    let mut leak_edge_count = 0usize;
    let mut leak_edge_names: Vec<String> = Vec::new();
    for edge in footprint.derivation_subgraph.edges.iter() {
        if !matches!(edge.kind, OriginEdgeKind::ProjectMember) {
            continue;
        }
        if let OriginEdgeMetaDto::ProjectMember { member_name, .. } = &edge.meta {
            if LEAK_MEMBERS.contains(&member_name.as_ref()) {
                leak_edge_count += 1;
                leak_edge_names.push(member_name.to_string());
            }
        }
    }

    let mut leak_path_count = 0usize;
    for projection in footprint.projections.iter() {
        for seg in projection.path.iter() {
            if let verter_audit::origin_graph::ProjectPathSegment::Member { name } = seg {
                if LEAK_MEMBERS.contains(&name.as_ref()) {
                    leak_path_count += 1;
                }
            }
        }
    }

    let total = leak_edge_count + leak_path_count;

    assert_eq!(
        total, 0,
        "Chain W: the per-prop publication path MUST NOT reify \
         `ProjectMember` edges for keys that live only inside a \
         Conditional's `extends` arm. The `TypeExpr::Conditional` \
         lowering lowered `extends` under the inherited outer \
         `reduction_context`; eager `build_mapped_type` over the \
         `Partial<ToolBody>` keyspace enumerated `'outputSchema' | \
         'execute'` and emitted one `ProjectMember` edge per literal. \
         The Conditional's `check` and `extends` arguments are shape- \
         decision consumers, not publication consumers — they MUST \
         lower under `StructuralTransit` regardless of the outer \
         context, so nested `Instantiate` / `KeyOf` / `MappedType` \
         operators along the relation-input lowering frame propagate \
         the transit demand and `may_reduce_operator` evaluates \
         `false`. Got: leak_edges={leak_edge_count} \
         (names={leak_edge_names:?}), \
         projection_path_member_hits={leak_path_count}."
    );
}

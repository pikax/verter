//! Transit-Shallow Publication — primary gate.
//!
//! Counts `OriginEdgeKind::ProjectMember` edges in the audit's
//! derivation subgraph whose `OriginEdgeMetaDto::ProjectMember.member_name`
//! is `outputSchema` or `execute` — the exact pattern the ChatMessages
//! cold-seq corpus exposes. Hermetic SFC fixture that mimics
//! ChatMessages's slot shape (Mapped over `keyof` of an imported
//! generic interface with `outputSchema` + `execute` keys).
//!
//! ## Why this discriminates
//!
//! A publication path where `produce_one_macro_object_shape_for_slots`
//! lowers the slot expression at `ProjectionMode::Expanded`
//! (`project_expr_class_a_via_dispatch_threaded`) leaks: the Expanded
//! lowering reduces nested `BuiltinUtility` shells; reducing the
//! Mapped's keyspace `keyof Tool<I,O>` triggers `build_key_of` over
//! the instantiated `Tool<I,O>` Object, which calls
//! `intern_keyspace_names` and records one
//! `OriginEdgeKind::ProjectMember` edge per enumerated key (one for
//! `outputSchema`, one for `execute`).
//!
//! The publication helper instead lowers at
//! `structural_transit_with_mode(Navigate)`. The Mapped carrier-stops;
//! the KeyOf carrier-stops; `build_key_of` never reaches the
//! `intern_keyspace_names` arm; no ProjectMember edges with member
//! name `outputSchema` / `execute` reach the audit subgraph through
//! the producer path. The consumer (`compute_bindings_via_graph`)
//! reads members via the source-surface helper which dispatches
//! `ProjectPath { source, [], Published(Shallow) }` and walks the
//! Tool Object's surface directly (a member-surface read, not a
//! KeyOf reduction — no `intern_keyspace_names` involvement). The
//! `StructuralTransit(Navigate)` publication does not reduce KeyOf,
//! so the count is 0.

#![allow(clippy::too_many_lines, dead_code, unused_imports)]

use crate::harness;

use verter_audit::origin_graph::{OriginEdgeKind, OriginEdgeMetaDto};
use verter_session::audited_request::AuditedRequest;

const TOOL_TS: &str = r#"
export interface Tool<INPUT = unknown, OUTPUT = unknown> {
  outputSchema: OUTPUT;
  execute: (input: INPUT) => OUTPUT;
}
"#;

// ChatMessages-shape SFC: a defineSlots whose slot type is a Mapped
// over `keyof Tool<I,O>`. The HEAD publication path lowers this Mapped
// at Expanded, reducing the KeyOf and firing intern_keyspace_names ⇒
// `outputSchema` + `execute` `ProjectMember` edges land in the audit
// derivation subgraph.
const CHAT_MESSAGES_VUE: &str = r#"<script setup lang="ts" generic="I, O">
import type { Tool } from './tool';
defineSlots<{
  [K in keyof Tool<I, O>]?: (props: { schema: Tool<I, O>[K] }) => unknown
}>();
</script>
<template><div></div></template>
"#;

#[test]
fn chatmessages_shape_audit_has_zero_outputschema_execute_project_member_edges() {
    let host = harness::build_hermetic_host(&[
        ("/tool.ts", TOOL_TS),
        ("/ChatMessages.vue", CHAT_MESSAGES_VUE),
    ]);

    let (_analysis, _resolved, audit) = AuditedRequest::builder()
        .attach_to(host)
        .resolve_component_meta("/ChatMessages.vue")
        .expect("hermetic resolve must succeed");

    let footprint = audit
        .footprint
        .as_ref()
        .expect("footprint_capture is enabled in this harness");

    // Sum ProjectMember edges in the derivation subgraph whose
    // member_name is one of the leak keys.
    //
    // R18 — scope the leak counter to intermediate
    // provenances (`PathProjection` / `KeyOfEnumerated` /
    // `MappedKeyEnumerated`). The `PublishedField` provenance is the
    // producer-side declaration "this member is admitted to the
    // user-visible surface" and is the audit signal Rule-5 USES, not a
    // leak it observes. Counting `PublishedField` edges here would
    // criminalise the legitimate publication of a `defineSlots`
    // surface whose declared slot names happen to be `outputSchema` /
    // `execute` — that publication is the user's intent.
    let mut outputschema_count = 0usize;
    let mut execute_count = 0usize;
    for edge in footprint.derivation_subgraph.edges.iter() {
        if !matches!(edge.kind, OriginEdgeKind::ProjectMember) {
            continue;
        }
        if let OriginEdgeMetaDto::ProjectMember {
            member_name,
            provenance,
        } = &edge.meta
        {
            if matches!(
                provenance,
                verter_audit::MemberEdgeProvenance::PublishedField
            ) {
                continue;
            }
            match member_name.as_ref() {
                "outputSchema" => outputschema_count += 1,
                "execute" => execute_count += 1,
                _ => {}
            }
        }
    }

    // Also scan the `projections` records (paths containing
    // `ProjectPathSegment::Member { name }`) — the diagnostic report
    // showed both shapes contribute to the corpus grep count.
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
        "Transit-Shallow Publication primary gate — the ChatMessages-shape \
         hermetic fixture MUST NOT emit any `outputSchema` / `execute` ProjectMember \
         edges through the audit derivation subgraph or projections. Got: \
         outputSchema(edges)={outputschema_count}, execute(edges)={execute_count}, \
         projection_path_member_hits={projection_path_count}. \
         An Expanded publication path fires `build_key_of` over \
         the imported `Tool` interface, reducing `keyof Tool` to a literal-anchor union \
         and recording one `ProjectMember` edge per enumerated key. The publication runs \
         on `StructuralTransit(Navigate)` which \
         carrier-stops the KeyOf; the consumer reads source members via the source-surface \
         helper without invoking KeyOf reduction.",
    );
}

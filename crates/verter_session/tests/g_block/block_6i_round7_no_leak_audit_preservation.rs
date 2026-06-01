//! Block 6.i Round 7 — discriminator: **no-leak audit preservation**.
//!
//! Regression guard locked at the round-7 boundary. The substrate
//! extensions in Commit 2 (selected-key mapped helper + diagnostic
//! probe + branch-merged shallow primitive) are **non-publishing**
//! additions: they extend the substrate without touching the macro
//! publication path. Hermetic fixtures whose audit footprint already
//! had 0 leak-key ProjectMember edges at HEAD `ed969b44a` MUST keep
//! that 0 across every round-7 commit.
//!
//! ## Fixture choice
//!
//! Uses a literal-union-keyed `defineSlots<{ [K in 'alpha' | 'beta']?: ... }>()`.
//! At HEAD the empty-path Shallow publication enumerates the
//! literal-union keys directly through `collect_literal_keys` (no
//! `keyof` reduction, no `intern_keyspace_names`) so the audit
//! derivation subgraph publishes 0 ProjectMember edges whose member
//! name matches the leak keys (`outputSchema` / `execute`).
//!
//! This is distinct from the PRIMARY gate
//! (`block_6i_leak_chatmessages_audit`) which exercises a `keyof
//! Tool<I,O>`-keyed Mapped whose Expanded publication path DOES leak
//! at HEAD (8 edges) and only closes after Commit 3's atomic cutover.
//! Round 7's substrate must not push edges INTO this hermetic
//! literal-union fixture either — that would mean the selected-key
//! helper widened beyond the codex Q6 scope ("branch-merged shallow
//! conditional surfaces may widen a non-macro shallow surface unless
//! scoped").
//!
//! ## Discrimination progression
//!
//! - **Commit 1 (no substrate extensions):** PASS — literal-union
//!   keyspace; the publication's `collect_literal_keys` path emits 0
//!   ProjectMember edges for `outputSchema|execute`.
//! - **Commit 2 (selected-key helper + diagnostic probe + branch-merge):**
//!   PASS — substrate extensions are non-publishing; the hermetic
//!   count stays 0. If the selected-key helper inadvertently widens
//!   to a member-projection cascade that emits leak-key edges, this
//!   test fails at Commit 2 BEFORE the cutover ever lands.
//! - **Commit 3 (atomic cutover):** PASS — cutover keeps the
//!   literal-union path's contract.

#![allow(clippy::too_many_lines, dead_code, unused_imports)]

use crate::harness;

use verter_audit::origin_graph::{OriginEdgeKind, OriginEdgeMetaDto};
use verter_session::audited_request::AuditedRequest;

// Literal-union-keyed Mapped defineSlots. The publication's
// `collect_literal_keys` path enumerates 'alpha' | 'beta' directly;
// neither `outputSchema` nor `execute` ever appears as a member name
// in the derivation subgraph for THIS fixture.
const LITERAL_UNION_SFC: &str = r#"<script setup lang="ts">
defineSlots<{
  [K in 'alpha' | 'beta']?: (props: { label: string }) => unknown
}>();
</script>
<template><div></div></template>
"#;

#[test]
fn round7_literal_union_slots_audit_stays_at_zero_outputschema_execute_edges() {
    let host = harness::build_hermetic_host(&[("/LiteralUnionSlots.vue", LITERAL_UNION_SFC)]);

    let (_analysis, _resolved, audit) = AuditedRequest::builder()
        .attach_to(host)
        .resolve_component_meta("/LiteralUnionSlots.vue")
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
        "Block 6.i Round 7 — substrate extensions MUST NOT widen the hermetic \
         literal-union-keyed defineSlots audit. The selected-key mapped helper + \
         diagnostic probe + branch-merge primitive are non-publishing additions \
         and the literal-union publication path never reduces `keyof` (no \
         `intern_keyspace_names` invocation). A leak here means the selected-key \
         helper inadvertently widened beyond the codex Q6 scope. Got: \
         outputSchema(edges)={outputschema_count}, execute(edges)={execute_count}, \
         projection_path_member_hits={projection_path_count}.",
    );
}

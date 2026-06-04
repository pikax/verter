//! Type-resolution audit — cache reuse across entry-points.
//!
//! Drives the same query through (1) the component-meta entry-point
//! (which causes the cold resolver to run end-to-end through every
//! shared cache layer) AND (2) `resolve_type_with_audit` for the
//! same query. The second request must surface a payload whose
//! `expansions` is small / zero, demonstrating that the shared
//! semantic state is REUSED — not rebuilt — across entry-points
//! (CLAUDE.md §"Shared Optimized Codebase").
//!
//! Discrimination contract: a regression that gave the
//! type-resolution surface a private cache (a "second resolver")
//! would surface non-zero expansions on the second request even
//! though the component-meta entry already populated the shared
//! semantic graph. The post-change tree shares one
//! `SemanticGraphStore` across surfaces, so the second request
//! short-circuits via the warm memo entry.

use std::sync::Arc;

use verter_session::semantic_query::{ResolveDeclKey, ScopeId, ScopeKind, SemanticQueryKey};
use verter_session::{FileKind, HostConfig, UpsertRequest, VerterHost};

const SFC: &str = r#"<script setup lang="ts">
type Inner = { value: string };
type Outer = { inner: Inner; depth: number };
defineProps<Outer>()
</script>
<template><div>{{ depth }}</div></template>
"#;

#[test]
fn type_resolution_audit_shared_graph_reused_across_entry_points() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig {
        audit_enabled: true,
        footprint_capture: true,
        ..HostConfig::default()
    }));
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/Probe.vue".to_string()),
        input_id: "/Probe.vue".to_string(),
        source: Arc::from(SFC),
        file_kind: FileKind::from_path("/Probe.vue"),
        aliases: Vec::new(),
    });

    // First entry-point: component-meta — exercises the cold
    // resolver pipeline end-to-end and warms the shared semantic
    // graph.
    let (analysis, _resolution) = host
        .get_component_meta_with_resolution("/Probe.vue")
        .expect("component-meta must resolve for the fixture");
    assert!(
        !analysis.props.is_empty(),
        "component-meta resolution must populate at least one prop — \
         the fixture defines defineProps<Outer>"
    );

    let memo_after_meta = host
        .project_type_store()
        .semantic_graph()
        .memo_entry_count();

    // Second entry-point: `resolve_type_with_audit` for `Outer`
    // (referenced by the SFC's `defineProps` macro). The
    // component-meta cold resolver had to materialise `Outer`'s
    // declaration, so the shared semantic graph already carries a
    // memo entry for `ResolveDecl(Outer)`. The second request must
    // hit that entry without allocating another.
    let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: ScopeId {
            canonical_id: Arc::from("/Probe.vue"),
            local_scope: None,
            kind: ScopeKind::File,
        },
        name: Arc::from("Outer"),
    });
    let (resolved, record) = host.resolve_type_with_audit(key, "/Probe.vue").into_parts();
    let resolved = resolved
        .expect("Outer must resolve through type-resolution surface")
        .expect("resolved node must be present");
    // record is always present now (carrier `audit` mandatory).
    let payload = record
        .type_resolution_payload()
        .expect("kind must be TypeResolution");

    let memo_after_second = host
        .project_type_store()
        .semantic_graph()
        .memo_entry_count();

    // Shared substrate: the memo MUST NOT have grown to cover a
    // separate `Outer` entry — the component-meta path already
    // populated the entry the second request needs.
    let memo_growth = memo_after_second.saturating_sub(memo_after_meta);
    assert!(
        memo_growth <= 1,
        "second entry-point must NOT grow the shared memo by more than one \
         entry (the bare ResolveDecl key itself). \
         memo_after_meta = {memo_after_meta}, memo_after_second = {memo_after_second}, \
         growth = {memo_growth}. Pre-change tree (private cache regression) would \
         surface a much larger growth here because the second resolver would \
         re-walk Inner / Outer's bodies."
    );
    let _ = resolved;

    // The second request's expansions counter must be small —
    // ideally zero. Discriminate vs the regression by asserting it
    // is strictly bounded.
    assert!(
        payload.expansions <= memo_growth as u32,
        "second request must reuse the warm memo, not allocate fresh expansions. \
         expansions = {}, memo_growth = {memo_growth}",
        payload.expansions
    );
}

//! F1 (Plan §3 Step 3) — `origin_graph` audit-only contract.
//!
//! Discriminator strategy: at HEAD `b8e0e1b4`, the materialization
//! path that produces component-meta does NOT route through
//! `ProjectSemanticDispatch::execute` (that's the Step 6 lift), so
//! `record_origin_edge` does not fire during normal `getComponentMeta`
//! resolution and `export_all_origin_edges()` returns an empty set
//! for every realistic fixture. A runtime-discriminator that asserts
//! `origin_graph.is_some()` in the audit-on case would be unprovable
//! pre-Step-6 and is therefore not used here.
//!
//! Instead the Step 3 discriminator is split:
//!
//! 1. **Static-text discriminator** (`gate_text_includes_audit_enabled`)
//!    — asserts the code at `meta_resolve.rs` contains the
//!    `audit_enabled && self.config.footprint_capture` guard. Pre-fix
//!    the guard isn't there; post-fix it is. This is mechanically
//!    discriminating against any tree that loses the gate.
//!
//! 2. **Forward-looking regression invariants** (the `_runtime_*`
//!    tests) — exercise the audit-off and audit-only paths end to end
//!    with realistic fixtures. They currently pass both pre- and
//!    post-fix because the dispatch path that populates origin edges
//!    isn't routed yet, but they become discriminators once Step 6
//!    routes materialization through dispatch and edges populate.
//!    Today they protect against regressions that would crash or
//!    surface the field unconditionally on a non-empty graph.

use std::sync::Arc;

use verter_session::{FileKind, HostConfig, UpsertRequest, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

const FIXTURE_VUE: &str = "<script setup lang=\"ts\">\n\
import type { ResolvedProps } from './props';\n\
defineProps<ResolvedProps>();\n\
</script>\n\
<template><div /></template>\n";

const FIXTURE_PROPS_TS: &str = "interface Base<T> {\n\
    label: T;\n\
    count: number;\n\
    nested: { value: T };\n\
}\n\
\n\
type Names = 'name' | 'title';\n\
\n\
export type ResolvedProps = Base<string> & { kind: Names };\n";

fn host_with(audit_enabled: bool, footprint_capture: bool) -> Arc<VerterHost> {
    let workspace: Arc<dyn WorkspaceAccess> =
        Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    let host = Arc::new(VerterHost::new(
        HostConfig {
            audit_enabled,
            footprint_capture,
            ..HostConfig::default()
        },
        workspace,
    ));
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/props.ts".into()),
        input_id: "/props.ts".into(),
        source: Arc::from(FIXTURE_PROPS_TS),
        file_kind: FileKind::NonSfc,
        aliases: vec![],
    });
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/Component.vue".into()),
        input_id: "/Component.vue".into(),
        source: Arc::from(FIXTURE_VUE),
        file_kind: FileKind::VueSfc,
        aliases: vec![],
    });
    host
}

#[test]
fn gate_text_includes_audit_enabled() {
    // FAIL-FIRST static-text discriminator. The guard is the literal
    // gate `audit_enabled && self.config.footprint_capture`. Pre-fix
    // this expression is absent; post-fix it is present at exactly one
    // location (the `origin_graph:` field of `ResolvedComponentMetaState`
    // construction in `compute_component_meta_state_inner`).
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("meta_resolve.rs");
    let body = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read {} failed: {e}", path.display()));
    let needle = "audit_enabled\n                && self.config.footprint_capture";
    assert!(
        body.contains(needle),
        "F1 contract: meta_resolve.rs must contain the gate \
         `audit_enabled && self.config.footprint_capture` for `origin_graph` \
         emission. Pre-fix that gate is absent and `origin_graph` would emit \
         under any audit configuration. (Plan §3 Step 3 D3 + D34.)",
    );
}

#[test]
fn runtime_audit_off_does_not_emit_origin() {
    // Regression invariant: with audit_enabled=false, origin_graph
    // is None whether edges are populated or not. Today this passes
    // both pre- and post-fix because the dispatch graph isn't being
    // populated for component-meta requests. Post-Step-6 (when
    // materialization routes through dispatch and edges populate),
    // this becomes the runtime FAIL-FIRST discriminator: pre-fix
    // returns Some(dto), post-fix returns None.
    let host = host_with(false, false);
    let (_meta, resolved) = host
        .get_component_meta_with_resolution("/Component.vue")
        .expect("component meta resolves");
    assert!(
        resolved.origin_graph.is_none(),
        "audit_enabled=false must suppress origin_graph emission",
    );
}

#[test]
fn runtime_audit_on_no_footprint_does_not_emit_origin() {
    // Regression invariant for D34: gating on `audit_enabled` alone
    // is not enough — `footprint_capture` must also be true (matches
    // the LSP hover-provenance gate). HostConfig::validate forbids
    // footprint_capture without audit_enabled, so this is the only
    // representable "audit halfway on" combination.
    let host = host_with(true, false);
    let (_meta, resolved) = host
        .get_component_meta_with_resolution("/Component.vue")
        .expect("component meta resolves");
    assert!(
        resolved.origin_graph.is_none(),
        "footprint_capture=false must suppress origin_graph emission \
         even when audit_enabled=true",
    );
}

#[test]
fn runtime_audit_fully_on_completes_without_panic() {
    // Regression invariant: enabling both audit_enabled and
    // footprint_capture must not crash the resolution path. The
    // origin_graph may be None today (no edges yet — see file
    // header), but the resolution must succeed and the audit
    // record must be reachable.
    let host = host_with(true, true);
    let (_meta, resolved) = host
        .get_component_meta_with_resolution("/Component.vue")
        .expect("component meta resolves under audit_enabled+footprint_capture");
    let _ = resolved.origin_graph; // structurally accessible
    assert_ne!(
        resolved.request_id, 0,
        "audit-on resolution must stamp a non-zero request_id",
    );
}

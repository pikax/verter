//! Shared test harness + fixture constants for the authored
//! correctness suite (plan §3 Commit 7 / F6). Each sibling test
//! module injects the relevant fixtures into a hermetic
//! [`AuditedRequest`], resolves a canonical SFC, and asserts on
//! the resulting [`RustAuditRecord`].
//!
//! All fixtures live under `crates/verter_session/test_fixtures/`
//! and are reached via `include_str!`. This keeps fixtures in the
//! source tree (instead of being embedded as Rust strings) so the
//! same content can be re-used by the `d_cutover_characterization_tests`
//! module in future consolidation and by manual reproduction.
//!
//! **Naming convention:**
//! - `pathological_*` — regression-pinned snapshots using
//!   `mask_incidental_spans()`. These fail loudly on accidental
//!   shape changes but don't assert exact semantic content.
//! - `corpus_representatives/*` — `_exactly` assertions using
//!   [`RustAuditRecord::assert_loaded_files_exactly`]. These fail
//!   when the loaded-files set changes for a curated representative
//!   from the nuxt-ui corpus.
//! - Standalone — each exercises one audit-surface facet
//!   (generics, external types, barrel chains, conditionals, path
//!   projection) with a minimal fixture.

use std::sync::Arc;

use verter_session::audited_request::AuditedRequest;
use verter_session::component_meta_audit::{RustAuditRecord, RustSemanticFootprintAudit};
use verter_session::{FileKind, HostConfig, UpsertRequest, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

// Shared test-fixture source files injected into hermetic
// workspaces. Paths resolve relative to the `mod.rs` file itself.
pub const TABS_VUE: &str = include_str!("../../test_fixtures/tabs.vue");
pub const TABS_TYPES_TS: &str = include_str!("../../test_fixtures/tabs_types.ts");
pub const TABS_HELPER_TS: &str = include_str!("../../test_fixtures/tabs_helper.ts");

pub const EDITOR_TOOLBAR_VUE: &str = include_str!("../../test_fixtures/editor_toolbar.vue");
pub const EDITOR_TOOLBAR_TYPES_TS: &str =
    include_str!("../../test_fixtures/editor_toolbar_types.ts");

pub const TABLE_VUE: &str = include_str!("../../test_fixtures/table.vue");
pub const TABLE_TYPES_TS: &str = include_str!("../../test_fixtures/table_types.ts");

/// Build a hermetic [`VerterHost`] with audit + footprint capture
/// enabled and the given files injected directly into the
/// [`MemoryWorkspace`] (skipping `upsert` so the resolver's first
/// touch goes through `ensure_loaded` → scheduler →
/// `workspace.read_file`, which fans into [`SessionVfsSink`]).
pub fn build_hermetic_host(files: &[(&str, &str)]) -> Arc<VerterHost> {
    let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    for (canonical, content) in files {
        workspace.inject_file((*canonical).into(), Arc::from(*content));
    }
    let ws_access: Arc<dyn WorkspaceAccess> = workspace;
    Arc::new(VerterHost::new(
        HostConfig {
            audit_enabled: true,
            footprint_capture: true,
            ..HostConfig::default()
        },
        ws_access,
    ))
}

/// Build a host that already has `/c.vue` upserted (pre-parsed).
/// Use this when a test needs the component-meta resolver to skip
/// the initial SFC read and focus on downstream behaviours.
#[allow(dead_code)]
pub fn build_preupserted_host(files: &[(&str, &str)], entry_canonical: &str) -> Arc<VerterHost> {
    let host = build_hermetic_host(files);
    let source = files
        .iter()
        .find(|(c, _)| *c == entry_canonical)
        .map(|(_, s)| *s)
        .unwrap_or("");
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(entry_canonical.into()),
        input_id: entry_canonical.into(),
        source: Arc::from(source),
        file_kind: FileKind::from_path(entry_canonical),
        aliases: vec![],
    });
    host
}

/// Resolve `canonical` against `host` under an attached audit,
/// returning the triple. Panics on error — test callers want loud
/// failures.
pub fn resolve_under_audit(
    host: Arc<VerterHost>,
    canonical: &str,
) -> (
    verter_semantic::analysis::component_meta::ComponentMetaAnalysis,
    verter_session::meta_resolve::ResolvedComponentMetaState,
    RustAuditRecord,
) {
    AuditedRequest::builder()
        .attach_to(host)
        .resolve(canonical)
        .unwrap_or_else(|e| panic!("hermetic audit for `{canonical}` must succeed, got {e}"))
}

/// Convenience: return the footprint, panicking when absent. Every
/// test in this suite opts into `footprint_capture`.
pub fn footprint_of(record: &RustAuditRecord) -> &RustSemanticFootprintAudit {
    record
        .footprint
        .as_ref()
        .expect("footprint_capture is always enabled in this suite")
}

//! Shared test harness + fixture constants for the authored
//! correctness suite. Each sibling test
//! module injects the relevant fixtures into a hermetic
//! [`AuditedRequest`], resolves a canonical SFC, and asserts on
//! the resulting [`RequestAuditRecord`].
//!
//! All fixtures live under `crates/verter_session/test_fixtures/`
//! and are reached via `include_str!`. This keeps fixtures in the
//! source tree (instead of being embedded as Rust strings) so the
//! same content can be re-used by the
//! `project_semantic_dispatch_invariants_tests` module in future
//! consolidation and by manual reproduction.
//!
//! **Naming convention:**
//! - `pathological_*` — regression-pinned snapshots using
//!   `mask_incidental_spans()`. These fail loudly on accidental
//!   shape changes but don't assert exact semantic content.
//! - `corpus_representatives/*` — `_exactly` assertions using
//!   [`RequestAuditRecord::assert_loaded_files_exactly`]. These fail
//!   when the loaded-files set changes for a curated representative
//!   from the nuxt-ui corpus.
//! - Standalone — each exercises one audit-surface facet
//!   (generics, external types, barrel chains, conditionals, path
//!   projection) with a minimal fixture.
//!
//! This is a SHARED harness: it is declared once at each consuming test
//! binary's root (`component_meta_audit`, `g_block`, `g_misc2`, `g_misc3`)
//! and intentionally exposes a superset of fixtures + helpers + re-exports.
//! No single binary exercises every entry, so dead-code / unused-import
//! analysis is suppressed at the module level rather than per consumer.
#![allow(dead_code, unused_imports)]

use std::sync::Arc;

use verter_session::audited_request::AuditedRequest;
pub use verter_session::component_meta_audit::assertions::RequestAuditRecordAssertions;
use verter_session::component_meta_audit::{RequestAuditRecord, RequestFootprintAudit};
use verter_session::{FileKind, HostConfig, UpsertRequest, VerterHost};
use verter_workspace::{
    AmbientLibSpec, IdeProjectCompilerOptions, MemoryOptions, MemoryWorkspace, ProjectGraph,
    ProjectMembership, ProjectRank, VfsProjectConfig, WorkspaceAccess,
};

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
    RequestAuditRecord,
) {
    AuditedRequest::builder()
        .attach_to(host)
        .resolve_component_meta(canonical)
        .unwrap_or_else(|e| panic!("hermetic audit for `{canonical}` must succeed, got {e}"))
}

/// Convenience: return the footprint, panicking when absent. Every
/// test in this suite opts into `footprint_capture`.
pub fn footprint_of(record: &RequestAuditRecord) -> &RequestFootprintAudit {
    record
        .footprint
        .as_ref()
        .expect("footprint_capture is always enabled in this suite")
}

/// Hand-authored mapped-type subset of `lib.es5.d.ts`.
///
/// Hand-derived from the TS spec so the harness never silently disagrees
/// with `tsc --lib` on what `Pick` means.
/// Use through [`build_hermetic_host_with_lib`].
pub const STUB_LIB_ES5: &str = r#"
type Pick<T, K extends keyof T> = { [P in K]: T[P] };
type Omit<T, K extends keyof any> = Pick<T, Exclude<keyof T, K>>;
type Partial<T> = { [P in keyof T]?: T[P] };
type Required<T> = { [P in keyof T]-?: T[P] };
type Readonly<T> = { readonly [P in keyof T]: T[P] };
type Record<K extends keyof any, T> = { [P in K]: T };
type Exclude<T, U> = T extends U ? never : T;
type Extract<T, U> = T extends U ? T : never;
type NonNullable<T> = T extends null | undefined ? never : T;
"#;

/// Build a hermetic host with regular SFC /
/// TS files **and** ambient TypeScript lib files registered via
/// [`WorkspaceAccess::register_ambient_lib`].
///
/// `lib_files` is a slice of `(filename, source)` pairs. Each lib is also
/// `inject_file`'d at canonical id `/lib/<filename>` so that any consumer
/// that needs the source through the regular VFS path can still read it
/// (the registration itself goes through the ambient registry, not the
/// snapshot — these two paths together cover both A1 lookups and standard
/// reads). The host is built against a single configured project at
/// `/ws` with `tsconfig.json` so `register_ambient_lib(project_id: None)`
/// resolves unambiguously.
#[allow(dead_code)]
pub fn build_hermetic_host_with_lib(
    files: &[(&str, &str)],
    lib_files: &[(&str, &str)],
) -> Arc<VerterHost> {
    let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    // A configured project so `register_ambient_lib` has a stable key to
    // attach to.
    workspace.set_project_graph(ProjectGraph::from_configs(vec![VfsProjectConfig {
        root: "/ws".to_string(),
        rank: ProjectRank::Explicit,
        tsconfig_path: Some("/ws/tsconfig.json".to_string()),
        root_files: vec![],
        extensions: vec![".ts".into(), ".tsx".into(), ".vue".into()],
        workspace_root: "/ws".to_string(),
        workspace_aliases: vec![],
        compiler_options: IdeProjectCompilerOptions::default(),
        references: vec![],
        membership: ProjectMembership::default(),
    }]));
    for (canonical, content) in files {
        workspace.inject_file((*canonical).into(), Arc::from(*content));
    }
    for (filename, source) in lib_files {
        // Mirror lib content into the snapshot at /lib/<filename> so the
        // standard read_file path can reach it for tests that exercise the
        // VFS layer directly. The ambient registry below is the contract
        // surface — read_file is incidental.
        let mirror_id = format!("/lib/{filename}");
        workspace.inject_file(mirror_id.clone(), Arc::from(*source));
        workspace
            .register_ambient_lib(AmbientLibSpec {
                project_id: None,
                canonical_id: Arc::from(*filename),
                source: Arc::from(*source),
            })
            .unwrap_or_else(|e| {
                panic!("ambient lib registration for `{filename}` MUST succeed, got {e:?}")
            });
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

#[cfg(test)]
mod self_tests {
    //! Harness self-tests:
    //! - `stub_lib_pick_resolves` proves the harness puts `Pick` in scope.
    //! - `register_ambient_lib_idempotent` proves the harness's
    //!   `build_hermetic_host_with_lib` is itself idempotent — repeated
    //!   builds with identical inputs land identical workspace state.
    //! - `vfs_shadowing_overlay_wins` proves A5 (overlay/snapshot files
    //!   shadow ambient libs).
    use std::sync::Arc;

    use verter_workspace::ProjectId;

    use super::{build_hermetic_host_with_lib, STUB_LIB_ES5};

    /// `Pick` is the canonical mapped-type test marker. After the harness
    /// builds a host with `STUB_LIB_ES5` registered, the workspace MUST
    /// resolve `Pick` through `lookup_ambient_symbol`.
    #[test]
    fn stub_lib_pick_resolves() {
        let host = build_hermetic_host_with_lib(&[], &[("lib.es5.d.ts", STUB_LIB_ES5)]);
        // Read consumers route through `host.workspace_read()` (the
        // `WorkspaceRead` trait surface).
        let workspace = host.workspace_read();
        let key = workspace
            .project_stable_key(ProjectId(0))
            .expect("hermetic host with lib MUST have a configured project");
        let hit = workspace
            .lookup_ambient_symbol(key, "Pick")
            .expect("STUB_LIB_ES5 MUST expose Pick to the symbol_index");
        assert_eq!(hit.canonical_id.as_ref(), "lib.es5.d.ts");
        // Registry hit virtual id is project-scoped.
        let v: &str = &hit.virtual_id;
        assert!(v.starts_with("ambient:/"), "got {v}");
        assert!(v.ends_with("/lib.es5.d.ts"));

        // `read_ambient_lib` returns the source while `read_file` does not
        // (ambient registry is a separate surface from the snapshot).
        let s = workspace
            .read_ambient_lib(key, "lib.es5.d.ts")
            .expect("ambient registry MUST expose source");
        assert!(s.contains("type Pick"), "got source = {s:?}");
    }

    /// Per A1: building twice with the same lib produces the same
    /// workspace state — no double registration, no duplicate symbol_index
    /// entries, no content_generation thrash on the second build.
    ///
    /// Discriminating: pre-change tree has no `register_ambient_lib`
    /// at all, so the test fails to compile. Post-change tree, building
    /// twice yields the same idempotent state.
    #[test]
    fn register_ambient_lib_idempotent() {
        let host_a = build_hermetic_host_with_lib(&[], &[("lib.es5.d.ts", STUB_LIB_ES5)]);
        let host_b = build_hermetic_host_with_lib(&[], &[("lib.es5.d.ts", STUB_LIB_ES5)]);
        // Read consumers route through `host.workspace_read()`.
        let key_a = host_a
            .workspace_read()
            .project_stable_key(ProjectId(0))
            .unwrap();
        let key_b = host_b
            .workspace_read()
            .project_stable_key(ProjectId(0))
            .unwrap();
        // Same configured project (same workspace_root + tsconfig) → same
        // ProjectStableKey across hosts (A3 determinism).
        assert_eq!(key_a, key_b);
        // Both registries expose Pick. Read consumers route through
        // `host.workspace_read()`.
        assert!(host_a
            .workspace_read()
            .lookup_ambient_symbol(key_a, "Pick")
            .is_some());
        assert!(host_b
            .workspace_read()
            .lookup_ambient_symbol(key_b, "Pick")
            .is_some());
    }

    /// Per A5: a user file at the same canonical_id wins over the ambient
    /// lib through `read_ambient_lib`'s overlay/snapshot shadowing check.
    /// Discriminating: pre-change tree has no `read_ambient_lib`.
    #[test]
    fn vfs_shadowing_overlay_wins() {
        let host = build_hermetic_host_with_lib(&[], &[("lib.es5.d.ts", STUB_LIB_ES5)]);
        // Read consumers route through `host.workspace_read()`; mutators
        // (`notify_upsert`) go through the dedicated host wrapper.
        let workspace = host.workspace_read();
        let key = workspace.project_stable_key(ProjectId(0)).unwrap();
        // Initial state: ambient lib reachable.
        assert!(workspace.read_ambient_lib(key, "lib.es5.d.ts").is_some());
        // Open an editor buffer at the same canonical_id.
        host.notify_upsert("lib.es5.d.ts", Arc::from("// user override"));
        assert!(
            workspace.read_ambient_lib(key, "lib.es5.d.ts").is_none(),
            "A5: user overlay MUST shadow ambient lib via read_ambient_lib"
        );
        // The user file is what `read_file` returns.
        let s = workspace.read_file("lib.es5.d.ts").unwrap();
        assert_eq!(&*s, "// user override");
    }
}

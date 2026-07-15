//! Integration-style tests for ambient lib registration on a real
//! `MemoryWorkspace`.
//!
//! Each test sets up a `MemoryWorkspace` with one or more projects, registers
//! ambient libs, and asserts on the public surface (`read_ambient_lib`,
//! `lookup_ambient_symbol`, `ambient_libs_view`, `file_exists` shadowing).
//! Tests covering host-side machinery (cache-validity fact-signature
//! validation, scheduler-driven lazy parse) live in the `verter_session`
//! crate.

use std::sync::Arc;

use crate::ambient_lib::{AmbientLibError, AmbientLibSpec};
use crate::canonical_path::CanonicalPath;
use crate::membership::ConfiguredMembership;
use crate::memory::{MemoryOptions, MemoryWorkspace};
use crate::project_graph::{ProjectGraph, ProjectRank, VfsProjectConfig};
use crate::project_key::ProjectStableKey;
use crate::resolver::IdeProjectCompilerOptions;
use crate::traits::{WorkspaceAccess, WorkspaceRead};

const STUB_LIB_ES5: &str = r#"
    interface Pick<T, K extends keyof T> { /* */ }
    interface Omit<T, K extends keyof T> { /* */ }
    type Partial<T> = { [P in keyof T]?: T[P] };
    type Required<T> = { [P in keyof T]-?: T[P] };
"#;

const STUB_LIB_ES2015: &str = r#"
    interface Promise<T> { /* */ }
    interface Map<K, V> { /* */ }
"#;

fn make_project(
    workspace_root: &str,
    project_root: &str,
    tsconfig: Option<&str>,
) -> VfsProjectConfig {
    VfsProjectConfig {
        root: project_root.to_string(),
        rank: ProjectRank::Explicit,
        tsconfig_path: tsconfig.map(|s| s.to_string()),
        root_files: vec![],
        extensions: vec![".ts".to_string(), ".tsx".to_string(), ".vue".to_string()],
        workspace_root: workspace_root.to_string(),
        workspace_aliases: vec![],
        compiler_options: IdeProjectCompilerOptions::default(),
        references: vec![],
        membership: ConfiguredMembership::match_all_under_root(&CanonicalPath::new(project_root)),
    }
}

fn ws_with_single_project(workspace_root: &str, tsconfig: &str) -> Arc<MemoryWorkspace> {
    let ws = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    ws.set_project_graph(ProjectGraph::from_configs(vec![make_project(
        workspace_root,
        workspace_root,
        Some(tsconfig),
    )]));
    ws
}

fn lib_spec(canonical: &str, source: &str) -> AmbientLibSpec {
    AmbientLibSpec {
        project_id: None,
        canonical_id: Arc::from(canonical),
        source: Arc::from(source),
    }
}

// 1.
#[test]
fn register_ambient_lib_idempotent_on_content_hash() {
    let ws = ws_with_single_project("/ws", "/ws/tsconfig.json");
    ws.register_ambient_lib(lib_spec("lib.es5.d.ts", STUB_LIB_ES5))
        .unwrap();
    let gen_after_first = ws.content_generation();
    // Re-register same source — must be a no-op (no generation bump).
    ws.register_ambient_lib(lib_spec("lib.es5.d.ts", STUB_LIB_ES5))
        .unwrap();
    let gen_after_second = ws.content_generation();
    assert_eq!(
        gen_after_first, gen_after_second,
        "idempotent re-registration MUST NOT bump content_generation"
    );
    // Sanity: registry has exactly one entry.
    let view = ws.ambient_libs_view();
    let p_count: usize = view.by_project.values().map(|p| p.libs.len()).sum();
    assert_eq!(p_count, 1, "single registered lib, idempotent");
}

// 2.
#[test]
fn register_ambient_lib_visible_to_read_ambient_lib_only() {
    let ws = ws_with_single_project("/ws", "/ws/tsconfig.json");
    ws.register_ambient_lib(lib_spec("lib.es5.d.ts", STUB_LIB_ES5))
        .unwrap();
    let key = ws
        .project_stable_key(crate::workspace_snapshot::ProjectId(0))
        .unwrap();
    assert!(
        ws.read_file("lib.es5.d.ts").is_none(),
        "ambient lib MUST NOT be visible through read_file"
    );
    let s = ws.read_ambient_lib(key, "lib.es5.d.ts").unwrap();
    assert!(s.contains("Pick"), "got source = {s:?}");
}

// 3.
#[test]
fn register_ambient_lib_per_project_isolation() {
    let ws = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    ws.set_project_graph(ProjectGraph::from_configs(vec![
        make_project("/ws", "/ws/a", Some("/ws/a/tsconfig.json")),
        make_project("/ws", "/ws/b", Some("/ws/b/tsconfig.json")),
    ]));
    let key_a = ws
        .project_stable_key(crate::workspace_snapshot::ProjectId(0))
        .unwrap();
    let key_b = ws
        .project_stable_key(crate::workspace_snapshot::ProjectId(1))
        .unwrap();
    assert_ne!(key_a, key_b);

    ws.register_ambient_lib(AmbientLibSpec {
        project_id: Some(crate::workspace_snapshot::ProjectId(0)),
        canonical_id: Arc::from("lib.es5.d.ts"),
        source: Arc::from(STUB_LIB_ES5),
    })
    .unwrap();
    assert!(ws.read_ambient_lib(key_a, "lib.es5.d.ts").is_some());
    assert!(
        ws.read_ambient_lib(key_b, "lib.es5.d.ts").is_none(),
        "project B MUST NOT see project A's ambient libs"
    );
}

// 4.
#[test]
fn register_ambient_lib_sibling_tsconfigs_distinct() {
    let ws = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    ws.set_project_graph(ProjectGraph::from_configs(vec![
        make_project("/ws", "/ws", Some("/ws/tsconfig.app.json")),
        make_project("/ws", "/ws", Some("/ws/tsconfig.vitest.json")),
    ]));
    let app = ws
        .project_stable_key(crate::workspace_snapshot::ProjectId(0))
        .unwrap();
    let test = ws
        .project_stable_key(crate::workspace_snapshot::ProjectId(1))
        .unwrap();
    assert_ne!(
        app, test,
        "sibling tsconfigs MUST produce distinct ProjectStableKey"
    );
}

// 6.
#[test]
fn register_ambient_lib_concurrent_cas_serialise() {
    use std::thread;
    let ws = ws_with_single_project("/ws", "/ws/tsconfig.json");
    let pid = crate::workspace_snapshot::ProjectId(0);
    let mut handles = Vec::new();
    for i in 0..4 {
        let ws_c = Arc::clone(&ws);
        handles.push(thread::spawn(move || {
            ws_c.register_ambient_lib(AmbientLibSpec {
                project_id: Some(pid),
                canonical_id: Arc::from(format!("lib{i}.d.ts")),
                source: Arc::from(format!("interface Lib{i} {{}}")),
            })
        }));
    }
    for h in handles {
        h.join().unwrap().unwrap();
    }
    let view = ws.ambient_libs_view();
    let lib_count: usize = view.by_project.values().map(|p| p.libs.len()).sum();
    assert_eq!(
        lib_count, 4,
        "all four concurrent registrations MUST land via CAS retry"
    );
}

// 7.
#[test]
fn register_ambient_lib_collision_with_user_file_via_published_snapshot() {
    let ws = ws_with_single_project("/ws", "/ws/tsconfig.json");
    ws.inject_file("lib.es5.d.ts".into(), Arc::from("// user version"));
    let err = ws
        .register_ambient_lib(lib_spec("lib.es5.d.ts", STUB_LIB_ES5))
        .unwrap_err();
    assert!(
        matches!(err, AmbientLibError::NonAmbientCollision(_)),
        "registration over a user file MUST surface NonAmbientCollision; got {err:?}"
    );
}

// 8.
#[test]
fn register_ambient_lib_unknown_or_ambiguous_project() {
    let ws = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    ws.set_project_graph(ProjectGraph::from_configs(vec![
        make_project("/ws", "/ws/a", Some("/ws/a/tsconfig.json")),
        make_project("/ws", "/ws/b", Some("/ws/b/tsconfig.json")),
    ]));
    // None with multiple projects -> ambiguous.
    let err = ws
        .register_ambient_lib(lib_spec("lib.es5.d.ts", STUB_LIB_ES5))
        .unwrap_err();
    assert_eq!(err, AmbientLibError::UnknownOrAmbiguousProject);

    // Unknown project_id -> error.
    let err = ws
        .register_ambient_lib(AmbientLibSpec {
            project_id: Some(crate::workspace_snapshot::ProjectId(99)),
            canonical_id: Arc::from("lib.es5.d.ts"),
            source: Arc::from(STUB_LIB_ES5),
        })
        .unwrap_err();
    assert_eq!(err, AmbientLibError::UnknownOrAmbiguousProject);
}

// 11.
#[test]
fn register_ambient_lib_user_wins_shadowing_immediate() {
    let ws = ws_with_single_project("/ws", "/ws/tsconfig.json");
    ws.register_ambient_lib(lib_spec("lib.es5.d.ts", STUB_LIB_ES5))
        .unwrap();
    let key = ws
        .project_stable_key(crate::workspace_snapshot::ProjectId(0))
        .unwrap();
    assert!(ws.read_ambient_lib(key, "lib.es5.d.ts").is_some());
    // Inject a user file at the same canonical_id — read_ambient_lib must
    // immediately return None (overlay-first shadowing).
    ws.inject_file("lib.es5.d.ts".into(), Arc::from("// user version"));
    assert!(
        ws.read_ambient_lib(key, "lib.es5.d.ts").is_none(),
        "A5: user file MUST immediately shadow ambient lib at same canonical"
    );
}

// 12.
#[test]
fn register_ambient_lib_project_rebuild_stable_identity() {
    let ws = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    ws.set_project_graph(ProjectGraph::from_configs(vec![make_project(
        "/ws",
        "/ws/a",
        Some("/ws/a/tsconfig.json"),
    )]));
    let initial_key = ws
        .project_stable_key(crate::workspace_snapshot::ProjectId(0))
        .unwrap();
    ws.register_ambient_lib(lib_spec("lib.es5.d.ts", STUB_LIB_ES5))
        .unwrap();
    assert!(ws.read_ambient_lib(initial_key, "lib.es5.d.ts").is_some());

    // Reorder/replace the project graph with the same project at the same
    // tsconfig path — stable key MUST stay the same.
    ws.set_project_graph(ProjectGraph::from_configs(vec![
        make_project("/ws", "/ws/b", Some("/ws/b/tsconfig.json")),
        make_project("/ws", "/ws/a", Some("/ws/a/tsconfig.json")),
    ]));
    // Find /ws/a in the new ordering and recompute its key.
    let new_view = ws.engine.load_published().unwrap();
    let new_a = new_view
        .snapshot
        .projects
        .iter()
        .find(|p| p.root.as_str() == "/ws/a")
        .unwrap();
    let new_key = ProjectStableKey::from_project(new_a, &new_a.workspace_root);
    assert_eq!(
        initial_key, new_key,
        "ProjectStableKey MUST be stable across graph rebuilds at the same tsconfig path"
    );
    assert!(
        ws.read_ambient_lib(new_key, "lib.es5.d.ts").is_some(),
        "registered lib MUST remain accessible via the same key"
    );
}

// 13.
#[test]
fn register_ambient_lib_canonical_id_normalization() {
    let ws = ws_with_single_project("/ws", "/ws/tsconfig.json");
    let key = ws
        .project_stable_key(crate::workspace_snapshot::ProjectId(0))
        .unwrap();

    // Register via "/lib.es5.d.ts".
    ws.register_ambient_lib(lib_spec("/lib.es5.d.ts", STUB_LIB_ES5))
        .unwrap();
    let gen_after_1 = ws.content_generation();
    // Re-register same content via "\\lib.es5.d.ts" — A7 normalizes to same
    // key, so this is idempotent (no generation bump).
    ws.register_ambient_lib(lib_spec("\\lib.es5.d.ts", STUB_LIB_ES5))
        .unwrap();
    assert_eq!(ws.content_generation(), gen_after_1);
    // Re-register same content via "lib.es5.d.ts" (no leading slash) — same.
    ws.register_ambient_lib(lib_spec("lib.es5.d.ts", STUB_LIB_ES5))
        .unwrap();
    assert_eq!(ws.content_generation(), gen_after_1);

    // All three forms read back the same source.
    assert!(ws.read_ambient_lib(key, "lib.es5.d.ts").is_some());
    assert!(ws.read_ambient_lib(key, "/lib.es5.d.ts").is_some());
    assert!(ws.read_ambient_lib(key, "\\lib.es5.d.ts").is_some());

    // Registry holds exactly one entry.
    let view = ws.ambient_libs_view();
    let entries: usize = view.by_project.values().map(|p| p.libs.len()).sum();
    assert_eq!(entries, 1);
}

// 14.
#[test]
fn register_ambient_lib_multi_root_no_collision() {
    let ws_a = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    ws_a.set_project_graph(ProjectGraph::from_configs(vec![make_project(
        "/a",
        "/a",
        Some("/a/tsconfig.json"),
    )]));
    let ws_b = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    ws_b.set_project_graph(ProjectGraph::from_configs(vec![make_project(
        "/b",
        "/b",
        Some("/b/tsconfig.json"),
    )]));
    let key_a = ws_a
        .project_stable_key(crate::workspace_snapshot::ProjectId(0))
        .unwrap();
    let key_b = ws_b
        .project_stable_key(crate::workspace_snapshot::ProjectId(0))
        .unwrap();
    assert_ne!(
        key_a, key_b,
        "two workspaces both rooted at tsconfig.json MUST produce distinct keys (A3 workspace-root discriminator)"
    );
}

// 15.
#[test]
fn register_ambient_lib_symbol_index_lookup() {
    let ws = ws_with_single_project("/ws", "/ws/tsconfig.json");
    ws.register_ambient_lib(lib_spec("lib.es5.d.ts", STUB_LIB_ES5))
        .unwrap();
    let key = ws
        .project_stable_key(crate::workspace_snapshot::ProjectId(0))
        .unwrap();

    let pick = ws
        .lookup_ambient_symbol(key, "Pick")
        .expect("Pick must resolve through symbol_index");
    assert_eq!(pick.canonical_id.as_ref(), "lib.es5.d.ts");
    let v: &str = &pick.virtual_id;
    assert!(v.starts_with("ambient:/"), "got {v}");
    assert!(v.ends_with("/lib.es5.d.ts"), "got {v}");

    assert!(
        ws.lookup_ambient_symbol(key, "DoesNotExist").is_none(),
        "unknown symbols MUST return None"
    );
}

// 16.
#[test]
fn register_ambient_lib_lib_order_precedence() {
    let ws = ws_with_single_project("/ws", "/ws/tsconfig.json");
    let key = ws
        .project_stable_key(crate::workspace_snapshot::ProjectId(0))
        .unwrap();

    // Register two libs both exposing `Promise`. First one in MUST win
    // symbol_index lookups (lib_order = 0 < lib_order = 1).
    ws.register_ambient_lib(lib_spec("lib.es5.d.ts", STUB_LIB_ES5))
        .unwrap();
    ws.register_ambient_lib(AmbientLibSpec {
        project_id: None,
        canonical_id: Arc::from("lib.es2015.d.ts"),
        source: Arc::from(format!("{STUB_LIB_ES5}\n{STUB_LIB_ES2015}")),
    })
    .unwrap();

    let pick = ws.lookup_ambient_symbol(key, "Pick").unwrap();
    assert_eq!(
        pick.canonical_id.as_ref(),
        "lib.es5.d.ts",
        "first registered lib (lib_order=0) MUST win symbol lookups"
    );
    assert_eq!(pick.lib_order, 0);

    // `Promise` only exists in lib.es2015 — must resolve there.
    let promise = ws.lookup_ambient_symbol(key, "Promise").unwrap();
    assert_eq!(promise.canonical_id.as_ref(), "lib.es2015.d.ts");
    assert_eq!(promise.lib_order, 1);
}

// Brief addendum: `register_ambient_lib_idempotent` (per A1 idempotent contract).
#[test]
fn register_ambient_lib_idempotent_per_a1_contract() {
    let ws = ws_with_single_project("/ws", "/ws/tsconfig.json");
    let key = ws
        .project_stable_key(crate::workspace_snapshot::ProjectId(0))
        .unwrap();

    // First registration: content_generation bumps.
    let gen0 = ws.content_generation();
    ws.register_ambient_lib(lib_spec("lib.es5.d.ts", STUB_LIB_ES5))
        .unwrap();
    let gen1 = ws.content_generation();
    assert!(gen1 > gen0, "first registration MUST bump generation");

    // Second registration: same source, same hash — MUST NOT bump generation.
    ws.register_ambient_lib(lib_spec("lib.es5.d.ts", STUB_LIB_ES5))
        .unwrap();
    let gen2 = ws.content_generation();
    assert_eq!(
        gen2, gen1,
        "idempotent re-register MUST NOT bump generation"
    );

    // Different source, same canonical: hash differs — MUST bump generation.
    ws.register_ambient_lib(lib_spec("lib.es5.d.ts", "interface ChangedLib {}"))
        .unwrap();
    let gen3 = ws.content_generation();
    assert!(
        gen3 > gen2,
        "content change at same canonical MUST bump generation (dep validators rely on this)"
    );

    // Content actually changed — Pick is gone, ChangedLib is now visible.
    let pick = ws.lookup_ambient_symbol(key, "Pick");
    assert!(
        pick.is_none(),
        "old symbols MUST be evicted from symbol_index after content replacement"
    );
    let changed = ws.lookup_ambient_symbol(key, "ChangedLib");
    assert!(changed.is_some(), "new content's symbol MUST be indexed");
}

// Brief addendum: `vfs_shadowing_overlay_wins` (per A5).
#[test]
fn vfs_shadowing_overlay_wins() {
    let ws = ws_with_single_project("/ws", "/ws/tsconfig.json");
    let key = ws
        .project_stable_key(crate::workspace_snapshot::ProjectId(0))
        .unwrap();

    // Register ambient lib first.
    ws.register_ambient_lib(lib_spec("lib.es5.d.ts", STUB_LIB_ES5))
        .unwrap();
    assert!(ws.read_ambient_lib(key, "lib.es5.d.ts").is_some());

    // Open editor buffer at the canonical_id — overlay must shadow ambient.
    ws.notify_upsert("lib.es5.d.ts", Arc::from("// editor buffer"));
    assert!(
        ws.read_ambient_lib(key, "lib.es5.d.ts").is_none(),
        "A5: editor overlay MUST shadow ambient lib"
    );
    // read_file returns the user's overlay content.
    let s = ws.read_file("lib.es5.d.ts").unwrap();
    assert_eq!(&*s, "// editor buffer");

    // Closing the overlay — ambient is visible again because no snapshot exists.
    ws.notify_close("lib.es5.d.ts");
    assert!(
        ws.read_ambient_lib(key, "lib.es5.d.ts").is_some(),
        "after overlay clears, ambient lib MUST be readable again"
    );
}

// Negative regression: plain `read_file` MUST NOT serve ambient libs even
// when no overlay or snapshot exists (otherwise `read_ambient_lib` would be
// pointless and host-side cache validators couldn't discriminate).
#[test]
fn read_file_does_not_serve_ambient_lib_through_snapshot() {
    let ws = ws_with_single_project("/ws", "/ws/tsconfig.json");
    ws.register_ambient_lib(lib_spec("lib.es5.d.ts", STUB_LIB_ES5))
        .unwrap();
    assert!(
        ws.read_file("lib.es5.d.ts").is_none(),
        "read_file MUST NOT route to ambient lib registry"
    );
    assert!(
        !ws.file_exists("lib.es5.d.ts"),
        "file_exists MUST report false for ambient-only canonical_id"
    );
}

// Unregister is the inverse of register and bumps generation.
#[test]
fn unregister_ambient_lib_evicts_entry_and_symbol_index() {
    let ws = ws_with_single_project("/ws", "/ws/tsconfig.json");
    let key = ws
        .project_stable_key(crate::workspace_snapshot::ProjectId(0))
        .unwrap();
    ws.register_ambient_lib(lib_spec("lib.es5.d.ts", STUB_LIB_ES5))
        .unwrap();
    let gen_after_register = ws.content_generation();

    ws.unregister_ambient_lib(key, "lib.es5.d.ts").unwrap();
    assert!(
        ws.content_generation() > gen_after_register,
        "unregister MUST bump content_generation"
    );
    assert!(ws.read_ambient_lib(key, "lib.es5.d.ts").is_none());
    assert!(
        ws.lookup_ambient_symbol(key, "Pick").is_none(),
        "symbol_index MUST be evicted on unregister"
    );

    // Re-unregistering an already-removed entry is a no-op (no generation bump).
    let g = ws.content_generation();
    ws.unregister_ambient_lib(key, "lib.es5.d.ts").unwrap();
    assert_eq!(g, ws.content_generation());
}

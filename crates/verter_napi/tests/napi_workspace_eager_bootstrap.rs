//! Architecture guards for `NapiWorkspace::new` eager bootstrap.
//!
//! Locked-down by the VFS audit-harness fix (Phase Y1, 2026-05-26).
//! See `D:/tmp/vfs-invest/X1-synthesis-plan.md` §2.3 and `W2-codex-out.txt`
//! for the empirical reproduction that motivated these guards.
//!
//! The shared-optimized-codebase rule requires that every JS consumer
//! of `new native.Workspace([root])` receives a populated
//! `WorkspaceSnapshot` by default — without these guards, the
//! "lazy stub constructor" failure mode that produced the misleading
//! 19/45 semanticMiss result on `Table.vue` can silently regress.
//!
//! Hermetic per the testing-hermeticity rule: a `tempfile::tempdir()`
//! with a self-contained tsconfig — no dependency on vendored
//! third-party corpora.

use verter_napi::NapiWorkspace;
use verter_workspace::workspace_snapshot::ProjectPayload;

/// Architecture guard: `NapiWorkspace::new(roots)` must eagerly
/// publish a `WorkspaceSnapshot` whose `projects` list reflects the
/// tsconfigs discovered under `roots`. Regressing the eager bootstrap
/// (e.g. reverting `NapiWorkspace::new` to "just store roots") will
/// cause this test to fail.
///
/// Discriminator: reverting `NapiWorkspace::new` to skip
/// `build_workspace_snapshot + publish_snapshot` causes
/// `load_published()` to return an empty bootstrap snapshot whose
/// `projects` slice has no `Configured` entry — this assertion fails
/// RED on the pre-fix tree and passes GREEN on the post-fix tree.
#[test]
fn napi_workspace_new_eagerly_publishes_project_graph_for_tsconfig_root() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let root_path = tmp.path();
    std::fs::create_dir_all(root_path.join("src")).expect("src dir");
    std::fs::write(
        root_path.join("tsconfig.json"),
        r#"{
  "include": ["src/**/*"]
}"#,
    )
    .expect("tsconfig should be written");
    std::fs::write(
        root_path.join("src").join("App.ts"),
        "export const app = 1;\n",
    )
    .expect("src file should be written");

    // Normalize to the forward-slash form `CanonicalPath::new`
    // produces internally.
    let root_str = root_path.to_string_lossy().replace('\\', "/");

    let ws = NapiWorkspace::new(vec![root_str.clone()]);
    let inner = ws.filesystem_workspace_for_tests();
    let published = inner
        .load_published()
        .expect("eager bootstrap must publish a `PublishedRoot`");

    assert!(
        published.ownership_ready,
        "eager bootstrap must publish with ownership_ready=true; got false (likely a regression to the empty bootstrap snapshot)"
    );

    let projects = &published.snapshot.projects;
    assert!(
        !projects.is_empty(),
        "eager bootstrap must materialize at least one project for a root containing tsconfig.json; got empty projects (regression — `Engine::resolve_import` will short-circuit against an empty graph)"
    );

    let configured_count = projects
        .iter()
        .filter(|p| matches!(p.payload, ProjectPayload::Configured { .. }))
        .count();
    assert!(
        configured_count >= 1,
        "eager bootstrap must materialize at least one Configured project from the tsconfig; got {configured_count} configured projects in {} total. Roots: {root_str:?}",
        projects.len(),
    );
}

/// Architecture guard companion: `NapiWorkspace::new` against a root
/// with NO tsconfig must still publish a real snapshot (with the
/// fallback project), not an empty bootstrap.
#[test]
fn napi_workspace_new_publishes_fallback_project_for_root_without_tsconfig() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let root_path = tmp.path();
    let root_str = root_path.to_string_lossy().replace('\\', "/");

    let ws = NapiWorkspace::new(vec![root_str]);
    let inner = ws.filesystem_workspace_for_tests();
    let published = inner
        .load_published()
        .expect("eager bootstrap must publish a `PublishedRoot` even without tsconfig");

    assert!(
        published.ownership_ready,
        "eager bootstrap must publish with ownership_ready=true"
    );
    // The fallback project is unconditionally appended per
    // `build_workspace_snapshot`, so the projects list is never
    // empty for a real root.
    assert!(
        !published.snapshot.projects.is_empty(),
        "eager bootstrap must produce a fallback project for tsconfig-less roots"
    );
}

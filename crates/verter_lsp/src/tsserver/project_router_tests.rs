//! Unit tests for [`crate::tsserver::project_router`].
//!
//! Extracted from an inline `#[cfg(test)] mod tests` in `project_router.rs`:
//! the fixtures build real on-disk TypeScript installs with `std::fs`, and the
//! D14 / VFS-boundary architecture guards scan whole PRODUCTION source files
//! for `std::fs::`, so a test-only fixture helper living in the production file
//! reads to those guards as a disk-boundary bypass. Wired back as a
//! `#[cfg(test)] #[path = "project_router_tests.rs"] mod tests;` child of
//! `project_router`, so `use super::*` resolves to its items.

use super::*;
use std::path::PathBuf;
use verter_session::external_ts::EnvDims;
use verter_session::file_artifact_store::ProjectIdentity;
use verter_workspace::workspace_snapshot::{ProjectId, SnapshotGeneration};

fn write_typescript(root: &Path, version: &str) -> PathBuf {
    let lib = root.join("node_modules/typescript/lib");
    std::fs::create_dir_all(&lib).unwrap();
    std::fs::write(lib.join("tsserver.js"), "// tsserver").unwrap();
    std::fs::write(lib.join("lib.es5.d.ts"), "interface Array<T> {}").unwrap();
    std::fs::write(
        root.join("node_modules/typescript/package.json"),
        format!(r#"{{ "name": "typescript", "version": "{version}" }}"#),
    )
    .unwrap();
    lib.join("tsserver.js").canonicalize().unwrap()
}

/// A pnpm-shaped install: the package's `node_modules/typescript` is a
/// SYMLINK into a workspace-level `.pnpm` store, exactly as pnpm lays a
/// monorepo out. Returns the REAL (canonical) `tsserver.js`.
#[cfg(unix)]
fn link_pnpm_typescript(workspace: &Path, package: &Path, version: &str) -> PathBuf {
    use std::os::unix::fs::symlink;
    let store = workspace
        .join("node_modules/.pnpm")
        .join(format!("typescript@{version}"))
        .join("node_modules/typescript");
    let lib = store.join("lib");
    std::fs::create_dir_all(&lib).unwrap();
    std::fs::write(lib.join("tsserver.js"), "// tsserver").unwrap();
    std::fs::write(lib.join("lib.es5.d.ts"), "interface Array<T> {}").unwrap();
    std::fs::write(
        store.join("package.json"),
        format!(r#"{{ "name": "typescript", "version": "{version}" }}"#),
    )
    .unwrap();
    std::fs::create_dir_all(package.join("node_modules")).unwrap();
    symlink(&store, package.join("node_modules/typescript")).unwrap();
    lib.join("tsserver.js").canonicalize().unwrap()
}

fn write_tsconfig(project: &Path) {
    std::fs::create_dir_all(project).unwrap();
    std::fs::write(project.join("tsconfig.json"), r#"{ "include": ["src"] }"#).unwrap();
}

fn binding(workspace: &Path, project: &Path, id: u32) -> ProjectBinding {
    ProjectBinding::new_for_test(
        workspace.to_string_lossy().into_owned(),
        project.join("tsconfig.json").to_string_lossy().into_owned(),
        "",
        EnvDims {
            parse_env_hash: [id as u8; 16],
            resolve_env_hash: [id as u8; 16],
            lib_env_hash: [id as u8; 16],
            project_identity: ProjectIdentity([id as u8; 16]),
        },
        Vec::new(),
        ProjectId(id),
        SnapshotGeneration(1),
    )
}

fn engine_spec(
    backend: &TsserverEngineBackend,
    binding: &ProjectBinding,
    tsdk: Option<&str>,
) -> Result<ProjectEngineSpec, String> {
    let bound = ensure_bound(backend, binding).expect("the witness mint is infallible");
    resolve_engine_spec(&bound, binding, tsdk)
}

/// @ai-generated - Pins distinct engine identity for different owning projects.
///
/// The whole point of the router: two packages in ONE workspace, pinned to
/// DIFFERENT TypeScript versions, resolve to two DIFFERENT `tsserver.js`
/// installs — so they can never share one process.
///
/// The fixture also plants a THIRD, unrelated TypeScript at the WORKSPACE
/// ROOT. Resolving from the workspace root (the behaviour this router
/// replaced) would hand BOTH packages that root install — the assertions
/// below fail in exactly that case, so this test discriminates the
/// per-project resolution from the workspace-level one.
#[cfg(unix)]
#[test]
fn different_projects_keep_their_own_typescript_engines() {
    let workspace = tempfile::tempdir().unwrap();
    let project_a = workspace.path().join("packages/a");
    let project_b = workspace.path().join("packages/b");
    std::fs::create_dir_all(&project_a).unwrap();
    std::fs::create_dir_all(&project_b).unwrap();
    let root_install = write_typescript(workspace.path(), "5.0.4");
    // pnpm layout: package symlinks into the workspace `.pnpm` store, so the
    // resolution must canonicalize to the REAL versioned install — tsserver
    // finds its `lib.*.d.ts` relative to its own script path.
    let expected_a = link_pnpm_typescript(workspace.path(), &project_a, "5.8.3");
    let expected_b = link_pnpm_typescript(workspace.path(), &project_b, "6.0.2");
    let backend = TsserverEngineBackend::with_default_host_version();

    let spec_a = engine_spec(&backend, &binding(workspace.path(), &project_a, 0), None).unwrap();
    let spec_b = engine_spec(&backend, &binding(workspace.path(), &project_b, 1), None).unwrap();

    assert_eq!(Path::new(&spec_a.key.tsserver_path), expected_a);
    assert_eq!(Path::new(&spec_b.key.tsserver_path), expected_b);
    assert_ne!(
        spec_a.key.tsserver_path, spec_b.key.tsserver_path,
        "two packages pinned to different TypeScript versions must not share an engine"
    );
    assert_ne!(spec_a.key, spec_b.key);
    for spec in [&spec_a, &spec_b] {
        assert_ne!(
            Path::new(&spec.key.tsserver_path),
            root_install,
            "a package must be served by its OWN install, never the workspace root's"
        );
    }
    assert!(spec_a.default_lib_count > 0 && spec_b.default_lib_count > 0);
}

/// @ai-generated - NEGATIVE CONTROL: a project with no resolvable TypeScript
/// fails closed with the actionable install message and is NEVER served by a
/// sibling project's engine.
#[cfg(unix)]
#[test]
fn project_without_typescript_fails_closed_and_never_borrows_a_sibling_engine() {
    let workspace = tempfile::tempdir().unwrap();
    let served = workspace.path().join("packages/served");
    let bare = workspace.path().join("packages/bare");
    std::fs::create_dir_all(&served).unwrap();
    std::fs::create_dir_all(&bare).unwrap();
    let served_tsserver = link_pnpm_typescript(workspace.path(), &served, "6.0.2");
    let backend = TsserverEngineBackend::with_default_host_version();

    let served_spec = engine_spec(&backend, &binding(workspace.path(), &served, 0), None).unwrap();
    assert_eq!(Path::new(&served_spec.key.tsserver_path), served_tsserver);

    // The bare package's ancestor walk escapes the tempdir, so the assertion
    // is conditional on the machine genuinely having no ambient TypeScript
    // above it; when one exists the meaningful invariant is still checked —
    // the refusal (or the resolution) is NEVER the sibling's engine.
    match engine_spec(&backend, &binding(workspace.path(), &bare, 1), None) {
        Err(message) => {
            assert!(
                message.contains("no usable TypeScript installation was found"),
                "the refusal names the missing install: {message}"
            );
            assert!(
                message.contains("npm install -D typescript"),
                "the refusal carries the actionable install command: {message}"
            );
            assert!(
                !message.contains(&served_spec.key.tsserver_path),
                "the refusal must not point at the sibling project's engine: {message}"
            );
        }
        Ok(spec) => assert_ne!(
            spec.key.tsserver_path, served_spec.key.tsserver_path,
            "a project must never be served by another project's resolved engine"
        ),
    }
}

/// @ai-generated - The route-selection probe reports the workspace as
/// servable when ANY configured project can obtain TypeScript, and computes
/// the advisory from the LOWEST serving version (not the first one found).
#[cfg(unix)]
#[test]
fn workspace_probe_serves_on_any_project_and_advises_on_the_lowest_version() {
    let workspace = tempfile::tempdir().unwrap();
    let legacy = workspace.path().join("packages/legacy");
    let current = workspace.path().join("packages/current");
    let bare = workspace.path().join("packages/bare");
    write_tsconfig(&legacy);
    write_tsconfig(&current);
    write_tsconfig(&bare);
    link_pnpm_typescript(workspace.path(), &legacy, "5.8.3");
    link_pnpm_typescript(workspace.path(), &current, "6.0.2");

    let probe = probe_workspace_tsserver(&workspace.path().to_string_lossy(), None);

    let servable = probe.servable.as_ref().expect("a servable project exists");
    assert!(
        servable.resolved.default_lib_count > 0,
        "a library-less install is never reported servable"
    );
    // `packages/bare` sorts first and cannot resolve locally; the probe must
    // keep walking rather than reporting the workspace unservable.
    assert_eq!(probe.lowest_servable_version, Some((5, 8)));
    let advisory = probe.advisory().expect("a 5.8 package is advised");
    assert!(
        advisory.contains("5.8"),
        "the advisory names 5.8: {advisory}"
    );
    assert!(probe.native_family_only.is_none());
}

/// @ai-generated - A workspace whose ONLY resolvable TypeScript is the TS7+
/// native family is never served over the Node tsserver protocol.
#[cfg(unix)]
#[test]
fn workspace_probe_reports_native_family_only() {
    let workspace = tempfile::tempdir().unwrap();
    let native = workspace.path().join("packages/native");
    write_tsconfig(&native);
    link_pnpm_typescript(workspace.path(), &native, "7.0.0");

    let probe = probe_project_dirs(&[native.to_string_lossy().into_owned()], None);

    assert!(probe.servable.is_none());
    assert_eq!(probe.native_family_only, Some(7));
}

/// @ai-generated - Guards the non-pnpm (plain `node_modules`) layout too.
#[test]
fn plain_node_modules_install_resolves_for_its_own_project() {
    let workspace = tempfile::tempdir().unwrap();
    let project = workspace.path().join("packages/plain");
    std::fs::create_dir_all(&project).unwrap();
    let expected = write_typescript(&project, "6.0.2");
    let backend = TsserverEngineBackend::with_default_host_version();

    let spec = engine_spec(&backend, &binding(workspace.path(), &project, 0), None).unwrap();

    assert_eq!(Path::new(&spec.key.tsserver_path), expected);
}

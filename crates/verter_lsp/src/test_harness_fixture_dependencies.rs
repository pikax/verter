//! Hermetic type-only package materialization for real-provider fixtures.

/// Resolve an E2E fixture workspace root as a canonical path.
pub(crate) fn fixture_workspace_root(name: &str) -> String {
    let path = std::fs::canonicalize(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("../../packages/vue-vscode/e2e/fixtures/{name}")),
    )
    .expect("fixture workspace path should canonicalize");
    crate::test_utils::canonical_test_path(&path)
}

/// Materialize the type-only dependencies declared by parity fixtures without
/// relying on incidental ancestor `node_modules` resolution.
pub(super) fn materialize_real_provider_framework_types(fixture: &str) {
    static MATERIALIZE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = MATERIALIZE_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let fixture_root = std::path::PathBuf::from(fixture_workspace_root(fixture));
    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let node_modules = fixture_root.join("node_modules");

    match fixture {
        "single-project" | "vue-parity" => {
            copy_type_package_with_dependencies(
                &repo_root.join("packages/types/node_modules/vue"),
                &node_modules,
            );
            if fixture == "vue-parity" {
                write_react_jsx_ambient_atomically(&node_modules.join("@types/react"));
            }
        }
        "svelte-parity" => {
            copy_type_package_atomically(
                &repo_root.join("packages/svelte-jsx/node_modules/svelte"),
                &node_modules.join("svelte"),
            );
            copy_type_package_atomically(
                &repo_root.join("packages/svelte-jsx"),
                &node_modules.join("@verter/svelte-jsx"),
            );
        }
        _ => {}
    }
}

fn copy_type_package_with_dependencies(source: &std::path::Path, node_modules: &std::path::Path) {
    fn visit(
        source: &std::path::Path,
        node_modules: &std::path::Path,
        visited: &mut std::collections::HashSet<String>,
    ) {
        let manifest_path = source.join("package.json");
        let manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&manifest_path).unwrap_or_else(|error| {
                panic!(
                    "read real-provider fixture dependency manifest {}: {error}",
                    manifest_path.display()
                )
            }))
            .unwrap_or_else(|error| {
                panic!(
                    "parse real-provider fixture dependency manifest {}: {error}",
                    manifest_path.display()
                )
            });
        let name = manifest["name"].as_str().unwrap_or_else(|| {
            panic!(
                "real-provider fixture dependency has no package name: {}",
                manifest_path.display()
            )
        });
        if !visited.insert(name.to_string()) {
            return;
        }

        copy_type_package_atomically(source, &node_modules.join(name));
        let Some(dependencies) = manifest["dependencies"].as_object() else {
            return;
        };
        for dependency in dependencies.keys() {
            let dependency_source = resolve_dependency_source(source, dependency)
                .unwrap_or_else(|| {
                    panic!(
                        "real-provider fixture transitive dependency `{dependency}` must resolve from {}",
                        source.display()
                    )
                });
            visit(&dependency_source, node_modules, visited);
        }
    }

    let source = std::fs::canonicalize(source).unwrap_or_else(|error| {
        panic!(
            "canonicalize real-provider fixture dependency {}: {error}",
            source.display()
        )
    });
    visit(&source, node_modules, &mut std::collections::HashSet::new());
}

fn resolve_dependency_source(
    package_root: &std::path::Path,
    dependency: &str,
) -> Option<std::path::PathBuf> {
    let mut current = Some(package_root);
    while let Some(directory) = current {
        let candidate = directory.join("node_modules").join(dependency);
        if candidate.is_dir() {
            return Some(candidate);
        }
        let pnpm_hoist = directory
            .join("node_modules/.pnpm/node_modules")
            .join(dependency);
        if pnpm_hoist.is_dir() {
            return Some(pnpm_hoist);
        }
        current = directory.parent();
    }
    None
}

fn copy_type_package_atomically(source: &std::path::Path, destination: &std::path::Path) {
    assert!(
        source.is_dir(),
        "real-provider fixture dependency must exist after the workspace package install: {}",
        source.display()
    );
    copy_type_package_directory(source, destination, true);
}

fn copy_type_package_directory(
    source: &std::path::Path,
    destination: &std::path::Path,
    is_package_root: bool,
) {
    std::fs::create_dir_all(destination).unwrap_or_else(|error| {
        panic!(
            "create real-provider fixture dependency directory {}: {error}",
            destination.display()
        )
    });
    let mut package_json = None;
    for entry in std::fs::read_dir(source)
        .unwrap_or_else(|error| panic!("read fixture dependency {}: {error}", source.display()))
    {
        let entry = entry.expect("read fixture dependency entry");
        let file_type = entry
            .file_type()
            .expect("read fixture dependency file type");
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        if file_type.is_dir() {
            if entry.file_name() != "node_modules" {
                copy_type_package_directory(&source_path, &destination_path, false);
            }
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == "package.json" {
            package_json = Some((source_path, destination_path));
        } else if name.ends_with(".d.ts") || name.ends_with(".d.mts") || name.ends_with(".d.cts") {
            copy_file_atomically(&source_path, &destination_path);
        }
    }
    if let Some((source_path, destination_path)) = package_json {
        copy_file_atomically(&source_path, &destination_path);
    } else if is_package_root {
        panic!(
            "real-provider fixture dependency has no package.json: {}",
            source.display()
        );
    }
}

fn copy_file_atomically(source: &std::path::Path, destination: &std::path::Path) {
    let bytes = std::fs::read(source)
        .unwrap_or_else(|error| panic!("read fixture dependency {}: {error}", source.display()));
    if std::fs::read(destination).is_ok_and(|existing| existing == bytes) {
        return;
    }
    write_file_atomically(destination, &bytes);
}

fn write_file_atomically(destination: &std::path::Path, bytes: &[u8]) {
    use std::io::Write;
    let parent = destination
        .parent()
        .expect("fixture dependency file has a parent");
    std::fs::create_dir_all(parent).unwrap_or_else(|error| {
        panic!(
            "create fixture dependency parent {}: {error}",
            parent.display()
        )
    });
    let mut temporary = tempfile::NamedTempFile::new_in(parent)
        .unwrap_or_else(|error| panic!("create atomic fixture dependency file: {error}"));
    temporary
        .write_all(bytes)
        .unwrap_or_else(|error| panic!("write atomic fixture dependency file: {error}"));
    temporary.persist(destination).unwrap_or_else(|error| {
        panic!(
            "publish fixture dependency {}: {error}",
            destination.display()
        )
    });
}

fn write_react_jsx_ambient_atomically(package_root: &std::path::Path) {
    const REACT_AMBIENT: &str = r#"declare namespace React {
  interface HTMLAttributes<T> { className?: string }
}
declare namespace JSX {
  interface Element {}
  interface IntrinsicElements { div: React.HTMLAttributes<HTMLDivElement> }
}
export = React;
export as namespace React;
"#;
    write_file_atomically(&package_root.join("index.d.ts"), REACT_AMBIENT.as_bytes());
    write_file_atomically(
        &package_root.join("package.json"),
        br#"{"name":"@types/react","types":"index.d.ts"}"#,
    );
}

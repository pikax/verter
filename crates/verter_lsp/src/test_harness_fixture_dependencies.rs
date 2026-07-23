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
    let fixture_root = std::path::PathBuf::from(fixture_workspace_root(fixture));
    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let node_modules = fixture_root.join("node_modules");

    match fixture {
        "vue-parity" => {
            copy_type_package_atomically(
                &repo_root.join("packages/types/node_modules/vue"),
                &node_modules.join("vue"),
            );
            write_type_package_atomically(
                &node_modules.join("@verter/types"),
                verter_session::VERTER_TYPES_STANDALONE_DTS,
            );
            write_react_jsx_ambient_atomically(&node_modules.join("@types/react"));
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

fn write_type_package_atomically(package_root: &std::path::Path, declarations: &str) {
    write_file_atomically(&package_root.join("index.d.ts"), declarations.as_bytes());
    write_file_atomically(
        &package_root.join("package.json"),
        br#"{"name":"@verter/types","types":"index.d.ts"}"#,
    );
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

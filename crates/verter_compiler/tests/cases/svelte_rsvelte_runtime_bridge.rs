use verter_compiler::framework_common::{CarrierCompiler, RuntimeCompileOptions};
use verter_compiler::svelte::SvelteCarrierCompiler;

/// @ai-generated - Pins the Verter carrier contract at the rsvelte runtime boundary.
#[test]
fn svelte_carrier_emits_server_runtime_through_the_registered_backend() {
    let source = r#"<script>
  let { greeting = "hello" } = $props();
</script>
<h1>{greeting}</h1>"#;
    let compiler = SvelteCarrierCompiler::default();
    let artifact = compiler.parse(source, &Default::default());
    let allocator = oxc_allocator::Allocator::default();
    let output = compiler
        .compile_bundle(
            source,
            &artifact,
            &RuntimeCompileOptions {
                filename: Some("Greeting.svelte".to_string()),
                ssr: true,
                source_map: true,
                ..Default::default()
            },
            &allocator,
        )
        .expect("the Svelte carrier must accept a registered runtime backend");

    assert!(
        !output.runtime_surface_refused(),
        "a supported server component must not be reported as a runtime refusal: {:?}",
        output.diagnostics
    );
    let module = output
        .main
        .body_code
        .expect("the server backend must produce a main module");
    assert!(module.contains("svelte/internal/server"), "{module}");
    assert!(
        module.contains("export default"),
        "the generated server module must expose the component: {module}"
    );
    assert_eq!(output.main.lang.as_deref(), Some("js"));
    let source_map = oxc_sourcemap::OwnedSourceMap::from_json_string(&output.main.source_map)
        .expect("the server backend must return a valid source map");
    assert_eq!(
        source_map.get_sources().collect::<Vec<_>>(),
        ["./Greeting.svelte"]
    );
    assert_eq!(source_map.get_source_content(0), Some(source));
}

#[test]
fn rsvelte_types_are_confined_to_the_private_bridge() {
    fn visit_rust_files(directory: &std::path::Path, files: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(directory)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", directory.display()))
        {
            let path = entry.expect("source directory entry").path();
            if path.is_dir() {
                visit_rust_files(&path, files);
            } else if path.extension().and_then(|extension| extension.to_str()) == Some("rs") {
                files.push(path);
            }
        }
    }

    let source_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let allowed = source_root.join("svelte/rsvelte_bridge.rs");
    let mut rust_files = Vec::new();
    visit_rust_files(&source_root, &mut rust_files);

    let offenders = rust_files
        .into_iter()
        .filter(|path| path != &allowed)
        .filter(|path| {
            std::fs::read_to_string(path)
                .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()))
                .contains("rsvelte_core")
        })
        .collect::<Vec<_>>();

    assert!(
        offenders.is_empty(),
        "rsvelte types must not cross the private neutral bridge: {offenders:?}"
    );
}

#[test]
fn application_runtime_matches_the_rsvelte_svelte_target() {
    use std::collections::BTreeSet;

    let workspace_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("verter_compiler must live at <workspace>/crates/verter_compiler")
        .to_path_buf();
    let bridge_path = workspace_root.join("crates/verter_compiler/src/svelte/rsvelte_bridge.rs");
    let bridge = std::fs::read_to_string(&bridge_path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", bridge_path.display()));
    let target = bridge
        .lines()
        .find_map(|line| {
            let value = line
                .trim()
                .strip_prefix("const EXPECTED_SVELTE_VERSION: &str = ")?;
            Some(value.trim_end_matches(';').trim_matches('"').to_string())
        })
        .expect("the private bridge must declare its expected Svelte target");

    let lock_path = workspace_root.join("pnpm-lock.yaml");
    let lock = std::fs::read_to_string(&lock_path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", lock_path.display()));
    let resolved = lock
        .lines()
        .filter_map(|line| {
            let package = line.trim().strip_prefix("svelte@")?.strip_suffix(':')?;
            let version = package.split('(').next()?;
            (!version.contains('/') && !version.is_empty()).then(|| version.to_string())
        })
        .collect::<BTreeSet<_>>();

    assert_eq!(
        resolved,
        BTreeSet::from([target]),
        "the application runtime must resolve exactly the Svelte version targeted by rsvelte"
    );
}

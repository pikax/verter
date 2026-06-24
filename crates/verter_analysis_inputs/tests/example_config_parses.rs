//! The committed example config must parse under the shared schema and yield only
//! opaque ids. This pins the `.example` to the loader contract: a future edit that
//! breaks the schema, or that slips a descriptive id into the example, fails here.

use std::path::PathBuf;

use verter_analysis_inputs::{parse_config, ProjectKind};

/// Resolve the workspace root from this crate's manifest dir
/// (`<root>/crates/verter_analysis_inputs`) → `<root>`.
fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("crate is two levels under the workspace root")
        .to_path_buf()
}

#[test]
fn committed_example_parses_and_is_all_opaque() {
    let example = workspace_root().join(".analysis/projects.local.json.example");
    let json = std::fs::read_to_string(&example)
        .unwrap_or_else(|e| panic!("read {}: {e}", example.display()));
    let cfg = parse_config(&json).expect("the committed example must parse");

    assert!(cfg.schema_matches(), "example uses the shared schema");
    assert!(!cfg.projects().is_empty(), "example carries at least one project");

    for project in cfg.projects() {
        let id = project.id().as_str();
        // Opaque-id shape is re-asserted end-to-end: `p` + four digits.
        assert_eq!(id.len(), 5, "opaque id must be 5 chars: {id}");
        assert!(id.starts_with('p'), "opaque id must start with p: {id}");
        assert!(
            id[1..].chars().all(|c| c.is_ascii_digit()),
            "opaque id tail must be digits: {id}"
        );
        // Kind is one of the known set (parsing would have failed otherwise).
        matches!(project.kind(), ProjectKind::Vite | ProjectKind::Nuxt | ProjectKind::Lib);
    }
}

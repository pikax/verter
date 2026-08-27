//! The auto-import preferred-specifier contract, end to end through the
//! workspace engine: reverse-alias candidates from the semantic kernel,
//! forward-resolution round-trip verification, and shortest-wins selection.
//!
//! These cases exercise the production composite:
//! `Engine::preferred_specifier` over
//! `ModuleResolverCore::preferred_specifier_candidates`.

use std::sync::Arc;

use verter_semantic::resolver_core::{IdeProjectCompilerOptions, WorkspaceAlias};

use crate::canonical_path::CanonicalPath;
use crate::memory::{MemoryOptions, MemoryWorkspace};
use crate::project_graph::{ProjectGraph, ProjectRank, VfsProjectConfig};
use crate::resolution_currency::ResolutionEvidenceSource;
use crate::traits::WorkspaceRead;

fn workspace_with(
    compiler_options: IdeProjectCompilerOptions,
    workspace_aliases: Vec<WorkspaceAlias>,
    files: &[&str],
) -> MemoryWorkspace {
    let ws = MemoryWorkspace::new(MemoryOptions::default());
    for file in files {
        ws.inject_file((*file).to_string(), Arc::from("export {}"));
    }
    ws.set_project_graph(ProjectGraph::from_configs(vec![VfsProjectConfig {
        root: "/workspace".to_string(),
        rank: ProjectRank::Explicit,
        tsconfig_path: Some("/workspace/tsconfig.app.json".to_string()),
        root_files: vec![],
        extensions: vec![".vue".to_string(), ".ts".to_string()],
        workspace_root: "/workspace".to_string(),
        workspace_aliases,
        compiler_options,
        references: vec![],
        membership: crate::membership::configured_membership_match_all_under_root(
            &CanonicalPath::new("/workspace"),
        ),
    }]));
    ws
}

fn paths(entries: Vec<(&str, Vec<&str>)>) -> IdeProjectCompilerOptions {
    IdeProjectCompilerOptions {
        base_url: None,
        paths: entries
            .into_iter()
            .map(|(pattern, targets)| {
                (
                    pattern.to_string(),
                    targets.into_iter().map(str::to_string).collect(),
                )
            })
            .collect(),
        ..Default::default()
    }
}

fn alias(find: &str, replacement: &str) -> Vec<WorkspaceAlias> {
    vec![WorkspaceAlias {
        find: find.to_string(),
        replacement: replacement.to_string(),
    }]
}

fn preferred(ws: &MemoryWorkspace, importer: &str, target: &str) -> Option<String> {
    ws.engine.preferred_specifier(
        ws,
        ResolutionEvidenceSource::ReaderAuthoritative,
        importer,
        target,
    )
}

#[test]
fn preferred_specifier_returns_tsconfig_alias() {
    let ws = workspace_with(
        paths(vec![("@/*", vec!["/workspace/src/*"])]),
        vec![],
        &["/workspace/src/Foo.vue"],
    );

    let result = preferred(&ws, "/workspace/src/App.ts", "/workspace/src/Foo.vue");

    assert_eq!(
        result.as_deref(),
        Some("@/Foo.vue"),
        "should return tsconfig path alias"
    );
}

#[test]
fn preferred_specifier_returns_none_when_no_match() {
    let ws = workspace_with(
        paths(vec![("@/*", vec!["/workspace/src/*"])]),
        vec![],
        &["/other/Foo.vue"],
    );

    let result = preferred(&ws, "/workspace/src/App.ts", "/other/Foo.vue");

    assert!(
        result.is_none(),
        "target outside all aliases should return None — got: {result:?}"
    );
}

#[test]
fn preferred_specifier_prefers_shortest() {
    let ws = workspace_with(
        paths(vec![
            ("@/*", vec!["/workspace/src/*"]),
            ("@components/*", vec!["/workspace/src/components/*"]),
        ]),
        vec![],
        &["/workspace/src/components/Bar.vue"],
    );

    let result = preferred(
        &ws,
        "/workspace/src/App.ts",
        "/workspace/src/components/Bar.vue",
    );

    assert_eq!(
        result.as_deref(),
        Some("@components/Bar.vue"),
        "should prefer shorter (more specific) alias"
    );
}

#[test]
fn preferred_specifier_round_trips() {
    let ws = workspace_with(
        paths(vec![("@/*", vec!["/workspace/src/*"])]),
        vec![],
        &["/workspace/src/Foo.vue"],
    );

    let specifier = preferred(&ws, "/workspace/src/App.ts", "/workspace/src/Foo.vue")
        .expect("should find alias specifier");

    // Forward-resolve the specifier and verify it matches the original target.
    let forward = ws
        .resolve_import(
            "/workspace/src/App.ts",
            &specifier,
            verter_semantic::resolver_core::ResolutionContext {
                phase: verter_semantic::resolver_core::ResolvePhase::ProviderGraph,
                kind: verter_semantic::resolver_core::ResolveRequestKind::EsmImport,
            },
        )
        .expect("forward resolve of preferred specifier should succeed");

    assert_eq!(
        forward.source_id, "/workspace/src/Foo.vue",
        "round-trip: forward({specifier}) should resolve to original target"
    );
}

#[test]
fn preferred_specifier_none_for_provider_paths() {
    let ws = workspace_with(
        paths(vec![("@/*", vec!["/workspace/src/*"])]),
        vec![],
        &["/workspace/src/Foo.vue"],
    );

    // .vue.verter.ts is a provider path, not a real file — should not match.
    let result = preferred(
        &ws,
        "/workspace/src/App.ts",
        "/workspace/src/Foo.vue.verter.ts",
    );

    assert!(
        result.is_none(),
        "provider paths (.vue.verter.ts) should return None — got: {result:?}"
    );
}

#[test]
fn preferred_specifier_multi_target_first_wins() {
    // "@/*" maps to both src/ and lib/, first target wins.
    let ws = workspace_with(
        paths(vec![("@/*", vec!["/workspace/src/*", "/workspace/lib/*"])]),
        vec![],
        &["/workspace/src/Foo.vue", "/workspace/lib/Foo.vue"],
    );

    let result = preferred(&ws, "/workspace/src/App.ts", "/workspace/src/Foo.vue");
    assert_eq!(
        result.as_deref(),
        Some("@/Foo.vue"),
        "target in first replacement should produce alias"
    );
}

#[test]
fn preferred_specifier_multi_target_shadowed() {
    // "@/*" maps to src/ first, then lib/. Only lib/Bar.vue exists, so
    // "@/Bar.vue" forward-resolves to lib/Bar.vue and round-trips.
    let ws = workspace_with(
        paths(vec![("@/*", vec!["/workspace/src/*", "/workspace/lib/*"])]),
        vec![],
        &["/workspace/lib/Bar.vue"],
    );

    let result = preferred(&ws, "/workspace/src/App.ts", "/workspace/lib/Bar.vue");
    assert_eq!(
        result.as_deref(),
        Some("@/Bar.vue"),
        "when first target doesn't exist, second target should round-trip"
    );
}

#[test]
fn preferred_specifier_workspace_alias_fallback() {
    // No tsconfig paths, but a workspace alias.
    let ws = workspace_with(
        IdeProjectCompilerOptions::default(),
        alias("~/", "/workspace/src/"),
        &["/workspace/src/Foo.vue"],
    );

    let result = preferred(&ws, "/workspace/src/App.ts", "/workspace/src/Foo.vue");

    assert_eq!(
        result.as_deref(),
        Some("~/Foo.vue"),
        "workspace alias should be used when no tsconfig paths match"
    );
}

/// Vite normalization stores find with trailing slash (`@/`) and replacement
/// WITHOUT trailing slash (`/workspace/src`). The reverse-alias must not
/// produce double-slash specifiers like `@//Foo.vue`.
#[test]
fn preferred_specifier_workspace_alias_no_double_slash() {
    let ws = workspace_with(
        IdeProjectCompilerOptions::default(),
        alias("@/", "/workspace/src"),
        &["/workspace/src/Foo.vue"],
    );

    let result = preferred(&ws, "/workspace/src/App.ts", "/workspace/src/Foo.vue");

    let specifier = result.expect("should find workspace alias specifier");
    assert_eq!(
        specifier, "@/Foo.vue",
        "must NOT produce double-slash like @//Foo.vue"
    );
    assert!(
        !specifier.contains("//"),
        "specifier must not contain double-slash: {specifier}"
    );
}

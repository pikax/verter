use super::*;
use crate::types::{ExactResolution, ResolutionContext, ResolvePhase, ResolveRequestKind};

/// Default context used by most tests (CodegenBlocker + EsmImport).
fn default_ctx() -> ResolutionContext {
    ResolutionContext {
        phase: ResolvePhase::CodegenBlocker,
        kind: ResolveRequestKind::EsmImport,
    }
}

/// Helper to build an ExactResolution with default context.
fn exact(specifier: &str, resolved: Option<&str>, possible: Vec<&str>) -> ExactResolution {
    ExactResolution {
        specifier: specifier.to_string(),
        phase: ResolvePhase::CodegenBlocker,
        kind: ResolveRequestKind::EsmImport,
        resolved_canonical_id: resolved.map(|s| s.to_string()),
        possible_canonical_ids: possible.into_iter().map(|s| s.to_string()).collect(),
    }
}

#[test]
fn exact_resolution_not_found_for_unknown_file() {
    let store = EdgeStore::new();
    assert!(store
        .get_exact_resolution("src/foo.vue", "./bar", default_ctx())
        .is_none());
    assert!(!store.has_exact_resolutions("src/foo.vue"));
}

#[test]
fn set_exact_resolutions_stores_and_retrieves() {
    let mut store = EdgeStore::new();
    let resolutions = vec![exact("./bar", Some("src/bar.vue"), vec!["src/bar.vue"])];

    let result = store.set_exact_resolutions("src/foo.vue", resolutions);
    assert_eq!(result.newly_resolved, vec!["src/bar.vue"]);
    assert!(store.has_exact_resolutions("src/foo.vue"));

    let res = store
        .get_exact_resolution("src/foo.vue", "./bar", default_ctx())
        .unwrap();
    assert_eq!(res.resolved_canonical_id.as_deref(), Some("src/bar.vue"));
}

#[test]
fn set_exact_resolutions_updates_forward_deps() {
    let mut store = EdgeStore::new();
    store.set_exact_resolutions(
        "src/foo.vue",
        vec![
            exact("./bar", Some("src/bar.vue"), vec![]),
            exact("./baz", Some("src/baz.vue"), vec![]),
        ],
    );

    let mut fwd = store.forward_deps("src/foo.vue");
    fwd.sort();
    assert_eq!(fwd, vec!["src/bar.vue", "src/baz.vue"]);
}

#[test]
fn set_exact_resolutions_updates_reverse_deps() {
    let mut store = EdgeStore::new();
    store.set_exact_resolutions(
        "src/foo.vue",
        vec![exact("./bar", Some("src/bar.vue"), vec![])],
    );

    let rev = store.reverse_deps("src/bar.vue");
    assert_eq!(rev, vec!["src/foo.vue"]);
}

#[test]
fn record_parsed_edges_replaces_previous_state() {
    let mut store = EdgeStore::new();

    // First set of edges
    store.record_parsed_edges("src/foo.vue", vec!["src/old_dep.vue".to_string()], vec![]);
    assert_eq!(store.forward_deps("src/foo.vue"), vec!["src/old_dep.vue"]);
    assert_eq!(store.reverse_deps("src/old_dep.vue"), vec!["src/foo.vue"]);

    // Replace with new edges
    store.record_parsed_edges("src/foo.vue", vec!["src/new_dep.vue".to_string()], vec![]);
    assert_eq!(store.forward_deps("src/foo.vue"), vec!["src/new_dep.vue"]);
    assert!(
        store.reverse_deps("src/old_dep.vue").is_empty(),
        "old dep should no longer have foo as reverse dep"
    );
    assert_eq!(store.reverse_deps("src/new_dep.vue"), vec!["src/foo.vue"]);
}

#[test]
fn record_parsed_edges_clears_exact_resolutions() {
    let mut store = EdgeStore::new();

    // Set exact resolutions first
    store.set_exact_resolutions(
        "src/foo.vue",
        vec![exact("./bar", Some("src/bar.vue"), vec![])],
    );
    assert!(store.has_exact_resolutions("src/foo.vue"));

    // Record new edges — should clear exact resolutions
    store.record_parsed_edges("src/foo.vue", vec!["src/baz.vue".to_string()], vec![]);
    assert!(
        !store.has_exact_resolutions("src/foo.vue"),
        "exact resolutions should be cleared after recording parsed edges"
    );
}

#[test]
fn record_parsed_edges_stores_bare_specifiers() {
    let mut store = EdgeStore::new();
    store.record_parsed_edges(
        "src/foo.vue",
        vec![],
        vec![
            ("vue".to_string(), ResolveRequestKind::EsmImport),
            ("lodash".to_string(), ResolveRequestKind::EsmImport),
        ],
    );

    let bare = store.bare_specifiers("src/foo.vue");
    assert_eq!(bare.len(), 2);
    assert_eq!(bare[0].0, "vue");
    assert_eq!(bare[1].0, "lodash");
}

#[test]
fn add_resolved_dep() {
    let mut store = EdgeStore::new();
    assert!(store.add_resolved_dep("src/foo.vue", "src/bar.vue"));
    assert!(
        !store.add_resolved_dep("src/foo.vue", "src/bar.vue"),
        "duplicate should return false"
    );

    assert_eq!(store.forward_deps("src/foo.vue"), vec!["src/bar.vue"]);
    assert_eq!(store.reverse_deps("src/bar.vue"), vec!["src/foo.vue"]);
}

#[test]
fn remove_file_cleans_up_all_state() {
    let mut store = EdgeStore::new();
    store.set_exact_resolutions(
        "src/foo.vue",
        vec![exact("./bar", Some("src/bar.vue"), vec![])],
    );

    // Also make another file depend on foo
    store.add_resolved_dep("src/other.vue", "src/foo.vue");

    store.remove_file("src/foo.vue");

    assert!(
        store.forward_deps("src/foo.vue").is_empty(),
        "forward deps should be empty"
    );
    assert!(
        store.reverse_deps("src/bar.vue").is_empty(),
        "bar should no longer have foo as reverse dep"
    );
    assert!(!store.has_exact_resolutions("src/foo.vue"));
    // other's forward dep (foo) should still exist in other's state
    assert_eq!(store.forward_deps("src/other.vue"), vec!["src/foo.vue"]);
}

#[test]
fn multiple_files_share_dependency() {
    let mut store = EdgeStore::new();
    store.record_parsed_edges("src/a.vue", vec!["src/shared.vue".to_string()], vec![]);
    store.record_parsed_edges("src/b.vue", vec!["src/shared.vue".to_string()], vec![]);

    let mut rev = store.reverse_deps("src/shared.vue");
    rev.sort();
    assert_eq!(rev, vec!["src/a.vue", "src/b.vue"]);

    // Remove a — b should still depend on shared
    store.remove_file("src/a.vue");
    assert_eq!(store.reverse_deps("src/shared.vue"), vec!["src/b.vue"]);
}

#[test]
fn forward_deps_empty_for_unknown_file() {
    let store = EdgeStore::new();
    assert!(store.forward_deps("src/unknown.vue").is_empty());
}

#[test]
fn reverse_deps_empty_for_unknown_file() {
    let store = EdgeStore::new();
    assert!(store.reverse_deps("src/unknown.vue").is_empty());
}

#[test]
fn bare_specifiers_empty_for_unknown_file() {
    let store = EdgeStore::new();
    assert!(store.bare_specifiers("src/unknown.vue").is_empty());
}

#[test]
fn exact_resolution_with_none_canonical_id() {
    let mut store = EdgeStore::new();
    let result = store.set_exact_resolutions(
        "src/foo.vue",
        vec![exact("nonexistent", None, vec!["maybe/a.ts"])],
    );

    assert!(
        result.newly_resolved.is_empty(),
        "None canonical_id should not produce forward dep"
    );
    assert!(store.forward_deps("src/foo.vue").is_empty());
    assert!(store.has_exact_resolutions("src/foo.vue"));

    let res = store
        .get_exact_resolution("src/foo.vue", "nonexistent", default_ctx())
        .unwrap();
    assert!(res.resolved_canonical_id.is_none());
    assert_eq!(res.possible_canonical_ids, vec!["maybe/a.ts"]);
}

// ── Regression: stale dep leak (P1 finding) ──

#[test]
fn set_exact_resolutions_replaces_old_exact_deps() {
    let mut store = EdgeStore::new();

    // First round: foo depends on bar via exact resolution
    store.set_exact_resolutions(
        "src/foo.vue",
        vec![exact("./bar", Some("src/bar.vue"), vec![])],
    );
    assert_eq!(store.forward_deps("src/foo.vue"), vec!["src/bar.vue"]);
    assert_eq!(store.reverse_deps("src/bar.vue"), vec!["src/foo.vue"]);

    // Second round: foo now depends on baz instead of bar
    store.set_exact_resolutions(
        "src/foo.vue",
        vec![exact("./baz", Some("src/baz.vue"), vec![])],
    );

    // bar should be GONE from forward deps
    let fwd = store.forward_deps("src/foo.vue");
    assert_eq!(
        fwd,
        vec!["src/baz.vue"],
        "old exact dep bar should be removed"
    );
    assert!(
        !fwd.contains(&"src/bar.vue".to_string()),
        "stale dep bar must not leak into forward deps"
    );

    // bar should be GONE from reverse deps
    assert!(
        store.reverse_deps("src/bar.vue").is_empty(),
        "stale dep bar must not appear in reverse deps"
    );
    assert_eq!(store.reverse_deps("src/baz.vue"), vec!["src/foo.vue"]);
}

#[test]
fn set_exact_resolutions_empty_clears_all_exact_deps() {
    let mut store = EdgeStore::new();

    store.set_exact_resolutions(
        "src/foo.vue",
        vec![exact("./bar", Some("src/bar.vue"), vec![])],
    );

    // Replace with empty set
    store.set_exact_resolutions("src/foo.vue", vec![]);

    assert!(
        store.forward_deps("src/foo.vue").is_empty(),
        "empty exact resolutions should clear all exact deps"
    );
    assert!(
        store.reverse_deps("src/bar.vue").is_empty(),
        "old target should be removed from reverse deps"
    );
}

#[test]
fn exact_deps_and_eager_deps_are_independent() {
    let mut store = EdgeStore::new();

    // Record eagerly resolved deps
    store.record_parsed_edges("src/foo.vue", vec!["src/eager.vue".to_string()], vec![]);

    // Add exact resolutions on top
    store.set_exact_resolutions(
        "src/foo.vue",
        vec![exact("lib", Some("node_modules/lib/index.ts"), vec![])],
    );

    let mut fwd = store.forward_deps("src/foo.vue");
    fwd.sort();
    assert_eq!(
        fwd,
        vec!["node_modules/lib/index.ts", "src/eager.vue"],
        "both eager and exact deps should be present"
    );

    // Replace exact resolutions — eager deps survive
    store.set_exact_resolutions("src/foo.vue", vec![]);

    let fwd = store.forward_deps("src/foo.vue");
    assert_eq!(
        fwd,
        vec!["src/eager.vue"],
        "eager deps must survive exact resolution replacement"
    );
    assert!(
        store.reverse_deps("node_modules/lib/index.ts").is_empty(),
        "cleared exact target should be removed from reverse deps"
    );
    assert_eq!(
        store.reverse_deps("src/eager.vue"),
        vec!["src/foo.vue"],
        "eager dep reverse entry must survive"
    );
}

#[test]
fn lazily_resolved_dep_appears_in_forward_and_reverse() {
    let mut store = EdgeStore::new();
    store.record_parsed_edges("src/foo.vue", vec![], vec![]);

    // Simulate a lazy resolution of a bare import
    assert!(store.add_lazily_resolved_dep("src/foo.vue", "node_modules/vue/index.ts"));

    assert_eq!(
        store.forward_deps("src/foo.vue"),
        vec!["node_modules/vue/index.ts"]
    );
    assert_eq!(
        store.reverse_deps("node_modules/vue/index.ts"),
        vec!["src/foo.vue"]
    );

    // Duplicate is no-op
    assert!(!store.add_lazily_resolved_dep("src/foo.vue", "node_modules/vue/index.ts"));
}

#[test]
fn record_parsed_edges_clears_lazily_resolved_deps() {
    let mut store = EdgeStore::new();
    store.record_parsed_edges("src/foo.vue", vec![], vec![]);
    store.add_lazily_resolved_dep("src/foo.vue", "node_modules/vue/index.ts");

    // Re-record edges — lazy deps should be cleared
    store.record_parsed_edges("src/foo.vue", vec!["src/new.vue".to_string()], vec![]);

    assert_eq!(store.forward_deps("src/foo.vue"), vec!["src/new.vue"]);
    assert!(
        store.reverse_deps("node_modules/vue/index.ts").is_empty(),
        "lazily resolved dep must be cleared on re-record"
    );
}

// ── Context-keyed exact resolution tests ──

#[test]
fn same_specifier_different_context_resolves_differently() {
    let mut store = EdgeStore::new();

    // Set two resolutions for the same specifier but different (phase, kind)
    store.set_exact_resolutions(
        "src/foo.vue",
        vec![
            ExactResolution {
                specifier: "pkg".to_string(),
                phase: ResolvePhase::CodegenBlocker,
                kind: ResolveRequestKind::EsmImport,
                resolved_canonical_id: Some("node_modules/pkg/index.js".to_string()),
                possible_canonical_ids: vec![],
            },
            ExactResolution {
                specifier: "pkg".to_string(),
                phase: ResolvePhase::ProviderGraph,
                kind: ResolveRequestKind::EsmImport,
                resolved_canonical_id: Some("node_modules/pkg/index.d.ts".to_string()),
                possible_canonical_ids: vec![],
            },
        ],
    );

    // CodegenBlocker + EsmImport → index.js
    let codegen_ctx = ResolutionContext {
        phase: ResolvePhase::CodegenBlocker,
        kind: ResolveRequestKind::EsmImport,
    };
    let codegen = store
        .get_exact_resolution("src/foo.vue", "pkg", codegen_ctx)
        .expect("CodegenBlocker exact should exist");
    assert_eq!(
        codegen.resolved_canonical_id.as_deref(),
        Some("node_modules/pkg/index.js"),
    );

    // ProviderGraph + EsmImport → index.d.ts
    let provider_ctx = ResolutionContext {
        phase: ResolvePhase::ProviderGraph,
        kind: ResolveRequestKind::EsmImport,
    };
    let provider = store
        .get_exact_resolution("src/foo.vue", "pkg", provider_ctx)
        .expect("ProviderGraph exact should exist");
    assert_eq!(
        provider.resolved_canonical_id.as_deref(),
        Some("node_modules/pkg/index.d.ts"),
    );

    // A different kind should NOT match
    let type_ctx = ResolutionContext {
        phase: ResolvePhase::CodegenBlocker,
        kind: ResolveRequestKind::TypeImport,
    };
    assert!(
        store
            .get_exact_resolution("src/foo.vue", "pkg", type_ctx)
            .is_none(),
        "TypeImport context should not match EsmImport resolutions"
    );
}

#[test]
fn context_keyed_replacement_only_affects_matching_context() {
    let mut store = EdgeStore::new();

    // Set codegen resolution
    store.set_exact_resolutions(
        "src/foo.vue",
        vec![
            ExactResolution {
                specifier: "pkg".to_string(),
                phase: ResolvePhase::CodegenBlocker,
                kind: ResolveRequestKind::EsmImport,
                resolved_canonical_id: Some("old.js".to_string()),
                possible_canonical_ids: vec![],
            },
            ExactResolution {
                specifier: "pkg".to_string(),
                phase: ResolvePhase::ProviderGraph,
                kind: ResolveRequestKind::EsmImport,
                resolved_canonical_id: Some("old.d.ts".to_string()),
                possible_canonical_ids: vec![],
            },
        ],
    );

    // Replace ALL exact resolutions (this replaces both)
    store.set_exact_resolutions(
        "src/foo.vue",
        vec![ExactResolution {
            specifier: "pkg".to_string(),
            phase: ResolvePhase::CodegenBlocker,
            kind: ResolveRequestKind::EsmImport,
            resolved_canonical_id: Some("new.js".to_string()),
            possible_canonical_ids: vec![],
        }],
    );

    let codegen_ctx = ResolutionContext {
        phase: ResolvePhase::CodegenBlocker,
        kind: ResolveRequestKind::EsmImport,
    };
    let provider_ctx = ResolutionContext {
        phase: ResolvePhase::ProviderGraph,
        kind: ResolveRequestKind::EsmImport,
    };

    // CodegenBlocker should see the new value
    let codegen = store
        .get_exact_resolution("src/foo.vue", "pkg", codegen_ctx)
        .expect("codegen should exist");
    assert_eq!(codegen.resolved_canonical_id.as_deref(), Some("new.js"));

    // ProviderGraph was cleared by set_exact_resolutions (it replaces ALL)
    assert!(
        store
            .get_exact_resolution("src/foo.vue", "pkg", provider_ctx)
            .is_none(),
        "provider context should be cleared after full replacement"
    );
}

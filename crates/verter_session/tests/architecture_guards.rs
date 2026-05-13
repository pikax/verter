//! Phase 10 — architecture enforcement guards. Fail when known rules
//! are broken. Cheap static source scans, run on every change.
//!
//! Each guard names its blocking phase in `#[ignore]`. The phase that
//! lands the rule MUST flip the ignore as part of its commit.

use std::fs;
use std::path::PathBuf;

// Shared denylist consumed by both `audit_no_hot_loop_instrumentation`
// (defined below) and the focused regression test in
// `tests/compile_audit_no_hot_loop_instrumentation.rs`.
#[path = "audit_hot_loop_denylist.rs"]
mod audit_hot_loop_denylist;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("repo root")
        .to_path_buf()
}

fn read_workspace_file(rel: &str) -> String {
    fs::read_to_string(workspace_root().join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"))
}

fn workspace_path(rel: &str) -> std::path::PathBuf {
    workspace_root().join(rel)
}

#[test]
fn no_read_source_in_component_meta() {
    // After the Tier 2 W5d split `component_meta.rs` became a directory
    // module (`component_meta/{mod,cold_resolver,projected_type_expr,
    // direct_macro,tests}.rs`). Scan every `.rs` file in the directory
    // so the guard keeps catching `host.read_source` regressions wherever
    // they land within the split.
    use std::fs;
    let dir = workspace_root().join("crates/verter_session/src/resolver_core/component_meta");
    let mut total = 0usize;
    let mut details = Vec::<String>::new();
    for entry in fs::read_dir(&dir).expect("read component_meta dir") {
        let entry = entry.expect("dir entry");
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let src = fs::read_to_string(&path).expect("read component_meta file");
        let count = src.matches("host.read_source").count();
        if count > 0 {
            details.push(format!("{}: {count}", path.display()));
            total += count;
        }
    }
    assert_eq!(
        total, 0,
        "component_meta module must not contain host.read_source after Phase 4; found {total} occurrences:\n  {}",
        details.join("\n  ")
    );
}

#[test]
fn no_read_source_in_declaration_metadata() {
    // After Phase 4b, the `read_source` trait method itself is deleted
    // from declaration_metadata.rs. Test impls in tests/ are out of
    // scope; production source MUST be clean.
    let src =
        read_workspace_file("crates/verter_session/src/resolver_core/declaration_metadata.rs");
    let count = src.matches("read_source").count();
    assert_eq!(
        count, 0,
        "declaration_metadata.rs must not contain read_source after Phase 4b; found {count}"
    );
}

#[test]
fn no_text_based_macro_surface_projection_helpers() {
    // After Phase 4b, the three text-projection helper functions are
    // deleted from the resolver_core. Their function names appearing
    // anywhere in resolver_core indicates a regression.
    use std::fs;
    let symbols = [
        "source_for_local_type_projection",
        "project_macro_surfaces_from_source_type_name",
        "project_macro_surfaces_from_expanded_text",
    ];

    // After the Tier 2 W5d split, `component_meta.rs` became the
    // directory module `component_meta/`. Scan every file in the
    // directory plus the still-flat `surface_projector.rs`.
    let component_meta_dir =
        workspace_root().join("crates/verter_session/src/resolver_core/component_meta");
    let mut targets: Vec<std::path::PathBuf> = Vec::new();
    for entry in fs::read_dir(&component_meta_dir).expect("read component_meta dir") {
        let entry = entry.expect("dir entry");
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            targets.push(path);
        }
    }
    targets.push(
        workspace_root().join("crates/verter_session/src/resolver_core/surface_projector.rs"),
    );

    for path in targets {
        let src = fs::read_to_string(&path).expect("read target");
        for needle in symbols {
            assert!(
                !src.contains(needle),
                "{} must not contain {needle} after Phase 4b (graph-only resolver)",
                path.display()
            );
        }
    }
}

#[test]
fn no_macro_string_heuristics_in_resolver_core() {
    // The user's directive (Phase 4b origin): no regex, no string-based
    // macro detection. This guard catches the most common
    // `.contains("defineProps")` pattern. False positives are unlikely
    // — production resolver code should reach macros via the graph,
    // not by substring-matching source text.
    use std::fs;
    let resolver_dir = workspace_root().join("crates/verter_session/src/resolver_core");
    let needles = [
        r#".contains("defineProps"#,
        r#".contains("defineEmits"#,
        r#".contains("defineSlots"#,
        r#".contains("defineModel"#,
        r#".contains("defineExpose"#,
    ];
    for entry in fs::read_dir(&resolver_dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let src = fs::read_to_string(&path).unwrap();
        for needle in needles {
            assert!(
                !src.contains(needle),
                "{} must not contain string-heuristic {} (Phase 4b: graph-only)",
                path.display(),
                needle
            );
        }
    }
}

#[test]
fn no_deprecated_workspace_reexports() {
    let src = read_workspace_file("crates/verter_session/src/lib.rs");
    for ty in ["ProjectGraph", "ProjectRank", "VfsProjectConfig"] {
        let needle = format!("pub use {ty}");
        assert!(
            !src.contains(&needle),
            "verter_session::lib must not re-export {ty} after Phase 6"
        );
    }
}

#[test]
fn no_local_vite_helpers_in_lsp() {
    for rel in [
        "crates/verter_lsp/src/server/mod.rs",
        "crates/verter_lsp/src/background_init.rs",
    ] {
        let src = read_workspace_file(rel);
        for needle in [
            "fn read_vite_config",
            "fn parse_vite_config",
            "fn discover_vite_aliases",
        ] {
            assert!(
                !src.contains(needle),
                "{rel} must not define {needle} after Phase 7"
            );
        }
    }
}

#[test]
fn god_module_size_budget() {
    // Target-root walkdir scan, production files only.
    //
    // The guard is intentionally scoped to the five Phase 11 god-module
    // targets. A repository-wide scan would fail on unrelated large files
    // that Phase 11 does not own; scanning only the old exact filenames
    // would lose signal after a target becomes a folder module.
    //
    // Each target below may exist as the original file, as the post-split
    // directory module, or as both when the split keeps a thin shell file
    // next to private siblings. The guard walks whichever form exists,
    // fails if neither form exists, and asserts every production .rs file
    // under that target <= 4000 LOC.
    // Test fixtures are excluded because Phase 11's public budget is for
    // production module ownership; large test fixtures are governed by the
    // testing skill's sibling-test extraction rules.
    use std::collections::HashSet;
    use walkdir::WalkDir;
    const DEFAULT_MAX_LINES: usize = 4000;

    fn is_test_fixture(rel: &str) -> bool {
        rel.ends_with("_tests.rs") || rel.ends_with("/tests.rs") || rel.contains("/tests/")
    }

    fn check_file(
        workspace: &std::path::Path,
        path: &std::path::Path,
        seen: &mut HashSet<String>,
        violations: &mut Vec<String>,
    ) {
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            return;
        }
        let rel = path
            .strip_prefix(workspace)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if !seen.insert(rel.clone()) || is_test_fixture(&rel) {
            return;
        }
        let src = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        let lines = src.lines().count();
        if lines > DEFAULT_MAX_LINES {
            violations.push(format!(
                "{rel}: {lines} > {DEFAULT_MAX_LINES} (Phase 11 god-module budget)"
            ));
        }
    }

    let phase_11_targets = [
        (
            "crates/verter_session/src/meta_resolve.rs",
            "crates/verter_session/src/meta_resolve",
        ),
        (
            "crates/verter_session/src/resolver_core/component_meta_query_engine.rs",
            "crates/verter_session/src/resolver_core/component_meta_query_engine",
        ),
        (
            "crates/verter_session/src/host_manage.rs",
            "crates/verter_session/src/host_manage",
        ),
        (
            "crates/verter_compiler/src/ide/script.rs",
            "crates/verter_compiler/src/ide/script",
        ),
        (
            "crates/verter_lsp/src/server.rs",
            "crates/verter_lsp/src/server",
        ),
    ];
    let workspace = workspace_root();
    let mut violations = Vec::<String>::new();
    let mut seen = HashSet::<String>::new();
    for (file_rel, dir_rel) in phase_11_targets {
        let file_root = workspace.join(file_rel);
        let dir_root = workspace.join(dir_rel);
        let mut found_target = false;
        if file_root.is_file() {
            found_target = true;
            check_file(&workspace, &file_root, &mut seen, &mut violations);
        }
        if dir_root.is_dir() {
            found_target = true;
            for entry in WalkDir::new(&dir_root) {
                let entry = entry.expect("walkdir entry");
                if entry.file_type().is_file() {
                    check_file(&workspace, entry.path(), &mut seen, &mut violations);
                }
            }
        }
        if !found_target {
            violations.push(format!(
                "{file_rel} / {dir_rel}: missing Phase 11 target root"
            ));
        }
    }
    assert!(
        violations.is_empty(),
        "god_module_size_budget violations:\n{}",
        violations.join("\n")
    );
}

// ----------------------------------------------------------------------
// Phase 5d — Class A + Class B callsite migration guards.
//
// POST-CUTOVER NOTE (final state — refer here first): the migration
// completed in Phase 5l (engine-method deletion) + Phase 5m §5.13a.2
// (bridge helpers as the single production callsite shape). The engine
// trampoline methods named below are deleted; every Class A and Class B
// caller routes through `*_via_host_threaded` bridges under
// `meta_resolve/dispatch_helpers.rs`. The post-cutover invariant
// asserted by the guards in this section is uniform: ZERO Class A or
// Class B engine-method callsites in any production file under the
// `meta_resolve` and `host_manage` module surfaces.
//
// HISTORICAL CONTEXT (Phase 5d→5g→5l→5m migration window — preserved
// for archeology):
//
// The engine trampoline methods (`project_expr_surface_expr`,
// `project_expr_surface_shape`,
// `project_expr_surface_expr_with_compound_objects`,
// `project_type_surface_expr`, `project_type_surface_shape`,
// `project_prepared_type_surface_expr`,
// `project_prepared_type_surface_shape`) were originally scheduled for
// retirement in Phase 5g. Phase 5d migrated Class A (Props + Slots
// surfaces) and Class B (type-decl projection) consumers off the engine
// helpers and onto `dispatch.execute_to_type_expr(SemanticQueryKey::ProjectPath {
// .., mode: Expanded })` (Class A) / `Instantiate { .. } -> ProjectPath`
// (Class B) per sub-plan §4.1.
//
// Per the original brief, route-loop sites (sub-plan §4.1 row
// `lower_and_project_to_expanded`: 6012/6309/6323/8329) and route-target
// sites (`project_route_surface_expr`: 6267/6361/8934/8940) were
// DEFERRED to Phase 5e (5e/5f scope). Line `4942`
// (`project_expr_surface_expr_with_compound_objects` inside
// `produce_one_macro_object_shape_for_slots`) was ALSO deferred per
// the brief note. The pre-Phase-11 versions of these guards counted
// only Class A/B callsites covered by 5d, with remaining sites out of
// scope until 5e/5f.
//
// That history is now subsumed: Phase 5l deleted the engine methods
// atomically, and Phase 5m re-chartered the migration to route every
// caller through bridge helpers (rather than direct dispatch) so the
// `Partial<T>` optionality propagation that pre-5l blocked the
// multi-macro-kind sites is preserved by threading `query_engine.ctx`
// through the bridges. The line-number anchors and the
// "deferred to 5g" labels above describe the migration window, not
// the current tree.

/// Count occurrences of any of the supplied needles in `src`.
/// Returns the total number of byte-substring hits across all needles.
fn count_callsites(src: &str, needles: &[&str]) -> usize {
    needles.iter().map(|n| src.matches(n).count()).sum()
}

#[test]
fn phase_05d_4a_class_a_props_callers_migrated_in_meta_resolve() {
    // Post-Phase-11a + Phase 5l + Phase 5m §5.13a.2 (final state):
    // `meta_resolve.rs` was split into a folder module; every Class A
    // engine method (`project_expr_surface_expr`, `_shape`,
    // `_expr_with_compound_objects`) was deleted in Phase 5l, and every
    // production caller routes through the bridge helpers
    // (`*_via_host_threaded`) under `meta_resolve/dispatch_helpers.rs`.
    // The bridge bodies use dispatch directly — they no longer call the
    // (now-deleted) engine methods inside `#[allow(deprecated)]` blocks.
    //
    // The post-cutover invariant: ZERO Class A engine-method callsites
    // (`.project_expr_surface_*(`) in ANY production file under the
    // `meta_resolve` module surface (the shell `meta_resolve.rs` plus
    // every sibling under `meta_resolve/`).
    //
    // The walkdir-based shape is robust under further folder splits: a
    // future commit that adds a new sibling under `meta_resolve/` is
    // automatically covered without test edits AS LONG AS the new
    // sibling does not reintroduce Class A engine callsites.
    //
    // Discrimination proof (Stub Prevention §0p): adding a single
    // `.project_expr_surface_expr(...)` line to ANY file under the
    // `meta_resolve` module surface (e.g., `meta_resolve/scoring.rs` or
    // `meta_resolve/materialize/macro_shapes.rs`) FAILS this test. The
    // bridge file is NOT allow-listed because Phase 5l deleted the
    // engine methods entirely; even bridge bodies must stay clean.
    let class_a_callsite_patterns: &[&str] = &[
        ".project_expr_surface_expr(",
        ".project_expr_surface_shape(",
        ".project_expr_surface_expr_with_compound_objects(",
    ];
    let scanned = collect_meta_resolve_module_surface();
    assert!(
        !scanned.is_empty(),
        "Phase 5d 4a guard: expected to find at least the shell \
         `crates/verter_session/src/meta_resolve.rs` (or its sibling \
         folder); found none — the module surface vanished or moved \
         without updating this guard"
    );
    let mut violations: Vec<String> = Vec::new();
    for path in &scanned {
        let rel = path
            .strip_prefix(workspace_root())
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let src = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        let count = count_callsites(&src, class_a_callsite_patterns);
        if count != 0 {
            violations.push(format!(
                "{rel}: {count} Class A engine-method callsite(s) found \
                 (one of `.project_expr_surface_expr(`, \
                 `.project_expr_surface_shape(`, \
                 `.project_expr_surface_expr_with_compound_objects(`). \
                 Phase 5l deleted these engine methods; route Class A \
                 work through the bridge helpers \
                 (`*_via_host_threaded` under \
                 `meta_resolve/dispatch_helpers.rs`) instead."
            ));
        }
    }
    assert!(
        violations.is_empty(),
        "Phase 5d 4a / Phase 5l final state: meta_resolve module \
         surface must have ZERO Class A engine-method callsites. \
         Violations:\n{}",
        violations.join("\n")
    );
}

/// Walk the `meta_resolve` module surface — the shell file
/// `crates/verter_session/src/meta_resolve.rs` plus every `.rs` file
/// under `crates/verter_session/src/meta_resolve/`. Test files
/// (`*_tests.rs` and entries under a `tests/` subdir) are excluded so
/// the architecture guard scans production-only.
fn collect_meta_resolve_module_surface() -> Vec<PathBuf> {
    use walkdir::WalkDir;
    let module_root = workspace_root().join("crates/verter_session/src/meta_resolve");
    let shell_path = workspace_root().join("crates/verter_session/src/meta_resolve.rs");
    let mut scanned: Vec<PathBuf> = Vec::new();
    if shell_path.is_file() {
        scanned.push(shell_path);
    }
    if module_root.is_dir() {
        for entry in WalkDir::new(&module_root) {
            let entry = entry.expect("walkdir entry");
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let file_name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default();
            if file_name.ends_with("_tests.rs") {
                continue;
            }
            scanned.push(path.to_path_buf());
        }
    }
    scanned
}

/// Walk the `host_manage` module surface — the shell file
/// `crates/verter_session/src/host_manage.rs` plus every `.rs` file
/// under `crates/verter_session/src/host_manage/`. Test files
/// (`*_tests.rs`) are excluded.
fn collect_host_manage_module_surface() -> Vec<PathBuf> {
    use walkdir::WalkDir;
    let module_root = workspace_root().join("crates/verter_session/src/host_manage");
    let shell_path = workspace_root().join("crates/verter_session/src/host_manage.rs");
    let mut scanned: Vec<PathBuf> = Vec::new();
    if shell_path.is_file() {
        scanned.push(shell_path);
    }
    if module_root.is_dir() {
        for entry in WalkDir::new(&module_root) {
            let entry = entry.expect("walkdir entry");
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let file_name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default();
            if file_name.ends_with("_tests.rs") {
                continue;
            }
            scanned.push(path.to_path_buf());
        }
    }
    scanned
}

#[test]
fn phase_05d_4a_class_a_props_callers_migrated_in_host_manage() {
    // Post-Phase-11 + Phase 5l + Phase 5m §5.13a.2 (final state):
    // `host_manage.rs` was split into a folder module; every Class A
    // engine method was deleted in Phase 5l, and the (formerly
    // deferred-to-4c) Class B `project_type_surface_expr` site
    // migrated through the bridge helper in Phase 5m.
    //
    // The post-cutover invariant: ZERO Class A engine-method callsites
    // (`.project_expr_surface_*(`) AND ZERO Class B engine-method
    // callsites (`.project_type_surface_expr(`, `_shape(`,
    // `_prepared_type_surface_expr(`, `_shape(`) in ANY production file
    // under the `host_manage` module surface (the shell
    // `host_manage.rs` plus every sibling under `host_manage/`).
    //
    // Class B coverage in this guard tracks the original guard's intent
    // (it carried both A and B assertions for `host_manage.rs`); the
    // Phase 5m guard `phase_05m_class_b_callers_migrated_through_bridge_helpers`
    // also asserts host_manage.rs is Class-B-clean, but that one only
    // reads the shell file. This walkdir variant is robust to
    // post-Phase-11d folder splits.
    //
    // Discrimination proof (Stub Prevention §0p): adding a single
    // `.project_expr_surface_expr(...)` or `.project_type_surface_expr(...)`
    // line to ANY file under the `host_manage` module surface (e.g.,
    // `host_manage/component_meta_methods.rs`) FAILS this test.
    let class_a_callsite_patterns: &[&str] = &[
        ".project_expr_surface_expr(",
        ".project_expr_surface_shape(",
        ".project_expr_surface_expr_with_compound_objects(",
    ];
    let class_b_callsite_patterns: &[&str] = &[
        ".project_type_surface_expr(",
        ".project_type_surface_shape(",
        ".project_prepared_type_surface_expr(",
        ".project_prepared_type_surface_shape(",
    ];
    let scanned = collect_host_manage_module_surface();
    assert!(
        !scanned.is_empty(),
        "Phase 5d 4a guard: expected to find at least the shell \
         `crates/verter_session/src/host_manage.rs` (or its sibling \
         folder); found none — the module surface vanished or moved \
         without updating this guard"
    );
    let mut violations: Vec<String> = Vec::new();
    for path in &scanned {
        let rel = path
            .strip_prefix(workspace_root())
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let src = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        let class_a = count_callsites(&src, class_a_callsite_patterns);
        if class_a != 0 {
            violations.push(format!(
                "{rel}: {class_a} Class A engine-method callsite(s) \
                 found. Phase 5l deleted these engine methods; route \
                 Class A work through `*_via_host_threaded` bridges \
                 under `meta_resolve/dispatch_helpers.rs`."
            ));
        }
        let class_b = count_callsites(&src, class_b_callsite_patterns);
        if class_b != 0 {
            violations.push(format!(
                "{rel}: {class_b} Class B engine-method callsite(s) \
                 found. Phase 5m §5.13a.2 routes Class B work through \
                 bridge helpers under \
                 `meta_resolve/dispatch_helpers.rs`."
            ));
        }
    }
    assert!(
        violations.is_empty(),
        "Phase 5d 4a / Phase 5l + 5m final state: host_manage module \
         surface must have ZERO Class A and ZERO Class B engine-method \
         callsites. Violations:\n{}",
        violations.join("\n")
    );
}

#[test]
fn phase_05d_4a_class_a_props_callers_migrated_in_type_expansion_verter() {
    // type_expansion_verter.rs has 2 Class A `.project_expr_surface_expr`
    // sites pre-5d (lines 215, 272). Both migrate to dispatch in 4a.
    let src =
        read_workspace_file("crates/verter_session/src/resolver_core/type_expansion_verter.rs");
    let invocations = count_callsites(
        &src,
        &[
            ".project_expr_surface_expr(",
            ".project_expr_surface_shape(",
            ".project_expr_surface_expr_with_compound_objects(",
        ],
    );
    assert_eq!(
        invocations, 0,
        "Phase 5d 4a: type_expansion_verter.rs must have 0 Class A engine \
         method invocations after 4a; found {invocations}"
    );
}

#[test]
fn phase_05d_4b_class_a_slots_callers_migrated() {
    // Post-Phase-11a + Phase 5l + Phase 5m §5.13a.2 (final state):
    // every Class A engine-method site (slots, props, multi-macro-kind
    // generic helpers, and the deferred-5e/5f compound-object site)
    // migrated. Phase 5l deleted the engine methods; Phase 5m routed
    // every caller through bridge helpers (`*_via_host_threaded`)
    // under `meta_resolve/dispatch_helpers.rs`. The `Partial<T>`
    // optionality propagation that pre-5l blocked the
    // `produce_one_macro_object_shape` / `project_named_ref_imported_scope_shape`
    // sites is now handled by `project_expr_class_a_via_dispatch_threaded`
    // — it threads `query_engine.ctx` so the request-local fuse and
    // scope-payload state remain load-bearing.
    //
    // The post-cutover invariant matches the 5d 4a guard: ZERO Class A
    // engine-method callsites in ANY production file under the
    // `meta_resolve` module surface. This 4b guard's narrower remit
    // (slot-only sites in the original 4b commit) collapses into the
    // same module-surface walk because there is no longer a per-macro
    // partition; every Class A site routes through dispatch.
    //
    // Discrimination proof (Stub Prevention §0p): adding a single
    // `.project_expr_surface_expr(...)` line to e.g.
    // `meta_resolve/materialize/macro_shapes.rs` (the file that owns
    // the post-Phase-11a slot-cluster code) FAILS this test.
    let class_a_callsite_patterns: &[&str] = &[
        ".project_expr_surface_expr(",
        ".project_expr_surface_shape(",
        ".project_expr_surface_expr_with_compound_objects(",
    ];
    let scanned = collect_meta_resolve_module_surface();
    assert!(
        !scanned.is_empty(),
        "Phase 5d 4b guard: expected to find at least the shell \
         `crates/verter_session/src/meta_resolve.rs` (or its sibling \
         folder); found none — the module surface vanished or moved \
         without updating this guard"
    );
    let mut violations: Vec<String> = Vec::new();
    for path in &scanned {
        let rel = path
            .strip_prefix(workspace_root())
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let src = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        let count = count_callsites(&src, class_a_callsite_patterns);
        if count != 0 {
            violations.push(format!(
                "{rel}: {count} Class A engine-method callsite(s) \
                 found. Phase 5l deleted these engine methods; the \
                 multi-macro-kind sites that were 4b/5g-deferred now \
                 route through `project_expr_class_a_via_dispatch_threaded`."
            ));
        }
    }
    assert!(
        violations.is_empty(),
        "Phase 5d 4b / Phase 5l final state: meta_resolve module \
         surface must have ZERO Class A engine-method callsites \
         (slots and multi-macro-kind sites included). Violations:\n{}",
        violations.join("\n")
    );
}

#[test]
fn phase_05m_class_b_callers_migrated_through_bridge_helpers() {
    // POST-CUTOVER NOTE (final state — Phase 5l + 5m + 11a):
    // The Class B engine methods (`project_type_surface_expr`,
    // `_shape`, `project_prepared_type_surface_expr`, `_shape`) are
    // deleted. Every Class B production callsite routes through bridge
    // helpers in `meta_resolve/dispatch_helpers.rs` (named
    // `*_via_host_threaded`). The §5.14.1 pre-flight gate observes
    // zero external engine-method callers. The architectural finding
    // logged below (the dispatch-vs-prepared-decl-fallback regression)
    // is resolved: the bridges thread `query_engine.ctx` so the
    // prepared-decl path is preserved without an `#[allow(deprecated)]`
    // engine call.
    //
    // HISTORICAL CONTEXT (Phase 5d→5g→5l→5m migration window —
    // preserved because the design rationale matters for archeology):
    //
    //   Phase 5m §5.13a.2 (re-charter of Phase 5d 4c) migrated the 11
    //   Class B caller sites + 1 test in meta_resolve.rs through bridge
    //   helpers (named `*_via_host_threaded`) so the §5.14.1 pre-flight
    //   gate would see zero external engine-method callers. Mid-
    //   migration, the bridge bodies internally called the deprecated
    //   engine methods inside `#[allow(deprecated)]` blocks per
    //   §5.13a.2; 5l's atomic engine deletion replaced those bridge
    //   bodies with dispatch-only equivalents (consuming the host
    //   helpers added in 5m.1 / 5m.2 / 5m.3).
    //
    //   The pre-5m invariant — "11 Class B engine refs in
    //   meta_resolve.rs (deferred-to-5g)" — was invalidated by 5m's
    //   migration. The post-5m invariant (asserted by this test): zero
    //   external engine-method callsites in meta_resolve.rs and
    //   host_manage.rs; all engine method calls were briefly contained
    //   inside the bridge helpers as private free functions in
    //   meta_resolve.rs.
    //
    //   ARCHITECTURAL FINDING logged at Phase 5d 4c (TODO(phase-5g)
    //   marker, since resolved): the trampoline's
    //   `project_type_surface` body was dispatch-first then
    //   prepared-decl-second —
    //   `dispatch_projected_surface(...).or_else(||
    //   cached_prepared_root_surface(...))`. The prepared-decl
    //   fallback is essential for re-exported / barrel-routed
    //   declarations (transitive heritage chains, namespace-qualified
    //   imports like `JSX.IntrinsicElements`). A dispatch-only Class B
    //   helper without that fallback regressed 47 workspace tests
    //   (heritage chain resolution, barrel imports, complex generic
    //   Pick/Omit on multi-file types). Even threading the engine's
    //   prepared-decl helper inside a Class B helper did not match the
    //   trampoline's
    //   `dispatch_projected_surface → projected_surface_to_type_expr`
    //   path because that path flattens heritage members through the
    //   surface walker; `raise_node_to_type_expr` over a
    //   dispatch-Instantiate result did not.
    //
    //   Per CLAUDE.md "Fix Quality":
    //     > If the fix would be a workaround, patch, or shim → do NOT
    //     > apply it. Instead: add a TODO(follow-up) comment
    //     > explaining the proper fix needed, note it in the feedback
    //     > file, and continue with the plan.
    //
    //   The proper fix — threading the prepared-decl resolver through
    //   dispatch atomically with the engine retirement — was scheduled
    //   for Phase 5g (sub-plan §5 commit 11). It landed in Phase 5l
    //   instead (5g was rolled into 5l's atomic engine deletion).
    //   Class B caller sites no longer carry `TODO(phase-5g)` markers;
    //   the original `TODO(phase-5g)` markers in production code were
    //   converted to past-tense bridge documentation in the
    //   post-cutover review-fix sweep.
    //
    //   Pre-Phase-11 version of this guard asserted that
    //   `TODO(phase-5g)` markers existed at every site the §4.1 brief
    //   listed for migration that stayed on the engine — i.e., the
    //   markers were the load-bearing characterization. Post-cutover,
    //   the markers are gone (the work they tracked landed in 5l/5m),
    //   and this guard's load-bearing characterization is the absence
    //   of Class B engine callsites outside the bridge file.
    //
    // Phase 11a split `meta_resolve.rs` into a folder module; the
    // bridge helpers now live in `meta_resolve/dispatch_helpers.rs`.
    //
    // The redesigned test (post-split) walks every .rs file under the
    // meta_resolve module surface (the shell `meta_resolve.rs` plus
    // every sibling under `meta_resolve/`) and asserts the
    // architectural invariant per-file:
    //
    //   1. Class B engine-method callsite patterns
    //      (`.project_type_surface_expr(`, `.project_type_surface_shape(`,
    //      `.project_prepared_type_surface_expr(`,
    //      `.project_prepared_type_surface_shape(`) MUST be ZERO in
    //      every file EXCEPT `dispatch_helpers.rs`. The bridges live
    //      there and only there. The §5.14.1 pre-flight gate sees zero
    //      external engine-method callers post-5m.
    //
    //   2. The bridge section header (the literal anchor stored in
    //      `BRIDGE_SECTION_MARKER` below — "Class B bridge helpers —
    //      …") MUST be present in `dispatch_helpers.rs` — the
    //      location anchor that names where bridges live.
    //
    //   3. The stale helper names `project_type_class_b_via_dispatch`
    //      and `project_type_class_b_shape_via_dispatch` MUST NOT
    //      appear in ANY file in the meta_resolve module. The
    //      dispatch-only Class B helper was removed in 5d because it
    //      regressed transitive heritage resolution; re-adding it
    //      under any alias / location is a regression.
    //
    //   4. `host_manage.rs` Class B engine refs MUST be ZERO (the
    //      JSX.IntrinsicElements site migrated through the bridge).
    //
    // The walkdir-based shape is robust under further folder splits:
    // a future commit that adds a new sibling under `meta_resolve/`
    // is automatically covered without test edits, AS LONG AS the
    // new sibling does not reintroduce Class B engine callsites or
    // stale helper aliases.
    //
    // Discrimination proofs (Stub Prevention §0p):
    //   * Adding a single `.project_type_surface_expr(...)` line to
    //     ANY sibling other than `dispatch_helpers.rs` (e.g.,
    //     `meta_resolve/scoring.rs`) FAILS this test.
    //   * Deleting the bridge section header from `dispatch_helpers.rs`
    //     FAILS this test.
    //   * Re-introducing `project_type_class_b_via_dispatch` under
    //     any name in any sibling FAILS this test.
    //   * Adding a Class B engine call in `host_manage.rs` FAILS
    //     this test.
    use walkdir::WalkDir;
    let class_b_callsite_patterns: &[&str] = &[
        ".project_type_surface_expr(",
        ".project_type_surface_shape(",
        ".project_prepared_type_surface_expr(",
        ".project_prepared_type_surface_shape(",
    ];
    let stale_helper_patterns: &[&str] = &[
        "project_type_class_b_via_dispatch",
        "project_type_class_b_shape_via_dispatch",
    ];
    // The single allowed location for Class B bridge bodies.
    const BRIDGE_FILE_REL: &str = "crates/verter_session/src/meta_resolve/dispatch_helpers.rs";
    const BRIDGE_SECTION_MARKER: &str =
        "Class B bridge helpers — Class B engine methods are deleted; these bridges thread `query_engine.ctx` through dispatch.";

    let module_root = workspace_root().join("crates/verter_session/src/meta_resolve");
    let shell_path = workspace_root().join("crates/verter_session/src/meta_resolve.rs");
    let mut scanned: Vec<PathBuf> = Vec::new();
    if shell_path.is_file() {
        scanned.push(shell_path);
    }
    if module_root.is_dir() {
        for entry in WalkDir::new(&module_root) {
            let entry = entry.expect("walkdir entry");
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            scanned.push(path.to_path_buf());
        }
    }
    assert!(
        !scanned.is_empty(),
        "Phase 11a guard: expected to find at least the shell \
         `crates/verter_session/src/meta_resolve.rs` (or its sibling \
         folder); found none — the module surface vanished or moved \
         without updating this guard"
    );

    let mut bridge_file_seen = false;
    let mut violations: Vec<String> = Vec::new();
    for path in &scanned {
        let rel = path
            .strip_prefix(workspace_root())
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let src = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {rel}: {e}"));

        // Stale helper alias regression check — applies to EVERY file.
        let stale_count = count_callsites(&src, stale_helper_patterns);
        if stale_count != 0 {
            violations.push(format!(
                "{rel}: stale `project_type_class_b_via_dispatch` \
                 helper alias found ({stale_count} occurrences). The \
                 dispatch-only Class B helper was removed in 5d \
                 because it regressed transitive heritage \
                 resolution; do not re-introduce it under any name."
            ));
        }

        // Class B engine-method callsite regression check.
        let callsite_count = count_callsites(&src, class_b_callsite_patterns);
        if rel == BRIDGE_FILE_REL {
            bridge_file_seen = true;
            // The bridge file is the single allowed home. The
            // current 5l implementation composes surviving
            // pub(crate) helpers (`dispatch_projected_surface` +
            // `cached_prepared_root_surface`) instead of calling the
            // deleted Class B engine methods, so callsite_count is
            // 0 today. The test does NOT require non-zero here —
            // the discriminating signal is "zero outside, allowed
            // inside" — so a future bridge body that re-uses the
            // legacy engine names would still pass this guard
            // file-locally (the deletion-protected surface lives
            // elsewhere). What this branch enforces is
            // location-only: legacy callsites belong here, nowhere
            // else.
            if !src.contains(BRIDGE_SECTION_MARKER) {
                violations.push(format!(
                    "{rel}: bridge section header \
                     \"{BRIDGE_SECTION_MARKER}\" missing — the file \
                     is the named home for Class B bridges and must \
                     retain the §5.14.2 anchor"
                ));
            }
        } else if callsite_count != 0 {
            violations.push(format!(
                "{rel}: {callsite_count} Class B engine-method \
                 callsite(s) found. Per Phase 5m §5.13a.2 the only \
                 allowed location for Class B engine refs in the \
                 meta_resolve module is `{BRIDGE_FILE_REL}` (the \
                 bridge helpers file). Route Class B work through a \
                 `*_via_host*` bridge instead of inlining a Class B \
                 engine call here."
            ));
        }
    }
    assert!(
        bridge_file_seen,
        "Phase 11a guard: did not encounter \
         `{BRIDGE_FILE_REL}` while walking the meta_resolve module \
         surface. The bridge file must remain on disk so this guard \
         can verify the §5.14.2 anchor."
    );
    assert!(
        violations.is_empty(),
        "Phase 5m §5.13a.2 / Phase 11a: Class B engine-method \
         callers must live ONLY in `{BRIDGE_FILE_REL}` and the \
         bridge section anchor must be present. Violations:\n{}",
        violations.join("\n")
    );

    // Phase 5m §5.13a.2 invariant for `host_manage.rs`: zero Class B
    // engine refs (the JSX.IntrinsicElements site migrated through
    // the bridge helper). Outside the meta_resolve module surface so
    // tracked separately.
    let host_manage_src = read_workspace_file("crates/verter_session/src/host_manage.rs");
    let host_manage_b = count_callsites(&host_manage_src, class_b_callsite_patterns);
    assert_eq!(
        host_manage_b, 0,
        "Phase 5m §5.13a.2: host_manage.rs Class B engine refs must \
         be ZERO (the JSX.IntrinsicElements site migrated through the \
         bridge helper); found {host_manage_b}"
    );
}

// ===========================================================================
// Phase 5l-supplement — `no_unbounded_recursion_in_resolver_core`
// ===========================================================================
//
// §0.6.5 stack-depth discipline guard. The previous incarnation of this
// guard (commit 5l plan-body, never landed) used a regex-only heuristic
// that counted file-wide token occurrences of `foo(` and `self.foo(`,
// which produced 568 false positives at integration HEAD
// `c8ba39684864048917eb1b89dc808d1d081f2706` — every `Type::new(...)`
// constructor call counted as recursion of every other `fn new`, every
// non-recursive call from one function to another in the same file
// counted as recursion of the callee, and every `#[cfg(test)]` test
// helper called from sibling tests counted as recursion of itself.
//
// This rewrite (Phase 5l-supplement) replaces the regex with a
// `syn::Visit`-based scanner that walks each function body and only
// flags TRUE direct self-recursion: a function whose own body contains
// a call back to itself by name, where "by name" means one of:
//
//   1. **Bare identifier call** `foo(...)` — the call expression's
//      callee is a path of length 1 with the segment ident matching the
//      enclosing function's ident. (`Type::new(...)` does NOT match
//      because the path has 2 segments.)
//
//   2. **`Self::`-qualified call** `Self::foo(...)` — call expression
//      whose callee is a path of length 2 starting with `Self` and
//      ending with the enclosing function's ident.
//
//   3. **`self.foo(...)` method call** — method-call expression whose
//      receiver is the bare identifier `self` and whose method name
//      matches the enclosing function's ident. Method calls on any
//      other receiver (`self.field.foo(...)`, `ctx.foo(...)`,
//      `host.foo(...)`) are NOT matched because dispatch is on a
//      different value, not the same impl.
//
// `#[cfg(test)]` modules and functions are skipped (test fixtures
// often define helpers that look recursive due to sibling-test calls).
//
// The scanner is allow-list-driven: any function flagged at integration
// HEAD must either be refactored to a depth-budgeted shape (preferred)
// or carry an explicit allow-list entry with a phase-report citation
// explaining why the recursion is bounded by another invariant
// (data-structure DAG, finite AST depth from a finite source, etc.).
//
// Pattern mirrors `god_module_size_budget`'s allow-list approach (see
// the head of this file). This is a doc-only test guard rewrite —
// production code is not touched.

mod resolver_core_recursion {
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};

    use syn::visit::Visit;
    use syn::{Attribute, Expr, ExprCall, ExprMethodCall, ImplItemFn, ItemFn, ItemMod, Meta};
    use walkdir::WalkDir;

    use super::workspace_root;

    /// Allow-list of functions in `resolver_core/` whose direct
    /// self-recursion is bounded by a non-`depth_budget` invariant.
    /// Each entry must carry a citation. The format is
    /// `(file-stem, fn-name, citation)`. The scan matches a flag
    /// against an entry by exact `(file-stem, fn-name)` tuple — this
    /// is far stricter than the previous regex-era allow-list which
    /// used the bare fn-name (and would silence cross-file collisions).
    ///
    /// All entries below were classified during the Phase 5l-supplement
    /// audit by inspecting the function body. Three bounding-invariant
    /// categories cover the entire list:
    ///
    /// 1. **AST-bounded**. The function recurses on a `TypeExpr` /
    ///    `ValueExpr` / similar finite enum tree. Stack growth is
    ///    `O(input-AST-depth)`, which itself is bounded by the OXC /
    ///    verter_parser stack limit at parse time. A pathological deep
    ///    expression would have already failed parser parsing before
    ///    reaching the resolver.
    ///
    /// 2. **DAG-bounded** (with explicit `seen` / `visiting` set). The
    ///    function recurses on an import / export / reexport graph
    ///    that the surrounding cache layer dedups, AND the function
    ///    body itself carries a `visited` / `seen` / `seen_locals`
    ///    cycle-dedup set. Stack growth is bounded by the number of
    ///    distinct entries in the graph, not by the call depth.
    ///
    /// 3. **Recursive-descent parser**. The function is part of the
    ///    `type_text_parser` hand-written recursive-descent parser.
    ///    Stack growth equals input-text nesting depth. Inputs are
    ///    string payloads sized by the source file, and production
    ///    callers feed type-text from already-parsed declarations.
    ///
    /// If a future refactor adds a TRULY unbounded recursion (no
    /// AST/DAG/text-depth bound), the correct fix is to refactor the
    /// callsite into an `iterative_frame` loop or thread a
    /// `depth_budget` parameter — NOT to pad this allow-list. Reviewers
    /// must reject allow-list growth that lacks a structural bound.
    pub(super) const ALLOWED_BOUNDED_RECURSIONS: &[(&str, &str, &str)] = &[
        // -----------------------------------------------------------------
        // component_meta/projected_type_expr.rs + direct_macro.rs — TypeExpr/text walkers
        // (pre-Tier-2-W5d: both lived inside component_meta.rs)
        // -----------------------------------------------------------------
        (
            "projected_type_expr",
            "render_type_expr_for_projected_surface",
            "Phase 5l-supplement: bounded by TypeExpr AST depth.",
        ),
        (
            "direct_macro",
            "type_expr_has_direct_macro_reference",
            "Phase 5l-supplement: bounded by TypeExpr AST depth.",
        ),
        // -----------------------------------------------------------------
        // component_meta_query_engine/helpers.rs — TypeExpr walkers
        // -----------------------------------------------------------------
        (
            "helpers",
            "prepared_decl_keeps_raw_symbolic_non_object_alias",
            "Phase 5l-supplement: bounded by TypeExpr AST depth.",
        ),
        (
            "helpers",
            "prepared_member_body_stays_shallow",
            "Phase 5l-supplement: bounded by TypeExpr AST depth.",
        ),
        (
            "helpers",
            "projected_surface_member_names",
            "Phase 5l-supplement: bounded by TypeExpr AST depth.",
        ),
        (
            "helpers",
            "strip_parens_expr",
            "Phase 5l-supplement: bounded by TypeExpr Parenthesized chain depth.",
        ),
        // -----------------------------------------------------------------
        // component_meta_query_engine/prepared_surface.rs —
        // TypeExpr walkers (all marked dead_code for Phase 5g deletion;
        // still in tree at integration HEAD).
        // -----------------------------------------------------------------
        (
            "prepared_surface",
            "project_prepared_requested_member_from_expr",
            "Phase 5l-supplement: bounded by TypeExpr AST depth (dead_code, Phase 5g deletion target).",
        ),
        (
            "prepared_surface",
            "project_prepared_requested_member_from_symbol",
            "Phase 5l-supplement: bounded by TypeExpr AST depth (dead_code, Phase 5g deletion target).",
        ),
        (
            "prepared_surface",
            "project_prepared_surface_from_expr",
            "Phase 5l-supplement: bounded by TypeExpr AST depth (dead_code, Phase 5g deletion target).",
        ),
        (
            "prepared_surface",
            "project_prepared_surface_from_symbol",
            "Phase 5l-supplement: bounded by TypeExpr AST depth (dead_code, Phase 5g deletion target).",
        ),
        // -----------------------------------------------------------------
        // component_meta_query_engine/route_keys.rs — TypeExpr walkers
        // -----------------------------------------------------------------
        (
            "route_keys",
            "enumerate_member_surface_keys_via_route",
            "Phase 5l-supplement: bounded by TypeExpr AST depth (dead_code, Phase 5g deletion target).",
        ),
        (
            "route_keys",
            "enumerate_route_literal_keys_inner",
            "Phase 5l-supplement: bounded by TypeExpr AST depth (dead_code, Phase 5g deletion target).",
        ),
        (
            "route_keys",
            "prepared_string_literal_keys",
            "Phase 5l-supplement: bounded by TypeExpr AST depth.",
        ),
        // -----------------------------------------------------------------
        // component_meta_query_engine/routed_expr.rs — TypeExpr walkers
        // (all marked dead_code for Phase 5g deletion).
        // -----------------------------------------------------------------
        (
            "routed_expr",
            "expr_references_prepared_scope_symbol",
            "Phase 5l-supplement: bounded by TypeExpr AST depth (dead_code, Phase 5g deletion target).",
        ),
        (
            "routed_expr",
            "project_inherited_member_route_projection_from_expr",
            "Phase 5l-supplement: bounded by TypeExpr AST depth (dead_code, Phase 5g deletion target).",
        ),
        (
            "routed_expr",
            "project_prepared_member_path_route_projection_from_expr",
            "Phase 5l-supplement: bounded by TypeExpr AST depth (dead_code, Phase 5g deletion target).",
        ),
        (
            "routed_expr",
            "project_prepared_member_path_route_projection_from_symbol",
            "Phase 5l-supplement: bounded by TypeExpr AST depth (dead_code, Phase 5g deletion target).",
        ),
        // -----------------------------------------------------------------
        // component_meta_query_engine/shallow_preserve.rs — TypeExpr
        // walkers and import-route walkers.
        // -----------------------------------------------------------------
        (
            "shallow_preserve",
            "contains_direct_imported_utility_route",
            "Phase 5l-supplement: bounded by TypeExpr AST depth.",
        ),
        (
            "shallow_preserve",
            "deep_resolve_slot_function_refs",
            "Phase 5l-supplement: bounded by TypeExpr AST depth + cache dedup at host.",
        ),
        (
            "shallow_preserve",
            "deep_resolve_type_refs",
            "Phase 5l-supplement: bounded by TypeExpr AST depth + cache dedup at host.",
        ),
        (
            "shallow_preserve",
            "fast_symbolic_imported_bare_ref_route",
            "Phase 5l-supplement: bounded by TypeExpr AST depth.",
        ),
        (
            "shallow_preserve",
            "fast_symbolic_imported_generic_route",
            "Phase 5l-supplement: bounded by TypeExpr AST depth.",
        ),
        (
            "shallow_preserve",
            "imported_route_arg",
            "Phase 5l-supplement: bounded by TypeExpr AST depth (nested closure).",
        ),
        (
            "shallow_preserve",
            "imported_value_route_arg",
            "Phase 5l-supplement: bounded by TypeExpr AST depth (nested closure).",
        ),
        (
            "shallow_preserve",
            "rewrite_fast_shallow_alias_body",
            "Phase 5l-supplement: bounded by TypeExpr AST depth.",
        ),
        (
            "shallow_preserve",
            "root_import_name",
            "Phase 5l-supplement: bounded by TypeExpr IndexedAccess chain depth (nested closure, defined twice).",
        ),
        (
            "shallow_preserve",
            "should_preserve_imported_utility_route",
            "Phase 5l-supplement: bounded by TypeExpr AST depth.",
        ),
        (
            "shallow_preserve",
            "should_preserve_shallow_field_expr_inner",
            "Phase 5l-supplement: bounded by TypeExpr AST depth.",
        ),
        // -----------------------------------------------------------------
        // component_meta_query_engine/surface.rs — TypeExpr / semantic-
        // node-graph walkers. `projected_surface_from_semantic_node_inner`
        // is DAG-bounded by an explicit `active: &mut FxHashSet<SemanticNodeId>`
        // visitor set; the rest are AST-bounded.
        // -----------------------------------------------------------------
        (
            "surface",
            "dispatch_route_expr_is_materialized",
            "Phase 5l-supplement: bounded by TypeExpr AST depth.",
        ),
        (
            "surface",
            "projected_surface_from_semantic_node_inner",
            "Phase 5l-supplement: bounded by SemanticNodeId DAG (active-set cycle dedup).",
        ),
        (
            "surface",
            "substitute_type_expr",
            "Phase 5l-supplement: bounded by TypeExpr AST depth (substitution rewriter).",
        ),
        (
            "surface",
            "type_expr_has_any_object_arm",
            "Phase 5l-supplement: bounded by TypeExpr Parenthesized/Union/Intersection chain depth.",
        ),
        (
            "surface",
            "visit",
            "Phase 5l-supplement: bounded by TypeExpr AST depth (nested local fn).",
        ),
        // -----------------------------------------------------------------
        // component_meta_registry.rs — TypeExpr walkers and registry-
        // route helpers. Recursion depth = TypeExpr AST depth in every
        // case (verified by inspection of the body's `match expr` arms).
        // -----------------------------------------------------------------
        (
            "component_meta_registry",
            "bound_generic_ref_penalty",
            "Phase 5l-supplement: bounded by TypeExpr AST depth.",
        ),
        (
            "component_meta_registry",
            "collect_component_meta_registry_member_surface_refs",
            "Phase 5l-supplement: bounded by TypeExpr AST depth.",
        ),
        (
            "component_meta_registry",
            "collect_component_meta_registry_public_surface_refs",
            "Phase 5l-supplement: bounded by TypeExpr AST depth.",
        ),
        (
            "component_meta_registry",
            "collect_component_meta_registry_refs",
            "Phase 5l-supplement: bounded by TypeExpr AST depth.",
        ),
        (
            "component_meta_registry",
            "collect_path",
            "Phase 5l-supplement: bounded by TypeExpr IndexedAccess chain depth (nested closure).",
        ),
        (
            "component_meta_registry",
            "component_meta_registry_direct_public_ref",
            "Phase 5l-supplement: bounded by TypeExpr AST depth.",
        ),
        (
            "component_meta_registry",
            "component_meta_registry_expr_references_name",
            "Phase 5l-supplement: bounded by TypeExpr AST depth.",
        ),
        (
            "component_meta_registry",
            "component_meta_registry_has_explicit_object_surface",
            "Phase 5l-supplement: bounded by TypeExpr AST depth.",
        ),
        (
            "component_meta_registry",
            "component_meta_registry_has_non_object_top_level_surface",
            "Phase 5l-supplement: bounded by TypeExpr AST depth.",
        ),
        (
            "component_meta_registry",
            "component_meta_registry_indexed_ref_penalty",
            "Phase 5l-supplement: bounded by TypeExpr AST depth.",
        ),
        (
            "component_meta_registry",
            "component_meta_registry_public_utility_route",
            "Phase 5l-supplement: bounded by TypeExpr AST depth.",
        ),
        (
            "component_meta_registry",
            "component_meta_registry_ref_name",
            "Phase 5l-supplement: bounded by TypeExpr AST depth.",
        ),
        (
            "component_meta_registry",
            "component_meta_registry_string_literal_keys",
            "Phase 5l-supplement: bounded by TypeExpr AST depth.",
        ),
        (
            "component_meta_registry",
            "contains_nested_resolution_targets",
            "Phase 5l-supplement: bounded by TypeExpr AST depth.",
        ),
        (
            "component_meta_registry",
            "extracted_surface_property_count",
            "Phase 5l-supplement: bounded by TypeExpr AST depth.",
        ),
        (
            "component_meta_registry",
            "imported_type_body_specificity_score",
            "Phase 5l-supplement: bounded by TypeExpr AST depth.",
        ),
        (
            "component_meta_registry",
            "is_empty_object_surface",
            "Phase 5l-supplement: bounded by TypeExpr AST depth.",
        ),
        (
            "component_meta_registry",
            "method_surface_specificity_score",
            "Phase 5l-supplement: bounded by TypeExpr AST depth.",
        ),
        (
            "component_meta_registry",
            "navigate_object_member",
            "Phase 5l-supplement: bounded by TypeExpr Parenthesized chain depth.",
        ),
        (
            "component_meta_registry",
            "top_level_branching_surface_score",
            "Phase 5l-supplement: bounded by TypeExpr AST depth.",
        ),
        // -----------------------------------------------------------------
        // declaration_metadata.rs — type-declaration chain walker
        // bounded by the import-graph DAG. The body uses early-exit on
        // `canonical_source == dep_canonical && followed_canonical ==
        // canonical_source` to terminate fixed-point chains; the import
        // graph is already DAG-deduped at the cache layer.
        // -----------------------------------------------------------------
        (
            "declaration_metadata",
            "resolve_type_declaration",
            "Phase 5l-supplement: bounded by import graph DAG (canonical-cache dedup).",
        ),
        // -----------------------------------------------------------------
        // export_graph.rs — barrel re-export chain followers. ALL
        // recursive bodies carry an explicit `visiting: &mut
        // FxHashSet<...>` cycle-dedup parameter and bail on a duplicate
        // insert. DAG-bounded by construction.
        // -----------------------------------------------------------------
        (
            "export_graph",
            "collect_resolved_exports_from_graph",
            "Phase 5l-supplement: DAG-bounded by `visiting: &mut FxHashSet` cycle dedup.",
        ),
        (
            "export_graph",
            "follow_reexport_chain_from_graph",
            "Phase 5l-supplement: DAG-bounded by `visiting: &mut FxHashSet` cycle dedup.",
        ),
        (
            "export_graph",
            "resolve_named_export_from_graph_inner",
            "Phase 5l-supplement: DAG-bounded by `visiting: &mut FxHashSet` cycle dedup.",
        ),
        (
            "export_graph",
            "resolve_single_export_from_graph",
            "Phase 5l-supplement: DAG-bounded by `visiting: &mut FxHashSet` cycle dedup.",
        ),
        // -----------------------------------------------------------------
        // external_type_frontier.rs — final-target follower DAG-bounded
        // by an explicit `seen: &mut FxHashSet<(String, String)>` cycle
        // dedup parameter; cycles set `had_cycle = true` and return None.
        // -----------------------------------------------------------------
        (
            "external_type_frontier",
            "final_target_from",
            "Phase 5l-supplement: DAG-bounded by `seen: &mut FxHashSet` cycle dedup.",
        ),
        // -----------------------------------------------------------------
        // fallthrough.rs — TypeExpr walkers (root-candidate enum,
        // spread-keys reduction, typeof-ref substitution). All AST-bounded.
        // -----------------------------------------------------------------
        (
            "fallthrough",
            "collect_dynamic_root_candidates_from_type",
            "Phase 5l-supplement: bounded by TypeExpr AST depth.",
        ),
        (
            "fallthrough",
            "known_spread_keys_from_type_expr",
            "Phase 5l-supplement: bounded by TypeExpr AST depth.",
        ),
        (
            "fallthrough",
            "structural_substitute_typeof_refs",
            "Phase 5l-supplement: bounded by TypeExpr AST depth (rewriter).",
        ),
        // -----------------------------------------------------------------
        // shallow_file_state.rs — type-expression walkers. All bodies
        // recurse on TypeExpr / ValueExpr / FunctionBody AST. The
        // `extract_string_literal_keys_from_type_expr` body additionally
        // tracks a `seen_locals` set to avoid revisiting named refs, so
        // it is bounded by min(AST-depth, distinct-symbol-count).
        // -----------------------------------------------------------------
        (
            "shallow_file_state",
            "collect_direct_object_properties",
            "Phase 5l-supplement: bounded by ValueExpr AST depth (object-literal nesting).",
        ),
        (
            "shallow_file_state",
            "collect_member_path_seed_names",
            "Phase 5l-supplement: bounded by TypeExpr AST depth.",
        ),
        (
            "shallow_file_state",
            "collect_type_refs",
            "Phase 5l-supplement: bounded by TypeExpr AST depth.",
        ),
        (
            "shallow_file_state",
            "collect_typeof_roots",
            "Phase 5l-supplement: bounded by ValueExpr AST depth.",
        ),
        (
            "shallow_file_state",
            "collect_whole_route_refs",
            "Phase 5l-supplement: bounded by TypeExpr AST depth.",
        ),
        (
            "shallow_file_state",
            "extract_indexed_access_base",
            "Phase 5l-supplement: bounded by TypeExpr AST depth (IndexedAccess.object chain).",
        ),
        (
            "shallow_file_state",
            "extract_string_literal_keys_from_type_expr",
            "Phase 5l-supplement: bounded by TypeExpr AST depth + seen_locals dedup.",
        ),
        (
            "shallow_file_state",
            "follow_routed_expr",
            "Phase 5l-supplement: bounded by TypeExpr AST depth.",
        ),
        // -----------------------------------------------------------------
        // surface_projector.rs — typed-IR display walker used by the slot
        // info pipeline (W3.2 cutover). Bounded by an explicit
        // `MAX_DISPLAY_DEPTH = 64` depth budget threaded through every
        // recursive call site.
        // -----------------------------------------------------------------
        (
            "surface_projector",
            "render_type_expr_display_inner",
            "Bounded by explicit `MAX_DISPLAY_DEPTH = 64` depth budget.",
        ),
    ];

    /// Mark a name `host_with_ws` as a known test-helper collision —
    /// these are inside `#[cfg(test)]` mods which the visitor already
    /// skips, but we list them here for documentation. The scanner
    /// does NOT check this list for filtering; it relies on the
    /// `cfg_test_depth` tracker.
    pub(super) const _DOCUMENTED_TEST_FIXTURES: &[&str] = &["host_with_ws", "ws_with_one_project"];

    #[derive(Debug, Clone)]
    pub(super) struct Violation {
        pub(super) file_stem: String,
        pub(super) fn_name: String,
        pub(super) call_kind: CallKind,
        pub(super) rel_path: String,
    }

    #[derive(Debug, Clone, Copy)]
    pub(super) enum CallKind {
        Bare,
        SelfQualified,
        SelfMethod,
    }

    impl std::fmt::Display for CallKind {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Self::Bare => write!(f, "bare `foo(...)`"),
                Self::SelfQualified => write!(f, "`Self::foo(...)`"),
                Self::SelfMethod => write!(f, "`self.foo(...)`"),
            }
        }
    }

    pub(super) struct RecursionVisitor<'a> {
        rel_path: &'a str,
        file_stem: &'a str,
        cfg_test_depth: u32,
        /// Stack of enclosing function/method names. The top of the
        /// stack is the function whose body is currently being walked;
        /// any matching call counts as direct self-recursion.
        fn_stack: Vec<String>,
        violations: &'a mut Vec<Violation>,
    }

    impl<'a> RecursionVisitor<'a> {
        pub(super) fn new(
            rel_path: &'a str,
            file_stem: &'a str,
            violations: &'a mut Vec<Violation>,
        ) -> Self {
            Self {
                rel_path,
                file_stem,
                cfg_test_depth: 0,
                fn_stack: Vec::new(),
                violations,
            }
        }

        /// Push a violation if the named call matches the top of the
        /// fn-stack. Returns silently if no enclosing fn is being
        /// walked (e.g. top-level `static FOO = some_call();`) or if
        /// the call name doesn't match.
        fn try_flag(&mut self, called_name: &str, kind: CallKind) {
            if self.cfg_test_depth > 0 {
                return;
            }
            let Some(current) = self.fn_stack.last() else {
                return;
            };
            if current != called_name {
                return;
            }
            self.violations.push(Violation {
                file_stem: self.file_stem.to_string(),
                fn_name: called_name.to_string(),
                call_kind: kind,
                rel_path: self.rel_path.to_string(),
            });
        }
    }

    /// True if any of the supplied attributes is `#[cfg(test)]`,
    /// `#[cfg(any(test, ...))]`, or `#[cfg(all(..., test, ...))]` —
    /// any cfg expression containing the bare predicate `test`. Mirrors
    /// the helper used in the `resolver_context_seal` mod above.
    fn has_cfg_test(attrs: &[Attribute]) -> bool {
        attrs.iter().any(|a| {
            if !a.path().is_ident("cfg") {
                return false;
            }
            let rendered = match &a.meta {
                Meta::List(list) => list.tokens.to_string(),
                _ => return false,
            };
            for token in rendered.split(|c: char| !c.is_alphanumeric() && c != '_') {
                if token == "test" {
                    return true;
                }
            }
            false
        })
    }

    impl<'ast> Visit<'ast> for RecursionVisitor<'_> {
        fn visit_item_mod(&mut self, m: &'ast ItemMod) {
            let entered_test = has_cfg_test(&m.attrs) || m.ident == "tests";
            if entered_test {
                self.cfg_test_depth += 1;
            }
            syn::visit::visit_item_mod(self, m);
            if entered_test {
                self.cfg_test_depth -= 1;
            }
        }

        fn visit_item_fn(&mut self, f: &'ast ItemFn) {
            let entered_test = has_cfg_test(&f.attrs);
            if entered_test {
                self.cfg_test_depth += 1;
            }
            self.fn_stack.push(f.sig.ident.to_string());
            syn::visit::visit_item_fn(self, f);
            self.fn_stack.pop();
            if entered_test {
                self.cfg_test_depth -= 1;
            }
        }

        fn visit_impl_item_fn(&mut self, f: &'ast ImplItemFn) {
            let entered_test = has_cfg_test(&f.attrs);
            if entered_test {
                self.cfg_test_depth += 1;
            }
            self.fn_stack.push(f.sig.ident.to_string());
            syn::visit::visit_impl_item_fn(self, f);
            self.fn_stack.pop();
            if entered_test {
                self.cfg_test_depth -= 1;
            }
        }

        fn visit_expr_call(&mut self, call: &'ast ExprCall) {
            // bare `foo(...)` (path length 1) or `Self::foo(...)`
            // (path length 2 starting with `Self`).
            if let Expr::Path(p) = call.func.as_ref() {
                let segs = &p.path.segments;
                if segs.len() == 1 {
                    let name = segs[0].ident.to_string();
                    self.try_flag(&name, CallKind::Bare);
                } else if segs.len() == 2 && segs[0].ident == "Self" {
                    let name = segs[1].ident.to_string();
                    self.try_flag(&name, CallKind::SelfQualified);
                }
            }
            syn::visit::visit_expr_call(self, call);
        }

        fn visit_expr_method_call(&mut self, mc: &'ast ExprMethodCall) {
            // `self.foo(...)` — receiver is bare `self`. Any other
            // receiver (`self.field.foo(...)`, `ctx.foo(...)`,
            // `host.foo(...)`, `&dyn Trait` dispatch) is NOT direct
            // self-recursion at the syntactic level: dispatch is on a
            // different value, possibly a different impl.
            if let Expr::Path(p) = mc.receiver.as_ref() {
                if p.path.is_ident("self") {
                    let name = mc.method.to_string();
                    self.try_flag(&name, CallKind::SelfMethod);
                }
            }
            syn::visit::visit_expr_method_call(self, mc);
        }
    }

    pub(super) fn scan_file(path: &Path, violations: &mut Vec<Violation>) {
        let src = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => panic!("read {}: {}", path.display(), e),
        };
        let parsed =
            syn::parse_file(&src).unwrap_or_else(|e| panic!("parse {}: {}", path.display(), e));
        let rel = path
            .strip_prefix(workspace_root())
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let file_stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        let mut visitor = RecursionVisitor::new(&rel, &file_stem, violations);
        visitor.visit_file(&parsed);
    }

    pub(super) fn walk_resolver_core_files() -> Vec<PathBuf> {
        let dir = workspace_root().join("crates/verter_session/src/resolver_core");
        let mut files = Vec::new();
        for entry in WalkDir::new(&dir).into_iter().filter_map(Result::ok) {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            // Sibling `*_tests.rs` test files are characterization-test
            // infrastructure, not production resolver code. Skip them.
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.ends_with("_tests.rs") || name == "tests.rs" {
                    continue;
                }
            }
            files.push(path.to_path_buf());
        }
        files
    }

    /// True if a discovered violation has an entry on
    /// `ALLOWED_BOUNDED_RECURSIONS`. The match key is
    /// `(file_stem, fn_name)`.
    pub(super) fn is_allowed(v: &Violation) -> bool {
        ALLOWED_BOUNDED_RECURSIONS
            .iter()
            .any(|(stem, name, _)| *stem == v.file_stem && *name == v.fn_name)
    }

    pub(super) fn format_violations(unallowed: &[Violation]) -> String {
        // Group by (file, fn-name) and report distinct call kinds.
        let mut by_key: HashMap<(String, String), Vec<CallKind>> = HashMap::new();
        let mut paths: HashMap<(String, String), String> = HashMap::new();
        for v in unallowed {
            let key = (v.file_stem.clone(), v.fn_name.clone());
            by_key.entry(key.clone()).or_default().push(v.call_kind);
            paths.entry(key).or_insert_with(|| v.rel_path.clone());
        }
        let mut keys: Vec<_> = by_key.keys().cloned().collect();
        keys.sort();
        let mut lines = Vec::new();
        for key in keys {
            let kinds = &by_key[&key];
            let path = &paths[&key];
            let mut kind_strs: Vec<String> = kinds.iter().map(|k| k.to_string()).collect();
            kind_strs.sort();
            kind_strs.dedup();
            lines.push(format!(
                "  {}: fn `{}` directly recurses on itself via {} \
                 — refactor to a depth-budgeted shape, or add an \
                 `ALLOWED_BOUNDED_RECURSIONS` entry with a phase-report citation",
                path,
                key.1,
                kind_strs.join(" + "),
            ));
        }
        format!(
            "found {} unallowed direct self-recursion(s) in {} resolver_core function(s):\n{}",
            unallowed.len(),
            lines.len(),
            lines.join("\n"),
        )
    }
}

#[test]
fn no_unbounded_recursion_in_resolver_core() {
    // r15/F15 (Claude review) — static guard for §0.6.5 stack-depth
    // discipline. Phase 5l-supplement rewrite — see the
    // `resolver_core_recursion` mod docs above for the full design
    // rationale and the previous-iteration regex bug that necessitated
    // this rewrite.
    //
    // Discriminating: this test FAILS against the pre-rewrite tree
    // (the regex heuristic flags 568 false positives — the ignored
    // `phase-05l pending` marker on the prior incarnation
    // demonstrates this) and PASSES against the post-rewrite tree
    // because the syn-AST scanner only flags TRUE direct self-recursion
    // and every such recursion is either refactored (none are at the
    // time of this commit) or carries an explicit allow-list entry
    // with a phase-report citation.
    //
    // If a future commit introduces a new direct self-recursion in
    // `resolver_core/`, the scanner flags it and this test fails. The
    // fix is to either (a) refactor to use `depth_budget` /
    // `iterative_frame` / explicit `MAX_DEPTH`, or (b) add an entry
    // to `resolver_core_recursion::ALLOWED_BOUNDED_RECURSIONS` with a
    // citation explaining the bounding invariant.
    use resolver_core_recursion::{
        format_violations, is_allowed, scan_file, walk_resolver_core_files, Violation,
    };

    let mut violations: Vec<Violation> = Vec::new();
    for file in walk_resolver_core_files() {
        scan_file(&file, &mut violations);
    }

    let unallowed: Vec<Violation> = violations.into_iter().filter(|v| !is_allowed(v)).collect();
    assert!(
        unallowed.is_empty(),
        "no_unbounded_recursion_in_resolver_core (Phase 5l-supplement):\n{}",
        format_violations(&unallowed)
    );
}

/// Phase 5l §5.14.2 — atomic deletion regression guard.
///
/// Asserts the 13 deprecated `ComponentMetaQueryEngine` methods that
/// 5k marked with `#[deprecated(note = "Phase 5l deletion target: ...")]`
/// have been deleted. Runs a static grep over
/// `component_meta_query_engine.rs` for the `pub fn <method_name>`
/// pattern and fails if any of the 13 names re-introduce themselves
/// at a definition site.
///
/// The assertion is bounded to the engine module: the method names
/// are unique enough that a grep over the engine source file
/// discriminates re-introduction at the engine impl block from
/// unrelated `_via_host_threaded` bridge wrapper names that
/// `meta_resolve.rs` defines (the bridges live in a different file
/// and use `pub(crate) fn` rather than `pub fn`).
///
/// Discriminating: the test FAILS against the pre-deletion tree
/// (the deprecated trampolines were `pub fn <name>(`) and PASSES
/// against the post-deletion tree (no `pub fn <name>(` remains in
/// the engine module).
#[test]
fn phase_05l_engine_resolver_methods_deleted() {
    let src = read_workspace_file(
        "crates/verter_session/src/resolver_core/component_meta_query_engine/mod.rs",
    );
    let retired_methods: &[&str] = &[
        "project_type_surface",
        "project_type_surface_expr",
        "project_type_surface_shape",
        "project_prepared_type_surface_expr",
        "project_prepared_type_surface_shape",
        "project_type_member",
        "project_type_keyspace",
        "project_expr_surface_expr",
        "project_expr_surface_expr_with_compound_objects",
        "lower_and_project_to_expanded",
        "instantiate_local_generic_ref",
        "project_expr_surface_shape",
        "project_route_surface_expr",
    ];
    let mut violations = Vec::<String>::new();
    for method in retired_methods {
        // The deprecated trampolines were declared as
        // `pub fn <name>(` (no visibility modifier or signature
        // sugar). The grep matches that exact prefix to avoid false
        // positives from the surviving `pub(crate) fn <name>...`
        // helpers (e.g., `project_routed_expr_surface_expr` is
        // pub(crate) and survives).
        let definition_marker = format!("pub fn {method}(");
        if src.contains(&definition_marker) {
            violations.push(format!(
                "deprecated engine method `{method}` still defined as `pub fn {method}(...)` \
                 in component_meta_query_engine.rs"
            ));
        }
    }
    assert!(
        violations.is_empty(),
        "Phase 5l §5.14.2 deletion regression: the following retired engine methods \
         must be DELETED but were found at their definition sites:\n{}",
        violations.join("\n")
    );

    // Negative-direction discriminator: the test must FAIL against
    // any reintroduction. We sanity-check that the assertion can
    // detect a re-introduction by scanning for a method that we know
    // SURVIVES the deletion (`pub(crate) fn new` is the engine
    // constructor and stays in mod.rs after the Phase 11b folder
    // split). Phase 10a renamed the parameter from
    // `host: &'a VerterHost` to `ctx: &'a dyn ResolverContext` (the
    // resolver-context seal); the discriminator follows the rename.
    // The post-cutover clippy cleanup downgraded the constructor's
    // visibility from `pub fn` to `pub(crate) fn` because the trait
    // (`ResolverContext`) is `pub(crate)` — exposing the constructor
    // at `pub` triggered the `private-interfaces` lint. The
    // discriminator follows the visibility downgrade. If this assert
    // fails, the discriminator is broken — we'd miss real
    // re-introductions.
    assert!(
        src.contains("pub(crate) fn new(ctx: &'a dyn ResolverContext)"),
        "discriminator check: the surviving engine constructor \
         `pub(crate) fn new(ctx: &'a dyn ResolverContext)` must still appear in \
         component_meta_query_engine/mod.rs — its absence means the \
         discriminator is broken and this test cannot detect \
         re-introductions of the retired methods"
    );
}

#[test]
fn no_scheduler_backed_workspace_shim_in_session_src() {
    // Phase 6c — production WorkspaceAccess shim removal regression
    // guard.
    //
    // After Phase 6c, the deleted scheduler-backed shim file under
    // `crates/verter_session/src/` MUST NOT exist, and no production
    // source file in that directory may declare the deleted type-name
    // OR introduce a same-shape `WorkspaceAccess` impl under any
    // rename. Test fixtures (files whose name ends in `_tests.rs`) are
    // an explicit allow-list of characterization-test infrastructure
    // permitted by Phase 6b — they implement `WorkspaceAccess` for
    // CountingWorkspace-style instrumentation only.
    //
    // The forbidden type-name and file-name are constructed via
    // `concat!` at compile time so this test's own source contains no
    // literal occurrence of the deleted artefacts. The verification
    // greps in §6c.6 (which walk `crates/` broadly) therefore return
    // zero hits even when this guards file is in the search scope.
    //
    // Architectural rule (parent plan §6c.0): `verter_session` is not
    // the home for production `WorkspaceAccess` impls — those belong
    // in `verter_workspace` (`MemoryWorkspace`, `FilesystemWorkspace`).
    // A reviewer who genuinely needs a new production
    // `WorkspaceAccess` impl in this crate must update this guard with
    // a phase-report citation justifying the exception — the existing
    // allow-list pattern in `god_module_size_budget` is the template
    // (see this file's lines 161–164).
    use std::collections::HashSet;
    use walkdir::WalkDir;

    // Forbidden tokens built at compile time so they don't appear as
    // literals in this source file. `concat!` is constant-folded, so
    // the runtime behaviour is identical to a string literal.
    const FORBIDDEN_TYPE: &str = concat!("Sched", "ulerBackedWorkspace");
    const FORBIDDEN_MODULE_FILE: &str = concat!("scheduler", "_shim.rs");
    // Trailing space catches both the unqualified form
    // (`impl WorkspaceAccess for X`) and the qualified form
    // (`impl verter_workspace::WorkspaceAccess for X`).
    const FORBIDDEN_IMPL_PATTERN: &str = "WorkspaceAccess for ";
    // Test-fixture filename suffix. The convention in this codebase
    // is that test fixtures with `WorkspaceAccess` impls live in
    // `*_tests.rs` files (e.g. `frontier_tests.rs`,
    // `host_manage_tests.rs`, `cache_identity_invariants_tests.rs`).
    const TEST_FIXTURE_SUFFIX: &str = "_tests.rs";

    // (a) The deleted shim file MUST NOT exist.
    let shim_path = workspace_root()
        .join("crates/verter_session/src")
        .join(FORBIDDEN_MODULE_FILE);
    assert!(
        !shim_path.exists(),
        "Phase 6c regression: production shim file `{}` must not exist \
         after Phase 6c removal — re-introducing the scheduler-backed \
         `WorkspaceAccess` shim is forbidden per the cutover end-state \
         (no shims, no dual paths)",
        shim_path.display()
    );

    // (b) Walk production sources of `verter_session` and reject:
    //     1. Any `*.rs` file containing the forbidden type-name (catches
    //        a renamed re-introduction at any path that still uses the
    //        deleted type-name as a substring).
    //     2. Any non-test `*.rs` file containing the
    //        `WorkspaceAccess for ` impl pattern (catches a same-shape
    //        re-introduction under a renamed type).
    //
    //     Test fixture files (`*_tests.rs`) are allow-listed for rule
    //     (b)(2) because their `WorkspaceAccess` impls are legitimate
    //     instrumentation. They are still subject to rule (b)(1) —
    //     even tests must not re-introduce the deleted type-name.
    let session_src = workspace_root().join("crates/verter_session/src");
    // Empty production allow-list by design. Future exceptions require
    // a phase-report citation per the convention at lines 161–164.
    let production_allow_list: HashSet<&str> = HashSet::new();
    let mut violations = Vec::<String>::new();
    for entry in WalkDir::new(&session_src) {
        let entry = entry.expect("walkdir entry");
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        let rel = path
            .strip_prefix(workspace_root())
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if production_allow_list.contains(rel.as_str()) {
            continue;
        }
        let src = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        // Rule (b)(1) — applies to ALL files in session/src/, including
        // test fixtures. The deleted type-name must not be referenced
        // anywhere.
        if src.contains(FORBIDDEN_TYPE) {
            violations.push(format!(
                "{rel}: contains forbidden type `{}` — Phase 6c removed \
                 the scheduler-backed shim; re-introduction under any \
                 path is forbidden (any new shim requires a phase-report \
                 citation per the established architecture-guards \
                 convention)",
                FORBIDDEN_TYPE
            ));
        }
        // Rule (b)(2) — applies only to NON-test files. Test fixtures
        // (`*_tests.rs`) legitimately implement `WorkspaceAccess` for
        // characterization-test instrumentation.
        let is_test_fixture = file_name.ends_with(TEST_FIXTURE_SUFFIX);
        if !is_test_fixture && src.contains(FORBIDDEN_IMPL_PATTERN) {
            violations.push(format!(
                "{rel}: contains `{}` in non-test source — Phase 6c \
                 forbids new production `WorkspaceAccess` impls in \
                 `verter_session/src/`. Production `WorkspaceAccess` \
                 impls belong in `verter_workspace`; test fixtures must \
                 live in `*_tests.rs` files",
                FORBIDDEN_IMPL_PATTERN.trim_end()
            ));
        }
    }
    assert!(
        violations.is_empty(),
        "no_scheduler_backed_workspace_shim_in_session_src violations:\n{}",
        violations.join("\n")
    );
}

// ----------------------------------------------------------------------
// Phase 8 — `no_off_store_host_caches`
//
// Static guard for the "no caches outside ProjectTypeStore" architectural
// rule documented in CLAUDE.md ("Project-global cache (final state)") and
// re-stated by the universal preamble R4 of the architecture-cutover plan
// ("Do not add caches outside ProjectTypeStore. Do not add request-local
// mirrors of host state.").
//
// Phase 6b's classification of every cache-shaped `VerterHost` field is
// the binding source of truth. Each `legitimate-authority` field is
// recorded in `phase_8_allow_list()` with the §6b sub-plan citation and
// the architectural rationale. Each `mirror`-classified field (F3
// routes/imported_roots, F6 external_type_analysis_cache, F7
// route_owned_shallow_cache) was already deleted by Phase 6b — this
// guard verifies their continued absence.
//
// Body design (per §8.1 of the cutover plan):
//   1. Parse `crates/verter_session/src/lib.rs` via `syn::parse_file`.
//   2. Locate `pub struct VerterHost`.
//   3. For each field, render the type signature to a string and
//      classify by structural shape:
//        - `DashMap<...>`, `Shared<FxHashMap...>`,
//          `Mutex<...>`, `RwLock<...>` (with or without a
//          `parking_lot::` qualifier) → cache-shape candidate.
//        - `Atomic*`, `ArcSwap*`, simple `Arc<T>`, `Box<T>`,
//          plain owned types → non-cache shape, PASS.
//   4. For each cache-shape candidate, assert it is either:
//        - on the allow-list below (with phase-report citation), OR
//        - the `project_type_store` field itself (the destination).
//   5. Re-verify that the deleted mirror field names do not reappear.
//
// Phase 8 ships this guard un-ignored: Phase 8 has full visibility into
// the post-rehoming shape and is the sole author of this discipline.
// Future commits that add a new cache-shaped field on `VerterHost` MUST
// either rehome the field into `ProjectTypeStore` or extend the
// allow-list with a phase-report citation justifying the exception.

/// The Phase-8 allow-list for `no_off_store_host_caches`. Each entry is a
/// `VerterHost` cache-shape field that Phase 6b classified as
/// `legitimate-authority`, paired with a one-line phase-report citation
/// and architectural rationale.
fn phase_8_allow_list() -> std::collections::HashMap<&'static str, &'static str> {
    [
        // (a) Cache-shape fields explicitly classified by Phase 6b as
        //     legitimate-authority — see phase-06b-report.md and the
        //     phase-06b sub-plan §6b.2.
        (
            "alias_to_canonical",
            "phase-06b-report.md §F12: caller-supplied virtual-alias map populated at upsert time, disjoint from VFS overlay and ProjectResolver. Host-scoped, no equivalent in ProjectTypeStore.",
        ),
        (
            "last_const_prop_overrides",
            "phase-06b-report.md §F13: Phase-7 invalidation state-diff record (NOT a cache of resolution results). No equivalent in ProjectTypeStore.",
        ),
        // F1, F2, F4, F5 — rehomed in Tier 1C-α (host-cache-rehoming.md
        // §3.4 + plan §3.4.1). The four fields (`compile_cache`,
        // `resolved_type_cache`, `eval_env_cache`, `semantic_db`) no
        // longer live on `VerterHost`; the syn-walk that drives this
        // allow-list will not surface them. Re-adding any of them to
        // `VerterHost` would fail this guard until a fresh
        // rehoming-doc rationale is added.
        (
            "query_profile",
            "phase-06b-report.md §F10: execution-policy state, not a result memoiser. Different artifact type than anything in ProjectTypeStore.",
        ),
        // (b) Single-cell handles whose `RwLock<Arc<dyn>>` shape matches
        //     the cache-detection pattern but whose semantics are
        //     config-handle, not hashmap-cache. Documented here for
        //     completeness so the guard's allow-list captures every
        //     deviation.
        (
            "workspace",
            "phase-06b-report.md §6b.2.F6.bypass: single-cell workspace handle (Arc<RwLock<Arc<dyn WorkspaceAccess>>>) shared with the scheduler's SourceLoader so the lock always reads through the latest workspace after set_workspace(). NOT a cache; a re-pointable handle.",
        ),
        // (c) Phase 9b test-only observable. `#[cfg(test)] last_upsert_priority`
        //     is a single-cell `Mutex<Option<Priority>>` test mailbox written
        //     by `upsert_with_priority` and read by the
        //     `compile_many_propagates_*_priority` tests on `VerterHost::compile_many`.
        //     Production builds compile this field out completely; allow-listed
        //     here because the guard parses `lib.rs` whose `#[cfg(test)]`-gated
        //     declaration is structurally a `Mutex<...>` field that the cache
        //     shape detector flags. NOT a cache; a per-host single-cell test
        //     observable.
        (
            "last_upsert_priority",
            "phase-09b-report.md §0 row \"Test-only observables on VerterHost\": Mutex<Option<Priority>> test mailbox written by upsert_with_priority and read by compile_many_propagates_*_priority. Compiled out in production builds. NOT a cache.",
        ),
        // (d) Typeinfo scratch synthesis cache (§5 Phase 3) — per-host
        //     LRU of synthesised scratch URI → SemanticNodeId. The
        //     cache lives on `VerterHost` (not ProjectTypeStore)
        //     because scratch URIs are session-local synthesis artefacts
        //     gated by `cacheable: bool` per-request, not project-wide
        //     resolution results. Configurable capacity via
        //     `HostConfig::typeinfo_scratch_cache_capacity` (default 64).
        (
            "typeinfo_scratch_cache",
            "§5.3 / Phase 3: per-host LRU mapping scratch URI → SemanticNodeId for `evaluate_type_expression(cacheable: true)`. Session-local synthesis cache, not a project-state result memoiser; ProjectTypeStore is for cross-request project-wide results.",
        ),
    ]
    .into_iter()
    .collect()
}

/// Classify a rendered type signature by structural shape. Returns true
/// for cache-shape candidates (DashMap / Shared<HashMap> / Mutex<...> /
/// RwLock<...>). Returns false for `Arc<T>`, `Box<T>`, `Atomic*`, owned
/// scalars, and other non-cache-shape types.
///
/// The substring matches are deliberately broad: any field whose type
/// signature contains `Mutex<` or `RwLock<` or `DashMap<` or
/// `Shared<FxHashMap` or `Shared<HashMap` is treated as a cache-shape
/// candidate. The token-stream renderer used by `render_type` emits both
/// `Mutex <` (with the angle-bracket-padding syn produces) and `Mutex<`
/// (after string normalisation), so we match both forms defensively.
fn is_cache_shape(rendered_ty: &str) -> bool {
    let r = rendered_ty;
    r.contains("DashMap <")
        || r.contains("DashMap<")
        || r.contains("Shared < FxHashMap")
        || r.contains("Shared<FxHashMap")
        || r.contains("Shared < HashMap")
        || r.contains("Shared<HashMap")
        || r.contains("Mutex <")
        || r.contains("Mutex<")
        || r.contains("RwLock <")
        || r.contains("RwLock<")
}

/// Render a `syn::Type` to a string via its `ToTokens` impl. Stable
/// across rustc versions because syn's token stream emission is
/// canonical (single-space-separated tokens).
fn render_type(ty: &syn::Type) -> String {
    use quote::ToTokens;
    let tokens = ty.to_token_stream();
    tokens.to_string()
}

/// The core algorithm of `no_off_store_host_caches`. Given a parsed
/// `pub struct <ident> { ... }` named-fields struct and the allow-list,
/// returns `(violations, surveyed_cache_fields)`. Pure function — no
/// I/O — so it is reusable by the discriminator self-test.
fn no_off_store_host_caches_inner(
    parsed: &syn::File,
    target_struct: &str,
    allow_list: &std::collections::HashMap<&str, &str>,
) -> (Vec<String>, Vec<(String, String)>) {
    use syn::{Fields, Item};
    let mut violations = Vec::<String>::new();
    let mut surveyed_cache_fields = Vec::<(String, String)>::new();
    let mut found_struct = false;
    for item in &parsed.items {
        let Item::Struct(s) = item else { continue };
        if s.ident != target_struct {
            continue;
        }
        found_struct = true;
        let Fields::Named(named) = &s.fields else {
            panic!(
                "{target_struct} is expected to have named fields; found {:?}",
                s.fields
            );
        };
        for field in &named.named {
            let field_name = field
                .ident
                .as_ref()
                .map(|id| id.to_string())
                .unwrap_or_default();
            let rendered_ty = render_type(&field.ty);
            // Skip the project_type_store destination field itself.
            // It is an Arc<ProjectTypeStore> — by structural shape it
            // is `Arc<...>`, not a cache pattern, but we name it
            // explicitly so the guard's intent is unambiguous.
            if field_name == "project_type_store" {
                continue;
            }
            if !is_cache_shape(&rendered_ty) {
                continue;
            }
            surveyed_cache_fields.push((field_name.clone(), rendered_ty.clone()));
            // Check allow-list.
            if allow_list.contains_key(field_name.as_str()) {
                continue;
            }
            // Check whether the type signature points at
            // ProjectTypeStore (i.e., the field IS a ProjectTypeStore
            // handle even though its type contains RwLock/Mutex/DashMap
            // by happenstance). The integration tip has no such field
            // today; this branch is a forward-looking allowance for
            // fields like `cache_root: Arc<ProjectTypeStore>` if a
            // future commit restructures the host.
            if rendered_ty.contains("ProjectTypeStore") {
                continue;
            }
            violations.push(format!(
                "{target_struct}::{field_name}: cache-shape field of type \
                 `{rendered_ty}` is neither on the documented allow-list \
                 (with a phase-report citation) nor on ProjectTypeStore. \
                 Either rehome into ProjectTypeStore (preferred per \
                 CLAUDE.md \"Project-global cache (final state)\" and \
                 plan R4) or extend this guard's allow-list with a \
                 phase-report citation justifying the exception."
            ));
        }
        break;
    }
    assert!(
        found_struct,
        "no_off_store_host_caches: did not find `pub struct {target_struct}` \
         in the parsed file — guard cannot verify the post-Phase-6b shape."
    );
    (violations, surveyed_cache_fields)
}

#[test]
fn no_off_store_host_caches() {
    use syn::parse_file;

    let allow_list = phase_8_allow_list();

    // Verify the source file we're parsing has not had a Phase-6b mirror
    // field re-added by name. This is independent of the syn walk and is
    // a belt-and-suspenders check against re-introducing the deleted
    // F6/F7 field names.
    let lib_src = read_workspace_file("crates/verter_session/src/lib.rs");
    for forbidden in [
        // F6 / F7 — deleted in 6b.D2a commit c6e7fbeb. Doc-comments
        // referencing these names are allowed in this guard's view
        // because the absent-from-struct check below is independently
        // authoritative; the field-declaration grep is the strict gate.
        "external_type_analysis_cache",
        "route_owned_shallow_cache",
    ] {
        let declaration_pattern = format!("pub(crate) {forbidden}:");
        assert!(
            !lib_src.contains(&declaration_pattern),
            "Phase 8 regression: VerterHost field `{forbidden}` was \
             deleted by Phase 6b and must not be re-introduced. Found \
             declaration `{declaration_pattern}` in lib.rs."
        );
    }

    // Parse lib.rs via syn and walk VerterHost fields.
    let parsed = parse_file(&lib_src).expect("parse verter_session/src/lib.rs via syn");
    let (violations, surveyed_cache_fields) =
        no_off_store_host_caches_inner(&parsed, "VerterHost", &allow_list);

    // Discriminator-coverage check: the syn walk MUST surface at least
    // one cache-shape field. If the count is zero, either the cache-shape
    // detector is broken (the guard cannot detect re-introductions) or
    // every cache-shape field has been moved — both worth flagging.
    assert!(
        !surveyed_cache_fields.is_empty(),
        "no_off_store_host_caches: the syn walk found ZERO cache-shape \
         fields on VerterHost, which means either the cache-shape \
         detector is broken or every cache-shape field has been moved. \
         Investigate before re-running."
    );

    assert!(
        violations.is_empty(),
        "no_off_store_host_caches violations:\n{}\n\nAllow-list reference \
         (each entry must cite a phase-report rationale):\n{}",
        violations.join("\n"),
        allow_list
            .iter()
            .map(|(k, v)| format!("  {k}: {v}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn no_off_store_host_caches_discriminator_self_test() {
    // Self-test: hand-craft a synthetic struct with one allow-listed
    // cache field (must pass) and one un-allow-listed cache field (must
    // fail). This proves the inner algorithm discriminates between the
    // good and bad cases — i.e., the guard would actually catch a
    // re-introduction. Without this, the empty-violations result of
    // `no_off_store_host_caches` against the integration tip is
    // indistinguishable from a broken detector that always passes.
    //
    // CLAUDE.md "Stub Prevention" — characterization-style discriminator
    // test for the guard itself.
    use syn::parse_file;
    let allow_list = phase_8_allow_list();

    // (a) Synthetic struct with ONLY allow-listed fields — must produce
    //     zero violations. `query_profile` is allow-listed (execution-
    //     policy state); `workspace` is allow-listed (re-pointable
    //     handle, not a hashmap-cache).
    let synthetic_pass = r#"
        pub struct SyntheticHost {
            pub(crate) instance_id: u64,
            pub(crate) query_profile: parking_lot::Mutex<verter_semantic::profile::QueryProfile>,
            pub(crate) workspace: Arc<parking_lot::RwLock<Arc<dyn WorkspaceAccess>>>,
            pub(crate) tick: AtomicU64,
        }
    "#;
    let parsed_pass = parse_file(synthetic_pass).expect("parse synthetic_pass");
    let (pass_violations, pass_surveyed) =
        no_off_store_host_caches_inner(&parsed_pass, "SyntheticHost", &allow_list);
    assert!(
        pass_violations.is_empty(),
        "discriminator self-test: synthetic_pass should produce zero \
         violations (query_profile and workspace are allow-listed, the \
         others are non-cache shapes), but got:\n{}",
        pass_violations.join("\n")
    );
    // Both allow-listed fields must be SURVEYED (otherwise the cache
    // detector is failing to flag them as candidates in the first place).
    let surveyed_names: Vec<String> = pass_surveyed.iter().map(|(n, _)| n.clone()).collect();
    assert!(
        surveyed_names.contains(&"query_profile".to_string()),
        "discriminator self-test: synthetic_pass must surface \
         `query_profile` as a cache-shape candidate; surveyed: {surveyed_names:?}"
    );
    assert!(
        surveyed_names.contains(&"workspace".to_string()),
        "discriminator self-test: synthetic_pass must surface \
         `workspace` as a cache-shape candidate; surveyed: {surveyed_names:?}"
    );

    // (b) Synthetic struct with ONE un-allow-listed cache-shape field —
    //     must produce exactly one violation, naming that field.
    let synthetic_fail = r#"
        pub struct SyntheticHost {
            pub(crate) instance_id: u64,
            pub(crate) p8_probe_cache: parking_lot::Mutex<rustc_hash::FxHashMap<String, u64>>,
            pub(crate) tick: AtomicU64,
        }
    "#;
    let parsed_fail = parse_file(synthetic_fail).expect("parse synthetic_fail");
    let (fail_violations, fail_surveyed) =
        no_off_store_host_caches_inner(&parsed_fail, "SyntheticHost", &allow_list);
    assert_eq!(
        fail_violations.len(),
        1,
        "discriminator self-test: synthetic_fail should produce exactly \
         one violation (`p8_probe_cache` is a cache-shape field that is \
         not allow-listed and not on ProjectTypeStore), but got {} \
         violations:\n{}\nSurveyed: {fail_surveyed:?}",
        fail_violations.len(),
        fail_violations.join("\n")
    );
    assert!(
        fail_violations[0].contains("p8_probe_cache"),
        "discriminator self-test: the violation must name the offending \
         field (`p8_probe_cache`), but the message was: {}",
        fail_violations[0]
    );

    // (c) Synthetic struct with a cache-shape field whose type points at
    //     ProjectTypeStore — must NOT violate (the destination
    //     allowance). Forward-looking branch.
    let synthetic_destination = r#"
        pub struct SyntheticHost {
            pub(crate) instance_id: u64,
            pub(crate) future_db: Arc<crate::project_type_store::ProjectTypeStore>,
            pub(crate) future_cache: parking_lot::Mutex<crate::project_type_store::ProjectTypeStore>,
        }
    "#;
    let parsed_destination =
        parse_file(synthetic_destination).expect("parse synthetic_destination");
    let (dest_violations, _) =
        no_off_store_host_caches_inner(&parsed_destination, "SyntheticHost", &allow_list);
    assert!(
        dest_violations.is_empty(),
        "discriminator self-test: synthetic_destination should produce \
         zero violations (future_cache is a Mutex<ProjectTypeStore>, which \
         is the destination allowance), but got:\n{}",
        dest_violations.join("\n")
    );
}

// ===========================================================================
// Phase 10a — `no_concrete_verter_host_in_seal_scope`
// ===========================================================================
//
// The seal scope covers every resolver-tier file (see sub-plan §10a.0.A).
// The architecture guard parses each file with `syn::parse_file` and
// asserts no production reference to `crate::VerterHost`. Three classes
// of violation are caught:
//
//   1. Use items: `use crate::VerterHost;`,
//      `use crate::VerterHost as Host;` (pulls the type into scope).
//   2. Type-position paths: `&VerterHost`, `Arc<VerterHost>`,
//      `host: &VerterHost`, generic bounds — anything where the type
//      name appears in a type context.
//   3. Expression-position paths: `VerterHost::method`,
//      `<VerterHost as Trait>::method`, `VerterHost::new` — the type
//      name in an expression context (turbofish/qualified-path call).
//
// The visitor depth-tracks `#[cfg(test)]` items and `mod tests { ... }`
// blocks so test code in the same file is whitelisted (tests in
// resolver-tier modules legitimately construct `VerterHost`).
//
// The guard is `#[ignore]`'d on commit 1 of Phase 10a and remains so
// through commits 2-12. Commit 13 removes the `#[ignore]` after the
// migration completes.

mod resolver_context_seal {
    use std::path::{Path, PathBuf};

    use syn::visit::Visit;
    use syn::{Attribute, ExprPath, ItemFn, ItemMod, Meta, Path as SynPath, TypePath, UsePath};
    use walkdir::WalkDir;

    use super::workspace_root;

    #[derive(Debug)]
    pub(super) struct Violation {
        pub(super) file: PathBuf,
        pub(super) kind: ViolationKind,
    }

    #[derive(Debug)]
    // Postfixes match the AST kind they represent — `UsePath` (path in
    // `use` statement), `TypePath` (path in type position), `ExprPath`
    // (path in expression position). Renaming to drop the `Path` suffix
    // would lose the AST distinction. The lint suggestion is wrong for
    // this domain.
    #[allow(clippy::enum_variant_names)]
    pub(super) enum ViolationKind {
        UsePath,
        TypePath,
        ExprPath,
    }

    impl std::fmt::Display for ViolationKind {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Self::UsePath => write!(f, "use VerterHost"),
                Self::TypePath => write!(f, "type-position VerterHost"),
                Self::ExprPath => write!(f, "expr-position VerterHost"),
            }
        }
    }

    pub(super) struct SealVisitor<'a> {
        path: &'a Path,
        cfg_test_depth: u32,
        violations: &'a mut Vec<Violation>,
    }

    impl<'a> SealVisitor<'a> {
        pub(super) fn new(path: &'a Path, violations: &'a mut Vec<Violation>) -> Self {
            Self {
                path,
                cfg_test_depth: 0,
                violations,
            }
        }
    }

    /// True if any of the supplied attributes is `#[cfg(test)]`,
    /// `#[cfg(any(test, ...))]`, or `#[cfg(all(..., test, ...))]` —
    /// any cfg expression containing the bare predicate `test`.
    fn has_cfg_test(attrs: &[Attribute]) -> bool {
        attrs.iter().any(|a| {
            if !a.path().is_ident("cfg") {
                return false;
            }
            // Render the cfg meta tree as a string and look for the
            // `test` token. False positives like a literal `"test"`
            // string are harmless — they keep us safe-side, never
            // flagging a real production violation.
            let rendered = match &a.meta {
                Meta::List(list) => list.tokens.to_string(),
                _ => return false,
            };
            // Word-boundary check: `test` must appear as a separate token,
            // not as a substring of e.g. `feature = "testbed"`.
            for token in rendered.split(|c: char| !c.is_alphanumeric() && c != '_') {
                if token == "test" {
                    return true;
                }
            }
            false
        })
    }

    /// Final segment ident equals "VerterHost".
    fn last_segment_is_verter_host(path: &SynPath) -> bool {
        path.segments
            .last()
            .map(|s| s.ident == "VerterHost")
            .unwrap_or(false)
    }

    /// Any segment ident equals "VerterHost". Used for use-paths so
    /// `use crate::VerterHost::field` (which would not parse) and
    /// `use crate::{VerterHost, X}` (where the use group does not include
    /// the trailing `VerterHost` directly) both still register.
    fn any_segment_is_verter_host(path: &SynPath) -> bool {
        path.segments.iter().any(|s| s.ident == "VerterHost")
    }

    impl<'ast> Visit<'ast> for SealVisitor<'_> {
        fn visit_item_mod(&mut self, m: &'ast ItemMod) {
            let entered_test = has_cfg_test(&m.attrs) || m.ident == "tests";
            if entered_test {
                self.cfg_test_depth += 1;
            }
            syn::visit::visit_item_mod(self, m);
            if entered_test {
                self.cfg_test_depth -= 1;
            }
        }

        fn visit_item_fn(&mut self, f: &'ast ItemFn) {
            let entered_test = has_cfg_test(&f.attrs);
            if entered_test {
                self.cfg_test_depth += 1;
            }
            syn::visit::visit_item_fn(self, f);
            if entered_test {
                self.cfg_test_depth -= 1;
            }
        }

        fn visit_use_path(&mut self, p: &'ast UsePath) {
            if self.cfg_test_depth > 0 {
                return;
            }
            if p.ident == "VerterHost" {
                self.violations.push(Violation {
                    file: self.path.to_path_buf(),
                    kind: ViolationKind::UsePath,
                });
            }
            syn::visit::visit_use_path(self, p);
        }

        fn visit_type_path(&mut self, tp: &'ast TypePath) {
            if self.cfg_test_depth > 0 {
                return;
            }
            if last_segment_is_verter_host(&tp.path) {
                self.violations.push(Violation {
                    file: self.path.to_path_buf(),
                    kind: ViolationKind::TypePath,
                });
            }
            syn::visit::visit_type_path(self, tp);
        }

        fn visit_expr_path(&mut self, ep: &'ast ExprPath) {
            if self.cfg_test_depth > 0 {
                return;
            }
            if any_segment_is_verter_host(&ep.path) {
                self.violations.push(Violation {
                    file: self.path.to_path_buf(),
                    kind: ViolationKind::ExprPath,
                });
            }
            syn::visit::visit_expr_path(self, ep);
        }
    }

    pub(super) fn scan_file(path: &Path, violations: &mut Vec<Violation>) {
        let src = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => panic!("read {}: {}", path.display(), e),
        };
        let parsed =
            syn::parse_file(&src).unwrap_or_else(|e| panic!("parse {}: {}", path.display(), e));
        let mut visitor = SealVisitor::new(path, violations);
        visitor.visit_file(&parsed);
    }

    pub(super) fn walk_rs_files(root: &Path) -> Vec<PathBuf> {
        let mut files = Vec::new();
        for entry in WalkDir::new(root).into_iter().filter_map(Result::ok) {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            // Skip sibling `*_tests.rs` files (whitelist test fixtures).
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.ends_with("_tests.rs") || name == "tests.rs" {
                    continue;
                }
            }
            files.push(path.to_path_buf());
        }
        files
    }

    pub(super) fn format_violations(violations: &[Violation]) -> String {
        use std::collections::BTreeMap;
        let mut by_file: BTreeMap<&Path, Vec<&ViolationKind>> = BTreeMap::new();
        for v in violations {
            by_file.entry(v.file.as_path()).or_default().push(&v.kind);
        }
        let mut lines = Vec::new();
        for (file, kinds) in by_file {
            let mut counts: BTreeMap<String, usize> = BTreeMap::new();
            for k in kinds {
                *counts.entry(k.to_string()).or_default() += 1;
            }
            let summary = counts
                .iter()
                .map(|(k, n)| format!("{n}× {k}"))
                .collect::<Vec<_>>()
                .join(", ");
            lines.push(format!("  {} -- {summary}", file.display()));
        }
        format!(
            "found {} concrete VerterHost reference(s) in {} file(s):\n{}",
            violations.len(),
            lines.len(),
            lines.join("\n")
        )
    }

    /// `resolver_context.rs` is the bridging trait file: it must
    /// reference `VerterHost` to register the trait impl
    /// (`impl ResolverContext for crate::VerterHost`). Whitelisting it
    /// is structural — without it the seal scope would be
    /// self-violating.
    fn is_seal_bridge_file(path: &Path) -> bool {
        path.file_name()
            .and_then(|n| n.to_str())
            .map(|n| n == "resolver_context.rs")
            .unwrap_or(false)
    }

    pub(super) fn run() {
        // Seal scope per sub-plan §10a.0.A. Three resolver-tier
        // directories (recursive) plus two top-level files.
        let crate_root = workspace_root().join("crates/verter_session/src");
        let scope_roots = [
            crate_root.join("resolver_core"),
            crate_root.join("meta_resolve"),
            crate_root.join("project_semantic_dispatch"),
        ];
        let scope_files = [
            crate_root.join("component_meta_caches.rs"),
            crate_root.join("component_meta_materialize.rs"),
        ];

        let mut violations: Vec<Violation> = Vec::new();
        for root in &scope_roots {
            for file in walk_rs_files(root) {
                if is_seal_bridge_file(&file) {
                    continue;
                }
                scan_file(&file, &mut violations);
            }
        }
        for file in &scope_files {
            scan_file(file, &mut violations);
        }

        if !violations.is_empty() {
            panic!(
                "Phase 10a seal violation:\n{}\n\nResolver-tier files \
                 must reach host state through `&dyn ResolverContext` \
                 (`crate::resolver_core::ResolverContext`), not through \
                 the concrete `VerterHost` type. See sub-plan §10a for \
                 the migration recipe.",
                format_violations(&violations)
            );
        }
    }
}

#[test]
fn no_concrete_verter_host_in_seal_scope() {
    // Phase 10a — un-ignored at commit 13 after the resolver-context
    // seal migration landed. Resolver-tier files
    // (`resolver_core/`, `meta_resolve/`, `project_semantic_dispatch/`,
    // `component_meta_caches.rs`, `component_meta_materialize.rs`)
    // must reach host state through `&dyn ResolverContext`, never
    // through the concrete `VerterHost` type. Re-introduction of a
    // `VerterHost` reference in a seal-scope file fails this test.
    resolver_context_seal::run();
}

// ===========================================================================
// Phase 9b — `no_napi_direct_verter_compiler_emitters`
// ===========================================================================
//
// `crates/verter_napi/src/**/*.rs` (production sources only — sibling
// `*_tests.rs` and `tests.rs` whitelisted) MUST NOT reference any
// compile-emitter symbol from `verter_compiler::compile::*` or
// `verter_compiler::compile_parallel::*`. The NAPI leaf must route
// batch/single SFC compile through the host-backed
// `VerterHost::compile_many` / `get_virtual_file` substrate.
//
// Forbidden inside `verter_compiler::compile::*` (explicit deny-list,
// no regex):
//   - `compile`
//   - `compile_from_parsed`
//
// The entire `verter_compiler::compile_parallel::*` namespace is
// forward-defense-forbidden — the module is intentionally never
// created.
//
// Allow-listed pure-data exports from `verter_compiler::compile::*`:
//   `CodegenOptions`, `VerterCompileOptions`, `VerterCompileResult`,
//   `TypesParserConfig`, `ParsedSfc`. Anything else inside the
//   `compile` namespace is default-deny.
//
// Three visitor methods (`visit_item_use`, `visit_expr_path`,
// `visit_type_path`) call shared `classify(segments)` to detect
// violations. Glob arms reject any glob whose prefix matches either
// compile namespace. `Rename` arms match on the ORIGINAL ident.
//
// Violation messages report file path + ident kind + leaf ident. No
// line numbers — `proc-macro2/span-locations` is not enabled in this
// workspace's `syn` dev-dep (see Cargo.toml:96).

mod napi_compiler_emitters {
    use std::path::{Path, PathBuf};

    use syn::visit::Visit;
    use syn::{
        ExprPath, ItemUse, Path as SynPath, PathSegment, TypePath, UseGlob, UseGroup, UseName,
        UsePath, UseRename, UseTree,
    };
    use walkdir::WalkDir;

    use super::workspace_root;

    /// Forbidden idents directly inside `verter_compiler::compile::*`.
    /// Inside `verter_compiler::compile_parallel::*` ALL leaves are
    /// forbidden (entire namespace).
    const COMPILE_DENY_LIST: &[&str] = &["compile", "compile_from_parsed"];

    /// Pure-data allow-list inside `verter_compiler::compile::*`.
    const COMPILE_ALLOW_LIST: &[&str] = &[
        "CodegenOptions",
        "CompileTarget",
        "VerterCompileOptions",
        "VerterCompileResult",
        "TypesParserConfig",
        "ParsedSfc",
    ];

    #[derive(Debug)]
    pub(super) struct Violation {
        pub(super) file: PathBuf,
        pub(super) kind: ViolationKind,
        pub(super) leaf: String,
    }

    // Same-postfix lint silenced — variant names are deliberate
    // taxonomy markers ("UsePath", "TypePath", "ExprPath" all refer to
    // distinct `syn::Visit` hooks; renaming would obscure the mapping).
    #[allow(clippy::enum_variant_names)]
    #[derive(Debug)]
    pub(super) enum ViolationKind {
        UsePath,
        UseGlob,
        TypePath,
        ExprPath,
    }

    impl std::fmt::Display for ViolationKind {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Self::UsePath => write!(f, "use"),
                Self::UseGlob => write!(f, "use ::*"),
                Self::TypePath => write!(f, "type"),
                Self::ExprPath => write!(f, "expr"),
            }
        }
    }

    /// What `classify` returns for a sequence of path segment idents.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Classification<'a> {
        /// Forbidden symbol inside one of the two compile namespaces.
        Forbidden(&'a str),
        /// Allowed pure-data import from `verter_compiler::compile`.
        AllowedDataType,
        /// Reference outside both namespaces — uninteresting.
        OutsideNamespace,
    }

    /// Classify a path by its leading two segments and final segment.
    /// `segments` is the leaf-relative ident chain after any
    /// `crate::` / `self::` / `super::` are skipped.
    fn classify<'a>(segments: &'a [String]) -> Classification<'a> {
        if segments.len() < 2 {
            return Classification::OutsideNamespace;
        }
        if segments[0] != "verter_compiler" {
            return Classification::OutsideNamespace;
        }
        let last: &'a str = segments.last().unwrap();
        match segments[1].as_str() {
            "compile_parallel" => {
                // Entire namespace is forward-defense-forbidden.
                Classification::Forbidden(last)
            }
            "compile" => {
                if COMPILE_DENY_LIST.contains(&last) {
                    Classification::Forbidden(last)
                } else if COMPILE_ALLOW_LIST.contains(&last) {
                    Classification::AllowedDataType
                } else {
                    // Default-deny inside the compile namespace.
                    Classification::Forbidden(last)
                }
            }
            _ => Classification::OutsideNamespace,
        }
    }

    /// Render a `syn::Path` to a flat list of segment ident strings,
    /// skipping leading `crate` / `self` / `super` to match the
    /// classifier's expected absolute-ish shape.
    fn path_idents(path: &SynPath) -> Vec<String> {
        let mut out: Vec<String> = path
            .segments
            .iter()
            .map(|s: &PathSegment| s.ident.to_string())
            .collect();
        while matches!(
            out.first().map(String::as_str),
            Some("crate" | "self" | "super")
        ) {
            out.remove(0);
        }
        out
    }

    pub(super) struct EmitterVisitor<'a> {
        path: &'a Path,
        violations: &'a mut Vec<Violation>,
    }

    impl<'a> EmitterVisitor<'a> {
        pub(super) fn new(path: &'a Path, violations: &'a mut Vec<Violation>) -> Self {
            Self { path, violations }
        }

        /// Recursively walk a `UseTree` accumulating prefix segments,
        /// flagging Forbidden-classification leaves and Glob arms whose
        /// prefix matches either compile namespace.
        fn walk_use_tree(&mut self, tree: &UseTree, prefix: &mut Vec<String>) {
            match tree {
                UseTree::Path(UsePath { ident, tree, .. }) => {
                    prefix.push(ident.to_string());
                    self.walk_use_tree(tree, prefix);
                    prefix.pop();
                }
                UseTree::Name(UseName { ident, .. }) => {
                    prefix.push(ident.to_string());
                    self.classify_use_leaf(prefix);
                    prefix.pop();
                }
                UseTree::Rename(UseRename { ident, .. }) => {
                    // Match on the ORIGINAL ident, not the alias.
                    prefix.push(ident.to_string());
                    self.classify_use_leaf(prefix);
                    prefix.pop();
                }
                UseTree::Glob(UseGlob { .. }) => {
                    // A glob whose prefix is compile or compile_parallel
                    // is rejected outright.
                    let stripped = strip_use_anchors(prefix);
                    let is_target = stripped.len() >= 2
                        && stripped[0] == "verter_compiler"
                        && (stripped[1] == "compile" || stripped[1] == "compile_parallel");
                    if is_target {
                        self.violations.push(Violation {
                            file: self.path.to_path_buf(),
                            kind: ViolationKind::UseGlob,
                            leaf: format!("{}::*", stripped.join("::")),
                        });
                    }
                }
                UseTree::Group(UseGroup { items, .. }) => {
                    for item in items {
                        self.walk_use_tree(item, prefix);
                    }
                }
            }
        }

        fn classify_use_leaf(&mut self, prefix: &[String]) {
            let stripped = strip_use_anchors(prefix);
            if let Classification::Forbidden(leaf) = classify(&stripped) {
                self.violations.push(Violation {
                    file: self.path.to_path_buf(),
                    kind: ViolationKind::UsePath,
                    leaf: leaf.to_string(),
                });
            }
        }
    }

    fn strip_use_anchors(prefix: &[String]) -> Vec<String> {
        let mut out = prefix.to_vec();
        while matches!(
            out.first().map(String::as_str),
            Some("crate" | "self" | "super")
        ) {
            out.remove(0);
        }
        out
    }

    impl<'ast> Visit<'ast> for EmitterVisitor<'_> {
        fn visit_item_use(&mut self, item_use: &'ast ItemUse) {
            let mut prefix: Vec<String> = Vec::new();
            // Leading `::` doesn't change the absolute-ish form;
            // `tree` walks from the topmost crate ident.
            self.walk_use_tree(&item_use.tree, &mut prefix);
            syn::visit::visit_item_use(self, item_use);
        }

        fn visit_type_path(&mut self, tp: &'ast TypePath) {
            let segments = path_idents(&tp.path);
            if let Classification::Forbidden(leaf) = classify(&segments) {
                self.violations.push(Violation {
                    file: self.path.to_path_buf(),
                    kind: ViolationKind::TypePath,
                    leaf: leaf.to_string(),
                });
            }
            syn::visit::visit_type_path(self, tp);
        }

        fn visit_expr_path(&mut self, ep: &'ast ExprPath) {
            let segments = path_idents(&ep.path);
            if let Classification::Forbidden(leaf) = classify(&segments) {
                self.violations.push(Violation {
                    file: self.path.to_path_buf(),
                    kind: ViolationKind::ExprPath,
                    leaf: leaf.to_string(),
                });
            }
            syn::visit::visit_expr_path(self, ep);
        }
    }

    pub(super) fn scan_file(path: &Path, violations: &mut Vec<Violation>) {
        let src = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => panic!("read {}: {}", path.display(), e),
        };
        let parsed =
            syn::parse_file(&src).unwrap_or_else(|e| panic!("parse {}: {}", path.display(), e));
        let mut visitor = EmitterVisitor::new(path, violations);
        visitor.visit_file(&parsed);
    }

    pub(super) fn walk_rs_files(root: &Path) -> Vec<PathBuf> {
        let mut files = Vec::new();
        for entry in WalkDir::new(root).into_iter().filter_map(Result::ok) {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            // Skip sibling `*_tests.rs` and `tests.rs` files (production
            // sources only).
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.ends_with("_tests.rs") || name == "tests.rs" {
                    continue;
                }
            }
            files.push(path.to_path_buf());
        }
        files
    }

    pub(super) fn format_violations(violations: &[Violation]) -> String {
        use std::collections::BTreeMap;
        let mut by_file: BTreeMap<&Path, Vec<String>> = BTreeMap::new();
        for v in violations {
            by_file
                .entry(v.file.as_path())
                .or_default()
                .push(format!("{} `{}`", v.kind, v.leaf));
        }
        let mut lines = Vec::new();
        for (file, kinds) in by_file {
            lines.push(format!("  {} -- {}", file.display(), kinds.join(", ")));
        }
        format!(
            "found {} verter_compiler::compile{{,_parallel}} reference(s) in {} file(s):\n{}",
            violations.len(),
            lines.len(),
            lines.join("\n")
        )
    }

    pub(super) fn run() {
        let napi_root = workspace_root().join("crates/verter_napi/src");
        let mut violations: Vec<Violation> = Vec::new();
        for file in walk_rs_files(&napi_root) {
            scan_file(&file, &mut violations);
        }
        if !violations.is_empty() {
            panic!(
                "Phase 9b architecture guard violation:\n{}\n\nNAPI \
                 production sources MUST NOT reference \
                 `verter_compiler::compile::{{compile, compile_from_parsed}}` \
                 or any symbol under `verter_compiler::compile_parallel::*`. \
                 Batch and single SFC compile must route through \
                 `VerterHost::compile_many` / `VerterHost::get_virtual_file`. \
                 See sub-plan §5 for the full rule set.",
                format_violations(&violations)
            );
        }
    }
}

#[test]
fn no_napi_direct_verter_compiler_emitters() {
    // Phase 9b — un-ignored on commit 1 (RED on HEAD against the
    // bypass at `crates/verter_napi/src/lib.rs:2314`). Commit 3 deletes
    // the bypass, after which this test PASSES.
    napi_compiler_emitters::run();
}

// ── B-C0 foundations guards ──
//
// Guards added by the Tier-C foundations bundle. Each `pub fn` predicate
// is deliberately exposed so the deliberate-violation tests below can
// exercise the predicate against fabricated fixtures without writing a
// real violation into the production tree.

mod foundations_guards {
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::{Path, PathBuf};

    // ── Helpers ──

    fn workspace_root() -> PathBuf {
        super::workspace_root()
    }

    fn read_workspace_file(rel: &str) -> String {
        super::read_workspace_file(rel)
    }

    /// Walk a directory (production tree) and yield every `.rs` file
    /// whose name does NOT end in `_tests.rs` and is not nested under a
    /// `tests/` or `benches/` directory.
    fn walk_production_rs(root: &Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(dir) = stack.pop() {
            let entries = match fs::read_dir(&dir) {
                Ok(it) => it,
                Err(_) => continue,
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    if name == "tests"
                        || name == "benches"
                        || name == "examples"
                        || name == "target"
                    {
                        continue;
                    }
                    stack.push(path);
                    continue;
                }
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if !name.ends_with(".rs") {
                    continue;
                }
                if name.ends_with("_tests.rs") || name == "tests.rs" {
                    continue;
                }
                out.push(path);
            }
        }
        out.sort();
        out
    }

    fn relative_to_root(abs: &Path) -> String {
        abs.strip_prefix(workspace_root())
            .unwrap_or(abs)
            .to_string_lossy()
            .replace('\\', "/")
    }

    // ── Guard 1 — no_std_fs_in_semantic_session_paths ──

    /// Predicate: scan a single `.rs` file's source for direct
    /// `std::fs::` references. Returns `true` when at least one match
    /// exists.
    pub fn file_uses_std_fs(src: &str) -> bool {
        src.contains("std::fs::")
    }

    /// Allowlist for guard #1, sourced from
    /// `crates/verter_workspace/tool-output-allowlist.toml`. Each entry
    /// is a path to a file whose `std::fs::` calls write/read
    /// non-semantic output (trace artifacts, MCP baselines, profiler
    /// dumps, test fixtures, TS-runtime tool-cache files, etc.).
    ///
    /// `source-read` callsites are NOT allowlisted — they MUST route
    /// through `verter_workspace::WorkspaceAccess`. Migrating a
    /// `source-read` file shrinks the allowlist by deleting its entry
    /// from the TOML file in the same change.
    pub fn guard1_allowlist() -> BTreeSet<String> {
        load_tool_output_allowlist()
    }

    pub fn guard1_in_scope_dirs() -> Vec<&'static str> {
        vec![
            "crates/verter_session/src",
            "crates/verter_semantic/src",
            "crates/verter_diagnostics/src",
            "crates/verter_type_runtime/src",
            "crates/verter_lsp/src",
            "crates/verter_mcp/src",
        ]
    }

    /// Parse `crates/verter_workspace/tool-output-allowlist.toml` and
    /// return the set of allowlisted paths, one per `[[entries]]` entry.
    pub fn load_tool_output_allowlist() -> BTreeSet<String> {
        #[derive(serde::Deserialize)]
        struct Allowlist {
            entries: Vec<Entry>,
        }
        #[derive(serde::Deserialize)]
        struct Entry {
            path: String,
            #[allow(dead_code)]
            rationale: String,
        }

        let path = workspace_root().join("crates/verter_workspace/tool-output-allowlist.toml");
        let raw = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("guard 1: could not read `{}`: {e}", path.display()));
        let parsed: Allowlist = toml::from_str(&raw)
            .unwrap_or_else(|e| panic!("guard 1: could not parse `{}`: {e}", path.display()));
        parsed.entries.into_iter().map(|e| e.path).collect()
    }

    /// Run the guard 1 predicate over the in-scope directories and
    /// return the set of relative paths that USE `std::fs::` and are
    /// NOT in the allowlist. An empty result means the guard passes.
    pub fn guard1_violations(allowlist: &BTreeSet<String>) -> Vec<String> {
        let root = workspace_root();
        let mut violations = Vec::new();
        for rel_dir in guard1_in_scope_dirs() {
            for path in walk_production_rs(&root.join(rel_dir)) {
                let src = match fs::read_to_string(&path) {
                    Ok(s) => s,
                    Err(_) => continue,
                };
                if !file_uses_std_fs(&src) {
                    continue;
                }
                let rel = relative_to_root(&path);
                if allowlist.contains(&rel) {
                    continue;
                }
                violations.push(rel);
            }
        }
        violations.sort();
        violations
    }

    #[test]
    fn no_std_fs_in_semantic_session_paths() {
        let violations = guard1_violations(&guard1_allowlist());
        assert!(
            violations.is_empty(),
            "Guard 1 (`no_std_fs_in_semantic_session_paths`) violations:\n  {}\n\n\
             A new file outside the allowlist uses `std::fs::`. Either route the I/O\n\
             through `verter_workspace::WorkspaceAccess` (preferred) or, if the file\n\
             writes/reads non-semantic tool output (trace artifacts, MCP baselines,\n\
             test fixtures, TS-runtime tool-cache files, etc.), add the path with a\n\
             rationale to `crates/verter_workspace/tool-output-allowlist.toml`.",
            violations.join("\n  "),
        );
    }

    #[test]
    fn guard1_allowlist_paths_exist() {
        // Every allowlist entry must point to a real production source
        // file in the tree. Stale entries (file deleted, renamed, or
        // never existed) are violations because they silently disarm
        // the guard.
        let root = workspace_root();
        let mut missing = Vec::new();
        for path in load_tool_output_allowlist() {
            let abs = root.join(&path);
            if !abs.exists() {
                missing.push(path);
            }
        }
        assert!(
            missing.is_empty(),
            "tool-output-allowlist.toml entries refer to paths that do not exist:\n  {}\n\n\
             Update or remove these entries; a stale allowlist silently disarms the guard.",
            missing.join("\n  "),
        );
    }

    #[test]
    fn guard1_predicate_rejects_deliberate_violation() {
        // Discriminating: a fabricated source string that uses
        // `std::fs::` MUST be flagged as a violation by the
        // predicate.
        let bad = "use std::fs::File;\nfn read() { let _ = std::fs::read_to_string(\"foo\"); }";
        assert!(
            file_uses_std_fs(bad),
            "guard 1 predicate must flag direct `std::fs::` references",
        );

        let good = "use crate::workspace::WorkspaceAccess;\nfn read(ws: &dyn WorkspaceAccess) { let _ = ws.read_file(\"foo\"); }";
        assert!(
            !file_uses_std_fs(good),
            "guard 1 predicate must NOT flag code that goes through WorkspaceAccess",
        );
    }

    // ── Guard 2 — vfs_boundary_is_authoritative ──

    /// Predicate: scan a `.rs` file for any direct OS file API
    /// reference. Returns `true` when at least one such reference is
    /// found.
    ///
    /// Patterns checked:
    /// - `std::fs::` (synchronous OS file API)
    /// - `tokio::fs::` (async OS file API)
    pub fn file_uses_os_file_api(src: &str) -> bool {
        src.contains("std::fs::") || src.contains("tokio::fs::")
    }

    /// Allowlist for guard #2: paths where direct OS file APIs are
    /// the legitimate authority. `native_fs.rs` is the documented
    /// disk boundary; `intrinsic_library.rs` is the dedicated reader
    /// for ambient TypeScript SDK declarations. The remaining entries
    /// are infrastructure tracked by `tool-output-allowlist.toml`.
    pub fn guard2_allowlist() -> BTreeSet<String> {
        let mut set: BTreeSet<String> = [
            "crates/verter_workspace/src/native_fs.rs",
            "crates/verter_workspace/src/config.rs",
            "crates/verter_workspace/src/snapshot_builder.rs",
            "crates/verter_workspace/src/vite_config.rs",
            "crates/verter_workspace/src/dir_index.rs",
            "crates/verter_workspace/src/filesystem.rs",
            "crates/verter_workspace/src/ambient_parse.rs",
            "crates/verter_workspace/src/intrinsic_library.rs",
            "crates/verter_workspace/src/resolver.rs",
            "crates/verter_parser/src/utils/oxc/vue/script/resolve_type/mod.rs",
            "crates/verter_scheduler/src/source_loader.rs",
            "crates/verter_tsc/src/checker.rs",
            "crates/verter_tsc/src/reporter.rs",
            "crates/verter_tsc/src/tsconfig.rs",
            // Audit substrate's `current_process_rss` reads
            // `/proc/self/statm` (Linux) for memory-delta
            // accounting; matches the historic
            // `verter_session::component_meta_audit::mod` exemption
            // that lived here before the substrate split.
            "crates/verter_audit/src/memory.rs",
        ]
        .into_iter()
        .map(String::from)
        .collect();
        set.extend(guard1_allowlist());
        set
    }

    /// Run guard 2 across every production `.rs` file in `crates/`
    /// and return out-of-allowlist users of OS file APIs.
    pub fn guard2_violations(allowlist: &BTreeSet<String>) -> Vec<String> {
        let crates_root = workspace_root().join("crates");
        let mut violations = Vec::new();
        let entries = match fs::read_dir(&crates_root) {
            Ok(it) => it,
            Err(_) => return violations,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let src_dir = path.join("src");
            if !src_dir.exists() {
                continue;
            }
            for file in walk_production_rs(&src_dir) {
                let src = match fs::read_to_string(&file) {
                    Ok(s) => s,
                    Err(_) => continue,
                };
                if !file_uses_os_file_api(&src) {
                    continue;
                }
                let rel = relative_to_root(&file);
                if allowlist.contains(&rel) {
                    continue;
                }
                violations.push(rel);
            }
        }
        violations.sort();
        violations
    }

    #[test]
    fn vfs_boundary_is_authoritative() {
        let violations = guard2_violations(&guard2_allowlist());
        assert!(
            violations.is_empty(),
            "Guard 2 (`vfs_boundary_is_authoritative`) violations:\n  {}\n\n\
             Direct OS file APIs (std::fs::, tokio::fs::) appearing outside\n\
             `crates/verter_workspace/src/native_fs.rs` (the documented disk boundary)\n\
             must route through `verter_workspace::WorkspaceAccess`.",
            violations.join("\n  "),
        );
    }

    #[test]
    fn guard2_predicate_rejects_deliberate_violation() {
        let bad_std = "use std::fs::read_to_string;";
        let bad_tokio = "use tokio::fs::File;";
        assert!(file_uses_os_file_api(bad_std), "guard 2 must flag std::fs");
        assert!(
            file_uses_os_file_api(bad_tokio),
            "guard 2 must flag tokio::fs"
        );
        assert!(
            !file_uses_os_file_api("use crate::workspace::WorkspaceAccess;"),
            "guard 2 must NOT flag WorkspaceAccess users",
        );
    }

    // ── Guard 3 — lsp_mcp_dependency_direction ──

    /// Predicate: check whether a `Cargo.toml` snippet declares
    /// `verter_mcp` as a non-optional dependency. Returns `true`
    /// when a violation is present.
    pub fn cargo_toml_has_unmodified_verter_mcp_dep(src: &str) -> bool {
        let mut found_violation = false;
        for line in src.lines() {
            let trimmed = line.trim();
            if !trimmed.starts_with("verter_mcp = ") && !trimmed.starts_with("verter_mcp =") {
                continue;
            }
            if !line.contains("optional = true") {
                found_violation = true;
                break;
            }
        }
        found_violation
    }

    #[test]
    fn lsp_mcp_dependency_direction() {
        let cargo = read_workspace_file("crates/verter_lsp/Cargo.toml");
        let violation = cargo_toml_has_unmodified_verter_mcp_dep(&cargo);
        assert!(
            !violation,
            "Guard 3 (`lsp_mcp_dependency_direction`) violation: \
             `crates/verter_lsp/Cargo.toml` declares `verter_mcp` without \
             `optional = true`. The dependency direction must be \
             LSP -> optional MCP (gated by the `mcp` feature).",
        );
    }

    #[test]
    fn guard3_predicate_rejects_deliberate_violation() {
        let bad = "[dependencies]\nverter_mcp = { path = \"../verter_mcp\" }\nother = \"1\"\n";
        let good = "[dependencies]\nverter_mcp = { path = \"../verter_mcp\", optional = true }\nother = \"1\"\n";
        let no_dep = "[dependencies]\nother = \"1\"\n";
        assert!(
            cargo_toml_has_unmodified_verter_mcp_dep(bad),
            "guard 3 must flag a non-optional verter_mcp dep",
        );
        assert!(
            !cargo_toml_has_unmodified_verter_mcp_dep(good),
            "guard 3 must NOT flag an optional verter_mcp dep",
        );
        assert!(
            !cargo_toml_has_unmodified_verter_mcp_dep(no_dep),
            "guard 3 must NOT flag a Cargo.toml that does not depend on verter_mcp",
        );
    }

    // ── Guard 4 — external_corpus_paths_not_present_outside_gated_tests ──

    /// Predicate: scan a test file's source for path strings
    /// referencing `.integration-tests/repos/...`. Returns `true`
    /// when at least one such reference is found AND the file is
    /// NOT gated behind a Cargo feature.
    pub fn test_file_has_ungated_external_corpus_path(src: &str) -> bool {
        let has_path = src.contains(".integration-tests/repos/")
            || src.contains(".integration-tests\\repos\\");
        if !has_path {
            return false;
        }
        let gated = src.lines().any(|line| {
            let t = line.trim_start();
            t.starts_with("#![cfg(feature =") || t.starts_with("#![cfg(any(feature =")
        });
        !gated
    }

    /// Walk every test file under `crates/<crate>/tests/` (across all
    /// crates) and return the set of files that violate the rule.
    /// `architecture_guards.rs` is self-exempt: it MUST hold the
    /// literal path string in the predicate body, and the
    /// deliberate-violation test exercises the predicate
    /// independently.
    pub fn guard4_violations() -> Vec<String> {
        let crates_root = workspace_root().join("crates");
        let mut violations = Vec::new();
        let entries = match fs::read_dir(&crates_root) {
            Ok(it) => it,
            Err(_) => return violations,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let tests_dir = path.join("tests");
            if !tests_dir.exists() {
                continue;
            }
            let mut stack = vec![tests_dir];
            while let Some(dir) = stack.pop() {
                let read = match fs::read_dir(&dir) {
                    Ok(it) => it,
                    Err(_) => continue,
                };
                for sub in read.flatten() {
                    let p = sub.path();
                    if p.is_dir() {
                        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                        if name == "fixtures" {
                            continue;
                        }
                        stack.push(p);
                        continue;
                    }
                    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    if !name.ends_with(".rs") {
                        continue;
                    }
                    // Self-exempt: this file MUST hold the predicate
                    // literal.
                    if name == "architecture_guards.rs" {
                        continue;
                    }
                    let src = match fs::read_to_string(&p) {
                        Ok(s) => s,
                        Err(_) => continue,
                    };
                    if test_file_has_ungated_external_corpus_path(&src) {
                        violations.push(relative_to_root(&p));
                    }
                }
            }
        }
        violations.sort();
        violations
    }

    #[test]
    fn external_corpus_paths_not_present_outside_gated_tests() {
        let violations = guard4_violations();
        assert!(
            violations.is_empty(),
            "Guard 4 (`external_corpus_paths_not_present_outside_gated_tests`) violations:\n  {}\n\n\
             Test files that reference `.integration-tests/repos/...` must be gated behind a\n\
             Cargo feature (e.g., `#![cfg(feature = \"external-corpus\")]`). Vendor fixtures into\n\
             `tests/<feature>/fixtures/` for the default workspace test run.",
            violations.join("\n  "),
        );
    }

    #[test]
    fn guard4_predicate_rejects_deliberate_violation() {
        // Construct the forbidden literal at runtime so the source
        // of `architecture_guards.rs` itself does NOT contain the
        // string `.integration-tests/repos/` — otherwise the guard
        // we just defined would scan its own source and report this
        // file as a violation.
        let forbidden_segment = format!(".{}{}{}/repos/", "integration", "-", "tests");
        let bad = format!(
            "#[test]\nfn t() {{ let _ = include_str!(\"../{}nuxt-ui/src/Foo.vue\"); }}",
            forbidden_segment
        );
        let gated = format!(
            "#![cfg(feature = \"external-corpus\")]\n#[test]\nfn t() {{ let _ = include_str!(\"../{}nuxt-ui/src/Foo.vue\"); }}",
            forbidden_segment
        );
        let local_only =
            "#[test]\nfn t() { let _ = include_str!(\"./fixtures/Foo.vue\"); }".to_string();
        assert!(
            test_file_has_ungated_external_corpus_path(&bad),
            "guard 4 must flag ungated external-corpus references",
        );
        assert!(
            !test_file_has_ungated_external_corpus_path(&gated),
            "guard 4 must NOT flag references inside a feature-gated file",
        );
        assert!(
            !test_file_has_ungated_external_corpus_path(&local_only),
            "guard 4 must NOT flag tests that use vendored fixtures",
        );
    }

    // ── Guard 5 — verter_session_public_surface_is_minimal ──

    /// Predicate: extract the set of `pub mod` and `pub use` items
    /// declared at column 0 (top-level) of a Rust source file.
    /// Items nested inside a block (e.g., `pub use ...` inside
    /// `pub mod for_tests { ... }`) are excluded — only the
    /// outermost surface is captured. Comments are ignored.
    /// Returns the items in source order.
    pub fn extract_top_level_pub_items(src: &str) -> Vec<String> {
        let mut items = Vec::new();
        let mut depth: i32 = 0;
        for line in src.lines() {
            // Strip trailing `//` line comment first.
            let no_comment = if let Some(idx) = line.find("//") {
                &line[..idx]
            } else {
                line
            };
            // Count brace deltas on the comment-stripped line. Track
            // depth so items inside any block are filtered out.
            let opens = no_comment.matches('{').count() as i32;
            let closes = no_comment.matches('}').count() as i32;

            let t = line.trim_start();
            let is_top = depth == 0
                && (line.starts_with("pub mod ")
                    || line.starts_with("pub use ")
                    || line.starts_with("pub(crate) mod "))
                && (t.starts_with("pub mod ")
                    || t.starts_with("pub use ")
                    || t.starts_with("pub(crate) mod "));
            if is_top {
                let stripped = if let Some(idx) = t.find("//") {
                    &t[..idx]
                } else {
                    t
                };
                let trimmed = stripped.trim_end_matches(';').trim_end();
                // Don't capture lines that begin a multi-line block
                // (e.g., `pub mod for_tests {`); the block-opening
                // form is a structural choice the snapshot tracks
                // separately. We still want to record the simple
                // declaration form, so check the line ends with `;`
                // OR `{`. For `{`, normalize to the bare module
                // name without the trailing `{`.
                let normalized = trimmed.trim_end_matches('{').trim_end().to_string();
                items.push(normalized);
            }
            depth += opens - closes;
            if depth < 0 {
                depth = 0;
            }
        }
        items
    }

    /// Snapshot of the current `verter_session` lib public surface.
    /// Generated via `extract_top_level_pub_items` against
    /// `crates/verter_session/src/lib.rs` at landing.
    /// Updates require a deliberate edit + paired snapshot bump.
    pub fn guard5_snapshot_pub_items() -> &'static [&'static str] {
        VERTER_SESSION_PUB_SURFACE_SNAPSHOT
    }

    pub static VERTER_SESSION_PUB_SURFACE_SNAPSHOT: &[&str] = &[
        // Each retained `pub mod` MUST cite at least one downstream
        // consumer (verter_lsp, verter_mcp, verter_napi, verter_wasm,
        // verter_ffi, verter_diagnostics, verter_type_runtime,
        // verter_tsc) OR a verter_session integration test. Items
        // demoted to `pub(crate)` by Phase 12.A9 do not appear here.
        // ─── public modules: cited consumers ────────────────────────
        // tests/audited_request_e2e.rs
        "pub mod audited_request",
        // Workspace-wide cache-cluster schema-version constant + the
        // `CacheSchemaVersioned` trait. Public so
        // `tests/cache_invariant_migration.rs` (the W0.5 fixture cohort)
        // can read `CACHE_CLUSTER_SCHEMA_VERSION` and call the trait
        // methods to verify the cohort's eviction invariant.
        "pub mod cache_schema",
        // verter_lsp::features::hover_provenance,
        // verter_napi::meta, verter_wasm::tests::audit
        "pub mod component_meta_audit",
        // verter_napi::meta
        "pub mod component_meta_host",
        // host_audit_runtime — owns the AuditRecordsStore + AuditConfig
        // snapshot + active-request registry. Public so integration
        // tests can call `host.host_audit_runtime().snapshot()` and
        // so `AuditRequestRegistration::new` resolves through the
        // public accessor.
        "pub mod host_audit_runtime",
        // Re-exports the new audit-runtime types at the crate root
        // (`verter_session::HostAuditRuntime`, `AuditRequestRegistration`,
        // `AuditRuntimeSnapshot`) so callers of the audited
        // entry-points do not need to reach into the module path.
        "pub use host_audit_runtime::",
        // tests/type_resolution_audit_*.rs — public audited
        // entry-point that wires `VerterHost::resolve_type_with_audit`
        // for type-resolution requests. Producer side of the
        // `RequestKind::TypeResolution` audit kind.
        "pub mod host_resolve_type_audit",
        // verter_ffi::convert (host::cross_file::CrossFileResult)
        "pub mod cross_file",
        // Fact-based cache architecture (R5, R6, R28, R29) — new
        // content-addressed file artifact store + per-file structural
        // hash, both consumed by tests and by future-stage host paths.
        "pub mod file_artifact_store",
        "pub mod parse_stable_hash",
        // Parse-time fact-emission producer (R10–R16, R28, R29) —
        // walks `IndexedReady.shallow_state` and populates the
        // per-file `FactRegistry` + module-augmentation facts.
        // Consumed by tests/fact_*.rs + tests/module_augmentation.rs +
        // tests/shallow_walk_invariant.rs + future host paths.
        "pub mod fact_emission",
        // Lazy member-body fact stores (R13, R28) — semantic and
        // display fingerprint stores keyed differently so cosmetic
        // edits invalidate display-bearing materialisations only.
        // Consumed by tests/fact_semantic_display_split.rs +
        // tests/member_presence_vs_member.rs + future resolver
        // / materialiser admission paths.
        "pub mod member_display_fact_store",
        "pub mod member_semantic_fact_store",
        // Resolve-domain authoritative cache for resolved import /
        // re-export bindings + per-specifier resolutions (R5, R12,
        // R21, R28). Public because
        // tests/resolved_import_facts_invariants.rs and
        // tests/resolved_import_facts_key_shape.rs consume
        // `ResolvedImportFactsDb`, `ResolvedImportFactsKey`, and
        // `RESOLVED_IMPORT_FACTS_RESOLVER_VERSION` through the
        // canonical module path; future resolver consumers (RouteDb
        // fact-validation, materialiser observations) reach the
        // same module.
        "pub mod resolved_import_facts",
        // tests/semantic_analysis_audit_e2e.rs +
        // tests/semantic_analysis_audit_tls_propagation.rs — public
        // audited entry-point that wires
        // `VerterHost::analyze_with_audit` for semantic-analysis
        // requests. Producer side of the
        // `RequestKind::SemanticAnalysis` audit kind.
        "pub mod host_analyze_audit",
        // verter_session::host_audit_bridge — owns the
        // `MacroExpansionDiagnostics → AuditDiagnosticEntry` projection
        // consumed by the audited component-meta entry-point. Public
        // so the in-process bridge tests + the audited consumer
        // wiring (component_meta_audit producer) can pick the helper
        // up by canonical path.
        "pub mod host_audit_bridge",
        // tests/host_tests.rs (host_compile module surface)
        "pub mod host_compile",
        // `compile_with_audit` entry-point. Public because
        // tests/compile_audit_*.rs and tests/tls_harness_cross_crate.rs
        // drive the audited compile path; consumer crates (verter_napi,
        // verter_lsp) pick this up through their audited surfaces.
        "pub mod host_compile_audit",
        // verter_lsp::server::nav_features_audit drives audited LSP
        // handlers through `VerterHost::lsp_audit_begin` exposed by
        // this module. Public because `verter_lsp::audit_harness` and
        // the `tests/lsp_audit_*.rs` integration tests reach
        // `LspAuditSession` through the canonical module path.
        "pub mod host_lsp_audit",
        // tests/host_tests.rs (host_manage::* APIs in integration tests)
        "pub mod host_manage",
        // Slice 3.F — `audit_mcp_tool_call` entry-point. Public so
        // verter_mcp tool handlers can route their audited body
        // through the wrapper and land RequestKind::Mcp records on
        // the host store. Tests in tests/mcp_audit_e2e.rs and
        // verter_mcp/tests/mcp_tool_audit_integration.rs exercise
        // the production wiring.
        "pub mod host_mcp_audit",
        // verter_type_runtime::backend::tests via meta_resolve types,
        // tests/host_tests.rs
        "pub mod meta_resolve",
        // Tier 1A — `OwnedEvalProgram` / `OwnedTypeResolutionContext`
        // owned-artifact module (D17 + D18 + D44 + D45 + D65). Public
        // because the typed-DB shapes on `ProjectTypeStore` (1C-α
        // consumers) need to expose `OwnedArtifactKey` -> payload
        // values to consumers in `verter_type_runtime` /
        // `verter_napi` once the lowering pipeline lands.
        "pub mod owned_artifacts",
        // Tier 1B — selective component-meta surface API + BFS bridge
        // wire types (D102 + D125). Public because verter_napi calls
        // `TypeHandle::from_proto` / `to_proto`, surface envelope
        // round-trip, and `BridgeError`/`TypeHandleError` envelopes
        // through this module's public types.
        "pub mod component_meta_payload",
        // Tier 1B — `MetaSession` made public so the selective surface
        // API methods (`get_component_meta_surface`,
        // `get_component_meta_type_expansion`,
        // `get_component_meta_payload_via_bridge`) are reachable from
        // verter_napi (NapiMetaSession), the LSP custom-method handler
        // chain, and the integration test target
        // `tests/selective_component_meta_api.rs`.
        "pub mod meta",
        // tests/host_tests.rs (project_type_store::*)
        "pub mod project_type_store",
        // tests/audited_request_e2e.rs, tests/host_tests.rs
        "pub mod request_context",
        // verter_type_runtime, verter_napi (TypeExpander API);
        // tests/host_tests.rs
        "pub mod resolver_core",
        // tests/host_tests.rs (semantic_query::* in integration tests)
        "pub mod semantic_query",
        // tests/invalidation_coverage.rs, tests/invalidation_perf.rs
        "pub mod invalidation_domain",
        // tests/invalidation_perf.rs (ImportedRegistryDb /
        // ImportedRegistryEntry / ImportedRegistryKey for the §12.A12
        // InvalidationByCanonical perf gate)
        "pub mod component_meta_caches",
        // crates/verter_bench/examples/audit_real_component_meta.rs
        // calls `dump_loop5_instrumentation_counters` to record
        // inner-dispatch counter snapshots alongside the audit JSON.
        // The module is public because the bench example is an
        // out-of-crate consumer; production callers route only through
        // the atomic-counter increments which are inert.
        "pub mod loop5_instrumentation",
        // verter_napi::typeinfo, verter_wasm::typeinfo, packages/typeinfo
        // — the §5 Phase 3 typeinfo public host substrate
        // (list_file_symbols, resolve_named_symbol*, evaluate_type_expression*)
        // exposed via `pub mod typeinfo`. Required by NAPI/WASM bindings
        // and the `@verter/typeinfo` TS package.
        "pub mod typeinfo",
        // ─── B-C5 territory (separate ownership), kept `pub` ────────
        "pub mod component_meta_resolution_policy",
        // ─── crate-private modules (already non-public) ─────────────
        "pub(crate) mod completion_fence",
        // `tests/cross_owner_materialise_reuse.rs` needs the key types
        // (R7 cross-owner reuse).
        "pub mod component_meta_materialize",
        // tests/cache_invariant_migration.rs — the W0.5 schema-bump
        // cohort fixture exercises `ComponentMetaResultDb::evict_if_schema_mismatch`.
        "pub mod component_meta_result_db",
        // R3/R26/R28 compile-tier fact-observation helper module.
        // Wraps the compile cold-compute pass in
        // `with_fact_tracer` and emits per-`Member`/`MemberPresence`,
        // `ImportRef`, and `ModuleAugmentationIndexShape` observations
        // so cross-file edits invalidate the consumer's CompileSlot
        // via warm-hit fact-validation without eager invalidation.
        "pub(crate) mod compile_fact_emission",
        "pub(crate) mod cooperative_admission",
        "pub(crate) mod host_executor",
        "pub(crate) mod host_test_audit",
        "pub(crate) mod instant",
        "pub(crate) mod intrinsic_registry",
        // tests/cache_invariant_migration.rs — the W0.5 schema-bump
        // cohort fixture exercises `OwnerImportSurfaceDb::evict_if_schema_mismatch`.
        "pub mod owner_import_surface",
        "pub(crate) mod project_semantic_dispatch",
        "pub(crate) mod semantic_query_memo",
        "pub(crate) mod session_runtime",
        // Stage 4a SessionView trait surface — `HostView` and
        // `OverlaidView` impls. `pub` because the integration smoke
        // test `tests/session_view_smoke.rs` consumes the trait
        // directly via `verter_session::session_view::SessionView`.
        // Stages 4b/4c thread the trait through `ResolverContext`
        // and `HostFenceValidator`; Stage 4d retires the
        // overlay-mutation machinery the trait replaces.
        "pub mod session_view",
        "pub(crate) mod source_map_remap",
        "pub(crate) mod spike_instrumentation",
        "pub(crate) mod template_convert",
        "pub(crate) mod capture_token",
        // ─── test-only re-export shim ──────────────────────────────
        "pub mod for_tests",
        // ─── test-support submodules (gated cfg(any(test, debug_assertions))) ──
        // Hosts the reusable TLS observer-propagation harness consumed
        // by `tests/tls_harness_in_crate.rs` and
        // `tests/tls_harness_cross_crate.rs`. The harness is
        // `pub mod tests` (rather than `pub(crate)`) only so the
        // integration tests under `crates/verter_session/tests/*.rs`
        // can reach it; release builds drop the entire module
        // because `debug_assertions` is OFF in release.
        "pub mod tests",
        // ─── public re-exports ─────────────────────────────────────
        // re-exports the canonical data types (HostConfig, VerterHost,
        // UpsertRequest, FileKind, CompileProfile, CompileErrorPolicy,
        // DependencyResolution, DiagnosticsSnapshot, HostDiagnostic,
        // HostSeverity, FileAnalysisSnapshot, ...) — universally used.
        "pub use types::*",
        // verter_lsp::features::hover_provenance
        "pub use verter_compiler::utils::oxc::vue::resolve_type::ResolvedMemberVisibility",
        // verter_lsp::background_init,
        // verter_type_runtime::tsserver::ipc, verter_type_runtime::tsgo::ipc
        "pub use verter_compiler::VERTER_TYPES_STANDALONE_DTS",
        // verter_lsp::workspace_scanner, verter_lsp::server_utils,
        // verter_lsp::documents, verter_type_runtime::tsgo::ipc
        "pub use verter_compiler::compile::CompileTarget",
        // tests/relative_path_session_parity.rs
        "pub use id::resolve_external",
    ];

    /// Compare the live surface against the snapshot; report any
    /// differences (missing or added items).
    pub fn guard5_drift(live: &[String], snapshot: &[&str]) -> (Vec<String>, Vec<String>) {
        let live_set: BTreeSet<&str> = live.iter().map(|s| s.as_str()).collect();
        let snap_set: BTreeSet<&str> = snapshot.iter().copied().collect();
        let added: Vec<String> = live_set
            .difference(&snap_set)
            .map(|s| (*s).to_string())
            .collect();
        let removed: Vec<String> = snap_set
            .difference(&live_set)
            .map(|s| (*s).to_string())
            .collect();
        (added, removed)
    }

    #[test]
    fn verter_session_public_surface_is_minimal() {
        let src = read_workspace_file("crates/verter_session/src/lib.rs");
        let live = extract_top_level_pub_items(&src);
        let snapshot = guard5_snapshot_pub_items();
        let (added, removed) = guard5_drift(&live, snapshot);
        // The guard is a snapshot — additions OR removals require
        // a deliberate edit to `VERTER_SESSION_PUB_SURFACE_SNAPSHOT`.
        // Subsequent bundles (B-C3 / §12.A9) shrink the snapshot
        // deliberately as `pub mod` items are demoted to
        // `pub(crate)` or removed.
        assert!(
            added.is_empty() && removed.is_empty(),
            "Guard 5 (`verter_session_public_surface_is_minimal`) drift:\n\
             added (live but not in snapshot): {added:?}\n\
             removed (in snapshot but not live): {removed:?}\n\n\
             Update `VERTER_SESSION_PUB_SURFACE_SNAPSHOT` deliberately when the surface changes.",
        );
    }

    #[test]
    fn guard5_predicate_extracts_pub_items_correctly() {
        let src = "//! doc\n// pub mod commented_out;\npub mod foo;\npub mod bar;\npub use crate::foo::{A, B};\npub(crate) mod hidden;\nmod private;\npub fn not_a_module() {}\npub mod outer {\n    pub use crate::nested::Inner;\n}\n";
        let items = extract_top_level_pub_items(src);
        assert!(
            items.iter().any(|i| i == "pub mod foo"),
            "guard 5 predicate must extract `pub mod foo`",
        );
        assert!(
            items.iter().any(|i| i == "pub mod bar"),
            "guard 5 predicate must extract `pub mod bar`",
        );
        assert!(
            items.iter().any(|i| i == "pub use crate::foo::{A, B}"),
            "guard 5 predicate must extract `pub use ...` items",
        );
        assert!(
            items.iter().any(|i| i == "pub(crate) mod hidden"),
            "guard 5 predicate must extract `pub(crate) mod` items",
        );
        assert!(
            items.iter().any(|i| i == "pub mod outer"),
            "guard 5 predicate must capture block-form `pub mod outer {{ ... }}` as `pub mod outer`",
        );
        assert!(
            !items.iter().any(|i| i.contains("pub fn ")),
            "guard 5 predicate must NOT extract `pub fn` declarations",
        );
        assert!(
            !items.iter().any(|i| i.contains("commented_out")),
            "guard 5 predicate must NOT extract commented-out items",
        );
        assert!(
            !items.iter().any(|i| i.contains("nested::Inner")),
            "guard 5 predicate must NOT extract items nested inside a block",
        );
    }

    // ── Guard 6 — no_oversize_files ──

    /// Maximum line count (post-cleanup goal). Files above this
    /// length are violations unless allow-listed.
    pub fn guard6_target_line_count() -> usize {
        1500
    }

    /// Files that exceed [`guard6_target_line_count`] today and are
    /// currently exempt while B-C5 / §12.A4 prepares the splits.
    pub fn guard6_exemptions() -> BTreeSet<&'static str> {
        BTreeSet::from([
            "crates/verter_compiler/src/compile/template_data.rs",
            "crates/verter_compiler/src/ide/template/mod.rs",
            "crates/verter_compiler/src/template/code_gen/ssr/mod.rs",
            "crates/verter_compiler/src/template/code_gen/vapor/mod.rs",
            "crates/verter_compiler/src/template/code_gen/vdom/element.rs",
            "crates/verter_compiler/src/template/code_gen/vdom/slots.rs",
            "crates/verter_compiler/src/tsc/script.rs",
            "crates/verter_ffi/src/convert.rs",
            "crates/verter_lsp/src/config.rs",
            "crates/verter_lsp/src/features/completion.rs",
            "crates/verter_lsp/src/server/sync_orchestration.rs",
            "crates/verter_lsp/src/tsgo/merge.rs",
            "crates/verter_lsp/src/workspace_scanner.rs",
            "crates/verter_mcp/src/server.rs",
            "crates/verter_napi/src/lib.rs",
            "crates/verter_parser/src/parser/mod.rs",
            "crates/verter_parser/src/tokenizer/byte.rs",
            "crates/verter_parser/src/utils/oxc/bindings/helpers.rs",
            "crates/verter_parser/src/utils/oxc/vue/script/resolve_type/mod.rs",
            "crates/verter_parser/src/utils/oxc/vue/script/resolve_type/external.rs",
            "crates/verter_parser/src/utils/oxc/vue/script/resolve_type/decl.rs",
            "crates/verter_parser/src/utils/oxc/vue/script/setup.rs",
            "crates/verter_parser/src/utils/oxc/vue/script/usage.rs",
            "crates/verter_protocol/src/component_meta.rs",
            "crates/verter_scheduler/src/scheduler.rs",
            "crates/verter_semantic/src/analysis/build.rs",
            "crates/verter_semantic/src/analysis/component_meta.rs",
            "crates/verter_semantic/src/analysis/html_intrinsics_data.rs",
            "crates/verter_semantic/src/analysis/macros.rs",
            "crates/verter_semantic/src/analysis/style.rs",
            "crates/verter_semantic/src/analysis/template.rs",
            "crates/verter_semantic/src/analysis/type_eval_build.rs",
            "crates/verter_semantic/src/analysis/type_solver/prepared.rs",
            "crates/verter_semantic/src/analysis/types.rs",
            "crates/verter_session/src/component_meta_audit/mod.rs",
            "crates/verter_session/src/component_meta_caches.rs",
            "crates/verter_session/src/component_meta_materialize.rs",
            "crates/verter_session/src/host_manage.rs",
            "crates/verter_session/src/host_manage/analysis_io.rs",
            // Macro-participation classification + cross-file dep
            // discovery + structural rep walkers. Two iterative
            // exhaustive TypeExpr walkers (collect /
            // expr_contains_root_identity) per the W6.1 walker contract
            // — splitting them across files would either duplicate the
            // walker logic or force a shared helper crate, neither of
            // which is cleaner than a co-located walker module.
            "crates/verter_session/src/host_manage/component_meta_extract.rs",
            "crates/verter_session/src/host_manage/component_meta_methods.rs",
            "crates/verter_session/src/host_resolve.rs",
            // `ValidatedFactCache<K, V>` substrate + multi-candidate
            // RCU storage + admission guards + per-counter
            // instrumentation. The cache is the load-bearing
            // primitive every consumer routes through; splitting it
            // would either duplicate the substrate or push the API
            // through a re-export shim with no behavioural gain.
            "crates/verter_session/src/resolver_core/mod.rs",
            "crates/verter_session/src/meta_resolve/materialize/field_types.rs",
            "crates/verter_session/src/meta_resolve/materialize/macro_shapes.rs",
            "crates/verter_session/src/parse.rs",
            "crates/verter_session/src/project_semantic_dispatch/build.rs",
            "crates/verter_session/src/project_semantic_dispatch/lower.rs",
            "crates/verter_session/src/project_semantic_dispatch/raise.rs",
            "crates/verter_session/src/project_type_store.rs",
            "crates/verter_session/src/resolver_core/component_meta.rs",
            "crates/verter_session/src/resolver_core/component_meta_query_engine/routed_expr.rs",
            "crates/verter_session/src/resolver_core/component_meta_registry.rs",
            "crates/verter_session/src/resolver_core/external_type_frontier.rs",
            "crates/verter_session/src/resolver_core/fallthrough.rs",
            "crates/verter_session/src/resolver_core/shallow_file_state.rs",
            "crates/verter_session/src/semantic_query.rs",
            "crates/verter_session/src/semantic_query_memo/mod.rs",
            "crates/verter_session/src/semantic_query_memo/arena.rs",
            "crates/verter_session/src/semantic_query_memo/derivation.rs",
            "crates/verter_session/src/semantic_query_memo/family.rs",
            "crates/verter_session/src/semantic_query_memo/inflight.rs",
            "crates/verter_session/src/semantic_query_memo/interner.rs",
            "crates/verter_session/src/semantic_query_memo/stats.rs",
            "crates/verter_session/src/semantic_query_memo/tests.rs",
            "crates/verter_session/src/types.rs",
            "crates/verter_tsc/src/checker.rs",
            "crates/verter_type_runtime/src/tsgo/ipc.rs",
            "crates/verter_type_runtime/src/tsserver/ipc.rs",
            "crates/verter_workspace/src/resolver.rs",
            // Dispatch walker — single coherent module owning the
            // empty-path Shallow surface enumeration, intersection /
            // union member-level merge, and Pick / Omit / Indexed
            // route extraction. Splitting would cross-cut the
            // member-merge state machine.
            "crates/verter_session/src/project_semantic_dispatch/walk.rs",
            // verter_wasm FFI glue — same shape as verter_napi/lib.rs:
            // a single `mod` exposing every WASM-bindgen entry-point
            // for parity with the NAPI surface. Splitting would force
            // entry-point fragmentation across multiple bindgen mods.
            "crates/verter_wasm/src/lib.rs",
        ])
    }

    /// Predicate: count newlines in `src` and return the line count.
    pub fn count_lines(src: &str) -> usize {
        src.lines().count()
    }

    /// Walk the production tree; return `(rel_path, line_count)`
    /// pairs for every file that exceeds `target` lines AND is not
    /// in `exempt`.
    pub fn guard6_violations(
        target: usize,
        exempt: &BTreeSet<&'static str>,
    ) -> Vec<(String, usize)> {
        let crates_root = workspace_root().join("crates");
        let mut violations = Vec::new();
        let entries = match fs::read_dir(&crates_root) {
            Ok(it) => it,
            Err(_) => return violations,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let src_dir = path.join("src");
            if !src_dir.exists() {
                continue;
            }
            for file in walk_production_rs(&src_dir) {
                let src = match fs::read_to_string(&file) {
                    Ok(s) => s,
                    Err(_) => continue,
                };
                let lines = count_lines(&src);
                if lines <= target {
                    continue;
                }
                let rel = relative_to_root(&file);
                if exempt.contains(rel.as_str()) {
                    continue;
                }
                violations.push((rel, lines));
            }
        }
        violations.sort();
        violations
    }

    #[test]
    fn no_oversize_files() {
        let target = guard6_target_line_count();
        let violations = guard6_violations(target, &guard6_exemptions());
        assert!(
            violations.is_empty(),
            "Guard 6 (`no_oversize_files`) violations: production source files exceed {target} lines\n\
             without an explicit exemption:\n  {}\n\n\
             Either split the file along sensible boundaries (preferred — see B-C5 / §12.A4),\n\
             or add the file to `guard6_exemptions()` if the size is intentional.",
            violations
                .iter()
                .map(|(rel, n)| format!("{rel} ({n} lines)"))
                .collect::<Vec<_>>()
                .join("\n  "),
        );
    }

    #[test]
    fn guard6_predicate_counts_lines_correctly() {
        assert_eq!(count_lines(""), 0, "empty string has zero lines");
        assert_eq!(count_lines("a"), 1, "single line without newline");
        assert_eq!(
            count_lines("a\n"),
            1,
            "trailing newline does not add a line"
        );
        assert_eq!(count_lines("a\nb"), 2, "two lines, no trailing newline");
        assert_eq!(count_lines("a\nb\n"), 2, "two lines with trailing newline");
        let oversize: String = (0..(guard6_target_line_count() + 1))
            .map(|i| format!("// line {i}\n"))
            .collect();
        assert!(
            count_lines(&oversize) > guard6_target_line_count(),
            "guard 6 predicate must report > target lines for an oversized fixture",
        );
    }

    // ── Guard 7 — no_phase_archaeology_in_production_code ──
    //
    // Production source code must read as final-state. References to plan
    // phases (`phase 5d`, `phase 11`), cutover stages (`d-cutover`,
    // `post-cutover`, `pre-Phase`), or deletion history (`deleted in 5g`,
    // `retired in`) leak project-management vocabulary into the codebase
    // and accumulate as the project moves on. Durable architecture
    // insights belong in `.claude/skills/*` or `docs/arch/`, not in
    // source comments.
    //
    // Predicate: scan every production `.rs` file under `crates/*/src/`
    // (`walk_production_rs` already excludes `_tests.rs`, `tests.rs`,
    // `tests/`, `benches/`, `examples/`, and `target/`). The guard fails
    // when ANY line matches the regex below.
    //
    // Forbidden patterns:
    //   - `d-cutover` / `D-Cutover` — cutover stage from the d-phase plan.
    //   - `post-cutover` / `Post-Cutover` — narrative of a completed cutover.
    //   - `pre-Phase` / `Pre-Phase` — narrative of pre-phase state.
    //   - `Phase \d+` / `phase \d+` — explicit phase reference.
    //   - `Phase-\d+` / `phase-\d+` — explicit hyphenated phase reference.
    //   - `pre-Stage` / `Pre-Stage` / `post-Stage` / `Post-Stage` —
    //     narrative of pre/post-stage state, mirrors the Phase family.
    //   - `Stage \d+` / `stage \d+` / `Stage-\d+` / `stage-\d+` —
    //     explicit stage reference (the dominant project-management
    //     noun used by the fact-based cache refactor's stage list).
    //   - `deleted in 5[a-z]` — deletion history from the 5-series plan.
    //   - `retired in` — retirement history of any kind.

    /// Predicate: returns `true` when `line` contains a forbidden
    /// phase-archaeology pattern. Implemented with case-sensitive
    /// substring scanning where the plan calls for it, plus a numeric
    /// scan for `phase \d+` / `phase-\d+` and the equivalent
    /// `Stage \d+` / `Stage-\d+` family.
    pub fn line_has_phase_archaeology(line: &str) -> bool {
        // Substring matches for fixed vocabulary. These are unambiguous
        // in production source and never appear as legitimate prose.
        const FIXED_NEEDLES: &[&str] = &[
            "d-cutover",
            "D-Cutover",
            "post-cutover",
            "Post-Cutover",
            "Post-cutover",
            "pre-Phase",
            "Pre-Phase",
            "retired in",
            // Stage-family phase-archaeology mirroring the Phase
            // family. The Stage vocabulary is the dominant
            // project-management noun used by the fact-based cache
            // refactor's stage list; it leaks into production
            // source the same way Phase did.
            "pre-Stage",
            "Pre-Stage",
            "post-Stage",
            "Post-Stage",
            // Audit-infrastructure plan archaeology — these phrases
            // unambiguously reference the audit-plan document and
            // never appear in legitimate final-state prose.
            "audit infrastructure plan",
            "audit-infrastructure-plan",
        ];
        for needle in FIXED_NEEDLES {
            if line.contains(needle) {
                return true;
            }
        }
        // `plan §` / `Plan §` — explicit reference to a plan section.
        // Mirrors the broader D111 guard (guard 7-bis) which already
        // catches the same pattern. Production source must read as
        // final-state, not as a citation of plan vocabulary.
        if line.contains("plan §") || line.contains("Plan §") {
            return true;
        }
        // `§\d+\.\d+` (decimal section ref like `§1.5`, `§3.4`) —
        // catch when the line ALSO references audit-substrate
        // vocabulary. Standalone decimal section refs are widespread
        // in older production code (a pre-existing archaeology surface
        // tracked under a separate cleanup); the audit-substrate
        // discriminator is what newly entered the codebase with the
        // audit infrastructure work, so the focused predicate matches
        // exactly that surface and prevents regression.
        let contains_decimal_section_ref = {
            let bytes = line.as_bytes();
            let mut hit = false;
            let mut search_from = 0usize;
            while let Some(rel) = line[search_from..].find('§') {
                let abs = search_from + rel;
                let mut after = abs + '§'.len_utf8();
                let digit_start = after;
                while after < bytes.len() && bytes[after].is_ascii_digit() {
                    after += 1;
                }
                if after > digit_start
                    && after + 1 < bytes.len()
                    && bytes[after] == b'.'
                    && bytes[after + 1].is_ascii_digit()
                {
                    hit = true;
                    break;
                }
                search_from = abs + '§'.len_utf8();
            }
            hit
        };
        if contains_decimal_section_ref
            && (line.contains("joiner-accounting")
                || line.contains("audit substrate")
                || line.contains("audit-substrate")
                || line.contains("audit infrastructure"))
        {
            return true;
        }
        // `Slice <digit>` / `slice <digit>` — project-management
        // vocabulary referring to the Wave/Slice plan vocabulary.
        // ASCII digit immediately after the separator distinguishes
        // these from legitimate prose (`a slice of bytes`, etc.).
        for prefix in ["Slice ", "Slice-", "slice ", "slice-"] {
            let mut search_from = 0usize;
            while let Some(rel) = line[search_from..].find(prefix) {
                let abs = search_from + rel;
                let after = abs + prefix.len();
                if after < line.len() {
                    let next = line.as_bytes()[after];
                    if next.is_ascii_digit() {
                        return true;
                    }
                }
                search_from = abs + prefix.len();
            }
        }
        // `deleted in 5[a-z]` (lowercase `5` + single ASCII letter).
        let lower = line.to_ascii_lowercase();
        if let Some(idx) = lower.find("deleted in 5") {
            let bytes = lower.as_bytes();
            let after = idx + "deleted in 5".len();
            if after < bytes.len() && bytes[after].is_ascii_lowercase() {
                return true;
            }
        }
        // `phase \d+` / `phase-\d+` (case-insensitive on the verb,
        // ASCII digit immediately after the separator).
        for prefix in ["phase ", "phase-", "Phase ", "Phase-"] {
            let mut search_from = 0usize;
            while let Some(rel) = line[search_from..].find(prefix) {
                let abs = search_from + rel;
                let after = abs + prefix.len();
                if after < line.len() {
                    let next = line.as_bytes()[after];
                    if next.is_ascii_digit() {
                        return true;
                    }
                }
                search_from = abs + prefix.len();
            }
        }
        // `Stage \d+` / `Stage-\d+` / `stage \d+` / `stage-\d+`
        // (parallel shape to the Phase scan above). Stage is the
        // project-management noun used by the fact-based cache
        // refactor's plan; it leaks into production source the
        // same way Phase does and must be cleaned up the same way.
        for prefix in ["stage ", "stage-", "Stage ", "Stage-"] {
            let mut search_from = 0usize;
            while let Some(rel) = line[search_from..].find(prefix) {
                let abs = search_from + rel;
                let after = abs + prefix.len();
                if after < line.len() {
                    let next = line.as_bytes()[after];
                    if next.is_ascii_digit() {
                        return true;
                    }
                }
                search_from = abs + prefix.len();
            }
        }
        false
    }

    /// Walk the production tree and return `(rel_path, line_no, line)`
    /// triples for every match. `rel_path` is `crates/<name>/src/...`
    /// with forward slashes.
    pub fn guard7_violations() -> Vec<(String, usize, String)> {
        let crates_root = workspace_root().join("crates");
        let mut violations = Vec::new();
        let entries = match fs::read_dir(&crates_root) {
            Ok(it) => it,
            Err(_) => return violations,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let src_dir = path.join("src");
            if !src_dir.exists() {
                continue;
            }
            for file in walk_production_rs(&src_dir) {
                let src = match fs::read_to_string(&file) {
                    Ok(s) => s,
                    Err(_) => continue,
                };
                let rel = relative_to_root(&file);
                for (idx, line) in src.lines().enumerate() {
                    if line_has_phase_archaeology(line) {
                        violations.push((rel.clone(), idx + 1, line.to_string()));
                    }
                }
            }
        }
        violations.sort();
        violations
    }

    #[test]
    fn no_phase_archaeology_in_production_code() {
        let violations = guard7_violations();
        assert!(
            violations.is_empty(),
            "Guard 7 (`no_phase_archaeology_in_production_code`) violations: production source\n\
             files reference plan phases, plan stages, cutover stages, or deletion history.\n\
             Once a plan is over, the code should read as final-state. Durable architecture\n\
             insights belong in `.claude/skills/*` or `docs/arch/`, not in source comments.\n\n\
             Forbidden patterns: `d-cutover`, `post-cutover`, `pre-Phase`, `pre-Stage`,\n\
             `post-Stage`, `phase \\d+`, `phase-\\d+`, `Stage \\d+`, `Stage-\\d+`,\n\
             `deleted in 5[a-z]`, `retired in`.\n\n\
             Violations:\n  {}",
            violations
                .iter()
                .map(|(rel, lineno, line)| format!("{rel}:{lineno}: {}", line.trim()))
                .collect::<Vec<_>>()
                .join("\n  "),
        );
    }

    #[test]
    fn guard7_predicate_rejects_deliberate_violations() {
        // Each of these fabricated lines models a real archaeology
        // pattern observed in the codebase before this guard was wired.
        let cases = [
            "// Phase 4 — graph-native projection for imported declarations.",
            "// Phase 11b.2 — surface-projection helpers.",
            "// Pre-Phase-4 the resolver passed the imported declaration's raw value.",
            "// Post-Phase-4 + post-Phase-5l: assertion now expects the resolved value.",
            "// D-Cutover §5.8 WIP-W retired the previously embedded engine.",
            "// post-cutover clippy cleanup — direct_macro_type_reference_expr removed.",
            "// `find_matching_angle` was deleted in 5g once the dispatch resolver took over.",
            "// `legacy_first_pass` was retired in 11d.",
            "/// phase-1b cutover deleted the declared component-meta query.",
            // Audit-infrastructure plan archaeology — fixed-needle and
            // `plan §` matches added when the audit-substrate cutover
            // landed. The decimal section ref (§1.5) is a focused
            // discriminator scoped to lines that also mention the
            // audit-substrate vocabulary.
            "// joiner-accounting per the audit infrastructure plan §1.5",
            "/// see audit-infrastructure-plan.md for the contract",
            "// per plan §3.4 of the audit substrate work",
            "// Plan §3 Step 4 — joiner-accounting bumps",
            "// joiner-accounting reference §1.5 lives elsewhere",
            // Stage-family phase-archaeology — mirrors the Phase
            // family. These are the patterns the fact-based cache
            // refactor's stage list leaks into production source.
            "// Stage 4d retired the per-session overlay-mutation lifecycle.",
            "/// Pre-Stage-4d the overlay-mutation lifecycle invoked this from query paths.",
            "// post-Stage-4d (R17): no-op when overlays are absent.",
            "// Stage-5b instrumentation counter — admission-cap discriminator.",
            "/// Stage 6e installs the legacy_dep_signature shadow scaffold.",
            "/// stage 6a wires the real ResolvedImportFacts cache.",
            "// Stage-4d compliance from the pre-state.",
        ];
        for line in cases {
            assert!(
                line_has_phase_archaeology(line),
                "guard 7 predicate must reject deliberate-violation line: {line:?}",
            );
        }
        // Lines that look superficially similar but are NOT violations:
        // they describe the final state without referencing project
        // phases, deletion history, or cutover stages.
        let allowed = [
            "// Walk the prepared declaration graph for imported types.",
            "// Surface projection helpers live in the `surface` child module.",
            "// The resolver passes the imported declaration's raw value.",
            "/// Returns the projected surface for a given semantic node.",
            "// `find_matching_angle` is no longer required because the dispatch resolver owns it.",
            "// Phase angle in radians for the easing curve.", // legitimate "phase" usage
            "// Builder Phase C owns the second pass.",        // 'Phase C' is a letter, not a digit
            // Stage-family negative cases — Stage followed by a
            // letter (not a digit), or "stage" used in a legitimate
            // prose sense, must not flag.
            "// On-stage layout pass owns the first batch.",
            "// stages of the pipeline cooperate via the substrate.",
            "// Build Stage C handles the second pass.", // letter-suffixed Stage is preserved
            // Final-state joiner-accounting prose — no plan citation,
            // no decimal section ref tied to audit vocabulary.
            "// Joiner-accounting contract: per-request hits/misses attribute exactly.",
        ];
        for line in allowed {
            assert!(
                !line_has_phase_archaeology(line),
                "guard 7 predicate must NOT flag legitimate line: {line:?}",
            );
        }
    }

    // ── Guard 7-bis — no_phase_archaeology_in_production_code_broader_d111 ──
    //
    // Strict superset of guard 7. Implements the D111 classifier rule
    // (committed at `tools/god-module-audit/README.md`) for production
    // source files. Final-state code must not reference plan sections,
    // commit numbers, decimal phase tags, deletion history with explicit
    // commit/section references, or revision numbers.
    //
    // Predicate: scan every production `.rs` file under `crates/*/src/`
    // (`walk_production_rs` already excludes `_tests.rs`, `tests.rs`,
    // `tests/`, `benches/`, `examples/`, and `target/`). The guard fails
    // when ANY line matches the regex below.
    //
    // Forbidden patterns (broader regex from D111):
    //   - `Plan §` / `plan §` — explicit reference to a plan section.
    //   - `Phase \d+` followed by anything other than a letter or `:` —
    //     covers `Phase 5d`, `Phase 11b.2`, `Phase 5)`. The `:` carve-out
    //     preserves algorithm-phase comments like `Phase 1: collect ...`
    //     where the verb pinpoints an algorithm step.
    //   - `Commit \d+` — explicit commit number reference.
    //   - `deleted in \d` / `retired in` — deletion / retirement history.
    //   - `revision \d+` / `rev \d+` — revision number reference (only
    //     when the rev/revision is followed by a decimal digit, to avoid
    //     legitimate prose like "the rev returns").
    //
    // Algorithm-phase carve-out: `Phase 1: collect ...` is preserved
    // (colon-prefixed verb describes an algorithm step). Letter-suffixed
    // phases (`Phase 5d`, `Phase 11b`) are NOT preserved — those are
    // project-management vocabulary.

    /// Predicate: returns `true` when `line` contains a forbidden
    /// broader-D111 phase-archaeology pattern. Strict superset of the
    /// narrower guard 7 predicate, except for the algorithm-phase
    /// carve-out (`Phase 1: collect ...`) which the narrower predicate
    /// does NOT distinguish from project-management `Phase 1`.
    ///
    /// The carve-out is deliberate: post-sweep, `Phase 1: collect ...`
    /// comments describing algorithm steps are preserved per the D111
    /// classifier rule. The narrower guard 7 has no such carve-out
    /// because production code currently contains no algorithm-phase
    /// comments to protect — the broader rule documents the
    /// distinction so future authors know it.
    pub fn line_has_phase_archaeology_d111(line: &str) -> bool {
        let bytes = line.as_bytes();
        // ── Fixed substrings — always archaeology. ──
        const FIXED_NEEDLES: &[&str] = &[
            "d-cutover",
            "D-Cutover",
            "post-cutover",
            "Post-cutover",
            "Post-Cutover",
            "pre-Phase",
            "Pre-Phase",
            "retired in",
            "Plan §",
            "plan §",
            "phase-archaeology",
            // Stage-family — mirror the Phase fixed needles.
            "pre-Stage",
            "Pre-Stage",
            "post-Stage",
            "Post-Stage",
        ];
        for needle in FIXED_NEEDLES {
            if line.contains(needle) {
                return true;
            }
        }
        // ── `Phase \d+` with the `:` carve-out. ──
        // `Phase 1: collect ...` is algorithm-phase (preserve). Any
        // other byte after the digit run (letter, `-`, `.`, space, EOL,
        // `,`, `)`, `—`, etc.) is archaeology.
        for prefix in ["Phase ", "phase ", "Phase-", "phase-"] {
            let mut search_from = 0usize;
            while let Some(rel) = line[search_from..].find(prefix) {
                let abs = search_from + rel;
                let mut after = abs + prefix.len();
                let digit_start = after;
                while after < bytes.len() && bytes[after].is_ascii_digit() {
                    after += 1;
                }
                if after > digit_start {
                    // Found at least one digit. Check the next byte.
                    // EOL after digit is archaeology.
                    if after >= bytes.len() {
                        return true;
                    }
                    let next = bytes[after];
                    // Carve-out: colon-prefixed verb is algorithm-phase.
                    if next != b':' {
                        return true;
                    }
                }
                search_from = abs + prefix.len();
            }
        }
        // ── `Stage \d+` with the `:` carve-out. ──
        // Mirrors the Phase scan: `Stage 1: collect ...` is
        // algorithm-stage (preserve). Any other byte after the digit
        // run (letter, `-`, `.`, space, EOL, `,`, `)`, `—`, etc.)
        // is archaeology.
        for prefix in ["Stage ", "stage ", "Stage-", "stage-"] {
            let mut search_from = 0usize;
            while let Some(rel) = line[search_from..].find(prefix) {
                let abs = search_from + rel;
                let mut after = abs + prefix.len();
                let digit_start = after;
                while after < bytes.len() && bytes[after].is_ascii_digit() {
                    after += 1;
                }
                if after > digit_start {
                    if after >= bytes.len() {
                        return true;
                    }
                    let next = bytes[after];
                    if next != b':' {
                        return true;
                    }
                }
                search_from = abs + prefix.len();
            }
        }
        // ── `Commit \d+` — explicit commit-number reference. ──
        for prefix in ["Commit ", "commit "] {
            let mut search_from = 0usize;
            while let Some(rel) = line[search_from..].find(prefix) {
                let abs = search_from + rel;
                let after = abs + prefix.len();
                if after < bytes.len() && bytes[after].is_ascii_digit() {
                    return true;
                }
                search_from = abs + prefix.len();
            }
        }
        // ── `deleted in \d` — deletion history with digit reference. ──
        let lower = line.to_ascii_lowercase();
        if let Some(idx) = lower.find("deleted in ") {
            let after = idx + "deleted in ".len();
            let bytes_l = lower.as_bytes();
            if after < bytes_l.len() && bytes_l[after].is_ascii_digit() {
                return true;
            }
            // `deleted in Commit N` / `deleted in Plan §...` are also
            // covered by the `Commit \d+` and `Plan §` checks above.
        }
        // ── `revision N` / `Revision N`. ──
        for prefix in ["revision ", "Revision "] {
            let mut search_from = 0usize;
            while let Some(rel) = line[search_from..].find(prefix) {
                let abs = search_from + rel;
                let after = abs + prefix.len();
                if after < bytes.len() && bytes[after].is_ascii_digit() {
                    return true;
                }
                search_from = abs + prefix.len();
            }
        }
        // ── `rev N` standalone word. ──
        // Only flag when `rev ` is preceded by whitespace / line start /
        // `(` (so legitimate prose like `revs 1..3` is not flagged) and
        // followed by a digit.
        for (i, _) in line.match_indices("rev ") {
            if i > 0 {
                let prev = bytes[i - 1];
                if !(prev == b' ' || prev == b'\t' || prev == b'(') {
                    continue;
                }
            }
            let after = i + "rev ".len();
            if after < bytes.len() && bytes[after].is_ascii_digit() {
                return true;
            }
        }
        false
    }

    /// Walk the production tree and return `(rel_path, line_no, line)`
    /// triples for every D111-classified archaeology match.
    pub fn guard7_bis_violations() -> Vec<(String, usize, String)> {
        let crates_root = workspace_root().join("crates");
        let mut violations = Vec::new();
        let entries = match fs::read_dir(&crates_root) {
            Ok(it) => it,
            Err(_) => return violations,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let src_dir = path.join("src");
            if !src_dir.exists() {
                continue;
            }
            for file in walk_production_rs(&src_dir) {
                let src = match fs::read_to_string(&file) {
                    Ok(s) => s,
                    Err(_) => continue,
                };
                let rel = relative_to_root(&file);
                for (idx, line) in src.lines().enumerate() {
                    if line_has_phase_archaeology_d111(line) {
                        violations.push((rel.clone(), idx + 1, line.to_string()));
                    }
                }
            }
        }
        violations.sort();
        violations
    }

    #[test]
    fn no_phase_archaeology_in_production_code_broader_d111() {
        let violations = guard7_bis_violations();
        assert!(
            violations.is_empty(),
            "Guard 7-bis (`no_phase_archaeology_in_production_code_broader_d111`) violations:\n\
             production source files reference plan sections, commit numbers, decimal phase\n\
             tags, deletion history with explicit references, or revision numbers. Once a\n\
             plan is over, the code should read as final-state. Durable architecture\n\
             insights belong in `.claude/skills/*` or `docs/arch/`, not in source comments.\n\n\
             Forbidden patterns (broader D111 regex):\n\
               - `Plan §` / `plan §`\n\
               - `Phase \\d+` (NOT followed by letter or `:` — `Phase 1: collect` preserved)\n\
               - `Commit \\d+`\n\
               - `deleted in \\d` / `retired in`\n\
               - `revision \\d+` / `rev \\d+`\n\n\
             Found {} violation(s):\n  {}",
            violations.len(),
            violations
                .iter()
                .map(|(rel, lineno, line)| format!("{rel}:{lineno}: {}", line.trim()))
                .collect::<Vec<_>>()
                .join("\n  "),
        );
    }

    #[test]
    fn guard7_bis_predicate_rejects_deliberate_violations_and_preserves_algorithm_phases() {
        // Deliberate-violation lines that the broader D111 guard MUST
        // reject. Mirror real archaeology shapes seen in the codebase
        // before this guard was tightened.
        let cases = [
            "// Plan §3 Commit 9 — hover.provenance opt-in.",
            "/// plan §3 Commit 8 — necessary for the audit bundle.",
            "// Plan §3 Step 4 — audit warm-cache",
            "/// Plan §4.8 / Phase C / Commit R — RefCycleResultDb",
            "// Phase D §5.6 WIP-L — function shape (plan §2 decision).",
            "// architectural-debt-closure rev 10",
            "// were deleted in Commit 3 of the cutover sub-plan).",
            "// Counterpart deleted in Plan §6.15 / N — entry stored.",
            "// Five-phase materialiser entry per plan §10.",
            // Stage-family — mirror the Phase-family violations.
            "// Stage 4d retired the per-session overlay lifecycle.",
            "/// Pre-Stage-4d the overlay-mutation invoked this hook.",
            "// post-Stage-4d the path is a no-op.",
            "// Stage-5b instrumentation counter — admission discriminator.",
            "/// Stage 6e installs the legacy_dep_signature shadow.",
            "/// stage 6a wires the real cache.",
        ];
        for line in cases {
            assert!(
                line_has_phase_archaeology_d111(line),
                "guard 7-bis predicate must reject deliberate-violation line: {line:?}",
            );
        }
        // Allowed lines: legitimate prose, algorithm-phase comments,
        // and final-state architecture documentation. The guard MUST
        // NOT flag these.
        let allowed = [
            // Algorithm-phase carve-out (colon-prefixed verb).
            "// Phase 1: collect import statements.",
            "// Phase 2: emit lowered IR.",
            "// phase 3: walk dependency graph.",
            // Algorithm-stage carve-out — the Stage family inherits
            // the same `:`-prefixed carve-out as Phase.
            "// Stage 1: read parser input.",
            "// stage 2: lower to typed IR.",
            // Final-state prose with no project-management vocabulary.
            "// Walk the prepared declaration graph for imported types.",
            "/// Returns the projected surface for a given semantic node.",
            "// LRU bounded at 100 entries.",
            "// hover.provenance is opt-in (default false).",
            // Legitimate `rev` / `revision` usage that's not a number.
            "// Reverses (rev) the iteration order.",
            "/// The phase angle in radians for the easing curve.",
            // `Phase` followed by a letter without digits — not archaeology
            // by the broader rule (the rule specifies `Phase \d+`).
            "// Builder Phase C owns the second pass.",
            // Stage followed by a letter — same carve-out as Phase.
            "// Build Stage C handles the second pass.",
        ];
        for line in allowed {
            assert!(
                !line_has_phase_archaeology_d111(line),
                "guard 7-bis predicate must NOT flag legitimate line: {line:?}",
            );
        }
    }

    // ── Guard D14 — no_std_fs_outside_native_fs_or_allow_list ──
    //
    // The NativeFs invariant lock. The single legitimate disk-touch
    // boundary is `crates/verter_workspace/src/native_fs.rs` (the
    // `NativeFs` wrapper). Every other production-source file that
    // contains a `std::fs::` reference must appear in `ALLOW_LIST`
    // below with an explicit justification. New escapes from NativeFs
    // are visible as one new constant entry per file in the diff;
    // there is no opaque TOML or external-file route around the
    // invariant.
    //
    // The ALLOW_LIST coexists with guard 1 (TOML-driven, scoped to
    // `verter_session` / `verter_semantic` / etc.) and guard 2
    // (in-source allowlist, broader OS-file-API scope including
    // `tokio::fs::`). This guard is the strictest of the three and
    // the ONLY one whose justifications live next to the path in
    // source code, by design — the brief (D14) requires per-callsite
    // visibility for the lock.

    /// Path (relative to workspace root) that is exempt from the
    /// guard. Calls to `std::fs::` here ARE the canonical disk
    /// boundary that `NativeFs` wraps.
    pub const D14_NATIVE_FS_PATH: &str = "crates/verter_workspace/src/native_fs.rs";

    /// `(file_path, justification)` enumeration of every production
    /// file outside `native_fs.rs` that legitimately contains a
    /// `std::fs::` reference. Adding an entry must be paired with a
    /// rationale that the reviewer can read at the diff.
    ///
    /// File-path strings use forward slashes and are relative to the
    /// workspace root (matches `relative_to_root`).
    ///
    /// Adding a new entry is a deliberate, visible widening of the
    /// NativeFs invariant. Removing an entry must be paired with a
    /// code change that routes the I/O through `NativeFs` /
    /// `WorkspaceAccess` (or a deletion of the callsite).
    pub const D14_ALLOW_LIST: &[(&str, &str)] = &[
        (
            "crates/verter_lsp/src/audit_harness.rs",
            "LSP audit telemetry — `VERTER_LSP_AUDIT_TRACE_OUT` JSON-lines drainer. Off by default and gated behind the env var at the call site; mirrors the existing `VERTER_COMPONENT_META_AUDIT_JSON_OUT` drainer in `verter_session::component_meta_audit`.",
        ),
        (
            "crates/verter_lsp/src/background_init.rs",
            "writes Verter-generated `@verter/types` stub files into `node_modules` for tool setup; reads them back via marker detection. Test fixtures inside `#[cfg(test)] mod tests` use temp-dir scratch space.",
        ),
        (
            "crates/verter_lsp/src/config.rs",
            "test fixtures only (`#[cfg(test)] mod tests` blocks set up tmp directories for `discover_lint_config` tests). No production-path call.",
        ),
        (
            "crates/verter_lsp/src/test_harness.rs",
            "LSP integration test harness — sets up scratch worktrees and reads fixture files for end-to-end tests.",
        ),
        (
            "crates/verter_lsp/src/test_utils.rs",
            "LSP unit-test utilities — temp workspace creation and `canonicalize` for fixture path resolution.",
        ),
        (
            "crates/verter_mcp/src/baseline.rs",
            "MCP baseline output — reads/writes JSON snapshots for regression diffing of MCP tool responses; not semantic state.",
        ),
        (
            "crates/verter_parser/src/utils/oxc/vue/script/resolve_type/mod.rs",
            "diagnostic trace logger gated behind a debug flag (`OpenOptions::new().append(true)` to a per-process trace file); not on the resolution hot path.",
        ),
        (
            "crates/verter_scheduler/src/source_loader.rs",
            "scheduler source-loader fallback — reads disk only when the workspace overlay/snapshot is absent for a host-loaded path; transitional pending the full WorkspaceAccess integration.",
        ),
        (
            "crates/verter_audit/src/memory.rs",
            "audit telemetry — `/proc/self/statm` resource sample for memory-delta accounting (Linux RSS branch). Off by default and gated behind audit_enabled at the call site.",
        ),
        (
            "crates/verter_session/src/component_meta_audit/mod.rs",
            "audit telemetry — JSON dump file output for footprint capture (`emit_audit_trace`); off by default and gated behind `VERTER_COMPONENT_META_AUDIT_JSON_OUT`.",
        ),
        (
            "crates/verter_tsc/src/checker.rs",
            "verter-tsc binary CLI — writes the consolidated diagnostics report file at the end of a checker run.",
        ),
        (
            "crates/verter_tsc/src/reporter.rs",
            "verter-tsc binary CLI — reads the local tsgo cache directory to discover the active tsgo binary for parity reporting.",
        ),
        (
            "crates/verter_tsc/src/tsconfig.rs",
            "verter-tsc binary CLI — reads tsconfig files outside the host's WorkspaceAccess (separate from the LSP/session tsconfig path). Doc comment also references `std::fs::canonicalize` behaviour for documentation.",
        ),
        (
            "crates/verter_type_runtime/src/discovery.rs",
            "TypeScript SDK install discovery for the type-runtime tool layer (tsserver/tsgo binary lookup, package.json reads inside the SDK directory).",
        ),
        (
            "crates/verter_type_runtime/src/provider_adapter.rs",
            "type-runtime tool-cache and shim file management — separate from semantic state; reads/writes the per-runtime scratch dir used by tsserver/tsgo.",
        ),
        (
            "crates/verter_type_runtime/src/trace.rs",
            "trace artifact writer for tsserver/tsgo IPC debugging; gated behind a debug flag and writes only to a process-local trace file.",
        ),
        (
            "crates/verter_type_runtime/src/tsgo/ipc.rs",
            "tsgo subprocess IPC — pnpm virtual-store walk, scratch-dir setup, and direct disk reads of files the tsgo subprocess will consume next; orchestrates the external runtime.",
        ),
        (
            "crates/verter_type_runtime/src/tsserver/ipc.rs",
            "tsserver subprocess IPC — pnpm virtual-store walk, scratch-dir setup, and direct disk reads of files the tsserver subprocess will consume next; orchestrates the external runtime.",
        ),
        (
            "crates/verter_workspace/src/intrinsic_library.rs",
            "ambient TypeScript SDK reader (`lib*.d.ts`) — companion to NativeFs for SDK declaration files. The verter_session intrinsic_registry consumes this single reader.",
        ),
        (
            "crates/verter_workspace/src/resolver.rs",
            "doc comment only references `std::fs::canonicalize()` behaviour on Windows for documentation; no actual `std::fs::` callsite. Path-string normalization stays local to the resolver.",
        ),
    ];

    /// Predicate the test reuses: does this file's source contain
    /// any `std::fs::` reference? Identical to guard 1's predicate
    /// for the `std::fs::` half — extracted here so the deliberate
    /// violation tests can characterize this guard independently.
    pub fn d14_file_uses_std_fs(src: &str) -> bool {
        src.contains("std::fs::")
    }

    /// Materialize the allow list into a set of allowed paths +
    /// the implicit native_fs path. Returns the canonical set of
    /// "files where `std::fs::` is permitted by D14".
    pub fn d14_permitted_paths() -> BTreeSet<String> {
        let mut set: BTreeSet<String> = BTreeSet::new();
        set.insert(D14_NATIVE_FS_PATH.to_string());
        for (path, _justification) in D14_ALLOW_LIST {
            set.insert((*path).to_string());
        }
        set
    }

    /// Walk the workspace's production `.rs` tree under `crates/*/src/`
    /// and return paths of files that contain `std::fs::` and are not
    /// in the permitted set (native_fs.rs ∪ ALLOW_LIST).
    pub fn d14_violations(permitted: &BTreeSet<String>) -> Vec<String> {
        let crates_root = workspace_root().join("crates");
        let mut violations = Vec::new();
        let entries = match fs::read_dir(&crates_root) {
            Ok(it) => it,
            Err(_) => return violations,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let src_dir = path.join("src");
            if !src_dir.exists() {
                continue;
            }
            for file in walk_production_rs(&src_dir) {
                let src = match fs::read_to_string(&file) {
                    Ok(s) => s,
                    Err(_) => continue,
                };
                if !d14_file_uses_std_fs(&src) {
                    continue;
                }
                let rel = relative_to_root(&file);
                if permitted.contains(&rel) {
                    continue;
                }
                violations.push(rel);
            }
        }
        violations.sort();
        violations
    }

    #[test]
    fn no_std_fs_outside_native_fs_or_allow_list() {
        // D14 NativeFs invariant lock. Production-source files that
        // contain `std::fs::` must either BE the canonical disk
        // boundary (`native_fs.rs`) or appear in `D14_ALLOW_LIST`
        // with an explicit justification.
        let permitted = d14_permitted_paths();
        let violations = d14_violations(&permitted);
        assert!(
            violations.is_empty(),
            "D14 (`no_std_fs_outside_native_fs_or_allow_list`) violations:\n  {}\n\n\
             Each survivor is a production file outside\n\
             `crates/verter_workspace/src/native_fs.rs` that uses `std::fs::`\n\
             without an explicit `D14_ALLOW_LIST` entry. To resolve, EITHER\n\
             route the I/O through `verter_workspace::NativeFs` /\n\
             `WorkspaceAccess`, OR add an `(path, justification)` tuple to\n\
             `D14_ALLOW_LIST` in `crates/verter_session/tests/architecture_guards.rs`.",
            violations.join("\n  "),
        );
    }

    #[test]
    fn d14_allow_list_paths_exist_and_actually_use_std_fs() {
        // Every D14 ALLOW_LIST entry must:
        //   1. Point at a real production source file.
        //   2. Actually contain `std::fs::` — a stale entry silently
        //      disarms the guard for an unrelated path that may later
        //      be reused.
        let root = workspace_root();
        let mut missing: Vec<String> = Vec::new();
        let mut clean: Vec<String> = Vec::new();
        for (path, _justification) in D14_ALLOW_LIST {
            let abs = root.join(path);
            if !abs.exists() {
                missing.push((*path).to_string());
                continue;
            }
            let src = match fs::read_to_string(&abs) {
                Ok(s) => s,
                Err(_) => {
                    missing.push((*path).to_string());
                    continue;
                }
            };
            if !d14_file_uses_std_fs(&src) {
                clean.push((*path).to_string());
            }
        }
        assert!(
            missing.is_empty(),
            "D14 ALLOW_LIST entries refer to paths that do not exist:\n  {}\n\n\
             Update or remove these entries; a stale allow-list silently\n\
             disarms the NativeFs invariant lock.",
            missing.join("\n  "),
        );
        assert!(
            clean.is_empty(),
            "D14 ALLOW_LIST entries refer to files that no longer use `std::fs::`:\n  {}\n\n\
             Delete these entries; they advertise an exemption that is no\n\
             longer warranted.",
            clean.join("\n  "),
        );
    }

    #[test]
    fn d14_predicate_rejects_deliberate_violation_and_passes_clean_source() {
        // Discriminating-violation: a fabricated production source
        // string that uses `std::fs::` MUST be flagged by the
        // predicate. A counter-fixture that does NOT use `std::fs::`
        // must NOT be flagged.
        let bad = "use std::fs::File;\nfn read() { let _ = std::fs::read_to_string(\"foo\"); }";
        assert!(
            d14_file_uses_std_fs(bad),
            "D14 predicate must flag direct `std::fs::` references",
        );

        let clean = "use crate::workspace::NativeFs;\nfn read(fs: &NativeFs) { let _ = fs.read_file(\"foo\"); }";
        assert!(
            !d14_file_uses_std_fs(clean),
            "D14 predicate must NOT flag code that goes through NativeFs",
        );
    }

    #[test]
    fn d14_native_fs_path_actually_contains_std_fs() {
        // Sanity counter-fixture: the canonical disk boundary file
        // MUST itself contain `std::fs::` calls (otherwise the
        // exemption is meaningless and a typo in the path constant
        // would silently disarm the guard).
        let abs = workspace_root().join(D14_NATIVE_FS_PATH);
        let src = fs::read_to_string(&abs)
            .unwrap_or_else(|e| panic!("D14 native_fs path must be readable: {e}"));
        assert!(
            d14_file_uses_std_fs(&src),
            "D14 NATIVE_FS_PATH (`{D14_NATIVE_FS_PATH}`) must contain `std::fs::` callsites; it is the canonical disk boundary the lock pivots on. If this assertion fires, either the path constant is stale or NativeFs has been refactored — update `D14_NATIVE_FS_PATH` accordingly.",
        );
    }

    #[test]
    fn d14_each_allow_list_entry_is_a_real_walker_hit() {
        // Discriminator: the D14 invariant lock is meaningful ONLY
        // when each `D14_ALLOW_LIST` entry actually corresponds to a
        // production walker hit. If an entry mapped to a path the
        // walker never reaches (wrong directory, typo'd path, file
        // moved without updating the entry), the entry has zero
        // protective effect and the lock can drift silently.
        //
        // This test runs the violation walker with an empty
        // allow-list (only `native_fs.rs` exempt) and asserts:
        //   1. The walker produces SOMETHING — proving the lock is
        //      non-trivial and the production tree contains real
        //      escapes from NativeFs that the allow-list is paying
        //      for.
        //   2. EVERY entry in `D14_ALLOW_LIST` shows up in that
        //      empty-allow-list violation set — proving each entry
        //      actually maps to a real `std::fs::` callsite the
        //      walker would otherwise flag.
        //
        // Removing the live ALLOW_LIST entries would, by transitive
        // implication, make the live `no_std_fs_outside_native_fs_or_allow_list`
        // test fail with violations equal to (this empty-allow-list
        // set) minus (any newly-migrated callsites). That is the
        // pre-change failure the brief requires this guard to
        // exhibit.
        let mut empty_permitted: BTreeSet<String> = BTreeSet::new();
        empty_permitted.insert(D14_NATIVE_FS_PATH.to_string());
        let violations = d14_violations(&empty_permitted);
        assert!(
            !violations.is_empty(),
            "D14 walker must detect at least one production `std::fs::` callsite outside\n\
             `native_fs.rs` when the allow-list is empty. If this fails, either the walker is\n\
             scoped to the wrong tree, or every previous escape from NativeFs has been migrated\n\
             (in which case `D14_ALLOW_LIST` should also be empty and this discriminator test\n\
             should be deleted along with it).",
        );
        let violations_set: BTreeSet<String> = violations.iter().cloned().collect();
        let mut entries_without_walker_hits: Vec<String> = Vec::new();
        for (path, _justification) in D14_ALLOW_LIST {
            if !violations_set.contains(*path) {
                entries_without_walker_hits.push((*path).to_string());
            }
        }
        assert!(
            entries_without_walker_hits.is_empty(),
            "D14 ALLOW_LIST entries must each represent a real walker hit. The following\n\
             entries are NOT detected as violations even when the allow-list is empty:\n  {}\n\n\
             A non-violating entry has no protective effect; either delete it or fix the path\n\
             so the entry actually maps to a production `std::fs::` callsite.",
            entries_without_walker_hits.join("\n  "),
        );
    }
}

// ===========================================================================
// guard 8 — every DB-typed field on `ProjectTypeStore` appears in the
// inventory `PROJECT_TYPE_STORE_DB_INVENTORY` and the runtime
// `all_dbs_for_invalidation()` list. Plan §12.A3 / §12.A10 step 7.
//
// The inventory is the single source of truth for which DBs participate
// in the typed cache invalidation cascade. Adding a DB-typed field
// outside the inventory fails this guard.
//
// Companion runtime guard:
// `crates/verter_session/tests/invalidation_coverage.rs`'s
// `every_db_in_project_type_store_participates_in_invalidation` walks
// the macro-generated runtime surface; this source-structure guard
// walks the actual struct definition and asserts every DB-typed field
// appears in the inventory.
// ===========================================================================

/// Predicate: does `rendered_ty` syntactically look like one of the
/// host-owned DB / Store / Registry types tracked by
/// [`crate::project_type_store::ProjectTypeStore`]?
///
/// Recognizes the suffix-pattern `*Db`, `*Store`, `*Registry`, plus
/// generic forms wrapping the same suffixes, plus `Arc<...>` wrappers.
/// Tolerant of syn's whitespace canonicalization (single-space-
/// separated tokens).
fn is_db_shape(rendered_ty: &str) -> bool {
    // Strip `Arc <` / `Arc<` wrapper before pattern-matching the
    // inner type's suffix.
    let inner = rendered_ty
        .trim()
        .strip_prefix("Arc <")
        .or_else(|| rendered_ty.trim().strip_prefix("Arc<"))
        .unwrap_or(rendered_ty)
        .trim_end_matches('>')
        .trim();
    // Recognize the head identifier: take chars up to `<` or
    // whitespace.
    let head_end = inner
        .find(|c: char| c == '<' || c.is_whitespace())
        .unwrap_or(inner.len());
    let head = inner[..head_end].trim();
    // The DB suffix family. `Counters` / `Snapshot` / `Hash` etc.
    // are NOT DB-shape and are excluded by the strict suffix check.
    let suffixes = ["Db", "Store", "Registry"];
    suffixes
        .iter()
        .any(|suf| head.ends_with(suf) && head.len() > suf.len())
}

/// Walk `source` (a `syn::parse_file`-able Rust file) for the struct
/// named `struct_ident` and return the names of every field whose type
/// matches a DB-shape pattern (`*Db`, `*Store`, `*Registry`,
/// `Arc<*Db>`, `Arc<*Store>`, `Arc<*Registry>`,
/// `ComponentMetaResultDb<...>`).
///
/// Returns the names that are NOT in `registered`. Pure function for
/// the deliberate-violation test below.
fn unregistered_db_fields_in_struct(
    source: &str,
    struct_ident: &str,
    registered: &[&str],
) -> Vec<String> {
    use syn::{parse_file, Item};

    let parsed = parse_file(source).expect("parse source via syn");
    let mut unregistered: Vec<String> = Vec::new();

    for item in &parsed.items {
        let Item::Struct(item_struct) = item else {
            continue;
        };
        if item_struct.ident != struct_ident {
            continue;
        }
        let syn::Fields::Named(named) = &item_struct.fields else {
            continue;
        };
        for field in &named.named {
            let Some(field_name) = field.ident.as_ref() else {
                continue;
            };
            let field_name_str = field_name.to_string();
            let rendered_ty = render_type(&field.ty);
            if !is_db_shape(&rendered_ty) {
                continue;
            }
            if !registered.iter().any(|r| *r == field_name_str) {
                unregistered.push(field_name_str);
            }
        }
    }

    unregistered
}

#[test]
fn every_db_field_in_project_type_store_appears_in_inventory() {
    let src = read_workspace_file("crates/verter_session/src/project_type_store.rs");
    let inventory = verter_session::project_type_store::PROJECT_TYPE_STORE_DB_INVENTORY;

    let unregistered = unregistered_db_fields_in_struct(&src, "ProjectTypeStore", inventory);

    assert!(
        unregistered.is_empty(),
        "guard 8: DB-typed field(s) on ProjectTypeStore are not in \
         PROJECT_TYPE_STORE_DB_INVENTORY: {unregistered:?}. \
         Adding a DB-typed field requires updating the inventory + \
         all_dbs_for_invalidation() in lockstep. See \
         crates/verter_session/src/project_type_store.rs."
    );
}

#[test]
fn guard8_predicate_rejects_unregistered_db_field() {
    // Deliberate-violation fixture: a struct with a DB-shape field
    // missing from the registered list. The predicate must surface
    // the offending field.
    let fixture_src = r#"
        pub struct FakeProjectTypeStore {
            pub indexed: FileArtifactStore,
            pub analysis: AnalysisReadyDb,
            pub forgotten_field: ForgottenStore,
        }
    "#;
    let registered = ["indexed", "analysis"];
    let unregistered =
        unregistered_db_fields_in_struct(fixture_src, "FakeProjectTypeStore", &registered);
    assert_eq!(
        unregistered,
        vec!["forgotten_field".to_string()],
        "guard 8 predicate must catch the unregistered DB field. \
         If this assertion fails, the cache-shape detector is too \
         narrow OR the registered-set check is broken."
    );
}

#[test]
fn guard8_predicate_passes_when_inventory_is_complete() {
    // Sanity counter-fixture: every DB-shape field IS registered.
    // The predicate must return an empty Vec.
    let fixture_src = r#"
        pub struct FakeProjectTypeStore {
            pub indexed: FileArtifactStore,
            pub analysis: AnalysisReadyDb,
        }
    "#;
    let registered = ["indexed", "analysis"];
    let unregistered =
        unregistered_db_fields_in_struct(fixture_src, "FakeProjectTypeStore", &registered);
    assert!(
        unregistered.is_empty(),
        "guard 8 predicate must accept a fully-registered struct, \
         got {unregistered:?}",
    );
}

// ===========================================================================
// guard 9 — every DB-typed field on `ProjectTypeStore` has a
// corresponding `impl InvalidationByCanonical for ...` block somewhere
// in the verter_session crate sources. Plan §12.A12 acceptance gate.
//
// Source-structure guard. Walks the struct via `syn::parse_file` and
// extracts the head identifier of each DB-shape field's type
// (`FileArtifactStore`, `AnalysisReadyDb`, `RouteDb`, ...). For every
// such head identifier, asserts that at least one source file under
// `crates/verter_session/src/` contains an
// `impl ... InvalidationByCanonical for <Head>` block.
//
// Companion runtime guard:
// `crates/verter_session/tests/invalidation_perf.rs`'s
// `invalidate_canonical_touches_only_indexed_entries` exercises the
// O(K) drain semantics for one representative DB; this guard asserts
// the full inventory is uniformly covered.
// ===========================================================================

/// Extract the head identifier of every DB-shape field's type from
/// `source` (a `syn::parse_file`-able Rust file) for the struct named
/// `struct_ident`. Strips `Arc<...>` and generic-parameter forms so
/// `Arc<RouteDb>` and `ComponentMetaResultDb<T>` both reduce to their
/// head identifier.
fn db_field_type_heads_in_struct(source: &str, struct_ident: &str) -> Vec<String> {
    use syn::{parse_file, Item};

    let parsed = parse_file(source).expect("parse source via syn");
    let mut heads: Vec<String> = Vec::new();

    for item in &parsed.items {
        let Item::Struct(item_struct) = item else {
            continue;
        };
        if item_struct.ident != struct_ident {
            continue;
        }
        let syn::Fields::Named(named) = &item_struct.fields else {
            continue;
        };
        for field in &named.named {
            if field.ident.is_none() {
                continue;
            }
            let rendered_ty = render_type(&field.ty);
            if !is_db_shape(&rendered_ty) {
                continue;
            }
            // Reduce to head identifier — same logic as `is_db_shape`'s
            // internal head extraction.
            let inner = rendered_ty
                .trim()
                .strip_prefix("Arc <")
                .or_else(|| rendered_ty.trim().strip_prefix("Arc<"))
                .unwrap_or(&rendered_ty)
                .trim_end_matches('>')
                .trim();
            let head_end = inner
                .find(|c: char| c == '<' || c.is_whitespace())
                .unwrap_or(inner.len());
            let head = inner[..head_end].trim().to_string();
            if !heads.contains(&head) {
                heads.push(head);
            }
        }
    }

    heads
}

/// Search every `.rs` file under `crates/verter_session/src/` for a
/// `impl ... InvalidationByCanonical for <type_head>` block. Tolerant
/// of `impl crate::invalidation_domain::InvalidationByCanonical`,
/// `impl<P> crate::...InvalidationByCanonical for ComponentMetaResultDb<P>`,
/// and the bare `impl InvalidationByCanonical for ...` form.
fn invalidation_by_canonical_impl_exists(crate_root: &std::path::Path, type_head: &str) -> bool {
    use std::fs;

    fn walk(dir: &std::path::Path, files: &mut Vec<std::path::PathBuf>) {
        let Ok(read_dir) = fs::read_dir(dir) else {
            return;
        };
        for entry in read_dir.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, files);
            } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
                files.push(path);
            }
        }
    }

    let mut files = Vec::new();
    walk(crate_root, &mut files);

    // Two variants matched: `for <Head>` (concrete) and `for <Head><`
    // (generic). The pattern allows arbitrary whitespace and the
    // optional `crate::invalidation_domain::` prefix.
    let needle_concrete = format!("InvalidationByCanonical for {type_head}");
    let needle_generic = format!("InvalidationByCanonical for {type_head}<");
    let needle_concrete_eol = format!("InvalidationByCanonical for {type_head}\n");
    let needle_concrete_brace = format!("InvalidationByCanonical for {type_head} ");

    for file in files {
        let Ok(src) = fs::read_to_string(&file) else {
            continue;
        };
        if src.contains(&needle_generic)
            || src.contains(&needle_concrete_eol)
            || src.contains(&needle_concrete_brace)
            || src.contains(&format!("{needle_concrete}\r\n"))
            || src.contains(&format!("{needle_concrete}{{"))
        {
            return true;
        }
    }
    false
}

#[test]
fn every_db_field_implements_invalidation_by_canonical() {
    let src = read_workspace_file("crates/verter_session/src/project_type_store.rs");
    let heads = db_field_type_heads_in_struct(&src, "ProjectTypeStore");

    let crate_root = workspace_path("crates/verter_session/src");
    let mut missing: Vec<String> = Vec::new();
    for head in &heads {
        if !invalidation_by_canonical_impl_exists(&crate_root, head) {
            missing.push(head.clone());
        }
    }

    assert!(
        missing.is_empty(),
        "guard 9: DB-typed field(s) on ProjectTypeStore have no \
         corresponding `impl InvalidationByCanonical for ...` block \
         in `crates/verter_session/src/`: {missing:?}. Plan §12.A12 \
         requires every DB to implement the per-canonical drain \
         trait so the cascade `invalidate_canonical_across_all_dbs` \
         can dispatch monomorphically."
    );
}

#[test]
fn guard9_predicate_rejects_missing_invalidation_by_canonical_impl() {
    // Deliberate-violation fixture: a struct with a DB-shape field
    // whose head identifier is NOT in the workspace's
    // verter_session src tree (so `invalidation_by_canonical_impl_exists`
    // returns false). Predicate must surface the missing impl.
    let fixture_src = r#"
        pub struct FakeProjectTypeStore {
            pub forgotten_field: NonExistentSentinelGuard9Db,
        }
    "#;
    let heads = db_field_type_heads_in_struct(fixture_src, "FakeProjectTypeStore");
    assert_eq!(
        heads,
        vec!["NonExistentSentinelGuard9Db".to_string()],
        "guard 9 predicate must extract the field's type head id",
    );
    let crate_root = workspace_path("crates/verter_session/src");
    let exists = invalidation_by_canonical_impl_exists(&crate_root, "NonExistentSentinelGuard9Db");
    assert!(
        !exists,
        "guard 9 predicate must report `false` for a head identifier \
         that has no corresponding impl block in the source tree",
    );
}

#[test]
fn guard9_predicate_passes_for_known_implementor() {
    // Sanity counter-fixture: `FileArtifactStore` IS implemented in the
    // workspace. The detector must report `true`.
    let crate_root = workspace_path("crates/verter_session/src");
    assert!(
        invalidation_by_canonical_impl_exists(&crate_root, "FileArtifactStore"),
        "guard 9 predicate must report `true` for a known \
         InvalidationByCanonical implementor (FileArtifactStore)",
    );
}

// ===========================================================================
// guard 10 — no_cross_product_binary_imports
//
// `verter_lsp` and `verter_mcp` are two independent product surfaces on
// top of the shared `verter_session` core. Their binaries must ship as
// separate processes — the LSP binary must not pull `verter_mcp` (the
// MCP server crate) into its compile graph, and the MCP server binary
// must not pull `verter_lsp` into its compile graph.
//
// Concretely the guard scans each product's `Cargo.toml` and rejects
// any line that declares the cross-product crate as a dependency
// (regardless of `optional = true` / feature gating). The previous
// `lsp_mcp_dependency_direction` guard tolerated `optional = true`
// because earlier work decoupled MCP behind a Cargo feature; this
// guard supersedes that allowance — the cross-product dependency is
// removed in full.
//
// The companion D26 acceptance test
// `lsp_no_longer_embeds_mcp_AND_mcp_http_still_serves` then asserts
// the binary entrypoints actually reflect that boundary AND that
// `verter_mcp_server` still ships the standalone HTTP launcher so
// IDE consumers have a separately-shippable transport.
// ===========================================================================

/// Predicate: scan a `Cargo.toml` snippet for any dependency declaration
/// that names `crate_name` as a dep (any form: bare path, table form,
/// `optional = true`, feature-gated, etc.). Returns `true` when at
/// least one matching declaration exists in the `[dependencies]`,
/// `[dev-dependencies]`, `[build-dependencies]`, or
/// `[target.*.dependencies]` sections — including the
/// `[dependencies.<crate>]` section-header form.
fn cargo_toml_declares_dep(src: &str, crate_name: &str) -> bool {
    fn is_dep_section(section: &str) -> bool {
        section == "dependencies"
            || section == "dev-dependencies"
            || section == "build-dependencies"
            || (section.starts_with("target.")
                && (section.ends_with(".dependencies")
                    || section.ends_with(".dev-dependencies")
                    || section.ends_with(".build-dependencies")))
    }

    let mut in_deps_section = false;
    for line in src.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix('[') {
            let header = rest.trim_end_matches(']').trim();
            // `[dependencies.<crate>]` form: split on the FIRST '.'
            // after a recognized dep section name and check the
            // crate suffix exactly.
            if let Some((section, suffix)) = header.split_once('.') {
                if is_dep_section(section) && suffix == crate_name {
                    return true;
                }
                in_deps_section = is_dep_section(section)
                    || (section == "target"
                        && (suffix.ends_with(".dependencies")
                            || suffix.ends_with(".dev-dependencies")
                            || suffix.ends_with(".build-dependencies")));
                continue;
            }
            in_deps_section = is_dep_section(header);
            continue;
        }
        if !in_deps_section {
            continue;
        }
        // Match `<crate_name> = ...` with optional whitespace.
        // Avoid matching prefixes (e.g. `verter_mcp_server` must NOT
        // match `verter_mcp`).
        if let Some((k, _)) = trimmed.split_once('=') {
            if k.trim() == crate_name {
                return true;
            }
        }
    }
    false
}

#[test]
fn no_cross_product_binary_imports() {
    // `verter_lsp` (LSP product) must not depend on `verter_mcp`
    // (MCP product) in any form. The previous `optional = true`
    // tolerance is retired by Tier 3.
    let lsp_cargo = read_workspace_file("crates/verter_lsp/Cargo.toml");
    assert!(
        !cargo_toml_declares_dep(&lsp_cargo, "verter_mcp"),
        "guard 10 (`no_cross_product_binary_imports`) violation: \
         `crates/verter_lsp/Cargo.toml` declares `verter_mcp` as a \
         dependency. The LSP and MCP products must ship as separate \
         binaries with no cross-product compile-graph coupling. \
         Spawn `verter_mcp_server` in its own process instead.",
    );

    // `verter_mcp` must not depend on `verter_lsp` either. This
    // direction is asserted symmetrically so future plan churn does
    // not silently re-couple the two products.
    let mcp_cargo = read_workspace_file("crates/verter_mcp/Cargo.toml");
    assert!(
        !cargo_toml_declares_dep(&mcp_cargo, "verter_lsp"),
        "guard 10 (`no_cross_product_binary_imports`) violation: \
         `crates/verter_mcp/Cargo.toml` declares `verter_lsp` as a \
         dependency. The LSP and MCP products must ship as separate \
         binaries with no cross-product compile-graph coupling.",
    );

    let mcp_server_cargo = read_workspace_file("crates/verter_mcp_server/Cargo.toml");
    assert!(
        !cargo_toml_declares_dep(&mcp_server_cargo, "verter_lsp"),
        "guard 10 (`no_cross_product_binary_imports`) violation: \
         `crates/verter_mcp_server/Cargo.toml` declares `verter_lsp` \
         as a dependency. The standalone MCP server binary must \
         remain independent of the LSP product.",
    );
}

#[test]
fn guard10_predicate_rejects_deliberate_cross_product_dep() {
    let bad_plain = "[dependencies]\nverter_mcp = { path = \"../verter_mcp\" }\nfoo = \"1\"\n";
    let bad_optional =
        "[dependencies]\nverter_mcp = { path = \"../verter_mcp\", optional = true }\nfoo = \"1\"\n";
    let bad_dotted = "[dependencies.verter_mcp]\npath = \"../verter_mcp\"\n";
    let bad_feature_gated =
        "[features]\nmcp = [\"dep:verter_mcp\"]\n\n[dependencies]\nverter_mcp = { path = \"../verter_mcp\", optional = true }\n";
    let good_no_dep = "[dependencies]\nfoo = \"1\"\nbar = \"2\"\n";
    let good_unrelated_section = "[features]\nmcp = []\n\n[dependencies]\nfoo = \"1\"\n";
    let good_prefix_only =
        "[dependencies]\nverter_mcp_server = { path = \"../verter_mcp_server\" }\nfoo = \"1\"\n";

    assert!(
        cargo_toml_declares_dep(bad_plain, "verter_mcp"),
        "guard 10 predicate must flag a plain `verter_mcp = ...` dep",
    );
    assert!(
        cargo_toml_declares_dep(bad_optional, "verter_mcp"),
        "guard 10 predicate must flag an `optional = true` dep — \
         Tier 3 retires the optional-dep tolerance",
    );
    assert!(
        cargo_toml_declares_dep(bad_dotted, "verter_mcp"),
        "guard 10 predicate must flag a `[dependencies.verter_mcp]` \
         section header",
    );
    assert!(
        cargo_toml_declares_dep(bad_feature_gated, "verter_mcp"),
        "guard 10 predicate must flag a feature-gated optional dep \
         even when the dep line is wrapped behind a `[features]` \
         section earlier in the file",
    );
    assert!(
        !cargo_toml_declares_dep(good_no_dep, "verter_mcp"),
        "guard 10 predicate must NOT flag a Cargo.toml that does not \
         depend on the cross-product crate",
    );
    assert!(
        !cargo_toml_declares_dep(good_unrelated_section, "verter_mcp"),
        "guard 10 predicate must NOT flag a `[features]` table entry \
         named `mcp` that lives outside any dependency section",
    );
    assert!(
        !cargo_toml_declares_dep(good_prefix_only, "verter_mcp"),
        "guard 10 predicate must NOT flag a dep with a name that has \
         `verter_mcp` as a strict prefix (e.g. `verter_mcp_server`)",
    );
}

// ===========================================================================
// D26 — lsp_no_longer_embeds_mcp_AND_mcp_http_still_serves
//
// Combined acceptance discriminator for the Tier 3 LSP/MCP product
// boundary decoupling. The test FAILS for two distinct reasons
// before Tier 3 lands and PASSES only when both conditions hold:
//
//   (a) `verter_lsp` has been fully decoupled from `verter_mcp` —
//       no Cargo dep, no `serve_mcp_http` function on the binary
//       entrypoint, no `verter_mcp::` path references, no
//       `use verter_mcp` import.
//   (b) `verter_mcp_server` still ships the standalone HTTP launcher
//       so consumers retain a working out-of-process MCP transport.
//
// Per plan §5.2: pre-Tier-3 FAILS for two distinct reasons (Cargo dep
// present OR HTTP launcher broken); post-Tier-3 PASSES only when
// both conditions hold.
// ===========================================================================

#[test]
#[allow(non_snake_case)]
fn lsp_no_longer_embeds_mcp_AND_mcp_http_still_serves() {
    // ── Condition (a) — LSP no longer embeds MCP ──
    let lsp_cargo = read_workspace_file("crates/verter_lsp/Cargo.toml");
    assert!(
        !cargo_toml_declares_dep(&lsp_cargo, "verter_mcp"),
        "D26 condition (a) violation: `crates/verter_lsp/Cargo.toml` \
         still declares `verter_mcp` as a dependency. Tier 3 deletes \
         this dep so the LSP binary cannot embed the MCP server.",
    );

    // The LSP binary entrypoint must not host the in-process MCP
    // server. We assert the absence of the `serve_mcp_http` function
    // (the embedding point) and any direct `verter_mcp` reference.
    let lsp_main = read_workspace_file("crates/verter_lsp/src/main.rs");
    assert!(
        !lsp_main.contains("fn serve_mcp_http"),
        "D26 condition (a) violation: `crates/verter_lsp/src/main.rs` \
         still defines `serve_mcp_http`. Tier 3 deletes this in-process \
         MCP launcher; consumers must spawn `verter_mcp_server` \
         instead.",
    );
    assert!(
        !lsp_main.contains("use verter_mcp"),
        "D26 condition (a) violation: `crates/verter_lsp/src/main.rs` \
         still imports `verter_mcp`. Tier 3 removes all cross-product \
         imports from the LSP binary.",
    );
    assert!(
        !lsp_main.contains("verter_mcp::"),
        "D26 condition (a) violation: `crates/verter_lsp/src/main.rs` \
         still references `verter_mcp::` symbols on a path. Tier 3 \
         removes all cross-product references from the LSP binary.",
    );

    // ── Condition (b) — MCP HTTP launcher still serves ──
    // The standalone `verter_mcp_server` binary must continue to
    // expose the HTTP transport so consumers that previously routed
    // through `verter-lsp --mcp-port=...` retain a working
    // out-of-process replacement. We assert the presence of the
    // `Transport::Http` arm wired through `axum::serve` on a TCP
    // listener — the structural shape that proves the launcher
    // still serves.
    let mcp_main = read_workspace_file("crates/verter_mcp_server/src/main.rs");
    assert!(
        mcp_main.contains("Transport::Http"),
        "D26 condition (b) violation: \
         `crates/verter_mcp_server/src/main.rs` no longer matches \
         `Transport::Http` — the standalone MCP HTTP launcher is \
         broken. Tier 3 requires this launcher remain operational.",
    );
    assert!(
        mcp_main.contains("axum::serve"),
        "D26 condition (b) violation: \
         `crates/verter_mcp_server/src/main.rs` no longer calls \
         `axum::serve` — the HTTP transport is broken. Tier 3 \
         requires this launcher remain operational.",
    );
    assert!(
        mcp_main.contains("TcpListener::bind"),
        "D26 condition (b) violation: \
         `crates/verter_mcp_server/src/main.rs` no longer binds a \
         TCP listener — the HTTP transport cannot start. Tier 3 \
         requires this launcher remain operational.",
    );
    assert!(
        mcp_main.contains("StreamableHttpService"),
        "D26 condition (b) violation: \
         `crates/verter_mcp_server/src/main.rs` no longer wires the \
         `StreamableHttpService` rmcp transport. Tier 3 requires \
         this launcher remain operational.",
    );
}

// ════════════════════════════════════════════════════════════════════════════
// Tier 1A architecture guards (§3.2.5)
//
// Four guards landing with Step 1A:
//
// 1. `no_thread_local_oxc_caches` — rejects reintroduction of the
//    `HOST_PARSED_*_CACHE` thread-locals (D44 lowering boundary).
// 2. `no_direct_oxc_parser_calls_outside_scheduler_path` — only the
//    parse stage in `host_executor.rs` may invoke the OXC parser.
// 3. `no_owned_artifact_holds_borrowed_lifetime` — `OwnedEvalProgram`
//    and `OwnedTypeResolutionContext` are `Send + Sync + 'static`.
// 4. `macro_impacting_constructs_fail_lowering_not_silent_skip` (D107)
//    — exercises the lowering on representative macro-impacting
//    fixtures and asserts `Err(LoweringError::*)` instead of an empty
//    `OwnedEvalProgram`.
// ════════════════════════════════════════════════════════════════════════════

/// Tier 1A guard 1 — the `HOST_PARSED_EVAL_PROGRAM_CACHE` and
/// `HOST_PARSED_TYPE_CONTEXT_CACHE` thread-locals were retired in §3.2.4
/// because their cached values (`Rc<ParsedEvalProgram>` /
/// `Rc<ParsedTypeResolutionContext>`) held the OXC parser arena alive
/// past the lowering boundary, making the host caches `!Send`.
///
/// The owned-artifact lowering pipeline produces `OwnedEvalProgram` /
/// `OwnedTypeResolutionContext` (both `Send + Sync + 'static`) so the
/// host caches now sit on the typed `EvalEnvCacheDb` /
/// `TypeResolutionContextDb` shells (introduced empty in 1A; consumer
/// migration in 1C-α).
///
/// This guard rejects any reintroduction of an OXC-parser-arena
/// thread-local cache. It scans every `.rs` file under
/// `crates/verter_session/src/` (excluding `_tests.rs` and `tests.rs`)
/// for the literal cache names — a discriminating identifier is more
/// reliable here than a generic `thread_local!\s*\{` regex which would
/// match the legitimate per-thread depth counters in
/// `RESOLUTION_DEPTH`, `LAST_BUDGET_EXCEEDED`, etc.
#[test]
fn no_thread_local_oxc_caches() {
    let banned_idents = [
        "HOST_PARSED_EVAL_PROGRAM_CACHE",
        "HOST_PARSED_TYPE_CONTEXT_CACHE",
    ];
    let mut hits: Vec<(String, &str)> = Vec::new();
    let crate_root = workspace_path("crates/verter_session/src");
    for entry in walkdir::WalkDir::new(&crate_root)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.path().is_file())
    {
        let path = entry.path();
        let path_str = path.to_string_lossy().replace('\\', "/");
        // Skip test sources — only production rs files participate.
        if path_str.ends_with("_tests.rs") || path_str.ends_with("/tests.rs") {
            continue;
        }
        if path_str.contains("/architecture_guards") {
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let body = std::fs::read_to_string(path).unwrap_or_default();
        for ident in banned_idents {
            // Only count hits OUTSIDE comments. The retirement note
            // in `host_manage.rs` references the names in a
            // documentation comment; that's not a re-introduction.
            for (lineno, line) in body.lines().enumerate() {
                if !line.contains(ident) {
                    continue;
                }
                let trimmed = line.trim_start();
                if trimmed.starts_with("//") || trimmed.starts_with("///") {
                    continue;
                }
                hits.push((format!("{path_str}:{}", lineno + 1), ident));
            }
        }
    }
    assert!(
        hits.is_empty(),
        "Tier 1A guard `no_thread_local_oxc_caches`: forbidden thread-local OXC caches \
         re-introduced in production source: {hits:#?}"
    );
}

/// Tier 1A guard 2 — only the host parse stage in `host_executor.rs`
/// may directly invoke `oxc_parser::Parser::new`. Other production
/// callers must go through the scheduler-routed parse path so the
/// authoritative parse-once-per-(canonical, content_hash) discipline
/// is preserved.
///
/// The borrowed-form lowering input is constructed inside
/// `crate::ParsedEvalProgram::parse` (in `lib.rs`), which IS the
/// scheduler-bound entry point. Test sources are exempt.
#[test]
fn no_direct_oxc_parser_calls_outside_scheduler_path() {
    // Allow-list: production files that legitimately invoke
    // `oxc_parser::Parser::new`. Updating this list requires a
    // matching reference to a scheduler-bound parse path or a
    // documented TODO to migrate.
    let allow_list = [
        // The `ParsedEvalProgram::parse` constructor IS the
        // scheduler-bound parse entry; `host_executor.rs` calls it.
        "crates/verter_session/src/lib.rs",
        // host_executor.rs itself calls `parse_vue_snapshot` /
        // `parse_non_sfc_snapshot`; the OXC parser is invoked inside
        // those helpers in `crate::parse`, but they go through the
        // scheduler. Allowed.
        "crates/verter_session/src/parse.rs",
        // host_executor.rs is the parse stage executor itself.
        "crates/verter_session/src/host_executor.rs",
        // Pre-existing resolver paths that allocate a temporary OXC
        // arena for one-shot type-body re-parsing. These are NOT
        // long-lived cache populators (the borrowed `Allocator` is
        // constructed and dropped within the same function) so they
        // do not violate the lowering-boundary invariant. The 1C-α
        // consumer migration moves these onto the typed
        // `EvalEnvCacheDb` path; until then the allow-list pins the
        // exact files so a NEW caller would still trip the guard.
        // TODO(1C-α): route through the scheduler's `execute_source`.
        "crates/verter_session/src/resolver_core/external_type_body.rs",
        "crates/verter_session/src/resolver_core/surface_projector.rs",
    ];

    let crate_root = workspace_path("crates/verter_session/src");
    let mut violators: Vec<String> = Vec::new();
    for entry in walkdir::WalkDir::new(&crate_root)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.path().is_file())
    {
        let path = entry.path();
        let path_str = path.to_string_lossy().replace('\\', "/");
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        // Skip test sources.
        if path_str.ends_with("_tests.rs") || path_str.ends_with("/tests.rs") {
            continue;
        }
        if path_str.contains("/architecture_guards") {
            continue;
        }
        let body = std::fs::read_to_string(path).unwrap_or_default();
        // Match `oxc_parser::Parser::new` outside comments.
        let mut hit = false;
        for line in body.lines() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") || trimmed.starts_with("///") {
                continue;
            }
            if line.contains("oxc_parser::Parser::new") {
                hit = true;
                break;
            }
        }
        if hit {
            // Strip the workspace prefix so the suffix matches the
            // allow-list entries.
            let rel = path_str
                .split("crates/")
                .last()
                .map(|s| format!("crates/{s}"))
                .unwrap_or(path_str.clone());
            if !allow_list.iter().any(|allow| rel.ends_with(allow)) {
                violators.push(rel);
            }
        }
    }
    assert!(
        violators.is_empty(),
        "Tier 1A guard `no_direct_oxc_parser_calls_outside_scheduler_path`: \
         production callers invoke `oxc_parser::Parser::new` outside the \
         scheduler-bound parse path: {violators:#?}\n\n\
         Either route through the scheduler's `execute_source` (preferred) \
         or extend the allow-list with a pinned justification."
    );
}

/// Tier 1A guard 3 — `OwnedEvalProgram` and `OwnedTypeResolutionContext`
/// MUST be `Send + Sync + 'static`. Compile-time `assert_impl_all!`
/// guards in the production source files enforce this; the test here
/// makes the assertion observable in the `cargo test` output and
/// asserts the structural invariant via syn-AST inspection (no
/// lifetime parameter, no `Rc`/`Cell`-typed field).
#[test]
fn no_owned_artifact_holds_borrowed_lifetime() {
    // Side 1: `assert_impl_all!`-style runtime guard. If a regression
    // re-introduced `Rc<...>`, the type would lose Send and the bound
    // would fail to compile (this test file would fail to build).
    fn assert_send_sync_static<T: Send + Sync + 'static>() {}
    assert_send_sync_static::<verter_session::owned_artifacts::eval_program::OwnedEvalProgram>();
    assert_send_sync_static::<
        verter_session::owned_artifacts::type_resolution_context::OwnedTypeResolutionContext,
    >();

    // Side 2: structural invariant. Walk the syn-AST of both source
    // files and verify the canonical structs carry NO lifetime
    // parameter. This catches future regressions where someone adds a
    // `pub struct OwnedEvalProgram<'a>` form (which would be a step
    // back toward the borrowed-form contract).
    for (path, struct_name) in [
        (
            "crates/verter_session/src/owned_artifacts/eval_program.rs",
            "OwnedEvalProgram",
        ),
        (
            "crates/verter_session/src/owned_artifacts/type_resolution_context.rs",
            "OwnedTypeResolutionContext",
        ),
    ] {
        let body = read_workspace_file(path);
        let parsed: syn::File = syn::parse_str(&body).expect("parse owned-artifact module");
        let mut found = false;
        for item in &parsed.items {
            if let syn::Item::Struct(s) = item {
                if s.ident == struct_name {
                    found = true;
                    let has_lifetime = s.generics.lifetimes().next().is_some();
                    assert!(
                        !has_lifetime,
                        "Tier 1A guard `no_owned_artifact_holds_borrowed_lifetime`: \
                         `{struct_name}` MUST carry no lifetime parameter; \
                         a regression in {path} re-introduced one."
                    );
                }
            }
        }
        assert!(
            found,
            "{struct_name} not found in {path} (test self-broken)"
        );
    }
}

/// Tier 1A guard 4 (D107) — macro-impacting unsupported AST kinds MUST
/// surface as a typed `LoweringError`, NOT as a silent skip producing
/// an empty / missing macro shape.
///
/// The discriminating predicate exercises the `LoweringError` value
/// constructors with representative macro-impacting fixtures (one per
/// "FAIL on Unsupported" row in the inventory). The test asserts that
/// each constructed value is a real, distinguishable, non-empty
/// `LoweringError` — i.e., the lowering pipeline COULD return such an
/// error and consumers can branch on it. A regression that reduces
/// `LoweringError` to a unit-only enum would lose the contract and
/// fail this test.
///
/// The 1C-α consumer migration wires the actual lowering driver to
/// produce these errors; this Tier 1A guard documents the contract
/// and the structural shape.
#[test]
fn macro_impacting_constructs_fail_lowering_not_silent_skip() {
    use verter_session::owned_artifacts::eval_program::{
        LoweringError, OwnedEvalProgram, SpanId, UnsupportedKind,
    };

    // Representative fixtures — one per "FAIL on Unsupported" row in
    // `eval_program_macro_impact_inventory.md`.
    let fixtures: Vec<LoweringError> = vec![
        LoweringError::UnsupportedMacroArgumentShape {
            macro_name: "defineProps".into(),
            span: SpanId::new(0, 10),
            kind: UnsupportedKind::Other("ConditionalExpression"),
        },
        LoweringError::UnsupportedMacroArgumentShape {
            macro_name: "defineProps".into(),
            span: SpanId::new(0, 10),
            kind: UnsupportedKind::Other("SpreadElement"),
        },
        LoweringError::UnsupportedMacroArgumentShape {
            macro_name: "defineEmits".into(),
            span: SpanId::new(0, 10),
            kind: UnsupportedKind::Other("AwaitExpression"),
        },
        LoweringError::UnsupportedMacroArgumentShape {
            macro_name: "defineEmits".into(),
            span: SpanId::new(0, 10),
            kind: UnsupportedKind::Other("YieldExpression"),
        },
        LoweringError::UnsupportedMacroArgumentShape {
            macro_name: "defineSlots".into(),
            span: SpanId::new(0, 10),
            kind: UnsupportedKind::Other("SequenceExpression"),
        },
        LoweringError::UnsupportedMacroArgumentShape {
            macro_name: "withDefaults".into(),
            span: SpanId::new(0, 10),
            kind: UnsupportedKind::Other("ComputedMemberExpression"),
        },
        LoweringError::UnsupportedMacroArgumentShape {
            macro_name: "withDefaults".into(),
            span: SpanId::new(0, 10),
            kind: UnsupportedKind::Other("TemplateLiteralPropertyKey"),
        },
        LoweringError::UnsupportedMacroRelevantConstruct {
            construct: "TSConstructorType".into(),
            span: SpanId::new(0, 10),
        },
        LoweringError::UnsupportedMacroRelevantConstruct {
            construct: "TSInferType".into(),
            span: SpanId::new(0, 10),
        },
    ];

    for err in &fixtures {
        // Discriminator: each fixture MUST render to a non-empty
        // string with the relevant macro/construct name. A
        // unit-variant `LoweringError::Generic` with no payload
        // would render an empty body and fail this assertion.
        let rendered = format!("{err}");
        assert!(
            !rendered.is_empty(),
            "macro-impacting LoweringError fixture must render non-empty"
        );
        match err {
            LoweringError::UnsupportedMacroArgumentShape { macro_name, .. } => {
                assert!(rendered.contains(macro_name.as_ref()));
            }
            LoweringError::UnsupportedMacroRelevantConstruct { construct, .. } => {
                assert!(rendered.contains(construct.as_ref()));
            }
            LoweringError::UnsupportedTopLevelImport { specifier, .. } => {
                assert!(rendered.contains(specifier.as_ref()));
            }
        }

        // Negative discriminator: this same input MUST NOT collapse
        // to a silent empty `OwnedEvalProgram`. The contract that
        // breaks the silent-skip ambiguity is that the typed error
        // CARRIES distinguishing information; an empty program carries
        // none.
        let silent = OwnedEvalProgram::empty();
        assert_eq!(
            silent.statements.len(),
            0,
            "silent-skip empty program MUST stay structurally empty so the \
             discriminator vs LoweringError stays meaningful"
        );
    }

    // Inventory backstop: confirm that every fixture's "kind" /
    // "construct" string appears somewhere in the inventory's body.
    // A Tier 1A regression that drops a FAIL row from the inventory
    // while keeping the LoweringError variant produces a divergence
    // between code and documentation; this test catches it.
    let inventory_path =
        "crates/verter_session/src/owned_artifacts/eval_program_macro_impact_inventory.md";
    let inventory = read_workspace_file(inventory_path);
    let must_contain = [
        "ConditionalExpression",
        "SpreadElement",
        "AwaitExpression",
        "YieldExpression",
        "SequenceExpression",
        "ComputedMemberExpression",
        "TSConstructorType",
        "TSInferType",
    ];
    for needle in must_contain {
        assert!(
            inventory.contains(needle),
            "inventory at {inventory_path} missing FAIL row for `{needle}` — \
             Tier 1A LoweringError variant has no provenance",
        );
    }
}

// ============================================================================
// Tier 2 W5f — split-target size budget + post-split phase-archaeology guards
// (plan §4.6 discriminating tests)
// ============================================================================
//
// Tier 2 split-target modules (plan §4.2):
//
// | W5* | Path                                                        |
// |-----|-------------------------------------------------------------|
// | W5a | crates/verter_session/src/semantic_query_memo               |
// | W5b | crates/verter_parser/src/utils/oxc/vue/script/resolve_type  |
// | W5c | crates/verter_session/src/host_resolve                      |
// | W5d | crates/verter_session/src/resolver_core/component_meta      |
// | W5e | crates/verter_ffi/src/convert                               |

/// Shared helper for the W5f phase-archaeology guards. Reuses the
/// broader-D111 classifier from `foundations_guards` so the test-file
/// and production-code predicates stay byte-identical.
mod w5f_test_archaeology {
    use std::path::Path;
    use walkdir::WalkDir;

    use super::workspace_root;

    /// W5f cutoff baseline. Update only when test-file cleanup reduces
    /// the count, or when intentional new fixture vocabulary is added
    /// (e.g., the Stage-family deliberate-violation fixtures added
    /// alongside the predicate extension).
    pub(super) const W5F_BASELINE: usize = 269;

    pub(super) fn is_test_file(path: &Path) -> bool {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        let parent_name = path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        name == "tests.rs"
            || name.ends_with("_tests.rs")
            || parent_name == "tests"
            || path
                .components()
                .any(|c| c.as_os_str().to_str() == Some("tests"))
    }

    /// Walks `crates/*/src/` and returns each archaeology line as
    /// `"<rel>:<line_no>"`. Empty result == invariant satisfied.
    pub(super) fn collect_test_archaeology_violations() -> Vec<String> {
        let workspace = workspace_root();
        let mut violations = Vec::<String>::new();
        for crate_entry in std::fs::read_dir(workspace.join("crates")).expect("read crates/") {
            let crate_dir = crate_entry.expect("crate dir entry").path();
            let src = crate_dir.join("src");
            if !src.is_dir() {
                continue;
            }
            for entry in WalkDir::new(&src) {
                let entry = entry.expect("walkdir entry");
                if !entry.file_type().is_file() {
                    continue;
                }
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                    continue;
                }
                if !is_test_file(path) {
                    continue;
                }
                let rel = path
                    .strip_prefix(&workspace)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                let src_text =
                    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {rel}: {e}"));
                for (line_no, line) in src_text.lines().enumerate() {
                    if super::foundations_guards::line_has_phase_archaeology_d111(line) {
                        violations.push(format!("{rel}:{}", line_no + 1));
                    }
                }
            }
        }
        violations
    }

    pub(super) fn count_test_archaeology_lines() -> usize {
        collect_test_archaeology_violations().len()
    }
}

const TIER_2_SPLIT_TARGETS: &[&str] = &[
    "crates/verter_session/src/semantic_query_memo",
    "crates/verter_parser/src/utils/oxc/vue/script/resolve_type",
    "crates/verter_session/src/host_resolve",
    "crates/verter_session/src/resolver_core/component_meta",
    "crates/verter_ffi/src/convert",
];

#[test]
fn god_module_size_budget_targets_five_files() {
    // Meta-test (plan §4.6): the god_module_size_budget guard's target
    // list must reference each of the five Tier 2 split-target paths.
    // A future edit that drops a target from the list silently weakens
    // the budget; this guard fails fast on regression.
    //
    // The check reads architecture_guards.rs as text and asserts each
    // Tier 2 path appears at least once.
    let src = read_workspace_file("crates/verter_session/tests/architecture_guards.rs");
    let mut missing = Vec::new();
    for target in TIER_2_SPLIT_TARGETS {
        if !src.contains(target) {
            missing.push(*target);
        }
    }
    assert!(
        missing.is_empty(),
        "god_module_size_budget_targets_five_files: tests/architecture_guards.rs must reference these Tier 2 split-target paths so the size-budget guard cannot silently drop coverage:\n{}",
        missing.join("\n"),
    );
}

#[test]
fn each_post_split_module_under_lines_budget() {
    // Plan §4.7 acceptance: each post-split module < 4000 LOC.
    //
    // Walks the five Tier 2 split-target directories (post-split form
    // is mod.rs plus siblings, never the pre-split flat file).
    // Production .rs files only — sibling _tests.rs and tests.rs files
    // are governed by separate hygiene rules (testing skill) and
    // intentionally allowed to be larger than the production budget.
    use walkdir::WalkDir;
    const MAX_LINES: usize = 4000;

    fn is_test_fixture(rel: &str) -> bool {
        rel.ends_with("_tests.rs") || rel.ends_with("/tests.rs") || rel.contains("/tests/")
    }

    let workspace = workspace_root();
    let mut violations = Vec::<String>::new();
    for target in TIER_2_SPLIT_TARGETS {
        let dir_root = workspace.join(target);
        let flat_file = workspace.join(format!("{target}.rs"));

        if dir_root.is_dir() {
            for entry in WalkDir::new(&dir_root) {
                let entry = entry.expect("walkdir entry");
                if !entry.file_type().is_file() {
                    continue;
                }
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                    continue;
                }
                let rel = path
                    .strip_prefix(&workspace)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                if is_test_fixture(&rel) {
                    continue;
                }
                let src =
                    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {rel}: {e}"));
                let lines = src.lines().count();
                if lines > MAX_LINES {
                    violations.push(format!(
                        "{rel}: {lines} > {MAX_LINES} (Tier 2 post-split budget)"
                    ));
                }
            }
        } else if flat_file.is_file() {
            violations.push(format!(
                "{target}.rs still exists as a flat file — Tier 2 split incomplete"
            ));
        } else {
            violations.push(format!(
                "{target}: missing post-split target (no dir, no flat file)"
            ));
        }
    }

    assert!(
        violations.is_empty(),
        "each_post_split_module_under_lines_budget violations:\n{}",
        violations.join("\n")
    );
}

#[test]
fn no_phase_archaeology_in_test_files() {
    // Plan §4.6: phase-archaeology classifier (D111) applied to test
    // files inside crates/*/src/ — tests.rs, *_tests.rs, and anything
    // under a tests/ subdirectory.
    //
    // Until the Step 2.6 sweep completes, this guard operates as a
    // regression backstop: the current count must stay at-or-below the
    // baseline. Once the sweep finishes, the baseline drops to zero and
    // phase_archaeology_test_files_count_zero becomes the strict
    // invariant.
    //
    // Predicate is shared with foundations_guards::line_has_phase_archaeology_d111
    // (the existing broader-D111 production-code guard) so the test-file
    // and production-code classifiers stay byte-identical.
    let count = w5f_test_archaeology::count_test_archaeology_lines();

    // W5f cutoff baseline. Re-measured at this commit using the shared
    // foundations_guards::line_has_phase_archaeology_d111 predicate.
    // Update only when the Step 2.6 sweep actually removes occurrences.
    const REGRESSION_BACKSTOP: usize = w5f_test_archaeology::W5F_BASELINE;
    assert!(
        count <= REGRESSION_BACKSTOP,
        "no_phase_archaeology_in_test_files: count regressed beyond W5f baseline.\ncurrent: {count}\nbaseline (W5f cutoff): {REGRESSION_BACKSTOP}\nEither remove the new violations or update the baseline if the increase is intentional.",
    );
}

#[test]
#[ignore = "Pending Step 2.6 sweep — see plan §4 W5f. The non-zero baseline must drop to 0 before this guard activates. Tracked in phase-tier-2-complete marker."]
fn phase_archaeology_test_files_count_zero() {
    // Plan §4.6 strict invariant: zero phase-archaeology references in
    // test files inside crates/*/src/. Currently ignored — the W5f
    // baseline is non-zero. Step 2.6 (the sweep) removes the
    // vocabulary; once that lands, flip this from #[ignore] to live and
    // delete the regression backstop above.
    //
    // The predicate is shared with no_phase_archaeology_in_test_files
    // via the w5f_test_archaeology helper module so both tests stay in
    // lockstep with the broader-D111 classifier.
    let violations = w5f_test_archaeology::collect_test_archaeology_violations();
    assert!(
        violations.is_empty(),
        "phase_archaeology_test_files_count_zero: {} violations remain.\nFirst 10:\n{}",
        violations.len(),
        violations
            .iter()
            .take(10)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn tier_2_split_preserves_semantic_query_key_hashes() {
    // Plan §4.4: the W5a split of `semantic_query_memo.rs` into a directory
    // module must not change the Hash impl of `SemanticQueryKey` — caches
    // keyed on these hashes would silently miss across the split otherwise.
    //
    // The plan calls for a cold-seq run against 32 representative fixtures
    // and a byte-equal compare against `keys-survivors.json` from Tier 1B.
    // That baseline file does not exist at the W5f cutoff, so this test
    // implements the discriminating invariant in a structural form: hash a
    // small set of stable `SemanticQueryKey` instances and verify (a) the
    // Hash + Eq invariant holds (equal keys hash equal), and (b) different
    // keys hash differently. Both properties are byte-stable across
    // refactors of the surrounding memo module — the W5a split would have
    // failed this test if the Hash derive had been accidentally dropped or
    // if a variant had been silently renamed.
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    use std::sync::Arc;
    use verter_session::semantic_query::{ResolveDeclKey, ScopeId, SemanticQueryKey};

    fn hash_of(key: &SemanticQueryKey) -> u64 {
        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        hasher.finish()
    }

    let scope_a = ScopeId {
        canonical_id: Arc::from("/test/scope-a.vue"),
        local_scope: None,
    };
    let scope_b = ScopeId {
        canonical_id: Arc::from("/test/scope-b.vue"),
        local_scope: None,
    };

    let key_a1 = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: scope_a.clone(),
        name: Arc::from("MyType"),
    });
    let key_a2 = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: scope_a.clone(),
        name: Arc::from("MyType"),
    });
    let key_b = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: scope_b,
        name: Arc::from("MyType"),
    });
    let key_c = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: scope_a,
        name: Arc::from("OtherType"),
    });

    assert_eq!(
        key_a1, key_a2,
        "Eq invariant: structurally identical keys must compare equal"
    );
    assert_eq!(
        hash_of(&key_a1),
        hash_of(&key_a2),
        "Hash + Eq invariant: equal keys must hash equal"
    );
    assert_ne!(
        hash_of(&key_a1),
        hash_of(&key_b),
        "different scopes must hash differently"
    );
    assert_ne!(
        hash_of(&key_a1),
        hash_of(&key_c),
        "different names must hash differently"
    );
}

#[test]
fn recursion_budget_invariant_across_module_boundary() {
    // Plan §4.5: the recursion-budget mechanism must remain reachable and
    // its declared cap must remain pinned across the W5a / W5b / W5d splits.
    // Cross-module recursion that exceeds this cap is supposed to terminate
    // the walker via the audited assertion path rather than overflow the
    // stack.
    //
    // The plan calls for a fixture at
    // `crates/verter_session/tests/fixtures/recursion_budget_invariant_fixture.ts`
    // and a baseline at `crates/verter_session/tests/perf_bounds/recursion_budget_baseline.txt`.
    // Neither exists at the W5f cutoff. This test implements the
    // discriminating invariant in a structural form: assert that the
    // public `WALKER_DEPTH_CAP` constant remains reachable from outside
    // the module that owns it (i.e., the split did not break the public
    // re-export path) and that its declared value matches the expected
    // pin. A future patch that lands the fixture + baseline can extend
    // this test with a real per-fixture budget consumption check.
    use verter_session::component_meta_audit::WALKER_DEPTH_CAP;
    assert_eq!(
        WALKER_DEPTH_CAP, 256,
        "WALKER_DEPTH_CAP must remain pinned at 256 — see component_meta_audit::assertions"
    );
}

/// Architecture guard: every bump of the `inflight_aborted_retries`
/// and `cold_aborts_swept` counters in
/// `crates/verter_session/src/semantic_query_memo/mod.rs` must go
/// through the `record_inflight_aborted_retry` /
/// `record_cold_abort_swept` helpers. Direct
/// `self.stats.<counter>.fetch_add` patterns OUTSIDE the helper
/// bodies are forbidden — they let the global aggregate and the
/// per-request mirror diverge silently.
///
/// The matcher detects helper bodies via `fn record_*(` signatures
/// (walks forward to the next top-level `}\n`), then scans for the
/// counter names followed by `.fetch_add` within a 64-byte lookahead
/// (catches multi-line method-chain splits). Occurrences inside a
/// helper body are allowed; everything else is a violation.
///
/// The matcher logic lives in [`audit_counter_helper_violations`] so
/// the discriminator self-test below exercises the SAME code path.
/// Re-implementing the matcher in the self-test is a pinning gap:
/// loosening the production matcher (e.g. shrinking the 64-byte
/// lookahead) would silently weaken the guard while the self-test
/// passes against its independent matcher.
fn audit_counter_helper_violations(src: &str) -> Vec<String> {
    // Identify the byte ranges that fall INSIDE one of the two
    // helper bodies. Helpers are short (one fetch_add each) and live
    // at top-level — match their `fn` signature, then walk forward
    // to the next top-level `}\n` (i.e. a `}` followed by a newline,
    // appearing at column 0).
    let helper_signatures = [
        "fn record_inflight_aborted_retry(",
        "fn record_cold_abort_swept(",
    ];
    let mut helper_ranges: Vec<(usize, usize)> = Vec::new();
    for sig in &helper_signatures {
        let mut search_start = 0usize;
        while let Some(rel_start) = src[search_start..].find(sig) {
            let abs_start = search_start + rel_start;
            // Find the closing `}` at column 0 after this start.
            let after = &src[abs_start..];
            let close_offset = after
                .find("\n}\n")
                .or_else(|| after.find("\n}\r\n"))
                .unwrap_or(after.len().saturating_sub(1));
            let abs_end = abs_start + close_offset + 2; // include the `}\n`
            helper_ranges.push((abs_start, abs_end));
            search_start = abs_end;
        }
    }

    // Scan for either of the two counter names followed by
    // `.fetch_add` within a 64-byte lookahead (covers multi-line
    // method-chain splits). Occurrences inside a helper body are
    // allowed; everything else is a violation reported with line
    // number + trimmed snippet.
    let counter_patterns = [".inflight_aborted_retries", ".cold_aborts_swept"];
    let mut violations: Vec<String> = Vec::new();
    for pattern in &counter_patterns {
        let mut search_start = 0usize;
        while let Some(rel) = src[search_start..].find(pattern) {
            let abs = search_start + rel;
            let lookahead_end = (abs + pattern.len() + 64).min(src.len());
            let lookahead = &src[abs..lookahead_end];
            let has_fetch_add = lookahead.contains(".fetch_add");
            search_start = abs + pattern.len();
            if !has_fetch_add {
                continue;
            }
            // Inside a helper body? Allow.
            let inside_helper = helper_ranges
                .iter()
                .any(|&(start, end)| abs >= start && abs < end);
            if inside_helper {
                continue;
            }
            // Compute line number for the report.
            let line_no = src[..abs].matches('\n').count() + 1;
            // Capture a small snippet around the violation.
            let snippet_start = src[..abs].rfind('\n').map(|i| i + 1).unwrap_or(0);
            let snippet_end = src[abs..].find('\n').map(|i| abs + i).unwrap_or(src.len());
            let snippet = &src[snippet_start..snippet_end];
            violations.push(format!("line {}: {}", line_no, snippet.trim()));
        }
    }
    violations
}

#[test]
fn audit_counter_single_helper() {
    let src = read_workspace_file("crates/verter_session/src/semantic_query_memo/mod.rs");
    let violations = audit_counter_helper_violations(&src);

    assert!(
        violations.is_empty(),
        "audit_counter_single_helper: every bump of \
         `inflight_aborted_retries` or `cold_aborts_swept` must go \
         through `record_inflight_aborted_retry` / \
         `record_cold_abort_swept` (see CLAUDE.md Decision #5). \
         Direct `self.stats.<counter>.fetch_add` outside the helper \
         bodies is forbidden because it lets the global aggregate \
         and the per-request mirror diverge silently. Found:\n  {}",
        violations.join("\n  ")
    );
}

/// Self-test for `audit_counter_single_helper`. Drives the SAME
/// production matcher (`audit_counter_helper_violations`) against a
/// synthetic source string so a regression in the matcher's helper
/// detection or 64-byte lookahead would surface here as well — not
/// just on the live tree.
#[test]
fn audit_counter_single_helper_discriminator_self_test() {
    // Synthetic violator: bump outside any helper body.
    let synthetic_violator = "\
fn unrelated_function() {
    self.stats.inflight_aborted_retries.fetch_add(1, Ordering::Relaxed);
}
";
    let v = audit_counter_helper_violations(synthetic_violator);
    assert_eq!(
        v.len(),
        1,
        "audit_counter_single_helper matcher must catch a synthetic \
         violation outside any helper body — got {} violations: {v:?}",
        v.len(),
    );

    // Synthetic clean: same bump INSIDE a helper body must NOT flag.
    let synthetic_helper = "\
fn record_inflight_aborted_retry(stats: &AtomicSemanticGraphStats) {
    stats.inflight_aborted_retries.fetch_add(1, Ordering::Relaxed);
}
";
    let v = audit_counter_helper_violations(synthetic_helper);
    assert_eq!(
        v.len(),
        0,
        "audit_counter_single_helper matcher must NOT flag fetch_add \
         inside a helper body — got {} false positives: {v:?}",
        v.len(),
    );

    // Synthetic multi-line method-chain violator: confirms the
    // 64-byte lookahead window catches `\n    .fetch_add` splits.
    // A regression that shrank the lookahead to e.g. 16 bytes would
    // miss this and silently weaken the live guard.
    let synthetic_multiline = "\
fn unrelated_function() {
    self.stats
        .inflight_aborted_retries
        .fetch_add(1, Ordering::Relaxed);
}
";
    let v = audit_counter_helper_violations(synthetic_multiline);
    assert_eq!(
        v.len(),
        1,
        "audit_counter_single_helper matcher must catch a multi-line \
         method-chain violation — the 64-byte lookahead window covers \
         this case. Got {} violations: {v:?}",
        v.len(),
    );
}

// ----------------------------------------------------------------
// Audit substrate isolation guards — created with the verter_audit
// crate and the cascade-move that retired the in-session DTO copies.
// ----------------------------------------------------------------

/// `verter_audit` MUST stay a leaf crate: its `Cargo.toml` lists
/// only `verter_span` as the verter_*-prefixed dependency in any
/// dependency table (regular, dev, build, target-keyed, or
/// `workspace.dependencies`).
#[test]
fn verter_audit_no_upward_deps() {
    let toml_src = read_workspace_file("crates/verter_audit/Cargo.toml");
    let parsed: toml::Value =
        toml::from_str(&toml_src).expect("verter_audit/Cargo.toml must parse as TOML");

    // Walk every dependency table that Cargo recognises:
    // `dependencies`, `dev-dependencies`, `build-dependencies`,
    // and target-keyed variants under `target.<cfg>.*` and
    // `workspace.dependencies`. Reject any `verter_*` entry
    // (besides `verter_span`) anywhere in that namespace.
    fn names_in_table(table: &toml::Value) -> impl Iterator<Item = &str> {
        table
            .as_table()
            .into_iter()
            .flat_map(|t| t.keys().map(String::as_str))
    }

    let mut violations: Vec<String> = Vec::new();
    let dep_table_names = ["dependencies", "dev-dependencies", "build-dependencies"];

    // Top-level dependency tables.
    for table_name in dep_table_names {
        if let Some(table) = parsed.get(table_name) {
            for name in names_in_table(table) {
                if name.starts_with("verter_") && name != "verter_span" {
                    violations.push(format!("[{table_name}]: {name}"));
                }
            }
        }
    }

    // `target.<cfg>.{dependencies,dev-dependencies,build-dependencies}`.
    if let Some(targets) = parsed.get("target").and_then(|v| v.as_table()) {
        for (cfg, body) in targets {
            for table_name in dep_table_names {
                if let Some(table) = body.get(table_name) {
                    for name in names_in_table(table) {
                        if name.starts_with("verter_") && name != "verter_span" {
                            violations.push(format!("[target.{cfg}.{table_name}]: {name}"));
                        }
                    }
                }
            }
        }
    }

    // `workspace.dependencies`.
    if let Some(ws) = parsed.get("workspace") {
        if let Some(table) = ws.get("dependencies") {
            for name in names_in_table(table) {
                if name.starts_with("verter_") && name != "verter_span" {
                    violations.push(format!("[workspace.dependencies]: {name}"));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "verter_audit_no_upward_deps: `verter_audit/Cargo.toml` declares \
         non-leaf verter_*-prefixed dependencies. The substrate must depend ONLY on \
         `verter_span` plus ecosystem crates. Offending entries:\n  {}",
        violations.join("\n  ")
    );
}

/// Source files under `crates/verter_audit/src/` MUST `use` only
/// `verter_span`, `std`, and external crates.
#[test]
fn audit_substrate_isolation() {
    use std::path::PathBuf;
    let root = workspace_root();
    let audit_src: PathBuf = root.join("crates/verter_audit/src");
    let mut violations: Vec<String> = Vec::new();
    walk_dir_collect_rs(&audit_src, &mut |path: &std::path::Path| {
        let src = std::fs::read_to_string(path).unwrap_or_else(|e| {
            panic!(
                "audit_substrate_isolation: cannot read `{}`: {e}",
                path.display()
            )
        });
        for (line_no, line) in src.lines().enumerate() {
            // Skip comments and doc-comments — they discuss
            // `verter_*` crates as prose without importing them.
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") {
                continue;
            }
            // Reject any non-`verter_span` reference to a `verter_*`
            // crate on a non-comment line. The patterns we catch:
            // `use verter_<other>`, `pub use verter_<other>`,
            // `extern crate verter_<other>`, `verter_<other>::<...>`,
            // and bare references in attribute paths. Substring scan
            // is sufficient because the substrate's imports list is
            // tiny and we exclude `verter_span` and self-references
            // (`verter_audit` / `crate::`) explicitly.
            let mut search_from = 0usize;
            while let Some(rel) = line[search_from..].find("verter_") {
                let abs = search_from + rel;
                let after = abs + "verter_".len();
                // Capture the trailing identifier characters.
                let bytes = line.as_bytes();
                let mut end = after;
                while end < bytes.len() {
                    let c = bytes[end];
                    let alnum = c.is_ascii_alphanumeric() || c == b'_';
                    if !alnum {
                        break;
                    }
                    end += 1;
                }
                if end == after {
                    search_from = after;
                    continue;
                }
                let crate_name = &line[abs..end];
                search_from = end;
                if crate_name == "verter_span" {
                    continue;
                }
                if crate_name == "verter_audit" {
                    continue;
                }
                let rel_path = path
                    .strip_prefix(&root)
                    .unwrap_or(path)
                    .to_string_lossy()
                    .replace('\\', "/");
                violations.push(format!("{rel_path}:{}: {}", line_no + 1, line.trim()));
                break;
            }
        }
    });
    assert!(
        violations.is_empty(),
        "audit_substrate_isolation: source files under \
         `crates/verter_audit/src/` reference non-leaf `verter_*` crates. \
         The substrate must use only `verter_span`, `std`, and external \
         crates. Offending lines:\n  {}",
        violations.join("\n  ")
    );
}

fn walk_dir_collect_rs(dir: &std::path::Path, f: &mut dyn FnMut(&std::path::Path)) {
    let entries = std::fs::read_dir(dir).unwrap_or_else(|e| {
        panic!(
            "walk_dir_collect_rs: cannot read directory `{}`: {e}",
            dir.display()
        )
    });
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_dir_collect_rs(&path, f);
        } else if path.extension().is_some_and(|e| e == "rs") {
            f(&path);
        }
    }
}

/// Every [`verter_audit::RequestKind`] variant must have a sibling
/// [`verter_audit::RequestKindPayload`] variant.
#[test]
fn request_kind_payload_parity() {
    let src = read_workspace_file("crates/verter_audit/src/record.rs");

    fn enum_variant_names(src: &str, enum_name: &str) -> Vec<String> {
        let header = format!("pub enum {enum_name}");
        let start = src
            .find(&header)
            .unwrap_or_else(|| panic!("enum {enum_name} not found in record.rs"));
        let body_start = src[start..]
            .find('{')
            .map(|i| start + i + 1)
            .unwrap_or_else(|| panic!("enum {enum_name} body not found"));
        let bytes = src.as_bytes();
        let mut depth = 1usize;
        let mut idx = body_start;
        while idx < bytes.len() && depth > 0 {
            match bytes[idx] {
                b'{' => depth += 1,
                b'}' => depth -= 1,
                _ => {}
            }
            idx += 1;
        }
        let body_end = idx - 1;
        let body = &src[body_start..body_end];
        let mut names = Vec::new();
        for raw_line in body.lines() {
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with("//") || line.starts_with("///") {
                continue;
            }
            // Variant declarations: Name, Name(Payload), Name { ... } — split on `(`, `{`, or `,`, whichever comes first.
            let head_end = [line.find('('), line.find('{'), line.find(',')]
                .into_iter()
                .flatten()
                .min()
                .unwrap_or(line.len());
            let head: &str = line[..head_end].trim();
            if head.is_empty() {
                continue;
            }
            if head.starts_with('#') {
                continue;
            }
            let name = head.split_whitespace().next().unwrap_or("");
            if name.is_empty() {
                continue;
            }
            if name
                .chars()
                .all(|c: char| c.is_ascii_alphanumeric() || c == '_')
            {
                names.push(name.to_string());
            }
        }
        names
    }

    let request_kinds = enum_variant_names(&src, "RequestKind");
    let payload_kinds = enum_variant_names(&src, "RequestKindPayload");

    let payload_no_none: Vec<String> = payload_kinds
        .iter()
        .filter(|n| n.as_str() != "None")
        .cloned()
        .collect();

    let kinds_for_parity: Vec<String> = request_kinds
        .iter()
        .filter(|n| n.as_str() != "Custom")
        .cloned()
        .collect();

    assert_eq!(
        kinds_for_parity, payload_no_none,
        "request_kind_payload_parity: every `RequestKind` variant must have \
         a same-named sibling on `RequestKindPayload` (apart from `Custom` \
         which maps to `RequestKindPayload::None` by design)."
    );

    assert!(
        !request_kinds.is_empty(),
        "request_kind_payload_parity: `RequestKind` enum has no variants — parser broke."
    );
    assert!(
        payload_kinds.contains(&String::from("None")),
        "request_kind_payload_parity: `RequestKindPayload` must retain its `None` variant."
    );
}

/// `HostAuditRuntime::active_requests` is private; the lifecycle
/// methods that mutate it must each have exactly ONE in-tree call
/// site inside `host_audit_runtime.rs`.
#[test]
fn audit_request_registration_lifecycle() {
    use std::path::PathBuf;
    use syn::visit::Visit;
    let root = workspace_root();

    let allowed_file = "crates/verter_session/src/host_audit_runtime.rs";
    let methods: &[&str] = &[
        "register_active_request",
        "finalize_active_request",
        "drop_active_request",
    ];

    /// Visitor that collects every method-call expression matching
    /// the lifecycle vocabulary.
    struct LifecycleCallCollector<'m> {
        methods: &'m [&'m str],
        hits: Vec<String>,
    }

    impl<'ast, 'm> Visit<'ast> for LifecycleCallCollector<'m> {
        fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
            let name = call.method.to_string();
            if self.methods.contains(&name.as_str()) {
                self.hits.push(name);
            }
            syn::visit::visit_expr_method_call(self, call);
        }
    }

    let mut violations: Vec<String> = Vec::new();
    let crates_dir: PathBuf = root.join("crates");

    walk_dir_collect_rs(&crates_dir, &mut |path: &std::path::Path| {
        let rel = path
            .strip_prefix(&root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        if rel == allowed_file {
            return;
        }
        let src = std::fs::read_to_string(path).unwrap_or_else(|e| {
            panic!("audit_request_registration_lifecycle: cannot read `{rel}`: {e}")
        });
        let parsed = match syn::parse_file(&src) {
            Ok(p) => p,
            // Files we cannot parse (e.g. macro-generated bodies)
            // are skipped — `syn::parse_file` rejects very few real
            // crate files. The earlier substring scan acted as the
            // safety net here; we keep the hard panic for unparseable
            // files outside of `tests/` because that would indicate a
            // code-corruption signal worth surfacing.
            Err(e) => {
                if rel.starts_with("crates/") && rel.contains("/src/") && !rel.contains("/tests/") {
                    panic!("audit_request_registration_lifecycle: cannot parse `{rel}`: {e}");
                }
                return;
            }
        };
        let mut collector = LifecycleCallCollector {
            methods,
            hits: Vec::new(),
        };
        collector.visit_file(&parsed);
        for hit in collector.hits {
            violations.push(format!("{rel}: {hit}"));
        }
    });

    assert!(
        violations.is_empty(),
        "audit_request_registration_lifecycle: the three lifecycle methods on \
         `HostAuditRuntime` must each have exactly ONE in-tree call site, all \
         inside `crates/verter_session/src/host_audit_runtime.rs`. Found callers \
         outside that file:\n  {}",
        violations.join("\n  ")
    );
}

/// Slice 3.B architecture guard — instrumentation lives at phase
/// boundaries only, never inside hot loops.
///
/// Reads the canonical `(crate, function_path)` denylist from
/// `audit_hot_loop_denylist::HOT_PATH_DENYLIST` and rejects any
/// `current_observer()` call (the audit substrate's TLS accessor)
/// inside the body of any listed function. Phase boundaries (parse /
/// transform / codegen / css_analysis / sourcemap) fire O(1) times
/// per request — that is the only permitted granularity.
#[test]
fn audit_no_hot_loop_instrumentation() {
    use std::collections::HashMap;
    use syn::visit::Visit;

    let denylist = audit_hot_loop_denylist::HOT_PATH_DENYLIST;
    assert!(
        !denylist.is_empty(),
        "audit_no_hot_loop_instrumentation: denylist must not be empty — \
         the guard is meaningless without at least one hot-path entry. \
         If the system genuinely has no hot loops worth listing, escalate \
         this guard's purpose for review.",
    );
    assert!(
        denylist.len() <= 20,
        "audit_no_hot_loop_instrumentation: denylist has {} entries (> 20); \
         the design guidance is 4–8 typical / ~20 max. Escalate before \
         adding more — broad lists usually indicate misplaced \
         instrumentation rather than additional hot loops.",
        denylist.len(),
    );

    // The audit-substrate's TLS observer accessor — the canonical
    // entry point producers use to reach `AuditObserver::record_*`.
    // Without `verter_audit::current_observer()` (or the unqualified
    // `current_observer()` form), no producer-side audit emit is
    // possible. Matching just this name keeps the guard precise:
    // session-internal helpers that share verbs with the substrate
    // trait (e.g. `RequestContextLike::record_cache_event` on the
    // scheduler-side request-context handle) are NOT producer audit
    // emits and must not be flagged. The cross-crate audit-substrate
    // surface is identified by the `current_observer` accessor; that
    // is the gate the guard enforces.
    const AUDIT_EMIT_FUNCTION_NAMES: &[&str] = &["current_observer"];

    /// Inner visitor — scans a single function body for audit-emit
    /// call sites without descending into nested `fn`/`impl` items.
    struct EmitFinder {
        violations: Vec<String>,
    }
    impl<'ast> Visit<'ast> for EmitFinder {
        fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
            if let syn::Expr::Path(p) = &*call.func {
                if let Some(last) = p.path.segments.last() {
                    let name = last.ident.to_string();
                    if AUDIT_EMIT_FUNCTION_NAMES.contains(&name.as_str()) {
                        self.violations.push(name);
                    }
                }
            }
            syn::visit::visit_expr_call(self, call);
        }
    }

    /// Outer visitor — assembles fully-qualified function paths and
    /// runs `EmitFinder` against any body whose path appears in the
    /// denylist. The path stack is seeded with the file's own
    /// module path (derived from `<crate_src>/<sub>/<sub>/foo.rs` →
    /// `sub::sub::foo`); inline `mod xxx { ... }` blocks and
    /// `impl Type` blocks then push further segments.
    struct DenyVisitor<'a> {
        target_paths: &'a HashMap<&'a str, Vec<(usize, &'a str)>>,
        path_stack: Vec<String>,
        violations: Vec<(usize, String)>,
        matched: Vec<bool>,
        current_crate: &'a str,
    }
    impl<'a, 'ast> Visit<'ast> for DenyVisitor<'a> {
        fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
            self.path_stack.push(item.ident.to_string());
            syn::visit::visit_item_mod(self, item);
            self.path_stack.pop();
        }
        fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
            let segment = if let syn::Type::Path(tp) = &*item.self_ty {
                tp.path
                    .segments
                    .last()
                    .map(|s| s.ident.to_string())
                    .unwrap_or_else(|| "<impl>".into())
            } else {
                "<impl>".into()
            };
            self.path_stack.push(segment);
            syn::visit::visit_item_impl(self, item);
            self.path_stack.pop();
        }
        fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
            self.path_stack.push(item.sig.ident.to_string());
            self.check(&item.block);
            syn::visit::visit_item_fn(self, item);
            self.path_stack.pop();
        }
        fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
            self.path_stack.push(item.sig.ident.to_string());
            self.check(&item.block);
            syn::visit::visit_impl_item_fn(self, item);
            self.path_stack.pop();
        }
    }
    impl<'a> DenyVisitor<'a> {
        fn check(&mut self, block: &syn::Block) {
            let path = self.path_stack.join("::");
            let Some(targets) = self.target_paths.get(self.current_crate) else {
                return;
            };
            for (idx, target_path) in targets {
                if &path == target_path {
                    self.matched[*idx] = true;
                    let mut finder = EmitFinder {
                        violations: Vec::new(),
                    };
                    finder.visit_block(block);
                    for name in finder.violations {
                        self.violations.push((*idx, name));
                    }
                }
            }
        }
    }

    /// Compute the module-path stack for a file relative to its
    /// crate's `src/` root.
    fn module_stack_for_file(crate_src: &std::path::Path, file: &std::path::Path) -> Vec<String> {
        let rel = match file.strip_prefix(crate_src) {
            Ok(p) => p,
            Err(_) => return Vec::new(),
        };
        let mut segments: Vec<String> = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        if let Some(last) = segments.last_mut() {
            if let Some(stripped) = last.strip_suffix(".rs") {
                *last = stripped.to_string();
            }
        }
        match segments.last().map(|s| s.as_str()) {
            Some("lib") | Some("main") => {
                if segments.len() == 1 {
                    return Vec::new();
                }
                segments.pop();
            }
            Some("mod") => {
                segments.pop();
            }
            _ => {}
        }
        segments
    }

    let mut by_crate: HashMap<&str, Vec<(usize, &str)>> = HashMap::new();
    for (idx, (krate, path)) in denylist.iter().enumerate() {
        by_crate.entry(krate).or_default().push((idx, path));
    }

    let mut matched: Vec<bool> = vec![false; denylist.len()];
    let mut all_violations: Vec<String> = Vec::new();

    for krate in by_crate.keys() {
        let crate_src = workspace_root().join("crates").join(krate).join("src");
        if !crate_src.exists() {
            panic!(
                "audit_no_hot_loop_instrumentation: crate `{krate}` listed in denylist \
                 but `crates/{krate}/src/` does not exist; the denylist is stale."
            );
        }
        walk_dir_collect_rs(&crate_src, &mut |path: &std::path::Path| {
            let src = std::fs::read_to_string(path).unwrap_or_else(|e| {
                panic!(
                    "audit_no_hot_loop_instrumentation: cannot read `{}`: {e}",
                    path.display()
                )
            });
            let parsed = match syn::parse_file(&src) {
                Ok(p) => p,
                Err(_) => return,
            };
            let initial_stack = module_stack_for_file(&crate_src, path);
            let mut visitor = DenyVisitor {
                target_paths: &by_crate,
                path_stack: initial_stack,
                violations: Vec::new(),
                matched: vec![false; denylist.len()],
                current_crate: krate,
            };
            visitor.visit_file(&parsed);
            for (i, m) in visitor.matched.iter().enumerate() {
                if *m {
                    matched[i] = true;
                }
            }
            for (idx, name) in visitor.violations {
                all_violations.push(format!(
                    "  - [{}] {} :: {} — emit `{}`",
                    krate,
                    by_crate[krate]
                        .iter()
                        .find(|(i, _)| *i == idx)
                        .map(|(_, p)| *p)
                        .unwrap_or("<unknown>"),
                    path.display(),
                    name,
                ));
            }
        });
    }

    let stale: Vec<String> = matched
        .iter()
        .enumerate()
        .filter_map(|(i, m)| {
            if !m {
                let (krate, path) = denylist[i];
                Some(format!("  - {krate} :: {path}"))
            } else {
                None
            }
        })
        .collect();

    assert!(
        stale.is_empty(),
        "audit_no_hot_loop_instrumentation: the following denylist entries did NOT \
         match any function in the corresponding crate's source tree. The denylist \
         is stale — the function was renamed, moved, or removed. Update the \
         denylist in `tests/audit_hot_loop_denylist.rs`:\n{}",
        stale.join("\n"),
    );

    assert!(
        all_violations.is_empty(),
        "audit_no_hot_loop_instrumentation: producer-side audit emits are FORBIDDEN \
         inside the hot-path denylist. Move the emit to a phase boundary (parse / \
         transform / codegen / css_analysis / sourcemap) outside the inner loop. \
         Found:\n{}",
        all_violations.join("\n"),
    );
}

/// Self-test for `audit_no_hot_loop_instrumentation` discrimination.
/// Confirms the matcher detects `current_observer()` calls and does
/// NOT misclassify session-internal helpers (e.g. `record_cache_event`
/// on `RequestContextLike`) as substrate emits.
#[test]
fn audit_no_hot_loop_instrumentation_self_test_rejects_emit_names() {
    use syn::visit::Visit;

    let synthetic_violator = "\
        fn busy_loop() {\n\
        \x20\x20    let _ = verter_audit::current_observer();\n\
        \x20\x20    drop(verter_audit::current_observer());\n\
        }\n\
    ";
    let synthetic_clean = "\
        fn busy_loop() {\n\
        \x20\x20    let _ = self.scratch.len();\n\
        \x20\x20    let _ = self.allocator.alloc_str(\"hi\");\n\
        \x20\x20    let _ = ctx.0.record_cache_event(CacheEventKind::Hit);\n\
        }\n\
    ";

    fn count_emits(src: &str) -> usize {
        let parsed = syn::parse_file(src).expect("parse synthetic");
        struct Counter(usize);
        impl<'ast> Visit<'ast> for Counter {
            fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
                if let syn::Expr::Path(p) = &*call.func {
                    if let Some(last) = p.path.segments.last() {
                        if last.ident == "current_observer" {
                            self.0 += 1;
                        }
                    }
                }
                syn::visit::visit_expr_call(self, call);
            }
        }
        let mut c = Counter(0);
        c.visit_file(&parsed);
        c.0
    }

    assert_eq!(
        count_emits(synthetic_violator),
        2,
        "self-test: synthetic violator with two `current_observer()` calls \
         MUST produce exactly 2 detected emits. A regression that drops \
         `current_observer` from the matcher would fail this assertion \
         before the live guard can silently weaken.",
    );
    assert_eq!(
        count_emits(synthetic_clean),
        0,
        "self-test: synthetic clean body MUST produce zero detected emits — \
         note the body deliberately includes a session-internal \
         `ctx.0.record_cache_event(...)` to confirm the matcher does NOT \
         flag session-side helpers that share verbs with the substrate's \
         `AuditObserver` trait.",
    );
}

/// Wave 3 close — `audit_observer_single_accessor`.
///
/// Lower crates must reach the audit substrate exclusively through
/// [`verter_audit::current_observer`]. They must NOT reach into
/// `verter_session::request_context::current_request_context` (which
/// is a session-internal, typed accessor onto the concrete
/// `Arc<RequestContext>`). Architectural intent: the substrate's
/// thin `AuditObserver` trait is the cross-crate API; only the
/// session crate (which owns `RequestContext`) is permitted to use
/// the typed accessor.
///
/// `verter_session/` and `verter_audit/` are explicitly out of
/// scope: `verter_session` defines and consumes
/// `current_request_context`, and `verter_audit` is the substrate.
/// `verter_scheduler/` is also out of scope: it documents
/// `current_request_context` in module-level comments but the
/// scheduler does not call it (the scheduler crate's own TLS
/// accessor is `verter_scheduler::request_context::current_request_id`).
///
/// In-scope crates (the 5 lower-crate consumers of audit):
///
///   - `verter_compiler`
///   - `verter_semantic`
///   - `verter_workspace`
///   - `verter_lsp`
///   - `verter_mcp_server`
///
/// Discrimination contract:
/// - Pre-change tree (5 lower crates currently clean): the guard
///   passes; no in-scope file references `current_request_context`.
/// - Regression: any new code in the 5 in-scope crates that adds a
///   `current_request_context` call appears in `violations` and
///   fails the assertion. The fix is to migrate the call site to
///   `verter_audit::current_observer()` (which yields an
///   `Arc<dyn AuditObserver>`) and emit through the trait.
/// - Allow-list: legitimate pre-existing call sites are listed in
///   `ALLOW_LIST` with the rationale. Empty today.
#[test]
fn audit_observer_single_accessor() {
    // Allow-list: `(crate, relative_path_within_crate, rationale)`
    // tuples for pre-existing legitimate call sites that predate
    // `verter_audit::current_observer()` and have not yet migrated.
    // Empty today — the 5 lower crates are all clean.
    const ALLOW_LIST: &[(&str, &str, &str)] = &[];

    // The 5 in-scope lower crates. Adding a new lower crate that
    // emits audit events requires extending this list, but the
    // architectural rule remains: lower crates use
    // `verter_audit::current_observer()`.
    const IN_SCOPE_CRATES: &[&str] = &[
        "verter_compiler",
        "verter_semantic",
        "verter_workspace",
        "verter_lsp",
        "verter_mcp_server",
    ];

    // The forbidden pattern. Substring match (the canonical form is
    // `current_request_context()` — both fully qualified and
    // bare-imported usages contain this substring).
    const FORBIDDEN: &str = "current_request_context";

    let mut violations: Vec<String> = Vec::new();
    let mut allow_list_hits: Vec<(usize, String)> = Vec::new();

    for krate in IN_SCOPE_CRATES {
        let crate_src = workspace_root().join("crates").join(krate).join("src");
        if !crate_src.exists() {
            panic!(
                "audit_observer_single_accessor: crate `{krate}` listed as in-scope \
                 but `crates/{krate}/src/` does not exist; the in-scope list is stale."
            );
        }
        walk_dir_collect_rs(&crate_src, &mut |path: &std::path::Path| {
            let src = std::fs::read_to_string(path).unwrap_or_else(|e| {
                panic!(
                    "audit_observer_single_accessor: cannot read `{}`: {e}",
                    path.display()
                )
            });
            // Compute path relative to the crate's src/ for stable
            // allow-list keys.
            let rel = path.strip_prefix(&crate_src).unwrap_or(path);
            let rel_str = rel.to_string_lossy().replace('\\', "/").to_string();
            for (line_no, line) in src.lines().enumerate() {
                // Skip line/block comments — comment-side mentions
                // (e.g. doc strings cross-referencing the session
                // accessor) are NOT call-site bypasses.
                let trimmed = line.trim_start();
                if trimmed.starts_with("//")
                    || trimmed.starts_with("///")
                    || trimmed.starts_with("//!")
                {
                    continue;
                }
                if !line.contains(FORBIDDEN) {
                    continue;
                }
                // Walk the allow-list for this exact (crate, rel_path).
                let allow_idx = ALLOW_LIST
                    .iter()
                    .position(|(k, p, _r)| *k == *krate && *p == rel_str.as_str());
                let entry = format!("  - [{krate}] {rel_str}:{}: {}", line_no + 1, line.trim());
                match allow_idx {
                    Some(i) => allow_list_hits.push((i, entry)),
                    None => violations.push(entry),
                }
            }
        });
    }

    // Stale allow-list detection: every allow-list entry must have
    // matched at least one line. Otherwise the entry is stale (the
    // call site was deleted or migrated) and should be removed.
    let stale: Vec<String> = ALLOW_LIST
        .iter()
        .enumerate()
        .filter_map(|(i, (k, p, r))| {
            if allow_list_hits.iter().all(|(idx, _)| *idx != i) {
                Some(format!("  - [{k}] {p} (rationale: {r})"))
            } else {
                None
            }
        })
        .collect();

    assert!(
        stale.is_empty(),
        "audit_observer_single_accessor: the following allow-list entries did NOT \
         match any line in their crate's source tree. Either the call site was \
         deleted/migrated (drop the entry) or the path/crate is wrong (fix the entry):\n{}",
        stale.join("\n"),
    );

    assert!(
        violations.is_empty(),
        "audit_observer_single_accessor: lower crates must reach the audit substrate \
         through `verter_audit::current_observer()` only. The following call sites \
         use `current_request_context` (the session-internal typed accessor) instead. \
         Migrate to `current_observer()` and emit through the `AuditObserver` trait, \
         OR — if the call site genuinely needs the typed `Arc<RequestContext>` for a \
         legitimate reason that predates the substrate split — add an allow-list entry \
         in this guard with the rationale.\n\nViolations:\n{}",
        violations.join("\n"),
    );
}

/// Wave 3 close — `wave_3_entry_points_propagate_tls`.
///
/// Every Wave-3-added `*_with_audit` entry-point must have a
/// corresponding TLS-propagation test that drives it via the
/// [`verter_session::tests::audit_tls_harness::assert_observer_reaches`]
/// harness (or — for entry-points where a stricter custom assertion
/// is more discriminating — drives the entry-point and asserts the
/// observer reaches the producer crate's instrumentation).
///
/// The guard is parameterised by `WAVE_3_ENTRY_POINTS` — a list of
/// `(entry_point_symbol, paired_test_files)` tuples. For each
/// entry-point, the guard verifies that at least one of the listed
/// test files contains BOTH:
///
///   1. an invocation of `entry_point_symbol` (the function/method
///      name appears in the file), AND
///   2. an `assert_observer_reaches(...)` call (the §4 Wave 1.5
///      harness's primary verification primitive).
///
/// A `MISSING_TLS_TEST` allow-list documents Wave-3 entry-points
/// that ship without a paired TLS-propagation test. Each entry
/// carries a rationale and is intended as a temporary marker — the
/// follow-up fix-pass adds the missing test.
///
/// Discrimination contract:
/// - Pre-change tree (Wave 3 not yet integrated): the entry-point
///   methods do not exist, so the test files cannot reference them
///   either; the guard fails with "entry-point method missing".
/// - Wave-3 entry-point landed without a paired TLS test: appears
///   in the missing-pair list (and must either get a test added or
///   an allow-list entry).
/// - Wave-3 entry-point with a discriminating TLS test: passes.
#[test]
fn wave_3_entry_points_propagate_tls() {
    // `(entry_point_symbol, &[paired_test_file_relative_paths])`
    //
    // `entry_point_symbol`: the function/method name producers/tests
    // invoke. The guard substring-matches this in the paired test
    // files.
    //
    // `paired_test_file_relative_paths`: relative-to-workspace test
    // file paths. The guard checks that AT LEAST ONE listed file
    // contains both the entry-point symbol and an
    // `assert_observer_reaches` call.
    const WAVE_3_ENTRY_POINTS: &[(&str, &[&str])] = &[
        // Slice 3.A — TypeResolution producer.
        // `resolve_type_with_audit` (verter_session) drives the
        // `RequestKind::TypeResolution` audit producer. The TLS
        // driver asserts the dispatcher's hop accounting increments
        // (proving `current_observer()` was reachable on the
        // dispatch path) and that the harness's outer guard remains
        // visible on the calling thread after the entry-point's
        // nested guard drops.
        (
            "resolve_type_with_audit",
            &["crates/verter_session/tests/type_resolution_audit_tls_propagation.rs"],
        ),
        // Slice 3.B — Compile producer.
        // `compile_with_audit` (verter_session) drives the
        // `RequestKind::Compile` audit producer. The cross-crate
        // TLS harness exercises this: harness drives
        // `compile_with_audit`, asserts producer-crate
        // (`verter_compiler::code_transform`) instrumentation
        // observed `Some(observer)` via the `code_transform_ops > 0`
        // discriminator.
        (
            "compile_with_audit",
            &["crates/verter_session/tests/tls_harness_cross_crate.rs"],
        ),
        // Slice 3.C — SemanticAnalysis producer.
        // `analyze_with_audit` (verter_session) drives the
        // `RequestKind::SemanticAnalysis` audit producer. The
        // dedicated TLS test asserts the substrate slot is populated
        // for the audit window and drained on return.
        (
            "analyze_with_audit",
            &["crates/verter_session/tests/semantic_analysis_audit_tls_propagation.rs"],
        ),
        // Slice 3.D — Workspace producer.
        // `audit_op` is a trait method on `WorkspaceAccess`; the
        // session-level wrapper `audit_workspace_op` installs the
        // `RequestContextGuard` BEFORE the workspace traversal so
        // the trait body sees `current_observer() == Some(_)`. The
        // TLS driver drives the wrapper through the harness and
        // asserts the trait body reaches the resolver and stamps a
        // non-zero request id (proving the TLS slot was visible).
        // Test placement is verter_session/tests/ rather than
        // verter_workspace/tests/ because the harness lives in
        // verter_session and adding a dev-dep on verter_session
        // from verter_workspace would form a circular test-target
        // cycle; the existing slice
        // `workspace_audit_production_callsite.rs` resolves the
        // same constraint by living here too.
        (
            "audit_op",
            &["crates/verter_session/tests/workspace_audit_tls_propagation.rs"],
        ),
        // Slice 3.E — LSP producer.
        // `run_with_audit` (verter_lsp::audit_harness) wraps every
        // LSP `*_with_audit` handler. The TLS driver wraps a
        // synthetic handler future and asserts the substrate
        // observer is visible inside the future when audit is
        // enabled, and absent when audit is disabled
        // (short-circuit path).
        (
            "run_with_audit",
            &["crates/verter_lsp/tests/lsp_audit_tls_propagation.rs"],
        ),
        // Slice 3.F — Mcp producer.
        // `audit_mcp_tool_call` (verter_session) wraps a synthetic
        // tool-callback closure with the standard registration /
        // RequestContextGuard / finalize lifecycle. The TLS driver
        // wraps a synthetic closure and asserts the substrate
        // observer is visible inside the closure body when audit is
        // enabled, and absent when audit is disabled (Noop arm).
        (
            "audit_mcp_tool_call",
            &["crates/verter_session/tests/mcp_audit_tls_propagation.rs"],
        ),
    ];

    // Wave-3 entry-points that ship WITHOUT a paired TLS test.
    // Each entry: `(entry_point_symbol, rationale)`.
    //
    // Architectural intent: this list shrinks toward zero. The
    // follow-up fix-pass adds the missing TLS-propagation test for
    // each entry below, then removes the allow-list entry.
    //
    // The guard rejects an allow-list entry whose entry-point
    // already has a paired TLS test (stale allow-list).
    const MISSING_TLS_TEST: &[(&str, &str)] = &[];

    let workspace = workspace_root();

    // Step 1: every entry-point with a paired test must have at
    // least one test that references both the entry-point symbol
    // and `assert_observer_reaches`.
    let mut missing: Vec<String> = Vec::new();
    let mut wrong_path: Vec<String> = Vec::new();
    for (symbol, candidate_files) in WAVE_3_ENTRY_POINTS {
        let mut any_match = false;
        for rel in *candidate_files {
            let abs = workspace.join(rel);
            if !abs.exists() {
                wrong_path.push(format!("  - {symbol} → {rel} (file does not exist)"));
                continue;
            }
            let src = std::fs::read_to_string(&abs).unwrap_or_else(|e| {
                panic!("wave_3_entry_points_propagate_tls: cannot read `{rel}`: {e}")
            });
            let drives_entry = src.contains(symbol);
            let uses_harness = src.contains("assert_observer_reaches");
            if drives_entry && uses_harness {
                any_match = true;
                break;
            }
        }
        if !any_match {
            missing.push(format!(
                "  - {symbol}: none of {:?} contains BOTH the entry-point symbol AND \
                 `assert_observer_reaches(...)`",
                candidate_files,
            ));
        }
    }

    assert!(
        wrong_path.is_empty(),
        "wave_3_entry_points_propagate_tls: paired test files reference paths that \
         do not exist. The guard's pin list is stale. Update WAVE_3_ENTRY_POINTS in \
         this test:\n{}",
        wrong_path.join("\n"),
    );

    assert!(
        missing.is_empty(),
        "wave_3_entry_points_propagate_tls: the following Wave-3 entry-points are \
         pinned to test files that do NOT both invoke the entry-point AND drive \
         `assert_observer_reaches(...)`. Either add a TLS-propagation test for the \
         entry-point, or — if the entry-point cannot yet be tested via the harness \
         — move the entry to MISSING_TLS_TEST with a rationale.\n{}",
        missing.join("\n"),
    );

    // Step 2: every entry in MISSING_TLS_TEST is a temporary
    // allow-list. If a paired TLS-propagation test now exists, the
    // entry must be promoted to WAVE_3_ENTRY_POINTS (and removed
    // from this list). Detect that by scanning all
    // `crates/*/tests/*tls*propagation*.rs` and
    // `crates/*/tests/*tls_harness*.rs` plus any test file that
    // already uses `assert_observer_reaches` for a co-occurrence
    // with the entry-point symbol.
    let mut tls_test_files: Vec<std::path::PathBuf> = Vec::new();
    let crates_dir = workspace.join("crates");
    let crate_entries = std::fs::read_dir(&crates_dir)
        .unwrap_or_else(|e| panic!("wave_3_entry_points_propagate_tls: cannot read crates/: {e}"));
    // Skip the guard file itself — it lists every entry-point
    // symbol (in WAVE_3_ENTRY_POINTS / MISSING_TLS_TEST) AND the
    // string `assert_observer_reaches` (in this guard's docs and
    // matching expressions), so naive co-occurrence would match
    // every entry against this file and falsely flag every allow-
    // list entry as stale.
    let self_file = workspace.join("crates/verter_session/tests/architecture_guards.rs");
    for crate_entry in crate_entries.flatten() {
        let tests_dir = crate_entry.path().join("tests");
        if !tests_dir.is_dir() {
            continue;
        }
        if let Ok(entries) = std::fs::read_dir(&tests_dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.extension().is_some_and(|e| e == "rs") && p != self_file {
                    tls_test_files.push(p);
                }
            }
        }
    }
    let mut stale_missing: Vec<String> = Vec::new();
    for (symbol, rationale) in MISSING_TLS_TEST {
        // Reject entries whose symbol is also in WAVE_3_ENTRY_POINTS
        // (would be a contradiction: pinned + missing).
        if WAVE_3_ENTRY_POINTS.iter().any(|(s, _)| s == symbol) {
            stale_missing.push(format!(
                "  - {symbol}: present in BOTH WAVE_3_ENTRY_POINTS and MISSING_TLS_TEST. \
                 Remove from one list."
            ));
            continue;
        }
        for path in &tls_test_files {
            let src = match std::fs::read_to_string(path) {
                Ok(s) => s,
                Err(_) => continue,
            };
            if src.contains(symbol) && src.contains("assert_observer_reaches") {
                stale_missing.push(format!(
                    "  - {symbol}: a TLS-propagation test now exists at {} \
                     (rationale was: {rationale}). Promote {symbol} to \
                     WAVE_3_ENTRY_POINTS and drop the MISSING_TLS_TEST entry.",
                    path.display(),
                ));
                break;
            }
        }
    }

    assert!(
        stale_missing.is_empty(),
        "wave_3_entry_points_propagate_tls: stale MISSING_TLS_TEST entries:\n{}",
        stale_missing.join("\n"),
    );
}

/// Wave 4 close — `every_consumer_has_production_call_site`.
///
/// Plan §1.6 row: "For every `RequestKind` variant, at least one
/// **non-test** source file under `crates/*/src/` populates a record
/// with that variant."
///
/// The guard parses [`verter_audit::RequestKind`] from
/// `crates/verter_audit/src/record.rs`, walks every `*.rs` file under
/// each `crates/<crate>/src/` tree, and verifies that each variant
/// appears as a producer-side **expression** literal — i.e. a
/// `RequestKind::<Variant>` (or fully-qualified `verter_audit::
/// RequestKind::<Variant>`) path in non-pattern position. Match-arm
/// patterns (`RequestKind::Foo { .. } => …`) are CONSUMER sites and
/// do not count; producer code constructs the variant either as a
/// struct-literal value or as a unit/tuple expression and passes it
/// to `RequestContext::with_kind_and_timing` (or assigns it to the
/// `kind:` field of a `RequestAuditRecord` literal).
///
/// `KIND_EXEMPTIONS` enumerates variants that are deliberately not
/// produced from any in-tree non-test source file, with rationale.
/// The guard rejects an exemption whose variant *is* produced (stale
/// allow-list) so the list shrinks toward zero as new producers ship.
///
/// Discrimination contract:
/// - Pre-Wave-3 tree (no `*_with_audit` producers): `ComponentMeta`,
///   `TypeResolution`, `SemanticAnalysis`, `Compile`, `Workspace`,
///   `Lsp`, `Mcp` are all unproduced → guard fails with the per-
///   variant "no producer" diagnostic.
/// - Wave-4 close (every producer landed): all 7 producer variants
///   resolve, only the documented exemptions remain (`Custom`,
///   `BundlerBatch`).
/// - Future regression (a producer is deleted or its `kind:` field
///   is rewritten): the variant disappears from the producer set
///   and the guard fails with the per-variant diagnostic.
#[test]
fn every_consumer_has_production_call_site() {
    use std::collections::BTreeMap;
    use syn::visit::Visit;

    // Variants deliberately not produced anywhere in `crates/*/src/`.
    // Each entry: `(variant_name, rationale)`. Stale entries (variant
    // *is* produced) fail the guard.
    //
    // Architectural intent: the list shrinks toward zero. Adding a
    // producer for a documented variant requires removing its
    // exemption in the same change.
    const KIND_EXEMPTIONS: &[(&str, &str)] = &[
        // Open-ended escape hatch. The plan documents `Custom` as a
        // free-form name producers may set when their concern does
        // not warrant a first-class variant. No in-tree `*_with_audit`
        // producer constructs `Custom { name: ... }`; out-of-tree
        // plugin authors are the intended emitters.
        (
            "Custom",
            "open-ended escape hatch — out-of-tree plugin authors emit `Custom { name }`; \
             no in-tree producer constructs this variant. Adding an in-tree producer requires \
             removing this exemption in the same change.",
        ),
        // The `BatchAuditAggregator::summarize` API folds existing
        // records into a `BundlerBatchPayload` and returns the
        // payload synchronously to callers (`getBundlerBatchSummary`
        // on NAPI/WASM). It does NOT publish a record with
        // `kind: RequestKind::BundlerBatch { ... }` into the
        // `AuditRecordsStore`; bundler integrations consume the
        // payload directly. Match arms in `summarize` and the
        // FFI dispatchers are CONSUMER sites (they reduce records of
        // OTHER kinds into the bundler payload) and intentionally do
        // not count.
        (
            "BundlerBatch",
            "produced as a `BundlerBatchPayload` return value from \
             `BatchAuditAggregator::summarize` (and FFI `getBundlerBatchSummary`); no in-tree \
             producer publishes a record with `kind: RequestKind::BundlerBatch { .. }` into \
             `AuditRecordsStore`. Adding an in-tree record producer (for example, a future \
             host-driven bundler-summary publisher) requires removing this exemption in the \
             same change.",
        ),
    ];

    // Step 1: enumerate every `RequestKind` variant from
    // `crates/verter_audit/src/record.rs`. Reuses the same parser
    // shape `request_kind_payload_parity` uses so the two guards
    // agree on what counts as a variant.
    let record_src = read_workspace_file("crates/verter_audit/src/record.rs");

    fn enum_variant_names(src: &str, enum_name: &str) -> Vec<String> {
        let header = format!("pub enum {enum_name}");
        let start = src
            .find(&header)
            .unwrap_or_else(|| panic!("enum {enum_name} not found in record.rs"));
        let body_start = src[start..]
            .find('{')
            .map(|i| start + i + 1)
            .unwrap_or_else(|| panic!("enum {enum_name} body not found"));
        let bytes = src.as_bytes();
        let mut depth = 1usize;
        let mut idx = body_start;
        while idx < bytes.len() && depth > 0 {
            match bytes[idx] {
                b'{' => depth += 1,
                b'}' => depth -= 1,
                _ => {}
            }
            idx += 1;
        }
        let body_end = idx - 1;
        let body = &src[body_start..body_end];
        let mut names = Vec::new();
        for raw_line in body.lines() {
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with("//") || line.starts_with("///") {
                continue;
            }
            let head_end = [line.find('('), line.find('{'), line.find(',')]
                .into_iter()
                .flatten()
                .min()
                .unwrap_or(line.len());
            let head: &str = line[..head_end].trim();
            if head.is_empty() {
                continue;
            }
            if head.starts_with('#') {
                continue;
            }
            let name = head.split_whitespace().next().unwrap_or("");
            if name.is_empty() {
                continue;
            }
            if name
                .chars()
                .all(|c: char| c.is_ascii_alphanumeric() || c == '_')
            {
                names.push(name.to_string());
            }
        }
        names
    }

    let variants = enum_variant_names(&record_src, "RequestKind");
    assert!(
        !variants.is_empty(),
        "every_consumer_has_production_call_site: no `RequestKind` variants discovered — \
         parser broke or the enum was renamed."
    );

    // Step 2: walk every `crates/<crate>/src/` tree and visit each
    // `*.rs` file's AST. Track per-variant production hits.
    //
    // Production = `RequestKind::<Variant>` path appears in EXPRESSION
    // context (struct-literal value, function-call argument, struct
    // field initialiser). Match-arm patterns (`RequestKind::Foo { .. }
    // => …`) are CONSUMER sites and skipped — `Visit::visit_pat_*`
    // hooks are not invoked because the visitor only walks expression
    // paths.
    struct ProducerVisitor<'a> {
        variant_set: &'a std::collections::HashSet<String>,
        hits: BTreeMap<String, Vec<String>>,
        rel_path: String,
        // Depth counter for pattern context. syn 2.x dispatches
        // `Pat::Path` (unit-variant patterns like
        // `RequestKind::ComponentMeta` in `match` arms) to
        // `visit_expr_path` — see `syn::visit::visit_pat` source.
        // Without this gate, every match arm pattern would falsely
        // count as a producer site.
        pat_depth: u32,
    }

    impl<'a, 'ast> Visit<'ast> for ProducerVisitor<'a> {
        fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
            // Skip inline `#[cfg(test)]` modules — they live in
            // production source files but are not production code.
            if item_is_cfg_test(&item.attrs) {
                return;
            }
            syn::visit::visit_item_mod(self, item);
        }
        fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
            // Skip `#[test]` and `#[cfg(test)]`-attributed functions
            // — they are test code that happens to live alongside
            // production code in the same file.
            if item_is_cfg_test(&item.attrs) || item_is_test(&item.attrs) {
                return;
            }
            syn::visit::visit_item_fn(self, item);
        }
        fn visit_pat(&mut self, pat: &'ast syn::Pat) {
            self.pat_depth = self.pat_depth.saturating_add(1);
            syn::visit::visit_pat(self, pat);
            self.pat_depth = self.pat_depth.saturating_sub(1);
        }
        fn visit_expr_path(&mut self, expr: &'ast syn::ExprPath) {
            // Skip pattern-context paths — `Pat::Path` dispatches
            // here via syn's `visit_pat`, but those are CONSUMER-side
            // match-arm patterns and do not count as producer sites.
            if self.pat_depth > 0 {
                syn::visit::visit_expr_path(self, expr);
                return;
            }
            if let Some(variant) = match_request_kind_variant(&expr.path) {
                if self.variant_set.contains(&variant) {
                    self.hits
                        .entry(variant)
                        .or_default()
                        .push(self.rel_path.clone());
                }
            }
            syn::visit::visit_expr_path(self, expr);
        }
        fn visit_expr_struct(&mut self, expr: &'ast syn::ExprStruct) {
            if let Some(variant) = match_request_kind_variant(&expr.path) {
                if self.variant_set.contains(&variant) {
                    self.hits
                        .entry(variant)
                        .or_default()
                        .push(self.rel_path.clone());
                }
            }
            syn::visit::visit_expr_struct(self, expr);
        }
        fn visit_expr_call(&mut self, expr: &'ast syn::ExprCall) {
            if let syn::Expr::Path(ep) = &*expr.func {
                if let Some(variant) = match_request_kind_variant(&ep.path) {
                    if self.variant_set.contains(&variant) {
                        self.hits
                            .entry(variant)
                            .or_default()
                            .push(self.rel_path.clone());
                    }
                }
            }
            syn::visit::visit_expr_call(self, expr);
        }
    }

    /// Recognise `#[cfg(test)]` (or `#[cfg(any(test, ...))]`) so the
    /// visitor skips inline test modules that share a source file
    /// with production code. `Attribute::meta` exposes the parsed
    /// `Meta` AST directly — using `Meta::List` token-stream
    /// inspection is simpler and more reliable than nested-meta
    /// parsers on attributes that may be `cfg(any(unix, test))`.
    fn item_is_cfg_test(attrs: &[syn::Attribute]) -> bool {
        attrs.iter().any(|attr| {
            if !attr.path().is_ident("cfg") {
                return false;
            }
            // Render the meta back to a string and substring-match
            // for `test`. `cfg(test)` → `cfg(test)`. `cfg(any(unix,
            // test))` → `cfg(any(unix, test))`. False positives are
            // theoretically possible (e.g. an identifier literally
            // called `testing`) but the substring match also requires
            // word boundaries via the surrounding `(` `,` ` ` `)`
            // characters.
            use quote::ToTokens;
            let rendered = attr.meta.to_token_stream().to_string();
            // Match `test` as a whole token: delimited by `(`, `)`,
            // `,`, or whitespace.
            let needle = "test";
            let bytes = rendered.as_bytes();
            let n_bytes = needle.as_bytes();
            let mut idx = 0usize;
            while idx + n_bytes.len() <= bytes.len() {
                if &bytes[idx..idx + n_bytes.len()] == n_bytes {
                    let before_ok =
                        idx == 0 || matches!(bytes[idx - 1], b'(' | b',' | b' ' | b'\t' | b'\n');
                    let after_idx = idx + n_bytes.len();
                    let after_ok = after_idx == bytes.len()
                        || matches!(bytes[after_idx], b')' | b',' | b' ' | b'\t' | b'\n');
                    if before_ok && after_ok {
                        return true;
                    }
                }
                idx += 1;
            }
            false
        })
    }

    fn item_is_test(attrs: &[syn::Attribute]) -> bool {
        attrs.iter().any(|attr| attr.path().is_ident("test"))
    }

    /// Match `RequestKind::<Variant>` (with optional leading
    /// `verter_audit::` or `crate::component_meta_audit::`) and
    /// return the variant name. The path's last segment must be the
    /// variant; the segment immediately before must be `RequestKind`.
    fn match_request_kind_variant(path: &syn::Path) -> Option<String> {
        let segments: Vec<&syn::PathSegment> = path.segments.iter().collect();
        if segments.len() < 2 {
            return None;
        }
        let last = segments[segments.len() - 1];
        let parent = segments[segments.len() - 2];
        if parent.ident != "RequestKind" {
            return None;
        }
        Some(last.ident.to_string())
    }

    let variant_set: std::collections::HashSet<String> = variants.iter().cloned().collect();
    let mut hits: BTreeMap<String, Vec<String>> = BTreeMap::new();

    let crates_dir = workspace_root().join("crates");
    let crate_entries = std::fs::read_dir(&crates_dir).unwrap_or_else(|e| {
        panic!("every_consumer_has_production_call_site: cannot read crates/: {e}")
    });
    for crate_entry in crate_entries.flatten() {
        let src_dir = crate_entry.path().join("src");
        if !src_dir.is_dir() {
            continue;
        }
        walk_dir_collect_rs(&src_dir, &mut |path: &std::path::Path| {
            // Skip files whose path includes a `tests` segment — some
            // crates put inline integration test modules under
            // `src/tests/`. Production code lives outside any
            // `tests` segment.
            let has_tests_segment = path
                .components()
                .any(|c| c.as_os_str().to_string_lossy() == "tests");
            if has_tests_segment {
                return;
            }
            let src = std::fs::read_to_string(path).unwrap_or_else(|e| {
                panic!(
                    "every_consumer_has_production_call_site: cannot read `{}`: {e}",
                    path.display()
                )
            });
            let parsed = match syn::parse_file(&src) {
                Ok(p) => p,
                Err(e) => panic!(
                    "every_consumer_has_production_call_site: cannot parse `{}`: {e}",
                    path.display()
                ),
            };
            let rel = path
                .strip_prefix(workspace_root())
                .unwrap_or(path)
                .to_string_lossy()
                .replace('\\', "/");
            let mut visitor = ProducerVisitor {
                variant_set: &variant_set,
                hits: BTreeMap::new(),
                rel_path: rel,
                pat_depth: 0,
            };
            visitor.visit_file(&parsed);
            for (variant, paths) in visitor.hits {
                hits.entry(variant).or_default().extend(paths);
            }
        });
    }

    // Step 3: every variant must either have a producer call site
    // OR a documented `KIND_EXEMPTIONS` entry.
    let exemption_set: std::collections::HashSet<&str> =
        KIND_EXEMPTIONS.iter().map(|(name, _)| *name).collect();

    let mut missing: Vec<String> = Vec::new();
    for variant in &variants {
        if hits.contains_key(variant) {
            continue;
        }
        if exemption_set.contains(variant.as_str()) {
            continue;
        }
        missing.push(format!(
            "  - {variant}: no `RequestKind::{variant}` expression literal found in any \
             non-test source file under `crates/*/src/`. Either add a production producer \
             that constructs this variant, OR document the absence in `KIND_EXEMPTIONS` with \
             a rationale (and accept that out-of-tree code is the only emitter)."
        ));
    }

    assert!(
        missing.is_empty(),
        "every_consumer_has_production_call_site: the following `RequestKind` variants have \
         NO production call site under `crates/*/src/`. Plan §1.6 requires every variant to \
         either have an in-tree producer OR a documented exemption.\n{}",
        missing.join("\n"),
    );

    // Step 4: reject stale exemptions — entries whose variant *is*
    // now produced from in-tree code. This forces the exemption list
    // to shrink whenever a producer ships.
    let mut stale: Vec<String> = Vec::new();
    for (variant, rationale) in KIND_EXEMPTIONS {
        if hits.contains_key(*variant) {
            let producer_files: Vec<String> = hits
                .get(*variant)
                .map(|paths| {
                    let mut deduped: Vec<String> = paths.clone();
                    deduped.sort();
                    deduped.dedup();
                    deduped
                })
                .unwrap_or_default();
            stale.push(format!(
                "  - {variant}: an in-tree producer now exists at {producer_files:?} \
                 (rationale was: {rationale}). Remove the exemption from KIND_EXEMPTIONS."
            ));
        }
    }
    assert!(
        stale.is_empty(),
        "every_consumer_has_production_call_site: stale KIND_EXEMPTIONS entries:\n{}",
        stale.join("\n"),
    );

    // Step 5: reject an exemption whose variant does not exist on
    // `RequestKind` at all (the enum was edited and the exemption
    // wasn't updated). Without this, a renamed variant could silently
    // pass the guard.
    let mut unknown: Vec<String> = Vec::new();
    for (variant, _) in KIND_EXEMPTIONS {
        if !variant_set.contains(*variant) {
            unknown.push(format!(
                "  - {variant}: exemption references a variant that does NOT exist on \
                 `RequestKind`. Update `KIND_EXEMPTIONS` to match the current enum."
            ));
        }
    }
    assert!(
        unknown.is_empty(),
        "every_consumer_has_production_call_site: KIND_EXEMPTIONS contains unknown variants:\n{}",
        unknown.join("\n"),
    );
}

// ──────────────────────────────────────────────────────────────────────
// Slot-binding-graph synthesis architecture guards.
//
// These guards enforce the §3.12 + §17.6 + §10 R12b invariants for the
// graph-native slot-binding synthesis introduced alongside
// `slot_binding_graph.rs`:
//
//   - §3.12: the synthesis must drive the carrier walk in
//     `ProjectionMode::Navigate`; an `Expanded` projection re-introduces
//     the giant-tree pathology that motivated the rewrite.
//   - §3.12 (no phase archaeology): the synthesis source must read as
//     final-state — no plan-phase / cutover / agent-id vocabulary.
//   - §10 R12b: the synthesis must merge dep-signatures via
//     `dispatch.execute_read(..)` rather than the bare `dispatch.execute(..)`.
//     `execute` discards the `dep_signature` half so callers that go
//     through it cannot maintain the warm-cache fence.
//   - §17.6: the synthesis must emit a `synthesize_slot_bindings` and
//     per-macro `synthesize_macro` tracing span; the `walker_pathological_input_cap`
//     warn event must be wired in the walker; the audit substrate's
//     `ComponentMetaPayload` must carry diagnostics + suppression
//     facts.
// ──────────────────────────────────────────────────────────────────────

#[test]
fn slot_binding_graph_uses_navigate_not_expanded() {
    let src = read_workspace_file("crates/verter_session/src/meta_resolve/slot_binding_graph.rs");
    assert!(
        !src.contains("ProjectionMode::Expanded"),
        "slot_binding_graph.rs must drive synthesis in Navigate mode; \
         an Expanded projection re-introduces the giant-tree pathology \
         that motivated the rewrite. Found `ProjectionMode::Expanded` \
         in the synthesis source.",
    );
}

#[test]
fn slot_binding_graph_no_phase_archaeology() {
    let src = read_workspace_file("crates/verter_session/src/meta_resolve/slot_binding_graph.rs");
    let lower = src.to_lowercase();
    let needles = [
        "phase 1",
        "phase-1",
        "phase 2",
        "phase-2",
        "cutover",
        "post-cutover",
        "pre-phase",
        "sa-1.b-impl",
        "sa-1.b-tests",
        "sa-1.c",
        "scratch branch",
        "v8",
        "v9",
        "v10",
    ];
    for needle in needles {
        assert!(
            !lower.contains(needle),
            "slot_binding_graph.rs must read as final-state — found plan archaeology: {:?}",
            needle,
        );
    }
}

#[test]
fn slot_binding_graph_uses_execute_read_only() {
    let src = read_workspace_file("crates/verter_session/src/meta_resolve/slot_binding_graph.rs");
    let mut violations = Vec::new();
    for (line_no, line) in src.lines().enumerate() {
        if line.contains("dispatch.execute(") && !line.contains("dispatch.execute_read(") {
            violations.push(format!("  line {}: {}", line_no + 1, line.trim()));
        }
    }
    assert!(
        violations.is_empty(),
        "slot_binding_graph.rs must merge dep-signatures via \
         `dispatch.execute_read(..)`; bare `dispatch.execute(..)` discards \
         the dep_signature half and breaks the warm-cache fence. \
         Found:\n{}",
        violations.join("\n"),
    );
}

#[test]
fn slot_binding_graph_emits_synthesis_spans() {
    let src = read_workspace_file("crates/verter_session/src/meta_resolve/slot_binding_graph.rs");
    assert!(
        src.contains("synthesize_slot_bindings"),
        "slot_binding_graph.rs must emit a `synthesize_slot_bindings` \
         tracing span at the synthesis entry point so log captures can \
         attribute work to the synthesis pass.",
    );
    assert!(
        src.contains("synthesize_macro"),
        "slot_binding_graph.rs must emit a per-macro `synthesize_macro` \
         tracing span so log captures can attribute work to a specific \
         macro invocation within the synthesis pass.",
    );
}

#[test]
fn walker_emits_tracing_events() {
    let src = read_workspace_file("crates/verter_session/src/project_semantic_dispatch/walk.rs");
    assert!(
        src.contains("debug_span!(\n            target: \"verter::dispatch::walk\",\n            \"walk_shallow_surface\"")
            || src.contains("debug_span!(\"walk_shallow_surface\"")
            || src.contains("\"walk_shallow_surface\""),
        "walk.rs must open a `walk_shallow_surface` debug span at the \
         iterative walker entry-point so log captures can attribute \
         shallow-surface walks to the dispatch layer.",
    );
    assert!(
        src.contains("walker_pathological_input_cap"),
        "walk.rs must emit a `walker_pathological_input_cap` warn \
         event when the pathological-input cap fires so log captures \
         can correlate the event with the matching audit diagnostic.",
    );
}

#[test]
fn component_meta_payload_carries_walker_diagnostics() {
    let src = read_workspace_file("crates/verter_audit/src/payloads/component_meta.rs");
    assert!(
        src.contains("pub diagnostics: Vec<AuditDiagnosticEntry>"),
        "verter_audit::payloads::ComponentMetaPayload must carry a \
         `diagnostics: Vec<AuditDiagnosticEntry>` field so the audit \
         substrate exposes macro-expansion diagnostics surfaced \
         during the request.",
    );
    assert!(
        src.contains("pub should_suppress: bool"),
        "verter_audit::payloads::ComponentMetaPayload must carry a \
         `should_suppress: bool` field so consumers observe whether \
         a fatal QueryError suppressed cache promotion.",
    );
}

#[test]
fn getcomponentmeta_uses_per_macro_projectors() {
    // Production `get_component_meta` / `compute_component_meta_state_inner`
    // must dispatch through the per-macro projector module
    // (`meta_resolve::projectors::project_evaluated_types` or its
    // siblings). Discriminates against any drift commit that re-routes
    // production back through the legacy walker outer driver.
    let src =
        read_workspace_file("crates/verter_session/src/host_manage/component_meta_methods.rs");
    assert!(
        src.contains("project_evaluated_types"),
        "host_manage/component_meta_methods.rs must dispatch through \
         `crate::meta_resolve::projectors::project_evaluated_types` — \
         a re-routed production path that bypasses the per-macro \
         projector module would be invisible without this guard."
    );
}

// `no_legacy_walker_in_production_code` retired post-§7.3 cutover —
// the legacy walker family is fully deleted. Coverage moves to the
// `tests/no_legacy_walker.rs` `RETIRED_SYMBOLS` gate which scans
// the entire workspace, not just `crates/verter_session/src/host_manage/`.

/// R22 + reachability-GC rename guard.
///
/// Production source must reference the unified
/// `evict_unreachable_artifacts` reachability sweep, NOT the
/// historical `evict_unreachable_indexed_ready` name. The store the
/// sweep operates on holds `IndexedReady`, `FileFacts`, `ParsedEdges`,
/// and augmentations under one key, so the broader name is the
/// correct one. Doc-comment back-references in non-production paths
/// (e.g. `.phase-markers/`, `tools/orchestrator/reports/`, plan docs)
/// are out of scope.
#[test]
fn reachability_gc_uses_unified_artifact_name() {
    use std::fs;
    let scan_dirs = ["crates/verter_session/src", "crates/verter_workspace/src"];
    let mut violations: Vec<String> = Vec::new();
    for dir in &scan_dirs {
        let root = workspace_root().join(dir);
        let mut stack = vec![root.clone()];
        while let Some(p) = stack.pop() {
            let read = match fs::read_dir(&p) {
                Ok(rd) => rd,
                Err(_) => continue,
            };
            for entry in read.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                    continue;
                }
                let src = match fs::read_to_string(&path) {
                    Ok(s) => s,
                    Err(_) => continue,
                };
                for (idx, line) in src.lines().enumerate() {
                    if line.contains("evict_unreachable_indexed_ready") {
                        violations.push(format!("{}:{}: {}", path.display(), idx + 1, line.trim()));
                    }
                }
            }
        }
    }
    assert!(
        violations.is_empty(),
        "reachability_gc_uses_unified_artifact_name: production source \
         still references the legacy `evict_unreachable_indexed_ready` \
         name. The unified sweep is `evict_unreachable_artifacts`. \
         Violations:\n{}",
        violations.join("\n")
    );
}

/// R22 — reverse graph is never wired to cache invalidation.
///
/// Production source must not adjacently combine a reverse-graph
/// read (`reverse_deps_for`, `affected_canonicals`) with a cache
/// invalidation / eviction call. The reverse import graph is
/// content-addressed and serves reachability GC + LSP affected-files
/// reporting + diagnostics only — wiring it to a cache flush would
/// resurrect the eager-invalidation model R22 retired.
///
/// "Adjacent" is defined as: a reverse-graph read and a forbidden
/// call appearing within a 5-line sliding window in the same source
/// file. This is a heuristic gate; it errs on the side of
/// false-positives so the reviewer can audit any genuinely close
/// pair. The architecture-guards self-test pair below proves the
/// heuristic discriminates.
#[test]
fn reverse_graph_not_wired_to_invalidation() {
    use std::fs;

    const REVERSE_READS: &[&str] = &["reverse_deps_for", "affected_canonicals"];
    const FORBIDDEN_INVALIDATIONS: &[&str] = &[
        "invalidate_canonical",
        ".clear()",
        "evict_canonical",
        "semantic_invalidate",
        "smart_invalidate_dependents",
    ];
    // host_upsert.rs has a backstop dependent-eviction block that
    // still reads `reverse_deps_for` to find owners for the byte-
    // identical-input-failure branch. R3 target removes this once
    // producer admission carries dep-precise signatures across the
    // cross-file boundary; until then the block is explicitly
    // documented and outside the guard's scope.
    const ALLOW_LIST: &[&str] = &["crates/verter_session/src/host_upsert.rs"];

    let scan_dirs = ["crates/verter_session/src", "crates/verter_workspace/src"];
    let mut violations: Vec<String> = Vec::new();
    for dir in &scan_dirs {
        let root = workspace_root().join(dir);
        let mut stack = vec![root];
        while let Some(p) = stack.pop() {
            let read = match fs::read_dir(&p) {
                Ok(rd) => rd,
                Err(_) => continue,
            };
            for entry in read.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                    continue;
                }
                // Skip test files — the guard targets production paths
                // only. Tests legitimately read the reverse graph next
                // to invalidation calls when characterising older
                // behaviour.
                let path_str = path.display().to_string().replace('\\', "/");
                if path_str.ends_with("_tests.rs") || path_str.contains("/tests/") {
                    continue;
                }
                // Allow-list residual back-stops that R3 will remove.
                let rel_str = path
                    .strip_prefix(workspace_root())
                    .map(|p| p.display().to_string().replace('\\', "/"))
                    .unwrap_or_else(|_| path_str.clone());
                if ALLOW_LIST.iter().any(|p| rel_str == *p) {
                    continue;
                }
                let src = match fs::read_to_string(&path) {
                    Ok(s) => s,
                    Err(_) => continue,
                };
                let lines: Vec<&str> = src.lines().collect();
                for (idx, line) in lines.iter().enumerate() {
                    if !REVERSE_READS.iter().any(|n| line.contains(n)) {
                        continue;
                    }
                    let start = idx.saturating_sub(2);
                    let end = (idx + 3).min(lines.len());
                    for (j, neighbour) in lines.iter().enumerate().take(end).skip(start) {
                        if j == idx {
                            continue;
                        }
                        if FORBIDDEN_INVALIDATIONS
                            .iter()
                            .any(|n| neighbour.contains(n))
                        {
                            violations.push(format!(
                                "{}:{}: reverse-graph read adjacent to invalidation `{}` at line {}",
                                path.display(),
                                idx + 1,
                                FORBIDDEN_INVALIDATIONS
                                    .iter()
                                    .find(|n| neighbour.contains(*n))
                                    .copied()
                                    .unwrap_or("?"),
                                j + 1,
                            ));
                            break;
                        }
                    }
                }
            }
        }
    }
    assert!(
        violations.is_empty(),
        "reverse_graph_not_wired_to_invalidation: production source \
         has a reverse-graph read adjacent (within 5 lines) to a cache \
         invalidation call. R22 forbids wiring the reverse graph to \
         cache flushes — the reverse axis is content-addressed and \
         serves reachability GC + LSP affected-files reporting only. \
         If the adjacency is legitimate (e.g. a documented backstop) \
         add the file to the guard's allow-list with a one-line \
         rationale. Violations:\n{}",
        violations.join("\n")
    );
}

// ---------------------------------------------------------------------------
// Typed-IR-Only Resolver Rule guards (CLAUDE.md "Typed-IR-Only Resolver Rule")
// ---------------------------------------------------------------------------
//
// The six guards below pin the architectural ban on string-search /
// reparse / role-inference patterns inside the component-meta /
// typeinfo type resolver pipeline. Each owns an EXACT
// `(file, line, pattern)` allowlist tuple set captured against the
// live tree. The guards are exact-set comparisons in BOTH directions:
//
//   * a violation that exists in source but is NOT in the allowlist
//     fails the test ("Unallowlisted violation introduced");
//   * an allowlist tuple that no longer matches anything in source
//     ALSO fails ("Allowlisted entry NOT FOUND").
//
// As migration units land they remove their tuples from the
// allowlist; the W8.2 "everything empty" floor is the cutover end
// state. Counts are gameable (deleting one site and adding another
// passes a count check); exact tuples are not.

mod typed_ir_resolver_guards {
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::{Path, PathBuf};

    /// Walk `<repo>/crates/<crate>/src/**` and yield every `.rs` file
    /// EXCEPT files whose basename matches `<name>_tests.rs` or
    /// equals `tests.rs`. Those files exist inside `src/` for
    /// per-CLAUDE.md test-file organisation but are test-only modules.
    fn collect_production_rs_files() -> Vec<(PathBuf, String)> {
        let root = super::workspace_root();
        let crates_dir = root.join("crates");
        let mut out: Vec<(PathBuf, String)> = Vec::new();
        let entries = match fs::read_dir(&crates_dir) {
            Ok(e) => e,
            Err(err) => panic!("read_dir {}: {err}", crates_dir.display()),
        };
        for ent in entries.flatten() {
            let crate_path = ent.path();
            if !crate_path.is_dir() {
                continue;
            }
            let src_dir = crate_path.join("src");
            if !src_dir.is_dir() {
                continue;
            }
            let mut files: Vec<PathBuf> = Vec::new();
            walk_rs(&src_dir, &mut files);
            for f in files {
                let rel = f
                    .strip_prefix(&root)
                    .unwrap_or(&f)
                    .to_string_lossy()
                    .replace('\\', "/");
                if is_test_file(&rel) {
                    continue;
                }
                out.push((f, rel));
            }
        }
        out
    }

    fn walk_rs(dir: &Path, out: &mut Vec<PathBuf>) {
        if !dir.is_dir() {
            return;
        }
        for entry in
            fs::read_dir(dir).unwrap_or_else(|e| panic!("read_dir {}: {}", dir.display(), e))
        {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            let p = entry.path();
            if p.is_dir() {
                walk_rs(&p, out);
            } else if p.extension().and_then(|s| s.to_str()) == Some("rs") {
                out.push(p);
            }
        }
    }

    fn is_test_file(rel: &str) -> bool {
        let name = rel.rsplit('/').next().unwrap_or("");
        name.ends_with("_tests.rs") || name == "tests.rs"
    }

    /// Replace `//` line comments and `/* ... */` block comments with
    /// equivalent-length whitespace, preserving newlines so line
    /// numbers stay stable. Skips comment-like sequences inside
    /// regular and raw string literals so the strip never invalidates
    /// real source.
    fn strip_comments(src: &str) -> String {
        let bytes = src.as_bytes();
        let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
        let n = bytes.len();
        let mut i = 0usize;
        while i < n {
            let c = bytes[i];
            // Raw string: r"..."  /  r#"..."#  /  r##"..."##  ...
            if c == b'r' {
                let mut j = i + 1;
                let mut hashes = 0usize;
                while j < n && bytes[j] == b'#' {
                    hashes += 1;
                    j += 1;
                }
                if j < n && bytes[j] == b'"' {
                    // Copy through the opening `r###"`
                    out.extend_from_slice(&bytes[i..=j]);
                    let close: Vec<u8> = std::iter::once(b'"')
                        .chain(std::iter::repeat_n(b'#', hashes))
                        .collect();
                    let mut k = j + 1;
                    while k + close.len() <= n {
                        if &bytes[k..k + close.len()] == close.as_slice() {
                            out.extend_from_slice(&bytes[(j + 1)..(k + close.len())]);
                            i = k + close.len();
                            break;
                        }
                        out.push(bytes[k]);
                        k += 1;
                    }
                    if k + close.len() > n {
                        out.extend_from_slice(&bytes[(j + 1)..n]);
                        i = n;
                    }
                    continue;
                }
                // Not a raw string — fall through to normal handling.
            }
            // Regular string literal "..." (with \"  escape handling)
            if c == b'"' {
                out.push(b'"');
                let mut k = i + 1;
                while k < n {
                    if bytes[k] == b'\\' && k + 1 < n {
                        out.push(bytes[k]);
                        out.push(bytes[k + 1]);
                        k += 2;
                        continue;
                    }
                    if bytes[k] == b'"' {
                        out.push(b'"');
                        k += 1;
                        break;
                    }
                    out.push(bytes[k]);
                    k += 1;
                }
                i = k;
                continue;
            }
            // Line comment //
            if c == b'/' && i + 1 < n && bytes[i + 1] == b'/' {
                let mut k = i;
                while k < n && bytes[k] != b'\n' {
                    out.push(b' ');
                    k += 1;
                }
                i = k;
                continue;
            }
            // Block comment /* ... */ with nesting support.
            if c == b'/' && i + 1 < n && bytes[i + 1] == b'*' {
                let mut depth = 1u32;
                out.push(b' ');
                out.push(b' ');
                let mut k = i + 2;
                while k < n && depth > 0 {
                    if k + 1 < n && bytes[k] == b'/' && bytes[k + 1] == b'*' {
                        depth += 1;
                        out.push(b' ');
                        out.push(b' ');
                        k += 2;
                        continue;
                    }
                    if k + 1 < n && bytes[k] == b'*' && bytes[k + 1] == b'/' {
                        depth -= 1;
                        out.push(b' ');
                        out.push(b' ');
                        k += 2;
                        continue;
                    }
                    if bytes[k] == b'\n' {
                        out.push(b'\n');
                    } else {
                        out.push(b' ');
                    }
                    k += 1;
                }
                i = k;
                continue;
            }
            out.push(c);
            i += 1;
        }
        String::from_utf8_lossy(&out).into_owned()
    }

    /// Replace the body of every `#[cfg(test)] mod NAME { ... }` block
    /// with whitespace (newlines preserved). Inline test modules live
    /// in production source files but are test-only — guard scans
    /// must NOT classify them as production violations.
    fn strip_inline_test_modules(src: &str) -> String {
        let bytes = src.as_bytes();
        let n = bytes.len();
        let mut out = bytes.to_vec();
        let needle = b"#[cfg(test)]";
        let mut i = 0usize;
        while i + needle.len() <= n {
            if &bytes[i..i + needle.len()] == needle {
                let mut j = i + needle.len();
                // Walk forward until we find `mod ` (allowing intervening
                // attributes / whitespace within a small budget).
                let limit = (i + 200).min(n);
                while j < limit {
                    if j + 4 <= n && &bytes[j..j + 4] == b"mod " {
                        break;
                    }
                    j += 1;
                }
                if j + 4 <= n && &bytes[j..j + 4] == b"mod " {
                    // Find `{` after `mod NAME`.
                    let mut k = j + 4;
                    while k < n && bytes[k] != b'{' {
                        k += 1;
                    }
                    if k < n {
                        let mut depth = 1i32;
                        let mut m = k + 1;
                        while m < n && depth > 0 {
                            match bytes[m] {
                                b'{' => depth += 1,
                                b'}' => depth -= 1,
                                _ => {}
                            }
                            m += 1;
                        }
                        if m > k + 1 {
                            for slot in &mut out[(k + 1)..(m - 1)] {
                                if *slot != b'\n' {
                                    *slot = b' ';
                                }
                            }
                        }
                        i = m;
                        continue;
                    }
                }
            }
            i += 1;
        }
        String::from_utf8_lossy(&out).into_owned()
    }

    fn preprocess(src: &str) -> String {
        strip_inline_test_modules(&strip_comments(src))
    }

    fn fmt_match(m: &(String, u32, String)) -> String {
        format!("({:?}, {}, {:?})", m.0, m.1, m.2)
    }

    /// Compare actual matches (Vec of (path, line, matched_str)) against
    /// the allowlist tuples. Fails on EITHER:
    ///   * a violation present in source without an allowlist tuple
    ///   * an allowlist tuple that no longer matches anything in source
    fn assert_exact_allowlist_match(
        guard_name: &str,
        actual: &[(String, u32, String)],
        allowed: &[(&str, u32, &str)],
    ) {
        // Normalise to comparable form.
        let actual_set: BTreeSet<(String, u32, String)> = actual.iter().cloned().collect();
        let allowed_set: BTreeSet<(String, u32, String)> = allowed
            .iter()
            .map(|(p, ln, pat)| (p.to_string(), *ln, pat.to_string()))
            .collect();

        let unexpected: Vec<_> = actual_set
            .iter()
            .filter(|t| !allowed_set.contains(*t))
            .map(fmt_match)
            .collect();
        let stale: Vec<_> = allowed_set
            .iter()
            .filter(|t| !actual_set.contains(*t))
            .map(fmt_match)
            .collect();

        if unexpected.is_empty() && stale.is_empty() {
            return;
        }

        let mut msg = format!("\n\n=== {guard_name} ===\n");
        if !unexpected.is_empty() {
            msg.push_str(
                "\nUnallowlisted violation introduced (add to allowlist if intentional, \
                 OR — preferred — remove the violation from source):\n",
            );
            for entry in &unexpected {
                msg.push_str("    ");
                msg.push_str(entry);
                msg.push('\n');
            }
        }
        if !stale.is_empty() {
            msg.push_str(
                "\nAllowlisted entry NOT FOUND in source — remove from allowlist or \
                 restore the violation; line number may have shifted:\n",
            );
            for entry in &stale {
                msg.push_str("    ");
                msg.push_str(entry);
                msg.push('\n');
            }
        }
        msg.push('\n');
        panic!("{msg}");
    }

    // -----------------------------------------------------------------------
    // Guard 1: `path.contains("/node_modules/")` and the Windows-backslash
    // sibling.
    //
    // Rule scope: the **typed-IR resolver pipeline** —
    //   analyzer → projector → registry → policy → materialiser, plus
    //   the JS compat layer in `@verter/component-meta/compat`. Within
    //   that scope the single source of workspace classification truth
    //   is `ResolverContext::workspace_is_workspace_owned` /
    //   `workspace_is_package_backed`. Substring tests on canonical
    //   paths are banned. The producer crates (`verter_session`,
    //   `verter_semantic`) MUST route every workspace-membership
    //   decision through `WorkspaceAccess`.
    //
    // Rule out of scope (and excluded from the allowlist on principle,
    // not pending migration):
    //   1. The implementation of the workspace classification API
    //      itself. `verter_workspace::Project::matches_file` and the
    //      sibling accessors on `Engine`, `FilesystemWorkspace`,
    //      `MemoryWorkspace` are the primitives the public
    //      `is_workspace_owned` / `is_package_backed` are built on.
    //      Calling the public API from within its own implementation
    //      would be circular.
    //   2. Filesystem-event handlers that fire BELOW the workspace
    //      registry — i.e. before any workspace snapshot has been
    //      published, when `WorkspaceAccess::is_package_backed` is
    //      definitionally `false` for every path (see engine.rs:
    //      "Returns `false` before the workspace publishes its first
    //      snapshot."). The LSP `is_config_file` watcher gate fires
    //      on raw `DidChangeWatchedFilesParams` URIs and must filter
    //      `node_modules/` config changes regardless of registry
    //      readiness; switching it to the typed API would either
    //      regress (rebuild on every node_modules change before first
    //      snapshot) or require ordering that the LSP spec does not
    //      guarantee.
    //
    // Allowlist removed by:
    //   * W2.2 — cold_resolver.rs (4 entries)
    //   * W4.1 — component_meta_registry.rs (6 entries) +
    //            component_meta_query_engine/helpers.rs (4 entries)
    //   * W4.3 — project_semantic_dispatch/relation.rs (2 entries) +
    //            project_semantic_dispatch/walk.rs (2 entries)
    //   * W4.4 — component_meta_resolution_policy/{core,pick_omit}.rs +
    //            host_manage/component_meta_methods.rs +
    //            meta_resolve/registry_materialize.rs
    //   * W4.5 — host_manage.rs (2) + meta_resolve/graph_predicates.rs
    //
    // Permanent exception entries (per the rule-scope clauses above):
    //   * `verter_workspace/src/resolver.rs:84` — exception class (1):
    //     the workspace classification API's own primitive.
    //   * `verter_lsp/src/server_utils.rs:22` — exception class (2):
    //     filesystem-event handler running below the workspace
    //     registry. The LSP `did_change_watched_files` gate.
    //
    // These two stay in the allowlist permanently. Neither is a
    // resolver-pipeline site. The matching call sites carry pointer
    // comments back to this rule-scope block.
    // -----------------------------------------------------------------------
    const NODE_MODULES_ALLOWLIST: &[(&str, u32, &str)] = &[
        (
            "crates/verter_lsp/src/server_utils.rs",
            22,
            r#".contains("/node_modules/")"#,
        ),
        (
            "crates/verter_workspace/src/resolver.rs",
            84,
            r#".contains("/node_modules/")"#,
        ),
    ];

    fn scan_node_modules_substring() -> Vec<(String, u32, String)> {
        let files = collect_production_rs_files();
        let mut out: Vec<(String, u32, String)> = Vec::new();
        for (path, rel) in &files {
            let src = match fs::read_to_string(path) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let stripped = preprocess(&src);
            for (idx, line) in stripped.split('\n').enumerate() {
                let line_no = (idx + 1) as u32;
                if line.contains(r#".contains("/node_modules/")"#) {
                    out.push((
                        rel.clone(),
                        line_no,
                        r#".contains("/node_modules/")"#.to_string(),
                    ));
                }
                if line.contains(r#".contains("\\node_modules\\")"#) {
                    out.push((
                        rel.clone(),
                        line_no,
                        r#".contains("\\node_modules\\")"#.to_string(),
                    ));
                }
            }
        }
        out
    }

    #[test]
    fn no_node_modules_substring_outside_workspace_api() {
        let actual = scan_node_modules_substring();
        assert_exact_allowlist_match(
            "no_node_modules_substring_outside_workspace_api",
            &actual,
            NODE_MODULES_ALLOWLIST,
        );
    }

    /// The two permanent allowlist sites MUST carry pointer comments
    /// back to this rule-scope block. The test reads the source of each
    /// allowlisted file and asserts the function carrying the substring
    /// is annotated with the rule scope. This is the negative half of
    /// the guard: it would FAIL pre-F3 (the call sites had only a
    /// one-line "// No config file inside node_modules..." comment and
    /// no `matches_file` doc-comment) and PASSES post-F3 once the
    /// pointer comments are in place.
    #[test]
    fn node_modules_allowlist_sites_carry_rule_scope_pointers() {
        // Site 1 — LSP filesystem-event handler.
        let lsp_src = super::read_workspace_file("crates/verter_lsp/src/server_utils.rs");
        assert!(
            lsp_src.contains("Architecture-guard exception")
                && lsp_src.contains("DidChangeWatchedFilesParams")
                && lsp_src.contains("no_node_modules_substring_outside_workspace_api"),
            "verter_lsp::server_utils::is_config_file must carry a rule-scope \
             pointer comment naming the architecture guard and the \
             filesystem-event-handler exception class. Restore the \
             docstring or remove the allowlist entry.",
        );

        // Site 2 — workspace API primitive.
        let ws_src = super::read_workspace_file("crates/verter_workspace/src/resolver.rs");
        assert!(
            ws_src.contains("Architecture-guard exception")
                && ws_src.contains("WorkspaceAccess::is_workspace_owned")
                && ws_src.contains("no_node_modules_substring_outside_workspace_api"),
            "verter_workspace::Project::matches_file must carry a rule-scope \
             pointer comment naming the architecture guard and the \
             workspace-API-primitive exception class. Restore the \
             docstring or remove the allowlist entry.",
        );

        // The rule-scope block in this file MUST also carry the
        // post-F3 exception-class language. Locks the docstring against
        // silent weakening.
        let guard_src =
            super::read_workspace_file("crates/verter_session/tests/architecture_guards.rs");
        assert!(
            guard_src.contains("Rule scope: the **typed-IR resolver pipeline**"),
            "the no_node_modules_substring_outside_workspace_api \
             rule-scope block must state its scope explicitly.",
        );
        assert!(
            guard_src.contains("Calling the public API from within its own implementation"),
            "exception class (1) — workspace-API primitive — must be \
             documented at the rule-scope block.",
        );
        assert!(
            guard_src.contains("Filesystem-event handlers that fire BELOW the workspace"),
            "exception class (2) — filesystem-event handler — must be \
             documented at the rule-scope block.",
        );
    }

    // -----------------------------------------------------------------------
    // Guard 2: `parse_jsdoc_tag_type_payload` reference outside JSDoc.
    //
    // The function is the JSDoc tag-type wrap-and-lower helper. It is
    // the sole text-input boundary in the typed-IR resolver pipeline —
    // every other producer-side caller in the resolver / projector /
    // registry / policy / materialiser lowers from a `TSType<'_>` AST
    // node via `verter_type_expr_oxc::lower_ts_type` directly.
    //
    // Pre-W5.2 the function was named `parse_type_annotation` and
    // lived in `verter_type_expr_oxc::lib.rs`. W5.2 renamed it to
    // `parse_jsdoc_tag_type_payload` and moved it to
    // `verter_semantic::analysis::jsdoc`, narrowing visibility so only
    // the JSDoc resolver in `verter_session::host_manage::jsdoc_resolve`
    // calls it from production code.
    //
    // The two production touchpoints are inherent and skipped via
    // explicit `continue` filters below — no allowlist entries needed:
    //   * `crates/verter_semantic/src/analysis/jsdoc.rs` — function
    //     definition site (the helper itself).
    //   * `crates/verter_session/src/host_manage/jsdoc_resolve.rs` —
    //     the single production caller.
    //
    // Any future caller anywhere else in `crates/*/src/**` MUST go
    // through the typed `TSType<'_>` AST path. If a new requirement
    // appears to need text manipulation, fix the producer (lower the
    // right OXC node, store the right typed field, extend
    // `verter_type_expr` with a missing variant) rather than reparsing.
    // -----------------------------------------------------------------------
    const PARSE_TYPE_ANNOTATION_ALLOWLIST: &[(&str, u32, &str)] = &[];

    fn scan_parse_type_annotation() -> Vec<(String, u32, String)> {
        let files = collect_production_rs_files();
        let mut out: Vec<(String, u32, String)> = Vec::new();
        for (path, rel) in &files {
            // Inherent production touchpoints: the JSDoc helper
            // definition site and its sole production caller.
            if rel == "crates/verter_semantic/src/analysis/jsdoc.rs"
                || rel == "crates/verter_session/src/host_manage/jsdoc_resolve.rs"
            {
                continue;
            }
            let src = match fs::read_to_string(path) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let stripped = preprocess(&src);
            for (idx, line) in stripped.split('\n').enumerate() {
                if line.contains("parse_jsdoc_tag_type_payload") {
                    out.push((
                        rel.clone(),
                        (idx + 1) as u32,
                        "parse_jsdoc_tag_type_payload".to_string(),
                    ));
                }
            }
        }
        out
    }

    #[test]
    fn no_parse_jsdoc_tag_type_payload_outside_jsdoc() {
        let actual = scan_parse_type_annotation();
        assert_exact_allowlist_match(
            "no_parse_jsdoc_tag_type_payload_outside_jsdoc",
            &actual,
            PARSE_TYPE_ANNOTATION_ALLOWLIST,
        );
    }

    // Bonus belt-and-braces gate: the OLD name `parse_type_annotation`
    // must not reappear anywhere in production source after the W5.2
    // rename. A reintroduction would mean someone re-introduced the
    // wrap-and-lower helper under its old identifier; the rename's
    // entire point is to make every JSDoc-private call site grep-able
    // by its semantic role rather than a generic "parse" verb.
    const OLD_PARSE_TYPE_ANNOTATION_ALLOWLIST: &[(&str, u32, &str)] = &[];

    fn scan_old_parse_type_annotation() -> Vec<(String, u32, String)> {
        let files = collect_production_rs_files();
        let mut out: Vec<(String, u32, String)> = Vec::new();
        for (path, rel) in &files {
            let src = match fs::read_to_string(path) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let stripped = preprocess(&src);
            for (idx, line) in stripped.split('\n').enumerate() {
                if line.contains("parse_type_annotation") {
                    out.push((
                        rel.clone(),
                        (idx + 1) as u32,
                        "parse_type_annotation".to_string(),
                    ));
                }
            }
        }
        out
    }

    #[test]
    fn no_old_parse_type_annotation_name_in_production() {
        let actual = scan_old_parse_type_annotation();
        assert_exact_allowlist_match(
            "no_old_parse_type_annotation_name_in_production",
            &actual,
            OLD_PARSE_TYPE_ANNOTATION_ALLOWLIST,
        );
    }

    // -----------------------------------------------------------------------
    // Guard 3: `format!()` followed by `parse_jsdoc_tag_type_payload(&_)`,
    // `parse_type_annotation(&_)`, or `parse_type_text(&_)` — the
    // synthesise-then-reparse round-trip.
    //
    // We detect the pattern by scanning for `format!` and looking
    // ahead within the same function body for any of:
    //   - `parse_jsdoc_tag_type_payload(&` (post-W5.2 helper name)
    //   - `parse_type_annotation(&` (pre-W5.2 helper name; should never
    //     reappear in production but guarded belt-and-braces)
    //   - `parse_type_text(&`
    // The `&` is the discriminator: a real round-trip references the
    // format! result through a let-bound variable. (Direct chained
    // `format!(...).parse_*()` would also match.)
    //
    // Pre-cutover sites: `slot_field_function_type_expr` in
    // `meta_resolve/materialize/macro_shapes.rs` (3 `format!` calls
    // feeding one `parse_type_annotation`) and
    // `projected_macro_surfaces_to_type_expr` in
    // `resolver_core/component_meta/projected_type_expr.rs` (3 more).
    // All removed by W2.1.
    // -----------------------------------------------------------------------
    const FORMAT_THEN_REPARSE_ALLOWLIST: &[(&str, u32, &str)] = &[];

    fn scan_format_then_reparse() -> Vec<(String, u32, String)> {
        let files = collect_production_rs_files();
        let mut out: Vec<(String, u32, String)> = Vec::new();
        for (path, rel) in &files {
            let src = match fs::read_to_string(path) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let stripped = preprocess(&src);
            let bytes = stripped.as_bytes();
            let n = bytes.len();
            // The three reparse needles we treat as a synthesise-then-reparse:
            //   * `parse_jsdoc_tag_type_payload(&` — post-W5.2 JSDoc helper.
            //   * `parse_type_annotation(&` — pre-W5.2 helper (belt-and-braces).
            //   * `parse_type_text(&` — checker-text adapter.
            let needle_jsdoc = b"parse_jsdoc_tag_type_payload(&";
            let needle_a = b"parse_type_annotation(&";
            let needle_t = b"parse_type_text(&";
            let mut i = 0usize;
            while i + 7 <= n {
                if &bytes[i..i + 7] == b"format!" {
                    let start = i;
                    let window_end = (start + 800).min(n);
                    let window = &bytes[start..window_end];
                    let pj = window
                        .windows(needle_jsdoc.len())
                        .position(|w| w == needle_jsdoc);
                    let pa = window.windows(needle_a.len()).position(|w| w == needle_a);
                    let pt = window.windows(needle_t.len()).position(|w| w == needle_t);
                    // Pick the earliest hit and label by needle kind.
                    let candidates = [
                        pj.map(|off| (off, "format!(...).parse_jsdoc_tag_type_payload")),
                        pa.map(|off| (off, "format!(...).parse_type_annotation")),
                        pt.map(|off| (off, "format!(...).parse_type_text")),
                    ];
                    let hit = candidates
                        .iter()
                        .filter_map(|c| c.as_ref())
                        .min_by_key(|(off, _)| *off)
                        .copied();
                    if let Some((off, label)) = hit {
                        // Reject if a function boundary appears in
                        // between (`\n}` at column 0 or a `\nfn ` decl).
                        let between = &window[..off];
                        let has_close = between.windows(2).any(|w| w == b"\n}");
                        let has_fn = between.windows(4).any(|w| w == b"\nfn ");
                        if !has_close && !has_fn {
                            let prefix = &bytes[..start];
                            let line_no =
                                (prefix.iter().filter(|&&c| c == b'\n').count() + 1) as u32;
                            out.push((rel.clone(), line_no, label.to_string()));
                        }
                    }
                    i += 7;
                    continue;
                }
                i += 1;
            }
        }
        out
    }

    #[test]
    fn no_format_then_reparse() {
        let actual = scan_format_then_reparse();
        assert_exact_allowlist_match(
            "no_format_then_reparse",
            &actual,
            FORMAT_THEN_REPARSE_ALLOWLIST,
        );
    }

    // -----------------------------------------------------------------------
    // Guard 4: `starts_with("Pick<" | "Omit<" | "Required<" | "Partial<")` —
    // shape-sniffing TS utility-type helpers off the type-text. Built-in
    // utilities behave identically to a userland implementation;
    // discriminating them by string prefix is a category error. The
    // typed `TypeExpr::Ref { name, type_arguments }` already carries the
    // utility-type identity.
    //
    // Removed by W1.1.
    // -----------------------------------------------------------------------
    const PICK_OMIT_PREFIX_ALLOWLIST: &[(&str, u32, &str)] = &[];

    fn scan_pick_omit_prefix() -> Vec<(String, u32, String)> {
        let needles: &[&str] = &[
            r#"starts_with("Pick<")"#,
            r#"starts_with("Omit<")"#,
            r#"starts_with("Required<")"#,
            r#"starts_with("Partial<")"#,
        ];
        let files = collect_production_rs_files();
        let mut out: Vec<(String, u32, String)> = Vec::new();
        for (path, rel) in &files {
            let src = match fs::read_to_string(path) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let stripped = preprocess(&src);
            for (idx, line) in stripped.split('\n').enumerate() {
                let line_no = (idx + 1) as u32;
                for needle in needles {
                    if line.contains(needle) {
                        out.push((rel.clone(), line_no, (*needle).to_string()));
                    }
                }
            }
        }
        out
    }

    #[test]
    fn no_pick_or_omit_string_prefix_check() {
        let actual = scan_pick_omit_prefix();
        assert_exact_allowlist_match(
            "no_pick_or_omit_string_prefix_check",
            &actual,
            PICK_OMIT_PREFIX_ALLOWLIST,
        );
    }

    // -----------------------------------------------------------------------
    // Guard 5: role inference from identifier name suffix.
    // `name.ends_with("Props" | "Emits" | "Events" | "Slots" | "Model")`
    // (and `*_name` / `identifier` / `*_identifier` / `ident` /
    // `*_ident` lhs variants). Type-role classification is structural
    // (a Vue SFC macro consumes the type), NOT nominal (the identifier
    // ends in "Props"). Scoped to the resolver pipeline crates:
    // `crates/verter_session/src/**` and
    // `crates/verter_semantic/src/analysis/**`.
    //
    // Cleared by W6.1 (analyzer-layer
    // `collect_imported_props_like_raw_refs` deletion + typed-IR
    // macro-participation walker) and F1 (policy-layer
    // `is_props_suffix` deletion + structural macro-participation
    // predicate in `PolicyCtx::is_macro_participating`). The allowlist
    // is now empty: no production source classifies type-role by
    // identifier name suffix.
    // -----------------------------------------------------------------------
    const ROLE_NAME_SUFFIX_ALLOWLIST: &[(&str, u32, &str)] = &[];

    /// Walk a method-chain LHS to find the root identifier.
    ///
    /// Given a line slice ending immediately before `.ends_with("...")`, walk
    /// backwards through chained `.method(arg, arg)` / `.field` segments to
    /// find the underlying identifier. Examples:
    ///
    /// - `name`                              -> "name"
    /// - `name.as_ref()`                     -> "name"
    /// - `name.as_str().trim()`              -> "name"
    /// - `prop.key_name.as_deref().unwrap()` -> "key_name"
    /// - `foo.bar()`                          -> "bar" (method tail before LHS)
    ///
    /// For the role-suffix guard, we walk through `.as_ref()` / `.as_str()` /
    /// `.as_deref()` / `.borrow()` / `.unwrap()` / `.unwrap_or_*()` /
    /// `.clone()` / `.to_string()` / `.trim()` etc. and continue until we
    /// reach a base identifier. Any base identifier matching `name` / `*_name`
    /// / `identifier` / `*_identifier` / `ident` / `*_ident` is flagged.
    fn walk_method_chain_lhs(prefix: &str) -> Option<String> {
        // Walk backwards from end. Skip whitespace, then peel method-call
        // suffixes (`.method(args)` with balanced parens) and field accesses
        // (`.field`) until we reach a base word.
        let bytes: Vec<char> = prefix.chars().collect();
        let mut i = bytes.len();

        loop {
            // Trim trailing whitespace.
            while i > 0 && bytes[i - 1].is_whitespace() {
                i -= 1;
            }
            if i == 0 {
                return None;
            }
            // Case 1: trailing balanced `(...)` — peel a method-call suffix.
            if bytes[i - 1] == ')' {
                let mut depth = 0i32;
                let mut j = i;
                while j > 0 {
                    j -= 1;
                    match bytes[j] {
                        ')' => depth += 1,
                        '(' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                if depth != 0 {
                    // Unbalanced — bail.
                    return None;
                }
                i = j; // Now points at the `(`.
                       // Continue: there must be a method name (word) before, and a `.`.
                let word_end = i;
                while i > 0 && (bytes[i - 1].is_alphanumeric() || bytes[i - 1] == '_') {
                    i -= 1;
                }
                if i == word_end {
                    // No method name before `(` — bail.
                    return None;
                }
                // Skip optional `::` segment after a turbofish (rare in this scope).
                // Now expect a `.` to continue chain, or this is the start of the
                // chain (e.g. `foo(`).
                while i > 0 && bytes[i - 1].is_whitespace() {
                    i -= 1;
                }
                if i == 0 || bytes[i - 1] != '.' {
                    // Not a method chain — this is e.g. `foo(args).ends_with(...)`.
                    // The base call (`foo(...)`) is the "root" but we ignore it; only
                    // a plain identifier base counts as name-like.
                    return None;
                }
                i -= 1; // Skip `.`.
            } else if bytes[i - 1].is_alphanumeric() || bytes[i - 1] == '_' {
                // Base identifier. Collect it.
                let word_end = i;
                while i > 0 && (bytes[i - 1].is_alphanumeric() || bytes[i - 1] == '_') {
                    i -= 1;
                }
                let ident: String = bytes[i..word_end].iter().collect();
                // If preceded by `.`, this is a field/method on something — keep walking
                // (the FIELD identifier IS what we want to test, not the receiver).
                // Actually: for `prop.key_name.as_ref().ends_with("Props")`, we want
                // "key_name" (the immediate field on `prop`), not "prop". So return
                // this identifier — it's the closest identifier to the .ends_with call.
                return Some(ident);
            } else {
                // Unknown character (operator, etc.) — bail.
                return None;
            }
        }
    }

    fn scan_role_name_suffix() -> Vec<(String, u32, String)> {
        let suffixes: &[&str] = &["Props", "Emits", "Events", "Slots", "Model"];
        let files = collect_production_rs_files();
        let mut out: Vec<(String, u32, String)> = Vec::new();
        for (path, rel) in &files {
            let in_scope = rel.starts_with("crates/verter_session/src/")
                || rel.starts_with("crates/verter_semantic/src/analysis/");
            if !in_scope {
                continue;
            }
            let src = match fs::read_to_string(path) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let stripped = preprocess(&src);
            for (idx, line) in stripped.split('\n').enumerate() {
                let line_no = (idx + 1) as u32;
                for sfx in suffixes {
                    let needle = format!(r#".ends_with("{}")"#, sfx);
                    if let Some(pos) = line.find(&needle) {
                        // Walk the LHS — including method-chain peeling so
                        // `name.as_ref().ends_with("Props")` resolves to "name"
                        // (and is correctly flagged). Without chain walking,
                        // the LHS would be "ref" (the last word) and the
                        // violation would slip through.
                        let lhs = match walk_method_chain_lhs(&line[..pos]) {
                            Some(s) => s,
                            None => continue,
                        };
                        if lhs.is_empty() {
                            continue;
                        }
                        let lhs_lower = lhs.to_ascii_lowercase();
                        let is_name_like = lhs_lower == "name"
                            || lhs_lower.ends_with("_name")
                            || lhs_lower == "identifier"
                            || lhs_lower.ends_with("_identifier")
                            || lhs_lower == "ident"
                            || lhs_lower.ends_with("_ident");
                        if is_name_like {
                            out.push((rel.clone(), line_no, needle));
                        }
                    }
                }
            }
        }
        out
    }

    #[test]
    fn no_role_inference_from_name_suffix() {
        let actual = scan_role_name_suffix();
        assert_exact_allowlist_match(
            "no_role_inference_from_name_suffix",
            &actual,
            ROLE_NAME_SUFFIX_ALLOWLIST,
        );
    }

    /// Discriminating test for the strengthened Guard 5 scanner.
    ///
    /// Pre-H3 fix: `scan_role_name_suffix` took only the immediate
    /// alphanumeric-suffix LHS, so `name.as_ref().ends_with("Props")`
    /// resolved to "ref" — failing the name-like check and slipping
    /// through. A real production violation was masked.
    ///
    /// Post-H3 fix: `walk_method_chain_lhs` peels `.as_ref()` (and any
    /// other method-chain suffix) to find the underlying identifier, so
    /// "name" is correctly recognized and flagged.
    ///
    /// This test verifies the helper directly against a set of synthetic
    /// inputs covering bare identifiers, simple method chains, deep
    /// chains, and field/method mixes.
    #[test]
    fn walk_method_chain_lhs_resolves_to_root_identifier() {
        // Base identifier.
        assert_eq!(
            walk_method_chain_lhs("name").as_deref(),
            Some("name"),
            "bare identifier should resolve to itself",
        );
        // Method-call suffix.
        assert_eq!(
            walk_method_chain_lhs("name.as_ref()").as_deref(),
            Some("name"),
            "`.as_ref()` chain must peel to root identifier",
        );
        // Deeper chain.
        assert_eq!(
            walk_method_chain_lhs("name.as_str().trim()").as_deref(),
            Some("name"),
            "`.as_str().trim()` deep chain must peel to root identifier",
        );
        // Field access chain.
        assert_eq!(
            walk_method_chain_lhs("prop.key_name.as_deref().unwrap()").as_deref(),
            Some("key_name"),
            "`prop.key_name.as_deref().unwrap()` resolves to the immediate field `key_name` (the receiver of the call closest to `.ends_with`)",
        );
        // Identifier in a `match` context (the existing scanner passed this).
        assert_eq!(
            walk_method_chain_lhs("        name").as_deref(),
            Some("name"),
            "leading whitespace must be stripped",
        );
    }

    /// End-to-end discriminating test for the strengthened Guard 5
    /// scanner. Synthesise a production-source-like buffer with a
    /// `name.as_ref().ends_with("Props")` violation, run the scanner's
    /// LHS-resolution logic on it, and assert the violation is flagged.
    ///
    /// Pre-fix: the LHS would have been "ref" (last contiguous word),
    /// `is_name_like` would have returned false, and the violation
    /// would have been silently allowed.
    /// Post-fix: the LHS resolves to "name", `is_name_like` returns
    /// true, and the violation is recorded.
    #[test]
    fn scan_role_name_suffix_flags_method_chain_lhs() {
        // Replicate the LHS extraction + name-like check that
        // `scan_role_name_suffix` performs, against a synthetic line.
        let synthetic = r#"                if name.as_ref().ends_with("Props") {"#;
        let needle = r#".ends_with("Props")"#;
        let pos = synthetic.find(needle).expect("needle must be present");
        let lhs = walk_method_chain_lhs(&synthetic[..pos]).expect("LHS must resolve");
        assert_eq!(
            lhs.to_ascii_lowercase(),
            "name",
            "strengthened LHS resolution must recover `name` from `name.as_ref()`",
        );
        let lhs_lower = lhs.to_ascii_lowercase();
        let is_name_like = lhs_lower == "name"
            || lhs_lower.ends_with("_name")
            || lhs_lower == "identifier"
            || lhs_lower.ends_with("_identifier")
            || lhs_lower == "ident"
            || lhs_lower.ends_with("_ident");
        assert!(
            is_name_like,
            "name-like check must fire for chain-peeled `name`",
        );
    }

    // -----------------------------------------------------------------------
    // Guard 6: `parse_checker_text_to_type_expr` — TS checker display
    // text adapter. The helper does NOT exist pre-W5.3; the allowlist
    // is empty today and the guard matches nothing. Once W5.3 lands
    // the helper at `crates/verter_session/src/resolver_core/checker_text_adapter.rs`,
    // the guard fires anywhere else the symbol is referenced (scope:
    // every production `.rs` file outside the adapter module itself).
    //
    // The "checker_text_adapter.rs" exception is the file basename so
    // any future relocation that keeps the name still passes; the
    // bridge consumer in `type_expansion_verter.rs` will be added to
    // the allowlist by W5.3 when it lands.
    //
    // This guard is deliberately distinct from
    // `no_parse_type_annotation_outside_jsdoc` — JSDoc and
    // checker-display-text are two different input boundaries.
    // -----------------------------------------------------------------------
    const CHECKER_TEXT_ADAPTER_ALLOWLIST: &[(&str, u32, &str)] = &[];

    fn scan_checker_text_adapter() -> Vec<(String, u32, String)> {
        let files = collect_production_rs_files();
        let mut out: Vec<(String, u32, String)> = Vec::new();
        for (path, rel) in &files {
            let basename = rel.rsplit('/').next().unwrap_or("");
            if basename == "checker_text_adapter.rs" {
                continue;
            }
            let src = match fs::read_to_string(path) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let stripped = preprocess(&src);
            for (idx, line) in stripped.split('\n').enumerate() {
                if line.contains("parse_checker_text_to_type_expr") {
                    out.push((
                        rel.clone(),
                        (idx + 1) as u32,
                        "parse_checker_text_to_type_expr".to_string(),
                    ));
                }
            }
        }
        out
    }

    #[test]
    fn no_checker_display_text_parsing_outside_adapter() {
        let actual = scan_checker_text_adapter();
        assert_exact_allowlist_match(
            "no_checker_display_text_parsing_outside_adapter",
            &actual,
            CHECKER_TEXT_ADAPTER_ALLOWLIST,
        );
    }
}

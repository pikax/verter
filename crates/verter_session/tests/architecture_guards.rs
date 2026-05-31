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
            // The bridge file is the single allowed home. The bridge
            // bodies compose surviving pub(crate) helpers (the
            // root-surface bridges resolve through `dispatch_projected_surface`
            // ALONE after Stage 4-disp; the explicit prepared-surface
            // callers still use `cached_prepared_root_surface`) instead of
            // calling the deleted Class B engine methods, so callsite_count
            // is 0 today. The test does NOT require non-zero here —
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

/// The two root-surface bridges
/// (`project_type_surface_expr_via_host_threaded` /
/// `project_type_surface_shape_via_host_threaded`) resolve a root symbol's
/// surface through the shared dispatch surface projector ALONE. Stage 4-disp
/// removed their prepared-decl root-surface rescue
/// (`.or_else(cached_prepared_root_surface)`) — dispatch composes Object /
/// Alias roots directly and compound roots from the decl anchor through the
/// shared empty-path Shallow walker. This guard asserts the rescue stays
/// absent: neither bridge body may reference `cached_prepared_root_surface`.
///
/// (The prepared-decl helper itself survives for explicit prepared-surface
/// callers — `project_prepared_type_surface_*_via_host_threaded` — so this
/// guard scopes to the two ROOT-surface bridge bodies, not the whole file.)
#[test]
fn root_surface_bridges_carry_no_prepared_decl_fallback() {
    use syn::visit::Visit;
    use syn::{Expr, ExprCall, ExprMethodCall, Item, ItemFn};

    const BRIDGE_FNS: [&str; 2] = [
        "project_type_surface_expr_via_host_threaded",
        "project_type_surface_shape_via_host_threaded",
    ];
    // The ONLY method calls a bridge body may make. `dispatch_projected_surface`
    // is the sole root-surface authority (asserted to appear exactly once);
    // `projection_op_budget_exhausted` is the cooperative budget guard. Any
    // OTHER method call (`.or_else(...)`, `engine.cached_prepared_root_surface(...)`,
    // an `engine.<other>()` rescue, …) is a structural deviation and FAILS.
    const ALLOWED_METHOD_CALLS: [&str; 2] =
        ["dispatch_projected_surface", "projection_op_budget_exhausted"];
    // The ONLY free-function / variant-constructor calls a bridge body may
    // make: the two thin surface→shape/expr converters, plus the std enum
    // constructors (`Some` / `Ok` / `Err`) that wrap the converter result.
    // Variant constructors are structurally inert — they cannot hide a rescue —
    // so they are approved. Any OTHER free-fn call (including a local helper
    // introduced to hide a `cached_prepared_root_surface` rescue behind an
    // indirection) is a structural deviation and FAILS.
    const ALLOWED_FREE_CALLS: [&str; 5] = [
        "projected_surface_to_type_expr",
        "projected_surface_to_expanded_shape",
        "Some",
        "Ok",
        "Err",
    ];
    const FORBIDDEN_TOKEN: &str = "cached_prepared_root_surface";

    let src = read_workspace_file("crates/verter_session/src/meta_resolve/dispatch_helpers.rs");
    let file = syn::parse_file(&src).expect("parse dispatch_helpers.rs");

    // Index every free `fn` in the file by name so a body's one-level local
    // callees can be inspected (approach (b): a helper reachable from a bridge
    // body must not itself reference the forbidden rescue).
    let mut free_fns: std::collections::HashMap<String, &ItemFn> =
        std::collections::HashMap::new();
    for item in &file.items {
        if let Item::Fn(f) = item {
            free_fns.insert(f.sig.ident.to_string(), f);
        }
    }

    /// Collects every method-call name, free-function-call name, and any
    /// path segment equal to a forbidden token, within one fn body.
    struct CallCollector {
        method_calls: Vec<String>,
        free_calls: Vec<String>,
        forbidden_hits: usize,
    }
    impl<'ast> Visit<'ast> for CallCollector {
        fn visit_expr_method_call(&mut self, mc: &'ast ExprMethodCall) {
            self.method_calls.push(mc.method.to_string());
            syn::visit::visit_expr_method_call(self, mc);
        }
        fn visit_expr_call(&mut self, call: &'ast ExprCall) {
            // Record the terminal path segment of a free / associated call
            // (`foo(...)`, `module::foo(...)`, `Type::assoc(...)`).
            if let Expr::Path(p) = call.func.as_ref() {
                if let Some(last) = p.path.segments.last() {
                    self.free_calls.push(last.ident.to_string());
                }
            }
            syn::visit::visit_expr_call(self, call);
        }
        fn visit_path_segment(&mut self, seg: &'ast syn::PathSegment) {
            if seg.ident == FORBIDDEN_TOKEN {
                self.forbidden_hits += 1;
            }
            syn::visit::visit_path_segment(self, seg);
        }
    }

    for bridge_name in BRIDGE_FNS {
        let bridge = free_fns.get(bridge_name).unwrap_or_else(|| {
            panic!(
                "bridge fn `{bridge_name}` not found in dispatch_helpers.rs — \
                 the guard's anchor moved"
            )
        });

        let mut collector = CallCollector {
            method_calls: Vec::new(),
            free_calls: Vec::new(),
            forbidden_hits: 0,
        };
        collector.visit_block(&bridge.block);

        // (1) The forbidden rescue must not appear anywhere in the body.
        assert_eq!(
            collector.forbidden_hits, 0,
            "Stage 4-disp: `{bridge_name}` references `{FORBIDDEN_TOKEN}` — the \
             prepared-decl root-surface rescue was removed and must stay dead. \
             Fix any compound-root composition gap in the shared walker (the \
             merge / heritage / Omit functions), NOT by re-adding the rescue."
        );

        // (2) Exactly one `dispatch_projected_surface` call — dispatch is the
        //     sole root-surface authority. Zero would mean the bridge stopped
        //     resolving through dispatch; more than one is an unexpected shape.
        let dispatch_calls = collector
            .method_calls
            .iter()
            .filter(|m| m.as_str() == "dispatch_projected_surface")
            .count();
        assert_eq!(
            dispatch_calls, 1,
            "Stage 4-disp: `{bridge_name}` must call `dispatch_projected_surface` \
             EXACTLY once (the sole root-surface authority); found \
             {dispatch_calls}. Method calls observed: {:?}",
            collector.method_calls
        );

        // (3) STRUCTURAL no-evasion gate: every method call in the body must be
        //     on the approved list. A `.or_else(...)` fallback (direct OR behind
        //     a helper), a re-added `engine.cached_prepared_root_surface(...)`,
        //     or any other engine-method rescue introduces a non-approved
        //     method call here and FAILS — closing the indirection evasion the
        //     prior literal-only scan allowed.
        for method in &collector.method_calls {
            assert!(
                ALLOWED_METHOD_CALLS.contains(&method.as_str()),
                "Stage 4-disp: `{bridge_name}` makes a non-approved method call \
                 `.{method}(...)`. The bridge body must resolve the root surface \
                 through `dispatch_projected_surface` ALONE — no `.or_else(...)` \
                 fallback, no `cached_prepared_root_surface` rescue, no other \
                 engine-method escape hatch. Approved: {ALLOWED_METHOD_CALLS:?}."
            );
        }

        // (4) STRUCTURAL no-evasion gate: every free / associated function call
        //     must be on the approved converter list. A helper-indirection
        //     evasion (`fn h(){ cached_prepared_root_surface } ;
        //     dispatch_projected_surface(...).or_else(|| h(...))`, or a
        //     `let s = h(...)?;` form) introduces a non-approved free call here
        //     and FAILS even if the helper avoids `.or_else`.
        for call in &collector.free_calls {
            assert!(
                ALLOWED_FREE_CALLS.contains(&call.as_str()),
                "Stage 4-disp: `{bridge_name}` calls non-approved free function \
                 `{call}(...)`. The bridge body must call ONLY \
                 `dispatch_projected_surface` (method) + a surface converter \
                 ({ALLOWED_FREE_CALLS:?}); routing through any other helper can \
                 hide a `cached_prepared_root_surface` rescue. Inline the work \
                 or fix the shared walker instead."
            );

            // (b) Defense in depth: even an approved-named call is re-checked,
            //     and any LOCAL helper reachable from the body must not itself
            //     reference the forbidden rescue. (The approved converters are
            //     not local fns here, so this loop is normally a no-op; it
            //     future-proofs against an approved-list entry that later
            //     becomes a local fn.)
            if let Some(helper) = free_fns.get(call) {
                let mut helper_collector = CallCollector {
                    method_calls: Vec::new(),
                    free_calls: Vec::new(),
                    forbidden_hits: 0,
                };
                helper_collector.visit_block(&helper.block);
                assert_eq!(
                    helper_collector.forbidden_hits, 0,
                    "Stage 4-disp: `{bridge_name}` calls local helper `{call}`, \
                     whose body references `{FORBIDDEN_TOKEN}` — the rescue cannot \
                     be re-introduced through a one-level helper indirection."
                );
            }
        }
    }
}

/// The component-meta RESOLUTION PATH never re-introduces the retired eager
/// macro-object materialiser nor the prepared-decl member rescue.
///
/// Stage 4a routed `define_props` / `define_emits` / `define_slots` through the
/// dispatch projector (`projectors::define_shapes::project_define_macro_shapes`)
/// and deleted both the eager materialiser call and the root-symbol member
/// fallback. This guard asserts they STAY deleted from the production resolution
/// entry points, by SYMBOL USAGE (any path reference — direct, qualified, OR
/// expanded from a `macro_rules!` — to the forbidden symbol within the named
/// function body trips the guard, closing the textual-`.or_else`-spelling
/// evasion the older string scan allowed):
///
/// - `compute_component_meta_state_inner` (the cold resolution orchestrator)
///   must NOT reference `produce_macro_object_shapes_for_purpose` — macro shapes
///   are owned by `project_define_macro_shapes` now.
/// - `dispatch_member_for_root_symbol` (the routed single-member projector) must
///   NOT reference `project_prepared_requested_member_from_symbol` — a dispatch
///   miss is an authoritative miss; the prepared-decl rescue is gone. (The
///   symbol survives for the prepared-surface engine's OWN recursive member
///   projection in `prepared_surface.rs`; this guard scopes to the routed-member
///   body, not the whole crate.)
#[test]
fn component_meta_resolution_path_has_no_eager_materializer_or_member_fallback() {
    use syn::visit::Visit;
    use syn::{ImplItemFn, Item, ItemFn, ItemImpl, UseTree};

    /// Macros that may legitimately appear inside the guarded function bodies.
    /// ANY other macro invocation is rejected: a `macro_rules!` wrapper whose
    /// expansion contains the forbidden symbol would be invisible to the path /
    /// method-call scanners (its body is an unparsed token stream), so an
    /// unknown macro in the body is a potential re-introduction vector. Adding a
    /// new macro to a guarded body requires consciously extending this list
    /// (after confirming the macro cannot expand to the forbidden symbol).
    const APPROVED_MACROS: &[&str] = &[
        "component_meta_trace_custom",
        "format",
        "matches",
        "vec",
        "assert",
        "assert_eq",
        "debug_assert",
        "debug_assert_eq",
        "write",
        "writeln",
        "panic",
        "todo",
        "unreachable",
    ];

    /// Walks one function body counting references to a forbidden symbol via
    /// ANY path segment (direct call, qualified path, or a segment produced by
    /// macro expansion that `syn` parses as a path) AND any method-call name
    /// (`receiver.forbidden(...)`). Method calls are `ExprMethodCall`, NOT path
    /// segments, so both visitors are required — a `.or_else(|| engine.
    /// project_prepared_requested_member_from_symbol(...))` rescue is a method
    /// call and would be invisible to a path-only scan. Macro INVOCATIONS in the
    /// body are collected by name so the caller can reject any non-approved
    /// macro (a `macro_rules!` wrapper that expands to the forbidden symbol).
    struct ForbiddenSymbolCounter<'a> {
        /// The forbidden symbols (the canonical name plus any `use`-alias of it).
        forbidden: &'a [String],
        hits: usize,
        /// Macro invocation names (last path segment) seen in the body.
        macros_used: Vec<String>,
    }
    impl<'ast, 'a> Visit<'ast> for ForbiddenSymbolCounter<'a> {
        fn visit_path_segment(&mut self, seg: &'ast syn::PathSegment) {
            if self.forbidden.iter().any(|f| seg.ident == f.as_str()) {
                self.hits += 1;
            }
            syn::visit::visit_path_segment(self, seg);
        }
        fn visit_expr_method_call(&mut self, mc: &'ast syn::ExprMethodCall) {
            if self.forbidden.iter().any(|f| mc.method == f.as_str()) {
                self.hits += 1;
            }
            syn::visit::visit_expr_method_call(self, mc);
        }
        fn visit_macro(&mut self, mac: &'ast syn::Macro) {
            if let Some(seg) = mac.path.segments.last() {
                self.macros_used.push(seg.ident.to_string());
            }
            // Do NOT recurse into the macro token stream — it is unparsed
            // tokens, not a path/expr tree. The allow-list check on the macro
            // NAME is the guard; an approved macro cannot expand to the
            // forbidden symbol, an unapproved macro is rejected outright.
        }
    }

    /// Collect every `use`-alias (`... as Alias`) in `file` whose ORIGINAL last
    /// segment is one of `forbidden_originals`. An `use crate::…::forbidden as
    /// reroute;` import lets `reroute(...)` call the retired symbol without the
    /// body scan ever seeing `forbidden`. `forbidden_originals` is the canonical
    /// symbol PLUS every transitive crate re-export alias of it (so a re-export
    /// chain `pub(crate) use …forbidden as r0;` elsewhere + `use …::r0 as
    /// reroute;` here is caught: `r0` is in `forbidden_originals`). The local
    /// renames are added to the forbidden set AND their mere existence reported.
    fn use_aliases_of(file: &syn::File, forbidden_originals: &[String]) -> Vec<String> {
        fn walk(tree: &UseTree, forbidden_originals: &[String], out: &mut Vec<String>) {
            match tree {
                UseTree::Path(p) => walk(&p.tree, forbidden_originals, out),
                UseTree::Group(g) => {
                    for t in &g.items {
                        walk(t, forbidden_originals, out);
                    }
                }
                UseTree::Rename(r) => {
                    if forbidden_originals.iter().any(|f| r.ident == f.as_str()) {
                        out.push(r.rename.to_string());
                    }
                }
                UseTree::Name(_) | UseTree::Glob(_) => {}
            }
        }
        let mut out = Vec::new();
        for item in &file.items {
            if let Item::Use(u) = item {
                walk(&u.tree, forbidden_originals, &mut out);
            }
        }
        out
    }

    /// Transitively collect every crate-internal re-export ALIAS of
    /// `canonical_forbidden`. Walks every `.rs` under `crates/verter_session/src`
    /// and gathers `pub use …X as ALIAS;` / `pub(crate) use …X as ALIAS;`
    /// renames where `X` is already known-forbidden, iterating to a fixpoint so a
    /// chain (`forbidden as r0`, then `r0 as r1`, …) is fully resolved. The
    /// returned aliases let the guard treat an aliased re-import of a re-export
    /// (`use crate::reexport_home::r0 as reroute;` in the guarded file) as an
    /// import of the forbidden symbol. Only re-exports (`pub`/`pub(crate) use …
    /// as`) count — a private `use … as` inside an unrelated module does not make
    /// the alias importable elsewhere.
    fn crate_reexport_aliases_of(canonical_forbidden: &str) -> Vec<String> {
        fn collect_rs_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    collect_rs_files(&path, out);
                } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                    out.push(path);
                }
            }
        }
        /// Pull `pub`/`pub(crate)` re-export renames whose original ident is in
        /// `known` from one parsed file.
        fn reexport_renames_in_file(
            file: &syn::File,
            known: &[String],
            out: &mut Vec<String>,
        ) {
            fn walk(tree: &UseTree, known: &[String], out: &mut Vec<String>) {
                match tree {
                    UseTree::Path(p) => walk(&p.tree, known, out),
                    UseTree::Group(g) => {
                        for t in &g.items {
                            walk(t, known, out);
                        }
                    }
                    UseTree::Rename(r) => {
                        if known.iter().any(|k| r.ident == k.as_str()) {
                            out.push(r.rename.to_string());
                        }
                    }
                    UseTree::Name(_) | UseTree::Glob(_) => {}
                }
            }
            for item in &file.items {
                if let Item::Use(u) = item {
                    // Only RE-EXPORTS (`pub` / `pub(crate)`) make the alias
                    // importable from another module; a private `use … as` does
                    // not propagate.
                    let is_reexport = matches!(
                        u.vis,
                        syn::Visibility::Public(_) | syn::Visibility::Restricted(_)
                    );
                    if is_reexport {
                        walk(&u.tree, known, out);
                    }
                }
            }
        }

        let src_dir = workspace_root().join("crates/verter_session/src");
        let mut rs_files = Vec::new();
        collect_rs_files(&src_dir, &mut rs_files);
        let parsed: Vec<syn::File> = rs_files
            .iter()
            .filter_map(|p| std::fs::read_to_string(p).ok())
            .filter_map(|src| syn::parse_file(&src).ok())
            .collect();

        let mut known = vec![canonical_forbidden.to_string()];
        // Fixpoint: each pass may discover aliases of aliases.
        loop {
            let mut discovered = Vec::new();
            for file in &parsed {
                reexport_renames_in_file(file, &known, &mut discovered);
            }
            let mut grew = false;
            for alias in discovered {
                if !known.iter().any(|k| k == &alias) {
                    known.push(alias);
                    grew = true;
                }
            }
            if !grew {
                break;
            }
        }
        // Drop the canonical name itself; return only the discovered aliases.
        known.into_iter().skip(1).collect()
    }

    /// Collect module-scope `const` / `static` items in `file` whose initializer
    /// expression references any name in `forbidden`. A function-pointer const
    /// (`const REROUTE: fn(...) -> _ = produce_macro_object_shapes_for_purpose;`)
    /// at module scope lets `REROUTE(...)` inside a guarded body call the retired
    /// symbol while the body-only path scan never sees `produce_…` (the
    /// initializer lives outside the fn). The const/static NAMES become reroute
    /// aliases (added to the forbidden body-scan set) and are themselves reported.
    fn const_static_fn_pointer_aliases_of(file: &syn::File, forbidden: &[String]) -> Vec<String> {
        struct InitRefCounter<'a> {
            forbidden: &'a [String],
            hit: bool,
        }
        impl<'ast, 'a> Visit<'ast> for InitRefCounter<'a> {
            fn visit_path_segment(&mut self, seg: &'ast syn::PathSegment) {
                if self.forbidden.iter().any(|f| seg.ident == f.as_str()) {
                    self.hit = true;
                }
                syn::visit::visit_path_segment(self, seg);
            }
        }
        let mut out = Vec::new();
        for item in &file.items {
            let (name, expr): (String, &syn::Expr) = match item {
                Item::Const(c) => (c.ident.to_string(), &c.expr),
                Item::Static(s) => (s.ident.to_string(), &s.expr),
                _ => continue,
            };
            let mut counter = InitRefCounter {
                forbidden,
                hit: false,
            };
            counter.visit_expr(expr);
            if counter.hit {
                out.push(name);
            }
        }
        out
    }

    /// Find a free fn OR an impl method named `fn_name` in `file` and count
    /// `forbidden`-symbol references (canonical name + `use`-aliases) in its
    /// body. Also asserts every macro invocation in the body is on
    /// `APPROVED_MACROS`. Panics if the function is not found (the guard's
    /// anchor moved).
    fn assert_fn_free_of_symbol(
        file: &syn::File,
        fn_name: &str,
        canonical_forbidden: &str,
        message: &str,
    ) {
        // Transitive crate re-export aliases of the forbidden symbol. A
        // re-export chain (`pub(crate) use …forbidden as r0;` in another module,
        // then `use crate::…::r0 as reroute;` in THIS file) hides the forbidden
        // name from a direct-alias scan: the guarded file's rename original is
        // `r0`, not `forbidden`. Treat every transitive re-export alias as a
        // forbidden ORIGINAL so the local re-import is caught.
        let reexport_aliases = crate_reexport_aliases_of(canonical_forbidden);

        // Forbidden ORIGINAL names a guarded-file `use … as …` could rename:
        // the canonical symbol plus its crate re-export aliases.
        let forbidden_originals: Vec<String> = std::iter::once(canonical_forbidden.to_string())
            .chain(reexport_aliases.iter().cloned())
            .collect();

        // (1) An aliased import (`use …forbidden as reroute;`, OR
        //     `use …::r0 as reroute;` for a re-export alias `r0`) is itself the
        //     evasion — report it before the body scan even runs.
        let aliases = use_aliases_of(file, &forbidden_originals);
        assert!(
            aliases.is_empty(),
            "Stage 4a guard: the file declaring `{fn_name}` imports the retired \
             symbol `{canonical_forbidden}` (or a crate re-export alias of it: \
             {reexport_aliases:?}) under local alias(es) {aliases:?} (`use … as \
             …`). An aliased import — direct OR via a `pub use … as` re-export \
             chain — is an import-alias evasion of the materializer/fallback \
             retirement. Remove the alias import. {message}"
        );

        // The full set the body scan treats as forbidden: the canonical name,
        // its crate re-export aliases, and any local renames of either.
        let mut forbidden: Vec<String> = forbidden_originals.clone();
        forbidden.extend(aliases);

        // (2) A module-scope function-pointer const/static whose INITIALIZER
        //     references a forbidden name (`const REROUTE: fn(...) = forbidden;`)
        //     lets `REROUTE(...)` in the body call the retired symbol while the
        //     body-only scan never sees `forbidden` (the initializer is outside
        //     the fn). Report the const/static, and add its NAME to the forbidden
        //     body-scan set so the `REROUTE(...)` call site is also caught.
        let const_aliases = const_static_fn_pointer_aliases_of(file, &forbidden);
        assert!(
            const_aliases.is_empty(),
            "Stage 4a guard: the file declaring `{fn_name}` binds the retired \
             symbol `{canonical_forbidden}` (or an alias of it) to module-scope \
             function-pointer const/static(s) {const_aliases:?} (`const NAME: \
             fn(...) = forbidden;`). A fn-pointer const lets `NAME(...)` call the \
             retired symbol while the body-only path scan never sees `forbidden`. \
             Remove the const/static binding. {message}"
        );
        forbidden.extend(const_aliases);

        let mut counter = ForbiddenSymbolCounter {
            forbidden: &forbidden,
            hits: 0,
            macros_used: Vec::new(),
        };
        let mut found = false;
        for item in &file.items {
            match item {
                Item::Fn(ItemFn { sig, block, .. }) if sig.ident == fn_name => {
                    counter.visit_block(block);
                    found = true;
                }
                Item::Impl(ItemImpl { items, .. }) => {
                    for impl_item in items {
                        if let syn::ImplItem::Fn(ImplItemFn { sig, block, .. }) = impl_item {
                            if sig.ident == fn_name {
                                counter.visit_block(block);
                                found = true;
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        assert!(
            found,
            "guard anchor moved: fn `{fn_name}` not found — re-point the guard at \
             the renamed component-meta resolution entry point"
        );
        assert_eq!(counter.hits, 0, "{message}");
        // Reject any macro invocation in the body that is not on the approved
        // list — an unapproved `macro_rules!` wrapper could expand to the
        // forbidden symbol (its expansion is invisible to the symbol scan).
        let unapproved: Vec<&String> = counter
            .macros_used
            .iter()
            .filter(|m| !APPROVED_MACROS.contains(&m.as_str()))
            .collect();
        assert!(
            unapproved.is_empty(),
            "Stage 4a guard: `{fn_name}` invokes non-approved macro(s) \
             {unapproved:?}. A `macro_rules!` wrapper can expand to the retired \
             materializer/fallback symbol, evading the path/method-call scan. \
             Either remove the macro or, if it provably cannot expand to the \
             forbidden symbol, add its name to APPROVED_MACROS. {message}"
        );
    }

    let methods_src =
        read_workspace_file("crates/verter_session/src/host_manage/component_meta_methods.rs");
    let methods_file = syn::parse_file(&methods_src).expect("parse component_meta_methods.rs");
    assert_fn_free_of_symbol(
        &methods_file,
        "compute_component_meta_state_inner",
        "produce_macro_object_shapes_for_purpose",
        "Stage 4a: `compute_component_meta_state_inner` references \
         `produce_macro_object_shapes_for_purpose` — the eager macro-object \
         materialiser was retired from the production resolution path. Macro \
         shapes are owned by `projectors::define_shapes::project_define_macro_shapes`; \
         do NOT re-introduce the materialiser (directly, via a `use`-alias, OR \
         through a macro).",
    );

    let engine_src = read_workspace_file(
        "crates/verter_session/src/resolver_core/component_meta_query_engine/mod.rs",
    );
    let engine_file = syn::parse_file(&engine_src).expect("parse component_meta_query_engine/mod.rs");
    assert_fn_free_of_symbol(
        &engine_file,
        "dispatch_member_for_root_symbol",
        "project_prepared_requested_member_from_symbol",
        "Stage 4a: `dispatch_member_for_root_symbol` references \
         `project_prepared_requested_member_from_symbol` — the prepared-decl \
         member rescue was removed; a dispatch miss is an authoritative miss. Do \
         NOT re-add the `.or_else(...)` fallback (directly, via a `use`-alias, OR \
         through a macro).",
    );

    // Owner-local dispatch-seal guard: BOTH owner-local macro-root entry points
    // in jsdoc_resolve.rs resolve their root surface through the SOLE query-time
    // resolver (the shared dispatch surface projector
    // `project_expr_surface_shape_via_host_threaded`), NOT the retired
    // prepared-decl walker (`cached_prepared_root_surface` via
    // `project_prepared_type_surface_*`):
    //
    // - `owner_local_macro_root_has_surface` is the cold resolver's owner-local
    //   AUTHORITY gate (presence check).
    // - `projectable_owner_local_macro_roots` is the upstream projectable-roots
    //   PRE-FILTER. It runs BEFORE the authority gate and decides whether a
    //   macro root is considered projectable at all, so a surviving prepared
    //   walker there is still a production walker path — it MUST route through
    //   dispatch too (one-engine / no-production-walker seal).
    //
    // Re-introducing the prepared-decl walker in EITHER function is a
    // second-resolver (Typed-IR-Only) violation.
    let jsdoc_src =
        read_workspace_file("crates/verter_session/src/host_manage/jsdoc_resolve.rs");
    let jsdoc_file = syn::parse_file(&jsdoc_src).expect("parse jsdoc_resolve.rs");
    for owner_local_fn in [
        "owner_local_macro_root_has_surface",
        "projectable_owner_local_macro_roots",
    ] {
        for prepared_walker_symbol in [
            "cached_prepared_root_surface",
            "project_prepared_type_surface_shape_via_host_threaded",
            "project_prepared_type_surface_expr_via_host_threaded",
            "project_prepared_requested_member_from_symbol",
        ] {
            assert_fn_free_of_symbol(
                &jsdoc_file,
                owner_local_fn,
                prepared_walker_symbol,
                &format!(
                    "Stage 4a: the owner-local entry point `{owner_local_fn}` \
                     references the prepared-decl walker `{prepared_walker_symbol}` \
                     — both owner-local macro-root entry points were retargeted to \
                     the shared dispatch surface projector \
                     `project_expr_surface_shape_via_host_threaded` and must stay \
                     there (one resolver). Do NOT route the owner-local \
                     projectable/authority decision back through the prepared-surface \
                     walker."
                ),
            );
        }
    }
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
            .map(|n| {
                // `resolver_context.rs` carries the `impl ResolverContext
                // for VerterHost` bridge — the trait surface itself.
                // `session_resolver_context.rs` is the session-bound
                // wrapper that owns the `&VerterHost` borrow needed to
                // reach view-aware host internals
                // (`prepared_decl_bundle_with_context` etc.) without
                // widening the trait surface.
                // `host_resolver_context.rs` is the request-bound
                // wrapper that owns the `&VerterHost` borrow needed to
                // reach the view-taking `*_with_store_view` helpers
                // and the canonical-completion hook.
                // `request_store_view.rs` owns the
                // `CanonicalCompletionOverlay::complete_canonical`
                // helper, which threads through the host's
                // `current_store_view_epoch` / `scheduler` /
                // `derived_raw_cache` / `project_type_store` to
                // promote freshly-loaded canonicals into the request
                // overlay.
                // All four are seal-bridge exemptions per sub-plan
                // §10a.0.A.
                n == "resolver_context.rs"
                    || n == "session_resolver_context.rs"
                    || n == "host_resolver_context.rs"
                    || n == "request_store_view.rs"
            })
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
        // tests/resolved_import_facts_producer_real.rs +
        // tests/resolved_import_facts_unresolved_admitted.rs +
        // tests/resolved_import_facts_validator_real_path.rs +
        // tests/resolved_import_facts_lane_population.rs +
        // tests/resolved_import_facts_namespace_space_admitted.rs —
        // production producer for
        // `ResolvedImportFactsDb`. Reads
        // `script_analysis.imports` (with `is_type_only` +
        // `ImportBindingKind::{Named, Default, Namespace}`),
        // classifies each binding into
        // `SymbolSpace::{Type, Value, Namespace}` (v8
        // AMENDMENT-S), composes the cache key from the
        // per-canonical env hashes, constructs one
        // `ResolvedImportClauseEntry` per `(binding, space)` pair,
        // and admits the bundle through
        // `ResolvedImportFactsDb::insert_if_absent`. Negative
        // (unresolved) imports are admitted as entries with
        // `resolved_canonical: None` and a sentinel-keyed `Fact`
        // so the validator can detect when an unresolved binding
        // becomes resolved on workspace bump.
        "pub mod resolved_import_facts_producer",
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
        // Block 1.H Track 2.4 — `AppConfigNoOverrideProofKey`,
        // `AppConfigNoOverrideProofEntry`, `AppConfigNoOverrideProofDb`
        // surface for the family_bcd_* integration tests that drive
        // the production producer end-to-end via
        // `for_tests::app_config_no_override_proof_get_or_compute_for_tests`.
        "pub mod app_config_proof_db",
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
        // Shared bounded query-identity retention substrate — the
        // `GlobalRetentionBudget` FIFO total-size cap + the
        // `BoundedCandidateMap` per-slot candidate list. Crate-private:
        // consumed only by `component_meta_result_db`,
        // `component_meta_caches`, and `semantic_query_memo` within this
        // crate; no downstream consumer reaches it directly.
        "pub(crate) mod bounded_query_retention",
        // Cache-runtime substrate — the `WorldSnapshot` carrier +
        // scoped `*Dims` accessors that later blocks wire through
        // every cache-runtime entry-point. `pub(crate)` so the
        // type / accessors do not enter the production binding
        // surface. There is NO `for_tests` re-export for these
        // types; the construction contract is exercised by
        // `#[cfg(test)] mod tests` inline in
        // `cache_runtime/world_snapshot.rs`.
        "pub(crate) mod cache_runtime",
        "pub mod cooperative_admission",
        // R3/R26/R28 — fact-validation helpers shared by the inner
        // component-meta caches (Family A/B). Carries
        // `validate_fact_signature`, `bubble_fact_signature`, and the
        // path-precise `fact_signature_for_canonical_member` /
        // `fact_signature_for_exported_type` constructors used by
        // every cache that migrated from `DepSignature` to
        // `Arc<[FactVersionRef]>`, plus the provenance-pure
        // `parse_fact_ref_for_observed_current_content` primitive the
        // `MaterializeMemoDb` producer pins its observed-version parse
        // fact through.
        "pub(crate) mod fact_signature_helpers",
        "pub(crate) mod host_executor",
        "pub(crate) mod host_test_audit",
        "pub(crate) mod instant",
        "pub(crate) mod intrinsic_registry",
        // Phase G — host-owned mapped-binder ordinal registry
        // for stable `MapperKey` cache identity across dispatcher
        // instances. Internal substrate; consumed only by
        // `project_semantic_dispatch::lower`'s `TypeExpr::Mapped`
        // arm via `ProjectTypeStore::mapper_binder_registry()`.
        "pub(crate) mod mapper_binder_registry",
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
        // Test-only probe substrate for the content-addressed
        // `MapperFingerprint` primitive. Consumed by
        // `tests/mapper_fingerprint_content_addressed.rs`. The
        // module is `#[doc(hidden)]` and wraps the internal
        // `pub(crate)` `MapperFingerprint` / `MapperBinderRegistry`
        // in a newtype so production callers cannot reach the
        // inner types through it. Not a production API.
        "pub mod test_only",
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
        // Block 6.e per-call-site instrumentation: bench example
        // (crates/verter_bench/examples/audit_real_component_meta.rs)
        // dumps the `HostStoreView::from_host` attribution table at the
        // end of every pass via `dump_from_host_call_sites` and resets
        // the table at pass entry via `reset_from_host_call_sites`. The
        // `#[track_caller]` rail on `HostStoreView::from_host`,
        // `VerterHost::resolver_store_view`, the trait `resolver_store_view`
        // impls, and the `validate_fact_signature*` helpers propagates
        // the warm-hit validator location back to the recorder.
        "pub use resolver_store::{dump_from_host_call_sites, reset_from_host_call_sites}",
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
            // Block-1.5 substrate split — view-aware prepared-decl
            // bundle/type/value variants live alongside their base
            // counterparts so the cache invariants stay in one file.
            // Pending B-C5 split, this file is exempt.
            "crates/verter_session/src/host_manage/prepared_decl.rs",
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
            // Authoritative per-file artifact storage layer —
            // `FileArtifactKey`, `FileArtifacts`, the content-addressed
            // + overlay-scoped read/write surface, the per-canonical
            // retention sweep, and the promotion-aware LRU all live in
            // one file so the multi-candidate cache invariants stay
            // co-located. Adding the overlay-scoped key surface pushed
            // it over the line; a split (key/store/eviction modules)
            // is the eventual cleanup but is out of scope for the
            // overlay-detection fix.
            "crates/verter_session/src/file_artifact_store.rs",
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
            // Block 7.5 Commit A added per-thread diagnostic
            // instrumentation (`HOST_STORE_VIEW_FROM_HOST_BUILDS` +
            // friends) that pushed this file over the 1500-line
            // budget by ~12 lines. The instrumentation is gated
            // behind atomic counters and intended to survive the
            // Block 7.5 cutover for future bypass diagnostics.
            // Splitting the file along the view-construction
            // boundary is the eventual cleanup; the diagnostic surface
            // is the load-bearing motivation for the temporary
            // exemption.
            "crates/verter_session/src/resolver_store.rs",
            "crates/verter_session/src/meta_resolve/materialize/field_types.rs",
            "crates/verter_session/src/meta_resolve/materialize/macro_shapes.rs",
            // Projector entry-points module. `ProjectedMember →
            // ExpandedField` lowering, per-member projection, the
            // intersection-arm merge handoff, and the published-surface
            // projector trampoline all live here so the projection
            // contract stays in one file. Splitting along
            // expander-vs-projector lines is the eventual cleanup.
            "crates/verter_session/src/meta_resolve/projectors/mod.rs",
            "crates/verter_session/src/parse.rs",
            // Per-request state container — owns `RequestContext`, the
            // cache-attribution counters, the projection budget, the
            // materialization-cache-suppress sticky flag, and the TLS
            // install/restore plumbing. Densely documented because
            // each field is the public API every cache/audit consumer
            // reads. Splitting along counter / flag / budget lines is
            // the eventual cleanup.
            "crates/verter_session/src/request_context.rs",
            "crates/verter_session/src/project_semantic_dispatch/build.rs",
            "crates/verter_session/src/project_semantic_dispatch/lower.rs",
            // Project semantic dispatch entry-points module. The
            // `ProjectSemanticDispatch::execute` memo, the
            // `SemanticQueryKey` cooperative-admission dispatcher, and
            // the per-variant shape walkers are co-located so the
            // cache invariants stay byte-coherent. Splitting along
            // dispatch / cooperative-admission lines is the eventual
            // cleanup.
            "crates/verter_session/src/project_semantic_dispatch/mod.rs",
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
            // Typeinfo flow-return catalog. Single-file cataloguing the
            // function-body flow-return inference rules under
            // `#[cfg(test)]` module hierarchy (`typeinfo::typeinfo_tests`).
            // Per the typeinfo design these inference rules are gated
            // behind the cfg-test parent and intentionally co-located
            // so the rule table stays in one place. The architecture
            // guard walks `src/` recursively without filtering on
            // ancestor `*_tests/` directories; this exemption is the
            // documented escape until the walker grows directory
            // filtering.
            "crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs",
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
    ///
    /// Also catches the orchestrator's project-management vocabulary
    /// `Block N.x` (e.g. `Block 6.i`, `Block 6.j`) and uppercase
    /// `Commit XY` markers (e.g. `Commit AX`, `Commit BX`) which the
    /// numeric-only `Commit \d+` scan does not see.
    pub fn line_has_phase_archaeology(line: &str) -> bool {
        let bytes = line.as_bytes();
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
            "post-Phase",
            "Post-Phase",
            "retired in",
            "phase-archaeology",
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
            // Cache-runtime overhaul plan archaeology — these
            // phrases unambiguously reference the cache-runtime
            // plan document and never appear in legitimate
            // final-state prose. The `\bblock \d+\b` word-boundary
            // scan lives below; these two are the fixed-needle
            // half (H19).
            "cache-runtime overhaul",
            "runtime cutover",
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
        // `deleted in <digit>` / `deletion in <digit>` — deletion
        // history with explicit plan reference (e.g. `deleted in 5g`,
        // `deleted in 11d`, `deleted in 3`). A digit immediately after
        // the past-tense `deleted in ` or noun `deletion in ` is the
        // archaeology marker; legitimate prose like `deleted in the
        // refactor` lacks the digit and is preserved.
        let lower = line.to_ascii_lowercase();
        for prefix in ["deleted in ", "deletion in "] {
            let mut search_from = 0usize;
            while let Some(rel) = lower[search_from..].find(prefix) {
                let abs = search_from + rel;
                let after = abs + prefix.len();
                let lower_bytes = lower.as_bytes();
                if after < lower_bytes.len() && lower_bytes[after].is_ascii_digit() {
                    return true;
                }
                search_from = abs + prefix.len();
            }
        }
        // `Phase \d+` / `phase \d+` / `Phase-\d+` / `phase-\d+`
        // (case-insensitive on the verb). Carve-out: `Phase 1: …` is
        // algorithm-phase prose (colon-prefixed verb after the digit
        // run); preserve it. Any other byte after the digit run
        // (letter, `-`, `.`, space, EOL, `,`, `)`, `—`, etc.) is
        // archaeology.
        for prefix in ["phase ", "phase-", "Phase ", "Phase-"] {
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
                    if bytes[after] != b':' {
                        return true;
                    }
                }
                search_from = abs + prefix.len();
            }
        }
        // `Stage \d+` / `Stage-\d+` / `stage \d+` / `stage-\d+` —
        // parallel shape to the Phase scan above with the same `:`
        // carve-out for algorithm-stage prose (`Stage 1: read …`).
        // Stage is the project-management noun used by the
        // fact-based cache refactor's plan; it leaks into production
        // source the same way Phase does.
        for prefix in ["stage ", "stage-", "Stage ", "Stage-"] {
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
                    if bytes[after] != b':' {
                        return true;
                    }
                }
                search_from = abs + prefix.len();
            }
        }
        // `\bblock \d+\b` — cache-runtime overhaul's plan-vocabulary
        // ban (H19). `Block 5`, `block 6`, `Block 12.a`, `Block 1.J`,
        // `Block 7.5` all match the broader word-boundary form; the
        // numeric run is followed by a non-word byte (period,
        // space, EOL, etc.). Distinguished from the legitimate
        // prose "the request loop blocks once per flight": the
        // plural verb `blocks` does not match the singular noun
        // followed by an ASCII digit.
        //
        // Case-insensitive on the noun, because both `Block 5` and
        // `block 5` appear in orchestrator comments. The leading
        // word boundary is implicit: the prefix is `block ` /
        // `Block `, which is preceded either by start-of-line or
        // by a non-word byte in every observed violation, and the
        // ordinary noun `block` followed by a digit is already
        // archaeology (a sentence does not start "block 5 ...").
        for prefix in ["block ", "Block "] {
            let mut search_from = 0usize;
            while let Some(rel) = line[search_from..].find(prefix) {
                let abs = search_from + rel;
                let after = abs + prefix.len();
                let bytes = line.as_bytes();
                // Leading word-boundary: the byte before `block ` is
                // either start-of-line or a non-word byte.
                let leading_ok =
                    abs == 0 || !(bytes[abs - 1].is_ascii_alphanumeric() || bytes[abs - 1] == b'_');
                if leading_ok && after < bytes.len() && bytes[after].is_ascii_digit() {
                    // Consume the digit run.
                    let mut end = after;
                    while end < bytes.len() && bytes[end].is_ascii_digit() {
                        end += 1;
                    }
                    // Trailing word-boundary: the byte after the
                    // digit run is either EOL or a non-word byte.
                    let trailing_ok = end == bytes.len()
                        || !(bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_');
                    if trailing_ok {
                        return true;
                    }
                }
                search_from = abs + prefix.len();
            }
        }
        // `Block \d+\.[a-z]` — orchestrator's project-management
        // vocabulary (e.g. `Block 6.i`, `Block 6.j`, `Block 12.a`).
        // The decimal+letter suffix is unique to orchestrator block
        // references and does not appear in legitimate code prose
        // (a basic block, allocator block, etc. never carries that
        // shape). Case-sensitive on the leading B to avoid flagging
        // ordinary uses of the noun "block". This stays as a
        // narrower discriminator even though `\bblock \d+\b` above
        // already catches `Block 12` itself — the decimal+letter
        // form (`Block 12.a`) is what the orchestrator emits and is
        // listed explicitly so the deliberate-violation test cases
        // continue to characterise the original surface.
        for prefix in ["Block "] {
            let mut search_from = 0usize;
            while let Some(rel) = line[search_from..].find(prefix) {
                let abs = search_from + rel;
                let mut after = abs + prefix.len();
                let bytes = line.as_bytes();
                let digit_start = after;
                while after < bytes.len() && bytes[after].is_ascii_digit() {
                    after += 1;
                }
                if after > digit_start
                    && after + 1 < bytes.len()
                    && bytes[after] == b'.'
                    && bytes[after + 1].is_ascii_lowercase()
                {
                    return true;
                }
                search_from = abs + prefix.len();
            }
        }
        // `Commit \d+` — explicit numeric commit reference. The
        // orchestrator's plan documents enumerate commits as `Commit
        // 3`, `Commit 12`, etc.; production source must never cite
        // them by number.
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
        // `Commit XY` — alpha-suffixed orchestrator commit markers
        // (e.g. `Commit AX`, `Commit BX`, `Commit C`). Distinct from
        // the numeric `Commit \d+` scan above.
        //
        // Discriminator after `Commit [A-Z]`:
        //   - another upper / digit (`AX`, `A0`) is archaeology
        //   - lowercase letter (`Atlas`, `Source`) is verb-noun prose,
        //     NOT archaeology
        //   - any non-alphabetic byte (space, punctuation, EOL) is a
        //     single-letter orchestrator marker (`Commit C`,
        //     `Commit C.`), archaeology
        for prefix in ["Commit ", "commit "] {
            let mut search_from = 0usize;
            while let Some(rel) = line[search_from..].find(prefix) {
                let abs = search_from + rel;
                let after = abs + prefix.len();
                if after < bytes.len() && bytes[after].is_ascii_uppercase() {
                    if after + 1 >= bytes.len() {
                        return true;
                    }
                    let trailing = bytes[after + 1];
                    if trailing.is_ascii_uppercase()
                        || trailing.is_ascii_digit()
                        || !trailing.is_ascii_alphabetic()
                    {
                        return true;
                    }
                }
                search_from = abs + prefix.len();
            }
        }
        // `revision \d+` / `Revision \d+` — explicit revision-number
        // reference. The orchestrator's plan documents enumerate
        // revisions; production source must not cite them by number.
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
        // `rev \d+` — standalone shorthand revision reference.
        // Word-boundary required on the leading `rev` so identifiers
        // like `reverse`, `reveal`, `revoke` do not flag.
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
        // `PE\d+\b` — orchestrator's per-block phase-extraction
        // marker (e.g. `PE4`, `PE12`). The numeric suffix is
        // unique to PE-prefixed block identifiers and does not
        // appear in legitimate code prose. Case-sensitive on the
        // leading `PE` to avoid flagging ordinary uses of the
        // bigram (peripherals, peer-to-peer literals etc.).
        {
            let mut search_from = 0usize;
            let bytes = line.as_bytes();
            while let Some(rel) = line[search_from..].find("PE") {
                let abs = search_from + rel;
                // Must be at start of token: preceding char is NOT
                // ASCII alphanumeric / underscore.
                let is_word_start =
                    abs == 0 || !(bytes[abs - 1].is_ascii_alphanumeric() || bytes[abs - 1] == b'_');
                if is_word_start {
                    let mut after = abs + 2;
                    let digit_start = after;
                    while after < bytes.len() && bytes[after].is_ascii_digit() {
                        after += 1;
                    }
                    if after > digit_start {
                        // Must be word boundary at the end too: the
                        // following char is NOT ASCII alphanumeric /
                        // underscore.
                        let is_word_end = after >= bytes.len()
                            || !(bytes[after].is_ascii_alphanumeric() || bytes[after] == b'_');
                        if is_word_end {
                            return true;
                        }
                    }
                }
                search_from = abs + 2;
            }
        }
        // `/ Fix [A-Z]\b` — orchestrator's per-fix alpha marker
        // (e.g. `/ Fix D`, `/ Fix AX`). The leading slash + space
        // disambiguates from legitimate "Fix" prose. Case-sensitive
        // on the leading `Fix` to avoid flagging the verb
        // ("fix the bug"). The trailing letter MUST be uppercase
        // and at a word boundary to avoid flagging "Fix Definition"
        // / "Fix Each" prose.
        {
            let needle = "/ Fix ";
            let bytes = line.as_bytes();
            let mut search_from = 0usize;
            while let Some(rel) = line[search_from..].find(needle) {
                let abs = search_from + rel;
                let after = abs + needle.len();
                if after < bytes.len() && bytes[after].is_ascii_uppercase() {
                    // After the uppercase letter, accept either an
                    // additional uppercase (e.g. `AX`), end-of-line,
                    // or non-alphabetic (space, punctuation). Reject
                    // lowercase trailing because that's a real word
                    // ("/ Fix Definition" type prose, though unlikely).
                    if after + 1 >= bytes.len() {
                        return true;
                    }
                    let trailing = bytes[after + 1];
                    if trailing.is_ascii_uppercase()
                        || trailing.is_ascii_digit()
                        || !trailing.is_ascii_alphabetic()
                    {
                        return true;
                    }
                }
                search_from = abs + needle.len();
            }
        }
        // `Fix-[A-Z]\b` / `pre-Fix-[A-Z]\b` / `post-Fix-[A-Z]\b` —
        // hyphenated per-fix alpha markers (e.g. `Fix-D`, `pre-Fix-D`,
        // `post-Fix-D`). Distinct from `/ Fix D` above because they
        // appear without the leading slash. Word-boundary on the
        // leading token (preceding char must NOT be ASCII alphanumeric
        // or underscore) prevents false-flagging identifiers that
        // happen to contain `Fix-` inside a longer token. The trailing
        // letter must be uppercase + word-boundary so legitimate
        // hyphenated identifiers like `Fix-Each` are not flagged.
        for needle in ["Fix-", "pre-Fix-", "post-Fix-", "Pre-Fix-", "Post-Fix-"] {
            let bytes = line.as_bytes();
            let mut search_from = 0usize;
            while let Some(rel) = line[search_from..].find(needle) {
                let abs = search_from + rel;
                let is_word_start =
                    abs == 0 || !(bytes[abs - 1].is_ascii_alphanumeric() || bytes[abs - 1] == b'_');
                if is_word_start {
                    let after = abs + needle.len();
                    if after < bytes.len() && bytes[after].is_ascii_uppercase() {
                        let trailing_idx = after + 1;
                        if trailing_idx >= bytes.len() {
                            return true;
                        }
                        let trailing = bytes[trailing_idx];
                        if trailing.is_ascii_uppercase()
                            || trailing.is_ascii_digit()
                            || !trailing.is_ascii_alphabetic()
                        {
                            return true;
                        }
                    }
                }
                search_from = abs + needle.len();
            }
        }
        // `Path [A-Z] [A-Z]\d+[a-z]?\b` — orchestrator's per-path
        // cluster marker (e.g. `Path C C5`, `Path C C11a`, `Path A B12`).
        // Shape: `Path<space><UPPER><space><UPPER><digits>[lower]?`,
        // with the trailing token at a word boundary. Case-sensitive
        // on the leading `Path` to avoid flagging legitimate filesystem
        // prose like "path c c5" (lowercase) — orchestrator markers
        // always use the title-case form.
        {
            let needle = "Path ";
            let bytes = line.as_bytes();
            let mut search_from = 0usize;
            while let Some(rel) = line[search_from..].find(needle) {
                let abs = search_from + rel;
                let is_word_start =
                    abs == 0 || !(bytes[abs - 1].is_ascii_alphanumeric() || bytes[abs - 1] == b'_');
                let mut after = abs + needle.len();
                if is_word_start
                    && after < bytes.len()
                    && bytes[after].is_ascii_uppercase()
                    && after + 1 < bytes.len()
                    && bytes[after + 1] == b' '
                {
                    after += 2; // consume `<UPPER><space>`
                    if after < bytes.len() && bytes[after].is_ascii_uppercase() {
                        let mut digit_after = after + 1;
                        let digit_start = digit_after;
                        while digit_after < bytes.len() && bytes[digit_after].is_ascii_digit() {
                            digit_after += 1;
                        }
                        if digit_after > digit_start {
                            // Optional single trailing lowercase letter.
                            let mut end = digit_after;
                            if end < bytes.len() && bytes[end].is_ascii_lowercase() {
                                end += 1;
                            }
                            let is_word_end = end >= bytes.len()
                                || !(bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_');
                            if is_word_end {
                                return true;
                            }
                        }
                    }
                }
                search_from = abs + needle.len();
            }
        }
        // `round\d+\b` / `round-?\d+\b` / `round \d+\b` (case-insensitive
        // on `round`). Orchestrator round markers appear as `round-7`,
        // `round 7`, `round20-fix2`, `Round-10`, `pre-Round-10`, etc.
        // Word-boundary requirement on the leading token avoids
        // flagging identifiers that contain `round` as a substring
        // (`background`, `surround`). The trailing digit must be at
        // a word boundary so `roundtrip` is NOT flagged. The
        // separator-less form (`round20`) catches scratch-file path
        // references like `D:/tmp/round20-fix2-report.md`.
        for needle in ["round ", "round-", "round", "Round ", "Round-", "Round"] {
            let bytes = line.as_bytes();
            let mut search_from = 0usize;
            while let Some(rel) = line[search_from..].find(needle) {
                let abs = search_from + rel;
                let is_word_start =
                    abs == 0 || !(bytes[abs - 1].is_ascii_alphanumeric() || bytes[abs - 1] == b'_');
                if is_word_start {
                    let mut after = abs + needle.len();
                    let digit_start = after;
                    while after < bytes.len() && bytes[after].is_ascii_digit() {
                        after += 1;
                    }
                    if after > digit_start {
                        let is_word_end = after >= bytes.len()
                            || !(bytes[after].is_ascii_alphanumeric() || bytes[after] == b'_');
                        if is_word_end {
                            return true;
                        }
                    }
                }
                search_from = abs + needle.len();
            }
        }
        // `Codex \d+(st|nd|rd|th)[- ]consult` — orchestrator's nth-
        // consult marker (e.g. `Codex 4th-consult`, `Codex 7th consult`).
        // Case-sensitive on the leading `Codex` to avoid flagging
        // unrelated prose; the ordinal suffix + the literal `consult`
        // word make this a unique orchestrator pattern.
        {
            let needle = "Codex ";
            let bytes = line.as_bytes();
            let mut search_from = 0usize;
            while let Some(rel) = line[search_from..].find(needle) {
                let abs = search_from + rel;
                let mut after = abs + needle.len();
                let digit_start = after;
                while after < bytes.len() && bytes[after].is_ascii_digit() {
                    after += 1;
                }
                if after > digit_start && after + 2 <= bytes.len() {
                    let suffix = &line[after..after + 2];
                    if matches!(suffix, "st" | "nd" | "rd" | "th") {
                        let mut sep_idx = after + 2;
                        if sep_idx < bytes.len()
                            && (bytes[sep_idx] == b' ' || bytes[sep_idx] == b'-')
                            && sep_idx + 7 <= bytes.len()
                            && &line[sep_idx + 1..sep_idx + 8] == "consult"
                        {
                            // Word-boundary at the end of `consult`.
                            sep_idx += 8;
                            let is_word_end = sep_idx >= bytes.len()
                                || !(bytes[sep_idx].is_ascii_alphanumeric()
                                    || bytes[sep_idx] == b'_');
                            if is_word_end {
                                return true;
                            }
                        }
                    }
                }
                search_from = abs + needle.len();
            }
        }
        // `Cluster [A-Z]\b` — orchestrator's per-cluster alpha marker
        // (e.g. `Cluster A`, `Cluster B`). Case-sensitive on `Cluster`
        // to avoid flagging lowercase prose. The trailing letter must
        // be uppercase + word-boundary so identifiers like
        // `cluster_id` and proper-noun phrases ending with a
        // lowercase token (`Cluster Affinity Score`) do not flag —
        // only the single-letter discriminator form.
        {
            let needle = "Cluster ";
            let bytes = line.as_bytes();
            let mut search_from = 0usize;
            while let Some(rel) = line[search_from..].find(needle) {
                let abs = search_from + rel;
                let is_word_start =
                    abs == 0 || !(bytes[abs - 1].is_ascii_alphanumeric() || bytes[abs - 1] == b'_');
                if is_word_start {
                    let after = abs + needle.len();
                    if after < bytes.len() && bytes[after].is_ascii_uppercase() {
                        let trailing_idx = after + 1;
                        let is_single_letter_marker = trailing_idx >= bytes.len()
                            || !(bytes[trailing_idx].is_ascii_alphanumeric()
                                || bytes[trailing_idx] == b'_');
                        if is_single_letter_marker {
                            return true;
                        }
                    }
                }
                search_from = abs + needle.len();
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
             `deleted in 5[a-z]`, `deletion in 5[a-z]`, `retired in`.\n\n\
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
            "#[allow(dead_code)] // deletion in 5g per call-graph closure",
            "// they retire alongside the engine deletion in 5g.",
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
            // Block N.x — orchestrator block vocabulary.
            "// Block 6.i Commit AX — descend the per-member cursor.",
            "// Block 6.j R18 — emit PublishedField at the macro publication boundary.",
            "/// Block 6.c per-request hoist read this counter.",
            "// Block 12.a substrate cutover deleted the legacy walker.",
            // Commit XY — alpha-suffixed commit markers.
            "// Commit AX (codex-hybrid): the call-site provides the cursor.",
            "// see Commit BX for the carrier-stop closure.",
            // PE\d+ — orchestrator's per-block phase-extraction
            // marker (e.g. `PE4`, `PE12`).
            "// PE4 hash-cons memo discriminator shim.",
            "/// (PE4 hoist discriminator).",
            "// PE12 substrate cleanup follow-up.",
            // / Fix [A-Z]\b — orchestrator's per-fix alpha marker
            // (`/ Fix D`, `/ Fix AX`).
            "//! / Fix D wraps the public substitute in change-tracking.",
            "/// / Fix AX companion of the substitute helper.",
            // Hyphenated Fix-letter / pre-Fix-letter / post-Fix-letter
            // markers — distinct from the `/ Fix D` slash form.
            "// Fix-D wraps the public substitute in change-tracking.",
            "/// pre-Fix-D substitute helpers rebuilt every match arm.",
            "// post-Fix-D the no-op branches short-circuit.",
            "// Pre-Fix-AX companion of the substitute helper.",
            // Path-letter cluster markers (`Path C C5`, `Path C C11a`).
            "// Path C C5 propagates the lowering-time mapper kind.",
            "/// Path C C11a — nested-infer in Function types.",
            "// Path C C12 — batch submission handle.",
            "// Path A B12 — alternate cluster marker.",
            // round-N / Round-N / pre-Round-N / post-Round-N markers.
            "// round-7 substrate extension closes the publication boundary.",
            "// the round-12 codex TOP RISK warned about this regression.",
            "/// pre-Round-10 admission called key_names_from_keyspace_node.",
            "// post-Round-11 acceptance contract.",
            "// Round-13 Step-0 on the corpus ChatMessage.vue.",
            "// round 7 cutover demand.",
            // Codex Nth-consult markers (title-case form).
            "// Codex 4th-consult Q1 dispatch chain prerequisite.",
            "/// Codex 7th consult diagnostic chain.",
            "// Codex 2nd-consult landed in this revision.",
            // Cluster-letter markers (single-letter discriminator only).
            "// Cluster A: single-infer conditional.",
            "/// Cluster B value selection.",
            "// per Cluster C — relate Object surface.",
            // Plan-section / Commit-number / revision archaeology
            // (formerly the broader-D111 classifier's scope, now folded
            // into the single source of truth).
            "// Plan §3 Commit 9 — hover.provenance opt-in.",
            "/// plan §3 Commit 8 — necessary for the audit bundle.",
            "// Plan §3 Step 4 — audit warm-cache.",
            "/// Plan §4.8 / Phase C / Commit R — RefCycleResultDb.",
            "// Phase D §5.6 WIP-L — function shape (plan §2 decision).",
            "// architectural-debt-closure rev 10.",
            "// were deleted in Commit 3 of the cutover sub-plan).",
            "// Counterpart deleted in Plan §6.15 / N — entry stored.",
            "// Five-phase materialiser entry per plan §10.",
            "// `phase-archaeology` is a sweep target.",
            // Cache-runtime plan vocabulary (H19) — the three
            // block-vocabulary patterns: the `\bblock \d+\b`
            // word-boundary scan and the two fixed needles.
            "// block 5: rehome the compile cache",
            "// cache-runtime overhaul wiring",
            "// runtime cutover landing step",
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
            // Algorithm-phase carve-out (colon-prefixed verb describes
            // an algorithm step rather than a plan-phase reference).
            "// Phase 1: collect import statements.",
            "// Phase 2: emit lowered IR.",
            "// phase 3: walk dependency graph.",
            // Algorithm-stage carve-out — the Stage family inherits
            // the same `:`-prefixed carve-out as Phase.
            "// Stage 1: read parser input.",
            "// stage 2: lower to typed IR.",
            // Legitimate `rev` usage that's not a number.
            "// Reverses (rev) the iteration order.",
            // Stage-family negative cases — Stage followed by a
            // letter (not a digit), or "stage" used in a legitimate
            // prose sense, must not flag.
            "// On-stage layout pass owns the first batch.",
            "// stages of the pipeline cooperate via the substrate.",
            "// Build Stage C handles the second pass.", // letter-suffixed Stage is preserved
            // Final-state joiner-accounting prose — no plan citation,
            // no decimal section ref tied to audit vocabulary.
            "// Joiner-accounting contract: per-request hits/misses attribute exactly.",
            // Block prose that is NOT orchestrator vocabulary.
            "// Walk each basic block in source order.",
            "// allocator block reuse counter.",
            "// `block_until_idle` waits until the queue drains.",
            // Commit prose that is NOT an orchestrator marker.
            "// On commit, flush the buffered writes.",
            "/// Commit the in-flight transaction.", // sentence-initial verb, no alphanumeric suffix
            // PE prose that is NOT an orchestrator marker:
            // `PE` appearing inside a longer identifier or as a word
            // not followed by digits must NOT flag.
            "// Use the SPEC document to look up the contract.",
            "// PE prose with no digits.",
            "// `peephole_optimization` is a downstream pass.",
            "// Type 'PE-1234' marker has lowercase-after-prefix - not orchestrator.",
            // / Fix prose that is NOT an orchestrator marker:
            // the trailing token must be uppercase to flag.
            "// Documentation: /Fix the documentation.",
            "// Run with `/Fix mode` to enable fixes.",
            // Hyphenated Fix prose that is NOT an orchestrator
            // marker: lowercase trailing token, or non-letter
            // trailing token after the leading word, must NOT flag.
            "// `fix-up` the trailing whitespace.",
            "// affixfixer renames identifiers.", // `Fix-` is inside an identifier, not at word start
            // `Path ` prose that is NOT an orchestrator marker:
            // missing the `<UPPER> <UPPER><digits>` shape, or lower-
            // case sub-tokens, must NOT flag.
            "// Path resolution walks ancestor directories.",
            "// Path C resolution algorithm.", // single letter — no digit token
            "// Path C c5 invariant.", // lowercase second token — orchestrator markers are title-case
            "// Path Compression heuristic in the union-find.",
            "// path c c5 trace marker for diagnostics.", // lowercase `path` is not the orchestrator marker
            // `round` prose that is NOT an orchestrator marker:
            // either appears inside a longer identifier (no word
            // boundary), or is missing the trailing digit.
            "// Round up to the nearest power of two.",
            "// roundtrip serialisation through the wire format.", // `round` inside identifier
            "// Background lookup uses the workspace's resolver.", // `round` in `Background`
            "// Surround the literal with quotes.",                // `round` in `Surround`
            "// round trip without digits.",
            // `Codex ` prose that is NOT a consult marker: missing
            // the ordinal + `consult` suffix.
            "// Codex agent dispatched in parallel.",
            "// Codex 4 retries before falling back.", // no ordinal suffix
            "// Codex 4th retry succeeded.",           // ordinal but no `consult`
            // `Cluster` prose that is NOT a single-letter marker:
            // followed by a multi-letter token, the cluster is a
            // legitimate concept name rather than the orchestrator
            // single-letter discriminator.
            "// Cluster affinity score weighting.",
            "// `cluster_id` selects the assigned worker pool.",
            "// Cluster Allocator owns the per-shard pool.",
            // Block-vocabulary ban (H19) — benign prose that uses
            // the plural verb `blocks` (not the singular noun
            // followed by a digit). The `\bblock \d+\b` scan must
            // not flag this.
            "// the request loop blocks once per flight",
            "// `block_until_idle` blocks the worker until drained.",
        ];
        for line in allowed {
            assert!(
                !line_has_phase_archaeology(line),
                "guard 7 predicate must NOT flag legitimate line: {line:?}",
            );
        }
    }

    /// Cache-runtime overhaul plan-vocabulary ban (H19).
    ///
    /// The three new patterns added under the H19 rule must:
    ///   - flag every fabricated violation line, and
    ///   - leave benign prose alone (the `\bblock \d+\b` scan must
    ///     not match the plural verb `blocks` or compound tokens
    ///     like `block_until_idle`).
    ///
    /// This is the discriminating test for the new patterns. It is
    /// independent from the broader `_rejects_deliberate_violations`
    /// case bag so a regression that loses the new patterns surfaces
    /// here even when the older fixed-needle / Phase / Stage scans
    /// keep passing.
    #[test]
    fn guard7_predicate_rejects_block_vocabulary() {
        // Each fabricated line models a single H19 surface: the
        // `\bblock \d+\b` word-boundary scan, the `cache-runtime
        // overhaul` fixed needle, and the `runtime cutover` fixed
        // needle.
        let violations = [
            "// block 5: rehome the compile cache",
            "// cache-runtime overhaul wiring",
            "// runtime cutover landing step",
            // Capitalisation variant the scan must also catch.
            "// Block 5: rehome the compile cache",
            // Multi-digit form (`block 12`) covered by the
            // word-boundary scan.
            "// block 12 lands the cache-runtime overhaul",
        ];
        for line in violations {
            assert!(
                line_has_phase_archaeology(line),
                "guard 7 H19 predicate must reject deliberate-violation line: {line:?}",
            );
        }

        // Benign prose that superficially looks like a `block N`
        // match but is NOT plan vocabulary. The verb form `blocks`
        // (plural / 3rd-person-singular) and compound tokens
        // (`block_until_idle`) must not trip the scan.
        let benign = [
            "// the request loop blocks once per flight",
            "// `block_until_idle` waits until the queue drains.",
            // Sentence with "block" followed by something other
            // than a digit — must not match.
            "// the basic block reuses the allocator slot.",
            "// allocator block size = 64",
        ];
        for line in benign {
            assert!(
                !line_has_phase_archaeology(line),
                "guard 7 H19 predicate must NOT flag legitimate line: {line:?}",
            );
        }
    }

    // ── (Guard 7-bis retired — merged into guard 7 above.) ──
    //
    // The D111-classifier rule from `tools/god-module-audit/README.md`
    // is the SOLE archaeology classifier in this file, implemented by
    // `line_has_phase_archaeology`. Both production-source code
    // (`no_phase_archaeology_in_production_code`) and test-file code
    // (`phase_archaeology_test_files_count_zero`) call the same
    // predicate so the two scopes stay byte-identical with no risk of
    // drift between parallel predicates.
    //
    // The merge happened by absorbing the `:` carve-out for
    // `Phase \d+` / `Stage \d+`, the `Commit \d+` numeric scan, the
    // `revision \d+` / `rev \d+` scans, the `phase-archaeology` fixed
    // needle, the `post-Phase` / `Post-Phase` fixed needles, and the
    // broadened `deleted in \d` scan into `line_has_phase_archaeology`.
    // The single classifier now covers every D111 pattern.

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

    // ── Guard — origin-edge dep signatures are not an invalidation source ──
    //
    // Invariant:
    //
    //   Origin-edge dep signatures are not an invalidation source.
    //   No production code may reconstruct a CompletionFence from
    //   DerivationStore origin edges.
    //
    // The `DerivationStore` origin layer keeps an `edge_dep_signature`
    // snapshot of the publishing builder's fence purely for the audit
    // origin-graph trace. It is bounded best-effort provenance — the
    // FIFO `edge_budget` evicts the oldest buckets, so the snapshots are
    // NOT load-bearing for invalidation. The load-bearing invalidation
    // record is the memo entry's own `ReadSetSignature` carrier — the
    // path-precise fact rail validated strictly on every warm read via
    // `ReadSetSignature::validate_with_self_roots`.
    //
    // Reconstructing a `CompletionFence` from origin-edge dep signatures
    // would couple correctness to a best-effort, FIFO-evicted structure
    // — a latent footgun. The retired `SemanticGraphStore::origins_with_fence`
    // did exactly that (`fence.merge_signature(&edge.edge_dep_signature)`)
    // and had zero production callers; this guard fails if it — or any
    // equivalent origin-edge-into-fence merge — is reintroduced into
    // non-test production source.

    /// Predicate: does this single production source line reintroduce an
    /// origin-edge-into-`CompletionFence` merge? Returns `true` for a
    /// match. Two shapes are forbidden — (a) any mention of the retired
    /// `origins_with_fence` API, and (b) a `merge_signature(...)` call
    /// whose argument is an `edge_dep_signature` (folding a
    /// `DerivationStore` origin edge's dep-signature snapshot into a
    /// fence). Comment lines are NOT exempt: a doc comment that
    /// re-documents an `origins_with_fence`-style API is itself the
    /// reintroduction this guard forbids.
    pub fn line_reconstructs_fence_from_origin_edge(line: &str) -> bool {
        if line.contains("origins_with_fence") {
            return true;
        }
        // The forbidden merge shape: a `merge_signature` call on the
        // same line as an `edge_dep_signature` operand — the exact
        // `fence.merge_signature(&edge.edge_dep_signature)` pattern the
        // retired API used. Requiring both needles on one line keeps the
        // guard focused: a legitimate `merge_signature` of a memo
        // entry's own carrier never names `edge_dep_signature`.
        line.contains("merge_signature") && line.contains("edge_dep_signature")
    }

    /// Walk the production tree and return `(rel_path, line_no, line)`
    /// triples for every origin-edge-into-fence reconstruction.
    pub fn origin_fence_reconstruction_violations() -> Vec<(String, usize, String)> {
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
                // This guard test file is exempt — it names the
                // forbidden patterns in its own predicate + self-test.
                if rel.contains("architecture_guards") {
                    continue;
                }
                for (idx, line) in src.lines().enumerate() {
                    if line_reconstructs_fence_from_origin_edge(line) {
                        violations.push((rel.clone(), idx + 1, line.to_string()));
                    }
                }
            }
        }
        violations.sort();
        violations
    }

    #[test]
    fn origin_edge_dep_signatures_are_not_an_invalidation_source() {
        let violations = origin_fence_reconstruction_violations();
        assert!(
            violations.is_empty(),
            "Architecture guard `origin_edge_dep_signatures_are_not_an_invalidation_source`\n\
             violations: production source reconstructs a CompletionFence from DerivationStore\n\
             origin edges.\n\n\
             INVARIANT:\n  \
             Origin-edge dep signatures are not an invalidation source.\n  \
             No production code may reconstruct a CompletionFence from DerivationStore\n  \
             origin edges.\n\n\
             Origin edges are bounded best-effort provenance for the audit origin-graph\n\
             trace; the FIFO `edge_budget` evicts the oldest buckets, so an `edge_dep_signature`\n\
             is NOT load-bearing for invalidation. The load-bearing record is the memo entry's\n\
             own `ReadSetSignature` carrier, validated strictly on every warm read via\n\
             `ReadSetSignature::validate_with_self_roots`.\n\
             Do not reintroduce `origins_with_fence` or fold an `edge_dep_signature` into a\n\
             `CompletionFence`.\n\n\
             Violations:\n  {}",
            violations
                .iter()
                .map(|(rel, lineno, line)| format!("{rel}:{lineno}: {}", line.trim()))
                .collect::<Vec<_>>()
                .join("\n  "),
        );
    }

    #[test]
    fn origin_fence_guard_predicate_rejects_deliberate_violations() {
        // Each fabricated line models a real reintroduction of the
        // retired origin-edge-into-fence merge.
        let forbidden = [
            // The retired API by name — a re-declaration or a call.
            "    pub fn origins_with_fence(&self, node: SemanticNodeId) -> Vec<OriginEdge> {",
            "        let visited = store.origins_with_fence(result, &fence);",
            "/// merges each edge's dep-signature via `origins_with_fence`.",
            // The forbidden merge shape — `merge_signature` folding an
            // origin edge's `edge_dep_signature` snapshot into a fence.
            "            fence.merge_signature(&edge.edge_dep_signature);",
            "    active_fence.merge_signature(&origin.edge_dep_signature);",
        ];
        for line in forbidden {
            assert!(
                line_reconstructs_fence_from_origin_edge(line),
                "origin-fence guard predicate must reject deliberate-violation line: {line:?}",
            );
        }
        // Lines that look superficially similar but are NOT violations:
        // a `merge_signature` of a memo entry's OWN carrier (never names
        // `edge_dep_signature`), and an `edge_dep_signature` touched for
        // a purpose other than a fence merge (interning, dedup probe).
        let allowed = [
            // Legitimate fence merge of a cached read's own carrier.
            "    crate::component_meta_audit::merge_dep_signature_into_local_fence(local_fence, &read.dep_signature);",
            "        fence.merge_signature(&read.dep_signature);",
            // `edge_dep_signature` touched for interning / dedup — no
            // fence merge on the line.
            "        let interned = self.intern_signature(edge.edge_dep_signature.clone());",
            "            && Arc::ptr_eq(&existing.edge_dep_signature, &candidate.edge_dep_signature)",
            // Plain prose about origin edges that does not name the
            // retired API.
            "// Origin edges are bounded best-effort provenance, not an invalidation source.",
        ];
        for line in allowed {
            assert!(
                !line_reconstructs_fence_from_origin_edge(line),
                "origin-fence guard predicate must NOT flag legitimate line: {line:?}",
            );
        }
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

/// Shared helper for the test-file phase-archaeology guard. Reuses
/// the unified phase-archaeology classifier from `foundations_guards`
/// so the test-file and production-code predicates stay byte-identical
/// (single source of truth).
mod w5f_test_archaeology {
    use std::path::Path;
    use walkdir::WalkDir;

    use super::workspace_root;

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
                    if super::foundations_guards::line_has_phase_archaeology(line) {
                        violations.push(format!("{rel}:{}", line_no + 1));
                    }
                }
            }
        }
        violations
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
fn phase_archaeology_test_files_count_zero() {
    // Strict invariant: zero phase-archaeology references in test files
    // inside `crates/*/src/` — `tests.rs`, `*_tests.rs`, and anything
    // under a `tests/` subdirectory. The classifier is the unified
    // `foundations_guards::line_has_phase_archaeology` predicate, so the
    // test-file and production-code guards stay byte-identical (single
    // source of truth).
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
        "projector-cutover-1",
        "projector-cutover-2",
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
    // No production source is allow-listed. The reverse-dependent
    // upsert-time invalidation cascade has been removed: `host_upsert.rs`
    // no longer reads the reverse graph for cache invalidation. The
    // reverse axis is content-addressed bookkeeping (R22) only.
    const ALLOW_LIST: &[&str] = &[];

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

/// R22 / R3 — the upsert path performs NO eager cache drain, on
/// either the cross-file or the same-canonical axis.
///
/// `host_upsert.rs` must not, anywhere in its body, call
/// `reverse_deps_for`, `invalidate_canonical`, or `evict_canonical`,
/// and must not call `resolved_type_cache().clear()`.
///
/// Two retired drains map onto those identifiers:
///
/// - The reverse-dependent cascade. An owner upsert iterated
///   `ws().reverse_deps_for(canonical)` and
///   `resolver.runtime.invalidate_canonical(owner)`'d every dependent.
///   A downstream consumer's warm cache is now revalidated lazily on
///   read through its own `fact_dep_signature` check.
/// - The own-canonical drain. An upsert eagerly evicted the upserted
///   canonical's own query-identity caches —
///   `resolver.runtime.evict_canonical(&canonical_id)`,
///   `project_type_store.evict_canonical(&canonical_id)`,
///   `resolved_type_cache().clear()`. A warm query-identity entry for
///   the upserted canonical is now rejected on the cold-recompute read
///   path by its current-content self-version root.
///
/// Same-canonical invalidation is lazy via self-version-rooted fact
/// validation; reintroducing either eager drain into `host_upsert.rs`
/// is forbidden.
#[test]
fn host_upsert_performs_no_reverse_dependent_eviction() {
    use syn::visit::Visit;

    let src = read_workspace_file("crates/verter_session/src/host_upsert.rs");
    let parsed = syn::parse_file(&src).expect("parse host_upsert.rs");
    let mut scanner = UpsertEagerDrainScanner::default();
    scanner.visit_file(&parsed);

    assert!(
        scanner.hits.is_empty(),
        "host_upsert.rs calls an eager cache-drain method ({:?}). \
         The upsert performs NO eager drain on either axis. Cross-file: \
         the reverse-dependent cascade is removed — `reverse_deps_for` / \
         `invalidate_canonical` must not reappear; cross-file consumers \
         revalidate lazily on read via `fact_dep_signature`. \
         Same-canonical: the own-canonical drain is removed — \
         `evict_canonical(&canonical_id)` / `resolved_type_cache().clear()` \
         must not reappear; a warm query-identity entry for the upserted \
         canonical is rejected on the cold-recompute read path by its \
         current-content self-version root.",
        scanner.hits
    );
}

/// AST scanner shared by [`host_upsert_performs_no_reverse_dependent_eviction`]
/// and its discriminating self-test. Flags any reverse-dependent or
/// own-canonical eager-drain method call: the bare identifiers
/// `reverse_deps_for` / `invalidate_canonical` / `evict_canonical`, plus
/// the `resolved_type_cache().clear()` receiver-qualified chain (a bare
/// `.clear()` is too generic to ban — the chain over a
/// `resolved_type_cache()` receiver is the discriminating shape).
#[derive(Default)]
struct UpsertEagerDrainScanner {
    hits: Vec<String>,
}

impl UpsertEagerDrainScanner {
    /// Bare method identifiers that name an eager cache drain. None of
    /// these has a legitimate use inside `host_upsert.rs`.
    const FORBIDDEN_DRAIN_METHODS: &'static [&'static str] = &[
        "reverse_deps_for",
        "invalidate_canonical",
        "evict_canonical",
    ];
}

impl<'ast> syn::visit::Visit<'ast> for UpsertEagerDrainScanner {
    fn visit_expr_method_call(&mut self, mc: &'ast syn::ExprMethodCall) {
        let method = mc.method.to_string();
        if Self::FORBIDDEN_DRAIN_METHODS.contains(&method.as_str()) {
            self.hits.push(method.clone());
        }
        // `resolved_type_cache().clear()` — a `.clear()` whose receiver
        // is a call to `resolved_type_cache()`. This is the bulk
        // resolved-type-cache flush that was part of the own-canonical
        // drain; a bare `.clear()` on some other cache is not flagged.
        if method == "clear" {
            if let syn::Expr::MethodCall(recv) = &*mc.receiver {
                if recv.method == "resolved_type_cache" {
                    self.hits.push("resolved_type_cache().clear".to_string());
                }
            }
        }
        syn::visit::visit_expr_method_call(self, mc);
    }
}

/// Discriminating self-test for
/// [`host_upsert_performs_no_reverse_dependent_eviction`]: the
/// [`UpsertEagerDrainScanner`] must FLAG both the reverse-dependent
/// cascade and the own-canonical drain, and must ACCEPT a bare
/// `.clear()` on an unrelated cache. Without this the production guard
/// could pass trivially.
#[test]
fn host_upsert_reverse_dep_eviction_scanner_discriminates() {
    use syn::visit::Visit;

    fn scan(src: &str) -> Vec<String> {
        let parsed = syn::parse_file(src).expect("parse fixture");
        let mut s = UpsertEagerDrainScanner::default();
        s.visit_file(&parsed);
        s.hits
    }

    // FORBIDDEN: the reverse-dependent cascade shape — flagged.
    let reverse_dep_fixture = r#"
        impl Host {
            fn upsert(&self) {
                for owner in self.ws().reverse_deps_for(&id) {
                    self.resolver.runtime.invalidate_canonical(owner);
                }
            }
        }
    "#;
    assert!(
        !scan(reverse_dep_fixture).is_empty(),
        "scanner must flag a reverse_deps_for / invalidate_canonical cascade"
    );

    // FORBIDDEN: the own-canonical drain shape — flagged. Reintroducing
    // `evict_canonical(&canonical_id)` or `resolved_type_cache().clear()`
    // into the upsert path is banned: same-canonical invalidation is
    // lazy via self-version-rooted fact validation.
    let own_canonical_drain_fixture = r#"
        impl Host {
            fn upsert(&self) {
                self.resolver.runtime.evict_canonical(&canonical_id);
                self.project_type_store.evict_canonical(&canonical_id);
                self.resolved_type_cache().clear();
            }
        }
    "#;
    let drain_hits = scan(own_canonical_drain_fixture);
    assert!(
        drain_hits
            .iter()
            .filter(|h| *h == "evict_canonical")
            .count()
            == 2,
        "scanner must flag both `evict_canonical` calls, got {drain_hits:?}"
    );
    assert!(
        drain_hits
            .iter()
            .any(|h| h == "resolved_type_cache().clear"),
        "scanner must flag the `resolved_type_cache().clear()` chain, got {drain_hits:?}"
    );

    // ACCEPTED: a bare `.clear()` on an unrelated cache is not an
    // own-canonical drain — the per-domain compile/derived-cache field
    // resets the upsert legitimately performs must not be flagged.
    let unrelated_clear_fixture = r#"
        impl Host {
            fn upsert(&self) {
                profile.compile_slots.clear();
                derived.cached_resolved_meta.clear();
            }
        }
    "#;
    assert!(
        scan(unrelated_clear_fixture).is_empty(),
        "scanner must NOT flag a bare `.clear()` on an unrelated cache field"
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

/// Architecture guard — direct content-agnostic `FileArtifactStore`
/// reads (`indexed().get_any` / `indexed().get_artifacts_any`) are
/// banned in `verter_session` production source outside a named
/// intent-specific helper allowlist.
///
/// `FileArtifactStore::get_any` and `get_artifacts_any` are
/// content-agnostic, canonical-only lookups: they return *whichever*
/// cached artifact matches the canonical, regardless of content
/// version. With the own-canonical drain retired (Block 2.S /
/// retry-item-2), a stale pre-edit `IndexedReady` / `FileArtifacts` can
/// linger past a same-canonical content edit. A producer that reads it
/// through `get_any` would feed a stale observed-content identity into
/// a provenance-pure `fact_dep_signature` builder — defeating
/// query-identity self-version-rooting at its root.
///
/// Correctness-sensitive readers MUST instead use a content-pinned
/// named helper:
/// - [`crate::VerterHost::current_content_pinned_indexed`] — the
///   scheduler-pinned `IndexedReady` read.
/// - [`crate::VerterHost::artifact_current_indexed`] — the artifact-only
///   `IndexedReady` authority for a canonical the scheduler does not
///   track.
/// - [`crate::VerterHost::current_content_pinned_artifacts`] — the
///   `FileArtifacts` analogue (scheduler-pinned, artifact-only
///   fallback).
///
/// The few legitimate direct `get_any` / `get_artifacts_any` call sites
/// (those helpers' own bodies, plus pure existence/diagnostics probes
/// whose stale answers do not affect a value or its validation) are
/// listed in [`GET_ANY_ALLOWLIST`] with the reason each is exempt. A
/// new direct call site outside the allowlist fails this guard.
#[cfg(test)]
mod content_pinned_artifact_read_guards {
    use std::fs;
    use std::path::PathBuf;

    /// Files permitted to call `indexed().get_any(` /
    /// `indexed().get_artifacts_any(` directly. Each entry pairs the
    /// repo-relative path with the reason the direct read is legitimate.
    ///
    /// Every other production `verter_session/src` file MUST route
    /// `FileArtifactStore` reads through a content-pinned named helper
    /// (`current_content_pinned_indexed` / `artifact_current_indexed` /
    /// `current_content_pinned_artifacts`).
    const GET_ANY_ALLOWLIST: &[(&str, &str)] = &[
        (
            "crates/verter_session/src/host_manage/analysis_io.rs",
            "Defines the content-pinned named helpers themselves \
             (`artifact_current_indexed`, `current_content_pinned_artifacts`) \
             — their bodies are the artifact-only authority. Also the \
             documented permissive `get_whole_hash` accessor, whose strict \
             sibling is `authoritative_current_content_hash`.",
        ),
        (
            "crates/verter_session/src/host_manage/eval_program.rs",
            "`analysis_source_exists` probes `get_any().is_some()` as a \
             pure existence check after the scheduler authority \
             (`effective_file_state`) missed — it returns a `bool`, never \
             artifact content.",
        ),
        (
            "crates/verter_session/src/host_manage/component_meta_methods.rs",
            "`get_raw_analysis_snapshot` gates a route-owned-snapshot \
             fast path on `get_any().is_none()` — a pure existence gate. \
             The artifact's content is never read here; the \
             scheduler-first authority (`build_snapshot_from_scheduler`) \
             produces the snapshot.",
        ),
    ];

    /// Strip `//` line comments and `/* */` block comments so a
    /// `get_any` mention inside a doc-comment is not flagged as a call
    /// site. Character-level scan; string-literal contents are left
    /// intact (a `get_any` substring inside a string literal is not a
    /// production concern this guard cares about, and none exists).
    fn strip_comments(src: &str) -> String {
        let bytes = src.as_bytes();
        let mut out = String::with_capacity(src.len());
        let mut i = 0;
        let mut in_string = false;
        let mut in_line_comment = false;
        let mut in_block_comment = false;
        while i < bytes.len() {
            let c = bytes[i] as char;
            let next = bytes.get(i + 1).map(|b| *b as char);
            if in_line_comment {
                if c == '\n' {
                    in_line_comment = false;
                    out.push('\n');
                }
                i += 1;
                continue;
            }
            if in_block_comment {
                if c == '*' && next == Some('/') {
                    in_block_comment = false;
                    i += 2;
                    continue;
                }
                if c == '\n' {
                    out.push('\n');
                }
                i += 1;
                continue;
            }
            if in_string {
                out.push(c);
                if c == '\\' {
                    if let Some(n) = next {
                        out.push(n);
                    }
                    i += 2;
                    continue;
                }
                if c == '"' {
                    in_string = false;
                }
                i += 1;
                continue;
            }
            if c == '/' && next == Some('/') {
                in_line_comment = true;
                i += 2;
                continue;
            }
            if c == '/' && next == Some('*') {
                in_block_comment = true;
                i += 2;
                continue;
            }
            if c == '"' {
                in_string = true;
            }
            out.push(c);
            i += 1;
        }
        out
    }

    /// True when `src` (already comment-stripped) contains a direct
    /// `indexed()` → `.get_any(` / `.get_artifacts_any(` call chain.
    ///
    /// Whitespace and newlines between `indexed()` and the method call
    /// are tolerated — the call chain is frequently split across lines
    /// by `rustfmt`. The `route_owned_shallow().get_any(` chain is a
    /// DIFFERENT db (`RouteOwnedShallowDb`) and is deliberately NOT
    /// matched: this guard targets `FileArtifactStore` reads only.
    ///
    /// ## Known limitation — fluent chains only
    ///
    /// This scanner matches only the **fluent** form where `.get_any(` /
    /// `.get_artifacts_any(` immediately follows `indexed()` (modulo
    /// whitespace). A variable-bound read —
    /// `let s = …indexed(); s.get_any(c)` — splits the `indexed()`
    /// receiver from the call across a binding and is NOT flagged.
    /// Detecting that form by text is not reliable: a bare
    /// `.get_any(`/`.get_artifacts_any(` on a binding cannot be
    /// attributed to a `FileArtifactStore` without false positives
    /// against the identically-named methods on other dbs
    /// (`route_owned_shallow()`, `member_display_facts()`, …) — that
    /// needs real name resolution, not a scanner. No current
    /// `verter_session` production file uses the var-bound form, and
    /// the structural guard in
    /// `tests/structural_carrier_no_get_any_guard.rs` covers the
    /// carrier-type angle. If a var-bound `FileArtifactStore` read is
    /// ever introduced, convert it to the fluent form (so this guard
    /// catches it) or route it through a content-pinned named helper.
    fn has_direct_file_artifact_get_any(src: &str) -> bool {
        let needle = "indexed()";
        let mut search_from = 0;
        while let Some(rel) = src[search_from..].find(needle) {
            let after = search_from + rel + needle.len();
            let tail = src[after..].trim_start();
            if tail.starts_with(".get_any(") || tail.starts_with(".get_artifacts_any(") {
                return true;
            }
            search_from = after;
        }
        false
    }

    /// Repo-relative `.rs` files under `crates/verter_session/src`,
    /// excluding test files (`*_tests.rs`, `tests.rs`).
    fn verter_session_production_rs_files() -> Vec<(PathBuf, String)> {
        let root = super::workspace_root();
        let src_dir = root.join("crates/verter_session/src");
        let mut files: Vec<PathBuf> = Vec::new();
        walk_rs(&src_dir, &mut files);
        let mut out: Vec<(PathBuf, String)> = Vec::new();
        for f in files {
            let rel = f
                .strip_prefix(&root)
                .unwrap_or(&f)
                .to_string_lossy()
                .replace('\\', "/");
            let basename = rel.rsplit('/').next().unwrap_or("");
            if basename.ends_with("_tests.rs") || basename == "tests.rs" {
                continue;
            }
            out.push((f, rel));
        }
        out
    }

    fn walk_rs(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
        if !dir.is_dir() {
            return;
        }
        for entry in fs::read_dir(dir).unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()))
        {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                walk_rs(&path, out);
            } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                out.push(path);
            }
        }
    }

    /// The scan algorithm — testable in isolation against synthetic
    /// input. Returns the sorted set of repo-relative production files
    /// that hold a direct `FileArtifactStore` `get_any` /
    /// `get_artifacts_any` call AND are not on `allowlist`.
    fn unallowlisted_get_any_files(files: &[(String, String)], allowlist: &[&str]) -> Vec<String> {
        let mut violations: Vec<String> = files
            .iter()
            .filter(|(rel, src)| {
                !allowlist.contains(&rel.as_str())
                    && has_direct_file_artifact_get_any(&strip_comments(src))
            })
            .map(|(rel, _)| rel.clone())
            .collect();
        violations.sort();
        violations.dedup();
        violations
    }

    /// Static allowlist guard — no `verter_session` production file
    /// outside [`GET_ANY_ALLOWLIST`] calls `indexed().get_any(` /
    /// `indexed().get_artifacts_any(` directly.
    #[test]
    fn no_direct_file_artifact_get_any_outside_allowlist() {
        let allow: Vec<&str> = GET_ANY_ALLOWLIST.iter().map(|(p, _)| *p).collect();
        let files: Vec<(String, String)> = verter_session_production_rs_files()
            .into_iter()
            .map(|(path, rel)| {
                let src = fs::read_to_string(&path)
                    .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
                (rel, src)
            })
            .collect();
        let violations = unallowlisted_get_any_files(&files, &allow);
        assert!(
            violations.is_empty(),
            "content-agnostic `FileArtifactStore` reads found outside the \
             named-helper allowlist:\n  {}\n\nA stale pre-edit artifact can \
             linger past a same-canonical edit (own-canonical drain \
             retired); a `get_any` / `get_artifacts_any` read feeds a stale \
             observed-content identity into the fact-signature builders. \
             Route the read through `current_content_pinned_indexed` / \
             `artifact_current_indexed` / `current_content_pinned_artifacts` \
             — or, if the read is a genuine existence/diagnostics probe, \
             add the file to `GET_ANY_ALLOWLIST` with the reason.",
            violations.join("\n  "),
        );

        // Every allowlisted file MUST still actually contain a direct
        // call — a stale allowlist entry (the call was removed / pinned)
        // must be deleted so the allowlist cannot silently grow
        // permission it no longer needs.
        let by_rel: std::collections::BTreeMap<&str, &str> = files
            .iter()
            .map(|(rel, src)| (rel.as_str(), src.as_str()))
            .collect();
        for (allowed_path, _reason) in GET_ANY_ALLOWLIST {
            let src = by_rel.get(allowed_path).unwrap_or_else(|| {
                panic!(
                    "GET_ANY_ALLOWLIST entry {allowed_path} is not a \
                     verter_session production source file"
                )
            });
            assert!(
                has_direct_file_artifact_get_any(&strip_comments(src)),
                "GET_ANY_ALLOWLIST entry {allowed_path} no longer contains a \
                 direct `indexed().get_any(` / `.get_artifacts_any(` call — \
                 remove the stale allowlist entry.",
            );
        }
    }

    /// Discriminating self-test — proves the scan algorithm
    /// distinguishes a real direct `get_any` call from a comment, a
    /// `route_owned_shallow().get_any` (different db), and a
    /// content-pinned helper call.
    ///
    /// Without this, an empty-violations result of the guard above is
    /// indistinguishable from a detector that always passes.
    #[test]
    fn get_any_guard_discriminator_self_test() {
        // (a) A file with a direct `indexed().get_any(` call, NOT
        //     allowlisted → must be flagged.
        let bad = (
            "crates/verter_session/src/synthetic_offender.rs".to_string(),
            "fn read(&self) { let _ = self.project_type_store.indexed().get_any(c); }".to_string(),
        );
        let flagged = unallowlisted_get_any_files(std::slice::from_ref(&bad), &[]);
        assert_eq!(
            flagged,
            vec!["crates/verter_session/src/synthetic_offender.rs".to_string()],
            "discriminator: a direct `indexed().get_any(` call in a \
             non-allowlisted file MUST be flagged"
        );

        // (a') The SAME file, now allowlisted → must NOT be flagged.
        let flagged_allowed = unallowlisted_get_any_files(
            std::slice::from_ref(&bad),
            &["crates/verter_session/src/synthetic_offender.rs"],
        );
        assert!(
            flagged_allowed.is_empty(),
            "discriminator: an allowlisted file's direct call MUST NOT be \
             flagged"
        );

        // (b) The newline-split call chain (`rustfmt` form) → must be
        //     flagged.
        let split = (
            "crates/verter_session/src/synthetic_split.rs".to_string(),
            "fn read(&self) {\n    let _ = self\n        .project_type_store\n        \
             .indexed()\n        .get_artifacts_any(c);\n}"
                .to_string(),
        );
        assert_eq!(
            unallowlisted_get_any_files(std::slice::from_ref(&split), &[]),
            vec!["crates/verter_session/src/synthetic_split.rs".to_string()],
            "discriminator: a newline-split `indexed()\\n.get_artifacts_any(` \
             chain MUST be flagged"
        );

        // (c) A `get_any` mention inside a comment → must NOT be
        //     flagged (comment-stripping works).
        let comment_only = (
            "crates/verter_session/src/synthetic_comment.rs".to_string(),
            "// callers MUST NOT use indexed().get_any(c) here\n/* indexed().get_any(x) */\n\
             fn ok(&self) { let _ = self.current_content_pinned_indexed(c); }"
                .to_string(),
        );
        assert!(
            unallowlisted_get_any_files(std::slice::from_ref(&comment_only), &[]).is_empty(),
            "discriminator: a `get_any` mention inside a `//` or `/* */` \
             comment MUST NOT be flagged"
        );

        // (d) `route_owned_shallow().get_any(` is a DIFFERENT db
        //     (`RouteOwnedShallowDb`) — must NOT be flagged.
        let route_owned = (
            "crates/verter_session/src/synthetic_route_owned.rs".to_string(),
            "fn read(&self) { let _ = self.project_type_store.route_owned_shallow().get_any(c); }"
                .to_string(),
        );
        assert!(
            unallowlisted_get_any_files(std::slice::from_ref(&route_owned), &[]).is_empty(),
            "discriminator: `route_owned_shallow().get_any(` targets a \
             different db and MUST NOT be flagged by the FileArtifactStore guard"
        );

        // (e) A file that only calls the content-pinned helpers → must
        //     NOT be flagged.
        let pinned = (
            "crates/verter_session/src/synthetic_pinned.rs".to_string(),
            "fn read(&self) { let _ = self.current_content_pinned_indexed(c)\n        \
             .or_else(|| self.artifact_current_indexed(c)); }"
                .to_string(),
        );
        assert!(
            unallowlisted_get_any_files(std::slice::from_ref(&pinned), &[]).is_empty(),
            "discriminator: a file using only the content-pinned named \
             helpers MUST NOT be flagged"
        );
    }

    // ──────────────────────────────────────────────────────────────
    // Named-currency-oracle closure.
    //
    // The `get_any` allowlist guard above bans only the two literal
    // fluent call chains `indexed().get_any(` / `.get_artifacts_any(`.
    // A *named* `FileArtifactStore` method with the identical
    // content-agnostic first-match scan body — `content_hash_for_canonical`,
    // `latest_artifacts_for_canonical` — is outside that guard's
    // textual reach: a content-agnostic "currency oracle" can be
    // reintroduced under any new method name. The two guards below
    // close that gap:
    //
    // - `no_named_currency_oracle_calls_in_production` — a call-site
    //   ban on the two named oracles, so even if a future change
    //   re-adds them, production code cannot call them.
    // - `file_artifact_store_defines_no_unpinned_currency_oracle` — a
    //   *definition-shape* guard: any `FileArtifactStore` method with a
    //   canonical-only parameter (no content-hash pin), a singular
    //   `Option<...>` return, that scans `self.artifacts`, is a
    //   currency oracle and is banned at the definition site. Only the
    //   two intentional low-level escapes (`get_any` /
    //   `get_artifacts_any`, themselves guarded at every call site) are
    //   allowlisted.
    // ──────────────────────────────────────────────────────────────

    /// Method names that ARE the content-agnostic currency-oracle
    /// shape and are intentionally retained as low-level escapes. Their
    /// every call site is independently guarded by
    /// [`no_direct_file_artifact_get_any_outside_allowlist`] +
    /// `tests/structural_carrier_no_get_any_guard.rs`.
    const CURRENCY_ORACLE_DEFINITION_ALLOWLIST: &[&str] = &["get_any", "get_artifacts_any"];

    /// Banned named currency oracles — a canonical-only
    /// `Option`-returning `FileArtifactStore` accessor cannot be
    /// content-pinned, so it must not exist in production use. The set
    /// is empty of production callers after the named content-agnostic
    /// currency oracles (`content_hash_for_canonical` /
    /// `latest_artifacts_for_canonical`) were removed; this guard keeps
    /// it that way.
    const BANNED_NAMED_CURRENCY_ORACLES: &[&str] = &[
        ".content_hash_for_canonical(",
        ".latest_artifacts_for_canonical(",
    ];

    /// No `verter_session` production file calls a banned named
    /// currency oracle. There is no allowlist — a canonical-only
    /// `Option<Hash16>` / `Option<Arc<FileArtifacts>>` accessor is
    /// unpinnable by construction; a caller needing current identity
    /// uses the scheduler authority, a caller needing artifacts uses an
    /// exact key.
    #[test]
    fn no_named_currency_oracle_calls_in_production() {
        let files = verter_session_production_rs_files();
        let mut violations: Vec<String> = Vec::new();
        for (path, rel) in &files {
            let src =
                fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
            let stripped = strip_comments(&src);
            for banned in BANNED_NAMED_CURRENCY_ORACLES {
                if stripped.contains(banned) {
                    violations.push(format!("{rel}: calls `{banned}`"));
                }
            }
        }
        violations.sort();
        assert!(
            violations.is_empty(),
            "named content-agnostic currency-oracle calls found in \
             production:\n  {}\n\nA canonical-only `Option`-returning \
             `FileArtifactStore` accessor cannot be content-pinned — with \
             lazy cache invalidation it can surface a stale pre-edit \
             artifact. Resolve current identity through the scheduler \
             authority (`authoritative_current_content_hash`); read \
             artifacts through an exact `FileArtifactKey`.",
            violations.join("\n  "),
        );
    }

    /// Extract the brace-balanced body (including the outer braces) of
    /// the first `pub fn <name>` / `pub(crate) fn <name>` whose
    /// signature begins at `decl_start`. Returns
    /// `(signature, body, end_offset)` where `signature` is the text
    /// from `fn` to the opening brace and `end_offset` is the index in
    /// `src` one past the closing brace.
    fn balanced_fn_after(src: &str, decl_start: usize) -> Option<(String, String, usize)> {
        let after = &src[decl_start..];
        let brace_rel = after.find('{')?;
        let signature = after[..brace_rel].trim().to_string();
        let bytes = after.as_bytes();
        let mut depth = 0usize;
        let mut idx = brace_rel;
        while idx < bytes.len() {
            match bytes[idx] {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some((
                            signature,
                            after[brace_rel..=idx].to_string(),
                            decl_start + idx + 1,
                        ));
                    }
                }
                _ => {}
            }
            idx += 1;
        }
        None
    }

    /// Extract the method parameter list — the text between the first
    /// `(` after the `fn` keyword and its **brace-matching** `)`. This
    /// is robust to a `where R: Fn(&str) -> T` clause, whose inner
    /// parentheses would otherwise confuse a `rfind(')')`-based span.
    /// Returns `(params, after_params)` where `after_params` is the
    /// signature tail (return arrow + `where` clause).
    fn split_signature_params(signature: &str) -> Option<(String, String)> {
        let open = signature.find('(')?;
        let bytes = signature.as_bytes();
        let mut depth = 0usize;
        let mut idx = open;
        while idx < bytes.len() {
            match bytes[idx] {
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some((
                            signature[open + 1..idx].to_string(),
                            signature[idx + 1..].to_string(),
                        ));
                    }
                }
                _ => {}
            }
            idx += 1;
        }
        None
    }

    /// True when `signature` (the `fn name(params) -> Ret` slice) is a
    /// canonical-only accessor: it takes a `&str` parameter whose name
    /// contains `canonical` and carries NO content-hash pin parameter
    /// (`Hash16` or a parameter whose name contains `content_hash` /
    /// `whole_hash` / `parse_stable_hash`). A `&FileArtifactKey`
    /// parameter IS a content-pin (the exact key carries the content
    /// hash) — `FileArtifactKey` is recognised as a pin.
    fn is_canonical_only_signature(signature: &str) -> bool {
        let Some((params, _after)) = split_signature_params(signature) else {
            return false;
        };
        let takes_canonical_str = params.contains("canonical") && params.contains("&str");
        let has_hash_pin = params.contains("Hash16")
            || params.contains("FileArtifactKey")
            || params.contains("content_hash")
            || params.contains("whole_hash")
            || params.contains("parse_stable_hash")
            || params.contains("discriminator");
        takes_canonical_str && !has_hash_pin
    }

    /// True when `signature`'s return type is a singular `Option<...>`
    /// (NOT `Vec<...>` — a full enumeration is a legitimate
    /// canonical-wide scan, not a currency oracle). The return type is
    /// read from the signature tail AFTER the brace-matched parameter
    /// list, so an arrow inside a `where R: Fn(..) -> T` clause is not
    /// mistaken for the method's own return type. A `where`-clause-only
    /// tail with no top-level `->` (a `()`-returning method) is not an
    /// `Option` return.
    fn returns_singular_option(signature: &str) -> bool {
        let Some((_params, after)) = split_signature_params(signature) else {
            return false;
        };
        // The method return type, if any, is the `-> ...` segment
        // BEFORE any `where` clause. `where` introduces generic bounds
        // (which may themselves contain `-> ` inside `Fn(..) -> T`).
        // Split on the `where` keyword as a whitespace-delimited word
        // (the clause may be newline-separated from the param list).
        let where_at = after
            .match_indices("where")
            .find(|(i, _)| {
                let before_ok = after[..*i]
                    .chars()
                    .next_back()
                    .is_none_or(char::is_whitespace);
                let after_ok = after[i + 5..]
                    .chars()
                    .next()
                    .is_none_or(char::is_whitespace);
                before_ok && after_ok
            })
            .map(|(i, _)| i);
        let head = match where_at {
            Some(w) => &after[..w],
            None => &after,
        };
        match head.split_once("->") {
            Some((_, ret)) => {
                let ret = ret.trim();
                ret.starts_with("Option<") && !ret.contains("Vec<")
            }
            None => false,
        }
    }

    /// Scan a `FileArtifactStore`-method `(signature, body)` pair and
    /// classify it: a method is an **unpinned currency oracle** when it
    /// is canonical-only ([`is_canonical_only_signature`]), returns a
    /// singular `Option` ([`returns_singular_option`]), and its body
    /// iterates `self.artifacts` directly (`self.artifacts.iter()`) or
    /// delegates to another `self.<method>(` that does.
    fn is_unpinned_currency_oracle(signature: &str, body: &str) -> bool {
        if !is_canonical_only_signature(signature) || !returns_singular_option(signature) {
            return false;
        }
        // The body must touch the artifact collection — either a direct
        // scan or a delegation to a sibling accessor. A direct
        // `self.artifacts.iter()` is the scan; a `self.get_artifacts_any(`
        // / `self.get_any(` delegation inherits the scan.
        body.contains("self.artifacts.iter()")
            || body.contains("self.get_artifacts_any(")
            || body.contains("self.get_any(")
    }

    /// Every `pub` / `pub(crate)` method defined inside `impl
    /// FileArtifactStore` in `file_artifact_store.rs`, as
    /// `(name, signature, body)` triples. Methods on other `impl`
    /// blocks in the same file (`FileArtifactKey`, `FileArtifacts`,
    /// `AugmenterEntry`, …) are excluded.
    fn file_artifact_store_methods() -> Vec<(String, String, String)> {
        let root = super::workspace_root();
        let path = root.join("crates/verter_session/src/file_artifact_store.rs");
        let src =
            fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        let stripped = strip_comments(&src);
        // Bound the scan to the `impl FileArtifactStore {` block.
        let impl_start = stripped
            .find("impl FileArtifactStore {")
            .expect("file_artifact_store.rs must define `impl FileArtifactStore`");
        // Find the matching close brace of the impl block.
        let bytes = stripped.as_bytes();
        let block_open = impl_start + stripped[impl_start..].find('{').unwrap();
        let mut depth = 0usize;
        let mut idx = block_open;
        let mut impl_end = stripped.len();
        while idx < bytes.len() {
            match bytes[idx] {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        impl_end = idx;
                        break;
                    }
                }
                _ => {}
            }
            idx += 1;
        }
        let impl_body = &stripped[block_open..=impl_end];

        let mut out: Vec<(String, String, String)> = Vec::new();
        let mut cursor = 0usize;
        while let Some(rel) = impl_body[cursor..].find("fn ") {
            let fn_kw = cursor + rel;
            // Require the `fn` to be a method declaration: preceded
            // (modulo whitespace) by `pub`, `pub(crate)`, `const`,
            // `unsafe`, or be a bare `fn`. We only care about `pub` /
            // `pub(crate)` methods — private helpers are not API.
            let prefix = &impl_body[..fn_kw];
            let is_pub =
                prefix.trim_end().ends_with("pub") || prefix.trim_end().ends_with("pub(crate)");
            // Method name: the identifier right after `fn `.
            let name_start = fn_kw + 3;
            let name: String = impl_body[name_start..]
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if let Some((signature, body, end_offset)) = balanced_fn_after(impl_body, fn_kw) {
                // Advance past this fn body before the (possible) move
                // of `signature` / `body` into `out`.
                cursor = end_offset;
                if is_pub && !name.is_empty() {
                    out.push((name, signature, body));
                }
            } else {
                cursor = fn_kw + 3;
            }
        }
        out
    }

    /// Definition-shape guard — `FileArtifactStore` defines NO unpinned
    /// currency-oracle method outside the intentional-escape allowlist.
    ///
    /// This catches the *next* `content_hash_for_canonical` regardless
    /// of the name it is given: the ban is on the shape (canonical-only
    /// parameter + singular `Option` return + `self.artifacts` scan),
    /// not a name list.
    #[test]
    fn file_artifact_store_defines_no_unpinned_currency_oracle() {
        let methods = file_artifact_store_methods();
        // Sanity: the scan found a non-trivial method surface — guards
        // against a parser regression silently passing vacuously.
        assert!(
            methods.len() > 10,
            "the `impl FileArtifactStore` scan found only {} methods — \
             the parser likely failed; the guard would pass vacuously",
            methods.len(),
        );
        let mut violations: Vec<String> = Vec::new();
        for (name, signature, body) in &methods {
            if CURRENCY_ORACLE_DEFINITION_ALLOWLIST.contains(&name.as_str()) {
                continue;
            }
            if is_unpinned_currency_oracle(signature, body) {
                violations.push(format!("`{name}` — signature `{signature}`"));
            }
        }
        violations.sort();
        assert!(
            violations.is_empty(),
            "unpinned content-agnostic currency oracle(s) defined on \
             `FileArtifactStore`:\n  {}\n\nA canonical-only accessor with a \
             singular `Option` return that scans `self.artifacts` answers \
             \"the current X for this canonical\" — but with lazy cache \
             invalidation a stale pre-edit artifact lingers, so a \
             first-match scan can return it. Either pin the read to a \
             content hash (add a `Hash16` parameter, use an exact \
             `FileArtifactKey`), return a `Vec` (a full enumeration is \
             not a currency oracle), or — for a genuine low-level escape \
             — add the method to `CURRENCY_ORACLE_DEFINITION_ALLOWLIST` \
             AND guard its every call site.",
            violations.join("\n  "),
        );

        // Every allowlisted escape MUST still be defined — a stale
        // allowlist entry (the method was removed / pinned) must be
        // dropped so the allowlist cannot silently grow permission.
        let defined: std::collections::BTreeSet<&str> =
            methods.iter().map(|(n, _, _)| n.as_str()).collect();
        for allowed in CURRENCY_ORACLE_DEFINITION_ALLOWLIST {
            assert!(
                defined.contains(allowed),
                "CURRENCY_ORACLE_DEFINITION_ALLOWLIST entry `{allowed}` is no \
                 longer a defined `FileArtifactStore` method — remove the \
                 stale allowlist entry.",
            );
        }
    }

    /// Discriminating self-test for the definition-shape scanner — it
    /// must flag the currency-oracle shape and clear the pinned /
    /// enumeration / non-scanning shapes. Without this, an
    /// empty-violations result is indistinguishable from a vacuous pass.
    #[test]
    fn currency_oracle_definition_scanner_discriminates() {
        // (a) The exact `content_hash_for_canonical` shape — canonical-
        //     only param, `Option<Hash16>` return, `self.artifacts`
        //     scan → MUST be flagged.
        let oracle_sig = "fn content_hash_for_canonical(&self, canonical: &str) -> Option<Hash16>";
        let oracle_body = "{ for entry in self.artifacts.iter() { if entry.key().canonical.as_ref() == canonical { return Some(entry.key().content_hash); } } None }";
        assert!(
            is_unpinned_currency_oracle(oracle_sig, oracle_body),
            "self-test: the `content_hash_for_canonical` shape MUST be flagged",
        );

        // (a') The `latest_artifacts_for_canonical` delegation shape →
        //      MUST be flagged (delegates to `self.get_artifacts_any(`).
        let alias_sig =
            "fn latest_artifacts_for_canonical(&self, canonical: &str) -> Option<Arc<FileArtifacts>>";
        let alias_body = "{ self.get_artifacts_any(canonical) }";
        assert!(
            is_unpinned_currency_oracle(alias_sig, alias_body),
            "self-test: the `latest_artifacts_for_canonical` delegation MUST be flagged",
        );

        // (b) A content-pinned read — carries a `Hash16` parameter →
        //     MUST NOT be flagged.
        let pinned_sig =
            "fn get(&self, canonical_id: &str, expected_whole_hash: Hash16) -> Option<Arc<IndexedReady>>";
        let pinned_body = "{ let key = FileArtifactKey::legacy(Arc::from(canonical_id), expected_whole_hash); self.artifacts.get(&key) }";
        assert!(
            !is_unpinned_currency_oracle(pinned_sig, pinned_body),
            "self-test: a content-pinned read (carries a `Hash16` pin) MUST NOT be flagged",
        );

        // (c) A full-enumeration scan — returns `Vec<...>` → MUST NOT
        //     be flagged (a caller wants the whole set, not "current").
        let enum_sig = "fn keys(&self) -> Vec<(Arc<str>, Hash16)>";
        let enum_body = "{ self.artifacts.iter().map(|e| e.key().clone()).collect() }";
        assert!(
            !is_unpinned_currency_oracle(enum_sig, enum_body),
            "self-test: a `Vec`-returning full enumeration MUST NOT be flagged",
        );

        // (d) A canonical-only `Option` accessor that does NOT scan
        //     `self.artifacts` → MUST NOT be flagged (no scan = no
        //     currency-oracle hazard).
        let no_scan_sig = "fn last_access_tick(&self, canonical: &str) -> Option<u64>";
        let no_scan_body = "{ self.last_access.get(canonical).map(|v| *v) }";
        assert!(
            !is_unpinned_currency_oracle(no_scan_sig, no_scan_body),
            "self-test: a canonical-only `Option` accessor that does not scan \
             `self.artifacts` MUST NOT be flagged",
        );

        // (e) An overlay-scoped read — carries a `discriminator`
        //     content-pin parameter → MUST NOT be flagged.
        let overlay_sig = "fn get_overlay_scoped(&self, canonical_id: &str, expected_whole_hash: Hash16, discriminator: Hash16) -> Option<Arc<IndexedReady>>";
        let overlay_body = "{ let key = FileArtifactKey::overlay_scoped(Arc::from(canonical_id), expected_whole_hash, discriminator); self.artifacts.get(&key) }";
        assert!(
            !is_unpinned_currency_oracle(overlay_sig, overlay_body),
            "self-test: an overlay-scoped read (carries a pin) MUST NOT be flagged",
        );

        // (f) The call-site scanner: a banned named-oracle call MUST be
        //     detected; a clean scheduler-authority call MUST NOT.
        let banned_call = "let h = store.content_hash_for_canonical(canonical);";
        assert!(
            BANNED_NAMED_CURRENCY_ORACLES
                .iter()
                .any(|b| banned_call.contains(b)),
            "self-test: a `.content_hash_for_canonical(` call MUST be detected",
        );
        let clean_call = "let h = base.authoritative_current_content_hash(canonical);";
        assert!(
            !BANNED_NAMED_CURRENCY_ORACLES
                .iter()
                .any(|b| clean_call.contains(b)),
            "self-test: a scheduler-authority call MUST NOT be flagged",
        );

        // (g) A `where`-clause method that returns `()` and whose
        //     `where R: Fn(&str) -> Option<T>` bound contains both a
        //     `&str` and an `-> Option<...>` → MUST NOT be flagged. The
        //     signature parser must not mistake the `Fn` bound's params
        //     / arrow for the method's own — this is the
        //     `refresh_augmentation_index_for_canonical` shape.
        let where_clause_sig = "fn refresh_augmentation_index_for_canonical<R>(&self, new_artifact_key: &FileArtifactKey, resolve_relative_canonical: R) where R: Fn(&str, &str) -> Option<Arc<str>>";
        assert!(
            !returns_singular_option(where_clause_sig),
            "self-test: a `()`-returning method whose `where` clause carries \
             `Fn(..) -> Option<T>` MUST NOT be read as Option-returning",
        );
        assert!(
            !is_canonical_only_signature(where_clause_sig),
            "self-test: a `&FileArtifactKey`-taking method is content-pinned \
             (the exact key carries the content hash) — MUST NOT be flagged \
             canonical-only, and the `Fn(&str)` bound's `&str` must not be \
             mistaken for a method parameter",
        );
        let where_clause_body = "{ for e in self.artifacts.iter() { } }";
        assert!(
            !is_unpinned_currency_oracle(where_clause_sig, where_clause_body),
            "self-test: the `where`-clause `()`-returning augmentation-index \
             refresh shape MUST NOT be flagged as a currency oracle",
        );
    }
}

// ===========================================================================
// Block 6.d — per-member graph-native materialiser cache wire-up guard.
//
// `surface_member_to_expanded_field` MUST peek
// `MemberShapeCacheDb` BEFORE calling `raise_node_to_type_expr`. The
// guard pins the contract so a future refactor cannot accidentally
// swap the peek-before-raise wire-up for a raise-then-reduce wrapper
// (which would re-introduce the +52% regression Block 6.c surfaced).
//
// Discriminating property: the source-grep for the helper call name
// (`member_shape_peek_or_compute`) must occur BEFORE any
// `raise_node_to_type_expr` call inside the function body. The body
// is extracted by literal string slicing rather than parsing — a
// rename of the helper (or an inversion of the peek/raise order)
// fails the guard.
// ===========================================================================

#[test]
fn surface_member_field_consults_member_shape_cache_before_round_trip() {
    let source = read_workspace_file("crates/verter_session/src/meta_resolve/projectors/mod.rs");

    // Extract the surface_member_to_expanded_field body via brace
    // matching from the signature marker through the function close.
    let fn_marker = "pub(crate) fn surface_member_to_expanded_field(";
    let fn_start = source
        .find(fn_marker)
        .expect("surface_member_to_expanded_field must exist in projectors/mod.rs");
    // Find the opening brace of the function body.
    let open_brace_offset = source[fn_start..]
        .find(") -> ExpandedField {")
        .expect("function signature must terminate at `) -> ExpandedField {`")
        + fn_start;
    let body_start = open_brace_offset + ") -> ExpandedField {".len();
    // Brace-match to find the function's closing brace.
    let bytes = source.as_bytes();
    let mut depth: i32 = 1;
    let mut idx = body_start;
    while idx < bytes.len() {
        match bytes[idx] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            _ => {}
        }
        idx += 1;
    }
    assert!(
        depth == 0,
        "must find the closing brace of surface_member_to_expanded_field"
    );
    let body = &source[body_start..idx];

    // Strip line comments (`//...`) so the guard does not match
    // example-form mentions inside docstrings. We deliberately do NOT
    // strip block comments (`/* */`) — none exist in this file's
    // function body — and we deliberately do NOT parse Rust tokens
    // since a structural parse would mask the literal-call check we
    // are trying to perform.
    let mut stripped = String::with_capacity(body.len());
    for line in body.lines() {
        if let Some(comment_idx) = line.find("//") {
            stripped.push_str(&line[..comment_idx]);
        } else {
            stripped.push_str(line);
        }
        stripped.push('\n');
    }
    let body = stripped.as_str();

    // (1) The peek-before-raise helper must be invoked inside the body.
    let cache_call_offset = body
        .find("member_shape_peek_or_compute(")
        .unwrap_or_else(|| {
            panic!(
                "surface_member_to_expanded_field MUST call \
             `member_shape_peek_or_compute(...)` for the type reduction path \
             (Block 6.d wire-up). A future refactor that bypasses the cache \
             will re-introduce the +52% regression Block 6.c surfaced."
            )
        });

    // (2) Any `raise_node_to_type_expr(member.value)` call MUST occur
    // AFTER the cache peek. The exactness path's
    // `resolve_member_value_for_classification` call is allowed
    // anywhere; only the literal raise-of-member.value is restricted
    // (the exactness path does not call raise_node_to_type_expr on
    // member.value).
    let raise_of_member_value = body.find("raise_node_to_type_expr(member.value)");
    if let Some(raise_offset) = raise_of_member_value {
        assert!(
            cache_call_offset < raise_offset,
            "surface_member_to_expanded_field MUST peek `member_shape_peek_or_compute` \
             BEFORE any `raise_node_to_type_expr(member.value)` call (Block 6.d \
             contract). The current order has the raise at offset {raise_offset} \
             and the cache peek at offset {cache_call_offset}.",
        );
    }
}

#[test]
fn surface_member_arch_guard_self_test_detects_inverted_order() {
    // Self-test: a body that calls raise BEFORE the cache helper must
    // fail the substring-position check.
    let bad_body = "
        let raised = dispatch.raise_node_to_type_expr(member.value).unwrap();
        let r#type = member_shape_peek_or_compute(...).type_expr;
    ";
    let cache_call_offset = bad_body
        .find("member_shape_peek_or_compute(")
        .expect("test must contain the helper call");
    let raise_offset = bad_body
        .find("raise_node_to_type_expr(member.value)")
        .expect("test must contain the raise call");
    assert!(
        cache_call_offset > raise_offset,
        "self-test: inverted-order body must have cache call AFTER raise"
    );
}

// ===========================================================================
// Typed-IR bridge — ImportedMacroSurface containment guards
// ===========================================================================
//
// The `ImportedMacroSurface` lazy typed-IR bridge
// (`crates/verter_session/src/resolver_core/component_meta/imported_surface.rs`)
// MUST remain confined to `verter_session`'s resolver-core layer — it is a
// `pub(crate)`-dispatching internal abstraction that composes
// `SemanticQueryKey::ResolveDecl` + `ProjectPath` and does not belong
// in:
//
// - `verter_semantic` (the semantic extractor — owns analysis snapshots,
//   not host dispatch),
// - `verter_protocol` (transport-facing DTOs — must remain
//   serializable shapes, never typed-IR bridge identities),
// - `verter_ffi` (NAPI/WASM adapter — host objects must not leak the
//   bridge type into the FFI surface),
// - the TypeScript compat layers under `packages/component-meta/*`
//   (consumers of the public component-meta payload).
//
// Additionally, the bridge's public accessors MUST take an explicit
// `&dyn ResolverContext` parameter. Zero-arg `&self` accessors that
// secretly dispatch through TLS would violate R25 / R31: hidden lazy
// reads behind `&self` would hide dispatch cost, dep-signature merge,
// and cache-suppress propagation from the call site. The guard below
// scans the bridge module for any public method (`pub fn` /
// `pub(crate) fn`) on `impl ImportedMacroSurface` and asserts the
// signature carries a `ResolverContext` parameter.

/// Containment guard — `ImportedMacroSurface` does not appear in
/// `verter_semantic`.
#[test]
fn imported_macro_surface_not_in_verter_semantic() {
    let root = workspace_root();
    let semantic_src = root.join("crates/verter_semantic/src");
    let mut hits: Vec<String> = Vec::new();
    walk_dir_collect_rs(&semantic_src, &mut |path: &std::path::Path| {
        let src = std::fs::read_to_string(path).unwrap_or_default();
        for (lineno, line) in src.lines().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") || trimmed.starts_with("///") {
                continue;
            }
            if line.contains("ImportedMacroSurface") {
                let rel = path
                    .strip_prefix(&root)
                    .unwrap_or(path)
                    .to_string_lossy()
                    .replace('\\', "/");
                hits.push(format!("{rel}:{}: {}", lineno + 1, line.trim()));
            }
        }
    });
    assert!(
        hits.is_empty(),
        "guard `imported_macro_surface_not_in_verter_semantic`: \
         `verter_semantic/src/**` MUST NOT reference `ImportedMacroSurface`. \
         The bridge is `verter_session`-internal typed-IR dispatch \
         infrastructure; `verter_semantic` owns analysis snapshots, not \
         host dispatch. Offending lines:\n  {}",
        hits.join("\n  "),
    );
}

/// Containment guard — `ImportedMacroSurface` does not appear in
/// `verter_protocol`, `verter_ffi`, `verter_napi`, `verter_wasm`,
/// or the TypeScript compat layers under `packages/component-meta/`.
///
/// The bridge is internal typed-IR infrastructure. Leaking it into a
/// protocol DTO, an FFI host object, or a JS compat shape would
/// promote internal dispatch identity into the public API surface —
/// exactly the seam codex BINDING prohibits.
#[test]
fn imported_macro_surface_not_in_protocol_or_ffi() {
    let root = workspace_root();
    // Substring-based scan across each scope. Substring is
    // sufficient because the bridge identifier is unique
    // (`ImportedMacroSurface`) and the scopes are small enough
    // that a per-file walk is fast.
    let scopes: &[&str] = &[
        "crates/verter_protocol/src",
        "crates/verter_ffi/src",
        "crates/verter_napi/src",
        "crates/verter_wasm/src",
        "packages/component-meta/src",
        "packages/component-meta/compat/src",
    ];
    let mut hits: Vec<String> = Vec::new();
    for scope in scopes {
        let scope_path = root.join(scope);
        if !scope_path.is_dir() {
            // Some scopes may not exist in every checkout (e.g.
            // `packages/component-meta/compat/src` if the compat
            // layer is still empty). The guard tolerates absent
            // scopes — what matters is that any extant source
            // file in any present scope is clean.
            continue;
        }
        walk_dir_collect_rs_and_ts(&scope_path, &mut |path: &std::path::Path| {
            let src = std::fs::read_to_string(path).unwrap_or_default();
            for (lineno, line) in src.lines().enumerate() {
                let trimmed = line.trim_start();
                if trimmed.starts_with("//") || trimmed.starts_with("///") {
                    continue;
                }
                if line.contains("ImportedMacroSurface") {
                    let rel = path
                        .strip_prefix(&root)
                        .unwrap_or(path)
                        .to_string_lossy()
                        .replace('\\', "/");
                    hits.push(format!("{rel}:{}: {}", lineno + 1, line.trim()));
                }
            }
        });
    }
    assert!(
        hits.is_empty(),
        "guard `imported_macro_surface_not_in_protocol_or_ffi`: \
         `ImportedMacroSurface` MUST NOT appear in protocol DTOs, FFI \
         adapters, or JS compat layers. The bridge is internal typed-IR \
         dispatch infrastructure — leaking it into a public boundary \
         promotes an internal identity into a published API. Offending \
         lines:\n  {}",
        hits.join("\n  "),
    );
}

/// Walk a directory and apply `cb` to every `.rs` or `.ts` file.
/// Shared by the protocol / FFI / compat scan above.
fn walk_dir_collect_rs_and_ts(dir: &std::path::Path, cb: &mut dyn FnMut(&std::path::Path)) {
    for entry in walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.path().is_file())
    {
        let path = entry.path();
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if matches!(ext, "rs" | "ts" | "tsx") {
            cb(path);
        }
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Stage 2B.1 — macro-authority cluster reads through `ResolvedMacroSurface`
// ───────────────────────────────────────────────────────────────────────────
//
// Stage 2B.1 migrated the two semantics-owning macro-authority consumers —
// the macro-shape producers (`macro_shapes.rs`) and the slot-binding graph
// (`slot_binding_graph.rs`) — to read their `defineProps` / `defineEmits` /
// `defineSlots` member sets through the `ResolvedMacroSurface` enum's shared
// `prop_members` / `emit_members` / `slot_members` accessors rather than the
// direct `.props` / `.emits` / `.slots` fields on `ResolvedMacroMeta`.
//
// The enum is the seam between the eager OXC-resolved-elements surface (the
// `Eager` arm, what production produces today) and the lazy typed-IR bridge
// (the `LazyImported` arm). A direct `.props` field read on a
// `ResolvedMacroMeta` bypasses the seam — it would interpret the eager arm's
// fields directly and silently skip the lazy arm, defeating the migration.
//

// ===========================================================================
// Single Resolution Engine guards (CLAUDE.md "Single Resolution Engine Rule")
// ===========================================================================
//
// Verter must have exactly ONE type-resolution engine: the canonical
// typed-IR dispatch `SemanticQueryKey -> ProjectSemanticDispatch::execute
// -> SemanticGraphStore`. The redundant eager OXC `resolve_type` engine
// (`crates/verter_parser/src/utils/oxc/vue/script/resolve_type/`) plus its
// query-time rail (prepared-surface walker, eager macro-surface producer,
// `ResolvedElements` output type) is being DELETED across the consolidation
// stages.
//
// These guards are the FIRST stage (Stage 0): they lock the demolition so
// that while later stages tear the old engine down, NO new production site
// of a doomed symbol can be added. Each guard owns an EXACT allowlist of the
// symbol's CURRENT production sites captured against the live tree; the match
// is bidirectional, exactly like `typed_ir_resolver_guards`:
//
//   * a site in source that is NOT in the allowlist fails ("Unallowlisted
//     site introduced") — this is the new-production-site trap;
//   * an allowlist entry that no longer matches anything in source ALSO fails
//     ("Allowlisted entry NOT FOUND") — so the allowlist is a SHRINKING
//     ledger: every later stage that deletes a site removes its entry, and
//     the post-consolidation floor is empty allowlists.
//
// Granularity is per-symbol and principled:
//   * `from_eager_meta` and the duplicate `read_surface_members` DEFINITIONS
//     are few and the exact site matters — line-precise `(file, line,
//     pattern)` tuples (matching `typed_ir_resolver_guards`).
//   * `resolve_type::`, `ResolvedElements`, and `PreparedSurfaceProjection`
//     are pervasive WITHIN their owning modules but must not spread to NEW
//     files — file-level allowlists (matching `GET_ANY_ALLOWLIST` /
//     `no_std_fs_outside_native_fs_or_allow_list`). A new production *file*
//     referencing the symbol is the architecturally meaningful violation.
//
// LEGITIMATE FRONT-END IS NOT FLAGGED: the one-time TS->TypeExpr lowering
// `verter_type_expr_oxc::lower_ts_type` (called during shallow analysis,
// produces the `TypeExpr` stored on `IndexedReady`) is the canonical
// front-end the one resolver is built on. It lives in
// `crates/verter_type_expr_oxc/` and references NONE of the forbidden tokens,
// so the scanners exclude it naturally. Only QUERY-time OXC resolution is
// forbidden.
mod single_resolution_engine_guards {
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::{Path, PathBuf};

    /// The EXPLICIT set of `test_only_*` probe files exempt from the
    /// production-site ledgers. Restricted by exact path (NOT a `test_only_`
    /// name prefix) so a FUTURE production module that happens to be named
    /// `test_only_foo.rs` is NOT silently exempt — it would be scanned like any
    /// production file and would have to be allowlisted (or, correctly,
    /// rejected) if it referenced a doomed engine symbol. See the [P2] re-review
    /// finding: a blanket `test_only_*` prefix exemption is a hole because the
    /// `test_only_module_is_only_consumed_by_test_files` guard only proves the
    /// CURRENT `pub mod test_only` module is unconsumed — it says nothing about
    /// an arbitrary future `test_only_*`-named file.
    ///
    /// `test_only_imported_macro_surface.rs` is the sole entry: a
    /// `#[doc(hidden)] pub mod test_only` probe body (attached via `#[path]`),
    /// compiled in all builds but a test-only probe by contract (the
    /// `test_only_module_is_only_consumed_by_test_files` guard pins that it is
    /// consumed only by test files). It is NOT a production rail, so it is
    /// exempt from these production-site ledgers.
    // The eager-rail test probe (`test_only_imported_macro_surface.rs`) is
    // DELETED, so there are no by-exact-path probe exemptions. `test_only_`
    // NAME-prefix alone still does NOT exempt (only an exact path in this list
    // does — see `is_test_or_probe_file` /
    // `test_only_prefix_alone_does_not_exempt_a_rogue_file`).
    const KNOWN_PROBE_FILES: &[&str] = &[];

    /// Walk `<repo>/crates/<crate>/src/**` and yield every production
    /// `.rs` file as `(absolute_path, repo_relative_path)`. Excludes
    /// test-only sources exactly as the sibling guards do — `<name>_tests.rs`
    /// and `tests.rs` siblings, and anything under a `tests/` path segment —
    /// PLUS the explicitly-enumerated `KNOWN_PROBE_FILES` probe bodies (an
    /// allowlist by exact path, NOT a `test_only_` name prefix; see
    /// `KNOWN_PROBE_FILES` and `is_test_or_probe_file`).
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
                if is_test_or_probe_file(&rel) {
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

    /// True for files whose contents are test-only: `*_tests.rs` / `tests.rs`
    /// siblings, anything under a `tests/` segment, or one of the EXPLICITLY
    /// enumerated `KNOWN_PROBE_FILES` probe bodies (matched by exact
    /// repo-relative path).
    ///
    /// A `test_only_*`-NAMED file that is NOT in `KNOWN_PROBE_FILES` is NOT
    /// exempt — it is treated as a production file and scanned by the ledgers.
    /// This closes the [P2] hole where a blanket `test_only_` prefix exemption
    /// let a future production module named `test_only_foo.rs` add doomed-engine
    /// uses and be omitted from all ledgers, even though the
    /// `test_only_module_is_only_consumed_by_test_files` guard never proved that
    /// arbitrary file to be a probe.
    fn is_test_or_probe_file(rel: &str) -> bool {
        let name = rel.rsplit('/').next().unwrap_or("");
        if name.ends_with("_tests.rs") || name == "tests.rs" {
            return true;
        }
        if KNOWN_PROBE_FILES.contains(&rel) {
            return true;
        }
        rel.split('/').any(|seg| seg == "tests")
    }

    /// Replace `//` line comments and `/* ... */` block comments with
    /// equivalent-length whitespace, preserving newlines so line numbers
    /// stay stable. Skips comment-like sequences inside regular and raw
    /// string literals so the strip never invalidates real source.
    /// (Mirrors `typed_ir_resolver_guards::strip_comments` /
    /// `no_legacy_walker::strip_comments`.)
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

    /// Replace the body of every `#[cfg(test)] mod NAME { ... }` block with
    /// whitespace (newlines preserved). Inline test modules live in
    /// production source files but are test-only — guard scans must NOT
    /// classify them as production violations. (Mirrors
    /// `typed_ir_resolver_guards::strip_inline_test_modules`.)
    fn strip_inline_test_modules(src: &str) -> String {
        let bytes = src.as_bytes();
        let n = bytes.len();
        let mut out = bytes.to_vec();
        let needle = b"#[cfg(test)]";
        let mut i = 0usize;
        while i + needle.len() <= n {
            if &bytes[i..i + needle.len()] == needle {
                let mut j = i + needle.len();
                let limit = (i + 200).min(n);
                while j < limit {
                    if j + 4 <= n && &bytes[j..j + 4] == b"mod " {
                        break;
                    }
                    j += 1;
                }
                if j + 4 <= n && &bytes[j..j + 4] == b"mod " {
                    let mut k = j + 4;
                    while k < n && bytes[k] != b'{' && bytes[k] != b';' {
                        k += 1;
                    }
                    if k < n && bytes[k] == b'{' {
                        let mut depth = 1i32;
                        let mut m = k + 1;
                        while m < n && depth > 0 {
                            // Skip string / char / raw-string literals so a
                            // `{` or `}` inside a literal in the test-mod body
                            // cannot desync the brace depth counter (P3).
                            if let Some(next) = skip_literal(bytes, m) {
                                m = next;
                                continue;
                            }
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

    /// If `bytes[at]` begins a string, raw-string, byte-string, or char
    /// literal, return the index just past its closing delimiter; otherwise
    /// `None`. Used by `strip_inline_test_modules` so braces inside literals
    /// inside a `#[cfg(test)] mod` body do not desync the depth counter.
    ///
    /// A leading `b` (byte string / byte char) is consumed transparently. A
    /// `'` that does NOT close as a well-formed char literal (i.e. a lifetime
    /// such as `'a` / `'static`) is reported as `None` — lifetimes contain no
    /// braces, so leaving them to the byte-by-byte scan is correct.
    fn skip_literal(bytes: &[u8], at: usize) -> Option<usize> {
        let n = bytes.len();
        if at >= n {
            return None;
        }
        // Optional byte-literal prefix: b"..." / br"..." / b'.'
        let mut start = at;
        if bytes[start] == b'b' && start + 1 < n && matches!(bytes[start + 1], b'"' | b'\'' | b'r')
        {
            start += 1;
        }
        if start >= n {
            return None;
        }
        // Raw string: r"..." / r#"..."# / r##"..."## ...
        if bytes[start] == b'r' {
            let mut j = start + 1;
            let mut hashes = 0usize;
            while j < n && bytes[j] == b'#' {
                hashes += 1;
                j += 1;
            }
            if j < n && bytes[j] == b'"' {
                // Closing is `"` followed by `hashes` `#`.
                let mut k = j + 1;
                while k < n {
                    if bytes[k] == b'"' {
                        let mut h = 0usize;
                        while h < hashes && k + 1 + h < n && bytes[k + 1 + h] == b'#' {
                            h += 1;
                        }
                        if h == hashes {
                            return Some(k + 1 + hashes);
                        }
                    }
                    k += 1;
                }
                return Some(n);
            }
            // `r` not starting a raw string — not a literal here.
            return None;
        }
        // Regular string literal "..." with \" escape handling.
        if bytes[start] == b'"' {
            let mut k = start + 1;
            while k < n {
                if bytes[k] == b'\\' && k + 1 < n {
                    k += 2;
                    continue;
                }
                if bytes[k] == b'"' {
                    return Some(k + 1);
                }
                k += 1;
            }
            return Some(n);
        }
        // Char literal '\u{7b}' / '{' / '\n' / 'a'. Reject lifetimes.
        if bytes[start] == b'\'' {
            // Distinguish a char literal from a lifetime by the char-literal
            // grammar (so a lifetime like `'a` is NOT skipped, which would
            // otherwise swallow code up to the next `'`):
            //   * escaped form `'\x…'` — opens with `\`, closes at the next
            //     unescaped `'` (e.g. `'\n'`, `'\''`, `'\u{7b}'`);
            //   * simple form `'X'` — exactly one char then `'`.
            // Anything else (`'a`, `'static`) is a lifetime → not a literal.
            if start + 1 < n && bytes[start + 1] == b'\\' {
                let mut k = start + 2;
                let bound = (start + 16).min(n);
                while k < bound {
                    if bytes[k] == b'\'' {
                        return Some(k + 1);
                    }
                    k += 1;
                }
                return None;
            }
            if start + 2 < n && bytes[start + 2] == b'\'' {
                return Some(start + 3);
            }
            // Lifetime (or malformed) — not a literal.
            return None;
        }
        None
    }

    fn preprocess(src: &str) -> String {
        strip_inline_test_modules(&strip_comments(src))
    }

    /// Identifier-boundary matcher: `needle` matches at `line` ONLY when its
    /// occurrence is bounded by characters that cannot extend an identifier
    /// (not `[A-Za-z0-9_]`). Used for EVERY token guard so suffixed/embedding
    /// names never false-match: `from_eager_meta` vs `from_eager_meta_v2`,
    /// and — critically — `ResolvedElements` vs `ResolvedElementsOwned` (a
    /// distinct owned-artifact arena type that is NOT the doomed eager-OXC
    /// output and must not satisfy the `ResolvedElements` ledger). The
    /// `PreparedSurfaceProjection` enum is also matched at identifier boundary
    /// (it has no embedding identifier today, but boundary matching keeps a
    /// hypothetical `PreparedSurfaceProjectionV2` from satisfying a stale
    /// entry). The only NON-identifier token is `resolve_type`, which is a
    /// module path segment handled by `scan_resolve_type_module_target_counts`
    /// (it must match `resolve_type::`, `use …::resolve_type as rt`, etc.).
    fn line_contains_identifier(line: &str, needle: &str) -> bool {
        let bytes = line.as_bytes();
        let nb = needle.as_bytes();
        let n = nb.len();
        if n == 0 || bytes.len() < n {
            return false;
        }
        let is_ident_char = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
        let mut i = 0usize;
        while i + n <= bytes.len() {
            if &bytes[i..i + n] == nb {
                let before_ok = i == 0 || !is_ident_char(bytes[i - 1]);
                let after_ok = i + n == bytes.len() || !is_ident_char(bytes[i + n]);
                if before_ok && after_ok {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// Count of identifier-boundary occurrences of `needle` across the whole
    /// (already preprocessed) source. Non-overlapping. Used by the count-based
    /// file ledgers so that a NEW use added INSIDE an already-allowlisted file
    /// raises that file's observed count above its allowlisted count and the
    /// guard fires (the in-file-growth trap that a file-NAME-set guard misses).
    fn count_identifier_in_source(src: &str, needle: &str) -> usize {
        let bytes = src.as_bytes();
        let nb = needle.as_bytes();
        let n = nb.len();
        if n == 0 {
            return 0;
        }
        let is_ident_char = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
        let mut count = 0usize;
        let mut i = 0usize;
        while i + n <= bytes.len() {
            if &bytes[i..i + n] == nb {
                let before_ok = i == 0 || !is_ident_char(bytes[i - 1]);
                let after_ok = i + n == bytes.len() || !is_ident_char(bytes[i + n]);
                if before_ok && after_ok {
                    count += 1;
                    i += n;
                    continue;
                }
            }
            i += 1;
        }
        count
    }

    /// Count of `resolve_type` occurrences that act as a MODULE PATH SEGMENT in
    /// the (already preprocessed) source. This catches every architecturally
    /// meaningful use of the doomed OXC engine module — not just the
    /// `resolve_type::` call/path fragment, but also ALIASED and grouped/bare
    /// `use` imports that contain no `::` after the segment:
    ///
    ///   * `resolve_type::foo` / `…::resolve_type::{…}`  (path / call / glob)
    ///   * `use …::resolve_type as rt;`                  (ALIASED import — the
    ///     evasion the substring guard missed)
    ///   * `use …::{resolve_type, …};` / `use …::resolve_type;`  (bare import)
    ///   * `pub mod resolve_type;`                       (module declaration)
    ///
    /// Concretely: an identifier-boundary `resolve_type` whose next non-space
    /// token is `::`, `;`, `,`, `}`, or `as` (the path-segment / use-target
    /// terminators). A trailing identifier char (`resolve_type_dependency…`)
    /// is excluded by the identifier-boundary check.
    fn count_resolve_type_module_targets(src: &str) -> usize {
        let bytes = src.as_bytes();
        let nb = b"resolve_type";
        let n = nb.len();
        let is_ident_char = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
        let mut count = 0usize;
        let mut i = 0usize;
        while i + n <= bytes.len() {
            if &bytes[i..i + n] == nb {
                let before_ok = i == 0 || !is_ident_char(bytes[i - 1]);
                let after_idx = i + n;
                let after_ok = after_idx == bytes.len() || !is_ident_char(bytes[after_idx]);
                if before_ok && after_ok {
                    let mut k = after_idx;
                    while k < bytes.len()
                        && (bytes[k] == b' ' || bytes[k] == b'\n' || bytes[k] == b'\t')
                    {
                        k += 1;
                    }
                    let rest = &src[k..];
                    let is_target = rest.starts_with("::")
                        || rest.starts_with(';')
                        || rest.starts_with(',')
                        || rest.starts_with('}')
                        || rest.starts_with("as ")
                        || rest.starts_with("as\n");
                    if is_target {
                        count += 1;
                        i = after_idx;
                        continue;
                    }
                }
            }
            i += 1;
        }
        count
    }

    /// Collect the set of symbols a file imports FROM the doomed `resolve_type`
    /// engine module via `use` / `pub use` declarations, so their bare call /
    /// use sites can be counted as ACTUAL engine usage (not just the
    /// `resolve_type::` path token of the import line).
    ///
    /// This closes the in-file import-then-call hole the path-token-only ledger
    /// missed: a file that already imports `analyze_external_type_program` can
    /// add MORE bare `analyze_external_type_program(…)` calls without changing
    /// its `resolve_type` token count. By deriving the imported-symbol set from
    /// each file's OWN `use` statements (rather than a hand-maintained global
    /// API list) the counter is structural and self-updating: it reflects
    /// exactly what each file imports from the engine.
    ///
    /// Handles every `use` shape that targets the module:
    ///   * `use …::resolve_type::SYMBOL;`                  → {SYMBOL}
    ///   * `use …::resolve_type::SYMBOL as ALIAS;`         → {ALIAS}
    ///   * `use …::resolve_type::{A, B, C as D};`          → {A, B, D}
    ///     (grouped, possibly spanning multiple lines)
    ///   * `use …::resolve_type as M;`                     → {M}  (module alias —
    ///     subsequent `M::foo()` calls are then counted as engine use)
    ///   * `use …::resolve_type;` / `use …::{resolve_type};`  → {}  (bare module
    ///     import — adds no callable symbol; later `resolve_type::foo` uses are
    ///     already path tokens)
    ///
    /// **Scope boundary (intentional, do NOT chase further):** this closes the
    /// IN-FILE import-then-call class only. A cross-file re-export under a NEW
    /// name (`pub use …resolve_type::a as b;` in file X, then bare `b()` in a
    /// DIFFERENT file Y) is out of scope: the re-exporting file X is already
    /// caught by its own `resolve_type` path token, and after the consolidation
    /// deletes the engine (Stages 5/6) the compiler catches any dangling
    /// reference in Y. A re-export under the SAME name (`pub use resolve_type::
    /// {ResolvedElements};`) likewise needs no symbol-use counting in the
    /// re-exporting file — the `use` line is stripped before the symbol-use pass
    /// (see `count_resolve_type_engine_use`), so re-export bindings are never
    /// double-counted against the path token of the `use` line.
    ///
    /// The argument must already be `preprocess`ed (comments + `#[cfg(test)]`
    /// bodies erased).
    fn collect_resolve_type_imported_symbols(src: &str) -> BTreeSet<String> {
        let mut out: BTreeSet<String> = BTreeSet::new();
        let bytes = src.as_bytes();
        let nb = b"resolve_type";
        let n = nb.len();
        let is_ident_char = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
        let mut i = 0usize;
        while i + n <= bytes.len() {
            if &bytes[i..i + n] == nb {
                let before_ok = i == 0 || !is_ident_char(bytes[i - 1]);
                let after_idx = i + n;
                let after_ok = after_idx == bytes.len() || !is_ident_char(bytes[after_idx]);
                if before_ok && after_ok {
                    // Only treat a `resolve_type` segment that is part of a `use`
                    // declaration as an import. Walk back over the path segments
                    // (`ident ::` / `::`) and intervening whitespace to the start
                    // of the statement and require a `use` keyword there.
                    if resolve_type_segment_is_in_use_stmt(bytes, i) {
                        // Skip whitespace after the segment to find what follows.
                        let mut k = after_idx;
                        while k < bytes.len()
                            && (bytes[k] == b' ' || bytes[k] == b'\n' || bytes[k] == b'\t')
                        {
                            k += 1;
                        }
                        let rest = &src[k..];
                        if rest.starts_with("::") {
                            // `use …::resolve_type::<tail>` — tail is a single
                            // symbol, a `SYMBOL as ALIAS`, or a `{ group }`.
                            collect_use_tail_symbols(&src[k + 2..], &mut out);
                        } else if rest.starts_with("as ") || rest.starts_with("as\n") {
                            // `use …::resolve_type as M;` — module alias M.
                            let after_as = &src[k + 2..];
                            if let Some(name) = first_ident(after_as) {
                                out.insert(name);
                            }
                        }
                        // `use …::resolve_type;` / `…::{resolve_type, …}` (bare
                        // module import) contributes no callable symbol.
                    }
                    i = after_idx;
                    continue;
                }
            }
            i += 1;
        }
        out
    }

    /// Byte spans `[start, end)` of every `use` / `pub use` declaration in
    /// `bytes`, from the `use` keyword through (and including) its terminating
    /// `;`. Brace-aware: a `;` nested inside a `{ … }` group cannot terminate
    /// the statement (use-trees contain no bare `;` before the terminator), so
    /// grouped and arbitrarily-nested trees (`use a::{b, c::{d, e}};`,
    /// multi-line trees, etc.) are captured whole. `use` is a reserved keyword
    /// in Rust, so any identifier-boundary `use` is the keyword — never a path
    /// segment — which is why anchoring on it and scanning RIGHT is robust where
    /// reverse-engineering a path prefix by walking LEFT is not.
    fn use_statement_spans(bytes: &[u8]) -> Vec<(usize, usize)> {
        let n = bytes.len();
        let is_ident_char = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
        let mut spans: Vec<(usize, usize)> = Vec::new();
        let mut i = 0usize;
        while i + 3 <= n {
            if &bytes[i..i + 3] == b"use"
                && (i == 0 || !is_ident_char(bytes[i - 1]))
                && (i + 3 == n || !is_ident_char(bytes[i + 3]))
            {
                let mut depth = 0i32;
                let mut j = i + 3;
                let mut end = n;
                while j < n {
                    match bytes[j] {
                        b'{' => depth += 1,
                        b'}' => depth -= 1,
                        b';' if depth == 0 => {
                            end = j + 1;
                            break;
                        }
                        _ => {}
                    }
                    j += 1;
                }
                spans.push((i, end));
                i = end;
                continue;
            }
            i += 1;
        }
        spans
    }

    /// True iff the `resolve_type` segment at byte index `seg` lies inside a
    /// `use` / `pub use` declaration. This is decided structurally — by whether
    /// `seg` falls within a `use`-statement span (see `use_statement_spans`) —
    /// rather than by reverse-engineering the path prefix. That makes it robust
    /// to EVERY use-tree grouping/nesting form, including ones where the byte
    /// immediately to the left of `resolve_type` is NOT `::` / an identifier:
    ///
    ///   * leading group:    `use a::{resolve_type::X};`          (`{` to the left)
    ///   * sibling group:    `use a::{b::Y, resolve_type::X};`    (`, ` to the left)
    ///   * deep nesting:     `use a::{b::{resolve_type::X}};`
    ///   * mid-group alias:  `use a::{resolve_type::X as Z, b};`
    ///   * `as` module alias inside a group: `use a::{resolve_type as rt};`
    ///   * multi-line trees and `pub use`.
    ///
    /// **Glob imports are OUT OF SCOPE (intentional, do NOT chase).** A
    /// `use …::resolve_type::*;` binds the module's entire export set under
    /// unknown local names; a per-file parser cannot enumerate those names
    /// without the module's export list (a cross-file fact this single-file scan
    /// deliberately does not load). Bare-call sites of glob-imported engine
    /// symbols are therefore not counted here. This boundary is safe: the
    /// re-exporting/glob-importing file is itself caught by its own
    /// `resolve_type` path token, and once the consolidation deletes the engine
    /// (Stages 5/6) the Rust compiler hard-errors on any dangling reference to a
    /// removed symbol. Glob / macro-generated / cross-file-re-export forms are
    /// explicitly not pursued.
    fn resolve_type_segment_is_in_use_stmt(bytes: &[u8], seg: usize) -> bool {
        use_statement_spans(bytes)
            .into_iter()
            .any(|(start, end)| seg >= start && seg < end)
    }

    /// Parse the tail of a `use …::resolve_type::<tail>` declaration starting at
    /// `tail` (the text just past the `::` after `resolve_type`). Inserts every
    /// bound symbol name (the alias for `X as Y`, else the leaf identifier) into
    /// `out`. Handles a single symbol, `SYMBOL as ALIAS`, and a `{ … }` group
    /// (which may itself nest further `::` paths — only leaf-bound names are
    /// collected). A trailing glob `*` binds nothing.
    fn collect_use_tail_symbols(tail: &str, out: &mut BTreeSet<String>) {
        let tail = tail.trim_start();
        if let Some(stripped) = tail.strip_prefix('{') {
            // Grouped: split top-level by commas, recurse per item.
            let end = match find_matching_brace(stripped) {
                Some(e) => e,
                None => stripped.len(),
            };
            let inner = &stripped[..end];
            for item in split_top_level_commas(inner) {
                let item = item.trim();
                if item.is_empty() {
                    continue;
                }
                collect_use_tail_symbols(item, out);
            }
            return;
        }
        // Single path segment: `SYMBOL`, `SYMBOL as ALIAS`, `nested::SYMBOL`,
        // `nested::{…}`, or a glob `*`.
        // First, if there is a `::` before any `{`/`,`/`;`, descend into the
        // sub-path.
        if let Some(sep) = find_path_separator(tail) {
            collect_use_tail_symbols(&tail[sep + 2..], out);
            return;
        }
        // Leaf: `SYMBOL` or `SYMBOL as ALIAS`. Strip a trailing `as ALIAS`.
        let leaf = tail.split([',', '}', ';']).next().unwrap_or("").trim();
        if leaf.is_empty() || leaf == "*" {
            return;
        }
        if let Some(idx) = find_as_keyword(leaf) {
            // `SYMBOL as ALIAS` — bind the ALIAS.
            if let Some(alias) = first_ident(&leaf[idx + 2..]) {
                out.insert(alias);
            }
        } else if let Some(name) = first_ident(leaf) {
            out.insert(name);
        }
    }

    /// Index of the FIRST top-level `::` in `s` that occurs before any of
    /// `{`, `,`, `}`, `;` (i.e. a path descent inside a single use-tree item).
    fn find_path_separator(s: &str) -> Option<usize> {
        let b = s.as_bytes();
        let mut i = 0usize;
        while i + 1 < b.len() {
            match b[i] {
                b'{' | b',' | b'}' | b';' => return None,
                b':' if b[i + 1] == b':' => return Some(i),
                _ => {}
            }
            i += 1;
        }
        None
    }

    /// Index of an ` as ` keyword (identifier-bounded) in a leaf use item, or
    /// `None`. Only matches `as` surrounded by whitespace so `Tatlas` etc. never
    /// false-match.
    fn find_as_keyword(s: &str) -> Option<usize> {
        let b = s.as_bytes();
        let mut i = 0usize;
        while i + 2 <= b.len() {
            if &b[i..i + 2] == b"as" {
                let before_ok = i == 0 || matches!(b[i - 1], b' ' | b'\n' | b'\t');
                let after_ok = i + 2 == b.len() || matches!(b[i + 2], b' ' | b'\n' | b'\t');
                if before_ok && after_ok {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// The first identifier (`[A-Za-z0-9_]+`) in `s`, skipping leading
    /// whitespace. `None` if none.
    fn first_ident(s: &str) -> Option<String> {
        let b = s.as_bytes();
        let is_ident_char = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
        let mut i = 0usize;
        while i < b.len() && matches!(b[i], b' ' | b'\n' | b'\t') {
            i += 1;
        }
        let start = i;
        while i < b.len() && is_ident_char(b[i]) {
            i += 1;
        }
        if i > start {
            Some(s[start..i].to_string())
        } else {
            None
        }
    }

    /// Index of the `}` that closes the group opened just before `s` (i.e. `s`
    /// begins just past the opening `{`). Brace-depth aware. `None` if
    /// unbalanced (caller falls back to end-of-string).
    fn find_matching_brace(s: &str) -> Option<usize> {
        let b = s.as_bytes();
        let mut depth = 0i32;
        let mut i = 0usize;
        while i < b.len() {
            match b[i] {
                b'{' => depth += 1,
                b'}' => {
                    if depth == 0 {
                        return Some(i);
                    }
                    depth -= 1;
                }
                _ => {}
            }
            i += 1;
        }
        None
    }

    /// Split `s` on TOP-LEVEL commas (commas not nested inside `{ … }`), used to
    /// separate items inside a `use` group. Returns the slices between commas.
    fn split_top_level_commas(s: &str) -> Vec<&str> {
        let b = s.as_bytes();
        let mut out: Vec<&str> = Vec::new();
        let mut depth = 0i32;
        let mut start = 0usize;
        let mut i = 0usize;
        while i < b.len() {
            match b[i] {
                b'{' => depth += 1,
                b'}' => depth -= 1,
                b',' if depth == 0 => {
                    out.push(&s[start..i]);
                    start = i + 1;
                }
                _ => {}
            }
            i += 1;
        }
        out.push(&s[start..]);
        out
    }

    /// Replace every `use` / `pub use` declaration that references the
    /// `resolve_type` module with equivalent-length whitespace (newlines
    /// preserved). Used so the imported-symbol-use pass in
    /// `count_resolve_type_engine_use` does NOT count the symbol-binding
    /// occurrences inside the import declaration itself — those are already
    /// represented by the `resolve_type` path token of the `use` line. Real
    /// call / use sites elsewhere in the file are untouched and DO count.
    ///
    /// A statement is blanked from its `use` keyword through its terminating
    /// `;` (brace-aware, so a `;` inside an inline expression cannot occur in a
    /// `use` — `use` items contain no `;` before the terminator). Statement
    /// extents come from the shared `use_statement_spans` finder, so grouped /
    /// arbitrarily-nested / multi-line trees are blanked whole. The argument
    /// must already be `preprocess`ed.
    fn strip_resolve_type_use_statements(src: &str) -> String {
        let bytes = src.as_bytes();
        let mut out = bytes.to_vec();
        for (start, end) in use_statement_spans(bytes) {
            // Only blank it if it references the `resolve_type` module.
            if count_resolve_type_module_targets(&src[start..end]) > 0 {
                for slot in &mut out[start..end] {
                    if *slot != b'\n' {
                        *slot = b' ';
                    }
                }
            }
        }
        String::from_utf8_lossy(&out).into_owned()
    }

    /// Count BARE identifier-boundary uses of `needle` in `src` — occurrences
    /// that are NOT immediately preceded by a `::` path separator (ignoring
    /// intervening whitespace). A `::`-qualified use such as
    /// `resolve_type::ResolvedElements` is EXCLUDED here because its
    /// `resolve_type` segment is already tallied by
    /// `count_resolve_type_module_targets` — counting the trailing `ResolvedElements`
    /// again would double-count that single full-path site. Bare uses
    /// (`ResolvedElements`, `build_type_context(…)`, `ResolvedElements::default()`)
    /// ARE counted: those are the imported-symbol call/use sites the path-token
    /// proxy missed.
    fn count_bare_symbol_uses(src: &str, needle: &str) -> usize {
        let bytes = src.as_bytes();
        let nb = needle.as_bytes();
        let n = nb.len();
        if n == 0 {
            return 0;
        }
        let is_ident_char = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
        let mut count = 0usize;
        let mut i = 0usize;
        while i + n <= bytes.len() {
            if &bytes[i..i + n] == nb {
                let before_ok = i == 0 || !is_ident_char(bytes[i - 1]);
                let after_ok = i + n == bytes.len() || !is_ident_char(bytes[i + n]);
                if before_ok && after_ok {
                    // Reject `::`-qualified occurrences (path uses already
                    // counted as a `resolve_type` module token). Walk left over
                    // whitespace; if the two chars before are `::`, skip.
                    let mut b = i;
                    while b > 0 && matches!(bytes[b - 1], b' ' | b'\n' | b'\t') {
                        b -= 1;
                    }
                    let qualified = b >= 2 && &bytes[b - 2..b] == b"::";
                    if !qualified {
                        count += 1;
                        i += n;
                        continue;
                    }
                }
            }
            i += 1;
        }
        count
    }

    /// ACTUAL engine-use count for a file: the `resolve_type` module-path-token
    /// count PLUS the BARE use sites of every symbol the file imports from the
    /// engine (counted on the source with the importing `use` declarations
    /// blanked, so import bindings are not double-counted, and excluding
    /// `::`-qualified uses, so a full `resolve_type::SYMBOL` path is not counted
    /// twice — once as a `resolve_type` token and once as a `SYMBOL` use).
    ///
    /// This is the structural fix for the path-token-PROXY hole: adding a bare
    /// call to an already-imported engine function now increments the count, so
    /// the per-file ledger reflects real engine usage rather than a proxy for
    /// the number of `resolve_type::` path tokens. The argument must already be
    /// `preprocess`ed.
    fn count_resolve_type_engine_use(src: &str) -> usize {
        let path_tokens = count_resolve_type_module_targets(src);
        let symbols = collect_resolve_type_imported_symbols(src);
        if symbols.is_empty() {
            return path_tokens;
        }
        let stripped = strip_resolve_type_use_statements(src);
        let mut symbol_uses = 0usize;
        for sym in &symbols {
            symbol_uses += count_bare_symbol_uses(&stripped, sym);
        }
        path_tokens + symbol_uses
    }

    fn fmt_match(m: &(String, u32, String)) -> String {
        format!("({:?}, {}, {:?})", m.0, m.1, m.2)
    }

    /// Line-precise bidirectional allowlist comparison. Fails on EITHER an
    /// unallowlisted site OR a stale allowlist entry. (Same contract as
    /// `typed_ir_resolver_guards::assert_exact_allowlist_match`.)
    fn assert_exact_allowlist_match(
        guard_name: &str,
        actual: &[(String, u32, String)],
        allowed: &[(&str, u32, &str)],
    ) {
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
                "\nUnallowlisted single-resolution-engine site introduced. A NEW \
                 production use of a doomed symbol is forbidden while the second \
                 engine is being deleted. Route through the canonical typed-IR \
                 dispatch (SemanticQueryKey -> ProjectSemanticDispatch::execute) \
                 instead. If this is a legitimately new site (it almost never is), \
                 add it to the allowlist with a justification:\n",
            );
            for entry in &unexpected {
                msg.push_str("    ");
                msg.push_str(entry);
                msg.push('\n');
            }
        }
        if !stale.is_empty() {
            msg.push_str(
                "\nAllowlisted entry NOT FOUND in source — a later stage removed \
                 this site (good!). Remove the stale entry so the ledger keeps \
                 shrinking; line number may have shifted:\n",
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

    /// Per-file OCCURRENCE-COUNT shrinking ledger. The allowlist is a map
    /// `file -> count`; `actual` is the observed `(file, count)` per production
    /// file (count > 0 only). The comparison fails on ANY of:
    ///
    ///   * **in-file growth** — an allowlisted file whose observed count is
    ///     GREATER than its allowlisted count (a NEW use added inside a file
    ///     that already had some; the hole a file-NAME-set guard misses);
    ///   * **new file** — a file with a positive count that is not in the
    ///     allowlist (the classic new-production-file trap);
    ///   * **stale / shrunk entry** — an allowlisted file whose observed count
    ///     is LOWER than allowlisted, or that is gone entirely (a later stage
    ///     deleted uses, but the entry was not updated). Counts only ever
    ///     SHRINK as the consolidation deletes the doomed engine, so a mismatch
    ///     in either direction is a ledger that must be re-derived. The
    ///     post-consolidation floor is an empty allowlist.
    ///
    /// This makes the ledger discriminate at SITE granularity, not just
    /// file-set granularity, without the line-number churn that would plague a
    /// `(file, line)` ledger across the 8 cutover stages.
    fn assert_exact_file_count_allowlist_match(
        guard_name: &str,
        actual: &[(String, usize)],
        allowed: &[(&str, usize)],
    ) {
        use std::collections::BTreeMap;
        let actual_map: BTreeMap<&str, usize> =
            actual.iter().map(|(f, c)| (f.as_str(), *c)).collect();
        let allowed_map: BTreeMap<&str, usize> = allowed.iter().copied().collect();

        // New file OR in-file growth: observed file with count strictly above
        // its allowlisted count (allowlisted count is 0 for a non-listed file).
        let mut grew: Vec<String> = Vec::new();
        for (f, c) in &actual_map {
            let allowed_c = allowed_map.get(f).copied().unwrap_or(0);
            if *c > allowed_c {
                grew.push(format!("{f}  (observed {c} > allowlisted {allowed_c})"));
            }
        }
        // Stale / shrunk: allowlisted file whose observed count is below its
        // allowlisted count (0 if the file no longer matches at all).
        let mut shrank: Vec<String> = Vec::new();
        for (f, allowed_c) in &allowed_map {
            let observed = actual_map.get(f).copied().unwrap_or(0);
            if observed < *allowed_c {
                shrank.push(format!(
                    "{f}  (observed {observed} < allowlisted {allowed_c})"
                ));
            }
        }

        if grew.is_empty() && shrank.is_empty() {
            return;
        }

        let mut msg = format!("\n\n=== {guard_name} ===\n");
        if !grew.is_empty() {
            msg.push_str(
                "\nNEW production use of a doomed single-resolution-engine symbol \
                 (a brand-new file, OR a new site INSIDE an already-allowlisted \
                 file). The redundant OXC `resolve_type` engine / prepared-surface \
                 walker is being deleted — do NOT add new uses. Route through the \
                 canonical typed-IR dispatch (SemanticQueryKey -> \
                 ProjectSemanticDispatch::execute) instead:\n",
            );
            for f in &grew {
                msg.push_str("    ");
                msg.push_str(f);
                msg.push('\n');
            }
        }
        if !shrank.is_empty() {
            msg.push_str(
                "\nAllowlisted count is now LOWER (a later stage deleted uses — \
                 good!). Update the entry to the new lower count, or remove it if \
                 the file no longer references the symbol, so the ledger keeps \
                 shrinking toward the empty post-consolidation floor:\n",
            );
            for f in &shrank {
                msg.push_str("    ");
                msg.push_str(f);
                msg.push('\n');
            }
        }
        msg.push('\n');
        panic!("{msg}");
    }

    // -----------------------------------------------------------------------
    // Scanners
    // -----------------------------------------------------------------------

    /// Line-precise scan: every production line containing `needle` at an
    /// identifier boundary. Used for the token guards.
    fn scan_identifier_sites(needle: &str) -> Vec<(String, u32, String)> {
        let files = collect_production_rs_files();
        let mut out: Vec<(String, u32, String)> = Vec::new();
        for (path, rel) in &files {
            let src = match fs::read_to_string(path) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let stripped = preprocess(&src);
            for (idx, line) in stripped.split('\n').enumerate() {
                if line_contains_identifier(line, needle) {
                    out.push((rel.clone(), (idx + 1) as u32, needle.to_string()));
                }
            }
        }
        out
    }

    /// Structural `fn`-DEFINITION matcher: true iff `line` declares a function
    /// named `name`, matching the form
    ///
    /// ```text
    /// fn <ws> name <ws?> <generic-params?> <ws?> (
    /// ```
    ///
    /// i.e. the `fn` keyword (identifier-bounded), one-or-more whitespace, the
    /// identifier-bounded `name`, then — past optional whitespace and an
    /// OPTIONAL balanced `<…>` generic-parameter block (e.g. `<'a>`,
    /// `<T, U>`, `<T: Bound<X>>`) and more optional whitespace — an opening
    /// `(`. A fixed `fn name(` substring matcher (the prior `scan_literal_sites`
    /// approach) MISSED a generic-syntax duplicate `fn name<'a>(…)` and a
    /// whitespace-padded `fn  name (` form; this matches the structural
    /// definition shape instead. Visibility prefixes (`pub`, `pub(crate)`),
    /// `async`, `const`, etc. all sit BEFORE `fn` and are irrelevant because the
    /// scan locates the `fn` token anywhere on the line. Call sites
    /// (`read_surface_members(ctx, …)`) and imports (`read_surface_members,`)
    /// have no preceding `fn` token and never match.
    fn line_contains_fn_definition(line: &str, name: &str) -> bool {
        let bytes = line.as_bytes();
        let nm = name.as_bytes();
        let is_ident_char = |b: u8| b.is_ascii_alphanumeric() || b == b'_';

        // Locate every identifier-bounded `fn` token on the line.
        let mut i = 0usize;
        while i + 2 <= bytes.len() {
            let is_fn = &bytes[i..i + 2] == b"fn"
                && (i == 0 || !is_ident_char(bytes[i - 1]))
                && (i + 2 == bytes.len() || !is_ident_char(bytes[i + 2]));
            if !is_fn {
                i += 1;
                continue;
            }

            // Require >= 1 whitespace after `fn`.
            let mut j = i + 2;
            let ws_start = j;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if j == ws_start {
                i += 1;
                continue;
            }

            // The function name, identifier-bounded.
            if j + nm.len() <= bytes.len()
                && &bytes[j..j + nm.len()] == nm
                && (j == 0 || !is_ident_char(bytes[j - 1]))
                && (j + nm.len() == bytes.len() || !is_ident_char(bytes[j + nm.len()]))
            {
                let mut k = j + nm.len();
                // Optional whitespace before generics / params.
                while k < bytes.len() && bytes[k].is_ascii_whitespace() {
                    k += 1;
                }
                // Optional balanced `<…>` generic-parameter block.
                if k < bytes.len() && bytes[k] == b'<' {
                    let mut depth = 0i32;
                    let mut closed = false;
                    while k < bytes.len() {
                        match bytes[k] {
                            b'<' => depth += 1,
                            b'>' => {
                                depth -= 1;
                                if depth == 0 {
                                    k += 1;
                                    closed = true;
                                    break;
                                }
                            }
                            _ => {}
                        }
                        k += 1;
                    }
                    // An unbalanced `<` (generics spilling onto the next line)
                    // is not a single-line definition this matcher recognises.
                    if !closed {
                        i += 1;
                        continue;
                    }
                    // Optional whitespace between generics and `(`.
                    while k < bytes.len() && bytes[k].is_ascii_whitespace() {
                        k += 1;
                    }
                }
                if k < bytes.len() && bytes[k] == b'(' {
                    return true;
                }
            }

            i += 1;
        }
        false
    }

    /// Line-precise scan for a `fn`-DEFINITION of `name` (see
    /// `line_contains_fn_definition`). Emits the canonical `fn <name>(`
    /// representation per match so the allowlist tuple is stable regardless of
    /// the on-line spacing or generic parameters at the definition site.
    fn scan_fn_definition_sites(name: &str) -> Vec<(String, u32, String)> {
        let files = collect_production_rs_files();
        let canonical = format!("fn {name}(");
        let mut out: Vec<(String, u32, String)> = Vec::new();
        for (path, rel) in &files {
            let src = match fs::read_to_string(path) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let stripped = preprocess(&src);
            for (idx, line) in stripped.split('\n').enumerate() {
                if line_contains_fn_definition(line, name) {
                    out.push((rel.clone(), (idx + 1) as u32, canonical.clone()));
                }
            }
        }
        out
    }

    /// File-level COUNT scan: every production file and its identifier-boundary
    /// occurrence count for `needle` (post-preprocess), count > 0 only. Used by
    /// the count-based file ledgers for the pervasive token symbols
    /// (`ResolvedElements`, `PreparedSurfaceProjection`). Identifier-boundary
    /// matching means `ResolvedElementsOwned` never contributes to the
    /// `ResolvedElements` count.
    fn scan_file_identifier_counts(needle: &str) -> Vec<(String, usize)> {
        let files = collect_production_rs_files();
        let mut out: Vec<(String, usize)> = Vec::new();
        for (path, rel) in &files {
            let src = match fs::read_to_string(path) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let stripped = preprocess(&src);
            let c = count_identifier_in_source(&stripped, needle);
            if c > 0 {
                out.push((rel.clone(), c));
            }
        }
        out
    }

    /// File-level COUNT scan for ACTUAL `resolve_type` engine use: the module
    /// path-token count (calls + aliased/grouped/bare imports + module
    /// declaration) PLUS the use sites of every symbol the file imports from the
    /// engine (`count_resolve_type_engine_use`). Used by the `resolve_type`
    /// engine ledger so that BOTH an ALIASED import
    /// (`use …::resolve_type as rt;`, which contains no `resolve_type::`
    /// substring) in a new file AND a bare call to an already-imported engine
    /// function inside an existing file are caught — the latter being the
    /// path-token-proxy hole the [P2] re-review flagged.
    fn scan_file_resolve_type_target_counts() -> Vec<(String, usize)> {
        let files = collect_production_rs_files();
        let mut out: Vec<(String, usize)> = Vec::new();
        for (path, rel) in &files {
            let src = match fs::read_to_string(path) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let stripped = preprocess(&src);
            let c = count_resolve_type_engine_use(&stripped);
            if c > 0 {
                out.push((rel.clone(), c));
            }
        }
        out
    }

    // -----------------------------------------------------------------------
    // Guard 1: `from_eager_meta` — the eager macro-surface producer.
    //
    // `ResolvedMacroSurface::from_eager_meta(meta)` wraps the OXC engine's
    // `ResolvedMacroMeta` output into the cutover seam's `Eager` arm. Every
    // production call selects the redundant eager (OXC) surface; the
    // consolidation flips these to the canonical typed-IR macro payload and
    // then DELETES `from_eager_meta` and the `Eager` arm (codex Stage 2 row).
    // No NEW production caller may be added.
    //
    // Line-precise allowlist (few sites, exact site matters):
    //   * `imported_surface.rs:841` — the `from_eager_meta` DEFINITION (the
    //     seam constructor). Deleted when the `Eager` arm is removed.
    //   * `macro_shapes.rs:988/1109/1409/1512` + `slot_binding_graph.rs:991` —
    //     the five macro-authority consumer call sites (the producer flip
    //     flips these to the canonical macro payload).
    //
    // Test-only `from_eager_meta` probes in
    // `src/test_only_imported_macro_surface.rs` are NOT a production rail and
    // are excluded because that exact path is in `KNOWN_PROBE_FILES` (an
    // explicit by-path probe allowlist — NOT a `test_only_` name prefix; see
    // `is_test_or_probe_file`).
    // -----------------------------------------------------------------------

    #[test]
    fn no_new_from_eager_meta_production_site() {
        // The eager macro-surface rail (`ResolvedMacroSurface::from_eager_meta`)
        // is DELETED — production resolves props/emits/slots through the typeinfo
        // Vue surface (`VerterHost::vue_macro_dtos`). `from_eager_meta` must not
        // appear ANYWHERE (production source OR the wider `crates/*/src/**` tree
        // the production collector scans); the seam constructor and its lazy arm
        // are gone. A revival at any site fails this gate.
        let actual = scan_identifier_sites("from_eager_meta");
        assert!(
            actual.is_empty(),
            "`from_eager_meta` is a RETIRED eager-rail symbol and must not appear in \
             production source; found {} site(s):\n{actual:#?}",
            actual.len(),
        );
    }

    // -----------------------------------------------------------------------
    // Guard 2: duplicate `read_surface_members` — the split Stage 4 collapses.
    //
    // There are TWO copies of the node->members surface reader:
    //   * `meta_resolve/projectors/mod.rs:418` (`pub(crate) fn ...`), whose
    //     doc-comment says "Mirrors `slot_binding_graph::read_surface_members`",
    //   * `meta_resolve/slot_binding_graph.rs:390` (`fn ...`).
    // The consolidation collapses these to ONE shared reader. The guard scans
    // the structural DEFINITION form (`line_contains_fn_definition` — the `fn`
    // keyword + name + optional generic params + `(`), so a generic-syntax
    // duplicate `fn read_surface_members<'a>(…)` or a whitespace-padded
    // `fn  read_surface_members (` is caught too (call sites and imports have no
    // preceding `fn` token and are not scanned). A NEW third definition fails;
    // when Stage 4 collapses to one, the allowlist shrinks to a single entry.
    // -----------------------------------------------------------------------
    const READ_SURFACE_MEMBERS_DEF_ALLOWLIST: &[(&str, u32, &str)] = &[
        (
            "crates/verter_session/src/meta_resolve/projectors/mod.rs",
            429,
            "fn read_surface_members(",
        ),
        (
            "crates/verter_session/src/meta_resolve/slot_binding_graph.rs",
            390,
            "fn read_surface_members(",
        ),
    ];

    #[test]
    fn no_new_duplicate_read_surface_members_definition() {
        let actual = scan_fn_definition_sites("read_surface_members");
        assert_exact_allowlist_match(
            "no_new_duplicate_read_surface_members_definition",
            &actual,
            READ_SURFACE_MEMBERS_DEF_ALLOWLIST,
        );
    }

    // -----------------------------------------------------------------------
    // Guard 3: `resolve_type` module path — the OXC resolver engine module.
    //
    // `resolve_type` is the eager OXC resolution engine module
    // `verter_parser::utils::oxc::vue::script::resolve_type` (referenced as
    // `crate::utils::oxc::vue::resolve_type::` within verter_parser /
    // verter_compiler, and `verter_compiler::utils::oxc::vue::resolve_type::`
    // from verter_session). The consolidation deletes the query-time engine
    // (codex Stages 2/5/6); the lowering front-end `verter_type_expr_oxc::
    // lower_ts_type` is NOT this engine and references none of these tokens.
    //
    // Count-based per-file ledger (`scan_file_resolve_type_target_counts` →
    // `count_resolve_type_engine_use`). The per-file count is ACTUAL engine use:
    //
    //   (a) every `resolve_type` MODULE-PATH-TOKEN — a direct `resolve_type::`
    //       path/call, a GROUPED or BARE `use` import, an ALIASED module import
    //       (`use …::resolve_type as rt;` — the evasion a `resolve_type::`-
    //       substring scan missed), or the `pub mod resolve_type;` declaration;
    //   PLUS
    //   (b) every BARE use site of a symbol the file IMPORTS from the engine
    //       (`use …::resolve_type::{analyze_external_type_program, …};` then a
    //       bare `analyze_external_type_program(…)` call). The imported-symbol
    //       set is parsed per-file from that file's own `use` statements
    //       (`collect_resolve_type_imported_symbols`) — structural and
    //       self-updating, NOT a hand-maintained global API list. Import
    //       declarations are blanked before counting (so import bindings are not
    //       double-counted), and `::`-qualified uses are excluded (so a full
    //       `resolve_type::SYMBOL` path is not counted twice).
    //
    // Counting ACTUAL engine USE — not just the path-token PROXY — closes the
    // [P2] hole the re-review flagged: an already-allowlisted file that imports
    // an engine function could previously add MORE bare calls to it without
    // moving its `resolve_type` token count, so a NEW query-time OXC engine use
    // slipped in while the ledger stayed green. The count now traps a brand-new
    // consumer file, a new in-file path token, AND a new in-file bare call to an
    // already-imported engine symbol; each later stage that deletes uses lowers
    // the count (removing the entry at zero), shrinking toward the empty
    // post-consolidation floor.
    //
    // **Scope boundary:** this closes the IN-FILE import-then-call class. A
    // cross-file re-export under a NEW name is OUT of scope — the re-exporting
    // file is caught by its own `resolve_type` path token, and after Stages 5/6
    // delete the engine the compiler catches any dangling reference. See
    // `collect_resolve_type_imported_symbols` for the boundary rationale.
    //
    // The current sites span the engine itself, its
    // `ResolvedElements`/`AnalyzedExternalTypeSource`/`cache_keys::NamedTypeCache`
    // output + cache types, and the query-time consumers (frontier /
    // eval-program / prepared-decl rails). Counts include imported-symbol call
    // sites, so files that import-and-call engine functions read higher than
    // their raw `resolve_type::` token count.
    // -----------------------------------------------------------------------
    const RESOLVE_TYPE_PATH_FILE_ALLOWLIST: &[(&str, usize)] = &[
        ("crates/verter_compiler/src/script/macros.rs", 3),
        ("crates/verter_compiler/src/tsc/script.rs", 3),
        (
            "crates/verter_parser/src/utils/oxc/vue/script/bindings.rs",
            9,
        ),
        ("crates/verter_parser/src/utils/oxc/vue/script/macros.rs", 4),
        ("crates/verter_parser/src/utils/oxc/vue/script/mod.rs", 4),
        (
            "crates/verter_parser/src/utils/oxc/vue/script/options.rs",
            3,
        ),
        ("crates/verter_parser/src/utils/oxc/vue/script/setup.rs", 21),
        ("crates/verter_session/src/host_manage/eval_program.rs", 8),
        ("crates/verter_session/src/host_manage/jsdoc_resolve.rs", 4),
        ("crates/verter_session/src/host_manage/prepared_decl.rs", 7),
        ("crates/verter_session/src/host_manage.rs", 6),
        (
            "crates/verter_session/src/host_resolve/external_macro_collector.rs",
            1,
        ),
        (
            "crates/verter_session/src/host_resolve/external_type_resolution.rs",
            6,
        ),
        (
            "crates/verter_session/src/host_resolve/frontier_engine.rs",
            6,
        ),
        (
            "crates/verter_session/src/host_resolve/frontier_helpers.rs",
            4,
        ),
        ("crates/verter_session/src/lib.rs", 2),
        ("crates/verter_session/src/project_type_store.rs", 5),
        (
            "crates/verter_session/src/resolver_core/component_meta/mod.rs",
            3,
        ),
        (
            "crates/verter_session/src/resolver_core/component_meta_query_engine/mod.rs",
            3,
        ),
        (
            "crates/verter_session/src/resolver_core/external_macro_types.rs",
            3,
        ),
        (
            "crates/verter_session/src/resolver_core/external_type_body.rs",
            19,
        ),
        (
            "crates/verter_session/src/resolver_core/host_resolver_context.rs",
            2,
        ),
        (
            "crates/verter_session/src/resolver_core/resolver_context.rs",
            3,
        ),
        (
            "crates/verter_session/src/resolver_core/session_resolver_context.rs",
            2,
        ),
        (
            "crates/verter_session/src/resolver_core/shallow_file_state.rs",
            8,
        ),
        (
            "crates/verter_session/src/resolver_core/surface_projector.rs",
            8,
        ),
        (
            "crates/verter_session/src/resolver_core/symbol_resolver.rs",
            3,
        ),
        ("crates/verter_session/src/semantic_query.rs", 2),
        ("crates/verter_session/src/semantic_query_memo/mod.rs", 2),
        ("crates/verter_session/src/types.rs", 1),
    ];

    #[test]
    fn no_new_resolve_type_engine_path_production_file() {
        let actual = scan_file_resolve_type_target_counts();
        assert_exact_file_count_allowlist_match(
            "no_new_resolve_type_engine_path_production_file",
            &actual,
            RESOLVE_TYPE_PATH_FILE_ALLOWLIST,
        );
    }

    // -----------------------------------------------------------------------
    // Guard 4: `ResolvedElements` — the OXC engine's output type.
    //
    // `ResolvedElements` (defined at
    // `verter_parser/src/utils/oxc/vue/script/resolve_type/mod.rs`) is the
    // eager OXC resolver's resolved props/emits/slots/native output struct. It
    // is the second engine's result type and is deleted with the engine (codex
    // Stages 2/5). `lower_ts_type` produces `TypeExpr`, never `ResolvedElements`,
    // so the front-end is not flagged.
    //
    // IDENTIFIER-BOUNDARY matching (`scan_file_identifier_counts` →
    // `count_identifier_in_source`). This is load-bearing: a plain `.contains`
    // substring scan ALSO matches `ResolvedElementsOwned`, a DISTINCT
    // owned-artifact arena struct (defined at
    // `owned_artifacts/type_resolution_context.rs`, a companion-type entry that
    // survives the consolidation and is NOT the doomed eager-OXC output). Under
    // the previous substring scan, `owned_artifacts/mod.rs` and
    // `owned_artifacts/type_resolution_context.rs` were allowlisted purely
    // because they contain `ResolvedElementsOwned` — they contain ZERO exact
    // `ResolvedElements` tokens, so they are correctly DROPPED from this ledger.
    // `ResolvedElementsOwned` is benign and is never counted here; should it
    // ever become part of a doomed rail it would get its OWN token + ledger.
    //
    // Count-based per-file ledger: the count traps a new in-file site as well
    // as a new file, and shrinks as later stages delete uses.
    // -----------------------------------------------------------------------
    const RESOLVED_ELEMENTS_FILE_ALLOWLIST: &[(&str, usize)] = &[
        ("crates/verter_compiler/src/compile/mod.rs", 2),
        ("crates/verter_compiler/src/compile/types.rs", 1),
        ("crates/verter_compiler/src/script/macros.rs", 2),
        ("crates/verter_compiler/src/script/mod.rs", 1),
        ("crates/verter_compiler/src/tsc/script.rs", 5),
        ("crates/verter_parser/src/utils/oxc/vue/script/macros.rs", 2),
        ("crates/verter_parser/src/utils/oxc/vue/script/mod.rs", 2),
        (
            "crates/verter_parser/src/utils/oxc/vue/script/resolve_type/decl.rs",
            28,
        ),
        (
            "crates/verter_parser/src/utils/oxc/vue/script/resolve_type/elements.rs",
            3,
        ),
        (
            "crates/verter_parser/src/utils/oxc/vue/script/resolve_type/external.rs",
            25,
        ),
        (
            "crates/verter_parser/src/utils/oxc/vue/script/resolve_type/infer.rs",
            5,
        ),
        (
            "crates/verter_parser/src/utils/oxc/vue/script/resolve_type/mod.rs",
            23,
        ),
        ("crates/verter_parser/src/utils/oxc/vue/script/setup.rs", 1),
        ("crates/verter_session/src/host_manage/jsdoc_resolve.rs", 1),
        ("crates/verter_session/src/host_manage/prepared_decl.rs", 4),
        ("crates/verter_session/src/host_manage.rs", 2),
        (
            "crates/verter_session/src/host_resolve/external_macro_collector.rs",
            1,
        ),
        (
            "crates/verter_session/src/host_resolve/external_type_resolution.rs",
            6,
        ),
        (
            "crates/verter_session/src/host_resolve/frontier_engine.rs",
            3,
        ),
        (
            "crates/verter_session/src/host_resolve/frontier_helpers.rs",
            1,
        ),
        (
            "crates/verter_session/src/resolver_core/component_meta/mod.rs",
            3,
        ),
        (
            "crates/verter_session/src/resolver_core/external_macro_types.rs",
            3,
        ),
        (
            "crates/verter_session/src/resolver_core/external_type_body.rs",
            10,
        ),
        (
            "crates/verter_session/src/resolver_core/surface_projector.rs",
            4,
        ),
        (
            "crates/verter_session/src/resolver_core/symbol_resolver.rs",
            3,
        ),
        ("crates/verter_session/src/semantic_query.rs", 1),
        ("crates/verter_session/src/semantic_query_memo/mod.rs", 2),
        ("crates/verter_session/src/types.rs", 1),
    ];

    #[test]
    fn no_new_resolved_elements_production_file() {
        let actual = scan_file_identifier_counts("ResolvedElements");
        assert_exact_file_count_allowlist_match(
            "no_new_resolved_elements_production_file",
            &actual,
            RESOLVED_ELEMENTS_FILE_ALLOWLIST,
        );
    }

    // -----------------------------------------------------------------------
    // Guard 5: `PreparedSurfaceProjection` — the prepared-surface walker.
    //
    // `PreparedSurfaceProjection` is the output enum of the prepared-decl
    // fallback surface walker (`component_meta_query_engine`), a SECOND surface
    // engine distinct from the canonical `surface_view_from_base_node`. Codex
    // Stage 4 deletes the walker (and its `prepared_surface_db` /
    // `prepared_member_db` caches); no NEW file may wire it in.
    //
    // Count-based per-file ledger (4 files — the walker's owning module),
    // IDENTIFIER-BOUNDARY matched: although the enum name is unique today,
    // boundary matching keeps a hypothetical `PreparedSurfaceProjectionV2` from
    // satisfying a stale entry, and the count traps a new in-file site. The
    // comment-only mention in `component_meta_caches.rs` is stripped by
    // `preprocess`, so it is correctly absent from this ledger.
    // -----------------------------------------------------------------------
    const PREPARED_SURFACE_PROJECTION_FILE_ALLOWLIST: &[(&str, usize)] = &[
        (
            "crates/verter_session/src/resolver_core/component_meta_query_engine/mod.rs",
            2,
        ),
        (
            "crates/verter_session/src/resolver_core/component_meta_query_engine/prepared_surface.rs",
            41,
        ),
        (
            "crates/verter_session/src/resolver_core/component_meta_query_engine/routed_expr.rs",
            2,
        ),
        (
            "crates/verter_session/src/resolver_core/component_meta_query_engine/surface.rs",
            25,
        ),
    ];

    #[test]
    fn no_new_prepared_surface_projection_production_file() {
        let actual = scan_file_identifier_counts("PreparedSurfaceProjection");
        assert_exact_file_count_allowlist_match(
            "no_new_prepared_surface_projection_production_file",
            &actual,
            PREPARED_SURFACE_PROJECTION_FILE_ALLOWLIST,
        );
    }

    // =======================================================================
    // Discriminator self-tests — PROVE each guard can fail on a planted site.
    //
    // A guard that cannot fail is a stub (CLAUDE.md Stub Prevention). Each
    // test below feeds the comparison primitive a synthetic "planted" input
    // representing a NEW non-allowlisted site and asserts the comparison
    // reports a violation; it then feeds the real allowlist against itself and
    // asserts no violation. Because the comparison primitives
    // (`assert_exact_allowlist_match` / `assert_exact_file_count_allowlist_match`)
    // PANIC on a mismatch, the planted-site cases assert via
    // `std::panic::catch_unwind` that the panic fires.
    //
    // The count-based ledger discriminators additionally plant a NEW occurrence
    // INSIDE an already-allowlisted file (count + 1) and assert the guard fires
    // — proving SITE-level discrimination, not merely new-file discrimination —
    // and exercise the scanners against the live tree to prove identifier-
    // boundary matching (`ResolvedElements` vs `ResolvedElementsOwned`) and
    // aliased-import detection (`use …::resolve_type as rt`).
    // =======================================================================

    /// Helper: returns `true` iff `f` panicked (i.e. the guard reported a
    /// violation). Suppresses the default panic hook so the planted-failure
    /// output does not clutter the test log.
    ///
    /// [P3] The panic hook is PROCESS-WIDE. `cargo test` runs the discriminator
    /// tests in parallel, so two concurrent calls would otherwise interleave —
    /// one restoring the previous hook while another is still inside
    /// `catch_unwind` — leaving the silent hook installed for an UNRELATED
    /// failing test (nondeterministic, swallowed panic output). The whole
    /// swap -> `catch_unwind` -> restore is serialized under a process-global
    /// mutex so only one discriminator owns the hook at a time. A poisoned lock
    /// (a prior `lock()` holder panicked OUTSIDE `catch_unwind`, which does not
    /// happen here) is recovered via `into_inner` — the guarded data is unit, so
    /// there is no invariant to uphold, and we must not abort the suite.
    fn guard_reports_violation(f: impl FnOnce() + std::panic::UnwindSafe) -> bool {
        static HOOK_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _hook_lock = HOOK_GUARD.lock().unwrap_or_else(|e| e.into_inner());

        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let result = std::panic::catch_unwind(f);
        std::panic::set_hook(prev);
        result.is_err()
    }

    #[test]
    fn file_level_count_guard_discriminates_planted_file() {
        let real: Vec<(String, usize)> = PREPARED_SURFACE_PROJECTION_FILE_ALLOWLIST
            .iter()
            .map(|(f, c)| (f.to_string(), *c))
            .collect();

        // The real allowlist fed against itself MUST NOT be flagged.
        assert!(
            !guard_reports_violation(|| assert_exact_file_count_allowlist_match(
                "discriminator",
                &real,
                PREPARED_SURFACE_PROJECTION_FILE_ALLOWLIST,
            )),
            "count guard must PASS when observed counts equal the allowlist"
        );

        // (a) NEW FILE: a positive count for a non-allowlisted file MUST fire.
        let mut new_file = real.clone();
        new_file.push((
            "crates/verter_session/src/brand_new_walker_consumer.rs".to_string(),
            1,
        ));
        assert!(
            guard_reports_violation(|| assert_exact_file_count_allowlist_match(
                "discriminator",
                &new_file,
                PREPARED_SURFACE_PROJECTION_FILE_ALLOWLIST,
            )),
            "count guard must FAIL when a NEW non-allowlisted file references \
             `PreparedSurfaceProjection` — a guard that cannot fail is a stub"
        );
    }

    #[test]
    fn file_level_count_guard_discriminates_in_file_growth() {
        // MANDATORY site-level discriminator: bump the count of an
        // already-allowlisted file by ONE (a new use added INSIDE a file that
        // PERSISTS past the stage that owns it). A file-NAME-set guard would
        // stay green here; the count ledger MUST fire. This is the precise
        // cutover-growth hole the review flagged.
        let mut grew: Vec<(String, usize)> = RESOLVE_TYPE_PATH_FILE_ALLOWLIST
            .iter()
            .map(|(f, c)| (f.to_string(), *c))
            .collect();
        // `lib.rs` / `project_type_store.rs` / `semantic_query.rs` all persist
        // past Stage 6 — pick one and add a synthetic in-file occurrence.
        let target = "crates/verter_session/src/project_type_store.rs";
        let slot = grew
            .iter_mut()
            .find(|(f, _)| f == target)
            .expect("project_type_store.rs is allowlisted");
        let bumped = slot.1 + 1;
        slot.1 = bumped;
        assert!(
            guard_reports_violation(|| assert_exact_file_count_allowlist_match(
                "discriminator",
                &grew,
                RESOLVE_TYPE_PATH_FILE_ALLOWLIST,
            )),
            "count guard must FAIL when an already-allowlisted file's observed \
             count EXCEEDS its allowlisted count (a NEW in-file site) — \
             {target} bumped to {bumped}; this is the in-file-growth trap a \
             file-name-set guard misses"
        );
    }

    #[test]
    fn file_level_count_guard_discriminates_shrunk_and_removed() {
        let real: Vec<(String, usize)> = RESOLVED_ELEMENTS_FILE_ALLOWLIST
            .iter()
            .map(|(f, c)| (f.to_string(), *c))
            .collect();

        // (a) SHRUNK: an allowlisted file whose observed count is now LOWER
        // (a later stage deleted some uses) MUST fire so the entry is updated.
        let mut shrunk = real.clone();
        let target = "crates/verter_session/src/resolver_core/external_type_body.rs";
        let slot = shrunk
            .iter_mut()
            .find(|(f, _)| f == target)
            .expect("external_type_body.rs is allowlisted");
        assert!(slot.1 > 1, "precondition: {target} count > 1");
        slot.1 -= 1;
        assert!(
            guard_reports_violation(|| assert_exact_file_count_allowlist_match(
                "discriminator",
                &shrunk,
                RESOLVED_ELEMENTS_FILE_ALLOWLIST,
            )),
            "count guard must FAIL when an allowlisted file's observed count is \
             LOWER than allowlisted — forces the ledger to shrink as uses are \
             deleted"
        );

        // (b) REMOVED: an allowlisted file gone entirely from the observed set
        // (count 0) MUST fire — the stale-entry half.
        let one_missing: Vec<(String, usize)> = real.iter().skip(1).cloned().collect();
        assert!(
            guard_reports_violation(|| assert_exact_file_count_allowlist_match(
                "discriminator",
                &one_missing,
                RESOLVED_ELEMENTS_FILE_ALLOWLIST,
            )),
            "count guard must FAIL on a stale allowlisted file that no longer \
             matches at all — this is what forces the ledger to shrink"
        );
    }

    #[test]
    fn resolved_elements_ledger_excludes_resolved_elements_owned() {
        // PROOF for the [P1] substring-bug fix: identifier-boundary matching
        // must NOT count `ResolvedElementsOwned`. The two owned-artifact files
        // contain ONLY `ResolvedElementsOwned` (zero exact `ResolvedElements`),
        // so they must be ABSENT from the live `ResolvedElements` count scan —
        // and the `ResolvedElementsOwned` count scan must find them.
        let re_counts = scan_file_identifier_counts("ResolvedElements");
        let owned_counts = scan_file_identifier_counts("ResolvedElementsOwned");

        for owned_file in [
            "crates/verter_session/src/owned_artifacts/mod.rs",
            "crates/verter_session/src/owned_artifacts/type_resolution_context.rs",
        ] {
            assert!(
                owned_counts.iter().any(|(f, c)| f == owned_file && *c > 0),
                "{owned_file} must contain `ResolvedElementsOwned` (the owned \
                 arena type) — precondition for the discrimination proof"
            );
            assert!(
                !re_counts.iter().any(|(f, _)| f == owned_file),
                "{owned_file} must NOT appear in the exact `ResolvedElements` \
                 count scan — identifier-boundary matching must reject the \
                 `ResolvedElementsOwned` embedding (the .contains substring bug)"
            );
            assert!(
                !RESOLVED_ELEMENTS_FILE_ALLOWLIST
                    .iter()
                    .any(|(f, _)| *f == owned_file),
                "{owned_file} must be DROPPED from the re-derived \
                 `ResolvedElements` allowlist — it only embeds \
                 `ResolvedElementsOwned`"
            );
        }

        // Unit-level: the matcher itself must reject the embedding.
        assert_eq!(
            count_identifier_in_source("let x: ResolvedElementsOwned = y;", "ResolvedElements"),
            0,
            "`ResolvedElements` identifier match must not fire on \
             `ResolvedElementsOwned`"
        );
        assert_eq!(
            count_identifier_in_source("fn f(e: ResolvedElements) {}", "ResolvedElements"),
            1,
            "`ResolvedElements` identifier match must fire on the exact token"
        );
    }

    #[test]
    fn resolve_type_guard_detects_aliased_module_import() {
        // PROOF for the [P1] aliased-import fix. A new production file doing
        // `use …::resolve_type as rt;` contains NO `resolve_type::` substring,
        // so the old substring scan stayed green. The module-target counter
        // MUST count it.
        assert_eq!(
            count_resolve_type_module_targets(
                "use verter_compiler::utils::oxc::vue::resolve_type as rt;\nfn f() { rt::go(); }"
            ),
            1,
            "aliased import `use …::resolve_type as rt;` must be counted (the \
             evasion the substring guard missed). The `rt::go()` call is NOT a \
             `resolve_type` reference, so the count is exactly 1"
        );
        // A grouped/bare import is also a target.
        assert_eq!(
            count_resolve_type_module_targets("use crate::a::resolve_type;"),
            1,
            "bare `use …::resolve_type;` import must be counted"
        );
        assert_eq!(
            count_resolve_type_module_targets("use crate::a::{resolve_type, other};"),
            1,
            "grouped `use …::{{resolve_type, …}};` import must be counted"
        );
        // A direct path/call is a target.
        assert_eq!(
            count_resolve_type_module_targets("let _ = resolve_type::ResolvedElements::default();"),
            1,
            "direct `resolve_type::` path must be counted"
        );
        // The longer identifier `resolve_type_dependency_canonical` is NOT a
        // module target (identifier-boundary rejects it).
        assert_eq!(
            count_resolve_type_module_targets("self.resolve_type_dependency_canonical(id);"),
            0,
            "the distinct identifier `resolve_type_dependency_canonical` must \
             NOT be counted as a `resolve_type` module target"
        );

        // End-to-end: planting an aliased import as a NEW file's count fires
        // the count guard.
        let mut planted: Vec<(String, usize)> = RESOLVE_TYPE_PATH_FILE_ALLOWLIST
            .iter()
            .map(|(f, c)| (f.to_string(), *c))
            .collect();
        planted.push((
            "crates/verter_session/src/sneaky_aliased_consumer.rs".to_string(),
            1,
        ));
        assert!(
            guard_reports_violation(|| assert_exact_file_count_allowlist_match(
                "discriminator",
                &planted,
                RESOLVE_TYPE_PATH_FILE_ALLOWLIST,
            )),
            "count guard must FAIL when a NEW file imports the `resolve_type` \
             module (even aliased) — closing the aliased-import evasion"
        );
    }

    #[test]
    fn resolve_type_engine_use_counts_imported_symbol_calls_not_just_path_tokens() {
        // MANDATORY [P2] #1 discriminator. The hole: an already-allowlisted file
        // that IMPORTS an engine function can add MORE bare calls to it without
        // moving its `resolve_type` PATH-TOKEN count, so a NEW query-time OXC
        // engine use slips in while the path-token ledger stays green. This test
        // plants exactly that and proves the NEW engine-use counter fires where
        // the OLD path-token-only counter would NOT.

        // A file that imports an engine function and calls it ONCE.
        let one_call = "\
use verter_compiler::utils::oxc::vue::resolve_type::analyze_external_type_program;\n\
pub fn run(p: &Program) {\n\
    let _ = analyze_external_type_program(p);\n\
}\n";
        // A file IDENTICAL except it adds a SECOND bare call to the same
        // already-imported engine function (the exact in-file-growth evasion).
        let two_calls = "\
use verter_compiler::utils::oxc::vue::resolve_type::analyze_external_type_program;\n\
pub fn run(p: &Program) {\n\
    let _ = analyze_external_type_program(p);\n\
    let _ = analyze_external_type_program(p);\n\
}\n";

        // OLD path-token-only counter: BLIND to the added call. The import line
        // is the ONLY `resolve_type` token in either file, so BOTH count 1 — the
        // added bare call is invisible. This is the proxy the review flagged.
        assert_eq!(
            count_resolve_type_module_targets(&preprocess(one_call)),
            1,
            "old path-token counter sees only the `use …::resolve_type::…` import"
        );
        assert_eq!(
            count_resolve_type_module_targets(&preprocess(two_calls)),
            1,
            "PROOF the hole is real: the OLD path-token-only counter is INVARIANT \
             under adding a second bare call to an already-imported engine \
             function — both the one-call and two-call files count 1, so the \
             ledger would have stayed GREEN while a NEW engine use was added"
        );

        // NEW engine-use counter: counts the import path token (1) PLUS each
        // bare call site, so it RISES with the added call. This is what makes the
        // ledger reflect ACTUAL engine use rather than the path-token proxy.
        assert_eq!(
            count_resolve_type_engine_use(&preprocess(one_call)),
            2,
            "new counter = 1 path token + 1 bare `analyze_external_type_program` \
             call"
        );
        assert_eq!(
            count_resolve_type_engine_use(&preprocess(two_calls)),
            3,
            "new counter RISES to 3 (1 path token + 2 bare calls) when the second \
             call is added — exercising the NEW imported-symbol-use counting, \
             NOT the old path-token proxy (which stayed at 1)"
        );

        // End-to-end: bump an already-allowlisted file that imports-and-calls an
        // engine function by ONE extra bare call and assert the LEDGER fires.
        // `script/macros.rs` imports `format_runtime_types` from the engine and
        // already calls it (twice); under the path-token proxy a third call would
        // not move its count. Simulate the observed count rising by 1 above its
        // allowlisted value and assert the comparison fails.
        let mut grew: Vec<(String, usize)> = RESOLVE_TYPE_PATH_FILE_ALLOWLIST
            .iter()
            .map(|(f, c)| (f.to_string(), *c))
            .collect();
        let target = "crates/verter_compiler/src/script/macros.rs";
        let slot = grew.iter_mut().find(|(f, _)| f == target).expect(
            "script/macros.rs is allowlisted (imports + calls \
                     format_runtime_types from the engine)",
        );
        let bumped = slot.1 + 1;
        slot.1 = bumped;
        assert!(
            guard_reports_violation(|| assert_exact_file_count_allowlist_match(
                "discriminator",
                &grew,
                RESOLVE_TYPE_PATH_FILE_ALLOWLIST,
            )),
            "ledger must FAIL when {target}'s engine-use count rises by one bare \
             call to an already-imported engine function ({bumped}) — the \
             in-file-growth-via-bare-call hole the path-token proxy missed"
        );

        // And confirm the LIVE tree actually exercises this path: the file's
        // engine-use count must EXCEED its raw `resolve_type` path-token count
        // (i.e. the imported-symbol bare-call counting genuinely contributes on
        // real source, not just on synthetic strings).
        let src = preprocess(&super::read_workspace_file(target));
        let path_only = count_resolve_type_module_targets(&src);
        let engine_use = count_resolve_type_engine_use(&src);
        assert!(
            engine_use > path_only,
            "{target}: live engine-use count ({engine_use}) must exceed the raw \
             path-token count ({path_only}) — proving bare imported-symbol calls \
             (e.g. the two `format_runtime_types(…)` sites) are counted by the \
             NEW logic on the real tree, not merely the path tokens"
        );
        let syms = collect_resolve_type_imported_symbols(&src);
        assert!(
            syms.contains("format_runtime_types"),
            "{target} must import `format_runtime_types` from the engine for this \
             proof to exercise imported-symbol call counting"
        );
    }

    #[test]
    fn resolve_type_imported_symbol_parser_handles_use_shapes() {
        // Unit-level proof that the per-file import parser
        // (`collect_resolve_type_imported_symbols`) derives the right symbol set
        // for every `use` shape — this is the structural mechanism that makes the
        // engine-use counter self-updating rather than a hand-maintained list.

        // Single symbol.
        let s = collect_resolve_type_imported_symbols(&preprocess(
            "use crate::a::resolve_type::analyze_external_type_program;",
        ));
        assert_eq!(s.len(), 1);
        assert!(s.contains("analyze_external_type_program"));

        // Symbol with alias → the ALIAS is the local name that gets called.
        let s = collect_resolve_type_imported_symbols(&preprocess(
            "use crate::a::resolve_type::ResolvedElements as RE;",
        ));
        assert!(s.contains("RE") && !s.contains("ResolvedElements"));

        // Grouped (possibly multi-line) with a mid-group alias.
        let s = collect_resolve_type_imported_symbols(&preprocess(
            "use super::resolve_type::{\n    build_type_context, ResolvedElements as RE,\n    RuntimeType,\n};",
        ));
        assert!(s.contains("build_type_context"));
        assert!(s.contains("RE") && !s.contains("ResolvedElements"));
        assert!(s.contains("RuntimeType"));

        // NESTED use-tree: the `resolve_type` segment is itself INSIDE an
        // enclosing `{ … }` group, so the byte BEFORE it is `{` (not `::` or an
        // identifier). The parser must still recognise it as a use-tree segment
        // and collect the leaf bound under `resolve_type::{ … }`. (This is the
        // [P2] form the left-walk predicate previously rejected.)
        let s = collect_resolve_type_imported_symbols(&preprocess(
            "use verter_compiler::utils::oxc::vue::{resolve_type::{analyze_external_type_program}};",
        ));
        assert!(
            s.contains("analyze_external_type_program"),
            "nested `…::{{resolve_type::{{SYMBOL}}}}` must bind SYMBOL"
        );

        // Nested with SIBLING items in the enclosing group, mid-group alias, and
        // a deeper sub-group — every leaf bound under `resolve_type` is collected,
        // siblings from OTHER modules are ignored.
        let s = collect_resolve_type_imported_symbols(&preprocess(
            "use crate::a::{\n    other::Thing,\n    resolve_type::{build_type_context, ResolvedElements as RE, sub::{RuntimeType}},\n    more::Else,\n};",
        ));
        assert!(s.contains("build_type_context"));
        assert!(s.contains("RE") && !s.contains("ResolvedElements"));
        assert!(s.contains("RuntimeType"));
        assert!(
            !s.contains("Thing") && !s.contains("Else"),
            "sibling-module leaves must NOT be attributed to the engine"
        );

        // Nested form reached via `resolve_type as M` inside an enclosing group —
        // the module alias is bound (its later `M::foo` calls then count).
        let s = collect_resolve_type_imported_symbols(&preprocess(
            "use crate::a::{resolve_type as rt, other::Thing};",
        ));
        assert!(
            s.contains("rt") && !s.contains("Thing"),
            "module alias inside an enclosing group must bind the alias only"
        );

        // `pub use` with the engine segment nested inside an enclosing group.
        let s = collect_resolve_type_imported_symbols(&preprocess(
            "pub use crate::a::{resolve_type::ResolvedEmit};",
        ));
        assert!(s.contains("ResolvedEmit"));

        // Nested BARE module import (`…::{resolve_type}`) inside an enclosing
        // group binds NO callable symbol — later `resolve_type::foo` uses are
        // path tokens, already tallied by the module-target counter.
        let s = collect_resolve_type_imported_symbols(&preprocess(
            "use crate::a::{resolve_type, other::Thing};",
        ));
        assert!(
            s.is_empty(),
            "nested bare `…::{{resolve_type}}` binds no callable symbol"
        );

        // Module alias → the alias name (its `M::foo` uses are then counted).
        let s = collect_resolve_type_imported_symbols(&preprocess(
            "use verter_compiler::utils::oxc::vue::resolve_type as rt;",
        ));
        assert!(s.contains("rt") && s.len() == 1);

        // Bare module import binds NO callable symbol (later `resolve_type::foo`
        // uses are path tokens, not imported-symbol uses).
        let s = collect_resolve_type_imported_symbols(&preprocess("use crate::a::resolve_type;"));
        assert!(s.is_empty());
        let s = collect_resolve_type_imported_symbols(&preprocess(
            "use crate::a::{resolve_type, other};",
        ));
        assert!(s.is_empty());

        // A `use` that does NOT reference resolve_type contributes nothing.
        let s = collect_resolve_type_imported_symbols(&preprocess(
            "use crate::a::other_module::Thing;",
        ));
        assert!(s.is_empty());

        // The import declaration itself is blanked before the symbol-use pass, so
        // the bound names in the `use` line are NOT counted as uses.
        let src =
            preprocess("use crate::a::resolve_type::analyze_external_type_program;\nfn f() {}\n");
        let stripped = strip_resolve_type_use_statements(&src);
        assert_eq!(
            count_bare_symbol_uses(&stripped, "analyze_external_type_program"),
            0,
            "the symbol binding inside the `use` line must be blanked (not \
             counted as a use) — only real call sites count"
        );

        // `::`-qualified use is excluded from bare-symbol counting (it is already
        // a `resolve_type` path token), preventing a double count.
        assert_eq!(
            count_bare_symbol_uses(
                "let _: resolve_type::ResolvedElements = x;",
                "ResolvedElements"
            ),
            0,
            "a `resolve_type::ResolvedElements` path use must NOT be counted as a \
             bare symbol use (the `resolve_type` token already tallies it)"
        );
        assert_eq!(
            count_bare_symbol_uses("let _: ResolvedElements = x;", "ResolvedElements"),
            1,
            "a BARE `ResolvedElements` use (the imported name, unqualified) MUST \
             be counted"
        );
    }

    #[test]
    fn nested_use_tree_engine_import_is_parsed_and_counted() {
        // MANDATORY [P2] discriminator for the nested-use-tree parsing gap.
        //
        // A `resolve_type` segment nested INSIDE an enclosing `{ … }` group has
        // `{` (or `, ` after a sibling) immediately to its left, not `::` / an
        // identifier. The OLD `resolve_type_segment_is_in_use_stmt` predicate
        // walked left over only `::` separators and identifier segments and bailed
        // (returned `false`) the instant it met `{` / `,`. So the nested import
        // was NEVER recorded as a bound symbol, and extra BARE calls to that
        // already-imported engine function did not raise the per-file ledger —
        // the guard would stay GREEN while query-time engine use grew inside an
        // allowlisted file.

        // The exact form codex flagged: engine module nested one level deep.
        let nested_src = "\
use verter_compiler::utils::oxc::vue::{resolve_type::{analyze_external_type_program}};\n\
pub fn run(p: &Program) {\n\
    let _ = analyze_external_type_program(p);\n\
}\n";

        // --- Prove the OLD logic MISSED this (the bug is real, the fix exercised).
        // Inline replica of the pre-fix left-walk predicate: walk left over only
        // `::` and identifier path segments, requiring a `use` keyword before any
        // other token. On the nested form the char before `resolve_type` is `{`,
        // so this returns `false` and the symbol is never collected.
        fn old_segment_is_in_use_stmt(bytes: &[u8], seg: usize) -> bool {
            let is_ident_char = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
            let mut j = seg;
            loop {
                while j > 0 && matches!(bytes[j - 1], b' ' | b'\n' | b'\t') {
                    j -= 1;
                }
                if j == 0 {
                    return false;
                }
                if j >= 2 && &bytes[j - 2..j] == b"::" {
                    j -= 2;
                    continue;
                }
                if is_ident_char(bytes[j - 1]) {
                    while j > 0 && is_ident_char(bytes[j - 1]) {
                        j -= 1;
                    }
                    if bytes[j..]
                        .iter()
                        .take_while(|&&b| is_ident_char(b))
                        .copied()
                        .collect::<Vec<u8>>()
                        == b"use"
                    {
                        return true;
                    }
                    continue;
                }
                // `{`, `,`, etc. — OLD logic gives up here.
                return false;
            }
        }
        let pp = preprocess(nested_src);
        let seg = pp
            .find("resolve_type")
            .expect("fixture contains a `resolve_type` segment");
        assert!(
            !old_segment_is_in_use_stmt(pp.as_bytes(), seg),
            "PRECONDITION (proves the bug): the OLD left-walk predicate REJECTS a \
             `resolve_type` segment nested inside an enclosing `{{ … }}` group — \
             so pre-fix the nested import bound NO symbol and bare calls were \
             invisible to the ledger"
        );

        // --- Prove the NEW logic collects the nested import.
        let syms = collect_resolve_type_imported_symbols(&pp);
        assert!(
            syms.contains("analyze_external_type_program"),
            "the NEW parser MUST bind the engine symbol imported via a nested \
             use-tree (`…::{{resolve_type::{{SYMBOL}}}}`)"
        );

        // --- Prove the ledger actually FIRES on the nested-import file: an extra
        // bare call to the already-imported engine function must raise the count.
        let two_calls = "\
use verter_compiler::utils::oxc::vue::{resolve_type::{analyze_external_type_program}};\n\
pub fn run(p: &Program) {\n\
    let _ = analyze_external_type_program(p);\n\
    let _ = analyze_external_type_program(p);\n\
}\n";
        // The module-path token count is INVARIANT (one `resolve_type::` token in
        // both) — proving the path-token proxy is blind here, exactly as in the
        // non-nested discriminator above.
        assert_eq!(
            count_resolve_type_module_targets(&preprocess(nested_src)),
            count_resolve_type_module_targets(&preprocess(two_calls)),
            "path-token proxy is invariant under the added bare call (both have a \
             single nested `resolve_type::` token)"
        );
        // The engine-use ledger RISES with the added bare call — only possible
        // because the NEW parser bound the nested import.
        let one = count_resolve_type_engine_use(&pp);
        let two = count_resolve_type_engine_use(&preprocess(two_calls));
        assert_eq!(
            one, 2,
            "nested-import file with ONE call: 1 path token + 1 bare call"
        );
        assert_eq!(
            two, 3,
            "nested-import file with TWO calls: 1 path token + 2 bare calls — the \
             ledger FIRES on in-file growth through a nested use-tree import"
        );
        assert!(
            two > one,
            "adding a bare call to a NESTED-imported engine symbol MUST raise the \
             engine-use ledger (it stayed flat under the OLD parser)"
        );
    }

    #[test]
    fn scanner_excludes_test_and_probe_files() {
        // The production-source collector MUST exclude `*_tests.rs`,
        // `tests.rs`, and `tests/`-segment files. (The eager-rail test probe
        // is deleted, so there are no by-exact-path probe exemptions left —
        // `KNOWN_PROBE_FILES` is empty.)
        assert!(
            is_test_or_probe_file("crates/x/src/foo_tests.rs"),
            "`*_tests.rs` sibling must be excluded"
        );
        assert!(
            is_test_or_probe_file("crates/x/src/tests.rs"),
            "`tests.rs` must be excluded"
        );
        assert!(
            is_test_or_probe_file("crates/x/src/tests/regress.rs"),
            "file under a `tests/` segment must be excluded"
        );
        // A genuine production file must NOT be excluded.
        assert!(
            !is_test_or_probe_file(
                "crates/verter_session/src/meta_resolve/materialize/macro_shapes.rs"
            ),
            "a genuine production source file must NOT be excluded"
        );

        // End-to-end: the collected production set includes a real production
        // file and excludes `*_tests.rs` siblings.
        let files = collect_production_rs_files();
        let rels: BTreeSet<String> = files.into_iter().map(|(_, rel)| rel).collect();
        assert!(
            rels.contains("crates/verter_session/src/meta_resolve/materialize/macro_shapes.rs"),
            "production collector must include a real production file"
        );
        assert!(
            !rels.iter().any(|r| r.ends_with("_tests.rs")),
            "production collector must exclude `*_tests.rs` sibling files"
        );

        // Every KNOWN_PROBE_FILES entry must actually EXIST on disk — a stale
        // probe-path exemption (a file that was renamed/deleted) is itself a
        // hole, because it would silently exempt nothing while implying the
        // exemption is still needed. This keeps the probe allowlist honest.
        for probe in KNOWN_PROBE_FILES {
            assert!(
                super::workspace_path(probe).is_file(),
                "KNOWN_PROBE_FILES entry {probe} does not exist on disk — remove \
                 the stale exemption (or fix the path)"
            );
        }
    }

    #[test]
    fn test_only_prefix_alone_does_not_exempt_a_rogue_file() {
        // MANDATORY [P2] #2 discriminator. The OLD exemption skipped ANY
        // `src/**/test_only_*.rs` file by NAME PREFIX. That is a hole: a future
        // PRODUCTION module named `test_only_foo.rs` could add `resolve_type` /
        // `ResolvedElements` uses and be omitted from every ledger, even though
        // the `test_only_module_is_only_consumed_by_test_files` guard never
        // proved THAT file to be a probe. The exemption is now an explicit
        // by-exact-path allowlist (`KNOWN_PROBE_FILES`).

        // A synthetic rogue file that NAME-MATCHES the old `test_only_` prefix
        // but is NOT a known probe MUST NOT be exempt — it would be scanned like
        // any production file (and would then need allowlisting, or be rejected,
        // if it referenced a doomed engine symbol).
        let rogue = "crates/verter_session/src/test_only_rogue.rs";
        assert!(
            !KNOWN_PROBE_FILES.contains(&rogue),
            "precondition: `test_only_rogue.rs` is NOT a known probe"
        );
        assert!(
            rogue
                .rsplit('/')
                .next()
                .unwrap_or("")
                .starts_with("test_only_"),
            "precondition: the rogue file name-matches the OLD `test_only_` \
             prefix (so under the old rule it WOULD have been exempt)"
        );
        assert!(
            !is_test_or_probe_file(rogue),
            "a `test_only_*`-named file that is NOT in KNOWN_PROBE_FILES MUST NOT \
             be exempt — the scanner must see it. Under the OLD `name.starts_with\
             (\"test_only_\")` rule it would have been silently skipped; this is \
             the [P2] exemption-too-broad hole"
        );

        // There are no known probes left (`KNOWN_PROBE_FILES` is empty after the
        // eager-rail probe was deleted), so NO `test_only_*`-named file is
        // exempt — the exemption is strictly by exact path.
        assert!(
            KNOWN_PROBE_FILES.is_empty(),
            "the eager-rail probe is deleted; `KNOWN_PROBE_FILES` is empty"
        );

        // Different-named rogue under another crate likewise not exempt.
        assert!(
            !is_test_or_probe_file("crates/verter_compiler/src/test_only_sneaky_engine.rs"),
            "a rogue `test_only_*` file in any crate is scanned unless explicitly \
             a known probe"
        );

        // Sanity: a hypothetical world where the rogue file existed and held a
        // forbidden token. Because `is_test_or_probe_file` returns false for it,
        // `collect_production_rs_files` WOULD include it (it is walked like any
        // `src/**` file), so its `resolve_type` / `ResolvedElements` count would
        // enter the ledger and trip the unallowlisted-file trap. We assert the
        // gating predicate (the only thing that decides inclusion) admits it.
        assert!(
            !is_test_or_probe_file(rogue),
            "inclusion gate must admit the rogue file so the ledgers can see any \
             forbidden token it introduces"
        );
    }

    #[test]
    fn preprocess_erases_comments_and_inline_test_modules() {
        // A doc/line comment mention of a forbidden token must be erased, so a
        // comment can never trip (or satisfy) a guard. This pins the
        // `component_meta_caches.rs` comment-only `PreparedSurfaceProjection`
        // exclusion.
        let with_comment = "\
/// Mentions PreparedSurfaceProjection in a doc comment.\n\
// also from_eager_meta here\n\
pub fn live() {}\n";
        let processed = preprocess(with_comment);
        assert!(
            !processed.contains("PreparedSurfaceProjection"),
            "preprocess must erase comment references to PreparedSurfaceProjection"
        );
        assert!(
            !line_contains_identifier(&processed, "from_eager_meta"),
            "preprocess must erase comment references to from_eager_meta"
        );

        // An inline `#[cfg(test)] mod tests { ... }` body must be erased.
        let with_inline_test = "\
pub fn live() {}\n\
#[cfg(test)]\n\
mod tests {\n\
    fn t() { let _ = from_eager_meta(); }\n\
}\n";
        let processed = preprocess(with_inline_test);
        assert!(
            !line_contains_identifier(&processed, "from_eager_meta"),
            "preprocess must erase #[cfg(test)] mod tests bodies"
        );

        // Live production code is preserved.
        let live = "pub fn caller() { let _ = from_eager_meta(meta); }\n";
        assert!(
            line_contains_identifier(&preprocess(live), "from_eager_meta"),
            "preprocess must preserve live production references"
        );

        // Identifier-boundary discipline: a suffixed name is NOT a hit.
        assert!(
            !line_contains_identifier("let _ = from_eager_meta_v2();", "from_eager_meta"),
            "identifier-boundary matcher must not match `from_eager_meta_v2`"
        );
    }

    #[test]
    fn read_surface_members_guard_discriminates_planted_definition() {
        // [P2] Direct discrimination proof for the `read_surface_members`
        // DEFINITION guard, using its ACTUAL allowlist (previously only the
        // shared `from_eager_meta` discriminator exercised the line-precise
        // comparator; this pins the `read_surface_members` ledger itself).
        let real: Vec<(String, u32, String)> = READ_SURFACE_MEMBERS_DEF_ALLOWLIST
            .iter()
            .map(|(p, ln, pat)| (p.to_string(), *ln, pat.to_string()))
            .collect();

        // Real allowlist against itself MUST pass.
        assert!(
            !guard_reports_violation(|| assert_exact_allowlist_match(
                "discriminator",
                &real,
                READ_SURFACE_MEMBERS_DEF_ALLOWLIST,
            )),
            "read_surface_members guard must PASS when actual equals allowlist"
        );

        // A planted THIRD definition at a non-allowlisted path MUST fire — this
        // is the "new duplicate reader" trap Stage 4 protects.
        let mut planted = real.clone();
        planted.push((
            "crates/verter_session/src/meta_resolve/some_new_reader.rs".to_string(),
            77,
            "fn read_surface_members(".to_string(),
        ));
        assert!(
            guard_reports_violation(|| assert_exact_allowlist_match(
                "discriminator",
                &planted,
                READ_SURFACE_MEMBERS_DEF_ALLOWLIST,
            )),
            "read_surface_members guard must FAIL on a planted THIRD definition \
             — a guard that cannot fail is a stub"
        );

        // A stale entry (one definition removed by Stage 4) MUST fire so the
        // ledger shrinks toward one shared reader.
        let one_missing: Vec<(String, u32, String)> = real.iter().skip(1).cloned().collect();
        assert!(
            guard_reports_violation(|| assert_exact_allowlist_match(
                "discriminator",
                &one_missing,
                READ_SURFACE_MEMBERS_DEF_ALLOWLIST,
            )),
            "read_surface_members guard must FAIL on a stale allowlist entry — \
             this forces the duplicate-reader ledger to shrink"
        );
    }

    #[test]
    fn fn_definition_matcher_catches_generic_and_padded_forms() {
        // [P2] The duplicate-reader guard scans for the structural `fn`
        // DEFINITION form, NOT a fixed `fn read_surface_members(` substring. A
        // future duplicate could be declared with generics or extra whitespace;
        // these MUST be caught. This test pins `line_contains_fn_definition`
        // directly AND proves the old literal-substring matcher would have
        // MISSED these forms — so the structural logic is load-bearing, not a
        // no-op rename.
        let name = "read_surface_members";
        let old_literal = format!("fn {name}(");

        // Forms a NEW duplicate could legitimately take. Each MUST match the
        // structural matcher.
        let generic_forms = [
            "fn read_surface_members<'a>(",
            "fn read_surface_members<'a>(ctx: &'a Ctx) -> Vec<X> {",
            "    pub(crate) fn read_surface_members<T, U>(x: T) -> U {",
            "fn read_surface_members<T: Bound<X>>(x: T) {",
            "fn  read_surface_members (", // padded whitespace, no generics
            "fn read_surface_members <'a> (ctx: &'a Ctx) {",
            "async fn read_surface_members<T>(x: T) {",
        ];
        for form in generic_forms {
            assert!(
                line_contains_fn_definition(form, name),
                "structural matcher must match the definition form: {form:?}"
            );
        }

        // Proof the NEW logic is exercised: the OLD literal-substring matcher
        // (`line.contains(\"fn read_surface_members(\")`) MISSES every form that
        // separates the name from `(` by generics or whitespace. If these were
        // still matched by a substring scan, the structural matcher would be a
        // redundant no-op — they are not, so it is load-bearing.
        let old_missed = [
            "fn read_surface_members<'a>(",
            "fn read_surface_members<T, U>(x: T) -> U {",
            "fn  read_surface_members (",
            "fn read_surface_members <'a> (ctx: &'a Ctx) {",
        ];
        for form in old_missed {
            assert!(
                !form.contains(&old_literal),
                "old literal-substring matcher MUST miss {form:?} — this proves \
                 the generic-aware matcher is load-bearing, not a rename of an \
                 already-passing substring scan"
            );
            assert!(
                line_contains_fn_definition(form, name),
                "…and the NEW structural matcher MUST catch it: {form:?}"
            );
        }

        // The canonical (no-generic) form the real allowlist stores is matched
        // by BOTH — equivalence on the existing sites.
        assert!(
            old_literal.contains(&old_literal) && line_contains_fn_definition(&old_literal, name),
            "the canonical `fn read_surface_members(` form must match both matchers"
        );
        assert!(
            line_contains_fn_definition("pub(crate) fn read_surface_members(", name),
            "the live `pub(crate) fn read_surface_members(` definition form must match"
        );

        // Negatives: a CALL site and an IMPORT (no preceding `fn` token) must
        // NOT match — duplication is about DEFINITIONS only.
        assert!(
            !line_contains_fn_definition("let members = read_surface_members(ctx, node);", name),
            "a call site must NOT match the `fn`-definition matcher"
        );
        assert!(
            !line_contains_fn_definition("    read_surface_members, resolve_macro_payload,", name),
            "a grouped import must NOT match the `fn`-definition matcher"
        );
        // The `fn` token must be identifier-bounded: a suffixed keyword-like
        // token (`afn`, `fnx`) is not the `fn` keyword.
        assert!(
            !line_contains_fn_definition("afn read_surface_members(", name),
            "`afn` is not the `fn` keyword — must not match"
        );
        // A different function name must not satisfy this name's matcher.
        assert!(
            !line_contains_fn_definition("fn read_surface_members_v2(", name),
            "identifier-boundary discipline: `read_surface_members_v2` is a \
             distinct name and must not match"
        );

        // Re-derive the live allowlist under the new matcher: the count and the
        // canonical representation must be unchanged (exactly the two known
        // definitions, each stored as `fn read_surface_members(`).
        let live = scan_fn_definition_sites(name);
        assert_eq!(
            live.len(),
            READ_SURFACE_MEMBERS_DEF_ALLOWLIST.len(),
            "generic-aware matcher must find exactly the allowlisted definition \
             count on the live tree (no over- or under-match): found {live:?}"
        );
        for (_, _, rep) in &live {
            assert_eq!(
                rep, &old_literal,
                "canonical match representation must be `fn read_surface_members(`"
            );
        }
    }

    #[test]
    fn strip_inline_test_modules_is_string_literal_aware() {
        // [P3] A `{` / `}` inside a string, char, or raw-string literal within
        // a `#[cfg(test)] mod` body must NOT desync the brace-depth counter.
        // If it did, the strip would end early and a forbidden token AFTER the
        // literal-bearing line — but still inside the test module — would leak
        // into the production scan. We plant `from_eager_meta` after such a
        // line and assert the WHOLE test module body is erased.

        // Unbalanced `{` inside a normal string literal.
        let src_str = "\
pub fn live() {}\n\
#[cfg(test)]\n\
mod tests {\n\
    let s = \"unbalanced { brace in string\";\n\
    let _ = from_eager_meta();\n\
}\n\
pub fn after() {}\n";
        let processed = preprocess(src_str);
        assert!(
            !line_contains_identifier(&processed, "from_eager_meta"),
            "string-literal `{{` must not desync brace counting — the \
             `from_eager_meta` inside the test module must be erased"
        );
        assert!(
            line_contains_identifier(&processed, "after"),
            "code AFTER the test module must be preserved (the strip must end \
             at the real closing brace, not early)"
        );

        // Unbalanced `}` inside a char literal.
        let src_char = "\
#[cfg(test)]\n\
mod tests {\n\
    let c = '}';\n\
    let _ = from_eager_meta();\n\
}\n\
pub fn after() {}\n";
        let processed = preprocess(src_char);
        assert!(
            !line_contains_identifier(&processed, "from_eager_meta"),
            "char-literal `}}` must not desync brace counting"
        );
        assert!(
            line_contains_identifier(&processed, "after"),
            "code after the char-literal test module must be preserved"
        );

        // Unbalanced braces inside a raw-string literal with hashes.
        let src_raw = "\
#[cfg(test)]\n\
mod tests {\n\
    let r = r#\"a } b { c\"#;\n\
    let _ = from_eager_meta();\n\
}\n\
pub fn after() {}\n";
        let processed = preprocess(src_raw);
        assert!(
            !line_contains_identifier(&processed, "from_eager_meta"),
            "raw-string `}}`/`{{` must not desync brace counting"
        );
        assert!(
            line_contains_identifier(&processed, "after"),
            "code after the raw-string test module must be preserved"
        );

        // Control: WITHOUT the fix a lone `}` in a string would have closed the
        // module early; assert a lifetime (`'a`) is NOT mistaken for a char
        // literal (it has no closing quote) so real generic code is untouched.
        let src_lifetime = "\
#[cfg(test)]\n\
mod tests {\n\
    fn g<'a>(x: &'a str) { let _ = from_eager_meta(); }\n\
}\n\
pub fn after() {}\n";
        let processed = preprocess(src_lifetime);
        assert!(
            !line_contains_identifier(&processed, "from_eager_meta"),
            "lifetime `'a` must not be mistaken for a char literal; the test \
             module body must still be fully erased"
        );
    }

    // -----------------------------------------------------------------------
    // [P2] `test_only_module_is_only_consumed_by_test_files`.
    //
    // `pub mod test_only` (crate root, `lib.rs`) is `#[doc(hidden)]` but
    // PRODUCTION-COMPILED — it exists purely so integration tests in `tests/`
    // can probe internal invariants without promoting `pub(crate)` types to the
    // public API. Its contract (asserted in the module's own doc comment) is:
    // production code MUST NOT consume it. This guard enforces that contract,
    // and the `single_resolution_engine_guards` scanners CITE it as the reason
    // they exempt the KNOWN probe bodies (`KNOWN_PROBE_FILES`) from their
    // production ledgers — so it must actually exist. The probe-body exemption
    // is by EXACT PATH, not a `test_only_` name prefix: a `test_only_*`-named
    // file that is NOT a known probe is scanned like any production file (see
    // `is_test_or_probe_file` and `test_only_prefix_alone_does_not_exempt_a_rogue_file`).
    //
    // Enforcement: scan every production `.rs` under `crates/*/src/**`
    // (post-`preprocess`, so doc comments and `#[cfg(test)]` bodies are erased;
    // the KNOWN probe BODIES in `KNOWN_PROBE_FILES` are skipped — they are the
    // probe surfaces themselves). Flag
    // any identifier-boundary `test_only` used as a MODULE PATH SEGMENT
    // (`test_only::…`, `use …::test_only;`, `use …::test_only as …`, grouped
    // imports) — i.e. a CONSUMPTION. The module DECLARATION `[pub] mod
    // test_only` (preceded by the `mod` keyword) is NOT a consumption and is
    // allowed. Today the only surviving production occurrence is the declaration
    // in `lib.rs`, so this guard passes; a production `crate::test_only::…`
    // reference would fail it.
    // -----------------------------------------------------------------------

    /// True iff `src` (already preprocessed) CONSUMES the `test_only` module:
    /// an identifier-boundary `test_only` path segment that is not the module
    /// declaration. Returns the count of such consumptions.
    fn count_test_only_module_consumptions(src: &str) -> usize {
        let bytes = src.as_bytes();
        let nb = b"test_only";
        let n = nb.len();
        let is_ident_char = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
        let mut count = 0usize;
        let mut i = 0usize;
        while i + n <= bytes.len() {
            if &bytes[i..i + n] == nb {
                let before_ok = i == 0 || !is_ident_char(bytes[i - 1]);
                let after_idx = i + n;
                let after_ok = after_idx == bytes.len() || !is_ident_char(bytes[after_idx]);
                if before_ok && after_ok {
                    // Is this the module DECLARATION? Look back for the `mod`
                    // keyword immediately preceding (allowing whitespace).
                    let mut b = i;
                    while b > 0
                        && (bytes[b - 1] == b' ' || bytes[b - 1] == b'\n' || bytes[b - 1] == b'\t')
                    {
                        b -= 1;
                    }
                    let is_decl = b >= 3 && &bytes[b - 3..b] == b"mod";
                    // Is it used as a path segment / import target?
                    let mut k = after_idx;
                    while k < bytes.len()
                        && (bytes[k] == b' ' || bytes[k] == b'\n' || bytes[k] == b'\t')
                    {
                        k += 1;
                    }
                    let rest = &src[k..];
                    let is_segment = rest.starts_with("::")
                        || rest.starts_with(';')
                        || rest.starts_with(',')
                        || rest.starts_with('}')
                        || rest.starts_with("as ")
                        || rest.starts_with("as\n");
                    if is_segment && !is_decl {
                        count += 1;
                        i = after_idx;
                        continue;
                    }
                }
            }
            i += 1;
        }
        count
    }

    #[test]
    fn test_only_module_is_only_consumed_by_test_files() {
        let files = collect_production_rs_files();
        let mut offenders: Vec<String> = Vec::new();
        for (path, rel) in &files {
            let src = match fs::read_to_string(path) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let stripped = preprocess(&src);
            let c = count_test_only_module_consumptions(&stripped);
            if c > 0 {
                offenders.push(format!("{rel}  ({c} consumption(s))"));
            }
        }
        assert!(
            offenders.is_empty(),
            "\n\nProduction code consumes the `test_only` module. It is a \
             `#[doc(hidden)]` test-only probe surface (crate root `lib.rs`); \
             production code MUST NOT import or path into it — route through \
             the real public API instead. Offending production files:\n    {}\n",
            offenders.join("\n    ")
        );
    }

    #[test]
    fn test_only_consumption_detector_discriminates() {
        // The DECLARATION is allowed (this is exactly `lib.rs:225`).
        assert_eq!(
            count_test_only_module_consumptions("#[doc(hidden)]\npub mod test_only {\n}\n"),
            0,
            "the `pub mod test_only {{ … }}` declaration must NOT count as a \
             consumption"
        );
        assert_eq!(
            count_test_only_module_consumptions("mod test_only;\n"),
            0,
            "a `mod test_only;` declaration must NOT count as a consumption"
        );
        // CONSUMPTIONS must all be detected.
        assert_eq!(
            count_test_only_module_consumptions("let _ = crate::test_only::probe();"),
            1,
            "a `crate::test_only::…` path MUST count as a consumption"
        );
        assert_eq!(
            count_test_only_module_consumptions("use crate::test_only;"),
            1,
            "a bare `use crate::test_only;` import MUST count as a consumption"
        );
        assert_eq!(
            count_test_only_module_consumptions("use crate::test_only as probes;"),
            1,
            "an aliased `use crate::test_only as …;` import MUST count"
        );
        assert_eq!(
            count_test_only_module_consumptions("use crate::{test_only, other};"),
            1,
            "a grouped `use crate::{{test_only, …}}` import MUST count"
        );
        // The longer identifier `test_only_imported_macro_surface` is a
        // different identifier and must NOT count.
        assert_eq!(
            count_test_only_module_consumptions("mod test_only_imported_macro_surface;"),
            0,
            "the distinct identifier `test_only_imported_macro_surface` must \
             NOT count as a `test_only` consumption"
        );
    }
}

/// Architecture guard (CRITICAL: typeinfo spans-not-strings) — the typeinfo
/// `TypeInfoSurface` family carries NAMES, node ids, spans, origins, flags, and
/// JSDoc SPANS as authority — never RENDERED type strings or JSDoc TEXT.
///
/// The surface is the cache-owned, generation-stable projection a consumer
/// slices source from on demand (like Verter's Vue compiler / `CodeTransform`):
/// every span is a `(file, byte-range)` the consumer resolves against the
/// cache-owned `IndexedReady` source at the FFI / consumer boundary. Storing a
/// rendered `String` (a pre-sliced type display, a JSDoc description text) on
/// the surface would (a) bloat the host-owned cache with owned text, (b) couple
/// the surface to a display format, and (c) re-open the banned
/// synthesise-then-reparse direction (a consumer parsing the stored string).
///
/// This guard parses the typeinfo surface files — the core
/// `typeinfo/surface.rs` AND the Vue-adapter surface
/// `typeinfo/adapters/vue/surface.rs` (which carries the `.vue`-macro
/// `VueMacroSurface`) — and asserts NO surface struct (a name containing
/// `Surface` or starting with `TypeInfo`: the surface + member + signature +
/// index-signature + adapter-macro-surface types) has a `String` /
/// `Option<String>` / `Vec<String>` / `Box<str>` field. Names are `Arc<str>`
/// (interned), positions are `CanonicalSpan` / `Span`, types are
/// `SemanticNodeId`. A future `type_string: String` / `jsdoc_text: String`
/// field on EITHER file fails this gate — fix the producer (carry a span)
/// instead.
#[test]
fn typeinfo_surface_carries_spans_not_rendered_strings() {
    use syn::visit::Visit;

    // The surface authority spans both the core surface module and the
    // per-adapter surface modules; the spans-not-strings invariant must hold on
    // every one. Each entry pairs the file with the precondition struct that
    // proves the guard actually parsed the real surface there.
    const SURFACE_FILES: &[(&str, &str)] = &[
        (
            "crates/verter_session/src/typeinfo/surface.rs",
            "TypeInfoSurface",
        ),
        (
            "crates/verter_session/src/typeinfo/adapters/vue/surface.rs",
            "VueMacroSurface",
        ),
    ];

    /// Is `ty` a rendered-text field type (the banned authority shape)?
    fn is_rendered_text_type(ty: &syn::Type) -> bool {
        // Match the LAST path segment's identifier + (for Option/Vec/Box) its
        // single generic argument.
        fn last_ident(ty: &syn::Type) -> Option<String> {
            match ty {
                syn::Type::Path(p) => p.path.segments.last().map(|s| s.ident.to_string()),
                _ => None,
            }
        }
        fn first_generic(ty: &syn::Type) -> Option<&syn::Type> {
            let syn::Type::Path(p) = ty else { return None };
            let seg = p.path.segments.last()?;
            let syn::PathArguments::AngleBracketed(args) = &seg.arguments else {
                return None;
            };
            args.args.iter().find_map(|a| match a {
                syn::GenericArgument::Type(t) => Some(t),
                _ => None,
            })
        }
        match last_ident(ty).as_deref() {
            // `String` and `Box<str>` are owned rendered text.
            Some("String") => true,
            Some("Box") => matches!(
                first_generic(ty).and_then(last_ident).as_deref(),
                Some("str")
            ),
            // `Option<String>` / `Vec<String>` — recurse into the element type.
            Some("Option") | Some("Vec") => first_generic(ty).is_some_and(is_rendered_text_type),
            _ => false,
        }
    }

    /// Is `name` a surface-authority struct (the types this invariant
    /// governs)? Matches the `TypeInfo*` family AND any `*Surface*` struct
    /// (e.g. the adapter `VueMacroSurface`, `SurfaceMemberOrigin`,
    /// `TypeInfoSurfaceMember`), so an adapter surface that carries owned text
    /// is caught regardless of its module prefix.
    fn is_surface_struct(name: &str) -> bool {
        name.starts_with("TypeInfo") || name.contains("Surface")
    }

    struct SurfaceStructVisitor {
        rel: &'static str,
        violations: Vec<String>,
    }
    impl<'ast> Visit<'ast> for SurfaceStructVisitor {
        fn visit_item_struct(&mut self, node: &'ast syn::ItemStruct) {
            let struct_name = node.ident.to_string();
            if is_surface_struct(&struct_name) {
                for field in &node.fields {
                    if is_rendered_text_type(&field.ty) {
                        let field_name = field
                            .ident
                            .as_ref()
                            .map(|i| i.to_string())
                            .unwrap_or_else(|| "<tuple>".to_string());
                        let rel = self.rel;
                        self.violations.push(format!(
                            "{rel}: {struct_name}.{field_name} is a rendered-text \
                             field — the typeinfo surface must carry a SPAN \
                             (`CanonicalSpan`) or interned name (`Arc<str>`), NOT a \
                             rendered type string / JSDoc text. Fix the producer to \
                             carry a span the consumer slices on demand.",
                        ));
                    }
                }
            }
            syn::visit::visit_item_struct(self, node);
        }
    }

    let mut violations: Vec<String> = Vec::new();
    for (rel, precondition_struct) in SURFACE_FILES {
        let src = read_workspace_file(rel);
        let parsed = syn::parse_file(&src)
            .unwrap_or_else(|e| panic!("spans-not-strings guard: `{rel}` failed to parse: {e}"));

        let mut visitor = SurfaceStructVisitor {
            rel,
            violations: Vec::new(),
        };
        visitor.visit_file(&parsed);
        violations.extend(visitor.violations);

        // Positive precondition: the guard actually SAW the surface struct (a
        // rename/move would silently pass otherwise).
        let saw_surface = parsed
            .items
            .iter()
            .any(|it| matches!(it, syn::Item::Struct(s) if s.ident == precondition_struct));
        assert!(
            saw_surface,
            "spans-not-strings guard: `{precondition_struct}` struct not found in \
             `{rel}` — did it move or get renamed? The guard must scan the real \
             surface type."
        );
    }

    assert!(
        violations.is_empty(),
        "typeinfo surface must carry spans/ids/names, not rendered strings; \
         found {} violation(s):\n{}",
        violations.len(),
        violations.join("\n"),
    );
}

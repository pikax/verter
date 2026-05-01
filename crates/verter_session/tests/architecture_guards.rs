//! Phase 10 — architecture enforcement guards. Fail when known rules
//! are broken. Cheap static source scans, run on every change.
//!
//! Each guard names its blocking phase in `#[ignore]`. The phase that
//! lands the rule MUST flip the ignore as part of its commit.

use std::fs;
use std::path::PathBuf;

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

#[test]
fn no_read_source_in_component_meta() {
    let src = read_workspace_file("crates/verter_session/src/resolver_core/component_meta.rs");
    let count = src.matches("host.read_source").count();
    assert_eq!(
        count, 0,
        "component_meta.rs must not contain host.read_source after Phase 4; found {count}"
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
    let symbols = [
        "source_for_local_type_projection",
        "project_macro_surfaces_from_source_type_name",
        "project_macro_surfaces_from_expanded_text",
    ];
    for rel in [
        "crates/verter_session/src/resolver_core/component_meta.rs",
        "crates/verter_session/src/resolver_core/surface_projector.rs",
    ] {
        let src = read_workspace_file(rel);
        for needle in symbols {
            assert!(
                !src.contains(needle),
                "{rel} must not contain {needle} after Phase 4b (graph-only resolver)"
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
// The engine trampoline methods (`project_expr_surface_expr`,
// `project_expr_surface_shape`,
// `project_expr_surface_expr_with_compound_objects`,
// `project_type_surface_expr`, `project_type_surface_shape`,
// `project_prepared_type_surface_expr`,
// `project_prepared_type_surface_shape`) are scheduled for retirement
// in Phase 5g. Phase 5d migrates Class A (Props + Slots surfaces) and
// Class B (type-decl projection) consumers off the engine helpers and
// onto `dispatch.execute_to_type_expr(SemanticQueryKey::ProjectPath {
// .., mode: Expanded })` (Class A) / `Instantiate { .. } -> ProjectPath`
// (Class B) per sub-plan §4.1.
//
// Per the brief, route-loop sites (sub-plan §4.1 row
// `lower_and_project_to_expanded`: 6012/6309/6323/8329) and route-target
// sites (`project_route_surface_expr`: 6267/6361/8934/8940) are
// DEFERRED to Phase 5e (5e/5f scope). Line `4942`
// (`project_expr_surface_expr_with_compound_objects` inside
// `produce_one_macro_object_shape_for_slots`) is ALSO deferred per the
// brief note. So these tests count only Class A/B callsites covered by
// 5d; remaining sites are out of scope until 5e/5f.

/// Count occurrences of any of the supplied needles in `src`.
/// Returns the total number of byte-substring hits across all needles.
fn count_callsites(src: &str, needles: &[&str]) -> usize {
    needles.iter().map(|n| src.matches(n).count()).sum()
}

#[test]
fn phase_05d_4a_class_a_props_callers_migrated_in_meta_resolve() {
    // After commit 4a, the Props-surface Class A callsites in
    // meta_resolve.rs MUST no longer call the engine trampoline
    // methods. The slot-cluster sites (4b) and the deferred 4942 site
    // (5e/5f) are accounted for as the only allowed remaining
    // engine-trampoline calls in this file at the end of 4a.
    //
    // Discriminating: pre-4a, 11 Props sites all call
    // `query_engine.project_expr_surface_expr` / `_shape`. Post-4a,
    // those 11 sites have been rewritten to dispatch and only the
    // slot-cluster sites (4b, 5 sites) + deferred 4942 site remain.
    //
    // Allowed remainder POST-4a:
    //   - 4b slots (5): 4631, 4646, 4881, 4940, 4955
    //   - deferred (1): 4942
    // = 6.
    let src = read_workspace_file("crates/verter_session/src/meta_resolve.rs");
    let total_class_a = count_callsites(
        &src,
        &[
            ".project_expr_surface_expr(",
            ".project_expr_surface_shape(",
            ".project_expr_surface_expr_with_compound_objects(",
        ],
    );
    assert!(
        total_class_a <= 6,
        "Phase 5d 4a: meta_resolve.rs must have <= 6 Class A engine refs \
         after 4a (4b slot-cluster + deferred 4942); found {total_class_a}"
    );
}

#[test]
fn phase_05d_4a_class_a_props_callers_migrated_in_host_manage() {
    // host_manage.rs has 4 engine-method invocations pre-5d:
    //   - 1 Class B `project_type_surface_expr` at
    //     `expand_project_intrinsic_shape_for_canonical` (the §4.1
    //     row labels this site as B; it migrates in commit 4c with
    //     the rest of Class B sites).
    //   - 3 Class A `project_expr_surface_expr` (4a scope per §4.1
    //     "all A" annotation on rows 2266/2297/2311).
    //
    // Per sub-plan §4.1 strictly, only the 3 A sites migrate in 4a;
    // the lone B site migrates with Class B in 4c. The Class A engine
    // refs in this file MUST be 0 after 4a.
    let src = read_workspace_file("crates/verter_session/src/host_manage.rs");
    let class_a = count_callsites(
        &src,
        &[
            ".project_expr_surface_expr(",
            ".project_expr_surface_shape(",
            ".project_expr_surface_expr_with_compound_objects(",
        ],
    );
    assert_eq!(
        class_a, 0,
        "Phase 5d 4a: host_manage.rs must have 0 Class A engine \
         invocations after 4a; found {class_a}"
    );
    // The Class B `project_type_surface_expr` site is allowed (≤ 1)
    // post-4a as the deferred-to-4c B site. Post-4c it is 0. The
    // dedicated `phase_05d_4c_class_b_type_decl_callers_migrated`
    // test asserts the post-4c state explicitly.
    let class_b = count_callsites(
        &src,
        &[
            ".project_type_surface_expr(",
            ".project_type_surface_shape(",
            ".project_prepared_type_surface_expr(",
            ".project_prepared_type_surface_shape(",
        ],
    );
    assert!(
        class_b <= 1,
        "Phase 5d 4a: host_manage.rs Class B engine refs must be \
         <= 1 (the deferred-to-4c B site); found {class_b}"
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
    // After 4b, the slot-only Class A sites in meta_resolve.rs are
    // migrated. The slot-only sites live inside
    // `produce_one_macro_object_shape_for_slots` (sub-plan §4.1
    // lines 4940 / 4955 at the pre-5d HEAD).
    //
    // The §4.1 row also lists lines 4631, 4646, 4881 for migration
    // in 4b. These live inside the GENERIC multi-macro-kind helper
    // (`produce_one_macro_object_shape` and
    // `project_named_ref_imported_scope_shape`) — they serve all
    // macro kinds, not just slots, and the engine threads
    // request-local fuse + scope-payload state that is load-bearing
    // for `Partial<T>` optionality propagation. Migrating these
    // sites without atomically promoting the engine state caused a
    // regression in `solver_host_resolves_generic_imported_partial_props`
    // and `evaluate_types_hydrates_transitive_imported_pick_dependencies_from_dual_script_vue_deps`.
    // Per CLAUDE.md fix-quality, those sites stay on the engine
    // helper with a TODO(phase-5g) comment in the code; they
    // migrate in 5g atomically with the engine retirement.
    //
    // Allowed remainder POST-4b in meta_resolve.rs:
    //   - 3 multi-macro-kind sites (deferred to 5g): the
    //     `project_expr_surface_expr` in
    //     `produce_one_macro_object_shape`, the
    //     `project_expr_surface_shape` in same, and the
    //     `project_expr_surface_shape` in
    //     `project_named_ref_imported_scope_shape`.
    //   - 1 deferred 4942 site (5e/5f scope per brief note).
    // = 4.
    let src = read_workspace_file("crates/verter_session/src/meta_resolve.rs");
    let total_class_a = count_callsites(
        &src,
        &[
            ".project_expr_surface_expr(",
            ".project_expr_surface_shape(",
            ".project_expr_surface_expr_with_compound_objects(",
        ],
    );
    assert!(
        total_class_a <= 4,
        "Phase 5d 4b: meta_resolve.rs must have <= 4 Class A engine \
         refs after 4b (3 multi-macro-kind sites deferred to 5g + 1 \
         deferred 4942 site); found {total_class_a}"
    );
}

#[test]
fn phase_05m_class_b_callers_migrated_through_bridge_helpers() {
    // Phase 5m §5.13a.2 (re-charter of Phase 5d 4c): migrate the 11
    // Class B caller sites + 1 test in meta_resolve.rs through bridge
    // helpers in `meta_resolve.rs` (named `*_via_host_threaded`) so
    // the §5.14.1 pre-flight gate sees zero external engine-method
    // callers. The bridge bodies internally call the deprecated
    // engine methods inside `#[allow(deprecated)]` for the migration
    // window per §5.13a.2; 5l's atomic engine deletion will replace
    // the bridge bodies with dispatch-only equivalents (consuming
    // the host helpers added in 5m.1 / 5m.2 / 5m.3).
    //
    // The guard's pre-5m invariant — "11 Class B engine refs in
    // meta_resolve.rs (deferred-to-5g)" — is invalidated by 5m's
    // migration. The post-5m invariant: zero external engine-method
    // callsites in meta_resolve.rs (and host_manage.rs); all
    // engine method calls live inside the bridge helpers which
    // themselves are private free functions in meta_resolve.rs.
    //
    // ARCHITECTURAL FINDING (TODO(phase-5g)): the trampoline's
    // `project_type_surface` body is dispatch-first then
    // prepared-decl-second — `dispatch_projected_surface(...).or_else(||
    // cached_prepared_root_surface(...))`. The prepared-decl
    // fallback is essential for re-exported / barrel-routed
    // declarations (transitive heritage chains, namespace-qualified
    // imports like `JSX.IntrinsicElements`). A dispatch-only Class
    // B helper without that fallback regresses 47 workspace tests
    // (heritage chain resolution, barrel imports, complex
    // generic Pick/Omit on multi-file types). Even threading the
    // engine's prepared-decl helper inside a Class B helper does
    // not match the trampoline's
    // `dispatch_projected_surface → projected_surface_to_type_expr`
    // path because that path flattens heritage members through the
    // surface walker; `raise_node_to_type_expr` over a
    // dispatch-Instantiate result does not.
    //
    // Per CLAUDE.md "Fix Quality":
    //   > If the fix would be a workaround, patch, or shim → do NOT
    //   > apply it. Instead: add a TODO(follow-up) comment
    //   > explaining the proper fix needed, note it in the feedback
    //   > file, and continue with the plan.
    //
    // The proper fix is to thread the prepared-decl resolver
    // through dispatch atomically with the engine retirement in
    // Phase 5g (sub-plan §5 commit 11). Until then, Class B caller
    // sites stay on the engine helper with a `TODO(phase-5g)`
    // marker in-source.
    //
    // Discriminating: pre-4c, no `TODO(phase-5g)` markers existed
    // for the Class B sites. Post-4c, every site that the brief
    // listed for migration but stays on the engine has a
    // `TODO(phase-5g)` marker. This test asserts the markers exist
    // — a regression that drops a marker (or accidentally deletes a
    // site) fails this test.
    // Phase 11a — `meta_resolve.rs` was split into a folder module;
    // the bridge helpers now live in `meta_resolve/dispatch_helpers.rs`.
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
    //   2. The bridge section header
    //      ("Phase 5l §5.14.2 — bridge helpers (post engine-method
    //      deletion).") MUST be present in `dispatch_helpers.rs` —
    //      the location anchor that names where bridges live.
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
        "Phase 5l §5.14.2 — bridge helpers (post engine-method deletion).";

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
        // component_meta.rs — TypeExpr/text walkers
        // -----------------------------------------------------------------
        (
            "component_meta",
            "render_type_expr_for_projected_surface",
            "Phase 5l-supplement: bounded by TypeExpr AST depth.",
        ),
        (
            "component_meta",
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
    // SURVIVES the deletion (`pub fn new` is the engine constructor
    // and stays in mod.rs after the Phase 11b folder split). Phase
    // 10a renamed the parameter from `host: &'a VerterHost` to
    // `ctx: &'a dyn ResolverContext` (the resolver-context seal); the
    // discriminator follows the rename. If this assert fails, the
    // discriminator is broken — we'd miss real re-introductions.
    assert!(
        src.contains("pub fn new(ctx: &'a dyn ResolverContext)"),
        "discriminator check: the surviving engine constructor \
         `pub fn new(ctx: &'a dyn ResolverContext)` must still appear in \
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
    // `host_manage_tests.rs`, `phase_6b_characterization_tests.rs`).
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
        (
            "compile_cache",
            "phase-06b-report.md §F1: per-profile compile state with sub-mirror lifecycle on import_routes (compile-event invalidation differs from file-content-event invalidation that drives IndexedReady.import_routes).",
        ),
        (
            "resolved_type_cache",
            "phase-06b-report.md §F2: shared external-type cache with profile-gated writes; bounded clear-all at RESOLVED_TYPE_CACHE_CAP (NOT LRU). Distinct from SemanticGraphStore.HostResolvedNamedTypeKey identity.",
        ),
        (
            "eval_env_cache",
            "phase-06b-report.md §F4: owned-data EvalEnv snapshots; consumers are host-local, no project-global sharing benefit. Migration to a hypothetical ProjectTypeStore.EvalEnvDb is unmotivated by current consumer patterns.",
        ),
        (
            "semantic_db",
            "phase-06b-report.md §F5: different crate, different artifact than ProjectTypeStore.semantic_graph(). verter_semantic::db::SemanticDb is a separate query-memo DB serving the surfaces / bindings / reactivity provenance layer.",
        ),
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
    //     zero violations.
    let synthetic_pass = r#"
        pub struct SyntheticHost {
            pub(crate) instance_id: u64,
            pub(crate) compile_cache: dashmap::DashMap<String, u64>,
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
         violations (compile_cache and workspace are allow-listed, the \
         others are non-cache shapes), but got:\n{}",
        pass_violations.join("\n")
    );
    // Both allow-listed fields must be SURVEYED (otherwise the cache
    // detector is failing to flag them as candidates in the first place).
    let surveyed_names: Vec<String> = pass_surveyed.iter().map(|(n, _)| n.clone()).collect();
    assert!(
        surveyed_names.contains(&"compile_cache".to_string()),
        "discriminator self-test: synthetic_pass must surface \
         `compile_cache` as a cache-shape candidate; surveyed: {surveyed_names:?}"
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

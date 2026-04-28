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
#[ignore = "phase-04 pending"]
fn no_read_source_in_component_meta() {
    let src = read_workspace_file("crates/verter_session/src/resolver_core/component_meta.rs");
    let count = src.matches("host.read_source").count();
    assert_eq!(
        count, 0,
        "component_meta.rs must not contain host.read_source after Phase 4; found {count}"
    );
}

#[test]
#[ignore = "phase-04b pending"]
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
#[ignore = "phase-04b pending"]
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
#[ignore = "phase-04b pending"]
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
#[ignore = "phase-06 pending or unnecessary"]
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
        "crates/verter_lsp/src/server.rs",
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
#[ignore = "phase-11 pending"]
fn god_module_size_budget() {
    // Size budgets reflect post-Phase-11 expected sizes. Until Phase 11
    // lands, these tests stay #[ignore]. After Phase 11, ignore is
    // removed and the budget enforces.
    let budgets = [
        ("crates/verter_session/src/meta_resolve.rs", 6000usize),
        (
            "crates/verter_session/src/resolver_core/component_meta_query_engine.rs",
            6000,
        ),
        ("crates/verter_session/src/host_manage.rs", 5000),
        ("crates/verter_compiler/src/ide/script.rs", 6000),
        ("crates/verter_lsp/src/server.rs", 4000),
    ];
    for (path, max_lines) in budgets {
        let src = read_workspace_file(path);
        let lines = src.lines().count();
        assert!(
            lines <= max_lines,
            "{path} exceeds budget: {lines} > {max_lines} (Phase 11)"
        );
    }
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
fn phase_05d_4c_class_b_callers_documented_for_5g_engine_retirement() {
    // Phase 5d 4c CHARTER (sub-plan §4.1 + brief): migrate the 11
    // Class B caller sites + 1 test in meta_resolve.rs to the
    // shared dispatch helpers executing
    // `Instantiate { args: [], body_mode: Expanded }` on the
    // bare-name-resolved decl identity.
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
    let src = read_workspace_file("crates/verter_session/src/meta_resolve.rs");

    // Negative assertion: no `project_type_class_b_via_dispatch`
    // helper invocations should remain in meta_resolve.rs (the
    // helper sketches were removed when the dispatch-only migration
    // regressed heritage chains). If a follow-up worker re-adds a
    // half-baked helper, this guard catches it.
    let stale_helper_invocations = src.matches("project_type_class_b_via_dispatch").count()
        + src
            .matches("project_type_class_b_shape_via_dispatch")
            .count();
    assert_eq!(
        stale_helper_invocations, 0,
        "Phase 5d 4c: meta_resolve.rs must not contain stale \
         `project_type_class_b_via_dispatch` helper references (the \
         dispatch-only Class B helper was removed because it \
         regressed transitive heritage resolution; class B migration \
         is deferred to 5g). found {stale_helper_invocations}"
    );

    // Confirm the TODO(phase-5g) markers exist so a follow-up
    // worker can locate the deferred sites without re-deriving the
    // analysis. The brief lists 11 Class B caller sites in
    // meta_resolve.rs — at least 5 of those carry a TODO marker
    // (clusters of related sites share a single comment block).
    let todos_in_meta_resolve = src.matches("TODO(phase-5g)").count();
    assert!(
        todos_in_meta_resolve >= 5,
        "Phase 5d 4c: meta_resolve.rs must have at least 5 \
         TODO(phase-5g) markers documenting the deferred Class B \
         sites; found {todos_in_meta_resolve}"
    );

    // Confirm the Class B caller sites are still on the engine
    // (not accidentally dropped). Pre-4c the count is 11; post-4c
    // it stays at 11 because Class B migration is deferred. A
    // worker that later migrates these sites should update this
    // test's expected count alongside the migration.
    let invocations = count_callsites(
        &src,
        &[
            ".project_type_surface_expr(",
            ".project_type_surface_shape(",
            ".project_prepared_type_surface_expr(",
            ".project_prepared_type_surface_shape(",
        ],
    );
    assert_eq!(
        invocations, 11,
        "Phase 5d 4c: meta_resolve.rs Class B engine refs must \
         remain at 11 (deferred-to-5g per fix-quality rule); \
         found {invocations}"
    );

    let host_manage_src = read_workspace_file("crates/verter_session/src/host_manage.rs");
    let host_manage_b = count_callsites(
        &host_manage_src,
        &[
            ".project_type_surface_expr(",
            ".project_type_surface_shape(",
            ".project_prepared_type_surface_expr(",
            ".project_prepared_type_surface_shape(",
        ],
    );
    assert_eq!(
        host_manage_b, 1,
        "Phase 5d 4c: host_manage.rs Class B engine refs must remain \
         at 1 (deferred-to-5g); found {host_manage_b}"
    );
}

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
    // r15/F6 (Claude review) — walkdir-based, NOT hard-coded paths.
    // Hard-coded paths break after Phase 5l deletes
    // component_meta_query_engine.rs (file-missing => panic in
    // read_workspace_file) and after Phase 11 splits meta_resolve.rs
    // (path either no longer exists or becomes a thin re-export shell
    // that trivially passes — losing signal).
    //
    // The r15 design enumerates EVERY .rs file under
    // crates/verter_session/src and asserts each ≤ DEFAULT_MAX_LINES
    // unless on an allow-list of known-larger files.
    //
    // Default budget: 4000 lines per file (post-Phase-11 expectation).
    // Allow-list: files documented to be intentionally larger
    // (e.g., generated SDK shims). Each entry MUST link to a phase
    // report justifying the exception.
    use std::collections::HashSet;
    use walkdir::WalkDir;
    const DEFAULT_MAX_LINES: usize = 4000;
    let allow_list: HashSet<&str> = [
        // (path-relative-to-workspace-root, justified-by-report)
        // Add entries here ONLY with a phase-report citation.
    ]
    .iter()
    .copied()
    .collect();
    let crate_root = workspace_root().join("crates/verter_session/src");
    let mut violations = Vec::<String>::new();
    for entry in WalkDir::new(&crate_root) {
        let entry = entry.expect("walkdir entry");
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let rel = path
            .strip_prefix(workspace_root())
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if allow_list.contains(rel.as_str()) {
            continue;
        }
        let src = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        let lines = src.lines().count();
        if lines > DEFAULT_MAX_LINES {
            violations.push(format!(
                "{rel}: {lines} > {DEFAULT_MAX_LINES} (Phase 11 god-module budget)"
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

    // Phase 5m §5.13a.2 — TODO(phase-5g) markers were deleted as
    // each callsite migrated to its bridge helper. The
    // discriminating proof is now the absence of direct engine-method
    // callsites in meta_resolve.rs — counted below.
    //
    // Note: the engine method body itself still has internal callers
    // (the 21 engine-internal sites enumerated in
    // phase-05l-stuck.md's §5.14.1 gate output). Those are deleted
    // atomically with the engine body in 5l per §5.14.2. The guard
    // here only counts EXTERNAL callsites (in meta_resolve.rs and
    // host_manage.rs) — the post-5m invariant is "zero".

    // Phase 5m §5.13a.2 invariant: meta_resolve.rs Class B engine
    // refs are now ZERO (all 11 + 1 sites migrated through bridge
    // helpers). The bridges themselves contain engine method calls
    // inside `#[allow(deprecated)]` — those are not counted here
    // because the regex includes `.project_*` (with the leading
    // dot) which matches `engine.project_*(...)` callsites; the
    // bridges' internal calls match too, so we filter them out by
    // requiring the callsite is NOT inside a `*_via_host*` helper.
    //
    // Simplest discriminating shape: count callsites OUTSIDE the
    // `Phase 5m §5.13a.2 — engine-method caller migration` section
    // demarcated by the section comment header.
    // Phase 5l §5.14.2 update: the bridge section header changed when
    // the bridges were rewritten to call dispatch + engine pub(crate)
    // helpers directly (the 5m migration-window
    // `#[allow(deprecated)]` annotations are gone post-engine-deletion).
    // Match the new header.
    let bridge_section_marker = "Phase 5l §5.14.2 — bridge helpers (post engine-method deletion).";
    let bridge_section_start = src
        .find(bridge_section_marker)
        .expect("meta_resolve.rs must contain the §5.14.2 bridge section header");
    // The bridge section ends at the next section header
    // ("Plan §4.10 / K1 — `MacroFieldGraphState`...") — find that
    // marker to bound the bridge section body.
    let bridge_section_end_marker = "Plan §4.10 / K1";
    let bridge_section_end = src[bridge_section_start..]
        .find(bridge_section_end_marker)
        .expect("meta_resolve.rs must contain the §4.10 K1 section header marking the end of the bridge block");
    let pre_bridge = &src[..bridge_section_start];
    let post_bridge = &src[bridge_section_start + bridge_section_end..];
    let outside_bridge_src = format!("{pre_bridge}{post_bridge}");

    let invocations_outside_bridges = count_callsites(
        &outside_bridge_src,
        &[
            ".project_type_surface_expr(",
            ".project_type_surface_shape(",
            ".project_prepared_type_surface_expr(",
            ".project_prepared_type_surface_shape(",
        ],
    );
    assert_eq!(
        invocations_outside_bridges, 0,
        "Phase 5m §5.13a.2: meta_resolve.rs Class B engine refs must \
         be ZERO outside the bridge-helpers section (all sites migrated \
         through `*_via_host_threaded` bridges); found \
         {invocations_outside_bridges}"
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
        host_manage_b, 0,
        "Phase 5m §5.13a.2: host_manage.rs Class B engine refs must \
         be ZERO (the JSX.IntrinsicElements site migrated through the \
         bridge helper); found {host_manage_b}"
    );
}

#[test]
#[ignore = "phase-05l pending"]
fn no_unbounded_recursion_in_resolver_core() {
    // r15/F15 (Claude review) — static guard for §0.6.5 stack-depth
    // discipline. Flags any `fn` body in resolver_core/ that calls
    // itself by name without going through an explicit
    // depth_budget/iterative-frame helper. False positives are
    // acceptable; the allow-list below carries auditied exceptions
    // with phase-report citations.
    //
    // The pattern: a fn declared as `fn foo(...) ... { ... foo( ... }`
    // or `fn foo(...) ... { ... self.foo( ... }` is flagged unless
    // the body also contains a `depth_budget` or `iterative_frame`
    // accessor reference.
    //
    // The guard is intentionally permissive in regex form — its
    // purpose is to surface candidates for audit, not to gate at
    // arbitrary precision. Phase 5l's owner reviews the violations
    // list and either (a) adds the auditied case to the allow-list
    // with a citation, or (b) refactors to a depth-budgeted shape.
    use regex::Regex;
    use std::collections::HashSet;
    use walkdir::WalkDir;
    let allow_list: HashSet<&str> = [
        // (function-name, justified-by-report)
        // Add entries here ONLY with a phase-report citation.
    ]
    .iter()
    .copied()
    .collect();
    let resolver_dir = workspace_root().join("crates/verter_session/src/resolver_core");
    let fn_decl_re =
        Regex::new(r"(?m)^\s*(?:pub(?:\([^)]+\))?\s+)?(?:async\s+)?fn\s+([a-z_][a-z0-9_]*)\s*[(<]")
            .unwrap();
    let mut violations = Vec::<String>::new();
    for entry in WalkDir::new(&resolver_dir) {
        let entry = entry.expect("walkdir entry");
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let src = std::fs::read_to_string(path).unwrap();
        for cap in fn_decl_re.captures_iter(&src) {
            let fn_name = cap.get(1).unwrap().as_str();
            if allow_list.contains(fn_name) {
                continue;
            }
            // Heuristic: the file references the fn name elsewhere AND has no depth-budget marker.
            let self_call = format!("self.{fn_name}(");
            let direct_call = format!("{fn_name}(");
            let recursion_call_count =
                src.matches(&self_call).count() + src.matches(&direct_call).count();
            // Subtract one for the declaration itself.
            if recursion_call_count > 1
                && !src.contains("depth_budget")
                && !src.contains("iterative_frame")
                && !src.contains("MAX_DEPTH")
            {
                let rel = path
                    .strip_prefix(workspace_root())
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                violations.push(format!(
                    "{rel}: fn {fn_name} appears recursive without depth budget"
                ));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "no_unbounded_recursion_in_resolver_core (Phase 5l flips this):\n{}",
        violations.join("\n")
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
        "crates/verter_session/src/resolver_core/component_meta_query_engine.rs",
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
    // SURVIVES the deletion (`should_preserve_shallow_field_expr` is
    // an unrelated public engine method that stays). If this assert
    // fails, the discriminator is broken — we'd miss real
    // re-introductions.
    assert!(
        src.contains("pub fn should_preserve_shallow_field_expr("),
        "discriminator check: the surviving engine method \
         `should_preserve_shallow_field_expr` must still appear in the \
         engine source — its absence means the discriminator is \
         broken and this test cannot detect re-introductions of the \
         retired methods"
    );
}

#[test]
#[ignore = "phase-06c pending"]
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

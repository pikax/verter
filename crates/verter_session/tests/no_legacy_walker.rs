//! LEGACY_GATE_SELF — static-grep gate (plan §11.4).
//!
//! Guards against re-introduction of the legacy walker family. The
//! walker's outer shim and entire inner body family
//! (`walker_cycle_key_node`,
//! `expand_generic_ref_via_scope_iteration`, and the visited-set
//! helper variant) no longer exist, along with the
//! `component_meta_dispatch_iteration` module that hosted the
//! walker's visited-set helper. The `RETIRED_SYMBOLS` constant below
//! holds the canonical list this gate enforces.
//!
//! Self-exclusion: the first 5 lines of this file contain
//! `LEGACY_GATE_SELF` so the recursive walk skips this file.

use std::path::PathBuf;

const RETIRED_SYMBOLS: &[&str] = &[
    // Phase 9 cutover (plan §11.2): the inner walker helpers are
    // DELETED. Re-introduction at any site is forbidden.
    "walker_cycle_key_node",
    "expand_generic_ref_via_scope_iteration",
    "walk_component_meta_member_surface_expr_with_visited",
    // The dispatch-iteration module that hosted the visited-set +
    // generic-rescue helpers was deleted in the same commit.
    "component_meta_dispatch_iteration",
    "WalkerVisitedNodes",
    "VisitedPushOutcome",
    // Plan §11.2 cleanup: the legacy walker's `MaterializedMemberSurfaceDb`
    // family had zero callers post-Phase-9 (the walker shim now delegates
    // to `materialize_component_meta_structure` which publishes through
    // `MaterializeStructureDb`). Re-introducing any of these names at a
    // call site would re-wire the dead cache lane.
    "MaterializedMemberSurfaceDb",
    "MaterializedMemberSurfaceEntry",
    "MaterializedMemberSurfaceKey",
    "MaterializedMemberSurfaceTarget",
    // Commit E (plan §6.6) — inline-registry-route legacy chain.
    "walk_member_route_via_alias_body",
    "materialize_inline_registry_member_route_from_decl_body",
    "materialize_inline_registry_member_route_if_materializable",
    // Commit D (plan §6.5) — TypeExpr legacy package-ref check (the
    // `_node` graph-native variant is retained).
    "component_meta_ref_resolves_to_package",
    // Commit F (plan §6.7) — TypeExpr legacy cycle walker.
    "decl_body_reaches_cycle_via_walker",
    // Commit G (plan §6.8) — walker shim outer entry.
    "walk_component_meta_member_surface_expr",
    // Commit I (plan §6.10 sub-task 4 / §4.19) — unconditionally
    // retired post-§4.19 deterministic deletion. The composition
    // predicate had zero production callers post-Phase-9 cutover; its
    // sole consumer was a unit test that has also been deleted.
    "registry_member_route_inline_materializable_node",
    // Commit I (plan §6.10 sub-task 4 / §4.19) — `raw_member_path_leaf`
    // was retired in commit E. The shared object-member navigation
    // logic that `explicit_object_member` provided is now inlined
    // into `component_meta_registry_raw_member_path_surface`'s body
    // as the private nested `navigate_object_member` helper.
    "raw_member_path_leaf",
    "explicit_object_member",
    // Commit N (plan §6.15) — 4 TypeExpr predicates retired after
    // Phase 11 callers migrated to graph-native `_node` counterparts
    // (J0/J1/J2/J4). The deletion targets (with identifier-boundary
    // matching, suffixed names like `_node` / `_typeexpr` /
    // `lowered_*` are NOT false-positives):
    //   - `type_expr_has_package_backed_root` — replaced by J0
    //     (`type_node_has_package_backed_root`).
    //   - `type_expr_needs_member_route_materialization` — replaced
    //     by J1 (`type_node_needs_member_route_materialization`).
    //     Surviving callers (`field_should_preserve_shallow_symbolic_raw_type`,
    //     `walk_component_meta_macro_shape_member_types`) lower their
    //     TypeExpr inputs to a SemanticNodeId via Navigate and call
    //     the J1 `_node` variant through the `lowered_*` helper.
    //   - `slot_binding_param_can_stay_symbolic_typeexpr` (the M
    //     lowering-failure fallback) — replaced by J2
    //     (`slot_binding_param_can_stay_symbolic_node`).
    //   - `preserve_package_backed_symbolic_refs` — replaced by J4
    //     (`preserve_package_backed_symbolic_refs_node`). Caller in
    //     `materialize_component_meta_registry_candidate` lowers
    //     materialized + raw TypeExprs to nodes, dispatches to J4,
    //     then raises the result back to TypeExpr.
    // The bare `slot_binding_param_can_stay_symbolic` identifier is
    // also retired: the post-M wrapper that retained that name is
    // renamed to `lowered_slot_binding_param_can_stay_symbolic`,
    // matching the lowered_* helper-naming convention introduced in
    // commit N. Plan §6.15 / N's deletion target list cites the bare
    // identifier; identifier-boundary matching keeps the surviving
    // `_node` variant + the renamed wrapper from triggering the gate.
    "type_expr_has_package_backed_root",
    "type_expr_needs_member_route_materialization",
    "slot_binding_param_can_stay_symbolic_typeexpr",
    "preserve_package_backed_symbolic_refs",
    // Commit O (plan §6.15) — the temporary
    // `engine.is_package_backed_decl` adapter (introduced in commit D
    // to satisfy the TypeExpr-walking caller migrations) is deleted.
    // After commit N migrated the last production caller to graph-
    // native predicates that consume `DeclIdentity` directly via
    // `component_meta_ref_resolves_to_package_node`, the adapter has
    // zero callers and is retired.
    "is_package_backed_decl",
    // Commit P (plan §6.15) — the temporary
    // `typeexpr_root_reaches_transitive_cycle` adapter (introduced in
    // commit F as a TypeExpr→graph-native cycle bridge) is deleted.
    // The 4 surviving callers (`expr_needs_projection_rescue` + 3
    // sites inside `materialize_component_meta_macro_shape_member_type_expr`)
    // are migrated to call `lowered_root_reaches_transitive_cycle` —
    // the lowered_*-named migration helper introduced for these
    // callers (consistent with the convention introduced in commit N
    // for `lowered_needs_member_route_materialization` and friends).
    // The graph-native primitive `ref_root_reaches_transitive_cycle_node`
    // is the canonical cycle-detection authority.
    "typeexpr_root_reaches_transitive_cycle",
    // SA-1.D (plan §3.8) — legacy parser-side slot-binding enrichment
    // helpers superseded by graph-native synthesis
    // (`slot_binding_graph::resolve_slot_bindings_graph_native`).
    // All five had zero production callers after SA-1.C removed the
    // two `enrich_missing_slot_bindings` call sites from
    // `compute_component_meta_state_inner`. Re-introducing any of these
    // at a production call site would re-wire the retired parser path.
    "enrich_missing_slot_bindings",
    "collect_expanded_slot_binding_param_types",
    "decide_typeexpr_conditional_with_function_extends",
    "substitute_infer_in_typeexpr",
    "collect_expanded_slot_bindings_from_object_type",
    // §7.3 cutover — the legacy outer macro-shape walker driver is
    // retired. Production routes through
    // `meta_resolve::projectors::project_evaluated_types`.
    "walk_component_meta_macro_shape_member_types",
    // §7.3 follow-up cutover — the per-member rescue cascade is
    // DELETED. The projector self-reduces nested `IndexedAccess` /
    // `KeyOf` / `TypeOf` / `Conditional` / `Mapped` / `Infer` chains
    // via `materialize_component_meta_type_expr_until_stable` on the
    // raised member surface (see `meta_resolve::projectors`). The
    // recursive-alias preservation case (e.g.
    // `Tree = { children?: Tree[] }`) is now handled by the
    // publication policy's `RecursiveRef` back-edge in
    // `component_meta_resolution_policy::core::rewrite_ref` —
    // there is no per-member rescue helper behind the projector.
    //
    // Re-introducing any of these symbols at a production call site
    // would re-wire the retired dual-path rescue cascade.
    "materialize_component_meta_macro_shape_member_type_expr",
    "publish_member_route_result",
    "MemberRouteResultDb",
    "MemberRouteResultEntry",
    "MemberRouteResultCacheKey",
    "member_route_result_db_get_or_compute",
    "member_route_result_db",
    "MEMBER_ROUTE_FAST_PATH_HITS",
    "MATERIALIZE_MACRO_SHAPE_MEMBER_TYPE_EXPR_CALLS",
    "MATERIALIZE_MACRO_SHAPE_MEMBER_TYPE_EXPR_NS",
    "FIELD_PROPS_MEMBER_ROUTE_LOOP_CALLS",
    "FIELD_PROPS_MEMBER_ROUTE_LOOP_NS",
    "define_props_member_can_stay_symbolic_without_rescue",
    // Retired in this commit: the per-field rescue cascade driver and
    // its helpers, the ComponentConfig fast-path, and the test-only
    // counters that observed them. The projector path
    // (`reduce_published_field_types` + `reduce_field_type_expr`) is
    // the sole post-projection authority for finalising published
    // field types — re-introducing any of these symbols would resurrect
    // the dual-path rescue architecture this cutover deletes.
    "materialize_component_meta_field_types",
    "rescue_field",
    "MEMBER_ROUTE_CALLS_COUNTER",
    "COMPONENT_CONFIG_FAST_PATH_HITS_COUNTER",
    "component_config_theme_variant_fast_path",
    "component_config_alias_classification",
    "collect_component_config_indexed_path",
    "FastPathOutcome",
    "field_should_preserve_shallow_symbolic_raw_type",
    "lowered_needs_member_route_materialization",
    "type_expr_is_slots_member_route",
    "type_expr_is_terminal_scalar_surface",
    "type_expr_is_indexed_access_route",
    "type_expr_is_non_empty_object_surface",
    "raw_indexed_access_root_is_workspace_owned",
    "interface_body_has_members_needing_materialization",
    "top_level_imported_ref_can_stay_symbolic",
    "parsed_field_raw_type",
    "FIELD_PROPS_RESCUE_FIELD_CALLS",
    "FIELD_PROPS_RESCUE_FIELD_NS",
    "FIELD_PROPS_NEEDS_MEMBER_ROUTE_CALLS",
    "FIELD_PROPS_NEEDS_MEMBER_ROUTE_NS",
    "FIELD_PROPS_ROUTED_SURFACE_CALLS",
    "FIELD_PROPS_ROUTED_SURFACE_NS",
    "MATERIALIZE_FIELD_TYPES_CALLS",
    "MATERIALIZE_FIELD_TYPES_NS",
];

const SCAN_DIRS: &[&str] = &["crates", ".claude/skills", "docs"];

const SCAN_FILES: &[&str] = &["CLAUDE.md", "AGENTS.md", "MEMORY.md"];

fn workspace_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    loop {
        if p.join("CLAUDE.md").exists() && p.join("crates").exists() {
            return p;
        }
        if !p.pop() {
            panic!(
                "unable to locate workspace root from {}",
                env!("CARGO_MANIFEST_DIR")
            );
        }
    }
}

fn is_self_excluded(path: &std::path::Path) -> bool {
    let Ok(text) = std::fs::read_to_string(path) else {
        return false;
    };
    text.lines().take(5).any(|l| l.contains("LEGACY_GATE_SELF"))
}

fn is_changelog(path: &std::path::Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.eq_ignore_ascii_case("CHANGELOG.md"))
        .unwrap_or(false)
}

fn collect_text_files(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // Skip target/ and node_modules/ — these don't contain hand-authored source.
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name == "target" || name == "node_modules" || name == ".git" {
                continue;
            }
            collect_text_files(&path, out);
        } else if path.is_file() {
            let ext_ok = matches!(
                path.extension().and_then(|e| e.to_str()),
                Some("rs" | "ts" | "tsx" | "js" | "vue" | "md")
            );
            if ext_ok && !is_self_excluded(&path) && !is_changelog(&path) {
                out.push(path);
            }
        }
    }
}

#[test]
fn no_legacy_walker_inner_helpers_outside_their_definitions() {
    let root = workspace_root();
    let mut files: Vec<PathBuf> = Vec::new();
    for dir in SCAN_DIRS {
        let p = root.join(dir);
        if p.exists() {
            collect_text_files(&p, &mut files);
        }
    }
    for file in SCAN_FILES {
        let p = root.join(file);
        if p.exists() && !is_self_excluded(&p) {
            files.push(p);
        }
    }

    // Tight scan: each retired symbol must appear AT MOST in its own
    // definition site (`fn <name>(`) — a re-introduction at another
    // site would mean the symbol has more than one definition + 1 call.
    // Pre-cutover the inner helpers had ~10+ call sites; post-cutover
    // they have ZERO callers (their bodies are unused, gated by
    // `#[allow(dead_code)]`).
    //
    // Plan §6.10 sub-task 3 — identifier-boundary matcher: a retired
    // symbol matches ONLY when its occurrence is bounded by characters
    // that can NOT extend an identifier (i.e., not [A-Za-z0-9_]).
    // This prevents false positives like
    // `component_meta_ref_resolves_to_package` matching the kept
    // `_node` variant `component_meta_ref_resolves_to_package_node`,
    // and `walk_component_meta_member_surface_expr` matching the
    // already-retired `_with_visited` variant.
    fn line_contains_identifier(line: &str, ident: &str) -> bool {
        let bytes = line.as_bytes();
        let needle = ident.as_bytes();
        let n = needle.len();
        if n == 0 || bytes.len() < n {
            return false;
        }
        let is_ident_char = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
        let mut i = 0usize;
        while i + n <= bytes.len() {
            if &bytes[i..i + n] == needle {
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

    for symbol in RETIRED_SYMBOLS {
        let mut hit_files: Vec<(PathBuf, Vec<usize>)> = Vec::new();
        for file in &files {
            let Ok(text) = std::fs::read_to_string(file) else {
                continue;
            };
            let lines: Vec<usize> = text
                .lines()
                .enumerate()
                .filter_map(|(i, l)| {
                    if line_contains_identifier(l, symbol) {
                        Some(i + 1)
                    } else {
                        None
                    }
                })
                .collect();
            if !lines.is_empty() {
                hit_files.push((file.clone(), lines));
            }
        }
        // Post-cutover the inner walker helpers are DELETED — the
        // only allowed references are in historical architecture
        // documentation (`docs/arch/debt-closure/`).
        for (file, lines) in &hit_files {
            let path_str = file.to_string_lossy();
            let is_allowed = path_str.contains("docs/arch/debt-closure/")
                || path_str.contains("docs\\arch\\debt-closure\\");
            assert!(
                is_allowed,
                "Phase 9 static-grep gate (plan §11.4): retired walker-family \
                 symbol `{symbol}` reintroduced at {file:?} lines {lines:?}. \
                 Post-cutover the inner walker family is DELETED — the only \
                 allowed references are historical docs under \
                 `docs/arch/debt-closure/`."
            );
        }
    }
}

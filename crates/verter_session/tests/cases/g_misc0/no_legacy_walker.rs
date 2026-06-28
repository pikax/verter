//! LEGACY_GATE_SELF — static-grep gate.
//!
//! Guards against re-introduction of retired implementation symbols.
//! The `RETIRED_SYMBOLS` constant below holds the canonical list of
//! identifiers that must not appear in production source. The scanner
//! follows the architecture-guard discipline:
//!
//!  - it scans ONLY `crates/*/src/**/*.rs` (production source),
//!  - it skips `_tests.rs` / `tests.rs` / files under a `tests/` segment,
//!  - it strips line, block, and `#[cfg(test)] mod` modules before
//!    matching (so doc comments and inline tests do not trip the gate),
//!  - it matches each symbol at identifier boundaries (so `foo` does
//!    NOT match `foo_node` or `lowered_foo`).
//!
//! Architecture documentation (`CLAUDE.md`, `.claude/skills/*`,
//! `docs/`), test files (`crates/*/tests/**`, `_tests.rs` siblings,
//! and inline `#[cfg(test)]` modules), benches, examples, and
//! generated artifacts are intentionally out of scope: a forbidden
//! identifier appearing in those contexts is description, not live
//! source.
//!
//! Self-exclusion: the first 5 lines of this file contain
//! `LEGACY_GATE_SELF` so the recursive walk skips this file.

use std::path::{Path, PathBuf};

const RETIRED_SYMBOLS: &[&str] = &[
    // The inner walker helpers are DELETED. Re-introduction at any
    // site is forbidden.
    "walker_cycle_key_node",
    "expand_generic_ref_via_scope_iteration",
    "walk_component_meta_member_surface_expr_with_visited",
    // The dispatch-iteration module that hosted the visited-set +
    // generic-rescue helpers is deleted.
    "component_meta_dispatch_iteration",
    "WalkerVisitedNodes",
    "VisitedPushOutcome",
    // The legacy walker's `MaterializedMemberSurfaceDb`
    // family has zero callers (the walker shim now delegates
    // to `materialize_component_meta_structure` which publishes through
    // `MaterializeStructureDb`). Re-introducing any of these names at a
    // call site would re-wire the dead cache lane.
    "MaterializedMemberSurfaceDb",
    "MaterializedMemberSurfaceEntry",
    "MaterializedMemberSurfaceKey",
    "MaterializedMemberSurfaceTarget",
    // Inline-registry-route legacy chain.
    "walk_member_route_via_alias_body",
    "materialize_inline_registry_member_route_from_decl_body",
    "materialize_inline_registry_member_route_if_materializable",
    // TypeExpr legacy package-ref check (the `_node` graph-native
    // variant is retained).
    "component_meta_ref_resolves_to_package",
    // TypeExpr legacy cycle walker.
    "decl_body_reaches_cycle_via_walker",
    // Walker shim outer entry.
    "walk_component_meta_member_surface_expr",
    // Unconditionally retired by deterministic deletion. The
    // composition predicate had zero production callers; its
    // sole consumer was a unit test that has also been deleted.
    "registry_member_route_inline_materializable_node",
    // `raw_member_path_leaf` is gone. The shared object-member navigation
    // logic that `explicit_object_member` provided is now inlined
    // into `component_meta_registry_raw_member_path_surface`'s body
    // as the private nested `navigate_object_member` helper.
    "raw_member_path_leaf",
    "explicit_object_member",
    // Retired TypeExpr predicates whose graph-native `_node`
    // counterparts are the sole authority. The cycle/package/route
    // checks consume `SemanticNodeId` directly — re-introducing the
    // TypeExpr-walking versions would resurrect the dual-path
    // (TypeExpr-walk + node-walk) materialiser is deleted.
    // Identifier-boundary matching keeps suffixed names like `_node`
    // and the renamed `lowered_*` migration helpers from tripping the
    // gate.
    "type_expr_has_package_backed_root",
    "type_expr_needs_member_route_materialization",
    "slot_binding_param_can_stay_symbolic_typeexpr",
    "preserve_package_backed_symbolic_refs",
    // Retired graph-native slot-binding stay-symbolic predicate +
    // its private helper + the body-shape probe it depended on. The
    // production stay-symbolic decision is owned by
    // `slot_binding_graph::slot_param_root_is_symbolic_only`, which
    // applies a strictly simpler shape allow-list (the synthesizer
    // handles Object/Union/Intersection/Array/Tuple by direct
    // empty-path Shallow walk and never asks "can the param stay
    // symbolic" for those). The retired triplet was added as an
    // additive variant intended for a materialiser wiring that never
    // landed — its Object recursion + non-object-top-level-surface
    // probe encode contracts the new architecture deliberately
    // rejects. Re-introducing any of these symbols would resurrect
    // a dead architectural exploration.
    "slot_binding_param_can_stay_symbolic_node",
    "node_value_is_concrete_or_symbolic",
    "node_has_non_object_top_level_surface",
    // The temporary `engine.is_package_backed_decl` adapter is deleted.
    // Production callers consume graph-native predicates that take a
    // `DeclIdentity` directly via
    // `component_meta_ref_resolves_to_package_node`, so the adapter has
    // zero callers and is gone.
    "is_package_backed_decl",
    // The temporary `typeexpr_root_reaches_transitive_cycle` adapter
    // (a TypeExpr→graph-native cycle bridge) is deleted. Callers
    // (`expr_needs_projection_rescue` + 3 sites inside
    // `materialize_component_meta_macro_shape_member_type_expr`)
    // call `lowered_root_reaches_transitive_cycle` — the lowered_*-named
    // migration helper for these callers (consistent with
    // `lowered_needs_member_route_materialization` and friends).
    // The graph-native primitive `ref_root_reaches_transitive_cycle_node`
    // is the canonical cycle-detection authority.
    "typeexpr_root_reaches_transitive_cycle",
    // Legacy parser-side slot-binding enrichment helpers superseded by
    // graph-native synthesis
    // (`slot_binding_graph::resolve_slot_bindings_graph_native`).
    // All five have zero production callers since the two
    // `enrich_missing_slot_bindings` call sites were removed from
    // `compute_component_meta_state_inner`. Re-introducing any of these
    // at a production call site would re-wire the retired parser path.
    "enrich_missing_slot_bindings",
    "collect_expanded_slot_binding_param_types",
    "decide_typeexpr_conditional_with_function_extends",
    "substitute_infer_in_typeexpr",
    "collect_expanded_slot_bindings_from_object_type",
    // The legacy outer macro-shape walker driver is gone. Production
    // routes through
    // `meta_resolve::projectors::project_evaluated_types`.
    "walk_component_meta_macro_shape_member_types",
    // The per-member rescue cascade is
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
    // would re-wire the deleted dual-path rescue cascade.
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
    // Deleted: the per-field rescue cascade driver and
    // its helpers, the ComponentConfig fast-path, and the test-only
    // counters that observed them. The projector path
    // (`reduce_published_field_types` + `reduce_field_type_expr`) is
    // the sole post-projection authority for finalising published
    // field types — re-introducing any of these symbols would resurrect
    // the deleted dual-path rescue architecture.
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
    // Dead scaffold deletion: `MacroFieldGraphState` and the
    // `dispatch_lower_counter_*` test instrumentation never had a
    // production caller. The struct was added for the rescue-cascade
    // pipeline that the projector deleted. The only consumers
    // were 12 scaffold-shape tests that have also been deleted.
    // Re-introducing any of these names would resurrect a dead
    // architectural exploration.
    "MacroFieldGraphState",
    "dispatch_lower_counter_get",
    "dispatch_lower_counter_reset",
    "dispatch_lower_counter_increment",
    // Dead graph-native predicate that had no production caller once
    // member-route-materialisation was deleted.
    // The TypeExpr predecessor `lowered_needs_member_route_materialization`
    // is gone with the rescue cascade deletion. This is the
    // graph-native counterpart that survived as `#[allow(dead_code)]`
    // scaffolding consumed only by 4 characterization tests; both the
    // predicate and its tests are deleted.
    "type_node_needs_member_route_materialization",
    // Dead future-use test helper. Built for a planned route-union
    // shape assertion that never landed; CLAUDE.md "Don't add features
    // beyond what the task requires" forbids carrying it as scaffolding.
    "assert_route_union_surface",
    // File-language routing authority: the duplicated per-crate
    // file-kind enums (session / scheduler source_loader / scheduler
    // node / workspace), the resolver-core export-graph clone, the
    // FFI kind parser with its silent `"vue"` default, and the LSP
    // path sniffing helper are all replaced by
    // `verter_language::FileLanguage` + `LanguageRegistry`
    // classification. Re-introducing any of these names would revive
    // a second language-kind definition next to the single routing
    // authority (see also
    // `single_language_classifier.rs::single_language_classifier`).
    "FileKind",
    "ExportGraphFileKind",
    "ffi_file_kind_to_host",
    "is_vue_file",
    // Framework parse-artifact carrier substrate: every production
    // `cached_parse` carrier field — `IndexedReady`,
    // `RouteOwnedShallowEntry`, `HostSourceData`, `CompileInput`,
    // `EffectiveFileState`, `ContentOverrideWithParse`,
    // `ExternalTypeResolutionInputs` — plus the producer/threading
    // locals and the `route_owned_snapshot_cached_parse_hits`
    // provenance-counter family are renamed/replaced by the
    // framework-neutral `framework_parse:
    // Option<Arc<FrameworkParseArtifact>>` payload. Re-introducing
    // the token would revive a Vue-typed parse carrier on the
    // neutral session surface.
    "cached_parse",
    // String-resolver eradication: the typed-IR-only
    // resolver rule deletes every hand-rolled type-text splitter,
    // source-slicing helper, `parse_type_annotation` reparse fallback,
    // and node_modules substring router from the analyzer / projector /
    // registry / policy / materialiser / compat pipeline. Re-introduction
    // at any production site is forbidden.
    "extract_slot_bindings_from_pick_type",
    "extract_slot_bindings_from_type_text",
    "split_top_level_type_segments",
    "extract_type_string_literal_name",
    "parse_annotation_or_unknown",
    "parse_annotation_or_unknown_for_public_instance",
    "collect_imported_props_like_raw_refs",
    "extract_pick_slot_bindings",
    "simplify_pick_slot_binding_type_text",
    "extract_string_literal_name",
    "trim_trailing_type_text",
    "find_top_level_char",
    // NOTE: `find_matching_delimiter` is intentionally OMITTED from
    // this list. The retired surface_projector helper of that name is
    // deleted, but `verter_lsp::features::hover::find_matching_delimiter`
    // is a live, unrelated LSP hover-preview helper that legitimately
    // owns the same identifier. The architecture guard
    // `no_text_based_macro_surface_projection_helpers` in
    // `architecture_guards.rs` covers re-introduction inside
    // `surface_projector.rs` specifically — a tighter and more
    // architecturally meaningful enforcement than a name collision
    // would provide here.
    "split_top_level_segments",
    "extract_first_slot_param_type_text",
    "slice_declaration_text",
    "extract_named_declaration_text",
    "extract_declaration_details",
    "canonical_resolves_to_package",
    "parse_indexed_access_from_raw",
    // The OLD name `parse_type_annotation` must NEVER reappear in
    // production source (the function was renamed to
    // `parse_jsdoc_tag_type_payload` and narrowed to JSDoc tag-type
    // payloads). This entry is a belt-and-braces companion to the
    // dedicated `no_old_parse_type_annotation_name_in_production`
    // architecture guard.
    "parse_type_annotation",
    // The retired `resolver_core::type_text_parser` module — a
    // hand-written recursive-descent parser for TS type text. It is
    // DELETED. The scanner's identifier-boundary match treats the
    // module name identically to a function name, so re-introduction
    // at any site is forbidden here too.
    "type_text_parser",
    // The retired `type_text_parser::parse_type_text` entry function.
    // Companion to `type_text_parser` (the module name) — covers the
    // case where someone re-introduces just the public entry as a
    // free function in a different module.
    "parse_type_text",
    // The retired TS-checker-display-text adapter. It wrapped checker
    // display text in `type __T = ...;` and re-parsed it via OXC to
    // produce a `TypeExpr`. It had NO production caller (only
    // self-tests + a perf bench) and is DELETED under the
    // no-dormant-legacy rule. Both the function name and the module
    // name are forbidden: re-introducing either at any production site
    // would revive the dead checker-display-text re-parse bridge. The
    // scanner treats the module name identically to a function name.
    "parse_checker_text_to_type_expr",
    "checker_text_adapter",
    // The component-meta policy's nominal `name.ends_with("Props")`
    // classifier is DELETED. The structural macro-participation
    // predicate (`PolicyCtx::is_macro_participating`) is the sole
    // authority for role-bearing-ref classification. Reintroducing
    // `is_props_suffix` at any production site would resurrect
    // nominal classification.
    "is_props_suffix",
    // The reverse-dependent upsert-time invalidation cascade and its
    // test-only gate are DELETED. An owner upsert drains only the
    // upserted canonical's own caches; cross-file consumers
    // revalidate lazily on read through their `fact_dep_signature`
    // checks. Re-introducing any of these names would resurrect the
    // eager reverse-dependent cascade. (`UpsertDependentEviction` was
    // the cascade's on/off enum; `dependent_eviction` its parameter;
    // `run_dependent_cascade` the gating local; the two
    // `*_without_*` entry points existed only to skip the cascade.)
    "UpsertDependentEviction",
    "upsert_without_dependent_eviction",
    "dependent_eviction",
    "run_dependent_cascade",
    "register_facts_for_new_content_without_eviction",
    // The eager/lazy macro-surface bridge +
    // its readers are DELETED. Production resolves props/emits/slots/exposed
    // through the typeinfo Vue surface (`VerterHost::vue_macro_dtos`). Re-introducing any of
    // these names at a production site would revive the deleted eager rail or
    // its lossy reader.
    "ImportedMacroSurface",
    "ImportedDeclarationIdentity",
    "EagerResolvedMacro",
    "ResolvedMacroSurface",
    "from_eager_meta",
    "MacroSurfaceView",
    "surface_view_from_base_node",
    "surface_view_from_decl_identity",
    "union_common_member_surface",
    "member_display_jsdoc",
    "declaring_decl_span",
    "project_imported_macro_surfaces",
    // Walker-cluster deletion: the prepared-surface / routed walker modules
    // (routed_expr.rs + prepared_surface.rs) and their request-local + host
    // caches are DELETED. Pick/Omit/member literal-key enumeration resolves
    // through the dispatch-backed `enumerate_route_literal_keys` chain; macro
    // surfaces resolve through `dispatch_routed_expr_surface_expr` /
    // `dispatch_projected_surface`. Re-introducing any of these resurrects a
    // walker resolution path the one-engine rule forbids.
    "project_routed_expr_surface_expr",
    "project_routed_expr_surface_expr_direct",
    "project_pick_route_surface_expr_via_routed_expr",
    "project_pick_route_surface_expr_via_members",
    "cached_prepared_root_surface",
    "project_prepared_root_surface",
    "project_prepared_root_surface_inner",
    "r21_c4_project_prepared_surface_from_symbol_with_flag",
    "project_prepared_surface_from_symbol",
    "project_prepared_surface_from_expr",
    "project_prepared_surface_from_ref",
    "project_prepared_requested_member_from_symbol",
    "project_prepared_requested_member_from_expr",
    "project_prepared_requested_member_surface_from_expr",
    "project_prepared_member_route_surface_expr",
    "project_prepared_pick_route_surface_expr",
    "publish_prepared_surface_to_host_db",
    "publish_prepared_member_to_host_db",
    "merge_prepared_intersection_arms",
    "dispatch_member_for_root_symbol",
    // The retired macro-object materialiser subgraph (define_* now via the
    // dispatch projectors `projectors::define_shapes`).
    "produce_macro_object_shapes",
    "produce_macro_object_shapes_for_purpose",
    "produce_one_macro_object_shape",
    "produce_one_macro_object_shape_for_slots",
    "synthesize_define_props_shape_from_known_surface_with_authority",
    "synthesize_define_props_shape_from_registry_root",
    "synthesize_define_emits_shape_from_known_surface",
    "synthesize_define_slots_shape_from_known_surface",
    "synthesize_macro_shape_from_registry_lowered_root",
    "synthesize_macro_object_surface_shape",
    // The macro-shape cursor finaliser that the materialiser subgraph
    // drove is deleted with the rest of the subgraph. define_*
    // finalisation now lives in the dispatch projectors.
    "finalize_macro_shape_through_cursor",
    "project_named_ref_prepared_surface_shape",
    "expr_needs_projection_rescue",
    "MacroShapeSource",
    // The retired prepared/transit-shallow bridge helpers (the surviving
    // root-surface bridge is `project_type_surface_expr_via_host_threaded`).
    "project_type_surface_shape_via_host_threaded",
    "project_type_surface_shape_transit_shallow_via_host_threaded",
    "project_prepared_type_surface_expr_via_host_threaded",
    "project_prepared_type_surface_shape_via_host_threaded",
    "project_expr_class_a_via_dispatch_transit_shallow",
    "project_expr_surface_expr_with_compound_objects_transit_shallow_via_host_threaded",
    // The 4 dead walker host DBs + their producers. PreparedTargetDb is
    // included per the premise correction: its only producer
    // (`resolve_prepared_surface_target`, fed only by the walker-only
    // `prepared_string_literal_keys`) died with the walker.
    "PreparedSurfaceDb",
    "PreparedMemberDb",
    "RoutedExprSurfaceDb",
    "PreparedTargetDb",
    "resolve_prepared_surface_target",
    "prepared_string_literal_keys",
    "engine_fact_signature_for_prepared_target",
    // Prepared-structural-substitution slow-lane removal. The engine-side
    // generic-Ref instantiation helper + the 6 whole-body substitution
    // rewriters are DELETED: generic-Ref instantiation now goes through
    // the shared dispatch lowering `build_instantiate` path
    // (`lower_type_expr_in_scope*` → `SemanticQueryKey::Instantiate`), which
    // binds args into the lowering env and substitutes while lowering. The
    // route-key leaf stabiliser's split-scope arm dispatches `Instantiate`
    // with NODE args directly. Re-introducing any of these names would
    // resurrect the structural whole-substitution slow lane that is
    // eliminated. (`type_expr_references_names` — the surviving
    // general-purpose name predicate — is NOT retired; only its
    // substitution-keyed wrapper `type_expr_references_substitutions` is.)
    "instantiate_local_generic_ref_via_engine",
    "apply_type_param_substitutions",
    "substitute_type_expr",
    "substitute_function_expr",
    "build_default_type_param_substitutions",
    "is_identity_type_param_binding",
    "type_expr_references_substitutions",
    // Owner-local generic-alias registry substitution slow lane — the
    // second raw-`TypeExpr` substitution engine. DELETED: the registry
    // candidate path
    // (`owner_local_generic_alias_substituted_body_via_dispatch` in
    // `host_manage/component_meta_methods.rs`) lowers the owner-local
    // generic ref to the graph's `InstantiationRef` carrier and runs the
    // shared `SemanticQueryKey::Instantiate` query (Navigate mode) — the
    // ONE type-resolution engine — instead of cloning and rewriting the
    // prepared body's `TypeExpr` in place. Re-introducing any of these
    // names would resurrect the parallel structure-preserving substitution
    // walker that is eliminated.
    "component_meta_owner_local_shallow_substituted_alias_body",
    "walk_substitute_typeexpr",
    "component_meta_substitute_typeexpr",
    // Materialised-record-point satisfaction: the unconditional
    // enum-rank backfill fan-out helper `backfill_targets` was replaced
    // by the directional + `cached_satisfies`-gated `slot_domain_siblings`.
    // Re-introducing the enum-rank fan-out would resurrect the
    // lattice-unsound `Shallow → Navigate` clone the satisfaction gate rejects.
    "backfill_targets",
    // Registry-local open-mapped pre-walk. DELETED: the registry
    // structural materialiser runs `mode: ProjectionMode::Navigate`
    // (shallow-by-default), so open carriers survive the materialised
    // structure through the shared L1 carrier-stop predicates — the
    // guard it provided (an Expanded fall-through over an open mapped
    // shell) no longer has a route to guard. Re-introducing it would
    // resurrect a second registry-local openness walker beside the
    // shared `raise.rs` predicates.
    "base_contains_open_mapped_or_unknown",
    // Per-property materialisation-scope re-resolution. DELETED with
    // the registry improvement re-solve loops: the imported-alias
    // refinement materialises every property in the alias's OWN
    // defining scope (`imported_generic_alias_scope`), so a per-value
    // declaration-scope re-resolution has nothing left to select.
    // Re-introducing it would resurrect a consumer-local
    // second-resolution path beside the shared resolver.
    "select_imported_materialization_scope",
    // "Emit" is Vue semantics, not a neutral script-surface concept.
    // The neutral type-surface element types carry the neutral names
    // `ResolvedNamedCallSignature` / `ResolvedCallPayloadForm`
    // (`verter_parser::utils::oxc::script::type_surface`); the retired
    // Vue-flavoured names must not reappear on the neutral surface or
    // as aliases beside it.
    "ResolvedEmit",
    "ResolvedEmitSignature",
    // The Vue adapter's host-owned shallow-metadata store + its key/value
    // types are RETIRED. The neutral, framework-generic
    // `FrameworkSurfaceStore<VueSurfaceKey, MacroSurfaceDtos>` (reached
    // through `VerterHost::vue_surface_store`) is the sole home of the
    // `.vue` macro-surface DTO cache; the relocated producer in
    // `typeinfo::framework_surface::vue_exec` keys it with the neutral
    // `FullKey<VueSurfaceKey>` + `StoredSurfaceDto<MacroSurfaceDtos>`.
    // Re-introducing any of these Vue-private store types would resurrect
    // a second, framework-specific cache lane beside the neutral store.
    "VueShallowMetadataStore",
    "VueMacroDtoKey",
    "VueMacroDtos",
    "VueMacroDtosEntry",
];

/// File names whose presence at the head of the path should make us
/// self-exclude (the gate file itself plus the sibling
/// `architecture_guards.rs`, which carries literal needle strings in
/// its assertions and would otherwise self-trip).
const SELF_EXCLUDED_FILE_NAMES: &[&str] = &["no_legacy_walker.rs", "architecture_guards.rs"];

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

/// True for files whose contents are test-only (siblings named
/// `*_tests.rs` or `tests.rs`, or anything inside a `tests/` segment
/// of the path). Mirrors the discipline used by
/// `architecture_guards::*::is_test_file`.
fn is_test_file(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    if name == "tests.rs" || name.ends_with("_tests.rs") {
        return true;
    }
    path.components()
        .any(|c| c.as_os_str().to_str() == Some("tests"))
}

fn is_self_excluded(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    SELF_EXCLUDED_FILE_NAMES.contains(&name)
}

/// Walk a `crates/*/src/` tree and collect every `.rs` file that is
/// production source (NOT a test file and NOT self-excluded).
fn collect_production_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name == "target" || name == "node_modules" || name == ".git" {
                continue;
            }
            collect_production_rs(&path, out);
        } else if path.is_file()
            && path.extension().and_then(|e| e.to_str()) == Some("rs")
            && !is_test_file(&path)
            && !is_self_excluded(&path)
        {
            out.push(path);
        }
    }
}

/// Replace `//` line comments and `/* ... */` block comments with
/// equivalent-length whitespace, preserving newlines so line numbers
/// stay stable. Skips comment-like sequences inside regular and raw
/// string literals so the strip never invalidates real source.
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

/// Replace the body of every `#[cfg(test)] mod NAME { ... }` block
/// with whitespace (newlines preserved). Inline test modules live
/// inside production source files but are test-only — guard scans
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
                // Find `{` after `mod NAME`. If the next token is
                // `;` instead, this is an out-of-line module
                // declaration (`#[cfg(test)] mod NAME;`) which has
                // no body to strip — let the next iteration advance.
                let mut k = j + 4;
                while k < n && bytes[k] != b'{' && bytes[k] != b';' {
                    k += 1;
                }
                if k < n && bytes[k] == b'{' {
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

/// Identifier-boundary matcher: a retired symbol matches ONLY when
/// its occurrence is bounded by characters that can NOT extend an
/// identifier (i.e., not [A-Za-z0-9_]). This prevents false
/// positives like `component_meta_ref_resolves_to_package` matching
/// the kept `_node` variant
/// `component_meta_ref_resolves_to_package_node`.
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

/// Collect every production `.rs` file under `crates/*/src/` plus
/// the optional self-exclusion list.
fn collect_production_sources() -> Vec<PathBuf> {
    let root = workspace_root();
    let mut files: Vec<PathBuf> = Vec::new();
    let crates_dir = root.join("crates");
    let Ok(crates) = std::fs::read_dir(&crates_dir) else {
        return files;
    };
    for entry in crates.flatten() {
        let crate_dir = entry.path();
        if !crate_dir.is_dir() {
            continue;
        }
        let src = crate_dir.join("src");
        if !src.is_dir() {
            continue;
        }
        collect_production_rs(&src, &mut files);
    }
    files
}

#[test]
fn retired_symbols_absent_from_production_source() {
    let files = collect_production_sources();

    // Read + preprocess each file ONCE, then test every retired symbol
    // against the cached processed text. (Previously this read+preprocessed
    // every file once PER symbol — O(symbols × files) — which dominated the
    // runtime; the inversion is O(files) reads with identical assertions.)
    let mut hits_by_symbol: std::collections::BTreeMap<&str, Vec<(PathBuf, Vec<usize>)>> =
        RETIRED_SYMBOLS.iter().map(|s| (*s, Vec::new())).collect();
    for file in &files {
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        let processed = preprocess(&text);
        let plines: Vec<&str> = processed.lines().collect();
        for symbol in RETIRED_SYMBOLS {
            // Cheap whole-text reject (coverage-identical): the per-line
            // tokenized `line_contains_identifier` scan can only match when the
            // symbol appears as a substring on some line, which implies the
            // processed text contains it. A file lacking the substring entirely
            // cannot host a hit, so skip the per-line scan for this symbol.
            if !processed.contains(symbol) {
                continue;
            }
            let lines: Vec<usize> = plines
                .iter()
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
                hits_by_symbol
                    .get_mut(symbol)
                    .expect("symbol pre-seeded")
                    .push((file.clone(), lines));
            }
        }
    }
    for symbol in RETIRED_SYMBOLS {
        let hits = &hits_by_symbol[symbol];
        assert!(
            hits.is_empty(),
            "retired symbol `{symbol}` reintroduced in production source. \
             RETIRED_SYMBOLS guards against revival of deleted helpers; \
             remove the offending site or, if the symbol is legitimately \
             back in use as a new construct, justify and delete its entry \
             from RETIRED_SYMBOLS.\nHits:\n{hits:#?}"
        );
    }
}

// ===== Discriminating tests for the scanner's restriction discipline =====
//
// The two tests below pin the scanner's discipline contract:
//
//  * `scanner_ignores_test_files_and_inline_test_modules` constructs
//    a synthetic fixture tree where a retired identifier appears
//    inside (a) a `*_tests.rs` sibling, (b) a `tests/` subdirectory,
//    (c) a doc comment, and (d) an inline `#[cfg(test)] mod tests`
//    block — and asserts every variant is IGNORED by `preprocess` +
//    `is_test_file`.
//
//  * `scanner_detects_retired_identifier_in_production_source`
//    constructs a synthetic production file with a live reference to
//    a retired identifier and asserts the scanner FINDS it.
//
// Together they discriminate the scanner-restriction upgrade: a
// scanner that scanned `.md` files, comments, and test files
// indiscriminately fails the first test. The restricted scanner
// passes both.

#[test]
fn scanner_ignores_test_files_and_inline_test_modules() {
    // (a) `_tests.rs` sibling — `is_test_file` must return true.
    let tests_sibling = PathBuf::from("crates/example/src/foo_tests.rs");
    assert!(
        is_test_file(&tests_sibling),
        "_tests.rs sibling must be classified as a test file"
    );

    // (b) `tests/` subdirectory — `is_test_file` must return true.
    let tests_subdir = PathBuf::from("crates/example/src/tests/regress.rs");
    assert!(
        is_test_file(&tests_subdir),
        "file under a tests/ segment must be classified as a test file"
    );

    // (c) Doc-comment references must be erased by `preprocess`.
    let source_with_doc = "\
/// This doc comment mentions parse_type_annotation as historical context.\n\
//! Module-level doc mentions canonical_resolves_to_package too.\n\
pub fn foo() {}\n";
    let processed = preprocess(source_with_doc);
    assert!(
        !line_contains_identifier(&processed, "parse_type_annotation"),
        "preprocess must erase /// doc-comment references"
    );
    assert!(
        !line_contains_identifier(&processed, "canonical_resolves_to_package"),
        "preprocess must erase //! module-doc references"
    );

    // (d) Inline `#[cfg(test)] mod tests { ... }` blocks must be
    // erased by `preprocess` — references inside are test-only.
    let source_with_inline_tests = "\
pub fn live() {}\n\
\n\
#[cfg(test)]\n\
mod tests {\n\
    fn touch() {\n\
        let _ = parse_type_annotation();\n\
    }\n\
}\n";
    let processed = preprocess(source_with_inline_tests);
    assert!(
        !line_contains_identifier(&processed, "parse_type_annotation"),
        "preprocess must erase #[cfg(test)] mod tests {{ ... }} bodies"
    );
}

#[test]
fn scanner_detects_retired_identifier_in_production_source() {
    // Synthetic production fixture: a live reference to a retired
    // identifier OUTSIDE comments, OUTSIDE strings, and OUTSIDE a
    // `#[cfg(test)]` module. `preprocess` must keep it intact and
    // `line_contains_identifier` must find it.
    let live_production = "\
pub fn live_caller() {\n\
    let _ = parse_type_annotation(input);\n\
}\n";
    let processed = preprocess(live_production);
    assert!(
        processed
            .lines()
            .any(|l| line_contains_identifier(l, "parse_type_annotation")),
        "scanner must detect live retired identifier in production source"
    );

    // Identifier-boundary discipline: `parse_type_annotation_v2` is
    // NOT a hit on `parse_type_annotation` because the byte after the
    // needle (`_`) extends the identifier.
    let unrelated = "let _ = parse_type_annotation_v2();\n";
    let processed2 = preprocess(unrelated);
    assert!(
        !processed2
            .lines()
            .any(|l| line_contains_identifier(l, "parse_type_annotation")),
        "identifier-boundary matcher must not match `parse_type_annotation_v2`"
    );
}

/// Targeted call-site guard: `compare_type_expr_improvement` survives
/// as the shallow-vs-raised pick inside the projector
/// (`meta_resolve/projectors/published_reducer.rs`), so the
/// symbol-level `RETIRED_SYMBOLS` rail cannot pin it. The registry
/// publication path (`host_manage/component_meta_methods.rs`) must
/// carry ZERO call sites: a per-property / per-member improvement
/// pick there is the consumer-local second resolution path the
/// publication-demand contract forbids (one materialise + one
/// Navigate-mode stabilisation per property, carriers re-resolved by
/// the consumer through the shared resolver).
#[test]
fn component_meta_methods_has_no_improvement_pick_call_sites() {
    let path = workspace_root()
        .join("crates")
        .join("verter_session")
        .join("src")
        .join("host_manage")
        .join("component_meta_methods.rs");
    let source =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let processed = preprocess(&source);
    let hits: Vec<usize> = processed
        .lines()
        .enumerate()
        .filter(|(_, line)| line_contains_identifier(line, "compare_type_expr_improvement"))
        .map(|(idx, _)| idx + 1)
        .collect();
    assert!(
        hits.is_empty(),
        "`compare_type_expr_improvement` must have ZERO call sites in \
         component_meta_methods.rs (the registry publication path publishes one \
         Navigate-mode stabilisation per property, no improvement pick); found at \
         lines {hits:?} of {}",
        path.display()
    );
}

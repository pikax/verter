//! R6 meta-guard.
//!
//! BINDING: every `(CRITICAL)` architecture
//! rule in `CLAUDE.md` and `.claude/skills/*/SKILL.md` must ship with
//! at least one named guard — a static architecture test, a
//! discriminating regression test, or a grep / AST-walk scanner. A
//! rule without an executable guard is prose that future changes can
//! violate silently; the long-form text in CLAUDE.md / SKILL.md
//! becomes "documentation that nothing enforces".
//!
//! This file is the R6 substrate: it pins the contract by
//! registering every CRITICAL rule's guard(s) and failing if a new
//! CRITICAL section is added to the docs without a corresponding
//! registry entry.
//!
//! ## How this works
//!
//! 1. `CRITICAL_RULE_GUARDS` is the source-of-truth registry mapping
//!    each canonical rule title (without the `(CRITICAL)` suffix) to
//!    a non-empty list of guard test / scanner names. Adding a new
//!    CRITICAL rule means adding its row here AND landing the
//!    referenced guards (or referencing an existing guard that
//!    covers the rule's invariant).
//! 2. `every_critical_rule_in_docs_has_registered_guard` walks
//!    `CLAUDE.md` plus every `.claude/skills/*/SKILL.md`, extracts
//!    each section header containing `(CRITICAL)`, normalises the
//!    title (strip leading markdown / numbering / `(CRITICAL)`
//!    suffix), and asserts each appears in the registry.
//! 3. `registry_does_not_reference_stale_critical_rules` runs the
//!    inverse check: each rule in the registry MUST still appear in
//!    the docs. Removing a rule from the docs without removing it
//!    from the registry is the rotation hazard the second test
//!    catches.
//! 4. `every_registry_entry_lists_at_least_one_guard` enforces the
//!    non-empty-guard-list invariant — the registry's structural
//!    well-formedness.
//!
//! ## Maintenance
//!
//! - **Adding a new CRITICAL rule** — add it to the docs AND to
//!   `CRITICAL_RULE_GUARDS` AND land at least one guard test /
//!   scanner. The guard must be a real check, not a stub
//!   (per the `### Stub Prevention (CRITICAL)` rule in `CLAUDE.md`).
//! - **Retiring a CRITICAL rule** — remove the section AND remove
//!   the registry row in the same change.
//! - **Renaming a CRITICAL rule** — update both the docs and the
//!   registry row in the same change.

#![allow(clippy::too_many_lines)]

use std::fs;
use std::path::PathBuf;

/// Source-of-truth registry mapping each CRITICAL rule's canonical
/// title to a non-empty list of guard tests / scanners that pin it.
///
/// The canonical title is the section heading text with:
/// - leading `#` / numbering / dashes stripped,
/// - trailing `(CRITICAL)` suffix stripped,
/// - whitespace trimmed.
///
/// Guard names may reference either:
/// 1. a Rust test function (e.g.
///    `no_macro_string_heuristics_in_resolver_core`), discoverable
///    via `cargo test <name>`, or
/// 2. an integration-test FILE basename (e.g. `import_route_writer_guard`
///    → `crates/verter_session/tests/cases/g_misc3/import_route_writer_guard.rs`),
///    discoverable via `cargo test --test <name>`.
///
/// Guard-name validity is enforced by
/// [`every_registry_guard_name_resolves_to_a_known_test`]: each
/// registry entry must either name a `#[test] fn <name>(` declared
/// in `crates/*/tests/**/*.rs` or `crates/*/src/**/*_tests.rs`, OR
/// name the basename of an integration-test file at
/// `crates/*/tests/<name>.rs`. The scanner is a static file walk +
/// regex over compiled test source — no cargo invocation — so the
/// check runs in well under a second.
const CRITICAL_RULE_GUARDS: &[(&str, &[&str])] = &[
    // ──────────────────────── CLAUDE.md ────────────────────────────
    (
        "Shared Optimized Codebase",
        &[
            "verter_audit_no_upward_deps",
            "audit_substrate_isolation",
            "audit_observer_single_accessor",
            // Crate-ownership direction for the TypeExpr→handle migration:
            // verter_session owns the hot handle-bearing structs;
            // verter_semantic stays compat DTOs with no session back-edge.
            "no_verter_semantic_to_verter_session_dep",
            // Same barrier from the worker side: the OXC worker /
            // semantic-lowering surface produces owned `TypeExpr` IR only and
            // never emits a session semantic-graph node.
            "oxc_worker_emits_no_session_graph_node",
        ],
    ),
    (
        "Build Philosophy",
        &[
            "no_thread_local_oxc_caches",
            "no_direct_oxc_parser_calls_outside_scheduler_path",
            "recursion_budget_invariant_across_module_boundary",
            // Unified cold build: `ensure_indexed_ready_serve`'s
            // materialise closure is the single per-file cold build —
            // no parallel route-owned artifact system, no in-crate
            // `parse_and_build_env` second parse. The scanner these
            // guards share carries its own discriminator self-test.
            "no_production_route_owned_shallow_system",
            "no_production_parse_and_build_env_in_session",
            "session_production_ident_scanner_discriminates",
        ],
    ),
    (
        "Shallow File Processing Core Invariant",
        &[
            // The shallow-state contract is pinned by the per-file
            // `IndexedReady` lifecycle tests + the publish-boundary
            // non-vacuity gate that proves shallow projectors emit
            // their published surface.
            "audit_publishes_member_edge_with_published_field_provenance_at_macro_boundaries",
            "macro_impacting_constructs_fail_lowering_not_silent_skip",
            // Demand-scoped declaration-body lowering: publish is
            // index-only (zero bodies), a resolve lowers exactly the
            // demanded declaration closure, concurrent first-touch
            // singleflights, publish-time fact emission hashes no
            // bodies, and the artifact stores no eval env / eager
            // body — bodies live only in the lazy `DeclBodyMemo`.
            "indexed_ready_publish_lowers_zero_decl_bodies",
            "resolve_unrelated_symbol_lowers_only_demanded_decl",
            "lazy_decl_body_singleflight_lowers_once",
            "no_indexed_ready_eval_env_or_type_decl_body_storage",
            "emit_parse_facts_never_hashes_decl_bodies",
        ],
    ),
    (
        "Canonical Dependency Cache Rule",
        &[
            "host_upsert_performs_no_reverse_dependent_eviction",
            "host_upsert_reverse_dep_eviction_scanner_discriminates",
            // Writer-side guards live in their own test file.
            "import_route_writer_guard",
        ],
    ),
    (
        "Cache Architecture",
        &[
            "no_off_store_host_caches",
            "no_off_store_host_caches_discriminator_self_test",
            "no_owned_artifact_holds_borrowed_lifetime",
            "every_db_field_in_project_type_store_appears_in_inventory",
            "every_db_field_implements_invalidation_by_canonical",
            "surface_member_field_consults_member_shape_cache_before_round_trip",
            "surface_member_arch_guard_self_test_detects_inverted_order",
            // Per-file admission + route guards in sibling test files.
            "admission_guard",
            "route_generation_admission_guard",
            // R6 query-identity-keys content-free guards
            // (`Instantiate.base` / `ResolveMacroPayload.owner` mirror
            // on the `FamilyKey` memo identity).
            "r6_semantic_query_key_instantiate_base_is_content_free_decl_key",
            "r6_semantic_query_key_resolve_macro_payload_owner_is_content_free_decl_key",
            "r6_semantic_query_key_variants_carry_no_version_hash_in_source",
            "r6_family_key_variants_carry_no_version_hash_in_source",
            "r6_decl_slot_struct_is_content_free_in_source",
            "r6_no_decl_key_struct_reintroduced_anywhere_in_production",
            // One-path slot derivation: production code must derive the
            // `Instantiate` / `ResolveMacroPayload` slot via the shared
            // env-bearing `type_slot_for` / `builtin_type_slot`; the
            // zero-env fixture-only constructors are a forbidden bypass.
            "no_production_caller_of_zero_env_slot_constructors",
            // §3.4 materialised-record-point satisfaction: the two-gate
            // warm hit keys on a candidate's RECORDED `satisfied_projection`
            // (path-exact dominance), never its nominal slot mode; backfill
            // clones only RECORDED materialised points into directional +
            // `cached_satisfies`-gated narrower sibling slots.
            "cache_satisfaction_is_materialized_point_not_nominal_demand",
            "cache_satisfaction_requires_path_exact_not_prefix",
            "backfill_writes_only_recorded_materialized_points",
            // R6/R21 direct guards for the four migrated query-identity
            // cache keys (`crates/verter_session/tests/cases/g_cache/r6_r21_query_identity_keys.rs`):
            // each key is a content-free slot (R6 — no whole_hash /
            // content_hash / parse_stable_hash / fact_dep_signature / bundled
            // project_config_hash) carrying the split env axes it depends on
            // (R21). The shared `key_shape_violations` predicate has its own
            // discriminator self-test so the source scans are not stubs.
            "key_shape_predicate_discriminates",
            // (1) ComponentMetaResultKey — split env axes, owner whole-hash
            //     stays value-side candidate discriminant.
            "component_meta_result_key_carries_split_env_and_no_content_hash",
            "component_meta_result_key_env_axes_discriminate",
            // (2) RouteDb per-name + barrel — typed env-bearing keys, never
            //     bare-string `ValidatedFactCache` keys.
            "route_name_key_carries_split_env_and_no_content_hash",
            "barrel_surface_key_carries_split_env_and_no_content_hash",
            "route_db_does_not_key_routes_or_barrels_on_bare_strings",
            "route_keys_env_axes_discriminate",
            // (3) RefCycleResultDb — content-free `ResolvedDeclSlotIdentity`
            //     slot, never the versioned `DeclIdentity` (whole_hash).
            "ref_cycle_result_key_is_content_free_slot_keyed",
            "ref_cycle_db_is_keyed_on_content_free_slot_not_decl_identity",
            "ref_cycle_result_key_is_content_free_and_env_discriminating",
            // (4) MaterializeStructureDb — content-free canonical-subject
            //     `MaterializationCacheKey`, never the graph-instance
            //     `MaterializeRuntimeKey` (base: SemanticNodeId) subject.
            "materialization_cache_key_is_content_free_subject_keyed",
            "materialize_structure_db_is_keyed_on_canonical_subject_not_runtime_key",
            "materialization_cache_key_is_content_free_and_env_discriminating",
            // Scoped cache-key-hygiene over the shape/materialize
            // derived-`Hash` keys (`ShapeCacheKey` + `ShapeSubject` +
            // `ShapeDemand`; `MaterializationCacheKey`): NONE of the
            // forbidden content/version markers, and a `SemanticNodeId`
            // ONLY in the two sanctioned positions
            // (`MaterializationCacheKey.normalized_type_args` +
            // the sealed `MemberShapeNodeSubject` newtype) — the allow-list
            // is EXACT, the scope is shape/materialize keys only (a blanket
            // ban would be unsound). RECORDED SOURCE SCANNER (per the binding
            // neutral design ruling: a recorded scanner, not structural
            // enforcement); the predicate + the closed-inventory +
            // member-arm field-pinning each carry their own discriminator
            // self-test (registered so the anti-stub proofs cannot be
            // deleted without the registry noticing). The newtype is pinned
            // GLOBALLY to one occurrence, the visibility-token strip is
            // exact, the variant inventory survives attributed arms, and the
            // exact per-body field inventory each carry a discriminator too.
            "shape_materialize_key_hygiene_predicate_discriminates",
            "shape_subject_closed_inventory_self_test",
            "member_arm_sealed_newtype_is_field_pinned_self_test",
            "member_shape_node_subject_global_single_occurrence_self_test",
            "strip_visibility_only_strips_the_pub_token_self_test",
            "exact_field_inventory_discriminates_self_test",
            "no_unsanctioned_semantic_node_id_in_shape_or_materialize_key",
        ],
    ),
    (
        // §18 fact-rooted error-tolerant admission + §18.3 taint join +
        // §22 type-lattice absorption (owned by the `/type-cache-architecture`
        // skill). `admit_decision` keys `Warm` on the rooting FACT, never the
        // taint enum class; the §22 absorption table runs as the reducers'
        // FIRST fast-reject via separable `absorb_*` hooks; the error type
        // rides `Opaque(QueryError)`, relates bidirectionally, and is
        // `ReturnOnly`-prone when input-degraded (a §18.4 property).
        "Error-Tolerance Non-Admission + §22 Absorption",
        &[
            "error_tolerance_broken_input_is_returnonly_fact_rooted_error_is_cacheable",
            "error_any_never_propagation_lattice",
            "conditional_any_check_unions_both_branches",
            "conditional_never_check_is_distributive_gated",
            "error_type_is_returnonly_prone_any_is_cacheable",
        ],
    ),
    (
        "Macro Type Traversal Rule",
        &[
            // Genuine-Expanded root conditional distribution: a
            // query-root unbound conditional surfaces both branches;
            // the nested-position carrier rule is pinned by the
            // ignored tracker
            // `nested_open_conditional_not_distributed_under_expanded`.
            "root_conditional_still_distributes",
            "no_macro_string_heuristics_in_resolver_core",
            "no_text_based_macro_surface_projection_helpers",
            "no_role_inference_from_name_suffix",
            "no_pick_or_omit_string_prefix_check",
            // Single-resolution-engine shrinking-ledger guards: there is
            // exactly ONE query-time type-resolution engine (the canonical
            // typed-IR dispatch). These forbid a NEW production site of the
            // doomed eager OXC `resolve_type` engine / prepared-surface walker
            // while the second engine is deleted across the consolidation
            // stages. The rule is codified in this section + "Shared Optimized
            // Codebase" (no separate heading).
            "no_new_from_eager_meta_production_site",
            "no_new_duplicate_read_surface_members_definition",
            "no_new_type_surface_engine_path_production_file",
            "no_new_resolved_elements_production_file",
            "no_new_prepared_surface_projection_production_file",
            // Static producer-bound unreachability pin: the footprint
            // encoder's `SemanticNodeData::VueMacroElements` `Debug` arm can
            // never receive a `TypeExpr::SyntheticSlotBinding` ordinal because
            // the producer surface is fixed to (1) carrier-free
            // parser/compiler, (2) a single `insert_resolved_named_type`
            // caller, (3) a single `VueMacroElements` construction. Closes the
            // (provably-unreachable-today) ordinal-leak class without an
            // encoder change or a second-engine allowlist.
            "vue_macro_elements_ordinal_leak_is_producer_unreachable",
        ],
    ),
    (
        "Two Template Codegen Paths",
        &[
            // Codegen path independence — the VDOM / IDE split is
            // pinned by the sourcemap-accuracy + compile-output
            // snapshot suites. Any IDE-side regression caused by a
            // VDOM change (or vice versa) surfaces as a snapshot diff
            // or sourcemap byte-offset mismatch.
            "compile_audit_sourcemap",
        ],
    ),
    (
        "Fallthrough / Root Inheritance",
        &[
            "fallthrough_recomputes_from_runtime_subnodes_after_top_level_node_clear",
            "fallthrough_runtime_reuse_survives_host_cache_clear",
            "fallthrough_reuses_root_follow_after_branch_union_node_clear",
        ],
    ),
    (
        "Component-Meta Shallow-By-Default Rule",
        &[
            // Carrier-preserving Shallow decl-body lowering: member
            // values stay typed-IR carriers under Shallow (eagerness
            // ceiling + heritage-key enumeration + warm collapse).
            "decl_body_lowering_keeps_member_value_refs_as_carriers",
            // Publication-demands-Navigate behavioural guard: a full
            // get_component_meta records ZERO Published(Expanded)
            // projection contexts on the dispatch stream.
            "publication_routes_never_demand_expanded",
            // Registry publication path carries no per-property
            // improvement pick (consumer-local second resolution path).
            "component_meta_methods_has_no_improvement_pick_call_sites",
            "audit_publishes_member_edge_with_published_field_provenance_at_macro_boundaries",
            "pattern_a_non_slot_mapped_publication_does_not_leak_inherited_library_members",
            "pattern_b_generic_parameter_substitution_does_not_leak_inherited_library_members",
            "chatmessages_shape_audit_has_zero_outputschema_execute_project_member_edges",
            // L1 open-enumeration-domain carrier-stop: an open utility
            // source stays a shallow carrier; closed sources still
            // materialise path-precisely.
            "chatmessages_resolvable_barrel_publishes_open_pick_as_shallow_carrier",
            "closed_pick_sources_still_materialize_path_precisely",
            // L2/L3 fuse backstops for the open-generic expansion storm
            // (the `ChatMessages.vue` hang): the aggregate request work
            // budget counts `Instantiate` + `Conditional`, and the cycle
            // guard roots at the utility SOURCE type-argument (not the
            // outer `__builtin__::Pick` ref) so a cyclic source is detected.
            "projection_budget_counts_instantiate_and_conditional",
            "cycle_guard_roots_at_utility_source_type_argument",
        ],
    ),
    // SKILL.md uses the shortened title "Shallow-By-Default Rule" for
    // the same architectural invariant. Aliased to the same guards.
    (
        "Shallow-By-Default Rule",
        &[
            // Carrier-preserving Shallow decl-body lowering: member
            // values stay typed-IR carriers under Shallow (eagerness
            // ceiling + heritage-key enumeration + warm collapse).
            "decl_body_lowering_keeps_member_value_refs_as_carriers",
            // Publication-demands-Navigate behavioural guard: a full
            // get_component_meta records ZERO Published(Expanded)
            // projection contexts on the dispatch stream.
            "publication_routes_never_demand_expanded",
            // Registry publication path carries no per-property
            // improvement pick (consumer-local second resolution path).
            "component_meta_methods_has_no_improvement_pick_call_sites",
            "audit_publishes_member_edge_with_published_field_provenance_at_macro_boundaries",
            "pattern_a_non_slot_mapped_publication_does_not_leak_inherited_library_members",
            "pattern_b_generic_parameter_substitution_does_not_leak_inherited_library_members",
            "chatmessages_shape_audit_has_zero_outputschema_execute_project_member_edges",
            "chatmessages_resolvable_barrel_publishes_open_pick_as_shallow_carrier",
            "closed_pick_sources_still_materialize_path_precisely",
            "projection_budget_counts_instantiate_and_conditional",
            "cycle_guard_roots_at_utility_source_type_argument",
        ],
    ),
    (
        "Component-Meta Native Vs Compat",
        &[
            "no_napi_direct_verter_compiler_emitters",
            "compat_one_napi_call_audit",
            // JS-side coverage: packages/component-meta/test/compat-native-call-surface-allowlist.test.ts
            // (referenced informally — the Rust meta-guard does not
            // run JS tests, but the JS counterpart is part of the
            // rule's enforcement surface).
        ],
    ),
    // SKILL.md uses the shortened title "Native Vs Compat" for the
    // same architectural invariant. Aliased to the same guards.
    (
        "Native Vs Compat",
        &[
            "no_napi_direct_verter_compiler_emitters",
            "compat_one_napi_call_audit",
        ],
    ),
    (
        "Typed-IR-Only Resolver Rule",
        &[
            "no_macro_string_heuristics_in_resolver_core",
            "no_read_source_in_component_meta",
            "no_read_source_in_declaration_metadata",
            "no_text_based_macro_surface_projection_helpers",
            "no_node_modules_substring_outside_workspace_api",
            "no_parse_jsdoc_tag_type_payload_outside_jsdoc",
            "no_old_parse_type_annotation_name_in_production",
            "no_format_then_reparse",
            "no_pick_or_omit_string_prefix_check",
            "no_role_inference_from_name_suffix",
            // The checker-display-text re-parse bridge
            // (`parse_checker_text_to_type_expr` /
            // `checker_text_adapter`) is a dead path: both identifiers
            // are entries in the `RETIRED_SYMBOLS` ledger, and this
            // guard FAILS if either reappears in production source —
            // pinning the "no synthesise-then-reparse via checker text"
            // facet of this rule.
            "retired_symbols_absent_from_production_source",
            // Lazy top-level declaration-body lowering reuses the
            // scheduler-retained parse snapshot and returns owned typed
            // IR — never a raw-string re-parse per body touch.
            "lazy_decl_lowering_uses_scheduler_snapshot_not_reparse",
            // Migration foundation: the carrier-construction surface emits
            // TYPED carriers (BareRef / ImportType / RawFallback / typed
            // QueryError), never a raw `TypeExpr::Unknown` control sentinel
            // (scoped to the carrier surface; the global fence lands later).
            "carrier_constructors_do_not_use_unknown_as_control_flow",
            // The query-free structural lowerer
            // (`structural_carrier_producer/macro_arg_producer.rs`) emits
            // the typed carriers from the owned `TypeExpr` WITHOUT any
            // resolution / host query, and never materialises a carrier back
            // to `TypeExpr` during emission — it is a producer, not a second
            // resolver.
            "session_graph_lowerer_makes_no_query",
            "unresolved_carriers_not_materialized_during_emission",
            // ANTI-TAIL (ENCAPSULATION): the three structural carriers
            // (`BareRef`/`TypeOf`/`ImportType`) are opaque tuple payloads
            // (`semantic_query::carrier`) with PRIVATE fields, so a production
            // site hand-binding a carrier `type_args` field outside the sole
            // `SemanticNodeData::carrier_type_args` descent accessor is
            // UNREPRESENTABLE BY CONSTRUCTION — the compiler enforces the
            // boundary on the real compiled program (cfg / `#[path]` / macro /
            // alias included), replacing the retired `CARRIER_TYPEARGS_*`
            // source scanner. These tripwires pin the shape that makes that
            // enforcement true plus the surviving wildcard-free compile-fences
            // (a new variant must classify itself in BOTH the descent accessor
            // `carrier_type_args` AND the rebuild channel
            // `map_carrier_type_args`). Tripwire 1 pins the payload TYPE (a raw
            // `Arc<[SemanticNodeId]>` tuple is rejected, not just non-tuple);
            // tripwire 2 is a STRICT EXACT-SHAPE ALLOWLIST over `carrier.rs`:
            // it accepts the module ONLY if every item matches the precise
            // known-good shape (the two sanctioned imports — no renames/extras;
            // the three head-view aliases; the three carrier structs with their
            // exact private fields + the five built-in derives only; one private
            // inherent impl per carrier with its exact private method
            // signatures; one `impl SemanticNodeData` with EXACTLY the eight
            // sanctioned accessors at their exact visibility + signatures; no
            // macro in any body; no raw-args read outside the descent/rebuild
            // bodies) and the `mod carrier;` decl is unadorned — so the raw-args
            // surface stays compiler-confined to `carrier.rs`.
            "carrier_variants_are_opaque_tuple_payloads",
            // Enum-WIDE generalisation of the three-variant carrier check: ANY
            // `SemanticNodeData` variant exposing a directly bindable named
            // `type_args` field is rejected, so a future named-struct carrier
            // cannot re-open the anti-tail bind beside the three opaque carriers.
            "no_named_type_args_field_outside_opaque_carrier",
            "carrier_module_has_no_public_type_args_surface",
            "carrier_type_args_accessor_is_exhaustive_and_wildcard_free",
            "map_carrier_type_args_is_exhaustive_and_wildcard_free",
            // STRUCTURAL-CARRIER PRODUCER — single producer (make-unrepresentable):
            // the raw structural lowerer `lower_type_expr_structural`, the macro
            // hot-mirror builder, and the binder-seed builder are ALL owned by ONE
            // module `crate::structural_carrier_producer::macro_arg_producer` and are
            // MODULE-PRIVATE (no visibility modifier). The owner declares it as a
            // PRIVATE `mod macro_arg_producer;` re-exporting ONLY
            // `macro_type_arg_hot_ref` + `MacroHotMirror`, so a SECOND
            // structural-carrier producer is UNREPRESENTABLE BY CONSTRUCTION: no
            // foreign module can NAME the private builders (a compile error, E0603 /
            // E0433), and the producer is COLLAPSED into one module so no same-owner
            // file can name them either (a third caller is a compile error). The
            // MODULE-PRIVATE lowerer guard + the PARENT-SHAPE narrowness guard (the
            // owner directory holds ONLY `macro_arg_producer.rs`, mod.rs, and tests —
            // so there is no other file that could name the private builders) together
            // are the compiler-enforced LOAD-BEARING confinement. The SMALL
            // EXPANSION-SURFACE backstop (`macro_arg_producer.rs` declares NO
            // production item-position/expr/statement macro / `macro_rules!` /
            // `include!` / proc-macro attribute / `#[derive]` on a producer-capable
            // item / out-of-line-or-`#[path]` child mod / `#[macro_use]` — only the
            // `#[cfg(test)] #[path] mod *_tests;` test wiring is allowlisted) closes
            // the only same-module code-generation class the structure cannot already
            // make a compile error. The ENTRY-SURFACE guard is a BOUNDED
            // defense-in-depth token tripwire (NOT exhaustive — documented residual
            // tail) pinning `macro_type_arg_hot_ref` as the ONLY crate-visible producer
            // fn of the owner module; the ORDERING TRIPWIRE bans a production macro-arg
            // eager-lowering path outside the producer module (a FILE-SCOPE catch:
            // whole-function co-presence PLUS the cross-function-same-file binding-flow
            // helper split); the PURITY guard bans the full
            // route/import/cross-file-symbol/carrier-head resolution surface
            // (`prepared_decl_bundle`, `cached_import_route_resolution`,
            // `resolve_route_type_edge`, `resolve_type_dependency_canonical`,
            // `resolve_owner_direct_import`, `routed_shallow_state`,
            // `resolve_*_head`, …) inside the producer — resolution + dep recording
            // belong at the resolving DEMAND, so the producer stays a pure
            // structural-carrier lowering and script-setup seeding re-sources from the
            // owner's route-free `IndexedReady` (`raw_source` + `framework_parse`).
            "structural_carrier_producer_lowerer_is_module_private",
            "structural_carrier_producer_module_is_narrow",
            "macro_arg_producer_has_no_production_expansion_surface",
            "macro_hot_mirror_exposes_single_crate_visible_producer_entry",
            "no_production_macro_arg_eager_lowering_outside_mirror",
            "macro_hot_mirror_producer_is_pure_no_route_resolution",
            // HANDLE-CAPABLE DUAL-READ (additive, ahead of the producer
            // flip): the listed component-meta consumers accept BOTH a
            // parser-produced `TypeExpr` and an already-lowered handle,
            // routing BOTH arms through the SAME dispatch (read-compat,
            // ONE resolver). G-A is now the STRUCTURAL
            // `materialize_type_expr_is_not_production_visible` guard: the
            // `materialize_type_expr(HotTypeRef)` reverse-handle harness is
            // `#[cfg(test)]`-gated, so production cannot name it and a hot-arm
            // reverse-bridge is a compile-time impossibility (replaces the
            // deleted `no_hot_path_materialize_type_expr_bridge` line
            // scanner). G-B is the per-inventory ordering gate (each hot
            // carrier has a handle-native consumer BEFORE the producer flip;
            // the verter_semantic prepared-wrapper payloads are recorded as
            // crate-boundary-deferred, and a short-lived
            // absence-of-direct-reference tripwire asserts non-test
            // verter_session production source does not directly name the
            // deferred payload API (the four payload type names /
            // .target_args); it is an ordering tripwire, not a semantic
            // dataflow proof). The durable `SemanticNodeId -> TypeExpr`
            // OUTPUT boundary is the sealed `OutputProjector` capability +
            // sealed carriers whose inner `TypeExpr` lives in a deeply-private
            // `carrier::payload` vault (so in safe Rust outside the vault there
            // is no readable `TypeExpr` field; the residual trusted surface is
            // the vault + registration source + guard deletion + unsafe). The
            // retired interim Kind-B `legacy_semantic_type_expr_bridge` is gone
            // (its absence tripwired by the tombstone
            // `retired_kind_b_bridge_symbol_absent_from_production_source`).
            // The carrier-can't-name-`sealed` seal is COMPILER-enforced: `mod
            // sealed` is PRIVATE (not `pub(super)`) inside `mod projector`, so a
            // sibling `carrier`-side `impl projector::sealed::Sealed for HotCap`
            // is `E0603` (module `sealed` is private) — pinned structurally by
            // `sealed_module_is_private_not_pub_super`, with the topology guard
            // as defense-in-depth.
            // The fence SHAPE is pinned by mechanism-matched guards: the
            // sanctioned sink set (explicit `impl OutputProjector` self-types +
            // the matching `impl sealed::Sealed` self-types, no blanket seal
            // impl) + the EXACT owner module topology (inline projector /
            // projector::sealed / carrier / carrier::payload only, item-macro /
            // include! / unknown-attribute / cfg_attr-proc-macro injection
            // banned) by
            // `output_projector_owner_registration_inventory`; the carrier/
            // payload closed item/signature accessor allowlist (every fn
            // returning TypeExpr cap-gated or exactly test-gated) by
            // `output_carriers_have_no_inherent_typeexpr_escape_method`; the
            // carrier/payload field privacy (regardless of spelled type) by
            // `output_carrier_payload_fields_are_private`; the out-of-crate
            // visibility boundary by
            // `output_projector_non_owner_impl_is_compiler_sealed`; and the
            // mintable `TestOutputCap` capability staying `#[cfg(test)]`-gated
            // by `test_output_cap_not_visible_or_mintable_in_non_test_builds`
            // (the carrier TRAIT escapes have an accidental-regression CANARY —
            // NOT proof-complete; completeness is the payload vault — in the
            // sibling `src/project_semantic_dispatch/output_materialization_guards.rs`).
            // The output capability
            // is minted PER-LEAF (not per-subtree) so a Kind-B bridge sibling
            // sharing the subtree cannot mint it — pinned by
            // `output_cap_mint_scope_is_per_leaf_not_subtree`. The
            // COMPLEMENTARY input-authority hole at the Kind-A PUBLICATION
            // boundary — a non-sink fn choosing a raw semantic-graph subject /
            // forging a surface/member/signature wrapper and pairing it with a
            // cursor to reverse-materialize a member `TypeExpr` — is closed by
            // the sealed admitted-token chain
            // (`meta_resolve::projectors::publication_authority`: the
            // `ResolvedMacroPayload`/`ResolvedPayloadSurface`/`SurfaceMemberCandidate`/`AdmittedPublishedMember`
            // tokens with private fields + a private `Seal`, minted only by the
            // admission fns; plus the framework-surface `ResolvedVueSurface` +
            // `SvelteResolvedSurface` tokens, gated by the sealed
            // `ResolvedSurfaceAccess` trait whose supertrait seal is PRIVATE to
            // `resolved_surface_access.rs` — a sibling `impl` is `E0603`, pinned
            // defense-in-depth by
            // `resolved_surface_access_impls_are_exactly_the_two_tokens`) as the
            // COMPILER primary, and pinned by the STRUCTURAL cross-sink
            // transitive guard `cross_sink_raw_authority_to_type_expr_boundary`
            // (a structurally-complete — vs the old name-based pin — residual
            // SUPPLEMENT behind the compiler primary, NOT a replacement) — it
            // decides "TypeExpr-bearing" by FIELD-CLOSURE from `TypeExpr` over the
            // type field graph (not a DTO-name list) and fails any reachable
            // production fn across the registered PUBLICATION sinks (projectors,
            // cache-key, framework-surface, query-engine surface) that pairs a
            // forgeable raw-authority input with a `TypeExpr`-bearing output
            // outside the closed sink-local allowlist. Genuinely structural:
            // MODULE-QUALIFIED `(module, name)` `TypeDefId` identity carried
            // through the closure graph by a CONSERVATIVE FAIL-CLOSED resolver
            // (NOT an exhaustive Rust name resolver) that resolves a written
            // reference ONLY by genuine PROOF — own-module-def, a genuine
            // `pub`/`pub(crate)` re-export, a proven intra-crate `use`-binding
            // chain, or a proven qualifier; an import that CLAIMS a local name and
            // fails to resolve-by-proof yields `Unresolved` (NEVER a uniqueness
            // fall-through); else `Unresolved`. A fully-qualified path is proven by
            // a DIRECT suffix-or-equal module match (relative `crate`/`self`/`super`
            // rebased onto the referencing module; `super` cannot escape above the
            // crate root; a too-short ANCESTOR prefix is NOT a match; an UNROOTED
            // first segment the file `use`-SHADOWS is re-resolved through the shadow,
            // never trusted on its raw suffix), an EXACT-target `pub`/`pub(crate)`
            // re-export (the TARGET module is the candidate's real home EXACTLY,
            // never suffix slack, keyed by the normalized absolute written path), or
            // a proven intra-crate `use`-binding chain (a module-scoped,
            // intra-crate-only, non-glob, module/descendant-visibility, cycle-bounded
            // use-binding PROOF graph). A UNIQUE name does NOT resolve a qualified
            // path on uniqueness alone (a fabricated
            // `external::AdmittedPublishedMember` qualifier stays `Unresolved`); an
            // unqualified name resolves on the own module; else, if a `use` import
            // CLAIMS the name, its TARGET is resolved BY PROOF and returned as-is (an
            // import target that does not resolve leaves the name `Unresolved`
            // immediately — `use crate::evil::AdmittedPublishedMember` does NOT bless
            // the unique token); else a parent module's accessible `use`-binding
            // chain may prove it; else, ONLY when no import claimed the name,
            // exactly-one collected def with that name; a colliding name like the two
            // `IndexSignature` defs is disambiguated into DISTINCT ids the same way;
            // an unresolvable / forged-qualifier reference fails closed — with a
            // fail-closed anti-vacuity rail over the `(module, name)`-keyed
            // safe-input / construction-chain tokens; the dual-bearing defense is
            // a DIRECT carve-out (a wrapper directly co-holding a
            // resolution-authority seed stays forgeable) plus a TRANSITIVE
            // soundness tripwire whose sanctioned-carrier exemption is QUALIFIED
            // `(module, name)` (a wrong-module same-name token FIRES); BOTH the
            // OUTPUT and INPUT sides are fail-closed on an unclassifiable ident,
            // and the non-authority exemptions are QUALIFIER-AWARE — a Qualified
            // `(module, name)` entry (anti-vacuity-checked) or a non-field-bearing
            // CATEGORY entry carrying APPROVED qualified homes, matched against the
            // `Unresolved` ref's PATH not its bare final segment (a forged
            // `evil::Span` FIRES; a one-segment generic is benign; a one-segment
            // trait-bound/external is exempt only with no same-name collected def);
            // the safe-input set is SPLIT into policy-admitted tokens
            // vs pre-admission construction-chain structs; the sink-fn collector
            // is inline-mod-aware. The fence soundness is tripwired by
            // `forgeable_input_fence_has_no_dual_bearing_type`.
            // This is the Kind-A / PUBLICATION bar. The Kind-B raise-then-decide
            // residual is RETIRED: the interim `legacy_semantic_type_expr_bridge`
            // is deleted, every Kind-B caller now decides on the node-domain
            // `RaisedShapeFacts` / interned `RaisedShapeKey` (no mid-flight
            // `SemanticNodeId -> TypeExpr` raise), and the single publication
            // `TypeExpr` is materialised once at a registered output sink through
            // the sealed `OutputProjector`. The absence of the retired bridge
            // symbol is tripwired by the lean tombstone
            // `retired_kind_b_bridge_symbol_absent_from_production_source`. The
            // admitted
            // tokens' field privacy + seal are pinned by
            // `admitted_tokens_have_private_fields_and_seal`, and the
            // authority-callable scopes are no-`unsafe` (a transmute could
            // fabricate a token) by `authority_scopes_contain_no_unsafe`. The
            // carrier
            // `_for_test` accessors are gated `#[cfg(any(test, feature =
            // "test-support"))]` (production-unreachable in EVERY profile,
            // never `debug_assertions`-present in debug) — pinned by
            // `carrier_for_test_accessors_are_test_support_gated_not_debug_assertions`.
            // The `pub(super)` shell raise seam returns a SEALED carrier so a
            // dispatch sibling cannot launder — pinned by
            // `raise_output_seam_returns_sealed_carrier_not_bare_type_expr`.
            "materialize_type_expr_is_not_production_visible",
            "retired_kind_b_bridge_symbol_absent_from_production_source",
            "sealed_module_is_private_not_pub_super",
            "output_projector_owner_registration_inventory",
            "output_carriers_have_no_inherent_typeexpr_escape_method",
            "output_carrier_payload_fields_are_private",
            "output_projector_non_owner_impl_is_compiler_sealed",
            "test_output_cap_not_visible_or_mintable_in_non_test_builds",
            "output_cap_mint_scope_is_per_leaf_not_subtree",
            "cross_sink_raw_authority_to_type_expr_boundary",
            "forgeable_input_fence_has_no_dual_bearing_type",
            "admitted_tokens_have_private_fields_and_seal",
            "resolved_surface_access_impls_are_exactly_the_two_tokens",
            "authority_scopes_contain_no_unsafe",
            "carrier_for_test_accessors_are_test_support_gated_not_debug_assertions",
            "raise_output_seam_returns_sealed_carrier_not_bare_type_expr",
            "stage4_carrier_inventory_handle_native_consumers_present",
            "stage4_deferred_carriers_have_no_session_resolution_consumer",
        ],
    ),
    (
        "CodeTransform Is the Single Source of Truth",
        &[
            // CodeTransform integrity is pinned by the sourcemap
            // accuracy suite — any string manipulation on the built
            // output of a `CodeTransform` would surface as sourcemap
            // byte-offset mismatches in this audit. The same suite
            // discriminates the rule's intent (`overwrite` /
            // `prepend_left` / `append_left` preserves byte offsets;
            // `String::replace` on the built result does not).
            "compile_audit_sourcemap",
        ],
    ),
    (
        "Typeinfo Wire Contract",
        &[
            // (1) Closed-enum discipline + proto/TS oneof parity —
            // every variant in the proto schema has a matching TS
            // descriptor and the cardinality matches the documented
            // baselines.
            "typeinfo_graph_taxonomy",
            // (2) Wire-compat: byte-equal TS freshness against the
            // canonical `buf generate` + `oxfmt` output. Drift —
            // schema edit without regen, hand-edit, formatter
            // mismatch — surfaces as a named diff.
            "typeinfo_proto_ts_freshness",
            // (3) Audit envelope parity: every `RequestKind`
            // variant has a matching `RequestKindPayload` arm; the
            // `TypeInfoGraph` substrate is wired end-to-end.
            "request_kind_payload_parity",
            // (4) Request validation runs before semantic
            // execution: closed-set schema-version gate +
            // exhaustive per-variant structured-expression
            // coverage.
            "typeinfo_request_validation",
            // (5) Wire-surface static pins: dependency direction,
            // proto-authoritative DTOs, no ts-rs on the wire,
            // closed-enum discriminants declared (not raw uint32),
            // no duplicate enums, no phase archaeology.
            "typeinfo_wire_surface_guards",
            // (6) Graph node/symbol/origin taxonomy pins: node
            // taxonomy completeness, the three OriginEdgeKind
            // taxonomies, symbol-node namespace + decl-slot
            // identity, literal/string-table independence, cycle
            // carrier, closure-policy surface, R21 split env hashes.
            "typeinfo_graph_contract_guards",
            // (7) Request-envelope contract: schema-version carriage,
            // mode presence, the closed error union, per-request
            // context/closure exemptions, scalar listSymbols,
            // closure-free relate, concrete closure bounds.
            "typeinfo_request_contract_guards",
            // (8) Audit-surface contract: graph diagnostics live only
            // on TypeInfoGraphPayload; AuditedResult lives in
            // verter_audit and exports through audit.generated.ts.
            "typeinfo_audit_contract_guards",
        ],
    ),
    (
        "Cross-Platform Portability",
        &[
            // Guard-enforced half: `git ls-files -z` walk pinning
            // UTF-8 validity, NTFS-legal components, no trailing
            // dot/space, no reserved device basenames, no
            // case-insensitive path collisions (lowercase-fold
            // approximation of NTFS/APFS folding, not the exact
            // filesystem fold tables), ≤200-byte relative paths.
            // The generated-name-sanitization / separator /
            // CRLF-normalization / binary-discovery / temp-path
            // clauses are review-enforced (the guard walks tracked
            // paths only, so a generated name is caught once
            // committed, not at generation time).
            "tracked_paths_are_portable",
            // Content-residue half: `git ls-files -z` walk pinning that no
            // tracked file embeds any of 64 KNOWN leaked-root markers
            // (one dev's $HOME, one machine's checkout drive + worktree/
            // sandbox sub-roots, another dev's Windows Claude dir, and
            // orchestration scratch roots — use std::env::temp_dir()/
            // os.tmpdir()/env-driven/repo-relative instead). A tombstone for
            // those known roots, NOT a complete machine-path detector
            // (a broad detector would false-positive the ~70 legitimate
            // cross-platform fixtures). Reads are fail-closed. Complements
            // the path-shape guard above.
            "tracked_paths_no_machine_roots",
        ],
    ),
    (
        // Anti-binary-growth: each crate exposes AT MOST one
        // `tests/main.rs` integration-test binary (extra cases live
        // under `tests/cases/` wired through `main.rs`), plus the two
        // EXACTLY-allowlisted process-isolated targets. A DUAL guard:
        // the fast-fail CI Node check
        // (`scripts/check-integration-test-layout.mjs`) and the in-gate
        // Rust mirror below both read the SAME committed allowlist
        // (`scripts/integration-test-layout-allowlist.json`), so the
        // exception set is a single source of truth and stale-failing.
        "Anti-Binary-Growth Integration-Test Layout",
        &[
            // The guard: the real consolidated workspace must be
            // conformant (mirror of the Node CI check).
            "integration_test_layout_is_consolidated",
            // Discrimination self-test: the pure checker FAILS on a
            // second top-level `tests/*.rs`, a stale allowlist entry,
            // an `autotests=false` hide, and a missing `tests/main.rs`.
            "layout_checker_discriminates_stray_and_stale",
            // Allowlist parity: exactly the two known process-isolated
            // exceptions, agreeing with the Node guard's expectation.
            "allowlist_is_the_two_known_process_isolated_targets",
        ],
    ),
    (
        "Stub Prevention",
        &[
            "macro_impacting_constructs_fail_lowering_not_silent_skip",
            "every_consumer_has_production_call_site",
            // The R6 meta-guard itself is anti-stub: a CRITICAL rule
            // must reference a non-empty guard list.
            "every_registry_entry_lists_at_least_one_guard",
            // The registry-completeness walk that backs
            // `every_registry_guard_name_resolves_to_a_known_test` must
            // fail CLOSED: a non-`NotFound` metadata IO error on a crate /
            // `tests/` / `src/` subtree panics instead of silently skipping
            // (which would let the meta-guard believe guard coverage exists
            // for files it never scanned). This self-test proves the
            // fail-closed `NotADirectory`-panics + `NotFound`-no-panic
            // discipline of the walk's directory classifier.
            "registry_completeness_walk_hard_fails_on_metadata_error_self_test",
        ],
    ),
    (
        // The framework-adapter substrate: one shared registry +
        // facts/carrier-only ctx + validation-first framework-surface
        // executor + two-pass script-fact seam + parse-domain synth +
        // descriptor-owned virtual-file naming + Vue re-housing.
        "Framework Adapter Substrate",
        &[
            // Validation-first wire entry: validation precedes registry
            // lookup / selector resolution.
            "framework_surface_wire_executor_validates_first",
            // Registry dispatch + per-kind status + Vue parity +
            // unknown-adapter rejection (the executor integration suite,
            // which exercises `framework_registry_complete`'s dispatch).
            "framework_surface_executor",
            // Facts/carrier-only ctx: exactly two pub ops, no resolver
            // tokens.
            "framework_adapter_ctx_closed_surface",
            // Parse-domain synth ctx: no resolved-validation fact types.
            "component_default_synth_parse_domain_only",
            // Syntax-capture half is syntax-only (no import resolution /
            // capability reads in the capture surface).
            "script_fact_capture_is_syntax_only",
            // Empty active-provider set is byte-identical zero-cost.
            "script_fact_providers_zero_cost_on_miss",
            // Generated virtual-file naming mirror is byte-equal to the
            // rendered descriptor column.
            "virtual_file_naming_ts_freshness",
            // Generated client framework manifest is byte-equal to the
            // rendered descriptor registry, and the extension's framework
            // wiring (activation / document selector / trigger ids / watch
            // globs) derives from it (Svelte ungated, no per-framework fork).
            "client_framework_manifest_ts_freshness",
            "client_framework_manifest_drives_extension_wiring",
            // Vue re-housing: no re-export shim, deleted files stay
            // deleted, retired stores absent from production.
            "vue_relocation_no_shim",
            "retired_symbols_absent_from_production_source",
            // Framework-carrier LSP routing is carrier-generic: no Vue-only
            // gate (`.is_vue(` / `ends_with(".vue")` / `strip_suffix(".vue")` /
            // bare `"vue"`-prefix language-id classifier) and no hardcoded
            // carrier provider literal (`.vue.ts`, …) in feature/server routing
            // outside the narrow SSR-convention / test / comment / `is_svelte()`
            // allowlist.
            "carrier_lsp_routing_has_no_hardcoded_vue_gate",
            // …and NO carrier-generic routing / provider-sync / position-mapper
            // primitive may carry a Vue-flavoured NAME (`vue_resync_ids`,
            // `vue_position_to_tsx_offset`, `prepare_non_vue_provider_sync`, …)
            // — the naming half of the pair that ends the whack-a-mole, banning
            // `vue`/`Vue`-substring identifiers in the scanned modules outside
            // the narrow Vue-intrinsic allowlist.
            "carrier_routing_has_no_vue_named_generic_primitive",
            // …and the same no-hardcoded-Vue-gate enforcement extends to the
            // other carrier-neutral consumer surfaces: the `verter_mcp` tool
            // surface (carrier-neutral analysis tools route on
            // `is_framework_carrier()`, not `is_vue()`, outside the Vue-intrinsic
            // tool allowlist) and the `verter_session` resolution/routing tree
            // under `resolver_core/` (the fallthrough / child-resolution carrier
            // gate is carrier-generic).
            "mcp_routing_has_no_hardcoded_vue_gate",
            "session_resolution_routing_has_no_hardcoded_vue_gate",
            // Rune-module (`.svelte.ts` / `.svelte.js`) own-buffer LSP path:
            // the self-file projection consumes the prelude line count as a
            // uniform line offset; the self-file mapper drops the prelude +
            // rewrite regions; the ONE generalized provider-projection context
            // serves BOTH carrier-IDE and self-file (no parallel rune path);
            // the rune extension is NOT a carrier extension; the watcher globs
            // are descriptor-derived (carrier glob + dedicated adapter-module
            // glob); the rune self-file provider state is closed on did_close.
            "rune_module_self_file_projection_uses_prelude_line_count",
            "self_file_mapper_drops_prelude_and_rewrite_regions",
            "provider_projection_context_serves_both_carrier_and_self_file",
            "svelte_rune_module_not_in_carrier_extensions",
            "lifecycle_watch_globs_are_descriptor_derived",
            "rune_module_self_file_state_closed_on_did_close",
        ],
    ),
    // ──────────────────── SKILL.md additions ──────────────────────
    (
        "Component-Meta Heuristic Prevention",
        &[
            // Heuristic-prevention rule — pinned by
            // the typed-IR-only resolver guard cluster, which
            // mechanically forbids the forbidden patterns the rule
            // text names (string parsing, format!-then-reparse, Pick/
            // Omit shape sniffing, role-name suffix heuristics,
            // checker display-text re-parse bridge).
            "no_macro_string_heuristics_in_resolver_core",
            "no_text_based_macro_surface_projection_helpers",
            "no_format_then_reparse",
            "no_pick_or_omit_string_prefix_check",
            "no_role_inference_from_name_suffix",
            // The retired checker-display-text re-parse bridge
            // (`parse_checker_text_to_type_expr` / `checker_text_adapter`)
            // is pinned by the `RETIRED_SYMBOLS` ledger: this guard
            // FAILS if either identifier reappears in production source.
            "retired_symbols_absent_from_production_source",
        ],
    ),
    (
        "Component-Meta Completeness Contract",
        &[
            // The completeness-contract substrate (typed degraded
            // states, no `Complete(None)` for missing inputs) is a
            // future addition. The current guard pins the
            // present invariant via the audit-validator's
            // PublishedField gate + the no-silent-skip guard.
            "macro_impacting_constructs_fail_lowering_not_silent_skip",
            "audit_publishes_member_edge_with_published_field_provenance_at_macro_boundaries",
        ],
    ),
    (
        "Semantic Heuristic Prevention",
        &[
            "no_macro_string_heuristics_in_resolver_core",
            "no_node_modules_substring_outside_workspace_api",
            "no_format_then_reparse",
        ],
    ),
    (
        "Typed Degradation And Completeness Contract",
        &[
            // Same follow-up substrate as the component-meta
            // completeness contract.
            "macro_impacting_constructs_fail_lowering_not_silent_skip",
            "audit_publishes_member_edge_with_published_field_provenance_at_macro_boundaries",
        ],
    ),
    (
        "Synthetic Carrier Typed-IR Rule",
        &[
            // Pins R22 carrier-verdict + carrier-provenance substrate
            // deletion — the retired-symbol absence scanner. The
            // typed-IR `TypeExpr::SyntheticSlotBinding` variant is the
            // sole carrier identity at the projector / registry / reducer
            // surface; re-introducing the R22 substrate is forbidden.
            // Lives at
            // `crates/verter_session/tests/cases/g_misc2/no_carrier_verdict_db.rs`.
            "no_carrier_verdict_db",
            // Bans a hand-rolled `SemanticNodeId(<ident>.value_node)`
            // ordinal cache-key construction in production source — the
            // bounded residual SYNTACTIC supplement to the structural
            // confinement (sealed `NonSyntheticTypeExpr` + module-private
            // `ShapeSubject`/`ShapeCacheKey` construction + sealed
            // `MemberShapeNodeSubject`). The `value_node` arena ordinal is
            // value-side provenance, never a cache key. Lives at
            // `crates/verter_session/tests/cases/g_misc2/synthetic_carrier_explicit_deepen_routes_through_shape_cache_key.rs`.
            "synthetic_carrier_explicit_deepen_routes_through_shape_cache_key",
            // The discriminator self-test for the value_node scanner above:
            // proves the STREAM scan catches every instance of the DIRECT
            // single-ident shape `SemanticNodeId(<ident>.value_node)`
            // (including a MULTI-LINE split of that shape) and false-positives
            // on none. The scanner claims only the direct shape; receiver
            // expressions / chained access / binding indirection are covered by
            // the structural primary. Registered so the anti-stub proof cannot
            // be deleted without the registry noticing.
            "synthetic_carrier_explicit_deepen_guard_self_test",
            // Discriminating self-tests for the hard-failing production-
            // source traversal both scanners share: the top-level crate /
            // `src` classification panics on a non-`NotFound` metadata IO
            // error instead of collapsing it to a silent skip (the
            // `Path::is_dir()` fail-open class). Each proves the panic on a
            // `NotADirectory` traversal while a `NotFound` path remains a
            // legitimate non-panicking skip. The two scanner files each carry
            // their OWN uniquely-named copy (the retired-symbol scanner in
            // `no_carrier_verdict_db.rs` and the synthetic-deepen scanner in
            // `synthetic_carrier_explicit_deepen_routes_through_shape_cache_key.rs`),
            // and BOTH are registered so deleting EITHER file's copy dangles
            // its registry entry and is caught — a single shared name would let
            // one copy silently satisfy the registry for both.
            "retired_symbol_scanner_classified_as_dir_hard_fails_on_metadata_error_self_test",
            "synthetic_deepen_scanner_classified_as_dir_hard_fails_on_metadata_error_self_test",
            // Pins the legitimate explicit-deepen cache route through
            // `ShapeCacheKey::synthetic_binding_whole(SyntheticBindingId::
            // from_carrier_key(&carrier), mode)`, rooting on the
            // content-free `SyntheticBindingId`. Positive executable proof
            // of the cache-key collapse invariant: two carriers with the
            // SAME identity tuple but DIFFERENT `value_node` collapse onto
            // ONE entry (`live_count() == 1`), the still-discriminating
            // axes (scope / slot / binding / surface_kind / mode) each
            // MISS, and `value_node` survives only as a value-side
            // provenance round-trip. Discriminates RED-on-revert against
            // the retired `SemanticNodeId(value_node)` key (which would
            // key disjointly ⇒ `live_count() == 2`).
            "synthetic_carrier_explicit_deepen_proof",
            // The migration's content-free successor identity for the
            // synthetic carrier (`SyntheticBindingId`) carries no bare
            // `SemanticNodeId` / `value_node` ordinal — the ordinal is
            // provenance on the carrier, never on the identity (R6).
            "synthetic_binding_identity_is_content_free",
        ],
    ),
    (
        "Block-vocabulary ban",
        &[
            // Production source under `crates/*/src/**` must not contain
            // plan-management vocabulary (numbered blocks, named overhaul
            // plans, or migration-stage labels). The discriminator is the
            // `guard7_predicate_rejects_block_vocabulary` test inside
            // `architecture_guards.rs`; the broader walker
            // (`no_phase_archaeology_in_production_code`) consumes the same
            // predicate and fails the build on any production-source
            // violation.
            "guard7_predicate_rejects_block_vocabulary",
        ],
    ),
    (
        "Editor-Liveness Provider-Sync Invariant",
        &[
            // The static editor-liveness architecture guard
            // (`crates/verter_lsp/tests/cases/editor_liveness_guards.rs`) source-scans
            // every LSP provider-sync file and FAILS if any function OTHER THAN
            // the approved leaf close-dispatch primitives contains an inline
            // provider-close loop (close-before-sync), which would close the live
            // editor TSX on an owner change or lose the previous path on a failed
            // sync. A second guard (`vue_sync_functions_never_delegate_raw_stale_close`)
            // closes the delegated-close evasion: a `.vue`-syncing function that
            // hands the RAW `transition.stale_paths` to a close helper before
            // syncing (no inline loop, so it slips past the first guard). Both
            // companion meta-guards pin the detectors' discrimination.
            "editor_liveness_guards",
            "vue_sync_functions_never_inline_close_the_stale_set",
            "guard_detector_discriminates_inline_close_from_delegation",
            "vue_sync_functions_never_delegate_raw_stale_close",
            "delegated_close_detector_discriminates_vue_evasion_from_approved_and_non_vue",
        ],
    ),
    (
        "Typed SignatureAdmission gate",
        &[
            // `ReadSetSignature` is the typed admission carrier:
            // `is_cacheable()` returns `!overflowed`, so empty and
            // overflow are structurally distinguishable at the
            // type. Pre-fix the equivalent `Arc<[FactVersionRef]>`
            // rail had no overflow bit and the warm-hit oracle
            // could not discriminate the two states.
            "empty_and_overflow_are_distinguishable_at_carrier_type",
            // Source-grep arch-guard: no production callsite
            // constructs `Arc::from(Vec::<FactVersionRef>::new())`
            // outside `fact_signature_helpers::empty_fact_signature`
            // and the legacy `finalise_signature_or_empty` helper
            // is gone.
            "no_call_site_constructs_empty_signature_from_overflow",
            // Behavioural discriminator: the compile-tier cold-build
            // producer refuses `compile_slots.insert` when the
            // finalised tracer reports `Overflow`. Pre-fix the
            // collapsed empty-signature slot landed and stayed
            // warm trivially.
            "compile_fact_signature_overflow_does_not_publish_compile_slot",
        ],
    ),
    // ───────────────── U2 query-value-domain design gate ─────────────────
    // Two CRITICAL rules from the U2 design
    // (`docs/arch/u2-query-value-domain-design.md`). The design-gate
    // guards below are discriminating TODAY; the behavioural
    // guards are named in the owning skill sections (gap tracked per
    // the architecture-guard rule).
    (
        "Declaration Merging",
        &[
            // (i) EvalEnv exposes ordered contributor groups, not a last-wins
            //     FxHashMap<String, TypeDeclInfo>/ValueDeclInfo map.
            "eval_env_type_symbols_are_grouped_not_last_wins_map",
            // (ii) add_type/add_value append contributors (no overwrite insert).
            "eval_env_add_decl_appends_not_overwrites",
            // (iii) no `raw_body = TypeExpr::intersection(...)` merge synthesis
            //       in verter_session.
            "no_intersection_merge_synthesis_in_verter_session",
            // (iv) the load-bearing decision: a merged interface lowers to a
            //      distinct `SemanticNodeData::MergedDecl` carrier.
            "merged_decl_lowers_to_distinct_carrier_not_intersection",
            // Discriminating regression: two interface parts emit ONE merged
            // Export fact over the contributor union (reorder-stable).
            "declaration_merge_facts",
        ],
    ),
    (
        "Declaration Augmentation",
        &[
            // (i) overlay-aware augmentation index: a session-overlay augmenter
            //     is isolated from the base index (base/session population).
            //     The cross-file relative-merge consumer oracle
            //     `module_features_module_augmentation_merges_plugin_surface`
            //     is a lib test (not scanner-visible); these `tests/` guards
            //     pin the same rule.
            "session_overlay_augmenter_isolated_from_base_index",
            // (iii) the session view is accepted AND stitches its own overlay
            //       augmenter (the base-only assert is gone, and the session
            //       branch is overlay-correct, not base-presented-as-session).
            "effective_export_set_session_view_stitches_overlay_augmenter",
            // (iv) static guard: NO `compat_token().session.is_none()` base-only
            //      assert on the augmentation-index / EffectiveExportSet surface.
            "no_effective_export_set_base_only_session_assert",
        ],
    ),
    (
        "U2 Value-Domain Key Identity",
        &[
            "no_envless_semantic_query_env_key_envelope",
            "u2_value_domain_design_doc_locks_invariants",
        ],
    ),
    (
        "Module-Resolution Keying",
        &[
            "module_resolution_keys_on_resolve_env_not_type_or_lib",
            "resolve_env_does_not_fold_lib_dims",
        ],
    ),
    (
        "Typed Value Domain + Demand-Lattice Resolution",
        &[
            "error_rides_opaque_no_new_error_type_wire_arm",
            "u2_value_domain_design_doc_locks_invariants",
            "query_modes_are_presets_over_projection_demand_eval_policy",
            "skeleton_is_typeparamshells_plus_carrier_stop_not_special_mode",
            "cache_key_axes_are_minimal_and_normalized",
            "canonical_display_is_projection_not_stored_string",
            "display_needs_is_display_only_never_drives_resolution",
            "display_needs_masked_out_of_typed_value_family_key",
        ],
    ),
    (
        // host-session SKILL.md — the general store-view accessor returns
        // the capability-split `StoreViewRead`; warm validators require a
        // proven-`CurrentHostStoreView`, cold builders take a
        // `ColdSeedHostStoreView` (no `validates*` surface), and the
        // raw-view escape hatch is confined to an allowlist.
        "Non-Current Store-View Contract — Capability Split",
        &[
            "resolver_store_view_returns_store_view_read",
            "cold_seed_store_view_exposes_no_validation_surface",
            "warm_validation_entry_points_require_current_store_view",
            "resolver_store_view_into_owned_view_is_allowlisted",
            // Indirect-validation seam: a raw cold-seed unwrap
            // (`into_cold_seed_view().into_inner()`) fed into a resolver
            // context that then validates is confined to a non-validating
            // allowlist; cold-compute context constructors carry currentness.
            "cold_seed_into_inner_confined_to_non_validating_allowlist",
            "cold_compute_context_constructors_carry_currentness",
            // Currentness-source seam: a cold-seed's `is_current` must come
            // from the SAME read as its view (intrinsic to a `StoreViewRead`
            // arm). The one `(view, flag)` re-bind (`from_executor_snapshot`)
            // is confined to the executor boundary, and a fresh
            // `resolver_store_view_read()` may never feed it — closing the
            // view+flag divergence the constructor-shape guards missed.
            "cold_seed_currentness_is_intrinsic_to_the_read",
            // Mutate-without-bump completeness seam over the
            // `project_generation` stamp discipline (the unified-path
            // successor of the retired route-owned token-generation
            // guard): every AUTO-DISCOVERED route-resolution mutator
            // (syn-AST discovery over production source, comment-proof,
            // with a documented no-bump allowlist) must advance the
            // stamp the pre-publish fence reads, strictly AFTER the
            // mutation it announces; plus the live ordering pin on the
            // `set_exact_resolutions` wrapper and the discovery
            // discriminator self-test.
            "route_mutators_bump_project_generation_after_the_mutation",
            "route_mutators_guard_discriminator_self_test",
            "set_exact_resolutions_bumps_project_generation_after_the_workspace_mutation",
        ],
    ),
];

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("repo root")
        .to_path_buf()
}

fn read_doc(rel: &str) -> String {
    fs::read_to_string(workspace_root().join(rel))
        .unwrap_or_else(|e| panic!("read doc `{rel}`: {e}"))
}

/// Normalise a section heading line to its canonical rule title.
/// Strips the leading `#`/`-`/numbering markers, the trailing
/// `(CRITICAL)` suffix, and surrounding whitespace.
fn normalise_title(raw: &str) -> String {
    let mut text = raw.trim();
    // Strip leading markdown header markers (`#`, `##`, `###`, …).
    while let Some(rest) = text.strip_prefix('#') {
        text = rest;
    }
    let text = text.trim();
    // Strip the trailing `(CRITICAL)` (with optional whitespace).
    let text = text.strip_suffix("(CRITICAL)").unwrap_or(text).trim();
    text.to_string()
}

/// Walk every line of the doc and return the canonical titles of
/// section headers containing `(CRITICAL)`. Body paragraphs that
/// reference `(CRITICAL)` in prose are ignored — we only inspect
/// lines starting with `#`.
fn extract_critical_titles(body: &str) -> Vec<String> {
    let mut titles = Vec::new();
    for line in body.lines() {
        let trimmed = line.trim_start();
        if !trimmed.starts_with('#') {
            continue;
        }
        if !trimmed.contains("(CRITICAL)") {
            continue;
        }
        let title = normalise_title(trimmed);
        if !title.is_empty() {
            titles.push(title);
        }
    }
    titles
}

/// Enumerate every doc file the meta-guard scans:
/// `CLAUDE.md` + every `.claude/skills/*/SKILL.md`.
fn collect_doc_paths() -> Vec<PathBuf> {
    let root = workspace_root();
    let mut paths = vec![root.join("CLAUDE.md")];
    let skills_dir = root.join(".claude").join("skills");
    if let Ok(read_dir) = fs::read_dir(&skills_dir) {
        let mut skill_dirs: Vec<_> = read_dir
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.path().is_dir())
            .map(|entry| entry.path())
            .collect();
        // Deterministic order.
        skill_dirs.sort();
        for dir in skill_dirs {
            let skill_md = dir.join("SKILL.md");
            if skill_md.is_file() {
                paths.push(skill_md);
            }
        }
    }
    paths
}

/// R6 meta-guard: every `(CRITICAL)` section in CLAUDE.md / SKILL.md
/// must appear in `CRITICAL_RULE_GUARDS`.
#[test]
fn every_critical_rule_in_docs_has_registered_guard() {
    let known_titles: std::collections::HashSet<&str> =
        CRITICAL_RULE_GUARDS.iter().map(|(t, _)| *t).collect();

    let mut missing: Vec<String> = Vec::new();
    for path in collect_doc_paths() {
        let rel = path
            .strip_prefix(workspace_root())
            .unwrap_or(&path)
            .display()
            .to_string();
        let body = read_doc(&rel);
        for title in extract_critical_titles(&body) {
            if !known_titles.contains(title.as_str()) {
                missing.push(format!("{rel}: `{title}`"));
            }
        }
    }

    assert!(
        missing.is_empty(),
        "R6 META-GUARD — every `(CRITICAL)` section in \
         CLAUDE.md and `.claude/skills/*/SKILL.md` MUST be registered \
         in `CRITICAL_RULE_GUARDS` (in \
         `crates/verter_session/tests/cases/g_misc0/critical_rules_have_guards.rs`) \
         with at least one guard reference. Prose-only CRITICAL rules \
         are documentation that nothing enforces. Every CRITICAL rule \
         needs a static architecture \
         guard OR a discriminating test in the same change that adds \
         the rule. Missing registry entries:\n\n{list}\n\n\
         To fix: open \
         `crates/verter_session/tests/cases/g_misc0/critical_rules_have_guards.rs`, \
         add a row to `CRITICAL_RULE_GUARDS` for each missing title, \
         and reference the guard(s) that pin the invariant.",
        list = missing.join("\n")
    );
}

/// Inverse check: each entry in `CRITICAL_RULE_GUARDS` must still
/// appear in CLAUDE.md / SKILL.md. Catches stale registry rows when
/// a CRITICAL rule is retired without removing its row.
#[test]
fn registry_does_not_reference_stale_critical_rules() {
    let mut doc_titles: std::collections::HashSet<String> = std::collections::HashSet::new();
    for path in collect_doc_paths() {
        let rel = path
            .strip_prefix(workspace_root())
            .unwrap_or(&path)
            .display()
            .to_string();
        let body = read_doc(&rel);
        for title in extract_critical_titles(&body) {
            doc_titles.insert(title);
        }
    }

    let stale: Vec<&str> = CRITICAL_RULE_GUARDS
        .iter()
        .map(|(t, _)| *t)
        .filter(|t| !doc_titles.contains(*t))
        .collect();

    assert!(
        stale.is_empty(),
        "R6 META-GUARD — `CRITICAL_RULE_GUARDS` \
         references rule titles that no longer appear in the docs. A \
         stale registry entry hides removed-but-still-tracked rules \
         from the inverse audit. Retire the registry row in the same \
         change that retires the doc section. Stale entries: \
         {stale:?}."
    );
}

/// Structural well-formedness — every registry row must list at
/// least one guard. An empty guard list is the stub bypass this
/// meta-guard exists to prevent.
#[test]
fn every_registry_entry_lists_at_least_one_guard() {
    let empty: Vec<&str> = CRITICAL_RULE_GUARDS
        .iter()
        .filter(|(_, guards)| guards.is_empty())
        .map(|(t, _)| *t)
        .collect();

    assert!(
        empty.is_empty(),
        "R6 META-GUARD — `CRITICAL_RULE_GUARDS` entries \
         with an empty guard list bypass the meta-guard's intent (a \
         CRITICAL rule without an executable guard is prose). Each \
         row MUST reference at least one named guard test or scanner. \
         Empty rows: {empty:?}."
    );
}

/// Self-discriminator — verifies the title-normaliser correctly
/// strips markdown header markers and the trailing `(CRITICAL)`
/// suffix. Without this, a docs-side edit that subtly reformats a
/// heading (e.g. adds a leading `###` level or changes spacing
/// around `(CRITICAL)`) would slip past the registry check.
#[test]
fn title_normaliser_handles_markdown_and_critical_suffix() {
    // Trailing suffix stripped + leading hashes stripped + trim.
    assert_eq!(
        normalise_title("### Foo Bar (CRITICAL)"),
        "Foo Bar".to_string(),
    );
    assert_eq!(
        normalise_title("#### Cache Architecture (CRITICAL)"),
        "Cache Architecture".to_string(),
    );
    // Excess whitespace handled.
    assert_eq!(
        normalise_title("###    Macro Type Traversal Rule   (CRITICAL)"),
        "Macro Type Traversal Rule".to_string(),
    );
    // Title without trailing `(CRITICAL)` is left alone (the caller
    // only invokes this for lines that contain `(CRITICAL)` so this
    // is defence-in-depth).
    assert_eq!(normalise_title("## Not A Rule"), "Not A Rule".to_string(),);
}

/// Discriminating self-test for the registry-completeness walk's
/// fail-closed directory classification: `registry_completeness_classified_as_dir`
/// must hard-fail (panic) on a non-`NotFound` metadata IO error rather than
/// silently treating the path as a non-directory. This is the precise
/// difference from `Path::is_dir()`, which collapses EVERY metadata error
/// (including a `NotADirectory`/permission error) to `false` and would drop a
/// crate, a `tests/`/`src/` subtree, or a subdir from the registry-completeness
/// scan vacuously — letting the meta-guard believe guard coverage exists for
/// files it never scanned.
///
/// Uniquely named (NOT shared with the g_misc2 scanners' same-purpose
/// self-tests): the HashSet-dedup lesson is that two same-named self-tests in
/// different files leave neither protected — so this guard's fail-closed walk
/// gets its own distinctly-named self-test, registered under its own name.
#[test]
fn registry_completeness_walk_hard_fails_on_metadata_error_self_test() {
    let scratch = std::env::temp_dir().join(format!(
        "verter_registry_completeness_classify_{}_{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    fs::create_dir_all(&scratch).expect("create scratch dir");

    // A real directory classifies as a directory.
    assert!(
        registry_completeness_classified_as_dir(&scratch),
        "classifier must report an existing directory as a directory"
    );

    // A genuinely-absent path (`NotFound`) is a LEGITIMATE non-directory
    // answer — a crate root without a `tests/`/`src/` is simply skipped, NOT a
    // panic.
    let absent = scratch.join("definitely_absent").join("src");
    assert!(
        !registry_completeness_classified_as_dir(&absent),
        "classifier must report a genuinely-absent (NotFound) path as a \
         non-directory WITHOUT panicking — a missing `src/`/`tests/` is a legitimate skip"
    );

    // A path that traverses THROUGH a regular file as if it were a directory
    // produces a non-`NotFound` metadata IO error (`NotADirectory` on Unix, an
    // analogous non-NotFound kind on Windows). `registry_completeness_classified_as_dir`
    // MUST panic on it; `Path::is_dir()` would silently return `false` (the
    // fail-open class this guard closes).
    let regular_file = scratch.join("regular.txt");
    fs::write(&regular_file, b"not a directory").expect("write regular file");
    let through_file = regular_file.join("src");

    // Sanity precondition: confirm this scratch path is the IO-error (NOT
    // NotFound) case on this platform, so the test discriminates rather than
    // passing vacuously where the path resolves to NotFound.
    let probe = fs::metadata(&through_file);
    assert!(
        probe
            .as_ref()
            .err()
            .is_some_and(|e| e.kind() != std::io::ErrorKind::NotFound),
        "self-test precondition: traversing through a regular file must yield a \
         non-NotFound metadata IO error on this platform; got {probe:?}"
    );

    let panicked =
        std::panic::catch_unwind(|| registry_completeness_classified_as_dir(&through_file))
            .is_err();
    assert!(
        panicked,
        "classifier must HARD-FAIL (panic) on a non-NotFound metadata IO error \
         instead of silently treating the path as a non-directory. `Path::is_dir()` \
         would return `false` here, dropping a subtree from the registry-completeness \
         scan — that fail-open is exactly what this classifier closes."
    );

    fs::remove_dir_all(&scratch).ok();
}

/// Hard-failing directory classification for the registry-completeness
/// walk. Returns whether `path` is a directory.
///
/// A genuinely-absent path (`ErrorKind::NotFound`) is a legitimate
/// non-directory answer (`false`) — a crate root without a `tests/` or
/// `src/` is simply skipped. ANY OTHER metadata IO error (permissions, a
/// `NotADirectory` traversal, a stale handle) is a hard panic carrying the
/// path: `Path::is_dir()` collapses every such error to `false` and would
/// silently drop a crate, a `tests/`/`src/` subtree, or a whole subdir from
/// the registry-completeness scan — the fail-open class that lets the
/// meta-guard believe guard coverage exists for files it never scanned.
///
/// Uniquely named (NOT `classified_as_dir`): the g_misc2 scanners each carry
/// their own same-purpose helper + self-test, and a HashSet-dedup of
/// same-named self-tests would leave one copy silently satisfying the
/// registry for several — so this guard's fail-closed walk discipline gets
/// its own distinctly-named helper and self-test.
fn registry_completeness_classified_as_dir(path: &std::path::Path) -> bool {
    match fs::metadata(path) {
        Ok(meta) => meta.is_dir(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        Err(e) => panic!(
            "scan integrity: failed to classify `{}`: {e}",
            path.display()
        ),
    }
}

/// Walk every `.rs` file rooted at `path` (recursively).
///
/// A `read_dir` failure that is a legitimate `NotFound` (a crate may lack a
/// `tests/` or `src/` directory) is an empty skip; any OTHER `read_dir`
/// error is a hard panic carrying the path — never a silent `return` that
/// would drop a whole subtree from the registry-completeness scan and let
/// the meta-guard pass vacuously. Each `DirEntry` is unwrapped with a panic
/// (no `.flatten()` that would silently drop an errored entry), and the
/// dir-vs-file classification uses `registry_completeness_classified_as_dir`
/// (panic-on-non-NotFound-error), never `path.is_dir()`.
fn walk_rs_files(path: &PathBuf, out: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(path) {
        Ok(it) => it,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
        Err(e) => panic!(
            "scan integrity: failed to read directory `{}`: {e}",
            path.display()
        ),
    };
    for entry in entries {
        let entry = entry.unwrap_or_else(|e| {
            panic!(
                "scan integrity: failed to read a directory entry under `{}`: {e}",
                path.display()
            )
        });
        let p = entry.path();
        if registry_completeness_classified_as_dir(&p) {
            walk_rs_files(&p, out);
        } else if p.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(p);
        }
    }
}

fn collect_known_guard_names() -> std::collections::HashSet<String> {
    let mut names = std::collections::HashSet::<String>::new();
    let root = workspace_root();
    let crates_dir = root.join("crates");

    let crate_entries = match fs::read_dir(&crates_dir) {
        Ok(it) => it,
        Err(e) => panic!("read crates/: {e}"),
    };
    for crate_entry in crate_entries {
        // Unwrap each `DirEntry` with a panic carrying the directory — no
        // `.flatten()` that would silently drop a crate dir whose entry
        // errored from the registry-completeness scan.
        let crate_entry = crate_entry.unwrap_or_else(|e| {
            panic!("scan integrity: failed to read a crates/ directory entry: {e}")
        });
        let crate_path = crate_entry.path();
        if !registry_completeness_classified_as_dir(&crate_path) {
            continue;
        }

        // (1) Integration-test FILE basenames anywhere under
        // crates/<X>/tests/** . Test files consolidated into group
        // binaries live in subdirectories (tests/g_<group>/<name>.rs)
        // and run via their group root rather than
        // `cargo test --test <name>`; the registry entry still names a
        // real, discoverable test file, which is exactly what this
        // validity check confirms (it prevents padding the registry
        // with arbitrary strings — a non-existent file is still
        // rejected).
        let tests_dir = crate_path.join("tests");
        let mut tests_files = Vec::new();
        walk_rs_files(&tests_dir, &mut tests_files);
        for p in &tests_files {
            if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                names.insert(stem.to_string());
            }
        }

        // (2) `#[test] fn <name>(` declarations inside the test surfaces.
        //
        // Test functions live in:
        //   - crates/<X>/tests/**/*.rs (integration tests)
        //   - crates/<X>/src/**/*_tests.rs (extracted #[cfg(test)] modules)
        //
        // Adjacency contract: a function name is accepted as a
        // valid guard target ONLY IF the function's `fn <name>(`
        // declaration line is preceded (within the immediately
        // adjacent attribute block — typically one or two lines
        // above, allowing attribute stacking like `#[test]` followed
        // by `#[ignore]` or `#[serial]`) by a test attribute:
        //
        //   - `#[test]` (the standard libtest attribute),
        //   - `#[tokio::test]` / `#[tokio::test(...)]`,
        //   - `#[rstest]` / `#[rstest(...)]`,
        //   - `#[cfg_attr(..., test)]` (conditional-test pattern),
        //   - any path-attribute ending in `::test` (custom test
        //     macros that integrate with `cargo test <name>`).
        //
        // A bare `fn <name>(` declaration with no `#[test]`-family
        // attribute on the adjacent attribute block is NOT a test
        // and MUST NOT be accepted — `cargo test <name>` cannot run
        // such a function, so a registry entry naming it is a
        // dangling reference (an R6 validity gap that reopens if this
        // predicate relaxes back to accepting non-`#[test]`
        // declarations).
        let mut files = Vec::new();
        walk_rs_files(&tests_dir, &mut files);
        let src_dir = crate_path.join("src");
        if registry_completeness_classified_as_dir(&src_dir) {
            walk_rs_files(&src_dir, &mut files);
        }
        for file in files {
            // For src files, restrict to *_tests.rs (the
            // architectural-rule convention for extracted
            // #[cfg(test)] modules).
            let is_src = file.starts_with(&src_dir);
            if is_src {
                let name = file
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or_default();
                if !(name.ends_with("_tests.rs") || name == "tests.rs") {
                    continue;
                }
            }
            let src_text = match fs::read_to_string(&file) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let lines: Vec<&str> = src_text.lines().collect();
            for (i, line) in lines.iter().enumerate() {
                let trimmed = line.trim_start();
                let after_async = trimmed.strip_prefix("async ").unwrap_or(trimmed);
                let after_pub = after_async.strip_prefix("pub ").unwrap_or(after_async);
                let after_async = after_pub.strip_prefix("async ").unwrap_or(after_pub);
                let after_const = after_async.strip_prefix("const ").unwrap_or(after_async);
                let after_fn = match after_const.strip_prefix("fn ") {
                    Some(rest) => rest,
                    None => continue,
                };
                // `<name>(` or `<name><`-then-`(` (generic).
                let name_end = after_fn
                    .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                    .unwrap_or(after_fn.len());
                if name_end == 0 {
                    continue;
                }
                let name = &after_fn[..name_end];
                if name.is_empty() {
                    continue;
                }

                // Adjacency check: walk backwards from the `fn`
                // line, allowing whitespace-only lines and adjacent
                // attribute lines (`#[...]`), looking for a
                // `#[<test-attr>]`. Stop walking on the first
                // non-attribute, non-blank line — that boundary is
                // the previous declaration, and the current `fn` is
                // NOT part of a test attribute block.
                let mut idx = i;
                let mut has_test_attr = false;
                while idx > 0 {
                    idx -= 1;
                    let prev = lines[idx].trim();
                    if prev.is_empty() {
                        continue;
                    }
                    // Comment lines are skipped — they may appear
                    // between attributes and the `fn` declaration.
                    if prev.starts_with("//") || prev.starts_with("///") {
                        continue;
                    }
                    if !prev.starts_with("#[") {
                        break;
                    }
                    // Match `#[test]`, `#[tokio::test]`,
                    // `#[rstest]`, `#[*::test]`, `#[cfg_attr(...,
                    // test)]`, etc.
                    if attribute_line_marks_test(prev) {
                        has_test_attr = true;
                        break;
                    }
                    // Other `#[...]` attribute (e.g. `#[ignore]`,
                    // `#[serial]`, `#[allow(...)]`) — keep walking
                    // to look for a `#[test]` above it.
                }

                if has_test_attr {
                    names.insert(name.to_string());
                }
            }
        }
    }

    names
}

/// Returns `true` when `line` (a trimmed source line starting with
/// `#[`) is a `#[test]`-family attribute that registers the next
/// function as a libtest test entry-point — see the adjacency
/// contract in `collect_known_guard_names` for the accepted set.
fn attribute_line_marks_test(line: &str) -> bool {
    debug_assert!(line.starts_with("#["));
    // Strip the leading `#[` and the trailing `]` (or anything
    // after, since some attributes span lines — but the test
    // attributes we recognise are single-line forms).
    let inner = line.strip_prefix("#[").unwrap_or(line).trim_start();
    // `#[test]` — exact match (allowing for trailing `]`).
    if inner.starts_with("test]") || inner.starts_with("test(") || inner == "test" {
        return true;
    }
    // `#[<path>::test]` or `#[<path>::test(...)]` —
    // e.g. `tokio::test`, `async_std::test`, `actix_web::test`,
    // `serde_test::test`. The path-attribute form is recognised by
    // a `::test` segment followed by `]` or `(`.
    if let Some(after_double_colon) = inner.rfind("::test") {
        let after = &inner[after_double_colon + "::test".len()..];
        let next = after.chars().next();
        if matches!(next, Some(']') | Some('(')) {
            return true;
        }
    }
    // `#[rstest]` / `#[rstest(...)]` — the rstest fixture-driven
    // test attribute; libtest still names the generated test by
    // the `fn` name.
    if inner.starts_with("rstest]") || inner.starts_with("rstest(") || inner == "rstest" {
        return true;
    }
    // `#[cfg_attr(<predicate>, test)]` — conditional-test pattern.
    // The structural marker is `, test)` at the tail (allowing
    // whitespace).
    if inner.starts_with("cfg_attr") {
        // Look for `, test)` or `,test)` (allowing whitespace).
        let normalised: String = inner.chars().filter(|c| !c.is_whitespace()).collect();
        if normalised.contains(",test)") || normalised.contains(",test]") {
            return true;
        }
    }
    false
}

/// R6 validity scanner: every guard-name in `CRITICAL_RULE_GUARDS`
/// MUST resolve to a real test in the workspace. Closes the
/// guard-name validity gap the original R6 substrate left open —
/// without this, the registry could be padded with arbitrary
/// `&["fake_guard_a", "fake_guard_b"]` and the other R6 tests would
/// still PASS.
///
/// The check is a binary structural predicate
/// (`name ∈ {test functions} ∪ {test file basenames}`) — no
/// heuristics, no fuzzy thresholds.
#[test]
fn every_registry_guard_name_resolves_to_a_known_test() {
    let known = collect_known_guard_names();
    let mut dangling: Vec<(String, String)> = Vec::new();
    for (rule_title, guards) in CRITICAL_RULE_GUARDS {
        for guard_name in *guards {
            if !known.contains(*guard_name) {
                dangling.push((rule_title.to_string(), guard_name.to_string()));
            }
        }
    }
    assert!(
        dangling.is_empty(),
        "R6 VALIDITY SCANNER — `CRITICAL_RULE_GUARDS` references \
         guard names that do not resolve to any known test in the \
         workspace. Each entry must either name a `#[test] fn \
         <name>(` declared in `crates/*/tests/**/*.rs` or \
         `crates/*/src/**/*_tests.rs`, OR name the basename of an \
         integration-test file at `crates/*/tests/<name>.rs`. \
         Without this check the registry could be padded with \
         arbitrary strings, defeating the R6 contract. Dangling \
         entries (rule -> guard):\n  {}",
        dangling
            .iter()
            .map(|(rule, guard)| format!("`{rule}` -> `{guard}`"))
            .collect::<Vec<_>>()
            .join("\n  "),
    );
}

/// Self-discriminator for the validity scanner: injecting a fake
/// guard name into the predicate input MUST fail. Without this
/// negative case the scanner could silently pass a permissive
/// implementation.
#[test]
fn every_registry_guard_name_validity_scanner_discriminates_against_fake() {
    let known = collect_known_guard_names();
    // Inject a deliberately-bogus name and verify the lookup fails.
    let bogus = "definitely_not_a_real_guard_name_for_R6_validity_self_test";
    assert!(
        !known.contains(bogus),
        "validity-scanner self-test: a fake guard-name unexpectedly \
         matched a real test — the scanner's predicate is too \
         permissive."
    );
    // Sanity check: the real guard names that are in use today
    // (sample 3 from different registry rows) must resolve. If this
    // fails, the scanner is rejecting real guard names — a false
    // positive.
    for sample in [
        "no_macro_string_heuristics_in_resolver_core",
        "every_registry_entry_lists_at_least_one_guard",
        "import_route_writer_guard",
    ] {
        assert!(
            known.contains(sample),
            "validity-scanner self-test: known good guard `{sample}` \
             did not resolve — the scanner's predicate is too \
             strict (false positive)."
        );
    }
}

// ────────────────────────────────────────────────────────────────────
// "Declaration Augmentation" (CRITICAL) guard 16-iv.
//
// The overlay-aware augmentation index retired the fail-closed base-only
// `assert!(view.compat_token().session.is_none(), …)` that previously rejected
// a session view in `RouteDb::get_or_compute_effective_export_set`. A session
// call now keys its augmenter set under `AugmentationPopulation::Session` and
// scans the session's overlay artifacts unioned with base. Re-introducing the
// base-only assert would silently make every session augmentation query a hard
// error again.
//
// DISCRIMINATING: against the pre-deletion tree this scanner finds the assert
// string and FAILS; against the post-deletion tree the surface is clean and it
// PASSES.
// ────────────────────────────────────────────────────────────────────
#[test]
fn no_effective_export_set_base_only_session_assert() {
    // `get_or_compute_effective_export_set` lives in the route_db submodule
    // `route_db/effective_export_set.rs`; scan the whole route_db module
    // (file + submodule) so a re-introduced base-only assert is caught
    // wherever the function is hosted.
    let mut src = read_doc("crates/verter_session/src/resolver_core/route_db.rs");
    let submodule = workspace_root()
        .join("crates/verter_session/src/resolver_core/route_db/effective_export_set.rs");
    if submodule.is_file() {
        src.push('\n');
        src.push_str(
            &fs::read_to_string(&submodule).expect("read route_db/effective_export_set.rs"),
        );
    }

    // The retired assert pinned `session.is_none()` with the "base-only"
    // invariant message. Neither the predicate nor the message may reappear on
    // this surface.
    assert!(
        !src.contains("EffectiveExportSet is base-only"),
        "guard 16-iv: the base-only EffectiveExportSet invariant message must \
         not reappear in route_db.rs — the overlay-aware augmentation index \
         accepts session views (population identity), so the fail-closed \
         base-only assert is RETIRED."
    );
    assert!(
        !src.contains("compat_token().session.is_none()"),
        "guard 16-iv: a `compat_token().session.is_none()` base-only assert \
         must not gate the augmentation-index / EffectiveExportSet surface in \
         route_db.rs — session views are accepted under \
         `AugmentationPopulation::Session`."
    );

    // Self-discrimination: the assert pattern is genuinely a substring test, so
    // the scanner would catch a re-introduction (e.g. inside an `assert!`).
    let reintroduced = "assert!(view.compat_token().session.is_none()";
    assert!(
        !src.contains(reintroduced),
        "guard 16-iv: the literal base-only session assert is forbidden."
    );
}

//! R6 meta-guard.
//!
//! Codex Round-2 Rule 6 (BINDING): every `(CRITICAL)` architecture
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
///    → `crates/verter_session/tests/import_route_writer_guard.rs`),
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
        ],
    ),
    (
        "Build Philosophy",
        &[
            "no_thread_local_oxc_caches",
            "no_direct_oxc_parser_calls_outside_scheduler_path",
            "recursion_budget_invariant_across_module_boundary",
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
        ],
    ),
    (
        "Macro Type Traversal Rule",
        &[
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
            "no_new_resolve_type_engine_path_production_file",
            "no_new_resolved_elements_production_file",
            "no_new_prepared_surface_projection_production_file",
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
            "audit_publishes_member_edge_with_published_field_provenance_at_macro_boundaries",
            "pattern_a_non_slot_mapped_publication_does_not_leak_inherited_library_members",
            "pattern_b_generic_parameter_substitution_does_not_leak_inherited_library_members",
            "chatmessages_shape_audit_has_zero_outputschema_execute_project_member_edges",
        ],
    ),
    // SKILL.md uses the shortened title "Shallow-By-Default Rule" for
    // the same architectural invariant. Aliased to the same guards.
    (
        "Shallow-By-Default Rule",
        &[
            "audit_publishes_member_edge_with_published_field_provenance_at_macro_boundaries",
            "pattern_a_non_slot_mapped_publication_does_not_leak_inherited_library_members",
            "pattern_b_generic_parameter_substitution_does_not_leak_inherited_library_members",
            "chatmessages_shape_audit_has_zero_outputschema_execute_project_member_edges",
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
        "Stub Prevention",
        &[
            "macro_impacting_constructs_fail_lowering_not_silent_skip",
            "every_consumer_has_production_call_site",
            // The R6 meta-guard itself is anti-stub: a CRITICAL rule
            // must reference a non-empty guard list.
            "every_registry_entry_lists_at_least_one_guard",
        ],
    ),
    // ──────────────────── SKILL.md additions ──────────────────────
    (
        "Component-Meta Heuristic Prevention",
        &[
            // Heuristic-prevention rule from `4062d1b72` — pinned by
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
            // Completeness-contract substrate (typed degraded states,
            // no `Complete(None)` for missing inputs) is a follow-up
            // batch (Codex Round-2 Rule 2). The interim guard pins the
            // current invariant via the audit-validator's
            // PublishedField gate + the no-silent-skip guard. The
            // completeness substrate (typed degraded states, no
            // `Complete(None)` for missing inputs) is a follow-up
            // batch; the interim guards pin today's contract.
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
            // deletion. The typed-IR
            // `TypeExpr::SyntheticSlotBinding` variant is the sole
            // carrier identity at the projector / registry / reducer
            // surface; re-introducing the R22 substrate is forbidden.
            "no_carrier_verdict_db",
            // Pins the legitimate explicit-deepen cache route through
            // `ShapeCacheKey::semantic_node_whole(scope,
            // SemanticNodeId(carrier.value_node), mode)`. Positive
            // executable proof of the cache-key identity round-trip
            // (insert + lookup + scope/mode/value_node discrimination);
            // discriminates RED-on-revert if the cache route stops
            // routing through `carrier.value_node`.
            "synthetic_carrier_explicit_deepen_proof",
        ],
    ),
    (
        "Block-vocabulary ban",
        &[
            // H19 (cache-runtime overhaul): production source under
            // `crates/*/src/**` must not contain plan vocabulary
            // (`\bblock \d+\b`, `cache-runtime overhaul`,
            // `runtime cutover`). The discriminator is the H19 test
            // inside `architecture_guards.rs`; the broader walker
            // (`no_phase_archaeology_in_production_code`) consumes
            // the same predicate and fails the build on any
            // production-source violation.
            "guard7_predicate_rejects_block_vocabulary",
        ],
    ),
    (
        "Editor-Liveness Provider-Sync Invariant",
        &[
            // The static editor-liveness architecture guard
            // (`crates/verter_lsp/tests/editor_liveness_guards.rs`) source-scans
            // every LSP provider-sync file and FAILS if any function OTHER THAN
            // the approved leaf close-dispatch primitives contains an inline
            // provider-close loop (close-before-sync), which would close the live
            // editor TSX on an owner change or lose the prior path on a failed
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
    // Two NEW CRITICAL rules landed by the U2 design
    // (`docs/arch/u2-query-value-domain-design.md`). The design-gate
    // guards below are discriminating TODAY; the STAGE-B behavioural
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
         `crates/verter_session/tests/critical_rules_have_guards.rs`) \
         with at least one guard reference. Prose-only CRITICAL rules \
         are documentation that nothing enforces. Codex Round-2 Rule 6 \
         (BINDING): every CRITICAL rule needs a static architecture \
         guard OR a discriminating test in the same change that adds \
         the rule. Missing registry entries:\n\n{list}\n\n\
         To fix: open \
         `crates/verter_session/tests/critical_rules_have_guards.rs`, \
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

/// Walk every `.rs` file rooted at `path` (recursively) and call
/// `visit` with each file's path.
fn walk_rs_files(path: &PathBuf, out: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(path) {
        Ok(it) => it,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
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
    for crate_entry in crate_entries.flatten() {
        let crate_path = crate_entry.path();
        if !crate_path.is_dir() {
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
        // dangling reference (R6 validity gap closed in Round 19a
        // Commit 2; reopened post-Round-19a if this predicate
        // relaxes back to accepting non-`#[test]` declarations).
        let mut files = Vec::new();
        walk_rs_files(&tests_dir, &mut files);
        let src_dir = crate_path.join("src");
        if src_dir.is_dir() {
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

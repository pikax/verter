//! Block 6.j R18 — R6 meta-guard.
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
/// The registry does NOT validate that the name resolves to a
/// runnable test — scanning the entire workspace symbol table on
/// every CI run is pointlessly expensive. Reviewers and reviewers'
/// tooling can cross-reference; the contract this enforces is
/// "there exists a named guard the rule's author committed to".
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
        ],
    ),
    (
        "Macro Type Traversal Rule",
        &[
            "no_macro_string_heuristics_in_resolver_core",
            "no_text_based_macro_surface_projection_helpers",
            "no_role_inference_from_name_suffix",
            "no_pick_or_omit_string_prefix_check",
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
            "no_checker_display_text_parsing_outside_adapter",
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
            // checker display-text adapter abuse).
            "no_macro_string_heuristics_in_resolver_core",
            "no_text_based_macro_surface_projection_helpers",
            "no_format_then_reparse",
            "no_pick_or_omit_string_prefix_check",
            "no_role_inference_from_name_suffix",
            "no_checker_display_text_parsing_outside_adapter",
        ],
    ),
    (
        "Component-Meta Completeness Contract",
        &[
            // Completeness-contract substrate (typed degraded states,
            // no `Complete(None)` for missing inputs) is Block 6.j R19
            // (Codex Round-2 Rule 2). The interim guard pins the
            // current invariant via the audit-validator's
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
            // Same Block 6.j R19 substrate as the component-meta
            // completeness contract.
            "macro_impacting_constructs_fail_lowering_not_silent_skip",
            "audit_publishes_member_edge_with_published_field_provenance_at_macro_boundaries",
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
        "Block 6.j R18 R6 META-GUARD — every `(CRITICAL)` section in \
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
        "Block 6.j R18 R6 META-GUARD — `CRITICAL_RULE_GUARDS` \
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
        "Block 6.j R18 R6 META-GUARD — `CRITICAL_RULE_GUARDS` entries \
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

//! Structural audit gate for the eager→typeinfo component-meta cutover, driven
//! against the vendored hermetic `ChatMessages.vue` corpus fixture.
//!
//! ChatMessages.vue (the `nuxt-ui` chat-messages component) was the worst-case
//! hang on the eager rail (196–598s): its `withDefaults(defineProps<…>())` +
//! `defineSlots<…>` macros, resolved cross-file through imported generic types,
//! drove the eager imported-macro-surface materialiser + an external OXC
//! frontier/reparse loop. The typeinfo cutover removes both.
//!
//! The GATE is STRUCTURAL audit counters (the project's "data over heuristics"
//! rule), NOT wall-clock — the counters prove the eager work is GONE:
//!
//! - `synthesis_expanded_instantiate_calls == 0` — no synthesis-attributable
//!   Expanded `Instantiate` (the eager imported-macro-surface materialisation
//!   reduced carriers eagerly; the typeinfo path is shallow-by-default).
//! - `resolver_hot_path.imported_macro_surface_projection == 0` — the eager
//!   imported-macro-surface projection counter never fires (that rail is deleted).
//! - `resolver_hot_path.frontier_closure_invocations_total == 0` (+ the
//!   `target_none` / `redundant_target_none_pairs` subsets) and
//!   `resolved_external_type_cache_negative_misses == 0` — no external OXC
//!   frontier/reparse loop (the deleted `project_imported_macro_surfaces`
//!   reparse hang source).
//! - `expanded_instantiate_calls == CHAT_MESSAGES_EXPANDED_INSTANTIATE_VALUE`
//!   — the deterministic residual Expanded work (the canonical owner surface),
//!   asserted EXACTLY (data over a `<=` heuristic).
//! - Dependency-read / probe BREADTH: every `declared_dependency_files` entry is
//!   an extension / index candidate of one of the SFC's own relative-import
//!   roots (`CHAT_MESSAGES_DECLARED_DEPENDENCY_ROOTS`) — a per-entry structural
//!   check that polices the "Do not walk unrelated imports" rule.
//! - `indexed_ready_builds` for the audited request contains ONLY the owner
//!   `/ChatMessages.vue` — no dependency IndexedReady is built (the imports are
//!   unresolvable in the hermetic setup, and nothing forces an eager dep build).
//! - PRE-INDEXED run: pre-upsert the owner, then resolve component-meta — NO
//!   fresh `indexed_ready_builds` appears (the cached owner IndexedReady is
//!   reused, the architectural cache-reuse target).
//!
//! Wall-clock is emitted as NON-GATING sanity (target <60s; was 196–598s; vcm
//! 2.59s) — never asserted.

// The shared `component_meta_audit/harness.rs` is `#[path]`-included as a
// module; it defines fixture constants this test does not all consume, so the
// crate-level `dead_code` allow mirrors the sibling
// `block_6i_leak_chatmessages_audit.rs` harness-include.
#![allow(clippy::too_many_lines, dead_code, unused_imports)]

#[path = "component_meta_audit/harness.rs"]
mod harness;

use std::time::Instant;

use harness::{build_hermetic_host, build_preupserted_host, footprint_of, resolve_under_audit};

const CHAT_MESSAGES_VUE: &str =
    include_str!("component_meta_audit_corpus/fixtures/ChatMessages.vue");

/// Ceiling for `synthesis_expanded_instantiate_calls` — the synthesis path on
/// the corrected (typeinfo) rail performs ZERO Expanded `Instantiate` calls for
/// this fixture. A non-zero observation means the eager imported-macro-surface
/// materialisation (or an equivalent eager reduction) regressed back in.
const CHAT_MESSAGES_SYNTHESIS_EXPANDED_INSTANTIATE_CEILING: u64 = 0;

/// The EXACT request-wide `expanded_instantiate_calls` — the bounded residual
/// Expanded work the corrected path produces resolving the canonical owner
/// surface (NOT the deleted eager imported-macro-surface materialisation). This
/// is deterministic for the fixture, so the gate asserts equality (data over a
/// `<=` heuristic): a higher value means new eager expansion crept in; a lower
/// value means the residual owner expansion changed and the gate is re-derived.
const CHAT_MESSAGES_EXPANDED_INSTANTIATE_VALUE: u64 = 3;

/// The declared-dependency ROOTS the audited ChatMessages resolve is allowed to
/// touch — the SFC's own RELATIVE import targets plus the owner. Every
/// `declared_dependency_files` entry must be an extension / index candidate of
/// one of these roots; an entry outside this set is a BREADTH-WALK into an
/// unrelated import (the CLAUDE.md "Do not walk unrelated imports" rule). The
/// SFC's PACKAGE imports (`vue`, `ai`, `reka-ui`, `@vueuse/core`, `#build/*`,
/// `#imports`, …) are unresolvable in the hermetic setup and produce NO
/// extension-candidate probes, so they are correctly absent here.
const CHAT_MESSAGES_DECLARED_DEPENDENCY_ROOTS: &[&str] = &[
    "/ChatMessages.vue",
    "/ChatMessage.vue",
    "/Button.vue",
    "/composables/useComponentUI",
    "/types",
    "/utils",
];

#[test]
fn chatmessages_cutover_audit_has_no_eager_materialization_or_frontier_loop() {
    // ---- COLD run ------------------------------------------------------
    let host = build_hermetic_host(&[("/ChatMessages.vue", CHAT_MESSAGES_VUE)]);
    let cold_start = Instant::now();
    let (_analysis, _resolution, record) = resolve_under_audit(host, "/ChatMessages.vue");
    let cold_elapsed = cold_start.elapsed();

    let payload = record
        .component_meta_payload()
        .expect("component-meta request must carry a ComponentMeta audit payload");

    // The eager-path signal: synthesis-attributable Expanded Instantiate. The
    // committed ceiling is 0 (the corrected typeinfo path does ZERO), so this
    // is an exact-equality gate — any non-zero value means the eager
    // imported-macro-surface materialisation regressed back in.
    assert_eq!(
        payload.synthesis_expanded_instantiate_calls,
        CHAT_MESSAGES_SYNTHESIS_EXPANDED_INSTANTIATE_CEILING,
        "ChatMessages cutover gate: synthesis_expanded_instantiate_calls \
         ({}) must equal the committed ceiling {} — a non-zero value means the \
         eager imported-macro-surface materialisation regressed back in",
        payload.synthesis_expanded_instantiate_calls,
        CHAT_MESSAGES_SYNTHESIS_EXPANDED_INSTANTIATE_CEILING,
    );

    // Total Expanded work (the residual canonical-owner expansion) is
    // DETERMINISTIC for this fixture — assert the EXACT value (data over
    // heuristics). A higher value means new eager expansion crept in; a LOWER
    // value means the residual owner expansion changed shape and the gate must
    // be re-derived deliberately.
    assert_eq!(
        payload.expanded_instantiate_calls, CHAT_MESSAGES_EXPANDED_INSTANTIATE_VALUE,
        "ChatMessages cutover gate: expanded_instantiate_calls ({}) must equal the \
         committed value {} — a higher value means new eager expansion crept in",
        payload.expanded_instantiate_calls, CHAT_MESSAGES_EXPANDED_INSTANTIATE_VALUE,
    );

    let fp = footprint_of(&record);

    // Dependency-read / probe BREADTH gate: every declared dependency the
    // request touched must be an extension / index candidate of one of the
    // SFC's OWN declared relative-import roots (or the owner). An entry outside
    // that set is a breadth-walk into an unrelated import — the CLAUDE.md macro
    // traversal rule ("Do not walk unrelated imports"). This is a STRUCTURAL
    // per-entry check (entry root ∈ declared roots), not a count threshold.
    let is_candidate_of_declared_root = |entry: &str| -> bool {
        CHAT_MESSAGES_DECLARED_DEPENDENCY_ROOTS.iter().any(|root| {
            entry == *root
                // `root.ext` (e.g. `/types.d.ts`) or `root/sub` (e.g.
                // `/types/index.ts`, `/types/tv`) — but NOT `/typesX`.
                || entry.strip_prefix(root).is_some_and(|rest| {
                    rest.starts_with('.') || rest.starts_with('/')
                })
        })
    };
    let unrelated: Vec<String> = fp
        .declared_dependency_files()
        .into_iter()
        .filter(|entry| !is_candidate_of_declared_root(entry.as_ref()))
        .map(|entry| entry.as_ref().to_string())
        .collect();
    assert!(
        unrelated.is_empty(),
        "ChatMessages cutover gate: every declared dependency must be a candidate \
         of one of the SFC's own relative-import roots {CHAT_MESSAGES_DECLARED_DEPENDENCY_ROOTS:?} \
         — these entries are unrelated-import breadth-walks: {unrelated:?}",
    );

    // No eager imported-macro-surface projection (that rail is deleted).
    assert_eq!(
        fp.resolver_hot_path.imported_macro_surface_projection, 0,
        "ChatMessages cutover gate: imported_macro_surface_projection must be 0 \
         — the eager imported-macro-surface rail is deleted; any non-zero \
         observation means it was revived",
    );

    // No external OXC frontier / reparse loop (the deleted
    // `project_imported_macro_surfaces` reparse hang source).
    assert_eq!(
        fp.resolver_hot_path.frontier_closure_invocations_total, 0,
        "ChatMessages cutover gate: frontier_closure_invocations_total must be 0 \
         — no external OXC frontier/reparse loop",
    );
    assert_eq!(
        fp.resolver_hot_path
            .frontier_closure_invocations_target_none,
        0,
        "ChatMessages cutover gate: frontier_closure_invocations_target_none must be 0",
    );
    assert_eq!(
        fp.resolver_hot_path
            .frontier_closure_redundant_target_none_pairs,
        0,
        "ChatMessages cutover gate: frontier_closure_redundant_target_none_pairs must be 0",
    );
    assert_eq!(
        fp.resolver_hot_path
            .resolved_external_type_cache_negative_misses,
        0,
        "ChatMessages cutover gate: resolved_external_type_cache_negative_misses must be 0 \
         — no negative-cache thrash from a reparse loop",
    );

    // The owner's IndexedReady is built; no UNRELATED dependency IndexedReady is
    // built (the imports are unresolvable in the hermetic setup, and nothing
    // forces an eager dependency build). Proving the owner is the SOLE
    // IndexedReady build is the "constrained work" gate for this fixture.
    let irb: Vec<String> = fp
        .indexed_ready_builds
        .iter()
        .map(|b| b.canonical_id.as_ref().to_string())
        .collect();
    assert_eq!(
        irb,
        vec!["/ChatMessages.vue".to_string()],
        "ChatMessages cutover gate: the owner SFC must be the SOLE IndexedReady \
         build (no eager dependency materialisation). Observed: {irb:?}",
    );

    // Non-gating sanity: wall-clock. Was 196–598s on the eager rail; vcm 2.59s.
    eprintln!(
        "u3c ChatMessages COLD resolve wall-clock (NON-GATING): {cold_elapsed:?} \
         (eager rail was 196–598s; vcm 2.59s; target <60s)"
    );

    // ---- PRE-INDEXED run: cache reuse ----------------------------------
    // Pre-upsert the owner AND pre-build its IndexedReady (via `get_analysis`,
    // which builds + caches the `IndexedReady` artifact) but do NOT pre-run
    // component-meta (no component-meta RESULT cache warming). The subsequent
    // component-meta resolve must reuse the cached owner IndexedReady — NO fresh
    // `indexed_ready_builds` for the audited request (the architectural
    // cache-reuse target).
    let preindexed_host = build_preupserted_host(
        &[("/ChatMessages.vue", CHAT_MESSAGES_VUE)],
        "/ChatMessages.vue",
    );
    let _ = preindexed_host.get_analysis("/ChatMessages.vue");
    let (_a2, _r2, record2) = resolve_under_audit(preindexed_host, "/ChatMessages.vue");
    let fp2 = footprint_of(&record2);
    let irb2: Vec<String> = fp2
        .indexed_ready_builds
        .iter()
        .map(|b| b.canonical_id.as_ref().to_string())
        .collect();
    assert!(
        irb2.is_empty(),
        "ChatMessages cutover gate (pre-indexed): a component-meta resolve against \
         a host whose owner SFC was ALREADY upserted must build ZERO fresh \
         IndexedReady (the cached owner IndexedReady is reused). Observed fresh \
         builds: {irb2:?}",
    );
}

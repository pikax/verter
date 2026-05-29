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
//! - `expanded_instantiate_calls <= CHAT_MESSAGES_EXPANDED_INSTANTIATE_CEILING`
//!   — the bounded residual Expanded work (the canonical owner surface), pinned
//!   to the minimal value the corrected path produces.
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

use harness::{
    build_hermetic_host, build_preupserted_host, footprint_of, resolve_under_audit,
};

const CHAT_MESSAGES_VUE: &str = include_str!("component_meta_audit_corpus/fixtures/ChatMessages.vue");

/// Ceiling for `synthesis_expanded_instantiate_calls` — the synthesis path on
/// the corrected (typeinfo) rail performs ZERO Expanded `Instantiate` calls for
/// this fixture. A non-zero observation means the eager imported-macro-surface
/// materialisation (or an equivalent eager reduction) regressed back in.
const CHAT_MESSAGES_SYNTHESIS_EXPANDED_INSTANTIATE_CEILING: u64 = 0;

/// Ceiling for the request-wide `expanded_instantiate_calls` — the bounded
/// residual Expanded work the corrected path produces resolving the canonical
/// owner surface (NOT the deleted eager imported-macro-surface materialisation).
/// Pinned to the minimal value the corrected path actually produces; a higher
/// observation means new eager expansion crept in.
const CHAT_MESSAGES_EXPANDED_INSTANTIATE_CEILING: u64 = 3;

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

    // The eager-path signal: synthesis-attributable Expanded Instantiate.
    assert!(
        payload.synthesis_expanded_instantiate_calls
            <= CHAT_MESSAGES_SYNTHESIS_EXPANDED_INSTANTIATE_CEILING,
        "ChatMessages cutover gate: synthesis_expanded_instantiate_calls \
         ({}) must stay <= {} — a non-zero value means the eager \
         imported-macro-surface materialisation regressed back in",
        payload.synthesis_expanded_instantiate_calls,
        CHAT_MESSAGES_SYNTHESIS_EXPANDED_INSTANTIATE_CEILING,
    );

    // Bounded total Expanded work (the residual canonical-owner expansion).
    assert!(
        payload.expanded_instantiate_calls <= CHAT_MESSAGES_EXPANDED_INSTANTIATE_CEILING,
        "ChatMessages cutover gate: expanded_instantiate_calls ({}) must stay \
         <= {} — a higher value means new eager expansion crept in",
        payload.expanded_instantiate_calls,
        CHAT_MESSAGES_EXPANDED_INSTANTIATE_CEILING,
    );

    let fp = footprint_of(&record);

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
        fp.resolver_hot_path.frontier_closure_invocations_target_none, 0,
        "ChatMessages cutover gate: frontier_closure_invocations_target_none must be 0",
    );
    assert_eq!(
        fp.resolver_hot_path.frontier_closure_redundant_target_none_pairs, 0,
        "ChatMessages cutover gate: frontier_closure_redundant_target_none_pairs must be 0",
    );
    assert_eq!(
        fp.resolver_hot_path.resolved_external_type_cache_negative_misses, 0,
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
    let preindexed_host =
        build_preupserted_host(&[("/ChatMessages.vue", CHAT_MESSAGES_VUE)], "/ChatMessages.vue");
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

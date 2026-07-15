//! Architecture guard — every
//! `accumulate_dispatch_dep_signature` call site in
//! `crates/verter_session/src/meta_resolve/slot_binding_graph.rs`
//! must route through the file-local dual-emit helper
//! `emit_slot_binding_graph_dispatch_facts`.
//!
//! This is a structural guard, not a behavioural test. A regression
//! that adds a NEW call site to `accumulate_dispatch_dep_signature`
//! directly — bypassing the dual-emit helper — would defeat the
//! eventual collapse (which changes the
//! `fact_dep_signature` source from `state.fact_versions` to the
//! tracer's `read_set.finalise()`, then deletes the legacy
//! accumulator; any unpaired direct call would lose coverage on
//! that deletion).
//!
//! The slot-binding-graph traversal has SEVEN legitimate dispatch
//! sites that must emit dispatch-facts:
//!
//! 1. `accumulate_lowered_node_carrier_deps` — site 1 of 7.
//! 2. `slot_param_root_is_symbolic_only` (open-generic gate, CONCRETE
//!    Conditional-check reduction `ProjectPath` read) — site 2 of 7.
//! 3. `slot_param_root_is_symbolic_only` (open-generic gate,
//!    `InstantiationRef` carrier Skeleton-`Instantiate` read) — site 3 of 7.
//!    Sites 2 and 3 are the generic slot-alias binding gate: it reduces a
//!    concrete Conditional check and Skeleton-instantiates an
//!    `InstantiationRef` carrier through real `execute_read`s, each
//!    paired-emitted here so a `generic="M"` slot resolves to NO bindings on
//!    both the DTO and graph-native paths.
//! 4. `resolve_slot_bindings_graph_native` — site 4 of 7.
//! 5. `compute_bindings_via_graph` (slot-surface read) — site 5 of 7.
//! 6. `compute_bindings_via_graph` (param-surface read) — site 6 of 7.
//! 7. `compute_bindings_via_graph` (binding-value carrier
//!    head-resolution read — the dep-observation `Navigate` read that
//!    loads and records an unresolved binding value's cross-file
//!    declaration dependency) — site 7 of 7.
//!
//! Each site MUST appear exactly once as a call to
//! `emit_slot_binding_graph_dispatch_facts`. The legacy
//! `accumulate_dispatch_dep_signature` symbol may appear in:
//!
//! - The `use` import statement (line 34-ish).
//! - The body of `emit_slot_binding_graph_dispatch_facts` itself.
//!
//! Anywhere else is a violation.

#![cfg(test)]

use std::fs;
use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .canonicalize()
        .unwrap();
    manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .map(PathBuf::from)
        .expect("workspace root must exist two levels above CARGO_MANIFEST_DIR")
}

fn read_workspace_file(rel: &str) -> String {
    let path = workspace_root().join(rel);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {} failed: {e}", path.display()))
}

#[test]

fn slot_binding_graph_helper_is_declared() {
    let src = read_workspace_file("crates/verter_session/src/meta_resolve/slot_binding_graph.rs");
    assert!(
        src.contains("fn emit_slot_binding_graph_dispatch_facts("),
        "Arch guard: `slot_binding_graph.rs` MUST declare \
         the dual-emit helper `emit_slot_binding_graph_dispatch_facts`. \
         Without the helper, the seven dispatch-emission sites cannot \
         route their `accumulate_dispatch_dep_signature` AND \
         `observe_fact_signature` calls through one shared paired \
         emission point."
    );
}

#[test]

fn slot_binding_graph_helper_calls_both_channels() {
    let src = read_workspace_file("crates/verter_session/src/meta_resolve/slot_binding_graph.rs");

    // Locate the helper body and inspect it.
    let helper_decl = "fn emit_slot_binding_graph_dispatch_facts(";
    let helper_idx = src
        .find(helper_decl)
        .expect("emit_slot_binding_graph_dispatch_facts must be declared");
    // The helper body extends from its `fn` line through the next
    // top-level `\n}` (closing brace of the helper). Slice
    // generously then trim to the first `\n}` after `fn`.
    let helper_window_end = src[helper_idx..]
        .find("\n}\n")
        .map(|rel| helper_idx + rel)
        .unwrap_or(src.len());
    let helper_body = &src[helper_idx..helper_window_end];

    assert!(
        helper_body.contains("accumulate_dispatch_dep_signature(sig)"),
        "Arch guard: \
         `emit_slot_binding_graph_dispatch_facts` MUST call \
         `accumulate_dispatch_dep_signature(sig)` so the legacy \
         drain path (compute_component_meta_state_inner line ~869) \
         continues to fold slot-binding-graph dispatch facts into \
         `state.fact_versions` during the transition to the \
         tracer-sourced signature. Helper body:\n{helper_body}"
    );

    assert!(
        helper_body.contains("observe_fact_signature"),
        "Arch guard: \
         `emit_slot_binding_graph_dispatch_facts` MUST call \
         `observe_fact_signature` so the fact-tracer fan-out path \
         delivers slot-binding-graph dispatch facts into every \
         active `FactReadSet` on the `ACTIVE_TRACERS` stack. Without \
         this call, the legacy single-channel emission persists and \
         the accumulator cannot be retired. Helper body:\n{helper_body}"
    );

    assert!(
        helper_body.contains("dep_signature_to_fact_signature"),
        "Arch guard: \
         `emit_slot_binding_graph_dispatch_facts` MUST call \
         `dep_signature_to_fact_signature` (the signature bridge) to \
         convert the legacy `DepSignature` payload into a \
         `Vec<FactVersionRef>` before fanning out — `observe_fact_signature` \
         takes `&[FactVersionRef]`, not `&DepSignature`. Helper body:\n{helper_body}"
    );

    assert!(
        helper_body.contains("slot_binding_graph_fact_tracer_emissions"),
        "Arch guard: \
         `emit_slot_binding_graph_dispatch_facts` MUST bump the \
         `slot_binding_graph_fact_tracer_emissions` provenance \
         counter so the positive behavioural test can discriminate \
         the fan-out path. Helper body:\n{helper_body}"
    );

    assert!(
        helper_body.contains("slot_binding_graph_legacy_accumulator_emissions"),
        "Arch guard: \
         `emit_slot_binding_graph_dispatch_facts` MUST bump the \
         `slot_binding_graph_legacy_accumulator_emissions` \
         provenance counter so the dual-emit lockstep invariant is \
         discriminable. Helper body:\n{helper_body}"
    );
}

#[test]

fn slot_binding_graph_uses_paired_emit_at_every_site() {
    let src = read_workspace_file("crates/verter_session/src/meta_resolve/slot_binding_graph.rs");

    // Count `emit_slot_binding_graph_dispatch_facts(` call sites —
    // there must be exactly 7 (the seven dispatch reads in this
    // file). The function declaration itself accounts for the
    // `fn emit_...(` form, not the `emit_...(` call form, so it
    // does not inflate the count.
    let call_substring = "emit_slot_binding_graph_dispatch_facts(";
    let calls = src.matches(call_substring).count();
    let decl_substring = "fn emit_slot_binding_graph_dispatch_facts(";
    let decl_count = src.matches(decl_substring).count();
    // Calls = total occurrences − declarations (1).
    let pure_calls = calls.saturating_sub(decl_count);
    assert_eq!(
        pure_calls, 7,
        "Arch guard: `slot_binding_graph.rs` MUST contain \
         exactly 7 calls to `emit_slot_binding_graph_dispatch_facts` \
         (one per dispatch-read site: accumulate_lowered_node_carrier_deps, \
         TWO sites in slot_param_root_is_symbolic_only (the open-generic \
         gate's concrete-Conditional `ProjectPath` reduction AND the \
         `InstantiationRef` carrier Skeleton-`Instantiate`), \
         resolve_slot_bindings_graph_native, and three sites inside \
         compute_bindings_via_graph (slot-surface, param-surface, and \
         the binding-value carrier head-resolution dep-observation \
         read)). observed_calls={pure_calls} \
         (total occurrences={calls}, declarations={decl_count})"
    );
}

#[test]

fn slot_binding_graph_has_no_direct_accumulate_calls_outside_helper() {
    let src = read_workspace_file("crates/verter_session/src/meta_resolve/slot_binding_graph.rs");

    // The legacy helper `accumulate_dispatch_dep_signature` may
    // appear in:
    // - `use super::dep_signature::accumulate_dispatch_dep_signature;` (1)
    // - `accumulate_dispatch_dep_signature(sig);` inside the
    //   helper body (1)
    //
    // Any other occurrence is a direct call from a non-helper site
    // and violates the dual-emit collapse contract.
    let total_occurrences = src.matches("accumulate_dispatch_dep_signature").count();
    assert!(
        total_occurrences <= 2,
        "Arch guard: `slot_binding_graph.rs` may reference \
         `accumulate_dispatch_dep_signature` AT MOST twice (once in \
         the `use` import, once inside the dual-emit helper body). \
         A third or later occurrence is a direct call from a \
         non-helper site, which would bypass the dual-emit pairing \
         and break the planned collapse. observed_occurrences={total_occurrences}"
    );

    // Conversely, the helper body MUST still call the legacy
    // accumulator — the collapse that retires the legacy drain path
    // owns the deletion of this call AND the entire legacy helper.
    assert!(
        total_occurrences >= 2,
        "Arch guard: the legacy `accumulate_dispatch_dep_signature` \
         call inside the dual-emit helper MUST remain until the \
         legacy drain path is retired. Removing it prematurely \
         shrinks `state.fact_versions` for slot-binding-graph \
         consumers. observed_occurrences={total_occurrences}"
    );
}

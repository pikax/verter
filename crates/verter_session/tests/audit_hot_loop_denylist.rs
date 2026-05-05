//! Shared canonical denylist for the
//! `audit_no_hot_loop_instrumentation` architecture guard.
//!
//! Per plan §6 Slice 3.B and §1.6: instrumentation
//! (`record_phase_timing`, `record_event(CompileCodeTransformOp)`,
//! and similar producer-side audit emits) MUST fire at phase
//! boundaries only — never inside any per-element / per-attribute /
//! per-node / per-token loop body. Functions listed below are the
//! known hot-path bodies; the architecture guard rejects any
//! `current_observer()` / `record_*` call site inside their bodies.
//!
//! This module is intentionally a `pub const` only so it can be
//! `#[path]`-included from BOTH
//! `tests/architecture_guards.rs` (the canonical home for arch
//! guards) and `tests/compile_audit_no_hot_loop_instrumentation.rs`
//! (the focused regression for Slice 3.B).
//!
//! When adding entries: keep this list small (4–8 entries; see plan
//! §6 — escalate at >20). Format is `(crate_name, fully_qualified_path)`
//! where `fully_qualified_path` is the symbol's `name_path` per
//! Serena / standard module-path conventions and matches the test's
//! AST visitor output (the visitor walks `mod`/`impl`/`fn` nodes and
//! assembles the same path).

#![allow(dead_code)]

/// `(crate_name, function_path)` pairs that the architecture guard
/// inspects. Each entry's body must be free of producer-side
/// instrumentation (`current_observer()` calls, `record_*` invocations
/// on observer-typed receivers, or any audit-event emit).
pub const HOT_PATH_DENYLIST: &[(&str, &str)] = &[
    // VDOM template codegen — `process_element_leave` runs once per
    // element traversal exit, which is the inner per-node loop for
    // VDOM render-function generation.
    (
        "verter_compiler",
        "template::code_gen::vdom::element::process_element_leave",
    ),
    // IDE / TSX template codegen — `walk_node` is the per-node
    // traversal in the LSP/tsgo path. Runs many times per template.
    ("verter_compiler", "ide::template::walk_node"),
    // Template-data extraction — runs once per node when
    // `CompileTarget::TEMPLATE_DATA` (or `META`) is set.
    (
        "verter_compiler",
        "compile::template_data::walk_node_for_extraction",
    ),
    // Resolver-side per-expression projection — invoked per
    // navigation hop in the prepared-surface drill-down. Hot loop for
    // type-resolution requests. (Inherent method on
    // `ComponentMetaQueryEngine<'a>`; the path includes the receiver
    // type segment.)
    (
        "verter_session",
        "resolver_core::component_meta_query_engine::prepared_surface::ComponentMetaQueryEngine::project_prepared_surface_from_expr",
    ),
    // Resolver-side member-route per-expression projection —
    // sibling hot loop to `project_prepared_surface_from_expr`.
    (
        "verter_session",
        "resolver_core::component_meta_query_engine::prepared_surface::ComponentMetaQueryEngine::project_prepared_requested_member_from_expr",
    ),
    // Semantic-graph cooperative dispatch loop — every memoized
    // semantic subquery fans out through this method and each
    // dispatch is a hot iteration.
    (
        "verter_session",
        "semantic_query_memo::SemanticGraphStore::execute_cooperative",
    ),
];

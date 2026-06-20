//! Shared structural-lowering helper: the `<script setup generic="…">`
//! binder-frame builder.
//!
//! `build_script_setup_seed_frames` interns the owner's script-setup generic
//! type parameters as [`SemanticNodeData::TypeParam`] binder nodes and returns
//! a seed [`BinderScope`] stack, so a `<script setup generic="T">` SFC's open
//! generics lower to their `TypeParam` binder rather than an unbound `BareRef`.
//!
//! This is a SHARED, route-free helper: it re-sources the clause from the
//! owner's LOCAL [`IndexedReady`] data (`raw_source` + `framework_parse`)
//! through
//! [`sfc_script_setup_type_params`](crate::host_resolve::sfc_script_setup_type_params)
//! — NO host route lookup — so it is a pure structural producer. Its ONLY
//! production caller today is the macro hot mirror's macro-arg builder
//! (`macro_hot_mirror/mod.rs`); the in-module isolation tests also build the
//! seed binder shape from it directly. A future decl-body structural producer
//! (the global declaration-body structural producer flip, NOT landed) would
//! build the SAME seed binder shape, but it does NOT call this helper today.
//!
//! It lives UNDER `crate::macro_hot_mirror` (not a foreign module) so it can
//! reach the ancestor-private
//! [`structural_lower::lower_type_expr_structural`](super::structural_lower)
//! entry to lower each binder's constraint / default expression — the
//! documented INTERNAL binder-seed lowering, NOT a second macro-arg producer.
//! It is `pub(in crate::macro_hot_mirror)` — ancestor-private to the mirror,
//! mirroring the lowerer's own confinement — so NO foreign module can NAME it.
//! That by-construction, compiler-enforced privacy (a foreign reference is a
//! compile error, not a lint) is the load-bearing guarantee that no second
//! binder-seed producer can be opened outside this module (the secondary
//! token-tripwire guard is bounded defense-in-depth on top of it). The ONLY
//! caller that builds the seed shape today — the mirror's macro-arg builder
//! (plus the in-module isolation tests) — reaches it from WITHIN this module
//! subtree; the ordinary decl-body structural producer does NOT call it today
//! (that routing is the unlanded global declaration-body structural producer
//! flip). If that flip later routes a decl-body structural producer through
//! this helper from a module OUTSIDE the subtree, the helper must be
//! DELIBERATELY re-homed (or re-scoped to the new caller's shared
//! ancestor-private boundary, e.g. `pub(in crate::<shared-ancestor>)`) —
//! preserving a COMPILER boundary, never passively widened to `pub(crate)`,
//! which would re-open the cross-module producer surface.

use std::sync::Arc;

use super::structural_lower::{self, BinderScope, StructuralLowerContext};
use crate::semantic_query::{
    DeclIdentity, HashValue, HotTypeRef, NodeScopeId, SemanticNodeData, SemanticNodeId,
};
use crate::semantic_query_memo::SemanticGraphStore;

/// Build the seed [`BinderScope`] stack from the owner's script-setup type
/// bindings. Returns a one-frame stack (or an empty stack when there are no
/// script-setup generics). Each binder interns a
/// [`SemanticNodeData::TypeParam`] node matching the eager path's shape
/// (`<script-setup>` decl sentinel + the binding ordinal + lowered
/// constraint / default + display name). The constraint / default lower
/// under the seed frame accumulated SO FAR, so an earlier generic is visible
/// to a later one's constraint (`generic="T, U extends T">`) per TS scoping.
///
/// The `<script setup generic="…">` clause is re-sourced from the owner's
/// ROUTE-FREE local [`IndexedReady`] data (`raw_source` + `framework_parse`)
/// through [`sfc_script_setup_type_params`](crate::host_resolve::sfc_script_setup_type_params)
/// — the SAME route-free extraction `host_manage` uses to populate the
/// prepared-decl bundle's `script_setup_type_bindings`, so the seed binder
/// shape is identical. The helper does NOT read the prepared-decl bundle
/// (whose cold path can route-resolve imports) — that would make the producer
/// impure.
pub(in crate::macro_hot_mirror) fn build_script_setup_seed_frames(
    indexed: &crate::project_type_store::IndexedReady,
    graph: &SemanticGraphStore,
    scope: &NodeScopeId,
) -> Vec<BinderScope> {
    // Re-source the `<script setup generic="…">` clause from the owner's local
    // route-free parse artifact. The clause-position index IS the ordinal (the
    // same `param_index` the eager path / prepared-decl bundle assigns), so the
    // interned `TypeParam` identity tuple matches.
    let params = crate::host_resolve::sfc_script_setup_type_params(
        indexed.raw_source.as_ref(),
        indexed.framework_parse.as_deref(),
    );
    if params.is_empty() {
        return Vec::new();
    }

    let decl = match scope {
        NodeScopeId::Global => DeclIdentity {
            canonical_id: Arc::from(""),
            whole_hash: HashValue::default(),
            decl_name: Arc::from("<script-setup>"),
        },
        NodeScopeId::File {
            canonical_id,
            whole_hash,
            ..
        } => DeclIdentity {
            canonical_id: Arc::clone(canonical_id),
            whole_hash: *whole_hash,
            decl_name: Arc::from("<script-setup>"),
        },
    };

    let mut frame = BinderScope::default();
    for (idx, param) in params.iter().enumerate() {
        // The constraint / default see the binders accumulated so far.
        let head_frames = vec![frame.clone()];
        let head_ctx = StructuralLowerContext::new(&head_frames);
        let constraint = param.constraint.as_ref().and_then(|c| {
            structural_lower::lower_type_expr_structural(graph, c, scope.clone(), &head_ctx)
                .ok()
                .map(HotTypeRef::node)
        });
        let default = param.default.as_ref().and_then(|d| {
            structural_lower::lower_type_expr_structural(graph, d, scope.clone(), &head_ctx)
                .ok()
                .map(HotTypeRef::node)
        });
        // The clause-position index is the ordinal / `param_index` the eager
        // path and prepared-decl bundle assign — matching identity tuples.
        let ordinal = u16::try_from(idx).unwrap_or(u16::MAX);
        let display_name: Arc<str> = Arc::from(param.name.as_str());
        let node: SemanticNodeId = graph.intern_node_with_scope(
            SemanticNodeData::TypeParam {
                decl: decl.clone(),
                param_index: ordinal,
                constraint,
                default,
                display_name: Arc::clone(&display_name),
            },
            scope.clone(),
        );
        frame.bind(display_name, node);
    }

    vec![frame]
}

#[cfg(test)]
#[path = "script_setup_binder_tests.rs"]
mod script_setup_binder_tests;

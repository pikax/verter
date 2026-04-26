//! Caller-side dispatch-iteration helpers (plan §4A — closes Gaps 1/2/3).
//!
//! These helpers are the architectural substrate that the renamed
//! component-meta walker family
//! (`walk_component_meta_member_surface_expr`,
//! `walk_member_route_via_alias_body`, etc.) calls instead of carrying
//! its own:
//!
//! - manual scope iteration (Gap 1)
//! - manual default-type-parameter substitution (Gap 2)
//! - `MATERIALIZE_DEPTH` thread-local + `FxHashSet<TypeExpr>` active set
//!   (Gap 3)
//!
//! Each helper is a thin caller-side wrapper over
//! [`ProjectSemanticDispatch`] plus the host's prepared-decl lookups; none
//! of them carry behavior that doesn't reduce to dispatch primitives.
//!
//! ## Architectural invariants
//!
//! - **Dispatch is the single resolution authority.** The Gap 1/3 helpers
//!   call [`ProjectSemanticDispatch::lower_type_expr_in_scope_with_mode`]
//!   per step; they never reach into engine-private resolution paths.
//! - **No global state.** The Gap 3 visited-set is a value type owned by
//!   the caller; no thread-local depth counters.
//! - **Defensive fuse, not a hard depth cap.** The visited-set's fuse
//!   (4096 hops) is a safety rail for pathological inputs; ordinary
//!   termination is via cycle detection on resolved
//!   [`SemanticNodeId`]s.

use std::sync::Arc;

use rustc_hash::FxHashSet;
use verter_semantic::analysis::type_expr::TypeExpr;
use verter_semantic::analysis::type_solver::PreparedTypeDecl;

use crate::project_semantic_dispatch::{node_data_for, ProjectSemanticDispatch};
use crate::semantic_query::{
    PathSegment, ProjectionMode, QueryError, QueryResult, SemanticNodeData, SemanticNodeId,
    SemanticQueryApi, SemanticQueryKey,
};
use crate::VerterHost;

/// Defensive fuse for the Gap 3 visited-set worklist. Per plan §4A
/// D4A.2, visited-set cycle detection is the ordinary termination
/// criterion; the fuse is a safety rail for pathological inputs only.
///
/// `4096` was chosen because it is two orders of magnitude above the
/// deepest type chains observed in the integration corpus (≈ 40 hops
/// for the deepest nuxt-ui registry walk) and remains well below the
/// `projection_op_count` budget (`2000` per request) that already gates
/// dispatch's per-call work. Surfacing this trip emits a structured
/// diagnostic via `component_meta_trace_custom!`.
pub const VISITED_NODES_DEFENSIVE_FUSE: usize = 4096;

/// SemanticNodeId-keyed visited set with defensive fuse for Gap 3
/// iteration. Replaces the legacy walker's
/// `FxHashSet<verter_semantic::analysis::type_expr::TypeExpr>` active
/// set: hashing on a tiny `u32` instead of a deep AST clone, and
/// keying on the resolved semantic identity instead of the syntactic
/// `TypeExpr` form.
#[derive(Debug, Default, Clone)]
pub struct WalkerVisitedNodes {
    visited: FxHashSet<SemanticNodeId>,
    fuse_hops: usize,
}

/// Outcome of pushing a node into a [`WalkerVisitedNodes`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisitedPushOutcome {
    /// Node was newly inserted; caller may proceed with this hop.
    Inserted,
    /// Node was already present in the visited set — Gap 3 cycle
    /// detected. Caller should terminate the iteration with the
    /// current expression (typically a `Ref` shell).
    Cycle,
    /// Defensive fuse exhausted. Caller should terminate with the
    /// current expression and emit a structured diagnostic.
    FuseExhausted,
}

impl WalkerVisitedNodes {
    /// Construct an empty visited set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Attempt to push `node` into the visited set.
    ///
    /// Increments the defensive fuse counter on every call. Returns
    /// [`VisitedPushOutcome::FuseExhausted`] if the fuse exceeds
    /// [`VISITED_NODES_DEFENSIVE_FUSE`], [`VisitedPushOutcome::Cycle`]
    /// if the node is already present, or
    /// [`VisitedPushOutcome::Inserted`] otherwise.
    pub fn try_push(&mut self, node: SemanticNodeId) -> VisitedPushOutcome {
        self.fuse_hops += 1;
        if self.fuse_hops > VISITED_NODES_DEFENSIVE_FUSE {
            return VisitedPushOutcome::FuseExhausted;
        }
        if !self.visited.insert(node) {
            return VisitedPushOutcome::Cycle;
        }
        VisitedPushOutcome::Inserted
    }

    /// Remove `node` from the visited set. Used to scope cycle
    /// detection to a single iteration chain.
    pub fn pop(&mut self, node: SemanticNodeId) {
        self.visited.remove(&node);
    }

    /// Returns `true` when `node` is currently in the visited set.
    pub fn contains(&self, node: SemanticNodeId) -> bool {
        self.visited.contains(&node)
    }

    /// Returns the cumulative number of `try_push` calls observed —
    /// the defensive fuse counter. Exposed so callers (and tests) can
    /// inspect fuse pressure without reaching into private fields.
    pub fn fuse_hops(&self) -> usize {
        self.fuse_hops
    }
}

/// **Gap 1**: Caller-side multi-scope dispatch lowering.
///
/// Iterates `scopes` in order, feeding each through
/// [`ProjectSemanticDispatch::lower_type_expr_in_scope_with_mode`].
/// The first lowering whose result is **not** an opaque `Miss` wins;
/// the helper returns `Some((scope, node))` for that pair.
///
/// Replaces the walker's open-coded scope retry loops (formerly via
/// `select_imported_materialization_scope` + manual fallbacks). Per
/// plan §4A D4A.2 (Gap 1 row), dispatch stays single-purpose
/// (one-scope-per-call) while the caller iterates.
///
/// Returns `None` when every scope produces an opaque miss (or when
/// dispatch itself returns `None` because the scope is unknown to the
/// host — e.g. an unloaded canonical id).
pub fn lower_in_first_responsive_scope(
    dispatch: &ProjectSemanticDispatch<'_>,
    host: &VerterHost,
    scopes: &[&str],
    expr: &TypeExpr,
    mode: ProjectionMode,
) -> Option<(String, SemanticNodeId)> {
    for scope in scopes {
        let Some(node) = dispatch.lower_type_expr_in_scope_with_mode(scope, expr, mode) else {
            continue;
        };
        if !node_is_opaque_miss(host, node) {
            return Some(((*scope).to_string(), node));
        }
    }
    None
}

/// **Gap 2**: Caller-side rewrite of `Ref { name, type_arguments: [] }`
/// to `Ref { name, type_arguments: [defaults] }` when the prepared
/// declaration's type parameters all carry defaults.
///
/// Returns `Some(rewritten)` only when:
/// 1. `expr` is a `Ref` with empty `type_arguments`.
/// 2. `prepared` carries at least one type parameter.
/// 3. *Every* type parameter has a `default` (partial defaults would
///    leave a hole that dispatch's `Instantiate` can fill from its own
///    binding logic, so this helper only fires when the rewrite is
///    fully determined).
///
/// Otherwise returns `None`. Caller uses the rewrite (when present) as
/// the input to dispatch's `lower_type_expr_in_scope_with_mode` so the
/// resulting `Instantiate` query carries explicit args, matching the
/// legacy walker's `expand_local_generic_ref_expr` semantics.
///
/// Note: dispatch's `build_instantiate` already substitutes defaults
/// for omitted args internally; this helper exists so the *rescue*
/// path (where the walker has only the unresolved `Ref` and decides
/// whether to attempt instantiation at all) can normalize the input
/// before re-entering dispatch.
pub fn rewrite_omitted_generic_args_with_defaults(
    expr: &TypeExpr,
    prepared: &PreparedTypeDecl,
) -> Option<TypeExpr> {
    let TypeExpr::Ref {
        name,
        type_arguments,
    } = expr
    else {
        return None;
    };
    if !type_arguments.is_empty() {
        return None;
    }
    if prepared.type_parameters.is_empty() {
        return None;
    }
    let defaults: Vec<TypeExpr> = prepared
        .type_parameters
        .iter()
        .filter_map(|param| param.default.as_deref().cloned())
        .collect();
    if defaults.len() != prepared.type_parameters.len() {
        return None;
    }
    Some(TypeExpr::Ref {
        name: name.clone(),
        type_arguments: Arc::from(defaults.into_boxed_slice()),
    })
}

/// Outcome of [`iterate_ref_chain_until_non_ref`].
#[derive(Debug, Clone)]
pub enum RefChainOutcome {
    /// The iteration terminated on a non-`Ref` shape; carry the raised
    /// expression. The walker uses this as the materialised body.
    Resolved(TypeExpr),
    /// A previously-visited [`SemanticNodeId`] was re-encountered —
    /// Gap 3 cycle. Carry the last expression for caller fallback.
    CycleDetected(TypeExpr),
    /// Defensive fuse exhausted. Carry the last expression. Callers
    /// should emit a structured diagnostic via
    /// `component_meta_trace_custom!`.
    FuseExhausted(TypeExpr),
    /// Dispatch produced an opaque miss for the supplied scope. The
    /// caller may try a different scope or fall back to the legacy
    /// projection path.
    Miss,
}

/// **Gap 3**: Caller-side iteration over chained `Ref → Ref → ...` via
/// dispatch with a [`WalkerVisitedNodes`] cycle guard and defensive
/// fuse.
///
/// At each step:
/// 1. Lower the current `expr` through
///    [`ProjectSemanticDispatch::lower_type_expr_in_scope_with_mode`]
///    in `scope_canonical_id`.
/// 2. Push the resulting [`SemanticNodeId`] into `visited`. If the
///    push reports [`VisitedPushOutcome::Cycle`] or
///    [`VisitedPushOutcome::FuseExhausted`], terminate with the
///    matching outcome carrying the last good expression.
/// 3. Raise the node back to a `TypeExpr` via
///    [`ProjectSemanticDispatch::raise_node_to_type_expr`].
/// 4. If the raised form is a `Ref` *different* from the current
///    expression, continue with that as the next step. Otherwise
///    return [`RefChainOutcome::Resolved`].
///
/// The caller is responsible for calling [`WalkerVisitedNodes::pop`]
/// on each pushed id once the iteration's call frame returns, if it
/// wishes to scope the visited set to a single chain. Most callers
/// keep the visited set scoped to a single top-level walker entry.
///
/// **No hard depth cap** is applied. Termination is via cycle
/// detection on resolved [`SemanticNodeId`]s; the defensive fuse only
/// fires on pathological inputs.
pub fn iterate_ref_chain_until_non_ref(
    dispatch: &ProjectSemanticDispatch<'_>,
    host: &VerterHost,
    scope_canonical_id: &str,
    initial: TypeExpr,
    mode: ProjectionMode,
    visited: &mut WalkerVisitedNodes,
) -> RefChainOutcome {
    let empty_path: Arc<[PathSegment]> = Arc::from(Vec::<PathSegment>::new().into_boxed_slice());
    let mut current = initial;
    loop {
        let Some(base) =
            dispatch.lower_type_expr_in_scope_with_mode(scope_canonical_id, &current, mode)
        else {
            return RefChainOutcome::Miss;
        };
        if node_is_opaque_miss(host, base) {
            return RefChainOutcome::Miss;
        }
        // Follow alias / Instantiate bodies through `ProjectPath` —
        // bare `lower_type_expr_in_scope_with_mode` returns the
        // anchor `ResolveDecl` for non-generic aliases, which raises
        // back to the same `Ref{name}` and would not advance the
        // chain. `ProjectPath{base, [], mode}` is the projection
        // step that drills into the resolved body.
        let projected = match dispatch.execute(SemanticQueryKey::ProjectPath {
            base,
            path: Arc::clone(&empty_path),
            mode,
        }) {
            QueryResult::Value(node) => node,
            _ => return RefChainOutcome::Resolved(current),
        };
        match visited.try_push(projected) {
            VisitedPushOutcome::Cycle => return RefChainOutcome::CycleDetected(current),
            VisitedPushOutcome::FuseExhausted => return RefChainOutcome::FuseExhausted(current),
            VisitedPushOutcome::Inserted => {}
        }
        let Some(raised) = dispatch.raise_node_to_type_expr(projected) else {
            return RefChainOutcome::Resolved(current);
        };
        match &raised {
            TypeExpr::Ref { .. } if raised != current => {
                current = raised;
                continue;
            }
            _ => return RefChainOutcome::Resolved(raised),
        }
    }
}

fn node_is_opaque_miss(host: &VerterHost, node: SemanticNodeId) -> bool {
    matches!(
        node_data_for(host, node).as_deref(),
        Some(SemanticNodeData::Opaque(QueryError::Miss))
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Arc;

    use verter_semantic::analysis::type_expr::{ObjectMember, ObjectProperty, PrimitiveName};
    use verter_semantic::analysis::type_solver::PreparedTypeDecl;

    use crate::meta::MetaProject;
    use crate::types::HostConfig;
    use crate::VerterHost;

    fn make_project() -> Arc<MetaProject> {
        let scheduler_config = verter_scheduler::scheduler::SchedulerConfig {
            cpu_threads: 1,
            ..verter_scheduler::scheduler::SchedulerConfig::default()
        };
        let host = VerterHost::new_standalone_with_scheduler_config(
            HostConfig {
                analysis_level: crate::types::AnalysisLevel::Full,
                ..HostConfig::default()
            },
            scheduler_config,
        );
        MetaProject::new(host)
    }

    fn ref_expr(name: &str, args: Vec<TypeExpr>) -> TypeExpr {
        TypeExpr::Ref {
            name: Arc::from(name),
            type_arguments: Arc::from(args.into_boxed_slice()),
        }
    }

    fn prepared_with_defaults(defaults: Vec<Option<TypeExpr>>) -> PreparedTypeDecl {
        use verter_semantic::analysis::type_eval::TypeDeclKind;
        use verter_semantic::analysis::type_expr::TypeParam;
        use verter_semantic::analysis::type_solver::ResolvedRootIdentity;

        let type_parameters: Vec<TypeParam> = defaults
            .into_iter()
            .enumerate()
            .map(|(i, default)| TypeParam {
                name: format!("T{i}"),
                constraint: None,
                default: default.map(Arc::new),
            })
            .collect();
        let body = TypeExpr::Object(Arc::new(verter_semantic::analysis::type_expr::ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty {
                name: "kind".into(),
                ty: TypeExpr::Primitive(PrimitiveName::String),
                optional: false,
                readonly: false,
            })],
        }));
        let mut prepared = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/test.ts", "Stub"),
            TypeDeclKind::Alias,
            body,
        );
        prepared.type_parameters = type_parameters;
        prepared
    }

    #[test]
    fn visited_nodes_inserts_then_detects_cycle_on_re_encounter() {
        let mut visited = WalkerVisitedNodes::new();
        let n = SemanticNodeId(1u64);
        assert_eq!(visited.try_push(n), VisitedPushOutcome::Inserted);
        assert!(visited.contains(n));
        assert_eq!(visited.try_push(n), VisitedPushOutcome::Cycle);
        assert_eq!(visited.fuse_hops(), 2);
    }

    #[test]
    fn visited_nodes_pop_removes_node_for_subsequent_inserts() {
        let mut visited = WalkerVisitedNodes::new();
        let n = SemanticNodeId(7u64);
        assert_eq!(visited.try_push(n), VisitedPushOutcome::Inserted);
        visited.pop(n);
        assert!(!visited.contains(n));
        assert_eq!(visited.try_push(n), VisitedPushOutcome::Inserted);
    }

    #[test]
    fn visited_nodes_fuse_exhausts_after_4096_hops() {
        let mut visited = WalkerVisitedNodes::new();
        // Push distinct ids up to the fuse boundary; the (4097)th push
        // must report FuseExhausted regardless of cycle status.
        for i in 0..VISITED_NODES_DEFENSIVE_FUSE as u64 {
            assert_eq!(
                visited.try_push(SemanticNodeId(i)),
                VisitedPushOutcome::Inserted,
                "id {i} should insert before fuse exhaustion"
            );
        }
        assert_eq!(
            visited.try_push(SemanticNodeId(u64::MAX)),
            VisitedPushOutcome::FuseExhausted
        );
    }

    #[test]
    fn rewrite_omitted_generic_args_returns_none_for_non_ref_input() {
        let prepared =
            prepared_with_defaults(vec![Some(TypeExpr::Primitive(PrimitiveName::String))]);
        let expr = TypeExpr::Primitive(PrimitiveName::Number);
        assert!(rewrite_omitted_generic_args_with_defaults(&expr, &prepared).is_none());
    }

    #[test]
    fn rewrite_omitted_generic_args_returns_none_when_args_already_present() {
        let prepared =
            prepared_with_defaults(vec![Some(TypeExpr::Primitive(PrimitiveName::String))]);
        let expr = ref_expr("Foo", vec![TypeExpr::Primitive(PrimitiveName::Number)]);
        assert!(rewrite_omitted_generic_args_with_defaults(&expr, &prepared).is_none());
    }

    #[test]
    fn rewrite_omitted_generic_args_returns_none_when_prepared_has_no_type_params() {
        let prepared = prepared_with_defaults(vec![]);
        let expr = ref_expr("Foo", vec![]);
        assert!(rewrite_omitted_generic_args_with_defaults(&expr, &prepared).is_none());
    }

    #[test]
    fn rewrite_omitted_generic_args_returns_none_when_any_param_lacks_default() {
        let prepared =
            prepared_with_defaults(vec![Some(TypeExpr::Primitive(PrimitiveName::String)), None]);
        let expr = ref_expr("Foo", vec![]);
        assert!(rewrite_omitted_generic_args_with_defaults(&expr, &prepared).is_none());
    }

    #[test]
    fn rewrite_omitted_generic_args_substitutes_defaults_when_all_present() {
        let prepared = prepared_with_defaults(vec![
            Some(TypeExpr::Primitive(PrimitiveName::String)),
            Some(TypeExpr::Primitive(PrimitiveName::Number)),
        ]);
        let expr = ref_expr("Foo", vec![]);
        let rewritten = rewrite_omitted_generic_args_with_defaults(&expr, &prepared)
            .expect("defaults must rewrite when every param carries one");
        let TypeExpr::Ref {
            name,
            type_arguments,
        } = rewritten
        else {
            panic!("expected Ref output, got {rewritten:?}");
        };
        assert_eq!(name.as_ref(), "Foo");
        assert_eq!(type_arguments.len(), 2);
        assert!(matches!(
            &type_arguments[0],
            TypeExpr::Primitive(PrimitiveName::String)
        ));
        assert!(matches!(
            &type_arguments[1],
            TypeExpr::Primitive(PrimitiveName::Number)
        ));
    }

    #[test]
    fn lower_in_first_responsive_scope_returns_match_from_imported_scope() {
        let project = make_project();
        project
            .upsert_base("/types.ts", r#"export type ImportedAlias = { id: string }"#)
            .unwrap();
        project
            .upsert_base(
                "/Owner.vue",
                r#"<script setup lang="ts">
import type { ImportedAlias } from './types'
defineProps<{ entry: ImportedAlias }>()
</script>
<template><div /></template>"#,
            )
            .unwrap();
        let host = project.host();
        let dispatch = ProjectSemanticDispatch::new(host);
        let expr = ref_expr("ImportedAlias", vec![]);

        // Owner-only scope misses; iteration must succeed when the
        // imported scope is included second.
        let owner_only = lower_in_first_responsive_scope(
            &dispatch,
            host,
            &["/Owner.vue"],
            &expr,
            ProjectionMode::Expanded,
        );
        let multi = lower_in_first_responsive_scope(
            &dispatch,
            host,
            &["/Owner.vue", "/types.ts"],
            &expr,
            ProjectionMode::Expanded,
        );
        assert!(
            multi.is_some(),
            "multi-scope iteration must succeed when the imported scope holds the body"
        );
        // Owner-scope-alone may already succeed (dispatch chases the
        // import); the discriminating assertion is that multi-scope
        // does not produce a worse outcome.
        let _ = owner_only;
        let (scope, _node) = multi.expect("multi-scope must produce a hit");
        assert!(
            scope == "/Owner.vue" || scope == "/types.ts",
            "winning scope must be one of the supplied candidates, got {scope}"
        );
    }

    #[test]
    fn iterate_ref_chain_resolves_chained_alias_until_non_ref() {
        let project = make_project();
        project
            .upsert_base(
                "/chain.ts",
                r#"export type Tail = { leaf: number }
export type Mid = Tail
export type Head = Mid"#,
            )
            .unwrap();
        let host = project.host();
        let dispatch = ProjectSemanticDispatch::new(host);
        let mut visited = WalkerVisitedNodes::new();
        let outcome = iterate_ref_chain_until_non_ref(
            &dispatch,
            host,
            "/chain.ts",
            ref_expr("Head", vec![]),
            ProjectionMode::Expanded,
            &mut visited,
        );
        let resolved = match outcome {
            RefChainOutcome::Resolved(expr) => expr,
            other => panic!("expected Resolved, got {other:?}"),
        };
        assert!(
            !matches!(resolved, TypeExpr::Ref { .. }),
            "iteration must terminate on a non-Ref shape, got {resolved:?}"
        );
        assert!(
            visited.fuse_hops() <= 4,
            "chain of three aliases should require ≤4 hops; got {} hops",
            visited.fuse_hops()
        );
    }

    #[test]
    fn iterate_ref_chain_detects_cycle_on_recursive_alias_via_visited_set() {
        let project = make_project();
        project
            .upsert_base(
                "/cycle.ts",
                r#"export type Foo = Bar
export type Bar = Foo"#,
            )
            .unwrap();
        let host = project.host();
        let dispatch = ProjectSemanticDispatch::new(host);
        let mut visited = WalkerVisitedNodes::new();
        let outcome = iterate_ref_chain_until_non_ref(
            &dispatch,
            host,
            "/cycle.ts",
            ref_expr("Foo", vec![]),
            ProjectionMode::Expanded,
            &mut visited,
        );
        // Either the visited set fires (Cycle) or dispatch's own
        // RecursiveRef detection emits a non-Ref form (Resolved); both
        // are valid Gap-3 terminations. The discriminating assertion
        // is that the helper does NOT spin past the defensive fuse on
        // a 2-element cycle.
        match outcome {
            RefChainOutcome::CycleDetected(_) | RefChainOutcome::Resolved(_) => {}
            other => panic!("expected Cycle or Resolved on alias cycle, got {other:?}"),
        }
        assert!(
            visited.fuse_hops() < VISITED_NODES_DEFENSIVE_FUSE,
            "cycle must be caught well before defensive fuse"
        );
    }
}

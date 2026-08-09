//! The apparent-type surface of a callable.
//!
//! A type that carries call signatures exposes, in addition to its own
//! members, the members of the ambient callable-function interface — that
//! is where `.call` / `.apply` / `.bind` / `.name` live. This producer owns
//! that widening for the `SemanticQueryKey::ApparentType` family, so the
//! PathWalker never performs an ambient lookup of its own: the walker asks
//! for the apparent type and continues its member step against whatever
//! surface comes back.
//!
//! The widening is registry-proved, never spelled:
//!
//! 1. the base must BE callable — a [`SemanticNodeData::Signature`], a
//!    [`SemanticNodeData::DeferredCallable`], or an object surface carrying
//!    call signatures — reached through alias hops;
//! 2. a scoping canonical resolves the project whose ambient registry is
//!    consulted. An AUTHORED callable is scoped by its declaring canonical
//!    (the same per-canonical env scoping every other context builder on
//!    this dispatch uses). A ROOTLESS callable (a parameter annotation, a
//!    local arrow, an object-type call signature — no authored occurrence)
//!    is scoped by the LEXICAL DEMAND canonical carried in the key's
//!    [`ApparentDemandScope::Rootless`] witness — the canonical containing
//!    the member-access/call site that demanded the surface;
//! 3. the interface is the ambient `Function` that project's registered
//!    corpus exposes (see [`CALLABLE_APPARENT_INTERFACE`]);
//! 4. the surface is the ambient declaration's own `Instantiate` result, so
//!    every member carries the ambient canonical as its declaration origin.
//!
//! A base that is not callable, a rootless callable whose key carries no
//! demand-scope witness, a canonical with no owning project, and a project
//! whose ambient corpus registers no such interface all return
//! [`QueryError::Miss`]. A ROOTLESS value resolves semantically but NEVER
//! enters a shared cache: the build is marked `cache_suppress`, and that
//! taint folds through every enclosing member/path/call query at the
//! universal read boundary. The primitive-to-wrapper widening (`string` →
//! `String`, `number` → `Number`, …) is a separate surface and is not
//! produced here; those bases also return `Miss`.

use std::sync::Arc;

use verter_workspace::ProjectStableKey;

use crate::semantic_query::{
    ApparentDemandScope, ApparentTypeContext, ProjectionMode, ProjectionReductionContext,
    QueryError, QueryResult, SemanticNodeData, SemanticNodeId, SemanticQueryApi, SemanticQueryKey,
    SemanticQueryOutput,
};

use super::ProjectSemanticDispatch;

/// The ambient interface a callable's apparent members come from.
///
/// TypeScript's `globalCallableFunctionType` is `CallableFunction` when
/// `strictBindCallApply` is on and `Function` otherwise. Verter's project
/// model carries no `strictBindCallApply` input — `type_env_hash` composes a
/// fixed non-strict type env — so the apparent callable surface is
/// `Function`.
const CALLABLE_APPARENT_INTERFACE: &str = "Function";

/// The producer's base classification: how (and whether) a callable node
/// anchors the ambient lookup.
enum CallableAnchor {
    /// The node carries no call signatures at all — no apparent surface.
    NotCallable,
    /// The callable carries an authored occurrence; its declaring canonical
    /// scopes the lookup.
    Authored(Arc<str>),
    /// The callable is rootless (no authored occurrence): only a lexical
    /// demand canonical can scope the lookup.
    Rootless,
}

impl ProjectSemanticDispatch<'_> {
    /// Build the `ApparentType` value for `base`.
    pub(super) fn build_apparent_type(
        &self,
        base: SemanticNodeId,
        context: &ApparentTypeContext,
    ) -> crate::project_semantic_dispatch::walk::QueryBuildOutput {
        let fence = self.project_generation_signature();
        let miss = || -> crate::project_semantic_dispatch::walk::QueryBuildOutput {
            (
                QueryResult::Error(QueryError::Miss),
                self.project_generation_signature(),
            )
                .into()
        };
        // The classification authority is the NODE; the key's demand-scope
        // witness is consulted only for a rootless base. A rootless value
        // is transaction-local: `cache_suppress` keeps it (and every
        // enclosing projection that read it) out of the shared memo.
        let (canonical, rootless): (Arc<str>, bool) = match self.callable_anchor(base) {
            CallableAnchor::NotCallable => return miss(),
            CallableAnchor::Authored(canonical) => (canonical, false),
            CallableAnchor::Rootless => match &context.demand_scope {
                ApparentDemandScope::Rootless { canonical } => (Arc::clone(canonical), true),
                // No demand-scope witness — no project to scope the ambient
                // lookup by. Fail closed.
                ApparentDemandScope::Anchored => return miss(),
            },
        };
        let Some(project) = self.project_stable_key_for_canonical(canonical.as_ref()) else {
            return miss();
        };
        let Some(hit) = self
            .ctx
            .lookup_ambient_symbol(project, CALLABLE_APPARENT_INTERFACE)
        else {
            return miss();
        };
        // The consumer now depends on this ambient registration: a
        // re-registration of the lib invalidates it through the standard
        // dependency-fact validators. For a rootless base the recorded
        // consumer is the demand canonical — the demand-project read.
        self.ctx
            .record_ambient_dependency(canonical.as_ref(), hit.virtual_id.as_ref());

        let slot = self.type_slot_for(
            Arc::clone(&hit.virtual_id),
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            Arc::from(CALLABLE_APPARENT_INTERFACE),
        );
        let surface = self.execute_type_node(SemanticQueryKey::Instantiate(
            crate::semantic_query::InstantiateKey::new(
                slot,
                Arc::from(Vec::new().into_boxed_slice()),
                self.instantiate_context_for(
                    hit.virtual_id.as_ref(),
                    ProjectionReductionContext::published(ProjectionMode::Expanded),
                ),
            ),
        ));
        match surface {
            QueryResult::Value(SemanticQueryOutput { value, .. }) => {
                let mut output: crate::project_semantic_dispatch::walk::QueryBuildOutput =
                    (QueryResult::Value(value), fence).into();
                // A rootless apparent value never enters the shared memo;
                // the value still flows to the caller, and the suppress
                // taint folds through every enclosing member/path/call
                // query at the universal read boundary.
                output.cache_suppress |= rootless;
                output
            }
            QueryResult::Recursive(_) | QueryResult::Error(_) => miss(),
        }
    }

    /// The apparent type of `base`, or `None` when the family produced no
    /// surface. The single walker-side entry: it derives the key's context
    /// from the same canonical the producer scopes its lookup by, so key
    /// identity and value basis agree. An authored callable is scoped by
    /// its declaring canonical; a rootless callable is scoped by the
    /// innermost lexical demand canonical (the member-access/call site
    /// currently being evaluated) and carries that canonical in the key's
    /// demand-scope witness. A rootless base with NO demand site on the
    /// stack fails closed.
    pub(super) fn apparent_type_of(&self, base: SemanticNodeId) -> Option<SemanticNodeId> {
        let (canonical, demand_scope) = match self.callable_anchor(base) {
            CallableAnchor::NotCallable => return None,
            CallableAnchor::Authored(canonical) => (canonical, ApparentDemandScope::Anchored),
            CallableAnchor::Rootless => {
                let canonical = self.lexical_demand_scope.borrow().last().cloned()?;
                (
                    Arc::clone(&canonical),
                    ApparentDemandScope::Rootless { canonical },
                )
            }
        };
        let key = SemanticQueryKey::ApparentType {
            base,
            context: self.apparent_type_context_scoped(canonical.as_ref(), demand_scope),
        };
        match self.execute_type_node(key) {
            QueryResult::Value(SemanticQueryOutput { value, .. }) => Some(value),
            QueryResult::Recursive(_) | QueryResult::Error(_) => None,
        }
    }

    /// Production constructor for the env-bearing [`ApparentTypeContext`]:
    /// the key has no decl slot, so the R21 `T L J` dimensions the
    /// apparent surface depends on ride in the context, plus the
    /// demand-scope witness. The env dims derive from the SAME canonical
    /// the scope resolves by (the declaring canonical for `Anchored`, the
    /// witness canonical for `Rootless`), so key identity and value basis
    /// agree on both arms. No content/version hash enters the
    /// query-identity key (R6).
    #[must_use]
    fn apparent_type_context_scoped(
        &self,
        canonical: &str,
        demand_scope: ApparentDemandScope,
    ) -> ApparentTypeContext {
        let host = self.ctx.host_for_fact_tracer_install();
        let env = host.host_view_env_hashes_for(canonical);
        ApparentTypeContext {
            type_env_hash: env.type_env_hash,
            lib_env_hash: env.lib_env_hash,
            project_identity: host.host_view_project_identity_for(canonical).fold_u32(),
            demand_scope,
        }
    }

    /// Classify how `node` anchors a callable-apparent lookup, reached
    /// through alias hops. [`CallableAnchor::NotCallable`] for a node that
    /// carries no call signatures at all; [`CallableAnchor::Authored`] with
    /// the declaring canonical for a callable with an authored occurrence;
    /// [`CallableAnchor::Rootless`] for a callable with none (an inline
    /// function value, a function-typed parameter, an object-type call
    /// signature).
    fn callable_anchor(&self, node: SemanticNodeId) -> CallableAnchor {
        let graph = self.graph();
        let mut node = node;
        let mut visited: rustc_hash::FxHashSet<SemanticNodeId> = rustc_hash::FxHashSet::default();
        loop {
            if !visited.insert(node) {
                return CallableAnchor::NotCallable;
            }
            let Some(data) = graph.node_data(node) else {
                return CallableAnchor::NotCallable;
            };
            match &*data {
                SemanticNodeData::Alias(target) => node = *target,
                SemanticNodeData::Signature { occurrence, .. } => {
                    return match occurrence {
                        Some(occurrence) => CallableAnchor::Authored(Arc::clone(
                            &occurrence.function.anchor.canonical_id,
                        )),
                        None => CallableAnchor::Rootless,
                    };
                }
                SemanticNodeData::DeferredCallable(callable) => {
                    return CallableAnchor::Authored(Arc::clone(callable.declaring_canonical()));
                }
                // A surface carrying call signatures is callable too; its
                // FIRST call signature anchors the lookup (every signature of
                // one callable position is authored in the same file).
                SemanticNodeData::Object(surface) => {
                    let Some(first) = surface.call_signatures.first().copied() else {
                        return CallableAnchor::NotCallable;
                    };
                    drop(data);
                    node = first;
                }
                _ => return CallableAnchor::NotCallable,
            }
        }
    }

    /// Resolve `canonical`'s owning project to the stable key the ambient
    /// registry is partitioned by.
    fn project_stable_key_for_canonical(&self, canonical: &str) -> Option<ProjectStableKey> {
        let host = self.ctx.host_for_fact_tracer_install();
        let project = host.resolve_project_for_canonical(canonical)?;
        host.workspace().project_stable_key(project)
    }
}

#[cfg(test)]
#[path = "apparent_type_tests.rs"]
mod apparent_type_tests;

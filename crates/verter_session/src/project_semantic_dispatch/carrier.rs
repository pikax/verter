//! Demand-time carrier-resolution context.
//!
//! [`CarrierResolverContext`] bundles the read-only resolution inputs a
//! `BareRef` / `ImportType` carrier needs to resolve at demand time — the
//! same inputs the eager
//! [`shallow_lower_type_expr_with_context`](super::ProjectSemanticDispatch::shallow_lower_type_expr_with_context)
//! `Ref` path consumes. The carrier-resolution dispatch reads it instead of
//! threading six positional arguments through every hop.
//!
//! **It is a RUNTIME / VALUE-SIDE context, NEVER a query key.** None of its
//! fields — `name_resolution`, the `DeclarationScopePayload`, the
//! `ScopeShadowing` set, the substitution env, or the reduction-demand axis
//! — may be hashed into a [`SemanticQueryKey`](crate::semantic_query::SemanticQueryKey):
//! query keys stay the content-free slot/fact identities (R6), and the
//! materialised VALUE roots its version through the produced node's
//! [`NodeScopeId`] + read-set. To make that misuse structurally impossible
//! the context borrows its inputs and deliberately derives neither `Hash`
//! nor `Eq`, so it cannot be a map key nor embedded in a derived-`Hash`
//! cache key.
//!
//! Two pieces the eager `Ref` path also touches are threaded SEPARATELY at
//! resolution time rather than living on this read-only bundle:
//! - the mutable `substitutions: &mut Vec<(Arc<str>, SemanticNodeId)>`
//!   accumulator (a write sink, not a read input), and
//! - the dispatcher-local active-instantiate stack (`instantiate_active`),
//!   which is `ProjectSemanticDispatch` state, not per-resolution context.
//!
//! The ambient-augmentation scope is NOT a separate field: it is derived
//! from `scope` (the owning canonical) plus the resolver's augmentation
//! index, exactly as the eager `Ref` path derives it.

use rustc_hash::FxHashMap;

use verter_semantic::analysis::type_solver::host::ResolvedRootIdentity;

use crate::resolver_core::bare_name_resolve::DeclarationScopePayload;
use crate::resolver_core::scope_shadowing::ScopeShadowing;
use crate::semantic_query::{
    NodeScopeId, ProjectionMode, ProjectionReductionContext, SemanticNodeId,
};

/// Read-only, value-side resolution context for resolving a graph carrier
/// (`BareRef` / `ImportType`) at demand time. See the module docs for the
/// no-query-key contract.
// The carrier-resolution dispatch is the consumer; this slice defines the
// type + constructor + accessors, exercised today by the unit test below.
#[allow(dead_code)]
pub(crate) struct CarrierResolverContext<'a> {
    /// Type-parameter binder environment (`param_name → arg node`). A bare
    /// reference that names a bound parameter substitutes to its argument.
    env: &'a FxHashMap<String, SemanticNodeId>,
    /// The lexical scope the carrier was captured in — the declaration-origin
    /// file + content generation + optional inner scope. Drives bare-name
    /// resolution and ambient-augmentation lookup.
    scope: &'a NodeScopeId,
    /// The prepared-decl `name_resolution` fast-path map (already-resolved
    /// imports from the body-file scope). Consulted before the host-owned
    /// bare-name resolver fallback.
    name_resolution: &'a FxHashMap<String, ResolvedRootIdentity>,
    /// The owner declaration-scope payload (scope-local type names /
    /// bindings), consulted by the bare-name resolver fallback. `None` for a
    /// global scope or a pre-bundle fixture.
    scope_payload: Option<&'a DeclarationScopePayload>,
    /// The builtin-shadowing set: bare names whose userland declaration must
    /// win over a same-named ambient-lib builtin.
    shadowing: &'a ScopeShadowing,
    /// The reduction-demand axis (`Published` / `StructuralTransit`) plus the
    /// query mode — selects carrier-vs-execute at the demand point.
    reduction_context: ProjectionReductionContext,
}

#[allow(dead_code)]
impl<'a> CarrierResolverContext<'a> {
    /// Bundle the read-only resolution inputs the eager `Ref` lowering path
    /// consumes. The argument order mirrors
    /// [`shallow_lower_type_expr_with_context`](super::ProjectSemanticDispatch::shallow_lower_type_expr_with_context)
    /// (minus the lowered `expr` and the mutable `substitutions` sink).
    pub(crate) fn new(
        env: &'a FxHashMap<String, SemanticNodeId>,
        scope: &'a NodeScopeId,
        name_resolution: &'a FxHashMap<String, ResolvedRootIdentity>,
        scope_payload: Option<&'a DeclarationScopePayload>,
        shadowing: &'a ScopeShadowing,
        reduction_context: ProjectionReductionContext,
    ) -> Self {
        Self {
            env,
            scope,
            name_resolution,
            scope_payload,
            shadowing,
            reduction_context,
        }
    }

    /// The type-parameter binder environment.
    pub(crate) fn env(&self) -> &FxHashMap<String, SemanticNodeId> {
        self.env
    }

    /// The captured lexical scope.
    pub(crate) fn scope(&self) -> &NodeScopeId {
        self.scope
    }

    /// The prepared-decl `name_resolution` fast-path map.
    pub(crate) fn name_resolution(&self) -> &FxHashMap<String, ResolvedRootIdentity> {
        self.name_resolution
    }

    /// The owner declaration-scope payload, if any.
    pub(crate) fn scope_payload(&self) -> Option<&DeclarationScopePayload> {
        self.scope_payload
    }

    /// The builtin-shadowing set.
    pub(crate) fn shadowing(&self) -> &ScopeShadowing {
        self.shadowing
    }

    /// The reduction-demand context (axis + mode).
    pub(crate) fn reduction_context(&self) -> ProjectionReductionContext {
        self.reduction_context
    }

    /// The query mode, projected out of the reduction context.
    pub(crate) fn mode(&self) -> ProjectionMode {
        self.reduction_context.mode
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn carrier_resolver_context_bundles_resolution_inputs() {
        // Construct from the same read-only inputs the eager `Ref` path uses,
        // and assert every accessor returns the wired value.
        let mut env: FxHashMap<String, SemanticNodeId> = FxHashMap::default();
        env.insert("T".to_string(), SemanticNodeId(11));
        let mut name_resolution: FxHashMap<String, ResolvedRootIdentity> = FxHashMap::default();
        name_resolution.insert(
            "Foo".to_string(),
            ResolvedRootIdentity::new("/foo.ts", "Foo"),
        );
        let scope = NodeScopeId::Global;
        let shadowing = ScopeShadowing::empty();
        let reduction = ProjectionReductionContext::published(ProjectionMode::Navigate);

        let ctx = CarrierResolverContext::new(
            &env,
            &scope,
            &name_resolution,
            None,
            &shadowing,
            reduction,
        );

        assert_eq!(ctx.env().get("T"), Some(&SemanticNodeId(11)));
        assert!(matches!(ctx.scope(), NodeScopeId::Global));
        assert_eq!(
            ctx.name_resolution()
                .get("Foo")
                .map(|r| r.symbol_name.as_str()),
            Some("Foo")
        );
        assert!(ctx.scope_payload().is_none());
        // The shadow set is the empty set here (no userland shadow).
        let _ = ctx.shadowing();
        assert_eq!(ctx.mode(), ProjectionMode::Navigate);
        assert_eq!(ctx.reduction_context().mode, ProjectionMode::Navigate);
    }
}

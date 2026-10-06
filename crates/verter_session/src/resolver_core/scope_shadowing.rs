//! r15/F11 — `ScopeShadowing` resolver context.
//!
//! Captures, once per resolver context, the set of bare type names the
//! owner scope already declares. The dispatch fast-path
//! ([`crate::project_semantic_dispatch::lower::ProjectSemanticDispatch::shallow_lower_type_expr`])
//! and the graph-native root-identity predicates
//! (`meta_resolve::graph_predicates`) both consult `is_shadowing_lib(name)` before routing through the
//! ambient-lib `__builtin__` fast-path. When `true`, the userland
//! declaration wins and the `__builtin__` route is suppressed —
//! preserving the plan's "user shadowing wins" rule across BOTH
//! lowering entry points.
//!
//! **Design rationale:** an earlier draft threaded a bare `bool`
//! through every route + registry caller.
//! That replicates the parameter-explosion pattern that the
//! `ResolverContext` sealed-trait migration is designed to fix. By
//! introducing this struct now, the threading axis stays
//! single-source-of-truth and can absorb `ScopeShadowing`
//! as one input field of `ResolverContext` without inventing a
//! parallel axis to undo.
//!
//! **Construction sources (single-source-of-truth):**
//!
//! - [`ScopeShadowing::from_scope_payload`] — used by the dispatch
//!   lowering path (`lower.rs`) where the prepared
//!   [`DeclarationScopePayload`] is already on hand.
//! - [`ScopeShadowing::from_host_scope`] — used by the materialise
//!   path entry where only `(host, scope_canonical_id)` is on hand.
//!   Looks the prepared decl bundle up via
//!   [`crate::host_manage`]'s `prepared_decl_bundle` accessor and
//!   builds the same shadow set the dispatch path observes.
//! - [`ScopeShadowing::empty`] — for paths that do NOT have a
//!   declaration scope (e.g. global / `NodeScopeId::Global` lowering
//!   sites). Behaves as if no userland declaration shadows any
//!   builtin.
//!
//! Both constructors produce structurally-equivalent shadow sets so
//! the two lowering entry points agree on which builtin names are
//! shadowed in any given owner scope.

use crate::resolver_core::bare_name_resolve::DeclarationScopePayload;
// `from_host_scope` migrates to `&dyn ResolverContext`; the
// `crate::VerterHost` type is no longer needed in this file.

/// The bare type names the owner scope already declares. Consumed by the
/// dispatch fast-path and the materialise-path identity gate so `Pick<…>`
/// / `Omit<…>` / etc. resolve to the userland declaration when the SFC's
/// same-file scope already declares one.
///
/// A VIEW over the owner scope's declaration-scope payload, as
/// [`DeclarationScopePayload`] itself is: construction is one refcount
/// bump, and a probe reads the prepared bundle's three name surfaces in
/// place. Folding them into a set of its own made every construction walk
/// every name the file declares — one construction per instantiation, so
/// a file's instantiations cost the square of its size.
///
/// See module docs for the construction-source matrix.
#[derive(Debug, Clone)]
pub(crate) struct ScopeShadowing {
    /// The owner scope's payload; `None` shadows nothing.
    payload: Option<DeclarationScopePayload>,
}

impl ScopeShadowing {
    /// The owner scope's payload this set was built from.
    #[cfg(any(test, feature = "test-support"))]
    #[cfg_attr(not(test), allow(dead_code))]
    #[doc(hidden)]
    pub(crate) fn payload_for_tests(&self) -> Option<&DeclarationScopePayload> {
        self.payload.as_ref()
    }

    /// The empty shadow set. Used by lowering call sites that have no
    /// declaration-scope payload on hand (`NodeScopeId::Global`,
    /// pre-bundle test fixtures). Equivalent to "no userland
    /// declaration shadows any builtin" — the ambient-lib fast-path
    /// stays active.
    pub(crate) fn empty() -> Self {
        Self { payload: None }
    }

    /// The shadow set of a [`DeclarationScopePayload`] — dispatch-path
    /// entry point. The payload's `scope_type_names` (covering
    /// script-setup type params + scope-local type aliases),
    /// `scope_type_bindings` (covering script-setup generics), AND
    /// `import_bindings` (covering imported names) each shadow a
    /// same-named ambient-lib builtin per the foundation (`524f469d`)
    /// gate.
    ///
    /// `import_bindings` membership is load-bearing for the carrier
    /// head-resolution path, which rehydrates an EMPTY `name_resolution`
    /// from the scope payload: the eager `Ref` path suppresses the builtin
    /// fast-path because an imported name (e.g. `import type { Partial }`)
    /// lives in `name_resolution`, but the carrier path has none — so the
    /// import binding must shadow the builtin THROUGH this set instead, or
    /// an imported `Partial` would wrongly resolve to `__builtin__.Partial`.
    pub(crate) fn from_scope_payload(payload: Option<&DeclarationScopePayload>) -> Self {
        Self {
            payload: payload.cloned(),
        }
    }

    /// The shadow set of `(host, scope_canonical_id)` — materialise-path
    /// entry point. Mirrors the dispatch-path shape by going through the
    /// host's prepared decl bundle so both paths observe identical
    /// scope-type-name / scope-type-binding sets.
    ///
    /// Returns [`ScopeShadowing::empty`] when the host has no bundle
    /// for the canonical id (e.g. the file is unknown to the
    /// scheduler). This matches the dispatch path's behaviour when
    /// `scope_payload` is `None`.
    pub(crate) fn from_host_scope<C: crate::resolver_core::ResolverCapabilities>(
        ctx: &dyn crate::resolver_core::ResolverContext<C>,
        scope_canonical_id: &str,
        owner: verter_type_expr::TopLevelOwnerId,
    ) -> Self {
        match ctx.prepared_decl_bundle(scope_canonical_id) {
            Some(bundle) => Self::from_prepared_decl_bundle(&bundle, owner),
            None => Self::empty(),
        }
    }

    /// The shadow set of a [`PreparedDeclBundle`]'s owner scope: the SAME
    /// three bundle surfaces [`Self::from_scope_payload`] reads through the
    /// payload view (`scope_type_names` + `script_setup_type_bindings`
    /// keys + `import_bindings` keys). Keeping the two construction shapes
    /// aligned is the load-bearing invariant: the dispatch path and the
    /// materialise path MUST observe the same shadow set per scope.
    pub(crate) fn from_prepared_decl_bundle(
        bundle: &impl crate::resolver_core::bare_name_resolve::PreparedInputSource,
        owner: verter_type_expr::TopLevelOwnerId,
    ) -> Self {
        Self::from_scope_payload(Some(&DeclarationScopePayload::from_bundle(bundle, owner)))
    }

    /// Returns `true` when `name` is declared in the owner scope and
    /// therefore shadows a same-named ambient-lib builtin
    /// (`Pick`, `Omit`, `Partial`, `Exclude`, …). The dispatch
    /// fast-path and the materialise-path identity gate suppress
    /// their `__builtin__` route when this returns `true`,
    /// dispatching through the standard `ResolveDecl` path so the
    /// userland declaration wins. Three hash probes, whatever the scope's
    /// size.
    pub(crate) fn is_shadowing_lib(&self, name: &str) -> bool {
        Self::scope_payload_shadows_lib(self.payload.as_ref(), name)
    }

    /// Whether `name` is shadowed in `payload`'s scope, answered without
    /// constructing a shadow set: it probes the three sources
    /// [`Self::is_shadowing_lib`] reads — pinned by
    /// `payload_probe_agrees_with_the_folded_shadow_set`.
    pub(crate) fn scope_payload_shadows_lib(
        payload: Option<&DeclarationScopePayload>,
        name: &str,
    ) -> bool {
        payload.is_some_and(|payload| {
            payload.scope_type_names().contains(name)
                || payload.scope_type_bindings().contains_key(name)
                || payload.import_bindings().contains_key(name)
        })
    }
}

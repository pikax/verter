#![deny(missing_docs)]
//! `VerterHost::resolve_shallow_surface` — the typeinfo-owned PUBLIC accessor
//! for a named declaration's span-rich one-level surface ([`TypeInfoSurface`]).
//!
//! This is the public projection layer over the shared semantic graph: it runs
//! the EMPTY-PATH `Shallow` `ProjectPath` terminal-surface synthesiser on the
//! declaration carrier (so heritage / intersection arms are merged under the
//! own-body-shadows-heritage rule), reads the resulting
//! [`SemanticNodeData::Object`]'s [`SurfaceView`], and projects it into the
//! span-rich [`TypeInfoSurface`].
//!
//! The internal `SemanticQueryKey` family still returns a `SemanticNodeId`; the
//! PUBLIC accessor returns the typeinfo-owned [`TypeInfoSurface`] (spans + ids +
//! flags + interned names), never the graph-internal `SurfaceView`.

use std::sync::Arc;

use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::semantic_query::{
    PathSegment, ProjectionMode, ProjectionReductionContext, QueryResult, ResolveDeclKey, ScopeId,
    SemanticNodeData, SemanticNodeId, SemanticQueryApi, SemanticQueryKey, SemanticQueryOutput,
};
use crate::typeinfo::surface::TypeInfoSurface;
use crate::typeinfo::types::{ShallowSurfaceRequest, TypeInfoQueryLevel};
use crate::VerterHost;

impl VerterHost {
    /// Resolve `name` in `canonical_id` to its span-rich one-level
    /// [`TypeInfoSurface`] at [`TypeInfoQueryLevel::FullMetadata`].
    ///
    /// Thin wrapper over [`Self::resolve_shallow_surface_for`] — the historical
    /// `(canonical, name)` accessor preserved for the many existing callers
    /// that always want full metadata.
    ///
    /// Runs the empty-path `Shallow` projection on the declaration carrier and
    /// projects the resulting object surface. Returns `None` when the symbol
    /// does not resolve, or when its terminal surface is not an object (a bare
    /// alias to a primitive / union / function has no one-level member surface).
    ///
    /// The surface is shallow-by-default: each member's `value` is a
    /// `SemanticNodeId` reference, not an expanded body. A consumer that needs a
    /// member's body issues a path projection rooted at that `value`.
    #[must_use]
    pub fn resolve_shallow_surface(
        &self,
        canonical_id: &str,
        name: &str,
    ) -> Option<TypeInfoSurface> {
        self.resolve_shallow_surface_for(&ShallowSurfaceRequest::new(
            Arc::from(canonical_id),
            Arc::from(name),
            TypeInfoQueryLevel::FullMetadata,
        ))
    }

    /// Resolve a declaration to its span-rich one-level [`TypeInfoSurface`]
    /// through the level-aware [`ShallowSurfaceRequest`].
    ///
    /// The [`TypeInfoQueryLevel`] is query identity, NOT an env-hash dimension
    /// (R21). For a plain TS declaration both levels resolve the SAME one-level
    /// surface — a named TS declaration has no "public vs full" distinction, so
    /// the underlying `ResolveDecl` + empty-path `Shallow` `ProjectPath`
    /// dispatch is level-independent and the two levels correctly share the
    /// content-addressed dispatch memo slot. The level divergence bites for
    /// `.vue` carriers, which the [`crate::typeinfo::adapters::vue`] adapter
    /// owns: a `.vue`'s PUBLIC component type is the synthesized `default`
    /// instance surface (`$props`/`$emit`/`$slots`), resolved via
    /// [`crate::VerterHost::resolve_vue_public_type`], not a user-named
    /// declaration reached through this path.
    #[must_use]
    pub fn resolve_shallow_surface_for(
        &self,
        request: &ShallowSurfaceRequest,
    ) -> Option<TypeInfoSurface> {
        // Query-RETURNER: it returns the shallow surface with no outer
        // publish fence, so it MUST resolve against a PROVEN-CURRENT
        // snapshot. On sustained churn surface a miss (`None`) — the
        // established surface miss signal — rather than a surface resolved
        // against superseded state. The bounded retry terminates.
        let current_view = crate::typeinfo::current_store_view_for_query(self)?;
        let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
        let host_ctx =
            crate::resolver_core::HostResolverContext::from_current(self, &current_view, overlay);
        let dispatch = ProjectSemanticDispatch::new(&host_ctx);

        // Base = the declaration CARRIER (a `DeclPlaceholder`), NOT a
        // pre-instantiated body. The empty-path Shallow synthesiser's decl-root
        // unwrap re-establishes the consuming declaration's KIND (interface /
        // class vs alias) and classifies its heritage arms.
        let base = match dispatch.execute_type_node(SemanticQueryKey::ResolveDecl(ResolveDeclKey {
            scope: ScopeId {
                canonical_id: Arc::clone(&request.canonical_id),
                local_scope: None,
            },
            name: Arc::clone(&request.name),
        })) {
            QueryResult::Value(SemanticQueryOutput { value: node, .. }) => node,
            QueryResult::Recursive(node) => node,
            QueryResult::Error(_) => return None,
        };

        self.project_shallow_surface_from_base(
            &host_ctx,
            &dispatch,
            base,
            Arc::from(Vec::<PathSegment>::new().into_boxed_slice()),
            ProjectionReductionContext::published(ProjectionMode::Shallow),
        )
    }

    /// Project a resolved base node to its span-rich one-level
    /// [`TypeInfoSurface`] via a `Shallow` `ProjectPath` synthesiser + JSDoc
    /// enrichment. Shared by the named-declaration accessor
    /// ([`Self::resolve_shallow_surface_for`]) and the Vue-macro surface
    /// adapter, so both produce the surface through ONE code path.
    ///
    /// `path` is the path-precise selector applied to `base` BEFORE the
    /// one-level surface synthesis. Most callers pass the empty path (the base
    /// IS the surface root). The Vue-macro adapter passes a non-empty path when
    /// the macro type argument is a deep indexed access
    /// (`defineProps<DeepConfig['ui']['header']>()`): the shared `ProjectPath`
    /// walker runs the intermediate hops (`['ui']`) in `Navigate` and the
    /// TERMINAL hop (`['header']`) in the caller's mode, so the leaf object's
    /// members surface without the intermediate siblings leaking — the
    /// path-precise rule. A bare empty-path synthesiser would instead see the
    /// unreduced `IndexedAccess` carrier and yield NO members.
    ///
    /// `context` is the `ProjectPath` reduction context. The named-declaration
    /// accessor passes `published(Shallow)` (structural provenance). The Vue
    /// **props** macro normalizer passes `macro_object_surface(Shallow,
    /// MacroTypeArgOwnBody)` so the macro type-argument's own-body members
    /// surface with `declared_in_macro_type_arg = true` while heritage-reached
    /// members stay `false` — the same own-body-vs-heritage provenance the eager
    /// rail records. `mode` MUST stay `Shallow` so the surface is one-level
    /// (member values stay reference-style).
    pub(crate) fn project_shallow_surface_from_base(
        &self,
        ctx: &dyn crate::resolver_core::ResolverContext,
        dispatch: &ProjectSemanticDispatch<'_>,
        base: SemanticNodeId,
        path: Arc<[PathSegment]>,
        context: ProjectionReductionContext,
    ) -> Option<TypeInfoSurface> {
        debug_assert_eq!(
            context.mode,
            ProjectionMode::Shallow,
            "project_shallow_surface_from_base synthesises a one-level surface; mode must be Shallow"
        );
        // Path-precise `Shallow` projection synthesises the one-level surface
        // (call / construct / index signatures + merged members) without
        // expanding member bodies. An empty `path` projects `base`'s own
        // one-level surface; a non-empty `path` walks the selector hops first
        // (intermediate hops `Navigate`, terminal in the caller's mode) and
        // synthesises the LEAF's surface. This path PRESERVES call / construct
        // signatures, so an emit interface's call signatures survive here (the
        // emit normalizer reads them).
        let terminal = match dispatch.execute_type_node(SemanticQueryKey::ProjectPath {
            base,
            path,
            context,
        }) {
            QueryResult::Value(SemanticQueryOutput { value: node, .. }) => node,
            QueryResult::Recursive(node) => node,
            QueryResult::Error(_) => return None,
        };

        let graph = ctx.project_type_store().semantic_graph();
        let surface = match graph.node_data(terminal).as_deref() {
            Some(SemanticNodeData::Object(view)) => TypeInfoSurface::build(graph, view),
            _ => return None,
        };

        // Enrich each member with its leading-JSDoc spans, sliced from the
        // member's DECLARATION file's cache-owned RAW source
        // (`IndexedReady.raw_source`). Member/signature spans are SFC-absolute
        // (the eval source is position-preserving), so the JSDoc anchor offset
        // and the slice source share the raw-file coordinate system. `build` is
        // a pure graph projection that holds no source, so this source-touching
        // step lives at the host layer. An inherited member's JSDoc is read from
        // its origin (heritage base) file via the member's `declaration_origin`
        // — see `TypeInfoSurface::with_member_jsdoc_spans`. The carrier-file
        // raw source is read through the SAME `ctx` the surface was projected
        // under, so an overlay session reads its overlay raw source.
        Some(surface.with_member_jsdoc_spans(|canonical| {
            ctx.ensure_indexed_ready_serve(canonical)
                .map(|serve| Arc::clone(&serve.indexed.raw_source))
        }))
    }
}

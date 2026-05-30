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
use crate::resolver_core::ResolverContext;
use crate::semantic_query::{
    ProjectionMode, ProjectionReductionContext, QueryResult, ResolveDeclKey, ScopeId,
    SemanticNodeData, SemanticNodeId, SemanticQueryApi, SemanticQueryKey,
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
        let store_view = self.resolver_store_view();
        let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
        let host_ctx = crate::resolver_core::HostResolverContext::new(self, &store_view, overlay);
        let dispatch = ProjectSemanticDispatch::new(&host_ctx);

        // Base = the declaration CARRIER (a `DeclPlaceholder`), NOT a
        // pre-instantiated body. The empty-path Shallow synthesiser's decl-root
        // unwrap re-establishes the consuming declaration's KIND (interface /
        // class vs alias) and classifies its heritage arms.
        let base = match dispatch.execute(SemanticQueryKey::ResolveDecl(ResolveDeclKey {
            scope: ScopeId {
                canonical_id: Arc::clone(&request.canonical_id),
                local_scope: None,
            },
            name: Arc::clone(&request.name),
        })) {
            QueryResult::Value(node) | QueryResult::Recursive(node) => node,
            QueryResult::Error(_) => return None,
        };

        self.project_shallow_surface_from_base(
            &host_ctx,
            &dispatch,
            base,
            ProjectionReductionContext::published(ProjectionMode::Shallow),
        )
    }

    /// Project a resolved base node to its span-rich one-level
    /// [`TypeInfoSurface`] via the empty-path `Shallow` synthesiser + JSDoc
    /// enrichment. Shared by the named-declaration accessor
    /// ([`Self::resolve_shallow_surface_for`]) and the Vue-macro surface
    /// adapter, so both produce the surface through ONE code path.
    ///
    /// `context` is the empty-path `ProjectPath` reduction context. The
    /// named-declaration accessor passes `published(Shallow)` (structural
    /// provenance). The Vue **props** macro normalizer passes
    /// `published_macro_type_arg_body(Shallow)` so the macro type-argument's
    /// own-body members surface with `declared_in_macro_type_arg = true` while
    /// heritage-reached members stay `false` — the same own-body-vs-heritage
    /// provenance the eager rail records. `mode` MUST stay `Shallow` so the
    /// surface is one-level (member values stay reference-style).
    pub(crate) fn project_shallow_surface_from_base(
        &self,
        host_ctx: &crate::resolver_core::HostResolverContext<'_>,
        dispatch: &ProjectSemanticDispatch<'_>,
        base: SemanticNodeId,
        context: ProjectionReductionContext,
    ) -> Option<TypeInfoSurface> {
        debug_assert_eq!(
            context.mode,
            ProjectionMode::Shallow,
            "project_shallow_surface_from_base synthesises a one-level surface; mode must be Shallow"
        );
        // Empty-path Shallow projection synthesises the one-level surface
        // (call / construct / index signatures + merged members) without
        // expanding member bodies. This path PRESERVES call / construct
        // signatures, so an emit interface's call signatures survive here (the
        // emit normalizer reads them).
        let terminal = match dispatch.execute(SemanticQueryKey::ProjectPath {
            base,
            path: Arc::from(Vec::new().into_boxed_slice()),
            context,
        }) {
            QueryResult::Value(node) | QueryResult::Recursive(node) => node,
            QueryResult::Error(_) => return None,
        };

        let graph = host_ctx.project_type_store().semantic_graph();
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
        // — see `TypeInfoSurface::with_member_jsdoc_spans`.
        Some(surface.with_member_jsdoc_spans(|canonical| {
            self.ensure_indexed_ready(canonical)
                .map(|indexed| Arc::clone(&indexed.raw_source))
        }))
    }
}

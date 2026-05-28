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
    SemanticNodeData, SemanticQueryApi, SemanticQueryKey,
};
use crate::typeinfo::surface::TypeInfoSurface;
use crate::VerterHost;

impl VerterHost {
    /// Resolve `name` in `canonical_id` to its span-rich one-level
    /// [`TypeInfoSurface`].
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
                canonical_id: Arc::from(canonical_id),
                local_scope: None,
            },
            name: Arc::from(name),
        })) {
            QueryResult::Value(node) | QueryResult::Recursive(node) => node,
            QueryResult::Error(_) => return None,
        };

        // Empty-path Shallow projection synthesises the one-level surface
        // (call / construct / index signatures + merged members) without
        // expanding member bodies.
        let terminal = match dispatch.execute(SemanticQueryKey::ProjectPath {
            base,
            path: Arc::from(Vec::new().into_boxed_slice()),
            context: ProjectionReductionContext::published(ProjectionMode::Shallow),
        }) {
            QueryResult::Value(node) | QueryResult::Recursive(node) => node,
            QueryResult::Error(_) => return None,
        };

        let graph = host_ctx.project_type_store().semantic_graph();
        match graph.node_data(terminal).as_deref() {
            Some(SemanticNodeData::Object(view)) => Some(TypeInfoSurface::build(graph, view)),
            _ => None,
        }
    }
}

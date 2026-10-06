use std::sync::Arc;

use rustc_hash::FxHashMap;
use verter_type_expr::MemberVisibility;

use crate::typeinfo::surface::TypeInfoSurfaceMember;
use verter_type_engine::semantic_query::{
    PathSegment, ProjectionMode, ProjectionReductionContext, QueryResult, ResolveDeclKey, ScopeId,
    SemanticQueryKey, SurfaceProvenanceContext,
};

/// One keep-all class-member visibility row published to component-meta.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedNativeProp {
    pub name: String,
    pub is_optional: bool,
    pub type_annotation: Option<String>,
    pub visibility: MemberVisibility,
    /// The wire has no declaration-file identity, so offsets cannot be
    /// anchored honestly and remain the wire-default span.
    pub span: verter_span::Span,
}

impl ResolvedNativeProp {
    pub(crate) fn from_surface_member(
        member: &TypeInfoSurfaceMember,
        type_annotation: Option<String>,
    ) -> Option<Self> {
        Some(Self {
            name: member.published_name()?.to_string(),
            is_optional: member.optional,
            type_annotation,
            visibility: member.visibility,
            span: verter_span::Span::default(),
        })
    }
}

/// Typed result of a component-meta native visibility projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedNativePropsOutcome {
    /// The declaration resolved to an object surface. Empty is authoritative.
    Resolved(Vec<ResolvedNativeProp>),
    /// A transient dispatch recursion back-edge; never negative-cache.
    Recursive,
    /// A genuine declaration or non-object miss.
    Miss,
}

/// Request-local native projection memo, deliberately separate from the
/// compile-facing external body cache.
#[derive(Debug, Default)]
pub struct NativePropProjectionCache {
    entries: FxHashMap<
        (String, verter_type_expr::TopLevelOwnerId, String),
        Option<Vec<ResolvedNativeProp>>,
    >,
}

impl NativePropProjectionCache {
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub(crate) fn get(
        &self,
        key: &(String, verter_type_expr::TopLevelOwnerId, String),
    ) -> Option<&Option<Vec<ResolvedNativeProp>>> {
        self.entries.get(key)
    }

    pub(crate) fn insert(
        &mut self,
        key: (String, verter_type_expr::TopLevelOwnerId, String),
        value: Option<Vec<ResolvedNativeProp>>,
    ) -> Option<Option<Vec<ResolvedNativeProp>>> {
        self.entries.insert(key, value)
    }
}

/// Resolve one named declaration to component-meta's native visibility rows.
/// This projection owns no runtime DTO and performs one graph-only shallow
/// demand. Member display rendering is publication-only.
pub(crate) fn named_native_props_outcome(
    ctx: &dyn verter_type_engine::resolver_core::ResolverContext<
        crate::resolver_core::HostCapabilities,
    >,
    dispatch: &verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch<
        '_,
        crate::resolver_core::HostCapabilities,
    >,
    root_canonical: &str,
    root_owner: verter_type_expr::TopLevelOwnerId,
    root_name: &str,
) -> ResolvedNativePropsOutcome {
    let read = dispatch.execute_read(SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: ScopeId {
            canonical_id: Arc::from(root_canonical),
            owner: root_owner,
            local_scope: None,
            binder_scope_id: verter_type_engine::semantic_query::BinderScopeId::file_scope(
                root_owner,
            ),
        },
        name: Arc::from(root_name),
    }));
    verter_type_engine::meta_resolve::emit_dispatch_dep_signature_facts(
        dispatch,
        &read.dep_signature,
    );
    let (base, recursive) = match read.value {
        QueryResult::Value(node) => (node, false),
        QueryResult::Recursive(node) => (node, true),
        QueryResult::Error(_) => return ResolvedNativePropsOutcome::Miss,
    };

    // An INCOMPLETE projection records its typed reason before degrading to
    // the miss outcome — a failed resolution never reads as a props-less
    // declaration.
    let Some(surface) = crate::typeinfo::shallow_surface::project_shallow_surface_graph_only(
        ctx,
        dispatch,
        base,
        Arc::from(Vec::<PathSegment>::new().into_boxed_slice()),
        ProjectionReductionContext::macro_object_surface(
            ProjectionMode::Shallow,
            SurfaceProvenanceContext::MacroTypeArgOwnBody,
        ),
        None,
    )
    .recorded() else {
        return if recursive {
            ResolvedNativePropsOutcome::Recursive
        } else {
            ResolvedNativePropsOutcome::Miss
        };
    };

    let rows = surface
        .members
        .iter()
        .filter_map(|member| {
            ResolvedNativeProp::from_surface_member(
                member,
                crate::typeinfo::raise::render_node_display_with_ctx(dispatch, member.value)
                    .map(|rendered| rendered.text),
            )
        })
        .collect();
    ResolvedNativePropsOutcome::Resolved(rows)
}

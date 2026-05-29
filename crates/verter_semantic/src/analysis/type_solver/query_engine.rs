//! Projection result types used by component-meta query engine consumers.
//!
//! The standalone `TypeQueryEngine` has been retired along with the arena
//! solver kernel. What remains are the simple data carriers that
//! `verter_session::resolver_core::component_meta_query_engine` surfaces as
//! return types.

use verter_type_expr::{MemberSpans, TypeExpr};

// ---------------------------------------------------------------------------
// Projection result types
// ---------------------------------------------------------------------------

/// A single projected member from a type surface.
#[derive(Debug, Clone)]
pub struct ProjectedMember {
    pub name: String,
    pub ty: TypeExpr,
    pub optional: bool,
    pub readonly: bool,
    pub is_method: bool,
    /// Whether this member was explicitly declared in the macro's type
    /// argument's own body (vs reached via heritage / Omit / intersection
    /// from an external source). See [`ResolvedProp::declared_in_macro_type_arg`]
    /// in `verter_parser` for the structural definition. Propagated by
    /// `surface_projector` and prepared-surface walker.
    pub declared_in_macro_type_arg: bool,
    /// OXC declaration-site spans for this member (declaration / name /
    /// type-annotation byte offsets), mirroring [`verter_type_expr::MemberSpans`]
    /// and carried verbatim from the source the projection was built from — the
    /// graph `SurfaceMember::spans` (in `verter_session`), a
    /// [`PreparedMember::spans`](super::prepared::PreparedMember::spans), or the
    /// IR [`verter_type_expr::ObjectProperty::spans`] /
    /// [`verter_type_expr::MethodSignature::spans`].
    ///
    /// Offsets are in the member's DECLARATION file's source coordinates; the
    /// file itself is implicit (the projection's scope) and not carried here —
    /// the component-meta reconstruction (`projected_surface_to_type_expr`) only
    /// re-emits these offsets onto the IR, which is likewise file-implicit.
    ///
    /// Spans are content-version facts: they are NOT a query-identity key
    /// dimension and never enter `parse_stable_hash`. `None` components ONLY for
    /// a genuinely synthetic member (a union common-member, a mapped-produced
    /// member, a generated macro artifact) with no single OXC declaration site —
    /// never as a "not implemented" placeholder.
    pub spans: MemberSpans,
}

/// The projected keyspace of a type surface — the set of known member names.
#[derive(Debug, Clone)]
pub struct ProjectedKeyspace {
    /// Concrete member names (from object properties, mapped finite keys).
    pub members: Vec<String>,
    /// Whether the keyspace also includes an open index signature.
    pub has_index_signature: bool,
}

/// The full projected surface of a type — all concrete members.
#[derive(Debug, Clone)]
pub struct ProjectedSurface {
    pub members: Vec<ProjectedMember>,
    /// Call signatures (for callable emits).
    pub call_signatures: Vec<TypeExpr>,
    /// Construct signatures.
    pub construct_signatures: Vec<TypeExpr>,
    /// Whether the surface includes an open index signature.
    pub has_index_signature: bool,
}

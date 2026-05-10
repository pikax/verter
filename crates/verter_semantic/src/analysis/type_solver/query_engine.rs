//! Projection result types used by component-meta query engine consumers.
//!
//! The standalone `TypeQueryEngine` has been retired along with the arena
//! solver kernel. What remains are the simple data carriers that
//! `verter_session::resolver_core::component_meta_query_engine` surfaces as
//! return types.

use verter_type_expr::TypeExpr;

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

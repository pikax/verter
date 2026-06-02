//! Projection result types used by component-meta query engine consumers.
//!
//! The standalone `TypeQueryEngine` has been retired along with the arena
//! solver kernel. What remains are the simple data carriers that
//! `verter_session::resolver_core::component_meta_query_engine` surfaces as
//! return types.

use std::sync::Arc;

use verter_type_expr::{IndexSignatureSpans, MemberSpans, TypeExpr};

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
    /// Declared accessibility of the member, carried verbatim from the
    /// projection source (`SurfaceMember::visibility` / the IR member's
    /// `visibility`). Reconstruction back to an IR
    /// [`verter_type_expr::ObjectProperty`] / [`verter_type_expr::MethodSignature`]
    /// (`projected_surface_to_type_expr`) MUST preserve this via
    /// `with_visibility` — `with_spans` would silently upgrade a non-public
    /// member to `Public`, a fidelity loss for the `native_props` carrier and a
    /// leak on any published surface. `Public` for every non-class origin.
    pub visibility: verter_type_expr::MemberVisibility,
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
    /// Offsets are in the member's DECLARATION file's source coordinates — the
    /// file recorded by [`Self::declaration_origin`]. A consumer pairs these
    /// offsets with that file (NOT with the projection scope, which for a
    /// cross-file `Pick<ImportedType, ...>` / `A & B` is a DIFFERENT file).
    ///
    /// Spans are content-version facts: they are NOT a query-identity key
    /// dimension and never enter `parse_stable_hash`. `None` components ONLY for
    /// a genuinely synthetic member (a union common-member, a mapped-produced
    /// member, a generated macro artifact) with no single OXC declaration site —
    /// never as a "not implemented" placeholder.
    pub spans: MemberSpans,
    /// Canonical file the member's DECLARATION (its `name` / `: T` annotation)
    /// lives in — mirrors [`SurfaceMember::declaration_origin`] /
    /// [`PreparedMember::declaration_origin`](super::prepared::PreparedMember::declaration_origin).
    /// Carried verbatim from the projection source: the graph
    /// `SurfaceMember::declaration_origin`, the prepared member's defining file,
    /// or the LOWERING scope of an IR object literal. A consumer pairs the
    /// [`Self::spans`] offsets with THIS file so a cross-file projected surface
    /// (`Pick<ImportedType, "x">` where `ImportedType` lives in a different file
    /// than the importing scope, or a cross-file intersection `A & B`) interprets
    /// the offsets against the correct source. `None` only for a genuinely
    /// synthetic / multi-origin member (a union common-member, a mapped-produced
    /// member) — the same members whose [`Self::spans`] are absent.
    pub declaration_origin: Option<Arc<str>>,
}

/// A single projected index signature (`[k: K]: V`) from a type surface.
///
/// Carries the declared key/value type shape AND the OXC declaration-site spans
/// so the component-meta reconstruction (`projected_surface_to_type_expr`)
/// re-emits a real `[k: K]: V` rather than collapsing it to the synthetic
/// open-surface placeholder. Populated from the graph
/// [`SurfaceView::index_signatures`](../../../../../verter_session) and the IR
/// [`verter_type_expr::IndexSignature`] at the object-expr projection sites.
///
/// `spans` follows the same provenance contract as
/// [`ProjectedMember::spans`]: real offsets in the declaration file, `None`
/// only for a genuinely synthetic index signature with no single OXC site.
#[derive(Debug, Clone)]
pub struct ProjectedIndexSignature {
    /// The index key parameter name (`k` in `[k: K]: V`). Display-only.
    pub key_name: String,
    /// The declared key type (`K`).
    pub key_type: TypeExpr,
    /// The declared value type (`V`).
    pub value_type: TypeExpr,
    pub readonly: bool,
    /// OXC declaration-site spans for this index signature, carried verbatim
    /// from the graph / IR source. `None` components only for a genuinely
    /// synthetic index signature.
    pub spans: IndexSignatureSpans,
    /// Canonical file the index-signature DECLARATION lives in — mirrors
    /// [`ProjectedMember::declaration_origin`]. `None` only for a genuinely
    /// synthetic index signature.
    pub declaration_origin: Option<Arc<str>>,
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
    /// Concrete declared index signatures (`[k: K]: V`) with their real key/value
    /// shape and OXC declaration-site spans. A REAL index signature lives here so
    /// the reconstruction can re-emit `[k: K]: V` losslessly. Distinct from the
    /// `has_index_signature` open-surface flag below: an entry here is a concrete
    /// signature sourced from an OXC declaration site, NOT a synthesized
    /// open-surface placeholder.
    pub index_signatures: Vec<ProjectedIndexSignature>,
    /// Whether the surface includes an index signature at all. When this is set
    /// but [`Self::index_signatures`] is empty, the surface is GENUINELY OPEN
    /// (e.g. a mapped-type / inferred open surface with no concrete declared
    /// key/value payload) — the reconstruction emits a synthetic-`None`
    /// placeholder for it.
    pub has_index_signature: bool,
}

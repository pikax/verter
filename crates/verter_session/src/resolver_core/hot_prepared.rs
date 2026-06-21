//! Session-owned HANDLE-NATIVE hot prepared-declaration carriers.
//!
//! These are the handle-native analogues of the lower-crate
//! [`verter_semantic::analysis::type_solver::prepared`] `Prepared*` shapes:
//! every type BODY that the lower-crate carriers store as a `TypeExpr` (or a
//! type transitively owning a `TypeExpr`) is replaced here by a
//! [`HotTypeRef`] handle over an interned `SemanticNodeId`, and every scalar
//! fact (`ResolvedRootIdentity`, `TypeDeclKind`, `ValueDeclKind`, member
//! visibility/spans, provenance, cache deps) is carried verbatim.
//!
//! INVARIANT (inviolable): NO field of any carrier in this module owns a
//! transitive lower-crate typed-IR type — no `TypeExpr` (nor any
//! `ObjectExpr` / bare-`TypeParam` / `TypeDeclBody` / `FunctionSignature` /
//! enum-member-value / prepared-decl / prepared-member / prepared-wrapper /
//! prepared-projection / prepared-forward owner of one).
//! Every type body is a [`HotTypeRef`] or scalar metadata. The structural
//! guard `hot_prepared_carriers_own_no_typeexpr`
//! (`tests/cases/handle_capable_consumer_guards.rs`) enforces this by parsing
//! this file with `syn` and rejecting any banned typed-IR identifier as a
//! field-type path segment.
//!
//! The carriers are assembled FROM already-computed handles + scalar metadata
//! via [`HotPreparedTypeDecl::from_parts`] / [`HotPreparedValueDecl::from_parts`]
//! (mirroring the `OwnedEvalProgram::from_parts` Vec→Arc wrapping style). The
//! population wiring that calls these from the prepared-decl producer, the
//! reader migration, and the clone-path deletion land in a later atomic
//! session; today these carriers are constructed only by the discriminating
//! tests in `src/hot_prepared_tests.rs`.

use std::sync::Arc;

use rustc_hash::FxHashMap;

use crate::semantic_query::HotTypeRef;
use verter_semantic::analysis::type_eval::{TypeDeclKind, ValueDeclKind};
use verter_semantic::analysis::type_solver::host::ResolvedRootIdentity;
use verter_semantic::analysis::type_solver::prepared::{
    DeclProvenance, PreparedCacheDeps, PreparedExternalDep,
};

// ---------------------------------------------------------------------------
// Hot generic-parameter declaration
// ---------------------------------------------------------------------------

/// A hot generic-parameter declaration: the constraint/default type bodies are
/// [`HotTypeRef`] handles (the lower-crate `TypeParam` carries
/// `Option<Arc<TypeExpr>>` for each), the name a scalar.
// Scalar carrier fields are read by the reader migration (S4); the producer
// flip populates them.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct HotTypeParamDecl {
    pub name: Arc<str>,
    pub constraint: Option<HotTypeRef>,
    pub default: Option<HotTypeRef>,
}

// ---------------------------------------------------------------------------
// Hot prepared member (type-side)
// ---------------------------------------------------------------------------

/// A hot prepared member: the member value is a [`HotTypeRef`] handle,
/// everything else is scalar metadata carried verbatim from the lower-crate
/// `PreparedMember`.
// Scalar metadata fields are read by the reader migration (S4); the producer
// flip populates them.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct HotPreparedMember {
    pub ty: HotTypeRef,
    pub optional: bool,
    pub readonly: bool,
    pub is_method: bool,
    pub visibility: verter_type_expr::MemberVisibility,
    pub spans: verter_type_expr::MemberSpans,
}

// ---------------------------------------------------------------------------
// Scalar wrapper / projection classification
// ---------------------------------------------------------------------------

/// Scalar mirror of the lower-crate `PreparedWrapperKind` classification.
///
/// The lower-crate wrapper-shape additionally carries payload-bearing
/// sub-enums (its key-filter / key-remap / value-rule arms each carry a
/// `TypeExpr`/opaque payload) that are the deferred wrapper-payload closure
/// (not handle-bearing yet). NONE of those payloads are carried here — this
/// enum captures ONLY the scalar discriminant, so the hot carrier owns no
/// transitive `TypeExpr`.
// Discriminant variants are produced by the producer flip (S4).
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HotWrapperKind {
    /// Not a recognized structural wrapper pattern.
    None,
    /// `{ [K in keyof T]: T[K] }` — collapse to base subject.
    Identity,
    /// Only modifier changes (optional/readonly), no key/value transform.
    PureOverlay,
    /// `Pick`/`Omit`-style literal key filtering.
    KeyFilter,
    /// Template-literal or case transform on keys.
    KeyRemap,
}

/// Scalar mirror of the lower-crate projection-class classification: the
/// `ForwardSubject` arm's forwarded symbolic type-argument payload (a
/// `Vec<TypeExpr>` on the lower-crate forward payload) is the deferred
/// wrapper-payload closure and is NOT carried — only the discriminant.
// Discriminant variants are produced by the producer flip (S4).
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HotProjectionKind {
    DirectMembers,
    Wrapper,
    ForwardSubject,
    Opaque,
}

/// Scalar wrapper/projection classification carried on a hot type carrier.
/// Holds ONLY the discriminants — the `TypeExpr`/`Opaque` payloads of the
/// lower-crate `PreparedWrapperShape` / `PreparedProjectionClass` are the
/// deferred wrapper-payload closure, NOT carried here.
// Scalar discriminant fields are read by the reader migration (S4).
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct HotPreparedClassifierMeta {
    pub wrapper_kind: HotWrapperKind,
    pub projection_kind: HotProjectionKind,
}

// ---------------------------------------------------------------------------
// Hot prepared TYPE declaration
// ---------------------------------------------------------------------------

/// Handle-native hot prepared TYPE declaration — the analogue of the
/// lower-crate `PreparedTypeDecl` with every type body as a [`HotTypeRef`].
///
/// The lower-crate `body: TypeExpr` is SPLIT into two handles:
/// [`semantic_body`](Self::semantic_body) (the semantic body — `MergedDecl`
/// for a merged interface) and [`lookup_body`](Self::lookup_body) (the legacy
/// shallow-lookup / compat body surface, KEPT SEPARATE — it is not always
/// identical to the semantic merge body).
// The scalar identity/provenance/cache/dep fields are read by the reader
// migration (S4); the producer flip populates them.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct HotPreparedTypeDecl {
    pub root_identity: ResolvedRootIdentity,
    pub exported_name: Option<String>,
    pub kind: TypeDeclKind,
    pub type_parameters: Arc<[HotTypeParamDecl]>,
    /// The SEMANTIC body handle (a `MergedDecl` node for a merged interface).
    pub semantic_body: HotTypeRef,
    /// The legacy shallow-lookup / compat body surface handle — kept SEPARATE
    /// from [`semantic_body`](Self::semantic_body) (not always identical to the
    /// semantic merge body).
    pub lookup_body: HotTypeRef,
    /// Ordered merged-interface contributor body handles (source order; empty
    /// for the common non-merged case).
    pub merged_contributors: Arc<[HotTypeRef]>,
    pub member_index: FxHashMap<Arc<str>, HotPreparedMember>,
    pub local_deps: Vec<String>,
    pub external_deps: Vec<PreparedExternalDep>,
    pub name_resolution: FxHashMap<String, ResolvedRootIdentity>,
    pub provenance: DeclProvenance,
    pub cache_deps: PreparedCacheDeps,
    pub classifier: HotPreparedClassifierMeta,
}

// `from_parts` + the hot accessors are called by the discriminating tests
// today and by the reader migration (the producer flip — S4); the lib-only
// build has no production caller yet.
#[allow(dead_code)]
impl HotPreparedTypeDecl {
    /// Assemble a hot prepared type declaration from already-computed handles
    /// and scalar metadata. The slice-shaped inputs (`type_parameters`,
    /// `merged_contributors`) arrive as owned `Vec`s and are wrapped into
    /// `Arc<[_]>` here, mirroring `OwnedEvalProgram::from_parts`.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn from_parts(
        root_identity: ResolvedRootIdentity,
        exported_name: Option<String>,
        kind: TypeDeclKind,
        type_parameters: Vec<HotTypeParamDecl>,
        semantic_body: HotTypeRef,
        lookup_body: HotTypeRef,
        merged_contributors: Vec<HotTypeRef>,
        member_index: FxHashMap<Arc<str>, HotPreparedMember>,
        local_deps: Vec<String>,
        external_deps: Vec<PreparedExternalDep>,
        name_resolution: FxHashMap<String, ResolvedRootIdentity>,
        provenance: DeclProvenance,
        cache_deps: PreparedCacheDeps,
        classifier: HotPreparedClassifierMeta,
    ) -> Self {
        Self {
            root_identity,
            exported_name,
            kind,
            type_parameters: Arc::from(type_parameters),
            semantic_body,
            lookup_body,
            merged_contributors: Arc::from(merged_contributors),
            member_index,
            local_deps,
            external_deps,
            name_resolution,
            provenance,
            cache_deps,
            classifier,
        }
    }

    /// The semantic body handle.
    #[must_use]
    pub(crate) fn semantic_body_handle(&self) -> HotTypeRef {
        self.semantic_body
    }

    /// The legacy shallow-lookup / compat body surface handle.
    #[must_use]
    pub(crate) fn lookup_body_handle(&self) -> HotTypeRef {
        self.lookup_body
    }

    /// The ordered merged-interface contributor body handles.
    #[must_use]
    pub(crate) fn merged_contributors(&self) -> &[HotTypeRef] {
        &self.merged_contributors
    }

    /// The handle of a member by name, or `None` if absent.
    #[must_use]
    pub(crate) fn member_handle(&self, name: &str) -> Option<HotTypeRef> {
        self.member_index.get(name).map(|m| m.ty)
    }

    /// Whether this declaration is a same-name merged interface (has ordered
    /// contributor bodies).
    #[must_use]
    pub(crate) fn is_merged(&self) -> bool {
        !self.merged_contributors.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Hot function parameter / signature
// ---------------------------------------------------------------------------

/// A hot function parameter — a SCALAR mirror of the lower-crate
/// `FunctionParam`. The param `type_annotation` STAYS a display string
/// (`Option<Arc<str>>`), NOT a handle, because the lower-crate field is a
/// display/IDE `Option<String>` (the typed-IR-rule's display-only passthrough),
/// never a `TypeExpr`. So a `HotFunctionParam` owns NO handle at all.
// Scalar param-metadata fields are read by the reader migration (S4).
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct HotFunctionParam {
    pub name: Arc<str>,
    pub type_annotation: Option<Arc<str>>,
    pub is_optional: bool,
    pub has_default: bool,
    pub span: verter_span::Span,
}

/// A hot function signature — the analogue of the lower-crate
/// `FunctionSignature`. ONLY [`return_type`](Self::return_type) and the
/// generic [`type_parameters`](Self::type_parameters) carry handles; the
/// [`parameters`](Self::parameters) stay scalar (the lower-crate
/// `FunctionParam` owns no `TypeExpr`).
// `has_implementation_body` is read by the overload-visibility projection (S4).
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct HotFunctionSignature {
    pub parameters: Vec<HotFunctionParam>,
    pub return_type: Option<HotTypeRef>,
    pub type_parameters: Arc<[HotTypeParamDecl]>,
    pub has_implementation_body: bool,
}

// ---------------------------------------------------------------------------
// Hot enum member value
// ---------------------------------------------------------------------------

/// The hot analogue of the lower-crate `EnumMemberValue`. BOTH the lower-crate
/// arms own a `TypeExpr` (the projected/folded literal type), so BOTH hot arms
/// carry a [`HotTypeRef`] — there is no purely-scalar arm.
// The `Deferred` arm is produced by the producer flip (S4) for unfoldable
// members; the test exercises `Folded`.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
pub(crate) enum HotEnumMemberValue {
    /// Statically folded to a literal — the literal type as a handle.
    Folded(HotTypeRef),
    /// Value deferred — the degraded sound primitive domain as a handle.
    Deferred(HotTypeRef),
}

// ---------------------------------------------------------------------------
// Hot prepared VALUE member
// ---------------------------------------------------------------------------

/// A hot prepared value member (for dotted `typeof` paths): the member value is
/// a [`HotTypeRef`] handle, everything else scalar. Mirrors how the type
/// carrier indexes object members with handle values.
// Scalar metadata fields are read by the reader migration (S4).
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct HotPreparedValueMember {
    pub ty: HotTypeRef,
    pub optional: bool,
    pub readonly: bool,
    pub is_method: bool,
    pub visibility: verter_type_expr::MemberVisibility,
    pub spans: verter_type_expr::MemberSpans,
}

// ---------------------------------------------------------------------------
// Hot prepared VALUE declaration
// ---------------------------------------------------------------------------

/// Handle-native hot prepared VALUE declaration — the analogue of the
/// lower-crate `PreparedValueDecl` with every type body as a [`HotTypeRef`].
///
/// The lower-crate `object_shape: Option<ObjectExpr>` (whose members own
/// `ty: TypeExpr`) is NOT stored as a `HotObjectExpr`; instead its members are
/// indexed in [`object_member_index`](Self::object_member_index) with handle
/// values, mirroring how the type carrier uses `member_index`.
// The scalar identity/kind/provenance/cache/dep fields are read by the reader
// migration (S4); the producer flip populates them.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct HotPreparedValueDecl {
    pub root_identity: ResolvedRootIdentity,
    pub exported_name: Option<String>,
    pub kind: ValueDeclKind,
    pub type_annotation: Option<HotTypeRef>,
    pub signatures: Arc<[HotFunctionSignature]>,
    /// The hot form of the lower-crate `object_shape` members — each member's
    /// value is a [`HotTypeRef`].
    pub object_member_index: FxHashMap<Arc<str>, HotPreparedValueMember>,
    /// The ordered enum member inventory; every member's value is a handle
    /// ([`HotEnumMemberValue`]). `Some` exactly when this value decl is an enum.
    pub enum_members: Option<Vec<(Arc<str>, HotEnumMemberValue)>>,
    pub local_deps: Vec<String>,
    pub name_resolution: FxHashMap<String, ResolvedRootIdentity>,
    pub provenance: DeclProvenance,
    pub cache_deps: PreparedCacheDeps,
}

// `from_parts` + `type_annotation_handle` are called by the discriminating
// tests today and by the reader migration (the producer flip — S4); the
// lib-only build has no production caller yet.
#[allow(dead_code)]
impl HotPreparedValueDecl {
    /// Assemble a hot prepared value declaration from already-computed handles
    /// and scalar metadata. `signatures` arrives as an owned `Vec` and is
    /// wrapped into `Arc<[_]>`, mirroring `OwnedEvalProgram::from_parts`.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn from_parts(
        root_identity: ResolvedRootIdentity,
        exported_name: Option<String>,
        kind: ValueDeclKind,
        type_annotation: Option<HotTypeRef>,
        signatures: Vec<HotFunctionSignature>,
        object_member_index: FxHashMap<Arc<str>, HotPreparedValueMember>,
        enum_members: Option<Vec<(Arc<str>, HotEnumMemberValue)>>,
        local_deps: Vec<String>,
        name_resolution: FxHashMap<String, ResolvedRootIdentity>,
        provenance: DeclProvenance,
        cache_deps: PreparedCacheDeps,
    ) -> Self {
        Self {
            root_identity,
            exported_name,
            kind,
            type_annotation,
            signatures: Arc::from(signatures),
            object_member_index,
            enum_members,
            local_deps,
            name_resolution,
            provenance,
            cache_deps,
        }
    }

    /// The value's type-annotation body handle, or `None` when unannotated.
    #[must_use]
    pub(crate) fn type_annotation_handle(&self) -> Option<HotTypeRef> {
        self.type_annotation
    }
}

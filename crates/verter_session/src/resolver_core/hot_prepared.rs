//! Session-owned HANDLE-NATIVE hot prepared-declaration carriers.
//!
//! These are the handle-native analogues of the lower-crate
//! [`verter_semantic::analysis::type_solver::prepared`] `Prepared*` shapes:
//! every type BODY that the lower-crate carriers store as a `TypeExpr` (or a
//! type transitively owning a `TypeExpr`) is replaced here by a
//! [`HotTypeRef`] handle over an interned `SemanticNodeId`, and every scalar
//! fact (`ResolvedRootIdentity`, `TypeDeclKind`, `ValueDeclKind`, member
//! visibility/spans/declaration-origin, provenance, cache deps, wrapper /
//! projection classification discriminants + modifiers) is carried verbatim.
//!
//! INVARIANT (inviolable): NO field of any carrier in this module owns a
//! transitive lower-crate typed-IR type — no `TypeExpr` (nor any
//! `ObjectExpr` / `ObjectMember` / `ObjectProperty` / `MethodSignature` /
//! `IndexSignature` / `FunctionExpr` / function-parameter / bare-`TypeParam` /
//! `TypeDeclBody` / `FunctionSignature` / enum-member-value / prepared-decl /
//! prepared-member / prepared-wrapper-shape / prepared-projection /
//! prepared-forward-payload owner of one).
//! Every type body is a [`HotTypeRef`] or scalar metadata. The invariant is
//! enforced STRUCTURALLY — not by an enumerated denylist of banned owners — by
//! two rails:
//! (a) the ALLOWLIST guard `hot_prepared_carriers_own_no_typeexpr`
//! (`tests/cases/handle_capable_consumer_guards.rs`) parses this file with
//! `syn`, walks every carrier field's type tree, and asserts each leaf bottoms
//! out in an ALLOWED constructor — a [`HotTypeRef`] handle, an allowed
//! container (`Option`/`Vec`/`Arc`/`Box`/map/tuple/slice) thereof, a nested
//! `Hot*` carrier, or an explicitly-allowlisted TypeExpr-free scalar. Any
//! UNRECOGNIZED type (a future `TypeExpr`-owner, a `use … as` alias) REDS by
//! construction — nothing unrecognized passes; and
//! (b) the compiler `assert_not_impl_any!` next to [`HotTypeRef`] keeps the
//! handle non-keyable (R6: no `Hash`/`Ord`/`PartialOrd`).
//! The fully compiler-enforced `NoTypeExpr` marker trait is the planned durable
//! end-state for this invariant; it is not in place yet — the allowlist plus
//! the compiler assert are the sound interim.
//!
//! These carriers are a FAITHFUL handle-native mirror of the lower-crate
//! shapes: every scalar field present on the lower-crate `Prepared*` shape is
//! present here, and every type-body position is a handle — no field is
//! dropped (the param type, the object node, the member declaration origin,
//! and the full wrapper/projection classification each round-trip), and no
//! field is invented that the lower shape does not own.
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
    DeclProvenance, PreparedCacheDeps, PreparedCaseTransformKind, PreparedExternalDep,
    PreparedForwardingKind, PreparedSurfaceModifiers, PreparedWrapperKind,
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
/// `PreparedMember` — including `declaration_origin` (the member's defining
/// file, which drives the macro-surface own-member overlay's span/JSDoc
/// pairing). The lower-crate field is a `String`; the hot-carrier-appropriate
/// shared form is `Arc<str>`.
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
    /// The member's defining file — the `root_identity.canonical_id` of the
    /// owning declaration, stamped at member-index build time. The macro-surface
    /// overlay pairs the member's `spans` with this file. Mirrors the
    /// lower-crate `PreparedMember::declaration_origin` (`String` there;
    /// `Arc<str>` here).
    pub declaration_origin: Arc<str>,
}

// ---------------------------------------------------------------------------
// Hot wrapper / projection classification (full handle-native B2 closure)
// ---------------------------------------------------------------------------
//
// The lower-crate `PreparedWrapperShape` / `PreparedProjectionClass` carry
// payload-bearing sub-enums whose `Opaque`/`Transform`/`target_args` arms own a
// `TypeExpr`. The hot mirror below carries the FULL classifier handle-native:
// every scalar arm is reused verbatim from the lower crate (those enums own no
// `TypeExpr`), and every typed payload arm becomes a [`HotTypeRef`] handle (or a
// handle slice). No discriminant is dropped — this closes the B2 wrapper-payload
// deferral handle-native.

/// How a hot wrapper filters its source keyspace — the handle-native mirror of
/// the lower-crate prepared key-filter shape. The literal arms keep their
/// (interned) key names; the `Opaque` arm's symbolic key type becomes a handle.
// Variants are produced by the producer flip (S4).
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) enum HotKeyFilterShape {
    All,
    IncludeLiteral(Vec<Arc<str>>),
    ExcludeLiteral(Vec<Arc<str>>),
    /// An undecidable key domain — the source key type as a handle.
    Opaque(HotTypeRef),
}

/// How a hot wrapper remaps key names — the handle-native mirror of the
/// lower-crate prepared key-remap shape. The scalar arms keep their data; the
/// `CaseTransform` kind is reused verbatim (it owns no `TypeExpr`); the
/// `Opaque` arm's symbolic name-type becomes a handle.
// Variants are produced by the producer flip (S4).
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) enum HotKeyRemapShape {
    Identity,
    Prefix(Arc<str>),
    Suffix(Arc<str>),
    /// The lower scalar case-transform kind is REUSED verbatim — it owns no
    /// `TypeExpr`.
    CaseTransform(PreparedCaseTransformKind),
    /// A non-literal name-type remap — the name-type as a handle.
    Opaque(HotTypeRef),
}

/// How a hot wrapper transforms member values — the handle-native mirror of the
/// lower-crate prepared value-rule shape. The `Transform` arm's symbolic
/// transform body becomes a handle.
// Variants are produced by the producer flip (S4).
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) enum HotValueRuleShape {
    /// Value is `T[K]` — pass through unchanged.
    PassThrough,
    /// Value involves a transform over `T[K]` — the transform body as a handle.
    Transform(HotTypeRef),
}

/// Structural wrapper classification carried on a hot type carrier — the FULL
/// handle-native mirror of the lower-crate `PreparedWrapperShape`. Scalar
/// fields (`kind`, `source_param_index`, `modifiers`) are reused verbatim (they
/// own no `TypeExpr`); the key-filter / key-remap / value-rule sub-shapes carry
/// their typed payloads as handles.
// Scalar + sub-shape fields are read by the reader migration (S4).
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct HotPreparedWrapperShape {
    /// The lower scalar wrapper-kind discriminant — REUSED verbatim (owns no
    /// `TypeExpr`).
    pub kind: PreparedWrapperKind,
    pub source_param_index: Option<u16>,
    pub key_filter: HotKeyFilterShape,
    pub key_remap: HotKeyRemapShape,
    pub value_rule: HotValueRuleShape,
    /// The lower scalar surface modifiers — REUSED verbatim (owns no
    /// `TypeExpr`).
    pub modifiers: PreparedSurfaceModifiers,
}

/// Structured forwarding payload for [`HotProjectionClass::ForwardSubject`] —
/// the handle-native mirror of the lower-crate prepared forward payload. The
/// `target_args` symbolic type arguments (a `Vec<TypeExpr>` on the lower
/// payload) become a handle slice; `forwarding_kind` is reused verbatim.
// Fields are read by the reader migration (S4).
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct HotPreparedForwardPayload {
    pub target_name: Arc<str>,
    /// The forwarded symbolic type arguments, each as a handle (B2:
    /// `Vec<TypeExpr>` → handle slice).
    pub target_args: Arc<[HotTypeRef]>,
    /// The lower scalar forwarding kind — REUSED verbatim (owns no `TypeExpr`).
    pub forwarding_kind: PreparedForwardingKind,
}

/// Projection-class classification carried on a hot type carrier — the FULL
/// handle-native mirror of the lower-crate `PreparedProjectionClass`. The
/// `ForwardSubject` arm carries the handle-native forward payload; the other
/// arms are pure discriminants.
// Variants are produced by the producer flip (S4).
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) enum HotProjectionClass {
    DirectMembers,
    Wrapper,
    ForwardSubject(HotPreparedForwardPayload),
    Opaque,
}

/// The full wrapper + projection classification carried on a hot type carrier.
/// Holds the COMPLETE handle-native classifier — the lower-crate
/// `PreparedWrapperShape` + `PreparedProjectionClass`, with every typed payload
/// carried as a handle (the B2 closure) and every scalar discriminant/modifier
/// reused verbatim.
// Fields are read by the reader migration (S4).
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct HotPreparedClassifierMeta {
    pub wrapper_shape: HotPreparedWrapperShape,
    pub projection_class: HotProjectionClass,
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

/// A hot function parameter — a FAITHFUL handle-native mirror of the lower-crate
/// `verter_type_expr::FunctionParam` (`name: Option<String>`, `ty: TypeExpr`,
/// `optional: bool`, `rest: bool`, `span: Option<Span>`, `has_ts_annotation:
/// bool`). The param TYPE is the load-bearing field: it is a real `TypeExpr` on
/// the lower carrier, so it is carried here as a [`HotTypeRef`] handle — NOT a
/// display string. `has_ts_annotation` is the OXC structural provenance the
/// JSDoc-`@param` backfill reader consults (it owns no type identity but is a
/// real scalar fact).
// Scalar param-metadata fields are read by the reader migration (S4).
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct HotFunctionParam {
    pub name: Option<Arc<str>>,
    /// The parameter type — a handle (the lower-crate `FunctionParam::ty` is a
    /// real `TypeExpr`, so dropping it to a display string was a storage hole).
    pub ty: HotTypeRef,
    pub optional: bool,
    pub rest: bool,
    pub span: Option<verter_span::Span>,
    /// Whether the parameter carried an explicit TS annotation at its
    /// declaration site — the OXC structural fact the JSDoc backfill reader
    /// consults. In-memory provenance, not part of type identity.
    pub has_ts_annotation: bool,
}

/// A hot function signature — the analogue of the lower-crate
/// `FunctionSignature`. The [`return_type`](Self::return_type), the generic
/// [`type_parameters`](Self::type_parameters), AND every
/// [`parameter`](Self::parameters) type carry handles (the lower-crate
/// `FunctionParam::ty` is a real `TypeExpr`).
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

/// A hot prepared value member (for dotted `typeof` paths) — a FAITHFUL
/// handle-native mirror of the lower-crate `PreparedValueMember`, which is
/// SMALL: it has ONLY `ty: TypeExpr` + `is_method: bool` (NO
/// optional/readonly/visibility/spans/declaration_origin). The value type is a
/// [`HotTypeRef`] handle; `is_method` is a scalar. Do NOT add member metadata
/// the lower value member does not own.
// Scalar metadata fields are read by the reader migration (S4).
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct HotPreparedValueMember {
    pub ty: HotTypeRef,
    pub is_method: bool,
}

// ---------------------------------------------------------------------------
// Hot prepared VALUE declaration
// ---------------------------------------------------------------------------

/// Handle-native hot prepared VALUE declaration — a FAITHFUL analogue of the
/// lower-crate `PreparedValueDecl` with every type body as a [`HotTypeRef`].
///
/// The lower-crate `object_shape: Option<ObjectExpr>` (whose members own
/// `ty: TypeExpr`, AND which also carries ordered index/call/construct
/// signatures) is carried as ONE [`HotTypeRef`] handle over the whole object
/// node (`object_shape`) — the structural lowerer lowers `TypeExpr::Object` to a
/// single `Object` node, so the handle preserves member ordering and the
/// index/call/construct signatures that a name-keyed map would drop. The
/// separate [`member_index`](Self::member_index) (handle-valued) is the
/// dotted-path fast-path lookup index, mirroring the lower-crate `member_index`.
// The scalar identity/kind/cache/dep fields are read by the reader migration
// (S4); the producer flip populates them.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct HotPreparedValueDecl {
    pub root_identity: ResolvedRootIdentity,
    pub exported_name: Option<String>,
    pub kind: ValueDeclKind,
    pub type_annotation: Option<HotTypeRef>,
    pub signatures: Arc<[HotFunctionSignature]>,
    /// The whole object node as ONE handle (the lower-crate
    /// `object_shape: Option<ObjectExpr>`) — preserves member ordering and the
    /// index/call/construct signatures a name-keyed map would drop.
    pub object_shape: Option<HotTypeRef>,
    /// The lower-crate `member_index` (handle-valued) — the dotted-path fast
    /// lookup index, distinct from [`object_shape`](Self::object_shape).
    pub member_index: FxHashMap<Arc<str>, HotPreparedValueMember>,
    /// The ordered enum member inventory; every member's value is a handle
    /// ([`HotEnumMemberValue`]). `Some` exactly when this value decl is an enum.
    pub enum_members: Option<Vec<(Arc<str>, HotEnumMemberValue)>>,
    pub external_deps: Vec<PreparedExternalDep>,
    pub name_resolution: FxHashMap<String, ResolvedRootIdentity>,
    pub cache_deps: PreparedCacheDeps,
}

// `from_parts` + the hot accessors are called by the discriminating tests
// today and by the reader migration (the producer flip — S4); the lib-only
// build has no production caller yet.
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
        object_shape: Option<HotTypeRef>,
        member_index: FxHashMap<Arc<str>, HotPreparedValueMember>,
        enum_members: Option<Vec<(Arc<str>, HotEnumMemberValue)>>,
        external_deps: Vec<PreparedExternalDep>,
        name_resolution: FxHashMap<String, ResolvedRootIdentity>,
        cache_deps: PreparedCacheDeps,
    ) -> Self {
        Self {
            root_identity,
            exported_name,
            kind,
            type_annotation,
            signatures: Arc::from(signatures),
            object_shape,
            member_index,
            enum_members,
            external_deps,
            name_resolution,
            cache_deps,
        }
    }

    /// The value's type-annotation body handle, or `None` when unannotated.
    #[must_use]
    pub(crate) fn type_annotation_handle(&self) -> Option<HotTypeRef> {
        self.type_annotation
    }

    /// The whole-object-shape body handle, or `None` when the value is not an
    /// object/namespace.
    #[must_use]
    pub(crate) fn object_shape_handle(&self) -> Option<HotTypeRef> {
        self.object_shape
    }
}

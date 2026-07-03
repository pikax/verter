//! Closed semantic fact families — the graph-free, content-free replacement for
//! query-time `TypeExpr` walking.
//!
//! Every family is a CLOSED FINITE enum/struct. There is NO `TypeExpr` /
//! `Box<Self>` open recursive arm / open body arm: unsupported structure is a
//! LOCATOR ([`crate::locators`]) — the single graph-engine-routed escape. Every
//! fact type derives `Eq + Hash + NoTypeExpr + NoStoredSpan` and stores NONE of:
//! `Span` / `MemberSpans` / `FunctionSpans` / `IndexSignatureSpans` / `TypeExpr`
//! / `SemanticNodeId` / `HotTypeRef`. Span information that participates in node
//! identity is carried as a producer-emitted ORIGIN LOCATOR
//! ([`crate::span_origins`]), recovered before identity — never stored as a
//! `Span` field.
//!
//! Adding an arm to any family is a reviewed schema event.

use std::sync::Arc;
use verter_no_storedspan::NoStoredSpan;
use verter_no_typeexpr::NoTypeExpr;

use crate::locators::{MacroPayloadLocator, SymbolBodyLocator, TypeArgLocator, TypeBodySlot};
use crate::span_origins::{
    FunctionParamSpanOrigin, FunctionSpansOrigin, IndexSignatureSpansOrigin, MemberSpansOrigin,
};
use crate::{MemberVisibility, PrimitiveName, TypeExprScope};

// ===========================================================================
// Supporting typed replacements introduced with the fact substrate
// ===========================================================================

/// Typed replacement for the untyped `declaration_origin: Option<Arc<str>>` /
/// `String` member field: the canonical file id the member's declaration lives
/// in, or an explicit synthetic/multi-origin marker.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub enum DeclarationOrigin {
    /// The member's declaration lives in this canonical file.
    Declared(Arc<str>),
    /// Genuinely synthetic or multi-origin — no single declaring file (the
    /// `None`/empty case in the untyped `declaration_origin` carriers).
    Synthetic,
}

/// The precomputed graph-free target of a `typeof X[.y.z]` value peel — the
/// value-space declaration identity, so the `TypeExpr::TypeOf` walk is replaced
/// by a stored identity. Content-free (canonical + symbol + member path).
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub struct ValueDeclIdentityPart {
    /// Canonical id of the file declaring the value symbol.
    pub canonical_id: Arc<str>,
    /// The value symbol name (`typeof x` → `x`).
    pub symbol: Arc<str>,
    /// Member path for `typeof x.y.z` (empty = the bare value symbol).
    pub member_path: Arc<[String]>,
}

// ===========================================================================
// Surface A facts — authored-shape closed facts (heritage / closedness /
// key-domain), consumed at dispatch time in place of query-time TypeExpr walking.
// ===========================================================================

/// One authored heritage base (an `extends` / `implements` clause). Carries ONLY
/// authored data — the resolved target `(canonical, symbol)` is computed at
/// dispatch time, NEVER stored (a stored resolved identity is a stale-identity
/// R21 hazard).
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub struct HeritageBaseFact {
    /// The base name exactly as written.
    pub name: String,
    /// Authored type arguments as locators (never embedded `TypeExpr`).
    pub type_args: Arc<[TypeArgLocator]>,
    /// The local `name_resolution` map key that routes this base's target at
    /// dispatch time (usually the leading segment of `name`).
    pub name_resolution_ref: String,
    /// Origin locator recovering the base-name span.
    pub base_name_origin: MemberSpansOrigin,
}

/// The role a followed body plays in a closedness decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub enum ClosednessFollowRole {
    /// The subject type whose closedness is being decided.
    Subject,
    /// A key-domain source (e.g. the `Src` of `Pick<Src, K>`).
    KeyDomainSource,
    /// A value / name-type source.
    ValueSource,
}

/// A symbolic substitution binding: a type-parameter name paired with the
/// locator of its authored argument. The live binding environment is RE-MINTED
/// at dispatch time from these — never a stored `SemanticNodeId` / `TypeExpr` /
/// live `KeyDomainBindings`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub struct SymbolicBinding {
    /// The bound type-parameter name.
    pub param_name: String,
    /// Locator of the authored argument for that parameter.
    pub argument: TypeArgLocator,
}

/// An ordered symbolic binding/substitution environment locator.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub struct SymbolicBindingLocator {
    /// Ordered bindings (type-param name → authored-argument locator).
    pub bindings: Arc<[SymbolicBinding]>,
}

/// The general closedness escape: follow an authored body under a role and a
/// symbolic binding environment. MUST NOT store a live `KeyDomainBindings`, a
/// borrowed `TypeExpr`, or a `SemanticNodeId` — only the symbolic locator.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub struct FollowLocatorPayload {
    /// The authored body to follow.
    pub locator: crate::locators::AuthoredBodyLocator,
    /// The role that body plays in the closedness decision.
    pub role: ClosednessFollowRole,
    /// The symbolic binding/substitution environment (re-minted at dispatch).
    pub binding: SymbolicBindingLocator,
}

/// A prep-time closedness recipe capturing only cheap decidable-from-syntax
/// shapes. The general/undecided escape is [`ClosednessRecipe::FollowLocator`];
/// a bare reference escapes via [`ClosednessRecipe::FollowSlot`].
///
/// Parentheses are normalized away at production time (they carry no semantic
/// content), so there is NO `Parenthesized` arm; the ONLY composition arm is
/// [`ClosednessRecipe::IntersectionAllArms`].
///
/// The marker witnesses are DERIVED with the opt-in recursive-self escape
/// (`#[no_typeexpr(recursive_self)]` / `#[no_storedspan(recursive_self)]`): for
/// the fixed-point `IntersectionAllArms(Arc<[ClosednessRecipe]>)` arm the derive
/// emits a compiler-resolved `RecursiveSelfArc<Self>` PROOF-BOUND instead of the
/// plain witness bound (which would otherwise ask the trait solver to prove
/// `Arc<[Self]>: Marker` while proving `Self: Marker`, an overflow — E0275),
/// while still emitting the per-field witness bound on EVERY non-recursive arm
/// payload. Only the genuine `std::sync::Arc<[ClosednessRecipe]>` satisfies the
/// proof-bound, so a bare/shadowed/custom `Arc` cannot masquerade as the approved
/// self-container; and the future-arm gap stays closed: a NEW non-recursive arm
/// carrying a `TypeExpr` / `Span` would fail the derive (a compile-fail fixture
/// proves this).
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
#[no_typeexpr(recursive_self)]
#[no_storedspan(recursive_self)]
pub enum ClosednessRecipe {
    /// A closed-named-members object (`{ a; b }`) ⇒ closed.
    ObjectClosed,
    /// An intersection is closed iff every arm's recipe is closed. The ONLY
    /// composition arm (parentheses are normalized away). The recursive-self
    /// escape omits the self-bound on this arm only.
    IntersectionAllArms(Arc<[ClosednessRecipe]>),
    /// A mapped type with an open key parameter ⇒ open.
    MappedOpenParam,
    /// A bare reference — follow the referenced slot to decide.
    FollowSlot(SymbolBodyLocator),
    /// The general escape for any complex/undecided subject.
    FollowLocator(FollowLocatorPayload),
}

/// The owned prep fact for a key domain — recipe-only arms. The live borrowed
/// `KeyDomainBinding` arms (`ClosedExpr` / `ClosedNode`) are NEVER stored; they
/// are re-minted during dispatch evaluation.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub enum KeyDomainFact {
    /// The key domain is open.
    Open,
    /// The key domain is closed but abstract (no concrete enumeration).
    ClosedAbstract,
    /// Follow the referenced slot to decide the key domain.
    FollowSlot(SymbolBodyLocator),
    /// The general escape — follow an authored body under a symbolic binding.
    FollowLocator(FollowLocatorPayload),
}

// ===========================================================================
// Surface B facts — graph-free frontier / shallow / eval-env locators + finite
// facts (the graph-free boundary that precedes the graph, never a HotTypeRef).
// ===========================================================================

/// A CLOSED frontier body — ONLY unresolved-symbolic arms plus the locator
/// escape. Deliberately NO object-members arm and NO general body arm.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub enum NarrowFrontierBody {
    /// A re-export route (specifier + exported name), no resolved body.
    ExportRoute {
        specifier: String,
        exported_name: String,
    },
    /// An unresolved external ref (name only).
    UnresolvedExternalRef { name: String },
    /// An unbound type-parameter shell.
    TypeParamShell { name: String },
    /// The escape: a resolvable body addressed by locator.
    Resolvable(SymbolBodyLocator),
}

/// The object-member-names route: a closed enumeration, or the open/undecidable
/// carrier-stop class (`OpenKeyDomain`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub enum MemberNamesRoute {
    /// A closed enumeration of object member names.
    Closed(Arc<[String]>),
    /// Open / undecidable key domain — the L1 carrier-stop class.
    OpenKeyDomain,
}

/// One per-member dependency edge (a NAME/REF enumeration, not a type-shape
/// evaluation).
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub struct MemberDependencyEdge {
    /// The member whose dependencies these are.
    pub member: String,
    /// The names this member depends on.
    pub depends_on: Arc<[String]>,
}

/// The shallow route closures narrowed to closed NAME/REF facts.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub struct ShallowRouteFacts {
    /// Object-member-names route (or the open carrier-stop marker).
    pub member_names: MemberNamesRoute,
    /// Member-path seed reference names.
    pub member_path_seeds: Arc<[String]>,
    /// Per-member dependency edges.
    pub member_dependency_edges: Arc<[MemberDependencyEdge]>,
    /// Whole-route ref-closure (the transitive ref names).
    pub whole_route_refs: Arc<[String]>,
}

/// Classification of a value annotation body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub enum ValueAnnotationClass {
    /// A `typeof X` peel target (paired with `typeof_alias_target`).
    TypeOfAlias,
    /// A direct annotation body (reached via the locator).
    Direct,
    /// No annotation.
    Absent,
}

/// The `PreparedValueDecl.type_annotation` narrowing: a precomputed
/// `typeof_alias_target` (replacing the `TypeExpr::TypeOf` peel) plus a
/// classification and, when present, the annotation body locator.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub struct ValueTypeAnnotationFact {
    /// The precomputed graph-free `typeof x[.y]` target, when the annotation is a
    /// value peel.
    pub typeof_alias_target: Option<ValueDeclIdentityPart>,
    /// The annotation classification.
    pub classification: ValueAnnotationClass,
    /// The annotation body locator (absent for [`ValueAnnotationClass::Absent`]).
    pub annotation: Option<TypeBodySlot>,
}

/// One narrowed type parameter: its name, ordinal, and constraint/default
/// locators (never embedded `TypeExpr`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub struct NarrowTypeParam {
    /// The type-parameter name.
    pub name: String,
    /// Its declaration ordinal within the owning type-parameter list.
    pub ordinal: u32,
    /// Constraint body locator (`T extends C`), if any.
    pub constraint: Option<TypeBodySlot>,
    /// Default body locator (`T = D`), if any.
    pub default: Option<TypeBodySlot>,
}

/// A whole type-parameter declaration list.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub struct TypeParamDeclFact {
    /// The ordered type parameters.
    pub params: Arc<[NarrowTypeParam]>,
}

// ===========================================================================
// Surface C facts — lower-crate Prepared* / Analyzed* / Projected* facts +
// locators, held in place inside verter_semantic (never a HotTypeRef).
// ===========================================================================

/// Structural classification of a prepared type-decl body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub enum TypeBodyClass {
    /// `type X = ...`.
    Alias,
    /// `interface X { ... }`.
    Interface,
    /// `class X { ... }`.
    Class,
    /// Multiple same-name `interface X` declarations (merged).
    MergedInterface,
}

/// The `PreparedTypeDecl.body: TypeExpr` narrowing — a classification plus the
/// body slot locator and the ordered merged-contributor slots.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub struct PreparedTypeBodyFacts {
    /// The body classification.
    pub classification: TypeBodyClass,
    /// The body slot locator.
    pub body_slot: TypeBodySlot,
    /// Ordered merged-declaration contributor slots (empty for non-merged).
    pub merged_contributor_slots: Arc<[TypeBodySlot]>,
}

/// A narrowed function parameter fact. `FunctionParam.span` participates in the
/// hand-written identity, so a `FunctionParamSpanOrigin` recovers it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub struct FunctionParamFact {
    /// The parameter name, if any.
    pub name: Option<String>,
    /// Whether the parameter is optional.
    pub optional: bool,
    /// Whether the parameter is a rest parameter.
    pub rest: bool,
    /// Whether an explicit TS annotation was authored (identity-relevant fact).
    pub has_ts_annotation: bool,
    /// The parameter type body locator.
    pub ty: TypeBodySlot,
    /// Origin locator recovering `FunctionParam.span`.
    pub span_origin: FunctionParamSpanOrigin,
}

/// A narrowed function signature (an overload-group member). `FunctionExpr`
/// carries `FunctionSpans` in identity, recovered via `spans_origin`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub struct FunctionSignatureFact {
    /// The function's own type parameters.
    pub type_parameters: Arc<[NarrowTypeParam]>,
    /// Ordered parameter facts.
    pub parameters: Arc<[FunctionParamFact]>,
    /// The return type body locator (absent = inferred / void).
    pub return_ty: Option<TypeBodySlot>,
    /// Overload-visibility fact: hide the trailing implementation signature.
    pub has_implementation_body: bool,
    /// Origin locator recovering `FunctionSpans`.
    pub spans_origin: FunctionSpansOrigin,
}

/// A narrowed object property member. Carries the identity-participating
/// `visibility` + `optional` + `readonly`; the member span is recovered via
/// `span_origin`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub struct ObjectPropertyFact {
    /// The member name.
    pub name: String,
    /// Whether the member is optional.
    pub optional: bool,
    /// Whether the member is readonly.
    pub readonly: bool,
    /// Member visibility (identity-participating, publication-filtered).
    pub visibility: MemberVisibility,
    /// The member value type body locator.
    pub ty: TypeBodySlot,
    /// Origin locator recovering the member's `MemberSpans`.
    pub span_origin: MemberSpansOrigin,
}

/// A narrowed object method member.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub struct ObjectMethodFact {
    /// The member name.
    pub name: String,
    /// Whether the method is optional.
    pub optional: bool,
    /// Member visibility (identity-participating, publication-filtered).
    pub visibility: MemberVisibility,
    /// The method's function signature.
    pub function: FunctionSignatureFact,
    /// Origin locator recovering the member's `MemberSpans`.
    pub span_origin: MemberSpansOrigin,
}

/// The declared SHAPE of an index-signature key (so `[k: string] ≠ [k: number]`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub enum KeyTypeShape {
    /// `[k: string]`.
    String,
    /// `[k: number]`.
    Number,
    /// `[k: symbol]`.
    Symbol,
    /// A non-primitive / complex key type addressed by locator (fact-or-locator).
    Other(TypeBodySlot),
}

/// A narrowed index signature. `IndexSignature` carries `IndexSignatureSpans` in
/// identity, recovered via `span_origin`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub struct IndexSignatureFact {
    /// The index key parameter name (`k`).
    pub key_name: String,
    /// The declared key SHAPE.
    pub key_type: KeyTypeShape,
    /// The value type body locator.
    pub value_type: TypeBodySlot,
    /// Whether the index signature is readonly.
    pub readonly: bool,
    /// Origin locator recovering the `IndexSignatureSpans`.
    pub span_origin: IndexSignatureSpansOrigin,
}

/// One narrowed object member over all five `ObjectMember` variants.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub enum ObjectMemberFact {
    /// A property member.
    Property(ObjectPropertyFact),
    /// A method member.
    Method(ObjectMethodFact),
    /// A call signature.
    CallSignature(FunctionSignatureFact),
    /// A construct signature.
    ConstructSignature(FunctionSignatureFact),
    /// An index signature.
    IndexSignature(IndexSignatureFact),
}

/// The `PreparedValueDecl.object_shape: Option<ObjectExpr>` narrowing — closed
/// over all five member variants.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub struct ObjectShapeFact {
    /// The ordered object members.
    pub members: Arc<[ObjectMemberFact]>,
}

/// A folded/sound enum member scalar. Numeric values are stored as their exact
/// source string (never `f64`, which would break `Eq`/`Hash` identity).
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub enum EnumScalar {
    /// A folded numeric literal (exact source repr).
    Number(String),
    /// A folded string literal.
    String(String),
    /// A sound primitive domain (an unfolded computed member).
    Primitive(EnumPrimitiveDomain),
}

/// The sound primitive domain of an unfolded computed enum member.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub enum EnumPrimitiveDomain {
    /// A numeric enum member domain.
    Number,
    /// A string enum member domain.
    String,
}

/// One ordered enum member (name → closed scalar).
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub struct EnumMemberEntry {
    /// The member name.
    pub name: String,
    /// The member's folded/sound value.
    pub value: EnumScalar,
}

/// The `PreparedValueDecl.enum_members` narrowing — the ordered inventory.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub struct EnumMemberFact {
    /// The ordered enum members.
    pub members: Arc<[EnumMemberEntry]>,
}

/// The `PreparedMember.ty` narrowing. `PreparedMember.spans` is recovered via
/// `span_origin`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub struct PreparedMemberFact {
    /// Whether the member is optional.
    pub optional: bool,
    /// Whether the member is readonly.
    pub readonly: bool,
    /// Whether the member is a method.
    pub is_method: bool,
    /// Member visibility.
    pub visibility: MemberVisibility,
    /// Typed declaration origin (defining file / synthetic).
    pub declaration_origin: DeclarationOrigin,
    /// The member type body locator.
    pub ty: TypeBodySlot,
    /// Origin locator recovering the member's `MemberSpans`.
    pub span_origin: MemberSpansOrigin,
}

/// The `PreparedValueMember.ty` narrowing (+ `is_method`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub struct PreparedValueMemberFact {
    /// Whether the value member is a method.
    pub is_method: bool,
    /// The member type body locator.
    pub ty: TypeBodySlot,
}

/// Case-transform kind for a key remap (lower-neutral copy of the prep kind).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub enum PreparedCaseTransformKind {
    /// `Capitalize<K>`.
    Capitalize,
    /// `Uncapitalize<K>`.
    Uncapitalize,
    /// `Uppercase<K>`.
    Uppercase,
    /// `Lowercase<K>`.
    Lowercase,
}

/// The narrowed `PreparedKeyFilterShape` — `Opaque(TypeExpr)` becomes a locator.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub enum PreparedKeyFilterShapeFact {
    /// No key filter.
    All,
    /// Include exactly these literal keys.
    IncludeLiteral(Arc<[String]>),
    /// Exclude exactly these literal keys.
    ExcludeLiteral(Arc<[String]>),
    /// A non-literal key filter addressed by locator.
    Opaque(TypeBodySlot),
}

/// The narrowed `PreparedKeyRemapShape` — `Opaque(TypeExpr)` becomes a locator.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub enum PreparedKeyRemapShapeFact {
    /// Identity remap.
    Identity,
    /// Prefix each key.
    Prefix(String),
    /// Suffix each key.
    Suffix(String),
    /// Case-transform each key.
    CaseTransform(PreparedCaseTransformKind),
    /// A non-literal key remap addressed by locator.
    Opaque(TypeBodySlot),
}

/// The narrowed `PreparedValueRuleShape` — `Transform(TypeExpr)` becomes a
/// locator.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub enum PreparedValueRuleShapeFact {
    /// The value passes through unchanged.
    PassThrough,
    /// The value is transformed via the located type.
    Transform(TypeBodySlot),
}

/// Forwarding kind for a forward-subject projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub enum PreparedForwardingKind {
    /// Identity type parameters are forwarded unchanged.
    IdentityParams,
    /// An applied alias forwards concrete arguments.
    AppliedAlias,
}

/// The narrowed `PreparedForwardPayload` — `target_args: Vec<TypeExpr>` becomes
/// `Arc<[TypeArgLocator]>` (keeping `target_name` + `forwarding_kind`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub struct PreparedForwardPayloadFact {
    /// The forwarded target name.
    pub target_name: String,
    /// The forwarding kind.
    pub forwarding_kind: PreparedForwardingKind,
    /// The forwarded type arguments as locators.
    pub target_args: Arc<[TypeArgLocator]>,
}

/// The structural wrapper classification discriminant (`PreparedWrapperKind`) —
/// a 1:1 lower-neutral copy (no `TypeExpr`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub enum PreparedWrapperKindFact {
    /// Not a wrapper.
    None,
    /// The identity wrapper (`{ [K in keyof T]: T[K] }`).
    Identity,
    /// A pure modifier overlay (`Partial`/`Required`/`Readonly`/`Mutable`).
    PureOverlay,
    /// A key-filtering wrapper (`Pick`/`Omit`).
    KeyFilter,
    /// A key-remapping wrapper (`as`-clause mapped type).
    KeyRemap,
}

/// The narrowed surface modifiers (`PreparedSurfaceModifiers`) — `+`/`-` optional
/// and readonly overlays. A `None` field means "unchanged".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub struct PreparedSurfaceModifiersFact {
    /// Optionality overlay (`Some(true)` add `?`, `Some(false)` remove, `None`
    /// unchanged).
    pub optional: Option<bool>,
    /// Readonly overlay (same tri-state semantics).
    pub readonly: Option<bool>,
}

/// The narrowed `PreparedWrapperShape` — the full structural wrapper
/// classification. Every `TypeExpr`-bearing sub-shape (`key_filter` / `key_remap`
/// / `value_rule`) is already narrowed to its `*Fact` (opaque payloads become
/// locators); no field is left as vague bundle prose.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub struct PreparedWrapperShapeFact {
    /// The wrapper kind discriminant.
    pub kind: PreparedWrapperKindFact,
    /// The mapped/source type-parameter index this wrapper keys off, if any.
    pub source_param_index: Option<u16>,
    /// The key-filter shape (`Pick`/`Omit` domain; opaque → locator).
    pub key_filter: PreparedKeyFilterShapeFact,
    /// The key-remap shape (`as`-clause remap; opaque → locator).
    pub key_remap: PreparedKeyRemapShapeFact,
    /// The value-transform rule (opaque transform → locator).
    pub value_rule: PreparedValueRuleShapeFact,
    /// The optional/readonly modifier overlays.
    pub modifiers: PreparedSurfaceModifiersFact,
}

/// The narrowed `PreparedProjectionClass` — the top-level projection strategy.
/// The `Wrapper` details live on the owning decl's `PreparedWrapperShapeFact`;
/// `ForwardSubject` carries the forward payload fact.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub enum PreparedProjectionClassFact {
    /// The decl projects its own directly-declared members.
    DirectMembers,
    /// The decl is a structural wrapper (details on `PreparedWrapperShapeFact`).
    Wrapper,
    /// The decl forwards to another target (payload carried here).
    ForwardSubject(PreparedForwardPayloadFact),
    /// An opaque projection (no cheap structural classification).
    Opaque,
}

// ===========================================================================
// Analyzed* / Projected* / synthesized facts (the [P2] named-instance surface)
// ===========================================================================

/// The structural macro role (from `AnalyzedMacro.kind` — the type-role
/// authority; role classification is structural, never nominal).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub enum AnalyzedMacroKindFact {
    /// `defineProps`.
    DefineProps,
    /// `defineEmits`.
    DefineEmits,
    /// `defineSlots`.
    DefineSlots,
    /// `defineOptions`.
    DefineOptions,
    /// `defineExpose`.
    DefineExpose,
    /// `defineModel`.
    DefineModel,
    /// `withDefaults`.
    WithDefaults,
}

/// The narrowed `AnalyzedPropField`. The authored `type_expr` becomes a payload
/// locator; the prop-name span is recovered via `name_span_origin`. Display-only
/// fields (`type_annotation`, `description`, `tags`, `resolution_error`) are
/// carve-outs, not stored here.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub struct AnalyzedPropFieldFact {
    /// The prop key.
    pub name: String,
    /// Whether the prop is optional.
    pub is_optional: bool,
    /// Whether the prop was author-declared in the macro type argument (vs
    /// heritage-derived). Policy-consumed at publication (NOT display).
    pub declared_in_macro_type_arg: bool,
    /// The scope-pairing (producing canonical of the narrowed payload body).
    pub type_expr_scope: Option<TypeExprScope>,
    /// The narrowed prop type body locator.
    pub payload: Option<MacroPayloadLocator>,
    /// Origin locator recovering the prop-name span.
    pub name_span_origin: MemberSpansOrigin,
}

/// The narrowed `AnalyzedEmitField`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub struct AnalyzedEmitFieldFact {
    /// The event key.
    pub name: String,
    /// The scope pairing.
    pub type_expr_scope: Option<TypeExprScope>,
    /// The narrowed emit-signature payload locator.
    pub payload: Option<MacroPayloadLocator>,
    /// Origin locator recovering the emit-name span.
    pub name_span_origin: MemberSpansOrigin,
}

/// The narrowed `AnalyzedSlotFieldBinding`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub struct AnalyzedSlotFieldBindingFact {
    /// The binding key.
    pub name: String,
    /// The scope pairing.
    pub type_expr_scope: Option<TypeExprScope>,
    /// The narrowed binding payload locator (typically an indexed access).
    pub payload: Option<MacroPayloadLocator>,
    /// Origin locator recovering the binding-name span.
    pub name_span_origin: MemberSpansOrigin,
}

/// The narrowed `AnalyzedSlotField`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub struct AnalyzedSlotFieldFact {
    /// The slot key.
    pub name: String,
    /// Whether the slot is required (`optional` inverse).
    pub is_required: bool,
    /// The scope pairing.
    pub type_expr_scope: Option<TypeExprScope>,
    /// The narrowed slot return payload locator.
    pub payload: Option<MacroPayloadLocator>,
    /// The slot bindings.
    pub bindings: Arc<[AnalyzedSlotFieldBindingFact]>,
    /// Origin locator recovering the slot-name span.
    pub name_span_origin: MemberSpansOrigin,
}

/// The narrowed `AnalyzedOptionsProp` (Options API).
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub struct AnalyzedOptionsPropFact {
    /// The prop key.
    pub name: String,
    /// Whether the prop is required.
    pub is_required: bool,
    /// Whether the prop has a default.
    pub has_default: bool,
    /// The Vue runtime constructor name (`String`, `Number`, …), if present.
    pub type_constructor: Option<String>,
    /// The scope pairing.
    pub type_expr_scope: Option<TypeExprScope>,
    /// The narrowed `PropType<T>` payload locator.
    pub payload: Option<MacroPayloadLocator>,
    /// Origin locator recovering the prop-name span.
    pub name_span_origin: MemberSpansOrigin,
}

/// The narrowed `AnalyzedExposeField`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub struct AnalyzedExposeFieldFact {
    /// The expose key.
    pub name: String,
    /// The scope pairing.
    pub type_expr_scope: Option<TypeExprScope>,
    /// The narrowed expose payload locator.
    pub payload: Option<MacroPayloadLocator>,
    /// Origin locator recovering the expose-name span (synthetic for type-arg
    /// surface members with no authored object-literal span).
    pub name_span_origin: MemberSpansOrigin,
}

/// The narrowed `AnalyzedMacro` (incl. `parsed_type_argument`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub struct AnalyzedMacroFact {
    /// The structural macro role (the type-role authority).
    pub kind: AnalyzedMacroKindFact,
    /// Whether the macro is type-based.
    pub is_type_based: bool,
    /// Whether the macro declares `inheritAttrs: false`.
    pub has_inherit_attrs_false: bool,
    /// Referenced type names.
    pub type_references: Arc<[String]>,
    /// The `defineModel` name, if any.
    pub model_name: Option<String>,
    /// The narrowed `parsed_type_argument` parent-shell payload locator.
    pub parsed_type_argument: Option<MacroPayloadLocator>,
    /// The scope pairing for `parsed_type_argument`.
    pub parsed_type_argument_scope: Option<TypeExprScope>,
}

/// The `ty` of a synthesized member: a closed scalar/leaf fact OR the locator
/// escape. There is NO open recursive `Box<Self>` arm — any non-leaf structure
/// is a locator.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub enum FactOrLocator {
    /// A closed scalar/leaf fact.
    Leaf(LeafTypeFact),
    /// The escape: any non-leaf structure addressed by a body locator.
    Locator(TypeBodySlot),
}

/// A closed leaf type fact (a primitive, a literal, or a bare named reference).
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub enum LeafTypeFact {
    /// A primitive type.
    Primitive(PrimitiveName),
    /// A string literal.
    StringLiteral(String),
    /// A numeric literal (exact source repr).
    NumberLiteral(String),
    /// A boolean literal.
    BooleanLiteral(bool),
    /// A bare named reference (shallow — resolved elsewhere on demand).
    Ref(String),
}

/// One synthesized object member (the [P1] synthesized-(d) schema). `readonly =
/// false` and `visibility = Public` are producer-constants (not stored). The
/// member span is recovered via `span_origin`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub struct SynthesizedMemberFact {
    /// The member name.
    pub name: String,
    /// Whether the member is optional.
    pub optional: bool,
    /// The member type (fact-or-locator).
    pub ty: FactOrLocator,
    /// Origin locator recovering `MemberSpans::name_only(field.span)`.
    pub span_origin: MemberSpansOrigin,
}

/// One tuple element fact.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub struct TupleElementFact {
    /// The element label, if a named tuple member.
    pub label: Option<String>,
    /// Whether the element is optional.
    pub optional: bool,
    /// Whether the element is a rest element.
    pub rest: bool,
    /// The element type (fact-or-locator).
    pub ty: FactOrLocator,
}

/// A tuple payload fact.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub struct TuplePayloadFact {
    /// Whether the tuple is readonly.
    pub readonly: bool,
    /// The ordered tuple elements.
    pub elements: Arc<[TupleElementFact]>,
}

/// An indexed-access fact (`Obj['a']['b']`) — path-precise, graph-free.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub struct IndexedAccessFact {
    /// The object body locator.
    pub object: TypeBodySlot,
    /// The ordered index-key path.
    pub index_path: Arc<[String]>,
}

/// The synthesized shape of a `ResolvedLocalType`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub enum ResolvedLocalShape {
    /// A synthesized object surface.
    Object(Arc<[SynthesizedMemberFact]>),
    /// A tuple payload.
    Tuple(TuplePayloadFact),
    /// An indexed access.
    IndexedAccess(IndexedAccessFact),
    /// A single leaf type.
    Leaf(LeafTypeFact),
    /// A bare reference resolved elsewhere (shallow).
    Ref(SymbolBodyLocator),
}

/// The `ResolvedLocalType` → `ResolvedLocalTypeFact` narrowing (synthesized-(d)).
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub struct ResolvedLocalTypeFact {
    /// The type name as referenced.
    pub name: String,
    /// The synthesized shape.
    pub shape: ResolvedLocalShape,
}

/// The narrowed `ProjectedMember`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub struct ProjectedMemberFact {
    /// The member name.
    pub name: String,
    /// Whether the member is optional.
    pub optional: bool,
    /// Whether the member is readonly.
    pub readonly: bool,
    /// Whether the member is a method.
    pub is_method: bool,
    /// Member visibility.
    pub visibility: MemberVisibility,
    /// Whether the member was author-declared in the macro type argument.
    pub declared_in_macro_type_arg: bool,
    /// The typed declaration origin.
    pub declaration_origin: DeclarationOrigin,
    /// The member value type body locator.
    pub ty: TypeBodySlot,
    /// Origin locator recovering the member's `MemberSpans`.
    pub span_origin: MemberSpansOrigin,
}

/// The narrowed `ProjectedIndexSignature`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub struct ProjectedIndexSignatureFact {
    /// The index key parameter name.
    pub key_name: String,
    /// The declared key SHAPE.
    pub key_type: KeyTypeShape,
    /// The value type body locator.
    pub value_type: TypeBodySlot,
    /// Whether the index signature is readonly.
    pub readonly: bool,
    /// The typed declaration origin.
    pub declaration_origin: DeclarationOrigin,
    /// Origin locator recovering the `IndexSignatureSpans`.
    pub span_origin: IndexSignatureSpansOrigin,
}

/// The narrowed `ProjectedSurface`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub struct ProjectedSurfaceFact {
    /// The projected members.
    pub members: Arc<[ProjectedMemberFact]>,
    /// Ordered call signatures.
    pub call_signatures: Arc<[FunctionSignatureFact]>,
    /// Ordered construct signatures.
    pub construct_signatures: Arc<[FunctionSignatureFact]>,
    /// Concrete declared index signatures.
    pub index_signatures: Arc<[ProjectedIndexSignatureFact]>,
    /// Open-surface flag (distinct from concrete `index_signatures`).
    pub has_index_signature: bool,
}

// ===========================================================================
// Svelte facts
// ===========================================================================

/// One Svelte legacy prop (`export let`) fact.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub struct SvelteLegacyPropFact {
    /// The prop name.
    pub name: String,
    /// Whether the prop has a default (optionality).
    pub has_default: bool,
}

/// The narrowed persisted `SvelteScriptFacts`. `props_type` /
/// `dispatcher_events` become body locators.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub struct SvelteScriptFactsFact {
    /// The props type body locator (shallow-by-default; bare `Ref` preserved).
    pub props_type: Option<SymbolBodyLocator>,
    /// The scope pairing for `props_type`.
    pub props_type_scope: Option<TypeExprScope>,
    /// MODEL binding names.
    pub bindable_members: Arc<[String]>,
    /// svelte-package-validated snippet prop names.
    pub validated_snippet_members: Arc<[String]>,
    /// Legacy props.
    pub legacy_props: Arc<[SvelteLegacyPropFact]>,
    /// The `createEventDispatcher<E>()` type-arg body locator, when provenance-
    /// validated.
    pub dispatcher_events: Option<SymbolBodyLocator>,
    /// The scope pairing for `dispatcher_events`.
    pub dispatcher_events_scope: Option<TypeExprScope>,
    /// EXPOSE surface (instance exports).
    pub instance_exports: Arc<[String]>,
}

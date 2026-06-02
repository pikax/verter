//! Internal type expression AST for lightweight type resolution.
//!
//! `TypeExpr` is an internal syntax-preserving representation used by
//! the native evaluator. It is **not** the public output IR — that role
//! belongs to `TypeDescriptor` in `packages/component-meta/src/type-ir.ts`.
//!
//! # Design
//!
//! The AST is populated from OXC's `TSType` nodes during analysis
//! (lowering lives in the sibling `verter_type_expr_oxc` crate so
//! consumers that only need the data tier — NAPI / WASM / JSON
//! readers — can avoid pulling in OXC).
//!
//! The evaluator reduces `TypeExpr` → `TypeDescriptor` through the
//! symbol tables and evaluation environment.
//!
//! Node kinds cover the TypeScript type syntax subset needed for
//! component metadata resolution — not the full TS type system.

use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, LazyLock};
use verter_span::Span;

/// In-place declaration-site span transforms over [`TypeExpr`]
/// ([`TypeExpr::shift_spans`] / [`TypeExpr::clear_spans`]).
mod span_transform;

/// Depth-safe iterative `Drop` + byte-identical-to-derive iterative
/// `Hash` for [`TypeExpr`] (orphan-rule-permitted in this crate-local
/// module; kept out of the crate root for file-size hygiene).
mod recursive_traversal;

/// Hand-rolled JSON (de)serialisation for [`TypeExpr`]: the
/// [`serde::Serialize`]/[`serde::Deserialize`] impls,
/// [`TypeExpr::to_json_value`], and [`type_expr_from_json`]
/// (orphan-rule-permitted in this crate-local module; kept out of the
/// crate root for file-size hygiene).
mod type_expr_json;
pub use type_expr_json::type_expr_from_json;

// ---------------------------------------------------------------------------
// Send + Sync invariant
// ---------------------------------------------------------------------------

const _: fn() = || {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<TypeExpr>();
    assert_send_sync::<TypeExprScope>();
};

// ---------------------------------------------------------------------------
// TypeExprScope — scope sidecar for paired `*_expr` schema fields
// ---------------------------------------------------------------------------

/// Scope sidecar for a paired `TypeExpr`. Carries the canonical_id of
/// the file whose OXC parse produced the typed expression. Consumers
/// walking nested `TypeExpr::Ref` nodes resolve them in the file where
/// the annotation was written — which differs from the SFC owner for
/// cross-file pre-resolved props.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct TypeExprScope(pub String);

impl TypeExprScope {
    pub fn new(canonical_id: impl Into<String>) -> Self {
        Self(canonical_id.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

// ---------------------------------------------------------------------------
// Core AST
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// SyntheticSlotBinding carrier — typed-IR variant minted by
// `publish_merged_bindings` at the no-parser branch
// ---------------------------------------------------------------------------

/// Surface kind for a synthetic carrier minted at slot-binding or
/// `defineSlots` binding publication when no parser-side binding
/// expression is available. Used to distinguish the two surfaces on
/// the typed-IR variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SyntheticCarrierSurfaceKind {
    SlotBinding,
    Binding,
}

/// Intrinsic, shallow-by-construction identity for a synthetic carrier
/// minted by `publish_merged_bindings`. Identity is the FULL
/// (scope_canonical_id, surface_kind, slot_name, binding_name, value_node)
/// tuple — `value_node` discriminates two same-named carriers in
/// different slots of the same component. The carrier is NEVER
/// resolved as a type alias via the type registry; same-name
/// poisoning of a real workspace alias is structurally impossible
/// because it lives on a distinct `TypeExpr` variant.
///
/// `value_node` is stored as `u64` because `verter_type_expr` cannot
/// depend on `verter_session`. FFI / JSON serialise `value_node` as a
/// decimal STRING to avoid JS Number precision loss.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SyntheticCarrierKey {
    pub scope_canonical_id: Arc<str>,
    pub surface_kind: SyntheticCarrierSurfaceKind,
    pub slot_name: Option<Arc<str>>,
    pub binding_name: Arc<str>,
    pub value_node: u64,
}

/// Internal type expression node.
///
/// Syntax-preserving — captures TypeScript type annotation structure
/// without evaluating or normalizing it.
///
/// `Hash` is implemented by hand (NOT derived) as a depth-safe
/// continuation-frame iterative walker — see the `impl Hash for TypeExpr`
/// in the [`recursive_traversal`] module. The derived `Hash` was
/// recursive over the `Arc<TypeExpr>` tree
/// and overflowed the stack on deeply-nested types (e.g.
/// `cycle_guard::hash_type_expr` routes a `TypeExpr` through `Hash`). The
/// manual impl emits a BYTE-IDENTICAL stream to the former derive
/// (pinned by `tests/hash_byte_stream_contract.rs`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeExpr {
    // -- Terminals --
    /// A primitive type name: `string`, `number`, `boolean`, `symbol`,
    /// `bigint`, `any`, `unknown`, `void`, `never`, `null`, `undefined`, `object`.
    Primitive(PrimitiveName),

    /// A literal type: `"hello"`, `42`, `true`, `false`.
    Literal(LiteralValue),

    // -- Compound --
    /// `A | B | C`
    Union(Arc<[TypeExpr]>),

    /// `A & B & C`
    Intersection(Arc<[TypeExpr]>),

    /// `T[]` or `Array<T>` or `ReadonlyArray<T>`.
    Array {
        element: Arc<TypeExpr>,
        readonly: bool,
    },

    /// `[A, B, C]` — optionally labeled.
    Tuple {
        elements: Arc<[TupleElement]>,
        readonly: bool,
    },

    /// `{ prop: Type; prop?: Type; [key: string]: Type }`
    Object(Arc<ObjectExpr>),

    /// `(x: T, y: U) => R`
    Function(Arc<FunctionExpr>),

    /// `new (x: T) => R` — a constructor type (TS `TSConstructorType`).
    ///
    /// Distinct from both [`Function`](Self::Function) and from a type-literal
    /// `{ new (): R }` (which lowers to [`Object`](Self::Object) carrying an
    /// [`ObjectMember::ConstructSignature`]). The two are structurally identical
    /// after lowering otherwise, yet Vue's runtime-constructor inference treats a
    /// bare constructor *type* as `Function` while a type-literal-with-construct-
    /// signature is `Object` — so the producer must keep them apart. Carries the
    /// same [`FunctionExpr`] payload (parameters / return / type parameters /
    /// spans) as a construct signature, so a consumer that wants the construct
    /// semantics walks the inner function exactly as it does for a
    /// `ConstructSignature` member.
    ConstructorType(Arc<FunctionExpr>),

    // -- References --
    /// A named type reference, optionally with type arguments.
    /// `MyType`, `Partial<T>`, `Record<K, V>`.
    Ref {
        name: Arc<str>,
        type_arguments: Arc<[TypeExpr]>,
    },

    /// A first-class generic type parameter reference carrying declaration metadata.
    TypeParameter(TypeParam),

    // -- Operators --
    /// `keyof T`
    KeyOf(Arc<TypeExpr>),

    /// `typeof x` — refers to a value binding.
    TypeOf(ValueRef),

    /// `T[K]` — indexed access.
    IndexedAccess {
        object: Arc<TypeExpr>,
        index: Arc<TypeExpr>,
    },

    /// `T extends U ? A : B`
    Conditional {
        check: Arc<TypeExpr>,
        extends: Arc<TypeExpr>,
        true_type: Arc<TypeExpr>,
        false_type: Arc<TypeExpr>,
    },

    /// `{ [K in Source]: Value }` — mapped type.
    Mapped {
        parameter: String,
        source: Arc<TypeExpr>,
        value: Arc<TypeExpr>,
        optional: MappedModifier,
        readonly: MappedModifier,
        name_type: Option<Arc<TypeExpr>>,
    },

    /// `` `prefix${T}suffix` `` — template literal type.
    TemplateLiteral {
        /// Alternating text spans and type expressions.
        /// `quasis[0]` expr[0] `quasis[1]` expr[1] ... `quasis[n]`
        quasis: Vec<String>,
        expressions: Arc<[TypeExpr]>,
    },

    /// `infer T` — only valid inside conditional types.
    Infer { name: String },

    /// `readonly T` or rest `...T` at tuple level (handled by TupleElement).
    /// This variant catches standalone `readonly` or rest when not in tuple context.
    Rest(Arc<TypeExpr>),

    /// Parenthesized type — `(A | B)`. Preserved for fidelity but
    /// transparent to evaluation.
    Parenthesized(Arc<TypeExpr>),

    /// A recursive type reference placeholder — produced by the solver when
    /// recursion is detected during type expansion. Preserves the recursive
    /// symbol name, applied type arguments, and active conditional context.
    RecursiveRef {
        name: Arc<str>,
        type_arguments: Arc<[TypeExpr]>,
        conditional_context: Arc<[RecursiveConditionalFrame]>,
    },

    /// Synthetic slot-binding / `defineSlots` binding carrier. Minted only
    /// at the no-parser branch of `publish_merged_bindings`. The
    /// projector pipeline and component-meta registry treat this variant
    /// as a shallow terminal — explicit deep materialisation routes
    /// through `ShapeCacheKey::semantic_node_whole(scope, value_node,
    /// mode)`. See `[[component-meta-shallow-by-default-rule]]`.
    SyntheticSlotBinding(Arc<SyntheticCarrierKey>),

    /// A type the lowering could not represent.
    /// Carries the raw source text for diagnostics.
    Unknown { raw: String },
}

// ---------------------------------------------------------------------------
// Recursive conditional context types
// ---------------------------------------------------------------------------

/// A snapshot of one conditional branch frame at the moment recursion was detected.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RecursiveConditionalFrame {
    pub branch: RecursiveConditionalBranch,
    pub decided: bool,
    pub check: Arc<TypeExpr>,
    pub extends: Arc<TypeExpr>,
}

/// Which branch of a conditional type was active.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RecursiveConditionalBranch {
    True,
    False,
}

// ---------------------------------------------------------------------------
// Supporting types
// ---------------------------------------------------------------------------

/// Primitive type names recognized by the evaluator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PrimitiveName {
    String,
    Number,
    Boolean,
    Symbol,
    BigInt,
    Any,
    Unknown,
    Void,
    Never,
    Null,
    Undefined,
    Object,
}

impl PrimitiveName {
    /// Try to parse a primitive name from a string.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "string" => Some(Self::String),
            "number" => Some(Self::Number),
            "boolean" => Some(Self::Boolean),
            "symbol" => Some(Self::Symbol),
            "bigint" => Some(Self::BigInt),
            "any" => Some(Self::Any),
            "unknown" => Some(Self::Unknown),
            "void" => Some(Self::Void),
            "never" => Some(Self::Never),
            "null" => Some(Self::Null),
            "undefined" => Some(Self::Undefined),
            "object" => Some(Self::Object),
            _ => None,
        }
    }

    /// Returns the canonical string representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::String => "string",
            Self::Number => "number",
            Self::Boolean => "boolean",
            Self::Symbol => "symbol",
            Self::BigInt => "bigint",
            Self::Any => "any",
            Self::Unknown => "unknown",
            Self::Void => "void",
            Self::Never => "never",
            Self::Null => "null",
            Self::Undefined => "undefined",
            Self::Object => "object",
        }
    }
}

impl fmt::Display for PrimitiveName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A literal value in a type position.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "literalKind", rename_all = "camelCase")]
pub enum LiteralValue {
    String(String),
    Number(f64),
    Boolean(bool),
    BigInt(String),
}

// Manual PartialEq: f64 NaN must compare as equal for type identity.
impl PartialEq for LiteralValue {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::String(a), Self::String(b)) => a == b,
            (Self::Number(a), Self::Number(b)) => a.to_bits() == b.to_bits(),
            (Self::Boolean(a), Self::Boolean(b)) => a == b,
            (Self::BigInt(a), Self::BigInt(b)) => a == b,
            _ => false,
        }
    }
}

impl Eq for LiteralValue {}

impl Hash for LiteralValue {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            Self::String(value) => {
                0u8.hash(state);
                value.hash(state);
            }
            Self::Number(value) => {
                1u8.hash(state);
                value.to_bits().hash(state);
            }
            Self::Boolean(value) => {
                2u8.hash(state);
                value.hash(state);
            }
            Self::BigInt(value) => {
                3u8.hash(state);
                value.hash(state);
            }
        }
    }
}

/// A reference to a value binding (for `typeof` expressions).
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValueRef {
    /// Dotted path segments: `typeof a.b.c` → `["a", "b", "c"]`.
    pub path: Vec<String>,
}

/// A single element in a tuple type.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TupleElement {
    /// Optional label name.
    pub label: Option<String>,
    /// The element type.
    pub ty: TypeExpr,
    /// Whether this element is optional (`?`).
    pub optional: bool,
    /// Whether this element is a rest element (`...T`).
    pub rest: bool,
}

/// An object type expression.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectExpr {
    pub properties: Vec<ObjectMember>,
}

/// A member of an object type.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(tag = "memberKind", rename_all = "camelCase")]
pub enum ObjectMember {
    /// Named property: `name: Type` or `name?: Type`.
    Property(ObjectProperty),
    /// Index signature: `[key: string]: Type`.
    IndexSignature(IndexSignature),
    /// Call signature: `(x: T): R`.
    CallSignature(FunctionExpr),
    /// Construct signature: `new (x: T): R`.
    ConstructSignature(FunctionExpr),
    /// Method signature: `method(x: T): R`.
    Method(MethodSignature),
}

/// OXC-derived declaration-site spans for a named member (property or method).
///
/// Stamped once during shallow OXC lowering (the sole place the AST offsets
/// exist) and carried verbatim through the IR into the semantic graph payload.
/// Every span is in the owning file's source coordinates. `None` only for a
/// genuinely synthetic member (one with no single source site); never as a
/// "not implemented" placeholder.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct MemberSpans {
    /// Span of the whole member declaration (`name?: T` / `name(): T`).
    pub declaration: Option<Span>,
    /// Span of the member's name token.
    pub name: Option<Span>,
    /// Span of the member's type-annotation (the `T` after `:`), when present.
    pub type_annotation: Option<Span>,
}

impl MemberSpans {
    /// Spans for a member where ONLY the name span is known (an aggregate
    /// surface synthesized from per-field analysis, where the field tracks the
    /// name span but the declaration is not a single contiguous source range).
    ///
    /// An empty span (`start >= end`, e.g. a default placeholder) carries no
    /// real provenance, so it maps to `None` rather than fabricating a byte-0
    /// span — honest absence, never a wrong offset.
    #[must_use]
    pub fn name_only(name: Span) -> Self {
        Self {
            declaration: None,
            name: (!name.is_empty()).then_some(name),
            type_annotation: None,
        }
    }
}

/// OXC-derived spans for a call / construct / method function signature.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct FunctionSpans {
    /// Span of the whole signature declaration.
    pub signature: Option<Span>,
    /// Span of the return-type annotation, when present.
    pub return_type: Option<Span>,
}

/// OXC-derived spans for an index signature (`[k: K]: V`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct IndexSignatureSpans {
    /// Span of the whole index-signature declaration.
    pub declaration: Option<Span>,
    /// Span of the key declaration (`[k: K]` parameter / key-type).
    pub key: Option<Span>,
    /// Span of the value-type annotation.
    pub value: Option<Span>,
}

/// Declared accessibility of a class member on the shared type-IR surface.
///
/// This is the canonical visibility carrier for [`ObjectProperty`] and
/// [`MethodSignature`]. It is populated from the OXC `TSAccessibility` token
/// (`None` / `public` → [`Public`], `protected` → [`Protected`], `private` →
/// [`Private`]) when the analyzer lowers a class declaration; every other
/// member origin (interface, type-literal, object-literal, mapped type,
/// synthetic merge) is [`Public`] by default.
///
/// Visibility participates in node identity (Eq / Hash): a `private foo` and a
/// `public foo` are genuinely distinct surfaces, mirroring how `spans` already
/// extends member identity. The published-prop surface re-applies a
/// [`Public`]-only filter at the publication boundary, so non-public members
/// stay recorded on the shared surface without leaking as Vue props.
///
/// [`Public`]: MemberVisibility::Public
/// [`Protected`]: MemberVisibility::Protected
/// [`Private`]: MemberVisibility::Private
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum MemberVisibility {
    /// `public` class member, or any non-class member (interface / type-literal
    /// / object-literal / mapped / synthetic) — the default.
    #[default]
    Public,
    /// `protected` class member.
    Protected,
    /// `private` class member.
    Private,
}

impl MemberVisibility {
    /// Whether this member is publicly visible. Mirrors
    /// `MacroVisibility::is_public` / `ResolvedMemberVisibility::is_public`.
    #[must_use]
    pub const fn is_public(self) -> bool {
        matches!(self, Self::Public)
    }

    /// The lowercase wire string for this visibility, matching the
    /// `MacroVisibility::as_wire_str` mapping the `native_props` carrier uses.
    #[must_use]
    pub const fn as_wire_str(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Protected => "protected",
            Self::Private => "private",
        }
    }

    /// Restrictiveness rank: `Public` (0) is least restrictive, `Private` (2) is
    /// most restrictive. Used by [`most_restrictive`](Self::most_restrictive) to
    /// aggregate contributor visibilities.
    const fn restrictiveness(self) -> u8 {
        match self {
            Self::Public => 0,
            Self::Protected => 1,
            Self::Private => 2,
        }
    }

    /// Aggregate two member visibilities to the MORE restrictive of the two
    /// (`Private` > `Protected` > `Public`). This is the single shared rule for
    /// every merge-synthesis contributor aggregation (intersection / union
    /// surface merge, registry object merge): a merged member is `Public` only
    /// when it is `Public` in BOTH inputs, so a member that is non-public in any
    /// contributor can never be synthesized as `Public`. Commutative and
    /// associative.
    #[must_use]
    pub const fn most_restrictive(self, other: Self) -> Self {
        if other.restrictiveness() > self.restrictiveness() {
            other
        } else {
            self
        }
    }

    /// Fold a set of contributor visibilities to the MOST restrictive of them
    /// via [`most_restrictive`](Self::most_restrictive). This is the single
    /// shared multi-contributor merge rule: every merge that aggregates a
    /// member from more than one source (ordinary union common-member,
    /// intersection merge, registry duplicate property/method merge, conditional
    /// macro-payload branch merge) MUST route through this fold rather than
    /// re-implementing the loop, so a member that is non-public in ANY
    /// contributor can never be synthesized as `Public`. An empty contributor
    /// set folds to `Public` (the identity: nothing restricts visibility).
    #[must_use]
    pub fn merge_member_visibility(
        contributors: impl IntoIterator<Item = MemberVisibility>,
    ) -> MemberVisibility {
        contributors
            .into_iter()
            .fold(MemberVisibility::Public, MemberVisibility::most_restrictive)
    }
}

/// A named property in an object type.
///
/// `spans` carries OXC declaration-site provenance (see [`MemberSpans`]) and is
/// in-memory-only — it is intentionally excluded from the JSON wire shape (the
/// manual `to_json_value` / `type_expr_from_json` helpers do not serialize it).
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct ObjectProperty {
    pub name: String,
    pub ty: TypeExpr,
    pub optional: bool,
    pub readonly: bool,
    /// Declared accessibility of the member. `Public` for every non-class
    /// origin; class members carry their `TSAccessibility`. Participates in
    /// node identity (Eq / Hash). Serialized with `#[serde(default)]` so a
    /// non-public value survives a roundtrip and pre-existing JSON without the
    /// field deserializes as `Public`.
    #[serde(default)]
    pub visibility: MemberVisibility,
    /// OXC declaration-site spans (in-memory provenance; not serialized).
    #[serde(skip)]
    pub spans: MemberSpans,
}

impl ObjectProperty {
    /// Construct a genuinely SOURCE-LESS, semantically-public property: no
    /// single declaration site and no accessibility origin (a synthesized
    /// framework member, an interface / type-literal / object-literal / enum
    /// member, a `$props`/`$slots` member, a test fixture). Visibility is
    /// `Public` by construction because the origin has no accessibility — NOT
    /// because visibility was unknown. Source-DERIVED reconstructions (where a
    /// member already carries a visibility) MUST use
    /// [`Self::synthetic_with_visibility`] or [`Self::with_visibility`] so a
    /// non-public member can never be silently minted as `Public`.
    #[must_use]
    pub fn synthetic_public(name: String, ty: TypeExpr, optional: bool, readonly: bool) -> Self {
        Self {
            name,
            ty,
            optional,
            readonly,
            visibility: MemberVisibility::Public,
            spans: MemberSpans::default(),
        }
    }

    /// Construct a SOURCE-DERIVED property with NO source spans, threading the
    /// source member's declared `visibility`. Used by member-path / Pick /
    /// indexed-access reconstruction where the navigated member already carries
    /// a visibility that must be preserved (so a non-public member is never
    /// re-minted as `Public`), but the reconstruction has no single span.
    #[must_use]
    pub fn synthetic_with_visibility(
        name: String,
        ty: TypeExpr,
        optional: bool,
        readonly: bool,
        visibility: MemberVisibility,
    ) -> Self {
        Self {
            name,
            ty,
            optional,
            readonly,
            visibility,
            spans: MemberSpans::default(),
        }
    }

    /// Construct a genuinely SOURCE-LESS, semantically-public property carrying
    /// its OXC declaration-site spans. The AST form (interface / type-literal /
    /// object-literal member) has no accessibility origin, so `Public` is
    /// correct by construction. Source-DERIVED reconstructions MUST use
    /// [`Self::with_visibility`].
    #[must_use]
    pub fn with_spans_public(
        name: String,
        ty: TypeExpr,
        optional: bool,
        readonly: bool,
        spans: MemberSpans,
    ) -> Self {
        Self {
            name,
            ty,
            optional,
            readonly,
            visibility: MemberVisibility::Public,
            spans,
        }
    }

    /// Construct a property carrying both its declared accessibility and its
    /// OXC declaration-site spans. Used by the analyzer's class lowerer to mint
    /// non-public class members onto the shared surface.
    #[must_use]
    pub fn with_visibility(
        name: String,
        ty: TypeExpr,
        optional: bool,
        readonly: bool,
        visibility: MemberVisibility,
        spans: MemberSpans,
    ) -> Self {
        Self {
            name,
            ty,
            optional,
            readonly,
            visibility,
            spans,
        }
    }
}

/// An index signature: `[key: KeyType]: ValueType`.
///
/// `spans` carries OXC declaration-site provenance and is in-memory-only.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct IndexSignature {
    pub key_name: String,
    pub key_type: TypeExpr,
    pub value_type: TypeExpr,
    pub readonly: bool,
    /// OXC declaration-site spans (in-memory provenance; not serialized).
    #[serde(skip)]
    pub spans: IndexSignatureSpans,
}

impl IndexSignature {
    /// Construct an index signature with NO source spans.
    #[must_use]
    pub fn synthetic(
        key_name: String,
        key_type: TypeExpr,
        value_type: TypeExpr,
        readonly: bool,
    ) -> Self {
        Self {
            key_name,
            key_type,
            value_type,
            readonly,
            spans: IndexSignatureSpans::default(),
        }
    }

    /// Construct an index signature carrying its OXC declaration-site spans.
    #[must_use]
    pub fn with_spans(
        key_name: String,
        key_type: TypeExpr,
        value_type: TypeExpr,
        readonly: bool,
        spans: IndexSignatureSpans,
    ) -> Self {
        Self {
            key_name,
            key_type,
            value_type,
            readonly,
            spans,
        }
    }
}

/// A method signature in an object type.
///
/// `spans` carries OXC declaration-site provenance and is in-memory-only.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct MethodSignature {
    pub name: String,
    pub function: FunctionExpr,
    pub optional: bool,
    /// Declared accessibility of the member. `Public` for every non-class
    /// origin; class methods carry their `TSAccessibility`. Participates in
    /// node identity (Eq / Hash). Serialized with `#[serde(default)]` so a
    /// non-public value survives a roundtrip and pre-existing JSON without the
    /// field deserializes as `Public`.
    #[serde(default)]
    pub visibility: MemberVisibility,
    /// OXC declaration-site spans (in-memory provenance; not serialized).
    #[serde(skip)]
    pub spans: MemberSpans,
}

impl MethodSignature {
    /// Construct a genuinely SOURCE-LESS, semantically-public method signature:
    /// no single declaration site and no accessibility origin (a synthesized
    /// framework member, an interface / type-literal method, a test fixture).
    /// Visibility is `Public` by construction because the origin has no
    /// accessibility. Source-DERIVED reconstructions MUST use
    /// [`Self::synthetic_with_visibility`] or [`Self::with_visibility`].
    #[must_use]
    pub fn synthetic_public(name: String, function: FunctionExpr, optional: bool) -> Self {
        Self {
            name,
            function,
            optional,
            visibility: MemberVisibility::Public,
            spans: MemberSpans::default(),
        }
    }

    /// Construct a SOURCE-DERIVED method signature with NO source spans,
    /// threading the source member's declared `visibility`. Used by member-path
    /// / Pick reconstruction where the navigated method already carries a
    /// visibility that must be preserved.
    #[must_use]
    pub fn synthetic_with_visibility(
        name: String,
        function: FunctionExpr,
        optional: bool,
        visibility: MemberVisibility,
    ) -> Self {
        Self {
            name,
            function,
            optional,
            visibility,
            spans: MemberSpans::default(),
        }
    }

    /// Construct a genuinely SOURCE-LESS, semantically-public method signature
    /// carrying its OXC declaration-site spans. The AST form (interface /
    /// type-literal method) has no accessibility origin, so `Public` is correct
    /// by construction. Source-DERIVED reconstructions MUST use
    /// [`Self::with_visibility`].
    #[must_use]
    pub fn with_spans_public(
        name: String,
        function: FunctionExpr,
        optional: bool,
        spans: MemberSpans,
    ) -> Self {
        Self {
            name,
            function,
            optional,
            visibility: MemberVisibility::Public,
            spans,
        }
    }

    /// Construct a method signature carrying both its declared accessibility
    /// and its OXC declaration-site spans. Used by the analyzer's class lowerer
    /// to mint non-public class methods onto the shared surface.
    #[must_use]
    pub fn with_visibility(
        name: String,
        function: FunctionExpr,
        optional: bool,
        visibility: MemberVisibility,
        spans: MemberSpans,
    ) -> Self {
        Self {
            name,
            function,
            optional,
            visibility,
            spans,
        }
    }
}

/// A function type expression.
///
/// `spans` carries OXC declaration-site provenance and is in-memory-only.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct FunctionExpr {
    pub parameters: Vec<FunctionParam>,
    pub return_type: Option<Arc<TypeExpr>>,
    pub type_parameters: Vec<TypeParam>,
    /// OXC signature / return spans (in-memory provenance; not serialized).
    #[serde(skip)]
    pub spans: FunctionSpans,
}

impl FunctionExpr {
    /// Construct a function expression with NO source spans.
    #[must_use]
    pub fn synthetic(
        parameters: Vec<FunctionParam>,
        return_type: Option<Arc<TypeExpr>>,
        type_parameters: Vec<TypeParam>,
    ) -> Self {
        Self {
            parameters,
            return_type,
            type_parameters,
            spans: FunctionSpans::default(),
        }
    }

    /// Construct a function expression carrying its OXC spans.
    #[must_use]
    pub fn with_spans(
        parameters: Vec<FunctionParam>,
        return_type: Option<Arc<TypeExpr>>,
        type_parameters: Vec<TypeParam>,
        spans: FunctionSpans,
    ) -> Self {
        Self {
            parameters,
            return_type,
            type_parameters,
            spans,
        }
    }
}

/// A function parameter.
///
/// `span` is the OXC parameter span (in-memory provenance; not serialized).
///
/// `PartialEq`/`Eq`/`Hash` are implemented by hand to EXCLUDE
/// [`has_ts_annotation`](Self::has_ts_annotation): that field is a transient
/// lowering-time gate for JSDoc `@param` precedence, not part of a parameter's
/// semantic type identity. Two parameters with the same name / type / optional /
/// rest / span are the same parameter whether the annotation was written
/// explicitly or filled from JSDoc — and the graph round-trip (the canonical
/// semantic form) intentionally does not preserve the fact, so it must not split
/// otherwise-equal parameters across cache keys or equivalence checks. `span`
/// remains part of identity (it is a real provenance coordinate).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct FunctionParam {
    pub name: Option<String>,
    pub ty: TypeExpr,
    pub optional: bool,
    pub rest: bool,
    /// OXC span of the whole parameter (in-memory provenance; not serialized).
    #[serde(skip)]
    pub span: Option<Span>,
    /// Whether this parameter carried an explicit TS type annotation at its
    /// declaration site (`FormalParameter`/`BindingPattern` had a
    /// `type_annotation`). This is the OXC STRUCTURAL FACT — captured once at
    /// the lowering site — NOT a sentinel inferred from the lowered [`TypeExpr`]
    /// (an explicit `: any` lowers to `Primitive(Any)` exactly like a missing
    /// annotation does, so the lowered type cannot distinguish them). JSDoc
    /// `@param` backfill fills a parameter ONLY when this is `false`, matching
    /// TS precedence (an explicit annotation — including `: any` — always wins).
    /// In-memory provenance; not serialized and NOT part of type identity (see
    /// the type-level note on the hand-written `PartialEq`/`Eq`/`Hash`).
    #[serde(skip)]
    pub has_ts_annotation: bool,
}

impl PartialEq for FunctionParam {
    fn eq(&self, other: &Self) -> bool {
        // `has_ts_annotation` is a transient lowering-time gate, not semantic
        // identity — deliberately excluded so equivalent parameters built by
        // different paths (e.g. the eager IR path vs the graph round-trip)
        // compare equal.
        self.name == other.name
            && self.ty == other.ty
            && self.optional == other.optional
            && self.rest == other.rest
            && self.span == other.span
    }
}

impl Eq for FunctionParam {}

impl std::hash::Hash for FunctionParam {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // Mirror `PartialEq`: hash every identity field EXCEPT
        // `has_ts_annotation`, so equal parameters hash equally.
        self.name.hash(state);
        self.ty.hash(state);
        self.optional.hash(state);
        self.rest.hash(state);
        self.span.hash(state);
    }
}

impl FunctionParam {
    /// Construct a parameter with NO source span. A synthesized parameter has no
    /// declaration site, so it carries no TS-annotation fact (`has_ts_annotation
    /// == false`); synthesized parameters are never JSDoc-enriched.
    #[must_use]
    pub fn synthetic(name: Option<String>, ty: TypeExpr, optional: bool, rest: bool) -> Self {
        Self {
            name,
            ty,
            optional,
            rest,
            span: None,
            has_ts_annotation: false,
        }
    }

    /// Construct a parameter carrying its OXC span and the structural fact of
    /// whether it had an explicit TS type annotation at its declaration site.
    #[must_use]
    pub fn with_span(
        name: Option<String>,
        ty: TypeExpr,
        optional: bool,
        rest: bool,
        span: Option<Span>,
        has_ts_annotation: bool,
    ) -> Self {
        Self {
            name,
            ty,
            optional,
            rest,
            span,
            has_ts_annotation,
        }
    }
}

/// A type parameter declaration: `T extends Constraint = Default`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TypeParam {
    pub name: String,
    pub constraint: Option<Arc<TypeExpr>>,
    pub default: Option<Arc<TypeExpr>>,
}

/// Modifier for mapped type `optional` and `readonly` fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MappedModifier {
    /// No modifier applied.
    None,
    /// `+` or bare modifier (add).
    Add,
    /// `-` modifier (remove).
    Remove,
}

// ---------------------------------------------------------------------------
// Shared constants
// ---------------------------------------------------------------------------

/// Returns a shared empty type argument slice, avoiding per-call allocation.
pub fn empty_type_args() -> Arc<[TypeExpr]> {
    static EMPTY: LazyLock<Arc<[TypeExpr]>> = LazyLock::new(|| Arc::from(Vec::<TypeExpr>::new()));
    Arc::clone(&EMPTY)
}

// ---------------------------------------------------------------------------
// Factory helpers
// ---------------------------------------------------------------------------

impl TypeExpr {
    /// Create a primitive type.
    pub fn primitive(name: PrimitiveName) -> Self {
        Self::Primitive(name)
    }

    /// Create a string literal type.
    pub fn string_literal(s: impl Into<String>) -> Self {
        Self::Literal(LiteralValue::String(s.into()))
    }

    /// Create a number literal type.
    pub fn number_literal(n: f64) -> Self {
        Self::Literal(LiteralValue::Number(n))
    }

    /// Create a boolean literal type.
    pub fn boolean_literal(b: bool) -> Self {
        Self::Literal(LiteralValue::Boolean(b))
    }

    /// Create a union type. Empty → `never`, single → unwrap.
    pub fn union(types: Vec<TypeExpr>) -> Self {
        match types.len() {
            0 => Self::Primitive(PrimitiveName::Never),
            1 => types.into_iter().next().unwrap(),
            _ => Self::Union(Arc::from(types)),
        }
    }

    /// Create an intersection type. Empty → `unknown`, single → unwrap.
    pub fn intersection(types: Vec<TypeExpr>) -> Self {
        match types.len() {
            0 => Self::Primitive(PrimitiveName::Unknown),
            1 => types.into_iter().next().unwrap(),
            _ => Self::Intersection(Arc::from(types)),
        }
    }

    /// Create a type reference without type arguments.
    pub fn named(name: impl Into<String>) -> Self {
        Self::Ref {
            name: Arc::from(name.into()),
            type_arguments: empty_type_args(),
        }
    }

    /// Create a type reference with type arguments.
    pub fn named_with_args(name: impl Into<String>, args: Vec<TypeExpr>) -> Self {
        Self::Ref {
            name: Arc::from(name.into()),
            type_arguments: Arc::from(args),
        }
    }

    /// Create a first-class generic type parameter reference.
    pub fn type_parameter(param: TypeParam) -> Self {
        Self::TypeParameter(param)
    }

    /// Create a recursive ref with no conditional context.
    pub fn recursive_ref(name: impl Into<String>, args: Vec<TypeExpr>) -> Self {
        Self::RecursiveRef {
            name: Arc::from(name.into()),
            type_arguments: Arc::from(args),
            conditional_context: Arc::from(Vec::<RecursiveConditionalFrame>::new()),
        }
    }

    /// Create a synthetic slot-binding / `defineSlots` binding carrier.
    /// See [`SyntheticCarrierKey`] for identity semantics.
    pub fn synthetic_slot_binding(key: SyntheticCarrierKey) -> Self {
        Self::SyntheticSlotBinding(Arc::new(key))
    }

    /// Returns `true` if this is a `RecursiveRef` node.
    pub fn is_recursive_ref(&self) -> bool {
        matches!(self, Self::RecursiveRef { .. })
    }

    /// Returns `true` if this is an `Unknown` node.
    pub fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown { .. })
    }

    /// Returns `true` if this is a primitive type.
    pub fn is_primitive(&self) -> bool {
        matches!(self, Self::Primitive(_))
    }
}

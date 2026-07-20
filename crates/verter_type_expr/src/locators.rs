//! Content-free authored-body locators — the cross-boundary keyable inverse of
//! a session `HotTypeRef`.
//!
//! A locator addresses an AUTHORED, parse-backed source position by a
//! content-free anchor (`canonical_id` + `symbol` + `space`) plus a
//! producer-emitted intra-decl / payload path of named positions / small
//! indices. A locator NEVER embeds a `TypeExpr`, NEVER stores a byte span as
//! identity, and carries NO env-hash dims / content hash / `SemanticNodeId` /
//! `HotTypeRef` / versioned `DeclIdentity`. It derives `Hash + Eq` so it can key
//! the session-side lowered-body memo (the keying is [`crate::locators`]-neutral;
//! the memo itself lives in `verter_session`).

use std::sync::Arc;
use verter_no_storedspan::NoStoredSpan;
use verter_no_typeexpr::NoTypeExpr;

use crate::TopLevelOwnerId;

/// Symbol space of an authored declaration anchor. The lower-neutral analogue of
/// the session `SemanticSymbolSpace` — `verter_type_expr` cannot depend on
/// `verter_session`, so the locator carries its own closed space tag.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub enum LocatorSymbolSpace {
    /// Type-space declaration (interface, type alias, enum/class type half).
    Type,
    /// Value-space declaration (function, const, enum/class value half).
    Value,
    /// Namespace-space declaration.
    Namespace,
}

/// The content-free anchor of an authored body: the PRODUCING canonical (the
/// `TypeExprScope` canonical, which may be a cross-file resolver's canonical, not
/// the component owner) + the merged symbol name + its space.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub struct AuthoredAnchor {
    /// Canonical id of the file whose parse produced the authored body.
    pub canonical_id: Arc<str>,
    /// Top-level lexical owner within the producing file.
    pub owner: TopLevelOwnerId,
    /// Stable merged-symbol name of the owning declaration.
    pub symbol: Arc<str>,
    /// Type / value / namespace space of the owning declaration.
    pub space: LocatorSymbolSpace,
}

/// Which authored bound slot of a type parameter a locator addresses.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub enum TypeParamBoundPosition {
    /// The constraint body (`T extends C`).
    Constraint,
    /// The default body (`T = D`).
    Default,
}

/// TypeScript lexical visibility of a declaration header's type parameters
/// from one authored position: which sibling parameters a reference at that
/// position may bind. Carried WITH a lowered-body product because a bare
/// parameter list cannot express a name that shadows outer declarations
/// while being forbidden as a reference (a default's self / later siblings).
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub enum TypeParamVisibility {
    /// A body position: every header parameter is a referenceable binder.
    Body,
    /// The constraint bound of the parameter at `ordinal`: EVERY sibling
    /// (prior, self, later) is referenceable — TypeScript constraints may
    /// reference later siblings (`type Foo<T extends U, U>`) and the
    /// parameter itself (F-bounded, `T extends Comparable<T>`).
    Constraint {
        /// Declared ordinal of the parameter owning the bound.
        ordinal: u32,
    },
    /// The default bound of the parameter at `ordinal`: parameters declared
    /// BEFORE `ordinal` are referenceable; the parameter itself and later
    /// siblings shadow outer same-named declarations but are FORBIDDEN as
    /// references (TypeScript rejects forward / self default references) —
    /// such a reference resolves unbound-within-frame, never to an outer
    /// declaration.
    Default {
        /// Declared ordinal of the parameter owning the bound.
        ordinal: u32,
    },
}

/// A producer-emitted step from a decl body toward an authored sub-position.
/// Named positions / small indices only — never a byte span, never a `TypeExpr`.
/// The arm set is a closed schema; adding an arm is a reviewed schema event.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub enum TypeBodyPathStep {
    /// Into an ordered merged-declaration contributor.
    MergedContributor { ordinal: u32 },
    /// Into an intersection arm of the body.
    IntersectionArm { ordinal: u32 },
    /// Into the object / interface member at this ordinal.
    Member { ordinal: u32 },
    /// Into the value-type surface of the current member.
    MemberValue,
    /// Into the constraint / default body of the owning declaration's type
    /// parameter at this ordinal (`T extends C` / `T = D`). The bound slot is
    /// selected by `position`. Valid ONLY as the first path step (rooted at the
    /// decl header): type parameters live on the declaration header, not inside
    /// the body expression, so this step never appears mid-path.
    TypeParamBound {
        ordinal: u32,
        position: TypeParamBoundPosition,
    },
    /// Into the type annotation of the current function-like node's parameter at
    /// this ordinal (source order). Deref selects the current function-like node
    /// — a `TSFunctionType` / `TSConstructorType`, or a method / call-signature /
    /// construct-signature member reached by the preceding path — then descends
    /// into its authored parameter list (`params.items`) at `ordinal` and derefs
    /// to that parameter's `type_annotation`. An ordinal past the parameter list,
    /// or a parameter that carries no authored annotation, is a TYPED miss (never
    /// a fabricated body).
    FunctionParam { ordinal: u32 },
    /// Into the return type of the current function-like node. Deref selects the
    /// current `TSFunctionType` / `TSConstructorType` / method / call-signature /
    /// construct-signature node's `return_type` annotation. An ABSENT return
    /// annotation (an inferred / `void` return with no authored `TSType`) is a
    /// TYPED miss — never a fabricated body.
    FunctionReturn,
    /// Into the value-space function signature at this ordinal (source order),
    /// BEFORE descending into its parameters / return. Deref selects the owning
    /// value declaration's `ordinal`-th authored function signature (one member
    /// of an overload group); a following [`Self::FunctionParam`] /
    /// [`Self::FunctionReturn`] step then descends into it. An ordinal past the
    /// overload group is a TYPED miss.
    ValueSignature { ordinal: u32 },
    /// Into the source / key-constraint type of the current mapped type
    /// (`{ [K in Source]: Value }`). Deref selects the current `TSMappedType`'s
    /// constraint (the `Source` of `[K in Source]`).
    MappedSource,
    /// Into the value / template type of the current mapped type
    /// (`{ [K in Source]: Value }`). Deref selects the current `TSMappedType`'s
    /// value `type_annotation`. An absent value annotation is a TYPED miss.
    MappedValue,
    /// Into the name-remap (`as N`) type of the current mapped type
    /// (`{ [K in Source as N]: Value }`). Deref selects the current
    /// `TSMappedType`'s `name_type`. An ABSENT `name_type` (a mapped type with no
    /// `as` clause) is a TYPED miss — never a fabricated body.
    MappedNameType,
    /// Into the check type of the current conditional
    /// (`Check extends Extends ? True : False`). Deref selects the current
    /// `TSConditionalType`'s `check_type`.
    ConditionalCheck,
    /// Into the extends type of the current conditional. Deref selects the
    /// current `TSConditionalType`'s `extends_type`.
    ConditionalExtends,
    /// Into the true branch of the current conditional. Deref selects the current
    /// `TSConditionalType`'s `true_type`.
    ConditionalTrue,
    /// Into the false branch of the current conditional. Deref selects the current
    /// `TSConditionalType`'s `false_type`.
    ConditionalFalse,
    /// Into the union arm at this ordinal (source order). Deref selects the
    /// current `TSUnionType`'s `types[ordinal]`. An ordinal past the arm count is
    /// a TYPED miss.
    UnionArm { ordinal: u32 },
    /// Into the object type of the current indexed access (`Object[Index]`). Deref
    /// selects the current `TSIndexedAccessType`'s `object_type`.
    IndexedAccessObject,
    /// Into the index type of the current indexed access (`Object[Index]`). Deref
    /// selects the current `TSIndexedAccessType`'s `index_type`.
    IndexedAccessIndex,
    /// Into the key type of the current index signature (`[k: Key]: Value`). Deref
    /// selects the current `TSIndexSignature`'s key parameter
    /// (`parameters[0].type_annotation`).
    IndexSignatureKey,
    /// Into the value type of the current index signature (`[k: Key]: Value`).
    /// Deref selects the current `TSIndexSignature`'s value `type_annotation`.
    IndexSignatureValue,
    /// Into the tuple element type at this ordinal (source order). Deref selects
    /// the current `TSTupleType`'s `element_types[ordinal]`, unwrapping a named /
    /// optional / rest tuple-member wrapper to the element's type. An ordinal past
    /// the element count is a TYPED miss.
    TupleElement { ordinal: u32 },
}

/// Locator for a whole authored declaration body OR a named sub-slot of it.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub struct TypeBodySlot {
    /// The owning declaration.
    pub anchor: AuthoredAnchor,
    /// Empty = the whole decl body; non-empty = the named sub-slot.
    pub path: Arc<[TypeBodyPathStep]>,
}

/// Locator for a resolvable symbol body — the frontier / shallow escape for any
/// body that resolves to a named declaration.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub struct SymbolBodyLocator {
    /// The resolvable symbol.
    pub anchor: AuthoredAnchor,
}

/// Locator for one authored type-argument position (heritage args, forward
/// payload args). Addresses the arg-bearing position via `path`, then the arg
/// ordinal within it.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub struct TypeArgLocator {
    /// The owning declaration.
    pub anchor: AuthoredAnchor,
    /// Path to the arg-bearing position (empty = the decl's own header).
    pub path: Arc<[TypeBodyPathStep]>,
    /// Which type argument at that position (source order).
    pub arg_index: u32,
}

/// Position of an authored macro/field payload within its owning declaration.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub enum MacroPayloadPosition {
    /// The generic type argument of a `define*<T>()` macro call.
    TypeArgument,
    /// The object argument of a `define*({ ... })` macro call.
    ObjectArgument,
    /// A specific analyzed field's payload (prop / emit / slot / expose / option).
    Field { field_index: u32 },
    /// The authored type payload of a `$props()` / rune candidate that is a
    /// binding ANNOTATION (`let {…}: T = $props()`) — an authored TYPE position
    /// on the macro call's declarator, not a runtime object argument and not a
    /// per-field payload.
    TypeAnnotation,
}

/// Locator for an authored macro / field payload body. Reuses the PRODUCING
/// canonical's `DeclBodyMemo` snapshot — never a new payload memo (which would
/// re-parse identical source under the identical snapshot key).
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub struct MacroPayloadLocator {
    /// The owning declaration whose retained snapshot backs this payload.
    pub anchor: AuthoredAnchor,
    /// Which macro call within the owning declaration (source order).
    pub macro_index: u32,
    /// The payload position within that macro call.
    pub payload: MacroPayloadPosition,
}

/// Ambient augmentation scope of an augmentation-scoped declaration body —
/// the lower-neutral analogue of the retained-inventory scope tag
/// (`declare global` / `declare module "<specifier>"`). The specifier is the
/// AUTHORED module-specifier text, exactly as retained by the inventory —
/// never a resolved path and never a content hash.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub enum AuthoredAugmentationScope {
    /// A `declare global { ... }` block.
    Global,
    /// A `declare module "<specifier>" { ... }` block.
    Module {
        /// The authored module specifier the block names.
        specifier: Arc<str>,
    },
}

/// Locator for a retained ambient-augmentation contribution body: the inner
/// `symbol` declaration a `declare global` / `declare module "<specifier>"`
/// block contributes inside the anchor's file. Augmentation-scoped inner
/// declarations never enter file-scope symbol inventories, so the plain
/// decl-body anchor cannot address them — the scope tag is part of the
/// authored position's identity.
///
/// An augmentation-scoped `interface` / `type` declaration is an authored
/// type-decl-header declaration, so its intra-body sub-positions — including
/// its type-parameter constraint / default bounds — are addressable through
/// the SAME `path` vocabulary as a top-level [`TypeBodySlot`]: `path` is empty
/// for the whole augmentation body, or a named sub-slot otherwise.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub struct AugmentationBodyLocator {
    /// The augmenter file + the augmented symbol name + its space.
    pub anchor: AuthoredAnchor,
    /// Which ambient augmentation block the contribution lives in.
    pub scope: AuthoredAugmentationScope,
    /// Empty = the whole augmentation contribution body; non-empty = the
    /// named sub-slot (mirrors [`TypeBodySlot::path`]).
    pub path: Arc<[TypeBodyPathStep]>,
}

/// Locator for a JSDoc `@typedef {T} Name` alias body — a NAMED sub-kind of the
/// (a) decl-body source. A `@typedef` registered as a first-class type-alias
/// declaration is a decl-body source whose AUTHORED form is COMMENT TEXT (parsed
/// by `parse_jsdoc_tag_type_payload`), NOT a `TSType` AST node — so the retained
/// snapshot's `TSType` deref cannot reach it directly. The payload is re-derived
/// by re-running `parse_jsdoc_tag_type_payload` on the ANCHORED comment span at
/// deref time — the single sanctioned JSDoc-text exception the typed-IR rule
/// already permits.
///
/// The anchor's `symbol` (the typedef NAME) + `canonical_id` re-locate the
/// `@typedef` comment (each `@typedef Name` registers a distinct alias by name,
/// so the anchor addresses exactly one), so NO byte span is stored as identity.
/// `path` addresses a named sub-position WITHIN the re-parsed payload (empty =
/// the whole `@typedef` body), reusing the closed [`TypeBodyPathStep`]
/// vocabulary — keeping this a NAMED decl-body sub-kind, never an open text
/// escape.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub struct JsdocTypedefBodyLocator {
    /// The typedef alias declaration (producing canonical + typedef NAME + the
    /// [`LocatorSymbolSpace::Type`] space).
    pub anchor: AuthoredAnchor,
    /// Empty = the whole `@typedef` payload body; non-empty = a named sub-slot
    /// within the re-parsed payload (mirrors [`TypeBodySlot::path`]).
    pub path: Arc<[TypeBodyPathStep]>,
}

/// The cross-boundary CONTENT-FREE slot identity — a CLOSED sum over the authored
/// parse-backed source kinds only: (a) decl-body (top-level, augmentation-scoped,
/// or JSDoc `@typedef`) + (b) authored macro/field payloads. The keyable inverse
/// of a session `HotTypeRef`.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub enum AuthoredBodyLocator {
    /// (a) a top-level declaration body (or a named sub-slot of it).
    DeclBody(TypeBodySlot),
    /// (a) an ambient augmentation-scoped contribution body.
    AugmentationBody(AugmentationBodyLocator),
    /// (a) a JSDoc `@typedef {T} Name` alias body — a named decl-body sub-kind
    /// whose authored form is COMMENT TEXT re-derived via
    /// `parse_jsdoc_tag_type_payload`, not a `TSType` node.
    JsdocTypedefBody(JsdocTypedefBodyLocator),
    /// (b) an authored macro / field payload body.
    MacroPayload(MacroPayloadLocator),
}

/// A content-free REFERENCE to one authored type payload: the locator carrying
/// the authored position for re-resolution PLUS a stable STRUCTURAL hash of the
/// authored type for cache discrimination.
///
/// The payload-hash axis exists because a locator alone is position-identity,
/// not content-identity: two candidate captures at the SAME authored position
/// (`$props<{ a: string }>()` edited to `$props<{ a: number }>()`) carry the
/// same locator but MUST discriminate in a content-addressed candidate slot.
/// `payload_hash` is producer-computed from the authored type via a
/// parse-stable structural fingerprint (span-free, alpha-normalised), so the
/// slot stays stable across formatting-only edits while discriminating every
/// authored content change. The referenced payload itself stays authored
/// parse-backed source — never an embedded `TypeExpr`.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub struct AuthoredTypePayloadRef {
    /// The authored position, for re-resolution through the one shared engine.
    pub locator: AuthoredBodyLocator,
    /// Parse-stable structural hash of the authored type (cache discrimination).
    pub payload_hash: [u8; 16],
}

// ===========================================================================
// Producer-local anchor absolutization (cross-owner self-anchoring)
// ===========================================================================
//
// The analyzer's local-file convention stamps `canonical_id: ""` on anchors
// whose producing file IS the consuming scope. That convention is scope-
// RELATIVE: a source cloned across an owner boundary (a child component's
// published source inherited into a parent's fallthrough row) would silently
// re-anchor to the WRONG file when the consumer's scope absolutizes it. The
// `absolutized_against` family rewrites every producer-local (empty) anchor
// to the supplied owning canonical — making the value SELF-ANCHORING — and
// NEVER rewrites an already-absolute anchor.
//
// Each method returns `None` when nothing needed rewriting so callers can
// keep the original allocation (`Arc` slices are shared, not rebuilt).

impl AuthoredAnchor {
    /// `Some(rewritten)` when the anchor is producer-local (empty
    /// `canonical_id`); `None` when it is already absolute.
    pub(crate) fn absolutize(&self, canonical_id: &str) -> Option<Self> {
        self.canonical_id.is_empty().then(|| Self {
            canonical_id: Arc::from(canonical_id),
            owner: self.owner,
            symbol: Arc::clone(&self.symbol),
            space: self.space,
        })
    }
}

impl TypeBodySlot {
    pub(crate) fn absolutize(&self, canonical_id: &str) -> Option<Self> {
        self.anchor.absolutize(canonical_id).map(|anchor| Self {
            anchor,
            path: Arc::clone(&self.path),
        })
    }
}

impl SymbolBodyLocator {
    pub(crate) fn absolutize(&self, canonical_id: &str) -> Option<Self> {
        self.anchor
            .absolutize(canonical_id)
            .map(|anchor| Self { anchor })
    }
}

impl MacroPayloadLocator {
    pub(crate) fn absolutize(&self, canonical_id: &str) -> Option<Self> {
        self.anchor.absolutize(canonical_id).map(|anchor| Self {
            anchor,
            macro_index: self.macro_index,
            payload: self.payload,
        })
    }
}

impl AugmentationBodyLocator {
    pub(crate) fn absolutize(&self, canonical_id: &str) -> Option<Self> {
        self.anchor.absolutize(canonical_id).map(|anchor| Self {
            anchor,
            scope: self.scope.clone(),
            path: Arc::clone(&self.path),
        })
    }
}

impl JsdocTypedefBodyLocator {
    pub(crate) fn absolutize(&self, canonical_id: &str) -> Option<Self> {
        self.anchor.absolutize(canonical_id).map(|anchor| Self {
            anchor,
            path: Arc::clone(&self.path),
        })
    }
}

impl AuthoredBodyLocator {
    pub(crate) fn absolutize(&self, canonical_id: &str) -> Option<Self> {
        match self {
            AuthoredBodyLocator::DeclBody(slot) => slot
                .absolutize(canonical_id)
                .map(AuthoredBodyLocator::DeclBody),
            AuthoredBodyLocator::AugmentationBody(body) => body
                .absolutize(canonical_id)
                .map(AuthoredBodyLocator::AugmentationBody),
            AuthoredBodyLocator::JsdocTypedefBody(body) => body
                .absolutize(canonical_id)
                .map(AuthoredBodyLocator::JsdocTypedefBody),
            AuthoredBodyLocator::MacroPayload(payload) => payload
                .absolutize(canonical_id)
                .map(AuthoredBodyLocator::MacroPayload),
        }
    }

    /// The locator with every producer-local (empty) anchor rewritten to the
    /// supplied owning canonical; an already-absolute locator is returned
    /// unchanged (shared allocations preserved).
    pub fn absolutized_against(&self, canonical_id: &str) -> Self {
        self.absolutize(canonical_id)
            .unwrap_or_else(|| self.clone())
    }
}

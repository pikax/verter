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

/// Symbol space of an authored declaration anchor. The lower-neutral analogue of
/// the session `SemanticSymbolSpace` — `verter_type_expr` cannot depend on
/// `verter_session`, so the locator carries its own closed space tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
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
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub struct AuthoredAnchor {
    /// Canonical id of the file whose parse produced the authored body.
    pub canonical_id: Arc<str>,
    /// Stable merged-symbol name of the owning declaration.
    pub symbol: Arc<str>,
    /// Type / value / namespace space of the owning declaration.
    pub space: LocatorSymbolSpace,
}

/// A producer-emitted step from a decl body toward an authored sub-position.
/// Named positions / small indices only — never a byte span, never a `TypeExpr`.
/// The arm set is a closed schema; adding an arm is a reviewed schema event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub enum TypeBodyPathStep {
    /// Into an ordered merged-declaration contributor.
    MergedContributor { ordinal: u32 },
    /// Into an intersection arm of the body.
    IntersectionArm { ordinal: u32 },
    /// Into the object / interface member at this ordinal.
    Member { ordinal: u32 },
    /// Into the value-type surface of the current member.
    MemberValue,
}

/// Locator for a whole authored declaration body OR a named sub-slot of it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub struct TypeBodySlot {
    /// The owning declaration.
    pub anchor: AuthoredAnchor,
    /// Empty = the whole decl body; non-empty = the named sub-slot.
    pub path: Arc<[TypeBodyPathStep]>,
}

/// Locator for a resolvable symbol body — the frontier / shallow escape for any
/// body that resolves to a named declaration.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub struct SymbolBodyLocator {
    /// The resolvable symbol.
    pub anchor: AuthoredAnchor,
}

/// Locator for one authored type-argument position (heritage args, forward
/// payload args). Addresses the arg-bearing position via `path`, then the arg
/// ordinal within it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub struct TypeArgLocator {
    /// The owning declaration.
    pub anchor: AuthoredAnchor,
    /// Path to the arg-bearing position (empty = the decl's own header).
    pub path: Arc<[TypeBodyPathStep]>,
    /// Which type argument at that position (source order).
    pub arg_index: u32,
}

/// Position of an authored macro/field payload within its owning declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub enum MacroPayloadPosition {
    /// The generic type argument of a `define*<T>()` macro call.
    TypeArgument,
    /// The object argument of a `define*({ ... })` macro call.
    ObjectArgument,
    /// A specific analyzed field's payload (prop / emit / slot / expose / option).
    Field { field_index: u32 },
}

/// Locator for an authored macro / field payload body. Reuses the PRODUCING
/// canonical's `DeclBodyMemo` snapshot — never a new payload memo (which would
/// re-parse identical source under the identical snapshot key).
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
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
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
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
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub struct AugmentationBodyLocator {
    /// The augmenter file + the augmented symbol name + its space.
    pub anchor: AuthoredAnchor,
    /// Which ambient augmentation block the contribution lives in.
    pub scope: AuthoredAugmentationScope,
}

/// The cross-boundary CONTENT-FREE slot identity — a CLOSED sum over the authored
/// parse-backed source kinds only: (a) decl-body (top-level or
/// augmentation-scoped) + (b) authored macro/field payloads. The keyable
/// inverse of a session `HotTypeRef`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub enum AuthoredBodyLocator {
    /// (a) a top-level declaration body (or a named sub-slot of it).
    DeclBody(TypeBodySlot),
    /// (a) an ambient augmentation-scoped contribution body.
    AugmentationBody(AugmentationBodyLocator),
    /// (b) an authored macro / field payload body.
    MacroPayload(MacroPayloadLocator),
}

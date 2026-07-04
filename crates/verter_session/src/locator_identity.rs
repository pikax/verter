//! Session-side key identities for locator-backed body lowering.
//!
//! [`LocatorLoweringKey`] is the session warm-memo key for a lowered authored
//! body: exactly the decl slot identity + the content-free
//! [`AuthoredBodyLocator`] + the live `parse_env_hash` (`P`) and
//! `resolve_env_hash` (`R`) dimensions. The `type_env_hash` / `lib_env_hash` /
//! `project_identity` dimensions (`T` / `L` / `J`) participate TRANSITIVELY
//! through the slot's own typed env tail ([`SlotEnvIdentity`]) — never as
//! standalone key fields or constructor parameters — so a mixed-env key
//! ("slot from env A, dims from env B") is UNCONSTRUCTIBLE BY SHAPE.
//! `LowerLocator` is strictly unsubstituted and carries no caller projection
//! axis: substituted demands route through `Instantiate { args }`, which owns
//! substitution plus all demand-sensitive reduction. [`SessionDemandIdentity`]
//! is the session-only replay identity for a graph-raised adapter payload; it
//! is NEVER stored in a lower crate and does NOT lower via
//! [`LocatorLoweringKey`].
//!
//! # Fail-closed sealed construction
//!
//! [`LocatorLoweringKey`]'s fields are PRIVATE; the sole constructor
//! [`LocatorLoweringKey::new_unsubstituted`] performs the slot/locator
//! anchor-match gate BEFORE the key exists and rejects a mismatch with the
//! typed [`LocatorKeyError`] — a malformed identity is unconstructible, never
//! a silent lower under the wrong slot and never deferred to a dispatch-time
//! `ReturnOnly`. Outside readers use the accessors; the exhaustive-destructure
//! R6 witness stays inside this defining module.
//!
//! # R6 type-level key witnesses
//!
//! R6 forbids content/whole hashes, `SemanticNodeId`, `HotTypeRef`, and versioned
//! `DeclIdentity` in a content-free query-identity key. Two complementary
//! compile-time witnesses enforce this:
//!
//! - [`R6KeyDimension`] — a SEALED trait over the allowed env dimension types.
//!   A forbidden dimension has no impl and cannot be given one (the private
//!   supertrait closes the set), so it can never occupy a standalone dimension
//!   position. [`assert_r6_key_dimension`] drives the compile-fail fixture.
//! - [`R6KeySafe`] — a SEALED trait over every type that may occupy ANY key
//!   position, built recursively from allowed dimensions + content-free
//!   structural components (`Arc<str>`, small ordinals, closed enums, and the
//!   content-free locator/slot composites). Each composite's [`R6KeySafe`]
//!   membership is backed by an EXHAUSTIVE-destructure witness (no `..`, no `_`
//!   composite field), so a NEW field or arm fails compilation until it is
//!   classified as key-safe. A forbidden dimension nested inside a composite key
//!   field (e.g. `Option<SemanticNodeId>`) also fails, because the container
//!   forwards the [`R6KeySafe`] bound.
//!
//! [`ResolvedDeclSlotIdentity`]'s env tail is the typed [`SlotEnvIdentity`]
//! composite (sealed [`TypeEnvHash`] / [`LibEnvHash`] / [`ProjectIdentityDim`]
//! dimensions), proven through its own exhaustive-destructure witness like
//! every other composite — this is how `T` / `L` / `J` prove through the slot
//! on the [`LocatorLoweringKey`] witness.
//!
//! The lowering consumers that read these identities are wired separately.

#![allow(dead_code)]

use std::sync::Arc;

use crate::semantic_query::{HashValue, ResolvedDeclSlotIdentity, SemanticSymbolSpace};
use verter_type_expr::locators::{
    AugmentationBodyLocator, AuthoredAnchor, AuthoredAugmentationScope, AuthoredBodyLocator,
    LocatorSymbolSpace, MacroPayloadLocator, MacroPayloadPosition, SymbolBodyLocator,
    TypeArgLocator, TypeBodyPathStep, TypeBodySlot,
};

mod sealed {
    /// Private seal: only this module can name it, so `R6KeyDimension` cannot be
    /// implemented downstream.
    pub trait Sealed {}
}

/// A dimension R6 permits in a content-free session query-identity key. SEALED:
/// implemented ONLY for the allowed env-dimension newtypes below. Forbidden
/// dimensions (content/whole hash, `SemanticNodeId`, `HotTypeRef`, versioned
/// `DeclIdentity`) are not members and have no impl, so they cannot occupy a key
/// dimension position; the private supertrait makes the set closed.
pub trait R6KeyDimension: sealed::Sealed {}

/// Compile-time witness anchor: instantiable ONLY for a sealed key dimension.
/// The `LocatorLoweringKey` witness and the R6 compile-fail fixture both drive
/// this — a forbidden dimension fails the `R6KeyDimension` bound here.
pub fn assert_r6_key_dimension<T: R6KeyDimension>() {}

/// The parse-env-hash dimension. Inner hash is PRIVATE — constructed only
/// in-crate via the `pub(crate)` [`ParseEnvHash::from_env_hash`], so this is a
/// DISTINCT nominal type from a content/whole hash (content-free BY TYPE): a
/// content hash value cannot occupy an env-dimension position without an
/// explicit in-crate wrap. This is NOT an absolute-impossibility claim —
/// `HashValue` cannot itself type-distinguish env bytes from content bytes at
/// the constructor boundary; the TYPE distinction, not the bytes, is the guard
/// (the byte-provenance carried obligation, design §9.1).
///
/// Also orders (`Ord`): the resolver-core fact rail sorts
/// [`crate::resolver_core::FactVersionRef`] observations canonically,
/// and the `FileSourceEnv` arm carries this dimension. Ordering is the
/// derived byte order of the private inner hash — opaque, stable, and
/// exposes no constructor surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ParseEnvHash(HashValue);
impl ParseEnvHash {
    /// Wrap the LIVE parse-env-hash dimension (never a file content/whole hash).
    #[must_use]
    pub(crate) const fn from_env_hash(hash: HashValue) -> Self {
        Self(hash)
    }
}
impl sealed::Sealed for ParseEnvHash {}
impl R6KeyDimension for ParseEnvHash {}

/// The resolve-env-hash dimension. Inner hash PRIVATE — see [`ParseEnvHash`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResolveEnvHash(HashValue);
impl ResolveEnvHash {
    /// Wrap the LIVE resolve-env-hash dimension (never a content/whole hash).
    #[must_use]
    pub(crate) const fn from_env_hash(hash: HashValue) -> Self {
        Self(hash)
    }
}
impl sealed::Sealed for ResolveEnvHash {}
impl R6KeyDimension for ResolveEnvHash {}

/// The type-env-hash dimension. Inner hash PRIVATE — see [`ParseEnvHash`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TypeEnvHash(HashValue);
impl TypeEnvHash {
    /// Wrap the LIVE type-env-hash dimension (never a content/whole hash).
    #[must_use]
    pub(crate) const fn from_env_hash(hash: HashValue) -> Self {
        Self(hash)
    }
}
impl sealed::Sealed for TypeEnvHash {}
impl R6KeyDimension for TypeEnvHash {}

/// The lib-env-hash dimension. Inner hash PRIVATE — see [`ParseEnvHash`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LibEnvHash(HashValue);
impl LibEnvHash {
    /// Wrap the LIVE lib-env-hash dimension (never a content/whole hash).
    #[must_use]
    pub(crate) const fn from_env_hash(hash: HashValue) -> Self {
        Self(hash)
    }
}
impl sealed::Sealed for LibEnvHash {}
impl R6KeyDimension for LibEnvHash {}

/// The slot project-identity dimension (`J`): a sealed newtype over the
/// folded `u32` project identity carried by [`ResolvedDeclSlotIdentity`]
/// (workspace + tsconfig + provider-root discriminator, folded via
/// `ProjectIdentity::fold_u32`). Inner value PRIVATE — constructed only
/// in-crate via [`ProjectIdentityDim::from_project_identity`], so a raw
/// ordinal/content value cannot occupy the slot's project position without an
/// explicit in-crate wrap. Deliberately NOT the 16-byte
/// `file_artifact_store::ProjectIdentity` (whose public `Hash16` inner still
/// admits arbitrary bytes by construction): the slot's project dimension is
/// the u32-derived one, and no full `ProjectIdentity` shadow field exists on
/// the slot or on [`LocatorLoweringKey`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProjectIdentityDim(u32);
impl ProjectIdentityDim {
    /// Wrap the folded `u32` project-identity dimension.
    #[must_use]
    pub(crate) const fn from_project_identity(project_identity: u32) -> Self {
        Self(project_identity)
    }
}
impl sealed::Sealed for ProjectIdentityDim {}
impl R6KeyDimension for ProjectIdentityDim {}

/// The typed env tail of [`ResolvedDeclSlotIdentity`]: the slot-intrinsic
/// `type_env_hash` (`T`) / `lib_env_hash` (`L`) / `project_identity` (`J`)
/// dimensions as ONE sealed composite. This is what makes `T` / `L` / `J`
/// SLOT-CARRIED key identity: a key that embeds the slot proves those
/// dimensions through this composite's exhaustive-destructure witness and
/// never carries them as standalone fields, so a mixed-env key cannot be
/// formed by shape. Fields are private; construction is in-crate only
/// ([`SlotEnvIdentity::new`] / [`SlotEnvIdentity::from_raw`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SlotEnvIdentity {
    /// Type-env dimension (`T`).
    type_env: TypeEnvHash,
    /// Lib-env dimension (`L`).
    lib_env: LibEnvHash,
    /// Project-identity dimension (`J`).
    project: ProjectIdentityDim,
}

impl SlotEnvIdentity {
    /// Compose the typed slot env tail from its sealed dimensions.
    #[must_use]
    pub(crate) const fn new(
        type_env: TypeEnvHash,
        lib_env: LibEnvHash,
        project: ProjectIdentityDim,
    ) -> Self {
        Self {
            type_env,
            lib_env,
            project,
        }
    }

    /// Wrap the raw env values read from the live host env (never file
    /// content/whole hashes) into the sealed slot env tail.
    #[must_use]
    pub(crate) const fn from_raw(
        project_identity: u32,
        type_env_hash: HashValue,
        lib_env_hash: HashValue,
    ) -> Self {
        Self::new(
            TypeEnvHash::from_env_hash(type_env_hash),
            LibEnvHash::from_env_hash(lib_env_hash),
            ProjectIdentityDim::from_project_identity(project_identity),
        )
    }
}

// ===========================================================================
// R6KeySafe — the recursive "safe to occupy ANY key position" witness.
// ===========================================================================

mod key_safe_sealed {
    /// Private seal for [`super::R6KeySafe`]: only this module can name it, so a
    /// downstream type can never join the key-safe set.
    pub trait Sealed {}
}

/// A type that is safe to occupy ANY position in an R6 content-free key: built
/// recursively from allowed sealed dimensions and content-free structural
/// components, transitively free of any forbidden dimension (content/whole hash,
/// `SemanticNodeId`, `HotTypeRef`, versioned `DeclIdentity`). SEALED. Each
/// composite impl is backed by an exhaustive-destructure witness below.
pub trait R6KeySafe: key_safe_sealed::Sealed {}

/// Compile-time witness anchor: instantiable ONLY for a key-safe type. Drives the
/// nested-forbidden-dimension compile-fail fixture (e.g.
/// `assert_r6_key_safe::<Option<SemanticNodeId>>()` fails, because a forbidden
/// dimension nested in a container is still not key-safe).
pub fn assert_r6_key_safe<T: R6KeySafe>() {}

/// Internal per-field witness call used by the exhaustive-destructure
/// witnesses — here and by the sealed-key-context witnesses that live with
/// their private-field definitions in other modules (e.g. the
/// `InstantiateContext` witness in `semantic_query`).
pub(crate) fn key_safe<T: R6KeySafe>(_: &T) {}

/// Stamp the sealed [`R6KeySafe`] witness for a sealed env/substitution DIMENSION
/// leaf. A dimension has NO key-bearing fields to destructure, so (unlike a
/// composite) it carries no `w_*` witness. BOUND to [`R6KeyDimension`]: the
/// `const _` anchor fails to compile unless `$ty` is a member of the sealed
/// dimension set, so a COMPOSITE (which is not an `R6KeyDimension`) can NEVER be
/// leaf-stamped through this macro — it must go through [`impl_r6_key_safe`] with
/// its exhaustive-destructure witness. This is the structural close for the
/// "composite routed through the leaf macro to skip its witness" bypass. The
/// language-primitive leaves (`bool` / `u32` / `String` / `str`) are NOT
/// dimensions and are instead hand-written below — so this macro's ONLY members
/// are sealed dimensions.
macro_rules! impl_r6_key_safe_leaf {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl key_safe_sealed::Sealed for $ty {}
            impl R6KeySafe for $ty {}
            // A composite (not a sealed `R6KeyDimension`) cannot be leaf-stamped:
            // this assertion fails to compile unless `$ty: R6KeyDimension`.
            const _: fn() = || {
                fn assert_dim<T: R6KeyDimension>() {}
                assert_dim::<$ty>();
            };
        )+
    };
}

/// Stamp the sealed [`R6KeySafe`] witness for a content-free COMPOSITE, BOUND to
/// its exhaustive-destructure witness fn. The `const _: fn(&$ty) = $witness;`
/// anchor guarantees a signature-matching `w_*` witness fn EXISTS — it must be
/// declared AND have signature `fn(&$ty)`, so a stamp without a matching witness
/// fails to compile. The anchor does NOT by itself prove the witness body is
/// exhaustive; that is enforced SEPARATELY by each `w_*`'s no-`..`,
/// no-`_`-composite-field destructure, which forces every new field/arm to be
/// classified key-safe. Together they mean a new composite cannot be declared
/// key-safe without an exhaustive field check.
macro_rules! impl_r6_key_safe {
    ($($ty:ty => $witness:path),+ $(,)?) => {
        $(
            impl key_safe_sealed::Sealed for $ty {}
            impl R6KeySafe for $ty {}
            // Bind the stamp to its exhaustive witness: an `impl_r6_key_safe!`
            // stamp WITHOUT its `w_*` destructure witness fails to compile here.
            const _: fn(&$ty) = $witness;
        )+
    };
}

// Foundational content-free PRIMITIVE leaves — small ordinals/flags and
// owned/borrowed strings. Hand-written (NOT macro-stamped) so there is no
// unguarded leaf-stamping macro a composite could be routed through: a composite
// is never one of these fixed language primitives, and the dimension leaf macro
// below rejects any non-dimension. Deliberately NOT `[u8; 16]` (a raw hash) — a
// raw hash is never key-safe on its own; only the sealed env-dimension newtypes
// are.
impl key_safe_sealed::Sealed for bool {}
impl R6KeySafe for bool {}
impl key_safe_sealed::Sealed for u32 {}
impl R6KeySafe for u32 {}
impl key_safe_sealed::Sealed for String {}
impl R6KeySafe for String {}
impl key_safe_sealed::Sealed for str {}
impl R6KeySafe for str {}

// Container forwarding: a container is key-safe iff its element is. This is what
// makes a forbidden dimension nested in a container (e.g. `Option<SemanticNodeId>`)
// fail the witness.
impl<T: R6KeySafe + ?Sized> key_safe_sealed::Sealed for Arc<T> {}
impl<T: R6KeySafe + ?Sized> R6KeySafe for Arc<T> {}
impl<T: R6KeySafe> key_safe_sealed::Sealed for [T] {}
impl<T: R6KeySafe> R6KeySafe for [T] {}
impl<T: R6KeySafe> key_safe_sealed::Sealed for Option<T> {}
impl<T: R6KeySafe> R6KeySafe for Option<T> {}
impl<T: R6KeySafe> key_safe_sealed::Sealed for Vec<T> {}
impl<T: R6KeySafe> R6KeySafe for Vec<T> {}

// The sealed env dimensions are key-safe LEAVES (no fields).
impl_r6_key_safe_leaf!(
    ParseEnvHash,
    ResolveEnvHash,
    TypeEnvHash,
    LibEnvHash,
    ProjectIdentityDim,
);

// The content-free locator composites + slot identity (with its typed env
// tail) are key-safe; each stamp is BOUND to its exhaustive-destructure
// witness below (a stamp without its `w_*` witness fails to compile).
impl_r6_key_safe!(
    LocatorSymbolSpace => w_locator_symbol_space,
    AuthoredAnchor => w_authored_anchor,
    TypeBodyPathStep => w_type_body_path_step,
    TypeBodySlot => w_type_body_slot,
    SymbolBodyLocator => w_symbol_body_locator,
    TypeArgLocator => w_type_arg_locator,
    MacroPayloadPosition => w_macro_payload_position,
    MacroPayloadLocator => w_macro_payload_locator,
    AuthoredAugmentationScope => w_authored_augmentation_scope,
    AugmentationBodyLocator => w_augmentation_body_locator,
    AuthoredBodyLocator => w_authored_body_locator,
    SemanticSymbolSpace => w_semantic_symbol_space,
    SlotEnvIdentity => w_slot_env_identity,
    ResolvedDeclSlotIdentity => w_resolved_decl_slot_identity,
);

// ---------------------------------------------------------------------------
// Exhaustive-destructure witnesses (no `..`, no `_` composite field). A new
// field/arm on any of these fails compilation until it is classified key-safe.
// The module `#![allow(dead_code)]` keeps these type-checked-but-uncalled;
// rustc type-checks every fn body regardless of use, so the witness is enforced.
// ---------------------------------------------------------------------------

fn w_authored_anchor(a: &AuthoredAnchor) {
    let AuthoredAnchor {
        canonical_id,
        symbol,
        space,
    } = a;
    key_safe(canonical_id);
    key_safe(symbol);
    key_safe(space);
}

fn w_locator_symbol_space(s: &LocatorSymbolSpace) {
    match s {
        LocatorSymbolSpace::Type | LocatorSymbolSpace::Value | LocatorSymbolSpace::Namespace => {}
    }
}

fn w_type_body_path_step(step: &TypeBodyPathStep) {
    match step {
        TypeBodyPathStep::MergedContributor { ordinal }
        | TypeBodyPathStep::IntersectionArm { ordinal }
        | TypeBodyPathStep::Member { ordinal } => key_safe(ordinal),
        TypeBodyPathStep::MemberValue => {}
    }
}

fn w_type_body_slot(s: &TypeBodySlot) {
    let TypeBodySlot { anchor, path } = s;
    key_safe(anchor);
    key_safe(path);
}

fn w_symbol_body_locator(s: &SymbolBodyLocator) {
    let SymbolBodyLocator { anchor } = s;
    key_safe(anchor);
}

fn w_type_arg_locator(l: &TypeArgLocator) {
    let TypeArgLocator {
        anchor,
        path,
        arg_index,
    } = l;
    key_safe(anchor);
    key_safe(path);
    key_safe(arg_index);
}

fn w_macro_payload_position(p: &MacroPayloadPosition) {
    match p {
        MacroPayloadPosition::TypeArgument | MacroPayloadPosition::ObjectArgument => {}
        MacroPayloadPosition::Field { field_index } => key_safe(field_index),
    }
}

fn w_macro_payload_locator(l: &MacroPayloadLocator) {
    let MacroPayloadLocator {
        anchor,
        macro_index,
        payload,
    } = l;
    key_safe(anchor);
    key_safe(macro_index);
    key_safe(payload);
}

fn w_authored_augmentation_scope(s: &AuthoredAugmentationScope) {
    match s {
        AuthoredAugmentationScope::Global => {}
        AuthoredAugmentationScope::Module { specifier } => key_safe(specifier),
    }
}

fn w_augmentation_body_locator(l: &AugmentationBodyLocator) {
    let AugmentationBodyLocator { anchor, scope } = l;
    key_safe(anchor);
    key_safe(scope);
}

fn w_authored_body_locator(l: &AuthoredBodyLocator) {
    match l {
        AuthoredBodyLocator::DeclBody(slot) => key_safe(slot),
        AuthoredBodyLocator::AugmentationBody(aug) => key_safe(aug),
        AuthoredBodyLocator::MacroPayload(payload) => key_safe(payload),
    }
}

fn w_semantic_symbol_space(s: &SemanticSymbolSpace) {
    match s {
        SemanticSymbolSpace::Type | SemanticSymbolSpace::Value | SemanticSymbolSpace::Namespace => {
        }
    }
}

fn w_slot_env_identity(e: &SlotEnvIdentity) {
    let SlotEnvIdentity {
        type_env,
        lib_env,
        project,
    } = e;
    key_safe(type_env);
    key_safe(lib_env);
    key_safe(project);
}

fn w_resolved_decl_slot_identity(s: &ResolvedDeclSlotIdentity) {
    let ResolvedDeclSlotIdentity {
        defining_canonical,
        merged_symbol_name,
        symbol_space,
        env,
    } = s;
    key_safe(defining_canonical);
    key_safe(merged_symbol_name);
    key_safe(symbol_space);
    key_safe(env);
}

/// Typed rejection of a malformed [`LocatorLoweringKey`] identity: the
/// locator's anchor does not name the slot's declaration. Each variant is one
/// mismatch axis of the anchor-match gate. A mismatch means the wrong
/// family/lane must never come into existence — construction fails; nothing
/// lowers under the wrong slot and nothing defers to a dispatch-time
/// `ReturnOnly`.
// The shared `Mismatch` postfix is the semantic content of every variant (the
// gate has exactly three mismatch axes), not naming noise.
#[allow(clippy::enum_variant_names)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocatorKeyError {
    /// `locator.anchor.canonical_id` ≠ `slot.defining_canonical`.
    CanonicalMismatch {
        /// The slot's defining canonical.
        slot_canonical: Arc<str>,
        /// The locator anchor's canonical.
        locator_canonical: Arc<str>,
    },
    /// `locator.anchor.symbol` ≠ `slot.merged_symbol_name`.
    SymbolMismatch {
        /// The slot's merged symbol name.
        slot_symbol: Arc<str>,
        /// The locator anchor's symbol.
        locator_symbol: Arc<str>,
    },
    /// `locator.anchor.space` does not map onto `slot.symbol_space`.
    SpaceMismatch {
        /// The slot's symbol space.
        slot_space: SemanticSymbolSpace,
        /// The locator anchor's symbol space.
        locator_space: LocatorSymbolSpace,
    },
}

/// Map a locator anchor space onto the session symbol space it names.
/// Exhaustive: both are closed three-arm spaces. Shared by the
/// [`LocatorLoweringKey::new_unsubstituted`] anchor-match gate and the
/// `lower_locator` provider's slot derivation, so the two cannot drift.
pub(crate) const fn semantic_space_for_locator_space(
    space: LocatorSymbolSpace,
) -> SemanticSymbolSpace {
    match space {
        LocatorSymbolSpace::Type => SemanticSymbolSpace::Type,
        LocatorSymbolSpace::Value => SemanticSymbolSpace::Value,
        LocatorSymbolSpace::Namespace => SemanticSymbolSpace::Namespace,
    }
}

/// The session warm-memo key for a lowered authored body: exactly
/// `slot + locator + P + R`.
///
/// Content-free (R6): NO content/whole hash, NO `FileWholeHash`, NO
/// `SemanticNodeId`, NO `HotTypeRef`, NO versioned `DeclIdentity`. The live
/// whole-hash is re-sourced at value-compute time and recorded in the caller's
/// read-set, never carried here. `T` / `L` / `J` are SLOT-CARRIED (the slot's
/// [`SlotEnvIdentity`] env tail is the sole carrier) — there are no standalone
/// type-env / lib-env / project fields, so a mixed-env key cannot be formed by
/// shape. The key is strictly UNSUBSTITUTED and carries NO caller projection
/// axis: the body lowers under the fixed locator-shape context, and
/// substituted / demand-sensitive reduction lives on `Instantiate { args }`.
///
/// Fields are PRIVATE (fail-closed sealed construction): the sole constructor
/// is [`Self::new_unsubstituted`], which gates on the slot/locator anchor
/// match before creation.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LocatorLoweringKey {
    /// The env-bearing content-free decl slot identity (carries `T`/`L`/`J`).
    slot: ResolvedDeclSlotIdentity,
    /// The authored body being lowered.
    locator: AuthoredBodyLocator,
    /// Live parse-env dimension (`P`).
    parse_env_hash: ParseEnvHash,
    /// Live resolve-env dimension (`R`).
    resolve_env_hash: ResolveEnvHash,
}

impl LocatorLoweringKey {
    /// The SOLE constructor: build the strictly-unsubstituted lowering key
    /// after the slot/locator anchor-match gate. REJECTS with a typed
    /// [`LocatorKeyError`] unless the locator anchor names the slot's
    /// declaration exactly (`anchor.canonical_id == slot.defining_canonical`,
    /// `anchor.symbol == slot.merged_symbol_name`, and `anchor.space` maps to
    /// `slot.symbol_space`). There are NO standalone `T`/`L`/`J` parameters to
    /// cross-check — env coherence is by construction (slot-carried).
    pub(crate) fn new_unsubstituted(
        slot: ResolvedDeclSlotIdentity,
        locator: AuthoredBodyLocator,
        parse_env_hash: ParseEnvHash,
        resolve_env_hash: ResolveEnvHash,
    ) -> Result<Self, LocatorKeyError> {
        let anchor = match &locator {
            AuthoredBodyLocator::DeclBody(slot_locator) => &slot_locator.anchor,
            AuthoredBodyLocator::AugmentationBody(aug) => &aug.anchor,
            AuthoredBodyLocator::MacroPayload(payload) => &payload.anchor,
        };
        if anchor.canonical_id != slot.defining_canonical {
            return Err(LocatorKeyError::CanonicalMismatch {
                slot_canonical: Arc::clone(&slot.defining_canonical),
                locator_canonical: Arc::clone(&anchor.canonical_id),
            });
        }
        if anchor.symbol != slot.merged_symbol_name {
            return Err(LocatorKeyError::SymbolMismatch {
                slot_symbol: Arc::clone(&slot.merged_symbol_name),
                locator_symbol: Arc::clone(&anchor.symbol),
            });
        }
        if semantic_space_for_locator_space(anchor.space) != slot.symbol_space {
            return Err(LocatorKeyError::SpaceMismatch {
                slot_space: slot.symbol_space,
                locator_space: anchor.space,
            });
        }
        Ok(Self {
            slot,
            locator,
            parse_env_hash,
            resolve_env_hash,
        })
    }

    /// The env-bearing content-free decl slot identity.
    #[must_use]
    pub(crate) fn slot(&self) -> &ResolvedDeclSlotIdentity {
        &self.slot
    }

    /// The authored body being lowered.
    #[must_use]
    pub(crate) fn locator(&self) -> &AuthoredBodyLocator {
        &self.locator
    }

    /// Live parse-env dimension (`P`).
    #[must_use]
    pub(crate) fn parse_env_hash(&self) -> ParseEnvHash {
        self.parse_env_hash
    }

    /// Live resolve-env dimension (`R`).
    #[must_use]
    pub(crate) fn resolve_env_hash(&self) -> ResolveEnvHash {
        self.resolve_env_hash
    }
}

// Bound to its whole-key exhaustive-destructure witness below.
impl_r6_key_safe!(LocatorLoweringKey => w_locator_lowering_key);

/// R6 type-level key witness for the WHOLE key. The EXHAUSTIVE destructure (no
/// `..`, no `_` composite field) calls `key_safe` on EVERY field — `slot`,
/// `locator`, `P`, `R` — so a forbidden dimension (content/whole hash,
/// `SemanticNodeId`, `HotTypeRef`, versioned `DeclIdentity`) cannot occupy ANY
/// position, including nested inside a composite field, and a NEW key field
/// fails compilation until it is classified `R6KeySafe`. `T` / `L` / `J` prove
/// through the slot's witness (its [`SlotEnvIdentity`] env tail). The witness
/// destructures inside this defining module, so private fields stay sealed to
/// outside readers.
fn w_locator_lowering_key(k: &LocatorLoweringKey) {
    let LocatorLoweringKey {
        slot,
        locator,
        parse_env_hash,
        resolve_env_hash,
    } = k;
    key_safe(slot);
    key_safe(locator);
    key_safe(parse_env_hash);
    key_safe(resolve_env_hash);
}

/// The OWNER anchor of a session demand: the component/surface canonical + a
/// macro/surface anchor.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SessionDemandOwner {
    /// The component / surface canonical id.
    pub canonical: Arc<str>,
    /// The macro / surface anchor within the owner.
    pub surface_anchor: Arc<str>,
}

/// The route discriminant of a session demand — the single replayable graph
/// route class.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SessionDemandRoute {
    /// A macro hot-mirror route.
    MacroHotMirror,
    /// A `ProjectPath` selector route.
    ProjectPath,
}

/// A session-only demand identity for a payload raised from the graph by a
/// registered adapter's session-side normalizer (classified by PRODUCER CLASS,
/// not framework name — covers the "single replayable graph route" class).
///
/// Content-free / env-free. It is NEVER stored in `verter_type_expr` or
/// `verter_semantic`, and does NOT lower via [`LocatorLoweringKey`]; its deref
/// REPLAYS the existing session graph route, memoized in the existing session
/// graph memo under env + read-set validation.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SessionDemandIdentity {
    /// The owner anchor.
    pub owner: SessionDemandOwner,
    /// The member / role path within the owner surface.
    pub member_role_path: Arc<[String]>,
    /// The route discriminant.
    pub route: SessionDemandRoute,
}

// `SessionDemandIdentity` is content-free / env-free / session-only. It does NOT
// lower via `LocatorLoweringKey`, but it IS still a keyable identity, so it gets
// the same exhaustive-destructure R6-key-safe witnesses (each stamp bound to its
// witness).
impl_r6_key_safe!(
    SessionDemandOwner => w_session_demand_owner,
    SessionDemandRoute => w_session_demand_route,
    SessionDemandIdentity => w_session_demand_identity,
);

fn w_session_demand_owner(o: &SessionDemandOwner) {
    let SessionDemandOwner {
        canonical,
        surface_anchor,
    } = o;
    key_safe(canonical);
    key_safe(surface_anchor);
}

fn w_session_demand_route(r: &SessionDemandRoute) {
    match r {
        SessionDemandRoute::MacroHotMirror | SessionDemandRoute::ProjectPath => {}
    }
}

fn w_session_demand_identity(d: &SessionDemandIdentity) {
    let SessionDemandIdentity {
        owner,
        member_role_path,
        route,
    } = d;
    key_safe(owner);
    key_safe(member_role_path);
    key_safe(route);
}

#[cfg(test)]
mod tests {
    use super::*;

    // Anchor the pub witness helpers to an in-crate call site so they are
    // exercised, and pin the sealed R6 sets (a forbidden dimension has no impl —
    // proven negatively by the trybuild fixtures).
    #[test]
    fn r6_key_dimensions_and_key_safe_witnesses_hold() {
        assert_r6_key_dimension::<ParseEnvHash>();
        assert_r6_key_dimension::<ResolveEnvHash>();
        assert_r6_key_dimension::<TypeEnvHash>();
        assert_r6_key_dimension::<LibEnvHash>();
        assert_r6_key_dimension::<ProjectIdentityDim>();

        // The whole key + its composites + a forbidden-dim-free container are
        // key-safe. (A forbidden dimension — standalone or nested — fails these
        // bounds; that negative is proven by the trybuild compile-fail fixtures.)
        assert_r6_key_safe::<LocatorLoweringKey>();
        assert_r6_key_safe::<AuthoredBodyLocator>();
        assert_r6_key_safe::<ResolvedDeclSlotIdentity>();
        assert_r6_key_safe::<SlotEnvIdentity>();
        assert_r6_key_safe::<SessionDemandIdentity>();
        assert_r6_key_safe::<Option<ParseEnvHash>>();
        assert_r6_key_safe::<Arc<[TypeArgLocator]>>();
    }

    /// A coherent slot/locator pair for the anchor-match gate fixtures: the
    /// locator's anchor names exactly the slot's defining canonical, merged
    /// symbol name, and (mapped) symbol space.
    fn matching_slot_and_locator() -> (ResolvedDeclSlotIdentity, AuthoredBodyLocator) {
        let slot = ResolvedDeclSlotIdentity::type_slot(
            Arc::from("/env/a.ts"),
            Arc::from("Foo"),
            7,
            [3u8; 16],
            [4u8; 16],
        );
        let locator = AuthoredBodyLocator::DeclBody(TypeBodySlot {
            anchor: AuthoredAnchor {
                canonical_id: Arc::from("/env/a.ts"),
                symbol: Arc::from("Foo"),
                space: LocatorSymbolSpace::Type,
            },
            path: Arc::from(Vec::<TypeBodyPathStep>::new().into_boxed_slice()),
        });
        (slot, locator)
    }

    /// The anchor-match gate ACCEPTS a coherent slot/locator pair, and the
    /// accessors echo exactly the four constructor inputs.
    #[test]
    fn locator_key_new_unsubstituted_accepts_matching_anchor() {
        let (slot, locator) = matching_slot_and_locator();
        let parse = ParseEnvHash::from_env_hash([1u8; 16]);
        let resolve = ResolveEnvHash::from_env_hash([2u8; 16]);
        let key =
            LocatorLoweringKey::new_unsubstituted(slot.clone(), locator.clone(), parse, resolve)
                .expect("a coherent slot/locator anchor pair must construct");
        assert_eq!(key.slot(), &slot);
        assert_eq!(key.locator(), &locator);
        assert_eq!(key.parse_env_hash(), parse);
        assert_eq!(key.resolve_env_hash(), resolve);
    }

    /// A locator anchored in a DIFFERENT canonical than the slot's defining
    /// canonical is a malformed identity: construction REJECTS with the typed
    /// canonical-mismatch error — the wrong-slot key never exists.
    #[test]
    fn locator_key_rejects_anchor_canonical_mismatch() {
        let (slot, _) = matching_slot_and_locator();
        let locator = AuthoredBodyLocator::DeclBody(TypeBodySlot {
            anchor: AuthoredAnchor {
                canonical_id: Arc::from("/env/other.ts"),
                symbol: Arc::from("Foo"),
                space: LocatorSymbolSpace::Type,
            },
            path: Arc::from(Vec::<TypeBodyPathStep>::new().into_boxed_slice()),
        });
        let err = LocatorLoweringKey::new_unsubstituted(
            slot,
            locator,
            ParseEnvHash::from_env_hash([1u8; 16]),
            ResolveEnvHash::from_env_hash([2u8; 16]),
        )
        .expect_err("a canonical-mismatched anchor must be rejected");
        assert!(
            matches!(err, LocatorKeyError::CanonicalMismatch { .. }),
            "expected CanonicalMismatch, got {err:?}"
        );
    }

    /// A locator anchored on a DIFFERENT symbol than the slot's merged symbol
    /// name REJECTS with the typed symbol-mismatch error.
    #[test]
    fn locator_key_rejects_anchor_symbol_mismatch() {
        let (slot, _) = matching_slot_and_locator();
        let locator = AuthoredBodyLocator::DeclBody(TypeBodySlot {
            anchor: AuthoredAnchor {
                canonical_id: Arc::from("/env/a.ts"),
                symbol: Arc::from("Bar"),
                space: LocatorSymbolSpace::Type,
            },
            path: Arc::from(Vec::<TypeBodyPathStep>::new().into_boxed_slice()),
        });
        let err = LocatorLoweringKey::new_unsubstituted(
            slot,
            locator,
            ParseEnvHash::from_env_hash([1u8; 16]),
            ResolveEnvHash::from_env_hash([2u8; 16]),
        )
        .expect_err("a symbol-mismatched anchor must be rejected");
        assert!(
            matches!(err, LocatorKeyError::SymbolMismatch { .. }),
            "expected SymbolMismatch, got {err:?}"
        );
    }

    /// A locator whose anchor SPACE does not map onto the slot's symbol space
    /// REJECTS with the typed space-mismatch error.
    #[test]
    fn locator_key_rejects_anchor_space_mismatch() {
        let (slot, _) = matching_slot_and_locator();
        let locator = AuthoredBodyLocator::DeclBody(TypeBodySlot {
            anchor: AuthoredAnchor {
                canonical_id: Arc::from("/env/a.ts"),
                symbol: Arc::from("Foo"),
                space: LocatorSymbolSpace::Value,
            },
            path: Arc::from(Vec::<TypeBodyPathStep>::new().into_boxed_slice()),
        });
        let err = LocatorLoweringKey::new_unsubstituted(
            slot,
            locator,
            ParseEnvHash::from_env_hash([1u8; 16]),
            ResolveEnvHash::from_env_hash([2u8; 16]),
        )
        .expect_err("a space-mismatched anchor must be rejected");
        assert!(
            matches!(err, LocatorKeyError::SpaceMismatch { .. }),
            "expected SpaceMismatch, got {err:?}"
        );
    }

    /// Mixed-env keys are unrepresentable BY SHAPE: the sole constructor takes
    /// exactly `(slot, locator, P, R)` — there are NO standalone
    /// type-env / lib-env / project-identity / substitution / projection
    /// parameters to mix, so "slot from env A, dims from env B" cannot be
    /// formed. This fn-pointer coercion pins that signature at compile time.
    #[test]
    fn locator_key_constructor_carries_no_standalone_env_or_substitution_axis() {
        let _pinned: fn(
            ResolvedDeclSlotIdentity,
            AuthoredBodyLocator,
            ParseEnvHash,
            ResolveEnvHash,
        ) -> Result<LocatorLoweringKey, LocatorKeyError> = LocatorLoweringKey::new_unsubstituted;
    }

    /// Key identity spans all four components: two keys equal iff slot,
    /// locator, `P`, and `R` all agree; each env dim independently separates.
    #[test]
    fn locator_key_identity_distinct_by_parse_and_resolve_env() {
        let (slot, locator) = matching_slot_and_locator();
        let p0 = ParseEnvHash::from_env_hash([1u8; 16]);
        let p1 = ParseEnvHash::from_env_hash([9u8; 16]);
        let r0 = ResolveEnvHash::from_env_hash([2u8; 16]);
        let r1 = ResolveEnvHash::from_env_hash([8u8; 16]);
        let base = LocatorLoweringKey::new_unsubstituted(slot.clone(), locator.clone(), p0, r0)
            .expect("coherent");
        let same = LocatorLoweringKey::new_unsubstituted(slot.clone(), locator.clone(), p0, r0)
            .expect("coherent");
        let parse_differs =
            LocatorLoweringKey::new_unsubstituted(slot.clone(), locator.clone(), p1, r0)
                .expect("coherent");
        let resolve_differs =
            LocatorLoweringKey::new_unsubstituted(slot, locator, p0, r1).expect("coherent");
        assert_eq!(base, same);
        assert_ne!(base, parse_differs);
        assert_ne!(base, resolve_differs);
    }

    /// The typed slot env tail: `SlotEnvIdentity` is R6-key-safe, the sealed
    /// `ProjectIdentityDim` is an allowed dimension, and each of the three env
    /// dimensions independently separates slot identity (T/L/J participate in
    /// a key TRANSITIVELY through the slot — never as standalone fields).
    #[test]
    fn slot_env_identity_types_the_slot_env_tail() {
        assert_r6_key_dimension::<ProjectIdentityDim>();
        assert_r6_key_safe::<SlotEnvIdentity>();
        assert_r6_key_safe::<ResolvedDeclSlotIdentity>();

        let slot = |project: u32, type_env: u8, lib_env: u8| {
            ResolvedDeclSlotIdentity::type_slot(
                Arc::from("/env/a.ts"),
                Arc::from("T"),
                project,
                [type_env; 16],
                [lib_env; 16],
            )
        };
        let base = slot(1, 1, 2);
        assert_eq!(base, slot(1, 1, 2));
        assert_ne!(base, slot(2, 1, 2), "project dim must separate");
        assert_ne!(base, slot(1, 9, 2), "type-env dim must separate");
        assert_ne!(base, slot(1, 1, 9), "lib-env dim must separate");
    }
}

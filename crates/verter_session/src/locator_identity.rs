//! Session-side key identities for locator-backed body lowering.
//!
//! [`LocatorLoweringKey`] is the session warm-memo key for a lowered authored
//! body: the decl slot identity + the content-free [`AuthoredBodyLocator`] + the
//! full live graph-lowering env dimensions + the projection-reduction axis + the
//! substitution axis. [`SessionDemandIdentity`] is the session-only replay
//! identity for a graph-raised adapter payload; it is NEVER stored in a lower
//! crate and does NOT lower via [`LocatorLoweringKey`].
//!
//! # R6 type-level key witnesses
//!
//! R6 forbids content/whole hashes, `SemanticNodeId`, `HotTypeRef`, and versioned
//! `DeclIdentity` in a content-free query-identity key. Two complementary
//! compile-time witnesses enforce this:
//!
//! - [`R6KeyDimension`] — a SEALED trait over the allowed env/substitution
//!   dimension types. A forbidden dimension has no impl and cannot be given one
//!   (the private supertrait closes the set), so it can never occupy a standalone
//!   dimension position. [`assert_r6_key_dimension`] drives the compile-fail
//!   fixture.
//! - [`R6KeySafe`] — a SEALED trait over every type that may occupy ANY key
//!   position, built recursively from allowed dimensions + content-free
//!   structural components (`Arc<str>`, small ordinals, closed enums, and the
//!   content-free locator/projection composites). Each composite's [`R6KeySafe`]
//!   membership is backed by an EXHAUSTIVE-destructure witness (no `..`, no `_`
//!   composite field), so a NEW field or arm fails compilation until it is
//!   classified as key-safe. A forbidden dimension nested inside a composite key
//!   field (e.g. `Option<SemanticNodeId>`) also fails, because the container
//!   forwards the [`R6KeySafe`] bound.
//!
//! The one exception is [`ResolvedDeclSlotIdentity`], a widely-used legacy query
//! identity whose raw `[u8; 16]` / `u32` env fields cannot yet be typed as
//! dimensions (restructuring it requires a consumer flip — out of additive-B1
//! scope; see the carried obligation in
//! `docs/arch/stage10-typeexpr-terminal-removal-design.md` §9). Its witness is
//! still EXHAUSTIVE, classifying those three fields through the NAMED
//! [`LegacyEnvDim`] path rather than an unchecked `_`, so a new field on the slot
//! still fails compilation until classified.
//!
//! These identities are additive substrate: defined and witnessed here, with no
//! production caller yet — the consumers that read them are wired separately.

#![allow(dead_code)]

use std::sync::Arc;

use crate::file_artifact_store::ProjectIdentity;
use crate::semantic_query::{
    HashValue, MemberMergeRole, ProjectionMode, ProjectionReductionContext, ReductionDemand,
    ResolvedDeclSlotIdentity, SemanticSymbolSpace, SubstitutionCanonicalHash,
    SurfaceProvenanceContext,
};
use verter_type_expr::locators::{
    AuthoredAnchor, AuthoredBodyLocator, LocatorSymbolSpace, MacroPayloadLocator,
    MacroPayloadPosition, SymbolBodyLocator, TypeArgLocator, TypeBodyPathStep, TypeBodySlot,
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

/// The parse-env-hash dimension. Inner hash is PRIVATE — construct only via
/// [`ParseEnvHash::from_env_hash`], so a raw content/whole hash cannot be
/// trivially wrapped into an env dimension.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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

// The project-identity dimension REUSES the existing full 16-byte
// `file_artifact_store::ProjectIdentity` (workspace + tsconfig + provider-root
// discriminator) rather than a duplicate local newtype — it is a project/env
// dimension, never a file content hash.
impl sealed::Sealed for ProjectIdentity {}
impl R6KeyDimension for ProjectIdentity {}

// The substitution axis REUSES the existing content-free
// `semantic_query::SubstitutionCanonicalHash` (a canonical hash of the
// substitution MAPPING, never a file content/whole hash — its inner field is
// private, constructed only via `empty()` / the typed test constructor).
// Distinguishing it as its own dimension prevents a `{locator + resolve_env_hash}`
// -only key from aliasing distinct lowered nodes.
impl sealed::Sealed for SubstitutionCanonicalHash {}
impl R6KeyDimension for SubstitutionCanonicalHash {}

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

/// Internal per-field witness call used by the exhaustive-destructure witnesses.
fn key_safe<T: R6KeySafe>(_: &T) {}

/// Stamp the sealed [`R6KeySafe`] witness for a content-free LEAF type — a
/// primitive, an owned/borrowed string, or a sealed env/substitution dimension
/// newtype. A leaf has NO key-bearing fields to destructure, so (unlike a
/// composite) it carries no `w_*` witness. Kept DISTINCT from
/// [`impl_r6_key_safe`] precisely so a COMPOSITE can never be stamped without its
/// exhaustive-destructure witness.
macro_rules! impl_r6_key_safe_leaf {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl key_safe_sealed::Sealed for $ty {}
            impl R6KeySafe for $ty {}
        )+
    };
}

/// Stamp the sealed [`R6KeySafe`] witness for a content-free COMPOSITE, BOUND to
/// its exhaustive-destructure witness fn. The `const _: fn(&$ty) = $witness;`
/// anchor makes a stamp WITHOUT a matching `w_*` witness fail to compile — a new
/// composite can never be declared key-safe without the exhaustive field check
/// that forces every new field/arm to be classified key-safe (the `$witness` fn
/// must exist AND have signature `fn(&$ty)`).
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

// Foundational content-free leaves: small ordinals/flags, owned/borrowed strings.
// Deliberately NOT `[u8; 16]` (a raw hash) — a raw hash is never key-safe on its
// own; only the sealed env-dimension newtypes are.
impl_r6_key_safe_leaf!(bool, u32, String, str);

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

// The sealed env/substitution dimensions are key-safe LEAVES (no fields).
impl_r6_key_safe_leaf!(
    ParseEnvHash,
    ResolveEnvHash,
    TypeEnvHash,
    LibEnvHash,
    ProjectIdentity,
    SubstitutionCanonicalHash,
);

// The content-free locator composites + projection axes + slot identity are
// key-safe; each stamp is BOUND to its exhaustive-destructure witness below (a
// stamp without its `w_*` witness fails to compile).
impl_r6_key_safe!(
    LocatorSymbolSpace => w_locator_symbol_space,
    AuthoredAnchor => w_authored_anchor,
    TypeBodyPathStep => w_type_body_path_step,
    TypeBodySlot => w_type_body_slot,
    SymbolBodyLocator => w_symbol_body_locator,
    TypeArgLocator => w_type_arg_locator,
    MacroPayloadPosition => w_macro_payload_position,
    MacroPayloadLocator => w_macro_payload_locator,
    AuthoredBodyLocator => w_authored_body_locator,
    SemanticSymbolSpace => w_semantic_symbol_space,
    ProjectionMode => w_projection_mode,
    ReductionDemand => w_reduction_demand,
    SurfaceProvenanceContext => w_surface_provenance,
    MemberMergeRole => w_member_merge_role,
    ProjectionReductionContext => w_projection_reduction_context,
    ResolvedDeclSlotIdentity => w_resolved_decl_slot_identity,
);

// ---------------------------------------------------------------------------
// LEGACY env-dimension classifier for `ResolvedDeclSlotIdentity`'s raw fields.
// ---------------------------------------------------------------------------

mod legacy_env_sealed {
    pub trait Sealed {}
}

/// The legacy raw env-hash dimension shapes carried on the pre-migration
/// [`ResolvedDeclSlotIdentity`] slot (a NAMED carried obligation — see design
/// §9). A raw `[u8; 16]` / `u32` cannot distinguish an env hash from a content
/// hash, so it is deliberately NOT [`R6KeySafe`]; this trait is the documented
/// legacy classification for exactly those three slot fields, never an unchecked
/// `_`. The full typed-newtype restructuring lands with the slot-identity
/// migration.
pub trait LegacyEnvDim: legacy_env_sealed::Sealed {}

/// Per-field classifier for the legacy raw env dimensions of the slot identity.
fn legacy_env_dimension<T: LegacyEnvDim>(_: &T) {}

impl legacy_env_sealed::Sealed for HashValue {}
impl LegacyEnvDim for HashValue {}
impl legacy_env_sealed::Sealed for u32 {}
impl LegacyEnvDim for u32 {}

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

fn w_authored_body_locator(l: &AuthoredBodyLocator) {
    match l {
        AuthoredBodyLocator::DeclBody(slot) => key_safe(slot),
        AuthoredBodyLocator::MacroPayload(payload) => key_safe(payload),
    }
}

fn w_semantic_symbol_space(s: &SemanticSymbolSpace) {
    match s {
        SemanticSymbolSpace::Type | SemanticSymbolSpace::Value | SemanticSymbolSpace::Namespace => {
        }
    }
}

fn w_projection_mode(m: &ProjectionMode) {
    match m {
        ProjectionMode::Identity
        | ProjectionMode::Navigate
        | ProjectionMode::Shallow
        | ProjectionMode::Expanded
        | ProjectionMode::Skeleton => {}
    }
}

fn w_reduction_demand(d: &ReductionDemand) {
    match d {
        ReductionDemand::Published
        | ReductionDemand::StructuralTransit
        | ReductionDemand::MacroObjectSurface => {}
    }
}

fn w_surface_provenance(p: &SurfaceProvenanceContext) {
    match p {
        SurfaceProvenanceContext::Structural | SurfaceProvenanceContext::MacroTypeArgOwnBody => {}
    }
}

fn w_member_merge_role(r: &MemberMergeRole) {
    match r {
        MemberMergeRole::Authored | MemberMergeRole::OwnBody | MemberMergeRole::Heritage => {}
    }
}

fn w_projection_reduction_context(c: &ProjectionReductionContext) {
    let ProjectionReductionContext {
        mode,
        demand,
        provenance,
        merge_role,
    } = c;
    key_safe(mode);
    key_safe(demand);
    key_safe(provenance);
    key_safe(merge_role);
}

fn w_resolved_decl_slot_identity(s: &ResolvedDeclSlotIdentity) {
    let ResolvedDeclSlotIdentity {
        defining_canonical,
        merged_symbol_name,
        symbol_space,
        project_identity,
        type_env_hash,
        lib_env_hash,
    } = s;
    key_safe(defining_canonical);
    key_safe(merged_symbol_name);
    key_safe(symbol_space);
    // LEGACY carried obligation (design §9): these three are raw env fields the
    // slot-identity migration will type as dimensions. Classified through the
    // NAMED legacy path, never an unchecked `_`.
    legacy_env_dimension(project_identity);
    legacy_env_dimension(type_env_hash);
    legacy_env_dimension(lib_env_hash);
}

/// The session warm-memo key for a lowered authored body.
///
/// Content-free (R6): NO content/whole hash, NO `FileWholeHash`, NO
/// `SemanticNodeId`, NO `HotTypeRef`, NO versioned `DeclIdentity`. The
/// `defining_canonical` env carried by `slot` is the DECL's defining env; the
/// standalone env-dimension wrappers are the LIVE graph-lowering env, which can
/// differ. The live whole-hash is re-sourced at value-compute time and recorded
/// in the caller's read-set, never carried here.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LocatorLoweringKey {
    /// The env-bearing content-free decl slot identity.
    pub slot: ResolvedDeclSlotIdentity,
    /// The authored body being lowered.
    pub locator: AuthoredBodyLocator,
    /// Live parse-env dimension.
    pub parse_env_hash: ParseEnvHash,
    /// Live resolve-env dimension.
    pub resolve_env_hash: ResolveEnvHash,
    /// Live type-env dimension.
    pub type_env_hash: TypeEnvHash,
    /// Live lib-env dimension.
    pub lib_env_hash: LibEnvHash,
    /// Live project-identity dimension.
    pub project_identity: ProjectIdentity,
    /// The full projection-reduction axis (mode / demand / provenance /
    /// merge-role).
    pub projection: ProjectionReductionContext,
    /// The substitution axis, when lowering under an instantiated body.
    pub substitution: SubstitutionCanonicalHash,
}

// Bound to its whole-key exhaustive-destructure witness below.
impl_r6_key_safe!(LocatorLoweringKey => w_locator_lowering_key);

/// R6 type-level key witness for the WHOLE key. The EXHAUSTIVE destructure (no
/// `..`, no `_` composite field) calls `key_safe` on EVERY field — `slot`,
/// `locator`, `projection`, and every dimension — so a forbidden dimension
/// (content/whole hash, `SemanticNodeId`, `HotTypeRef`, versioned `DeclIdentity`)
/// cannot occupy ANY position, including nested inside a composite field, and a
/// NEW key field fails compilation until it is classified `R6KeySafe`. The
/// composites are proven key-safe by their own destructure witnesses above.
fn w_locator_lowering_key(k: &LocatorLoweringKey) {
    let LocatorLoweringKey {
        slot,
        locator,
        parse_env_hash,
        resolve_env_hash,
        type_env_hash,
        lib_env_hash,
        project_identity,
        projection,
        substitution,
    } = k;
    key_safe(slot);
    key_safe(locator);
    key_safe(parse_env_hash);
    key_safe(resolve_env_hash);
    key_safe(type_env_hash);
    key_safe(lib_env_hash);
    key_safe(project_identity);
    key_safe(projection);
    key_safe(substitution);
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
        assert_r6_key_dimension::<ProjectIdentity>();
        assert_r6_key_dimension::<SubstitutionCanonicalHash>();

        // The whole key + its composites + a forbidden-dim-free container are
        // key-safe. (A forbidden dimension — standalone or nested — fails these
        // bounds; that negative is proven by the trybuild compile-fail fixtures.)
        assert_r6_key_safe::<LocatorLoweringKey>();
        assert_r6_key_safe::<AuthoredBodyLocator>();
        assert_r6_key_safe::<ResolvedDeclSlotIdentity>();
        assert_r6_key_safe::<ProjectionReductionContext>();
        assert_r6_key_safe::<SessionDemandIdentity>();
        assert_r6_key_safe::<Option<ParseEnvHash>>();
        assert_r6_key_safe::<Arc<[TypeArgLocator]>>();
    }
}

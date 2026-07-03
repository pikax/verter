//! Session-side key identities for locator-backed body lowering.
//!
//! [`LocatorLoweringKey`] is the session warm-memo key for a lowered authored
//! body: the decl slot identity + the content-free [`AuthoredBodyLocator`] + the
//! full live graph-lowering env dimensions + the projection-reduction axis + the
//! substitution axis. [`SessionDemandIdentity`] is the session-only replay
//! identity for a graph-raised adapter payload; it is NEVER stored in a lower
//! crate and does NOT lower via [`LocatorLoweringKey`].
//!
//! # R6 type-level key witness
//!
//! R6 forbids content/whole hashes, `SemanticNodeId`, `HotTypeRef`, and versioned
//! `DeclIdentity` in a content-free query-identity key. A raw `[u8; 16]` cannot
//! distinguish an ENV hash from a CONTENT hash, so the env dimensions are typed
//! newtypes ([`ParseEnvHash`] etc.) whose membership in the closed dimension set
//! is a SEALED trait ([`R6KeyDimension`]). A forbidden dimension is a different
//! type with no `R6KeyDimension` impl, and the trait is sealed so no downstream
//! type can join the set — a compile-time proof, not a name scan. The
//! [`LocatorLoweringKey`] witness below destructures the key exhaustively and
//! asserts every env dimension is sealed.
//!
//! These identities are the B1 substrate: defined and witnessed here, wired by
//! later blocks. They have no production caller yet.

#![allow(dead_code)]

use std::sync::Arc;

use crate::semantic_query::{HashValue, ProjectionReductionContext, ResolvedDeclSlotIdentity};
use verter_type_expr::locators::AuthoredBodyLocator;

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

/// The parse-env-hash dimension.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ParseEnvHash(pub HashValue);
impl sealed::Sealed for ParseEnvHash {}
impl R6KeyDimension for ParseEnvHash {}

/// The resolve-env-hash dimension.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResolveEnvHash(pub HashValue);
impl sealed::Sealed for ResolveEnvHash {}
impl R6KeyDimension for ResolveEnvHash {}

/// The type-env-hash dimension.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TypeEnvHash(pub HashValue);
impl sealed::Sealed for TypeEnvHash {}
impl R6KeyDimension for TypeEnvHash {}

/// The lib-env-hash dimension.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LibEnvHash(pub HashValue);
impl sealed::Sealed for LibEnvHash {}
impl R6KeyDimension for LibEnvHash {}

/// The project-identity dimension.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProjectIdentity(pub u32);
impl sealed::Sealed for ProjectIdentity {}
impl R6KeyDimension for ProjectIdentity {}

/// The content-free canonical identity of the instantiation substitution
/// environment lowered under (the `Instantiate.args` axis, §6.4). It is NOT a
/// file content/whole hash and NOT a raw `SemanticNodeId`: it is the stable
/// canonical hash of the resolved substitution, computed at value-compute time
/// (its production computation is a downstream block's concern). Distinguishing
/// it as its own dimension type prevents a `{locator + resolve_env_hash}`-only
/// key from aliasing distinct lowered nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SubstitutionCanonicalHash(pub HashValue);
impl sealed::Sealed for SubstitutionCanonicalHash {}
impl R6KeyDimension for SubstitutionCanonicalHash {}

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

const _: () = {
    // R6 type-level key witness: every env-hash dimension of `LocatorLoweringKey`
    // is a sealed R6 key dimension. The EXHAUSTIVE destructure (no `..`) forces a
    // new key field to be reflected here; each hash dimension is proven sealed
    // via `assert_r6_key_dimension`, so a forbidden dimension (content/whole
    // hash, SemanticNodeId, HotTypeRef, versioned DeclIdentity) could not occupy
    // one of these positions. The composite parts (`slot`, `locator`,
    // `projection`) are content-free by their own construction.
    fn env_dims_are_sealed_r6(k: &LocatorLoweringKey) {
        let LocatorLoweringKey {
            slot: _,
            locator: _,
            parse_env_hash,
            resolve_env_hash,
            type_env_hash,
            lib_env_hash,
            project_identity,
            projection: _,
            substitution,
        } = k;
        fn dim<T: R6KeyDimension>(_: &T) {}
        dim(parse_env_hash);
        dim(resolve_env_hash);
        dim(type_env_hash);
        dim(lib_env_hash);
        dim(project_identity);
        dim(substitution);
    }
    let _ = env_dims_are_sealed_r6;
    // Anchor the pub witness helper to an in-crate call site.
    let _ = assert_r6_key_dimension::<ParseEnvHash>;
};

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

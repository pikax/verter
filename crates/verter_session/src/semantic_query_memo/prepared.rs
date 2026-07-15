//! Prepared dispatch identity — the per-execute query token.
//!
//! `execute_cooperative` projects its [`SemanticQueryKey`] onto the
//! family memo through THREE derived identities: the `(FamilyKey,
//! ModeSlot)` pair (`family_and_slot`), the §3.4 requested
//! materialised point (`requested_point_for_key` — which itself runs
//! `family_and_slot` again), and the key's hash (recomputed by every
//! in-flight table probe). Before this module the warm probe, the
//! slow-path warm re-read, and the cold-winner publish each rebuilt
//! those identities from scratch, and the slow path cloned the FULL
//! key into the in-flight table, the recursion stack, and the panic
//! guard separately.
//!
//! [`PreparedKeyHandle`] computes the whole bundle ONCE per execute —
//! `{family, slot, requested_path, requested_point, cached_hash}` over
//! the owned key — behind one `Arc`, so every downstream identity use
//! is a pointer bump or a field read.
//!
//! **Equality delegates to the key.** Handle equality is DEFINED as
//! full-key equality (`cached_hash` fast-reject + `SemanticQueryKey ==`;
//! `Arc::ptr_eq` fast-accept). It is deliberately NOT `(family, slot)`
//! equality: `family_and_slot` is not injective — e.g. every
//! `MacroObjectSurface`-demand mode lands in the single
//! `MacroSurfaceShallow` slot — so keying admission on the projected
//! pair would coalesce DISTINCT queries onto one in-flight entry /
//! recursion frame. The bijection (`handle equality ⟺ key equality`)
//! is pinned per variant by `prepared_identity_bijection` in the memo
//! test suite.
//!
//! **R6.** The token carries ONLY the content-free key, identities
//! derived from that key, and a hash OF that key. No content/version
//! hash, no store-view generation, no `fact_dep_signature` enters the
//! token.

use std::hash::{BuildHasher, Hash, Hasher};
use std::sync::Arc;

use rustc_hash::FxBuildHasher;

use crate::semantic_query::demand::{MaterializedPoint, ProjectionPath};
use crate::semantic_query::SemanticQueryKey;

use super::family::{family_and_slot, point_for_slot, requested_path_for_key, FamilyKey, ModeSlot};

/// Hash a [`SemanticQueryKey`] with the same fixed hasher
/// [`PreparedKeyHandle::prepare`] caches — lets a by-key probe against
/// token collections fast-reject on the cached hash before falling
/// back to full-key equality.
pub(super) fn hash_key(key: &SemanticQueryKey) -> u64 {
    FxBuildHasher.hash_one(key)
}

/// The prepared identity bundle for one [`SemanticQueryKey`]. Every
/// field is a pure function of `key`; see the module docs for the
/// equality contract.
struct PreparedQueryIdentity {
    key: SemanticQueryKey,
    family: FamilyKey,
    slot: ModeSlot,
    requested_path: ProjectionPath,
    requested_point: MaterializedPoint,
    cached_hash: u64,
}

/// Shared handle over one prepared query identity. Cloning is an
/// `Arc` refcount bump — the in-flight table entry, the recursion
/// stack frame, and the panic guard all share ONE prepared bundle.
#[derive(Clone)]
pub(super) struct PreparedKeyHandle(Arc<PreparedQueryIdentity>);

impl PreparedKeyHandle {
    /// Project `key` onto its full prepared identity — ONE
    /// `family_and_slot` walk, ONE requested-point build, ONE key
    /// hash. The key is moved in, never cloned.
    pub(super) fn prepare(key: SemanticQueryKey) -> Self {
        let (family, slot) = family_and_slot(&key);
        let requested_path = requested_path_for_key(&key);
        // Same formula as `family::requested_point_for_key`, reusing
        // the `family_and_slot` result instead of re-projecting.
        let requested_point = MaterializedPoint::new(point_for_slot(slot, &requested_path));
        let cached_hash = hash_key(&key);
        Self(Arc::new(PreparedQueryIdentity {
            key,
            family,
            slot,
            requested_path,
            requested_point,
            cached_hash,
        }))
    }

    pub(super) fn key(&self) -> &SemanticQueryKey {
        &self.0.key
    }

    pub(super) fn family(&self) -> &FamilyKey {
        &self.0.family
    }

    pub(super) fn slot(&self) -> ModeSlot {
        self.0.slot
    }

    pub(super) fn requested_path(&self) -> &ProjectionPath {
        &self.0.requested_path
    }

    pub(super) fn requested_point(&self) -> &MaterializedPoint {
        &self.0.requested_point
    }

    /// Whether `other` is the SAME prepared instance (pointer
    /// identity). Used by the recursion-stack guard to pop exactly the
    /// frame it pushed.
    pub(super) fn same_instance(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    /// Whether this handle's key equals `key`, with `key_hash`
    /// (produced by [`hash_key`]) as the fast-reject accelerator.
    pub(super) fn key_matches(&self, key: &SemanticQueryKey, key_hash: u64) -> bool {
        self.0.cached_hash == key_hash && &self.0.key == key
    }
}

impl PartialEq for PreparedKeyHandle {
    fn eq(&self, other: &Self) -> bool {
        // Pointer fast-accept, cached-hash fast-reject, full-key
        // equality as the authority. Handle equality ⟺ key equality.
        Arc::ptr_eq(&self.0, &other.0)
            || (self.0.cached_hash == other.0.cached_hash && self.0.key == other.0.key)
    }
}

impl Eq for PreparedKeyHandle {}

impl Hash for PreparedKeyHandle {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Consistent with `Eq`: equal keys produce equal `cached_hash`
        // under the fixed `hash_key` hasher.
        state.write_u64(self.0.cached_hash);
    }
}

impl std::fmt::Debug for PreparedKeyHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreparedKeyHandle")
            .field("key", &self.0.key)
            .field("slot", &self.0.slot)
            .finish_non_exhaustive()
    }
}

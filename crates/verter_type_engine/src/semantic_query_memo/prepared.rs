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

use crate::semantic_query::checker_policy::TupleSplicePosition;
use crate::semantic_query::demand::{MaterializedPoint, ProjectionPath};
use crate::semantic_query::SemanticQueryKey;

use super::family::{point_for_slot, requested_path_for_key, FamilyKey, ModeSlot};

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
    _signature: Option<crate::signature_kernel::SignatureEpochLease>,
    _roots: super::node_roots::NodeRootSet,
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
pub struct PreparedKeyHandle(Arc<PreparedQueryIdentity>);

impl PreparedKeyHandle {
    /// Project `key` onto its full prepared identity — ONE
    /// `family_and_slot` walk, ONE requested-point build, ONE key
    /// hash. The key is moved in, never cloned.
    pub fn prepare(key: SemanticQueryKey) -> Self {
        Self::prepare_in(None, key)
    }

    /// [`Self::prepare`] whose interned family operands are owned in
    /// `arena`, the graph that issued the key.
    fn prepare_in(arena: Option<&Arc<super::arena::NodeArenaCore>>, key: SemanticQueryKey) -> Self {
        let (family, slot) = super::family::family_and_slot_in(arena, &key);
        let requested_path = requested_path_for_key(&key);
        // Same formula as `family::requested_point_for_key`, reusing
        // the `family_and_slot` result instead of re-projecting. Keys
        // that embed their own demand point (`FlowReturn`) gate on that
        // point, not the slot preset.
        let requested_point = match super::family::requested_demand_override(&key) {
            Some(point) => MaterializedPoint::new(point),
            None => MaterializedPoint::new(point_for_slot(slot, &requested_path)),
        };
        let cached_hash = hash_key(&key);
        Self(Arc::new(PreparedQueryIdentity {
            _signature: None,
            _roots: Default::default(),
            key,
            family,
            slot,
            requested_path,
            requested_point,
            cached_hash,
        }))
    }

    pub fn key(&self) -> &SemanticQueryKey {
        &self.0.key
    }

    /// The key's prepared hash.
    pub(super) fn cached_hash(&self) -> u64 {
        self.0.cached_hash
    }

    pub fn family(&self) -> &FamilyKey {
        &self.0.family
    }

    pub fn slot(&self) -> ModeSlot {
        self.0.slot
    }

    pub(super) fn requested_path(&self) -> &ProjectionPath {
        &self.0.requested_path
    }

    pub fn requested_point(&self) -> &MaterializedPoint {
        &self.0.requested_point
    }
}

impl super::SemanticGraphStore {
    /// Prepare `key` with its operands rooted, for a key built from live
    /// construction operands.
    pub(crate) fn prepare_rooted_key(&self, key: SemanticQueryKey) -> PreparedKeyHandle {
        self.try_prepare_rooted_key(key)
            .expect("a prepared key has live counted operands")
    }

    /// Prepare `key` with its operands rooted. A key naming a destroyed
    /// (`StaleSemanticOperand`) or never-allocated (`ForeignSemanticOperand`)
    /// operand is refused: it can neither be answered nor cached.
    pub(crate) fn try_prepare_rooted_key(
        &self,
        key: SemanticQueryKey,
    ) -> Result<PreparedKeyHandle, crate::semantic_query::QueryError> {
        let mut prepared = self.prepare_probe_key(key);
        self.try_root_prepared_key(&mut prepared)?;
        Ok(prepared)
    }

    /// The prepared identity of `key` for a warm probe: its family, slot,
    /// requested point and hash, with its operands still owned by the
    /// caller that holds `key`'s nodes. A probe that hits returns the
    /// stored result's own lease and drops this identity; one that goes on
    /// to claim roots it first ([`Self::try_root_prepared_key`]), because a
    /// claim keeps it past the caller's hold.
    pub(super) fn prepare_probe_key(&self, key: SemanticQueryKey) -> PreparedKeyHandle {
        let signature = match &key {
            SemanticQueryKey::ReadSignatureResult(key) => self
                .signature_store()
                .lease_epoch(key.descriptor.epoch())
                .ok(),
            _ => None,
        };
        let mut prepared = PreparedKeyHandle::prepare_in(Some(&self.arena.core), key);
        Arc::get_mut(&mut prepared.0)
            .expect("a fresh prepared identity is unshared")
            ._signature = signature;
        prepared
    }

    /// Root a probe identity's family operands in this arena, so the
    /// identity outlives the caller's hold on them. A destroyed or
    /// never-allocated operand is refused.
    pub(super) fn try_root_prepared_key(
        &self,
        prepared: &mut PreparedKeyHandle,
    ) -> Result<(), crate::semantic_query::QueryError> {
        let mut nodes = super::node_roots::RootIds::new();
        prepared.family().for_each_node_id(|node| {
            if node.0 < super::UNALLOCATABLE_ID_FLOOR {
                nodes.push(node);
            }
        });
        // A relation identity's interned key already roots its operands in
        // this arena for as long as the identity holds it: a second root set
        // over the same nodes would only duplicate that ownership.
        if let super::family::FamilyKey::Relate { key } = prepared.family() {
            if key.roots_all_in(&self.arena.core, &nodes) {
                return Ok(());
            }
        }
        let roots = self.root_nodes(nodes)?;
        Arc::get_mut(&mut prepared.0)
            .expect("a probe identity is unshared until its claim")
            ._roots = roots;
        Ok(())
    }
}

impl verter_execution::tasks::ProducerIdentity for PreparedQueryIdentity {
    fn producer_hash(&self) -> u64 {
        self.cached_hash
    }

    fn same_producer(&self, other: &dyn verter_execution::tasks::ProducerIdentity) -> bool {
        other.downcast_ref::<Self>().is_some_and(|other| {
            std::ptr::eq(self, other)
                || (self.cached_hash == other.cached_hash && self.key == other.key)
        })
    }
}

impl PreparedKeyHandle {
    /// This handle as the producer a task opens: a refcount bump, never a
    /// new allocation.
    pub(crate) fn as_task_producer(&self) -> Arc<dyn verter_execution::tasks::ProducerIdentity> {
        Arc::clone(&self.0) as Arc<dyn verter_execution::tasks::ProducerIdentity>
    }

    /// This handle as the producer a task opens for a demand at `site`. A
    /// type-site producer is the prepared identity itself (a refcount bump);
    /// an expression-site producer is a distinct identity over it, so a task
    /// producing a key at one site never reads its demand of the key at the
    /// other as same-path recursion.
    pub(super) fn as_site_producer(
        &self,
        site: TupleSplicePosition,
    ) -> Arc<dyn verter_execution::tasks::ProducerIdentity> {
        match site {
            TupleSplicePosition::Type => self.as_task_producer(),
            TupleSplicePosition::Expression => Arc::new(ExpressionSiteProducer(self.clone())),
        }
    }

    /// The task-producer hash of a key whose prepared hash is `key_hash`,
    /// demanded at `site`.
    pub(super) fn site_producer_hash(key_hash: u64, site: TupleSplicePosition) -> u64 {
        match site {
            TupleSplicePosition::Type => key_hash,
            TupleSplicePosition::Expression => key_hash.rotate_left(1) ^ EXPRESSION_SITE_SALT,
        }
    }

    /// The heap a prepared identity keeps reachable beyond its own block,
    /// when `producer` is one: its key, the family projected from it (which
    /// shares the key's slices and names), its requested path and point,
    /// and its operand roots, each `Arc` allocation once per `aliases`. The
    /// signature epoch a lease pins is the epoch's own charge.
    pub(crate) fn producer_heap_bytes(
        producer: &dyn verter_execution::tasks::ProducerIdentity,
        aliases: &mut verter_session_query::retention::allocation::AliasSet,
    ) -> Option<usize> {
        use crate::semantic_query::allocation_footprint::OwnedHeap as _;
        let PreparedQueryIdentity {
            _signature,
            _roots,
            key,
            family,
            slot: _,
            requested_path,
            requested_point,
            cached_hash: _,
        } = producer.downcast_ref::<PreparedQueryIdentity>()?;
        Some(
            super::family_footprint::query_key_heap_bytes(key, aliases)
                + super::family_footprint::family_key_heap_bytes(family, aliases)
                + requested_path.owned_heap_bytes(aliases)
                + requested_point.path().owned_heap_bytes(aliases)
                + _roots.owned_heap_bytes(aliases),
        )
    }

    /// Whether `producer` is the semantic producer for exactly `key`, whose
    /// prepared hash is `key_hash`.
    pub(super) fn producer_matches_key(
        producer: &dyn verter_execution::tasks::ProducerIdentity,
        key: &SemanticQueryKey,
        key_hash: u64,
        site: TupleSplicePosition,
    ) -> bool {
        let identity = match site {
            TupleSplicePosition::Type => producer.downcast_ref::<PreparedQueryIdentity>(),
            TupleSplicePosition::Expression => producer
                .downcast_ref::<ExpressionSiteProducer>()
                .map(|producer| producer.0 .0.as_ref()),
        };
        identity.is_some_and(|identity| identity.cached_hash == key_hash && &identity.key == key)
    }
}

/// Mixed into an expression-site producer's hash, so its open slot rejects
/// a type-site match on the hash alone.
const EXPRESSION_SITE_SALT: u64 = 0x9e37_79b9_7f4a_7c15;

/// The producer of a prepared key demanded at the expression checking site.
/// Its identity is the key and the site: the same key demanded at the type
/// site is a different computation (it recovers with TS2799, not TS2800).
struct ExpressionSiteProducer(PreparedKeyHandle);

impl verter_execution::tasks::ProducerIdentity for ExpressionSiteProducer {
    fn producer_hash(&self) -> u64 {
        PreparedKeyHandle::site_producer_hash(
            self.0 .0.cached_hash,
            TupleSplicePosition::Expression,
        )
    }

    fn same_producer(&self, other: &dyn verter_execution::tasks::ProducerIdentity) -> bool {
        other
            .downcast_ref::<Self>()
            .is_some_and(|other| other.0 == self.0)
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

/// Value-shape gate for the non-node object projection family. A buggy
/// evaluator (or a poisoned synthetic publish) must not disguise its result
/// as `TypeNode` or admit that payload into the projection slot.
pub(crate) fn enforce_projection_value_shape(
    key: &SemanticQueryKey,
    result: crate::semantic_query::QueryResult<crate::semantic_query::SemanticQueryValue>,
) -> crate::semantic_query::QueryResult<crate::semantic_query::SemanticQueryValue> {
    use crate::semantic_query::{QueryError, QueryResult, SemanticQueryValueTag};
    match (key, result) {
        (SemanticQueryKey::ProjectObjectSpread { .. }, QueryResult::Value(value))
            if value.tag() != SemanticQueryValueTag::ObjectProjection =>
        {
            QueryResult::Error(QueryError::ValueDomainMismatch {
                expected: SemanticQueryValueTag::ObjectProjection,
                actual: value.tag(),
            })
        }
        (_, result) => result,
    }
}

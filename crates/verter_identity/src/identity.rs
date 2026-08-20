//! Distinct identity types: each is a different nominal Rust type (never a
//! `type Alias = OtherType;`), so passing one where another is expected is
//! a compile error. `StableEntityId` and `SessionHandle` also differ in
//! shape (digest vs owner-cohort/generation/nonce): a `SessionHandle` is
//! not a serializable stable reference (`identity-encoding.md` §3), so
//! sharing `StableEntityId`'s shape would invite that misuse.
//!
//! Constructors take a caller-supplied [`CanonicalEncode`] descriptor (or
//! an architecturally fixed shape). Domain-field equality — what makes two
//! `SourceRevision`s equal — is owned by the type that defines those
//! fields, not this crate.

use core::cmp::Ordering;
use core::marker::PhantomData;

use crate::canonical::Canonical;
use crate::encoding::{CanonicalDigest, CanonicalEncode, CanonicalEncoder};

digest_identity!(
    /// Exact byte content identity.
    ContentId
);

impl ContentId {
    const RAW_CONTENT_DOMAIN_TAG: &'static str = "verter.identity.content_id.raw.v1";

    /// Hash exact content bytes; no further descriptor schema.
    pub fn from_content_bytes(bytes: &[u8]) -> Self {
        let mut encoder = CanonicalEncoder::new(Self::RAW_CONTENT_DOMAIN_TAG);
        encoder.field_bytes(1, bytes);
        Self(Canonical::from_encoder(&encoder))
    }
}

digest_identity!(
    /// Logical source identity.
    SourceId
);
digest_identity!(
    /// Exact source version.
    SourceRevision
);
digest_identity!(
    /// Stable logical carrier-unit identity.
    SourceUnitId
);
digest_identity!(
    /// Project topology identity.
    ProjectRevision
);
digest_identity!(
    /// Configuration identity.
    ConfigurationRevision
);
digest_identity!(
    /// Grammar / source-type / recovery / options identity.
    SyntaxProfileId
);
digest_identity!(
    /// Exact syntax-construction identity.
    ParseKey
);
digest_identity!(
    /// Open/close lifecycle identity. Not a monotonic counter: two
    /// incarnations of the same document opened in different orders
    /// across restarts are distinguishable, not comparable.
    DocumentIncarnation
);
digest_identity!(
    /// Provider route / version / capability interpretation.
    ProviderContractId
);
digest_identity!(
    /// Deterministic public/content-relative identity. Collision-sensitive:
    /// compare [`Self::canonical_bytes`], never the digest alone.
    StableEntityId
);
digest_identity!(
    /// In-flight semantic observation basis. Not part of cross-snapshot
    /// candidate lookup (`result-contract-and-flight.md` §1) — that is
    /// [`QueryIdentity`].
    InputBasisId
);
digest_identity!(
    /// Observable semantics / exactness / capability / approximation
    /// contract (`result-contract-and-flight.md` §1). Does not duplicate
    /// separately-keyed profile IDs.
    ResultContractId
);

/// LSP client document version: the client-assigned integer, not digest
/// backed. Comparison is the protocol's (monotonically non-decreasing).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct DocumentVersion(pub i64);

/// Committed-input ordering aid. A monotonic counter (`Ord`), not a
/// digest and not a universal cache key.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct EngineRevision(pub u64);

/// Selected-provider lifecycle epoch — monotonic, not a digest.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct ProviderEpoch(pub u64);

/// Supersession order for one request stream. Not comparable across
/// streams; this crate does not invent a stream identity.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct RequestGeneration(pub u64);

/// Serialized/persistent interpretation namespace. Kept separate from
/// [`CompatibilityEpoch`]: namespace and epoch are different concerns
/// (ADR-002), so they are not one struct that could mix unrelated pairs.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct CompatibilityDomainId(pub &'static str);

/// Monotonic epoch inside a [`CompatibilityDomainId`]. `0` is a valid
/// first epoch, not an uninitialized sentinel — unlike
/// `nonzero_version!` wire newtypes, whose `new` rejects zero.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct CompatibilityEpoch(pub u32);

/// Parse-owner kind. Closed enum, not a digest; `Managed` carries the
/// shard that distinguishes one managed owner from another.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum ParseOwnerDomainId {
    /// One-shot direct invocation or batch — no retained owner state.
    DirectOrBatch,
    /// `PreparedCarrier` progressive-execution owner.
    PreparedCarrier,
    /// Managed-engine owner/shard.
    Managed { shard: u32 },
}

/// `(ParseOwnerDomainId, ParseKey, instance generation)`.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct ParseInstanceId {
    pub owner_domain: ParseOwnerDomainId,
    pub parse_key: ParseKey,
    pub instance_generation: u64,
}

impl PartialOrd for ParseInstanceId {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for ParseInstanceId {
    fn cmp(&self, other: &Self) -> Ordering {
        self.owner_domain
            .cmp(&other.owner_domain)
            .then_with(|| self.parse_key.cmp(&other.parse_key))
            .then_with(|| self.instance_generation.cmp(&other.instance_generation))
    }
}

/// Typed artifact construction identity. The `T` marker keeps
/// `ArtifactKey<Foo>` and `ArtifactKey<Bar>` distinct even when their
/// canonical bytes coincide.
pub struct ArtifactKey<T> {
    canonical: Canonical,
    marker: PhantomData<fn() -> T>,
}

impl<T> ArtifactKey<T> {
    pub fn from_canonical<D: CanonicalEncode>(value: &D) -> Self {
        Self {
            canonical: Canonical::from_encodable(value),
            marker: PhantomData,
        }
    }

    pub fn digest(&self) -> CanonicalDigest {
        self.canonical.digest()
    }

    pub fn canonical_bytes(&self) -> &[u8] {
        self.canonical.bytes()
    }
}

// Manual impls: a derive would require `T: Clone + PartialEq + …`, but
// `T` is a phantom marker.
impl<T> Clone for ArtifactKey<T> {
    fn clone(&self) -> Self {
        Self {
            canonical: self.canonical.clone(),
            marker: PhantomData,
        }
    }
}
impl<T> PartialEq for ArtifactKey<T> {
    fn eq(&self, other: &Self) -> bool {
        self.canonical == other.canonical
    }
}
impl<T> Eq for ArtifactKey<T> {}
impl<T> core::hash::Hash for ArtifactKey<T> {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.canonical.hash(state);
    }
}
impl<T> core::fmt::Debug for ArtifactKey<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "ArtifactKey({:?})", self.canonical)
    }
}

/// Semantic arguments + observed profile IDs + [`ResultContractId`].
/// Snapshot-independent: no [`InputBasisId`], so this is a cross-snapshot
/// cache-candidate key. [`SemanticFlightKey`] adds the basis for in-flight
/// production. The `Q` marker distinguishes query kinds
/// (`QueryIdentity<ResolveImport>` vs `QueryIdentity<ComponentMeta>`),
/// same as [`ArtifactKey`].
pub struct QueryIdentity<Q> {
    canonical: Canonical,
    marker: PhantomData<fn() -> Q>,
}

impl<Q> QueryIdentity<Q> {
    /// Compose the three parts. `observed_profiles` is a canonical sorted
    /// set — profile order must not affect identity.
    pub fn compose(
        query_kind_domain_tag: &'static str,
        semantic_arguments: CanonicalDigest,
        observed_profiles: &[CanonicalDigest],
        result_contract: &ResultContractId,
    ) -> Self {
        let mut encoder = CanonicalEncoder::new(query_kind_domain_tag);
        encoder.field_bytes(1, semantic_arguments.as_bytes());
        encoder.field_sorted_set(2, observed_profiles.iter().map(|d| *d.as_bytes()));
        encoder.field_bytes(3, result_contract.digest().as_bytes());
        Self {
            canonical: Canonical::from_encoder(&encoder),
            marker: PhantomData,
        }
    }

    pub fn digest(&self) -> CanonicalDigest {
        self.canonical.digest()
    }

    pub fn canonical_bytes(&self) -> &[u8] {
        self.canonical.bytes()
    }
}

impl<Q> Clone for QueryIdentity<Q> {
    fn clone(&self) -> Self {
        Self {
            canonical: self.canonical.clone(),
            marker: PhantomData,
        }
    }
}
impl<Q> PartialEq for QueryIdentity<Q> {
    fn eq(&self, other: &Self) -> bool {
        self.canonical == other.canonical
    }
}
impl<Q> Eq for QueryIdentity<Q> {}
impl<Q> core::hash::Hash for QueryIdentity<Q> {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.canonical.hash(state);
    }
}
impl<Q> core::fmt::Debug for QueryIdentity<Q> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "QueryIdentity({:?})", self.canonical)
    }
}

/// `(QueryIdentity<Q>, InputBasisId)`. Strictly bigger than
/// [`QueryIdentity`], so the two cannot coerce. Cross-snapshot joining is
/// disabled by default (`result-contract-and-flight.md` §2.2) in the
/// flight runtime, not in this key type.
pub struct SemanticFlightKey<Q> {
    pub query_identity: QueryIdentity<Q>,
    pub input_basis: InputBasisId,
}

// Manual impls: a derive would require `Q: Trait` even though `Q` is only
// a phantom marker inside `QueryIdentity<Q>`.
impl<Q> Clone for SemanticFlightKey<Q> {
    fn clone(&self) -> Self {
        Self {
            query_identity: self.query_identity.clone(),
            input_basis: self.input_basis.clone(),
        }
    }
}
impl<Q> PartialEq for SemanticFlightKey<Q> {
    fn eq(&self, other: &Self) -> bool {
        self.query_identity == other.query_identity && self.input_basis == other.input_basis
    }
}
impl<Q> Eq for SemanticFlightKey<Q> {}
impl<Q> core::hash::Hash for SemanticFlightKey<Q> {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.query_identity.hash(state);
        self.input_basis.hash(state);
    }
}
impl<Q> core::fmt::Debug for SemanticFlightKey<Q> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SemanticFlightKey")
            .field("query_identity", &self.query_identity)
            .field("input_basis", &self.input_basis)
            .finish()
    }
}

/// Opaque owner/cohort-bound continuation handle. Not
/// [`StableEntityId`]-shaped (`identity-encoding.md` §3): a session handle
/// must carry owner cohort and generation, and must not serialize as a
/// stable reference without an explicit protocol. Fields are private;
/// mint only through [`Self::mint`] so it cannot be forged from bytes the
/// way a content-addressed [`StableEntityId`] can.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct SessionHandle {
    owner_cohort: CanonicalDigest,
    generation: u64,
    nonce: CanonicalDigest,
}

impl SessionHandle {
    /// Mint for a live owner cohort. The caller supplies a uniqueness
    /// nonce and must not reuse it within a cohort/generation pair. This
    /// crate does not generate randomness.
    pub fn mint(owner_cohort: CanonicalDigest, generation: u64, nonce: CanonicalDigest) -> Self {
        Self {
            owner_cohort,
            generation,
            nonce,
        }
    }

    pub fn owner_cohort(&self) -> CanonicalDigest {
        self.owner_cohort
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::{Deadline, ExecutionPolicy, MemoryBudget, WorkBudget, WorkPriority};

    struct Args(u64);
    impl CanonicalEncode for Args {
        const DOMAIN_TAG: &'static str = "identity-test.args.v1";
        fn encode_fields(&self, e: &mut CanonicalEncoder) {
            e.field_u64(1, self.0);
        }
    }

    #[test]
    fn content_id_is_content_addressed() {
        let a1 = ContentId::from_content_bytes(b"hello");
        let a2 = ContentId::from_content_bytes(b"hello");
        let b = ContentId::from_content_bytes(b"world");
        assert_eq!(a1, a2);
        assert_ne!(a1, b);
    }

    #[test]
    fn stable_entity_id_is_deterministic_and_content_sensitive() {
        let a = StableEntityId::from_canonical(&Args(1));
        let b = StableEntityId::from_canonical(&Args(1));
        let c = StableEntityId::from_canonical(&Args(2));
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn session_handle_mint_is_nonce_sensitive() {
        let cohort = StableEntityId::from_canonical(&Args(1)).digest();
        let a = SessionHandle::mint(cohort, 1, CanonicalDigest::of_bytes(b"nonce-a"));
        let b = SessionHandle::mint(cohort, 1, CanonicalDigest::of_bytes(b"nonce-b"));
        assert_ne!(a, b);
        assert_eq!(a.owner_cohort(), b.owner_cohort());
        assert_eq!(a.generation(), b.generation());
    }

    #[test]
    fn query_identity_excludes_basis_but_flight_key_includes_it() {
        struct FakeQuery;
        let contract = ResultContractId::from_canonical(&Args(9));
        let qid_a = QueryIdentity::<FakeQuery>::compose(
            "identity-test.query.v1",
            CanonicalDigest::of_bytes(b"args"),
            &[],
            &contract,
        );
        let qid_b = QueryIdentity::<FakeQuery>::compose(
            "identity-test.query.v1",
            CanonicalDigest::of_bytes(b"args"),
            &[],
            &contract,
        );
        assert_eq!(qid_a, qid_b, "same parts must compose to the same identity");

        let basis_1 = InputBasisId::from_canonical(&Args(1));
        let basis_2 = InputBasisId::from_canonical(&Args(2));
        let flight_1 = SemanticFlightKey {
            query_identity: qid_a.clone(),
            input_basis: basis_1,
        };
        let flight_2 = SemanticFlightKey {
            query_identity: qid_b,
            input_basis: basis_2,
        };
        assert_ne!(
            flight_1, flight_2,
            "flight keys over the same query identity but different bases must differ"
        );
    }

    /// Profiles are a sorted set (`field_sorted_set`), not a positional list.
    #[test]
    fn query_identity_is_profile_order_independent() {
        struct FakeQuery;
        let contract = ResultContractId::from_canonical(&Args(9));
        let p1 = CanonicalDigest::of_bytes(b"profile-1");
        let p2 = CanonicalDigest::of_bytes(b"profile-2");
        let forward = QueryIdentity::<FakeQuery>::compose(
            "identity-test.query.v1",
            CanonicalDigest::of_bytes(b"args"),
            &[p1, p2],
            &contract,
        );
        let reverse = QueryIdentity::<FakeQuery>::compose(
            "identity-test.query.v1",
            CanonicalDigest::of_bytes(b"args"),
            &[p2, p1],
            &contract,
        );
        assert_eq!(forward, reverse);
    }

    /// Hand-written `Ord`: `ParseKey` orders by digest, not field content.
    #[test]
    fn parse_instance_id_orders_by_generation_within_same_owner_and_key() {
        let key = ParseKey::from_canonical(&Args(1));
        let low = ParseInstanceId {
            owner_domain: ParseOwnerDomainId::DirectOrBatch,
            parse_key: key.clone(),
            instance_generation: 1,
        };
        let high = ParseInstanceId {
            owner_domain: ParseOwnerDomainId::DirectOrBatch,
            parse_key: key,
            instance_generation: 2,
        };
        assert!(low < high);
    }

    #[test]
    fn artifact_key_is_deterministic_per_kind() {
        enum KindA {}
        let a1 = ArtifactKey::<KindA>::from_canonical(&Args(1));
        let a2 = ArtifactKey::<KindA>::from_canonical(&Args(1));
        assert_eq!(a1, a2);
    }

    /// `ExecutionPolicy` has no `CanonicalEncode` and must not grow one.
    #[test]
    fn execution_policy_is_not_identity_shaped() {
        let policy = ExecutionPolicy::<()> {
            deadline: Some(Deadline(100)),
            cancellation: (),
            priority: WorkPriority::High,
            work_budget: WorkBudget(10),
            memory_budget: MemoryBudget(1024),
        };
        // Compiles only against `ExecutionPolicy` fields, never a digest.
        assert_eq!(policy.work_budget, WorkBudget(10));
    }
}

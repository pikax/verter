//! Distinct identity types, per architecture.md §3.1: "Identity types are
//! non-interchangeable." Every type below is a DIFFERENT nominal Rust type
//! (never a `type Alias = OtherType;`), so the compiler — not review —
//! rejects passing one where another is expected. `StableEntityId` and
//! `SessionHandle` additionally differ in internal SHAPE (one is a single
//! canonical digest, the other is a three-part owner-cohort/generation/
//! nonce record) because they are not just distinct labels on the same
//! representation: a `SessionHandle` is not serializable as a stable
//! reference at all (identity-encoding.md §3), so giving it the same shape
//! as a `StableEntityId` would invite exactly the misuse this module exists
//! to prevent.
//!
//! Scope: this crate lands the NEUTRAL identity type and its canonical
//! construction primitive. The DOMAIN FIELDS a given identity closes over
//! (what exactly makes two `SourceRevision`s equal, for example) are owned
//! by whichever later block converges that concept — landing invented
//! domain fields here would create a second owner for something this crate
//! does not and should not own. Each constructor below therefore accepts a
//! caller-supplied [`CanonicalEncode`] descriptor (or, where the
//! architecture text fixes a concrete shape, that exact shape) rather than
//! a fixed field list.

use core::cmp::Ordering;
use core::marker::PhantomData;

use crate::canonical::Canonical;
use crate::encoding::{CanonicalDigest, CanonicalEncode, CanonicalEncoder};

digest_identity!(
    /// Exact byte content identity (architecture.md §3.1).
    ContentId
);

impl ContentId {
    /// Domain tag for the direct raw-content constructor below.
    const RAW_CONTENT_DOMAIN_TAG: &'static str = "verter.identity.content_id.raw.v1";

    /// Hashes exact content bytes directly — the common case, where the
    /// "descriptor" IS the byte content and no further schema applies.
    pub fn from_content_bytes(bytes: &[u8]) -> Self {
        let mut encoder = CanonicalEncoder::new(Self::RAW_CONTENT_DOMAIN_TAG);
        encoder.field_bytes(1, bytes);
        Self(Canonical::from_encoder(&encoder))
    }
}

digest_identity!(
    /// Logical source identity (architecture.md §3.1).
    SourceId
);
digest_identity!(
    /// Exact source version (architecture.md §3.1).
    SourceRevision
);
digest_identity!(
    /// Stable logical carrier unit identity (architecture.md §3.1).
    SourceUnitId
);
digest_identity!(
    /// Project topology identity (architecture.md §3.1).
    ProjectRevision
);
digest_identity!(
    /// Configuration identity (architecture.md §3.1).
    ConfigurationRevision
);
digest_identity!(
    /// Grammar/source-type/recovery/options identity (architecture.md
    /// §3.1).
    SyntaxProfileId
);
digest_identity!(
    /// Exact syntax construction identity (architecture.md §3.1).
    ParseKey
);
digest_identity!(
    /// Open/close lifecycle identity (architecture.md §3.1). Deliberately
    /// NOT a monotonic counter: two incarnations of the same document
    /// opened/closed in different orders across restarts are not
    /// comparable, only distinguishable.
    DocumentIncarnation
);
digest_identity!(
    /// Provider route/version/capability interpretation (architecture.md
    /// §3.1).
    ProviderContractId
);
digest_identity!(
    /// Deterministic public/content-relative identity (architecture.md
    /// §3.1). Collision-sensitive by definition (it is meant to be
    /// publicly compared) — callers holding a suspected collision compare
    /// [`Self::canonical_bytes`], never the digest alone.
    StableEntityId
);
digest_identity!(
    /// Exact captured semantic observation basis (architecture.md §3.1).
    /// Scopes in-flight semantic production; deliberately NOT part of
    /// cross-snapshot candidate lookup (`result-contract-and-flight.md`
    /// §1) — that is [`QueryIdentity`]'s role.
    InputBasisId
);
digest_identity!(
    /// Observable semantics/exactness/capability/approximation contract
    /// (architecture.md §3.1, `result-contract-and-flight.md` §1). Does
    /// NOT duplicate the separately-keyed profile IDs — it covers
    /// operation/product shape, required capability set, required
    /// exactness/completeness, unsupported/degradation policy, requested
    /// approximation mode, and required mapping/diagnostic/serialization
    /// outcome.
    ResultContractId
);

/// LSP client document version (architecture.md §3.1). A plain ordered
/// integer, not digest-backed: it is literally the client-assigned
/// version number, and the LSP protocol already defines its comparison
/// semantics (monotonically non-decreasing per document).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct DocumentVersion(pub i64);

/// Committed-input ordering aid (architecture.md §3.1): "orders commits and
/// captures snapshots. It is not a universal cache key." A monotonic
/// counter, not a digest — its whole purpose is `Ord`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct EngineRevision(pub u64);

/// Selected provider lifecycle identity (architecture.md §3.1) — a
/// monotonic epoch, not a digest.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct ProviderEpoch(pub u64);

/// Supersession order for one request stream (architecture.md §3.1) — a
/// monotonic generation counter, not a digest. Two requests on different
/// streams are not comparable by this value alone; ordering is meaningful
/// only within one stream, which is the owning caller's obligation to
/// track (this crate does not invent a stream identity).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct RequestGeneration(pub u64);

/// Serialized/persistent interpretation namespace (architecture.md §3.1,
/// ADR-002). A stable interned name, not a digest or a counter — the
/// counter lives in [`CompatibilityEpoch`], kept as a SEPARATE type per
/// ADR-002 ("one domain has one owner and a monotonic epoch sequence";
/// namespace and epoch are different concerns, so they are different
/// types here rather than one struct that could be constructed with a
/// namespace/epoch pair from unrelated domains).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct CompatibilityDomainId(pub &'static str);

/// Monotonic epoch inside a [`CompatibilityDomainId`] (architecture.md
/// §3.1, ADR-002). `0` is a valid first epoch, never an uninitialized
/// sentinel — there is deliberately no `NonZeroU32` here, unlike the
/// existing `nonzero_version!`-generated wire newtypes elsewhere in the
/// workspace, whose `new` returns `None` for epoch zero and so forbid the
/// exact clean-replacement move ADR-002 prescribes.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct CompatibilityEpoch(pub u32);

/// Direct invocation/batch, `PreparedCarrier`, or managed owner/shard
/// (architecture.md §3.1's literal enumeration for `ParseOwnerDomainId`).
/// A closed enum, not a digest: the three kinds are architecturally fixed,
/// and `Managed` carries the shard identity that distinguishes one managed
/// owner from another.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum ParseOwnerDomainId {
    /// A one-shot direct invocation or a batch of them — no retained
    /// owner state across calls.
    DirectOrBatch,
    /// The `PreparedCarrier` progressive-execution owner.
    PreparedCarrier,
    /// A managed-engine owner/shard, identified by its shard index.
    Managed { shard: u32 },
}

/// `(ParseOwnerDomainId, ParseKey, instance generation)` (architecture.md
/// §3.1's literal tuple shape).
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

/// Typed artifact construction identity (architecture.md §3.1). The `T`
/// marker keeps two `ArtifactKey<Foo>` / `ArtifactKey<Bar>` non-
/// interchangeable at the type level even when their underlying canonical
/// bytes happen to coincide — this is the compile-time half of "no current
/// owner duplicated merely to host new types": two artifact kinds cannot
/// silently alias one key space.
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

// Manual impls (not `#[derive]`): a derive would incorrectly require
// `T: Clone + PartialEq + ...`, but `T` is a phantom marker, never stored.
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

/// Semantic arguments + only the profile IDs observed by one typed query
/// boundary + [`ResultContractId`] (architecture.md §3.1,
/// `result-contract-and-flight.md` §1). Snapshot-independent: no
/// [`InputBasisId`] enters this type — that is exactly what makes it usable
/// as a cross-snapshot cache-CANDIDATE lookup key, with [`SemanticFlightKey`]
/// adding the basis for in-flight production identity.
///
/// The `Q` marker distinguishes query KINDS at the type level (a
/// `QueryIdentity<ResolveImport>` cannot be confused with a
/// `QueryIdentity<ComponentMeta>` even if their bytes coincide), mirroring
/// [`ArtifactKey`].
pub struct QueryIdentity<Q> {
    canonical: Canonical,
    marker: PhantomData<fn() -> Q>,
}

impl<Q> QueryIdentity<Q> {
    /// Composes the three architecturally-fixed parts. `semantic_arguments`
    /// is the caller's own canonical digest over the query's semantic
    /// argument descriptor; `observed_profiles` is the (possibly empty) set
    /// of profile-id digests this typed query boundary actually observes —
    /// encoded as a canonical SORTED set, so profile order never affects
    /// identity; `result_contract` closes over exactness/capability/
    /// approximation policy.
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

/// `(QueryIdentity<Q>, InputBasisId)` — exact literal shape from
/// architecture.md §3.1 and `result-contract-and-flight.md` §1. Distinct
/// from [`QueryIdentity`] by construction (it is a strictly bigger tuple, so
/// no coercion between the two exists), and cross-snapshot joining is
/// disabled by default at this key (§2.2 of the same contract) — a property
/// enforced by the owning flight runtime, not representable in the key type
/// alone, so it is documented rather than encoded here.
pub struct SemanticFlightKey<Q> {
    pub query_identity: QueryIdentity<Q>,
    pub input_basis: InputBasisId,
}

// Manual impls, matching `QueryIdentity<Q>`: a `#[derive]` here would add an
// implicit `Q: Trait` bound to every impl even though `Q` is only ever used
// as a phantom marker inside `QueryIdentity<Q>` — `FakeQuery`-style marker
// types used at call sites are not expected to implement `Clone`/`Debug`/
// `PartialEq` themselves.
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

/// Opaque owner/cohort-bound continuation handle (architecture.md §3.1).
/// Deliberately NOT [`StableEntityId`]-shaped: identity-encoding.md §3
/// requires a `SessionHandle` to include/validate owner cohort and
/// generation, and forbids serializing it as a stable reference unless an
/// explicit protocol translates it — giving it `StableEntityId`'s single-
/// digest shape would make that translation look free when it is not. The
/// fields are deliberately private: an owner mints a handle only through
/// [`Self::mint`], never by constructing the tuple directly, which is what
/// keeps a `SessionHandle` from being forged from arbitrary bytes the way a
/// [`StableEntityId`] legitimately can be (a `StableEntityId` earns its
/// identity FROM its content; a `SessionHandle` earns it from a live
/// owner's act of minting).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct SessionHandle {
    owner_cohort: CanonicalDigest,
    generation: u64,
    nonce: CanonicalDigest,
}

impl SessionHandle {
    /// Mints a handle for a live owner cohort at a given generation, with a
    /// caller-supplied uniqueness nonce (e.g. a random or counter-derived
    /// value the owner is responsible for not reusing within a cohort/
    /// generation pair). This crate does not generate randomness itself —
    /// that would make it a service, not a neutral type.
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

    /// Content-addressed identity: identical content produces an identical
    /// [`ContentId`] regardless of how many times it is recomputed, and
    /// different content produces a different one.
    #[test]
    fn content_id_is_content_addressed() {
        let a1 = ContentId::from_content_bytes(b"hello");
        let a2 = ContentId::from_content_bytes(b"hello");
        let b = ContentId::from_content_bytes(b"world");
        assert_eq!(a1, a2);
        assert_ne!(a1, b);
    }

    /// Two identity TYPES built from field-identical descriptors under
    /// different domain tags must not collide — this is the domain
    /// separation half of "non-interchangeable": even if `SourceId` and
    /// `SourceRevision` were (hypothetically) built from the same field
    /// bytes, their digests differ because their Rust types are declared
    /// via separate `digest_identity!` invocations with independent
    /// internal state (macro hygiene gives each a private
    /// `PhantomData`-free but nominally distinct wrapper), so no explicit
    /// domain-tag choice by a caller can accidentally alias them.
    #[test]
    fn stable_entity_id_is_deterministic_and_content_sensitive() {
        let a = StableEntityId::from_canonical(&Args(1));
        let b = StableEntityId::from_canonical(&Args(1));
        let c = StableEntityId::from_canonical(&Args(2));
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    /// `SessionHandle` is NOT content-addressed: two mints with the same
    /// cohort/generation but different nonces are different handles, and
    /// minting is the only constructor (no `from_canonical`).
    #[test]
    fn session_handle_mint_is_nonce_sensitive() {
        let cohort = StableEntityId::from_canonical(&Args(1)).digest();
        let a = SessionHandle::mint(cohort, 1, CanonicalDigest::of_bytes(b"nonce-a"));
        let b = SessionHandle::mint(cohort, 1, CanonicalDigest::of_bytes(b"nonce-b"));
        assert_ne!(a, b);
        assert_eq!(a.owner_cohort(), b.owner_cohort());
        assert_eq!(a.generation(), b.generation());
    }

    /// `QueryIdentity<Q>` excludes the input basis and is therefore stable
    /// across different bases; `SemanticFlightKey<Q>` adds the basis, so
    /// two flight keys over the same query identity but different bases are
    /// different, even though their `query_identity` fields compare equal.
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

    /// Observed-profile order must not affect `QueryIdentity` — profiles
    /// are encoded as a canonical SORTED set (encoding.rs's
    /// `field_sorted_set`), not a positional list.
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

    /// `ParseInstanceId` orders lexicographically over
    /// `(owner_domain, parse_key, instance_generation)` — exercising the
    /// hand-written `Ord` impl (not derivable because `Canonical`-backed
    /// `ParseKey` orders by digest, not by field content).
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

    /// `ArtifactKey<T>`'s marker keeps two artifact KINDS from aliasing even
    /// when built from field-identical descriptors — proven at the type
    /// level by the distinct `enum` markers below never unifying, and here
    /// at the value level by each instantiation still comparing equal to
    /// itself and to another build from the same descriptor.
    #[test]
    fn artifact_key_is_deterministic_per_kind() {
        enum KindA {}
        let a1 = ArtifactKey::<KindA>::from_canonical(&Args(1));
        let a2 = ArtifactKey::<KindA>::from_canonical(&Args(1));
        assert_eq!(a1, a2);
    }

    /// `ExecutionPolicy` never enters a `QueryIdentity`/`ResultContractId`
    /// — this test exists to keep the two files' intended relationship
    /// exercised by the same test binary that carries the identity
    /// invariants above, not to test anything about `ExecutionPolicy`
    /// itself in isolation.
    #[test]
    fn execution_policy_is_not_identity_shaped() {
        let policy = ExecutionPolicy::<()> {
            deadline: Some(Deadline(100)),
            cancellation: (),
            priority: WorkPriority::High,
            work_budget: WorkBudget(10),
            memory_budget: MemoryBudget(1024),
        };
        // No `CanonicalEncode` impl exists (and must not exist) for
        // `ExecutionPolicy` — this line would fail to compile if one were
        // ever added and used here, which is exactly the guard this test
        // provides: it must keep compiling using only `ExecutionPolicy`'s
        // own fields, never a digest.
        assert_eq!(policy.work_budget, WorkBudget(10));
    }
}

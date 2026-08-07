//! Resolution-world identity, facts, and sealed transaction products.
//!
//! This module is deliberately below the session cache substrate.  It owns
//! resolver observations and immutable world publication; session consumers
//! lower the transaction fact signature onto the existing
//! `FactVersionRef::ResolveImports` / `SignatureAdmission` rail.

use std::sync::Arc;

use im::{HashMap, HashSet};
use parking_lot::Mutex;

use crate::published_state::PublishedRoot;
use crate::types::{
    ExactResolution, ResolutionContext, ResolvePhase, ResolveRequestKind, ResolveResult,
};
use crate::{
    AggregateStamp, FactReadSet, FactVersionRef, FactVersionValidator, ResolveImportsFactRef,
    SignatureAdmission,
};

/// Effective bytes revision for one canonical.
///
/// The representation is private so it cannot be confused with a world,
/// fact, epoch, or legacy generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ContentRevision([u8; 16]);

/// Version of one resolution-observable fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ResolutionFactVersion(u64);

impl ResolutionFactVersion {
    pub(crate) const INITIAL: Self = Self(0);

    pub(crate) fn fresh(raw: u64) -> Self {
        assert_ne!(raw, 0, "resolution fact versions must be non-zero");
        Self(raw)
    }
}

/// Identity of one immutable resolution world.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ResolutionWorldId(u64);

impl ResolutionWorldId {
    pub(crate) fn fresh(raw: u64) -> Self {
        assert_ne!(raw, 0, "resolution world ids must be non-zero");
        Self(raw)
    }
}

/// Compare-before-publication epoch. Stable epochs are even.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ResolutionEpoch(u64);

impl ResolutionEpoch {
    pub(crate) fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    pub(crate) fn is_stable(self) -> bool {
        self.0.is_multiple_of(2)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CanonicalResolutionId(String);

impl CanonicalResolutionId {
    pub(crate) fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NormalizedSpecifier(String);

impl NormalizedSpecifier {
    pub(crate) fn new(value: impl Into<String>) -> Self {
        Self(normalize_specifier(&value.into()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RawSpecifier(String);

impl RawSpecifier {
    pub(crate) fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

fn normalize_specifier(value: &str) -> String {
    if value.starts_with('.')
        || value.starts_with('\\')
        || value.contains("/./")
        || value.contains("\\.\\")
    {
        crate::relative_path::normalize_relative_specifier(value)
    } else {
        value.replace('\\', "/")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProjectIdentity([u8; 16]);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ResolverPolicyIdentity([u8; 16]);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProviderPolicyIdentity([u8; 16]);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ResolveEnvHash([u8; 16]);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SessionFingerprint([u8; 16]);

impl SessionFingerprint {
    pub(crate) fn fresh(raw: u64) -> Self {
        assert_ne!(raw, 0, "session fingerprints must be non-zero");
        let mut bytes = [0_u8; 16];
        bytes[..8].copy_from_slice(&raw.to_le_bytes());
        bytes[8..].copy_from_slice(&(!raw).to_le_bytes());
        Self(bytes)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ResolutionPopulation {
    Base,
    Session(SessionFingerprint),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ResolutionEntry {
    Importer(CanonicalResolutionId),
    ExplicitProject(ProjectIdentity),
}

/// Structurally split selected resolution context.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ResolveContextId {
    project_identity: ProjectIdentity,
    resolver_policy_identity: ResolverPolicyIdentity,
    provider_policy_identity: ProviderPolicyIdentity,
    resolve_env_hash: ResolveEnvHash,
}

impl ResolveContextId {
    pub(crate) fn from_hashes(project_identity: [u8; 16], resolve_env_hash: [u8; 16]) -> Self {
        Self {
            project_identity: ProjectIdentity(project_identity),
            // These policy domains are intentionally distinct types even while
            // the current published resolver derives them from the same
            // project-scoped inputs.
            resolver_policy_identity: ResolverPolicyIdentity(resolve_env_hash),
            provider_policy_identity: ProviderPolicyIdentity(project_identity),
            resolve_env_hash: ResolveEnvHash(resolve_env_hash),
        }
    }

    /// Stable context identity for an entry no configured project owns.
    ///
    /// "No owning project" is a complete observation of the published
    /// project-selection index — a real observed value, not a missing
    /// observation. The identity is a fixed constant so it never derives
    /// from the project set: a project republish that does not change the
    /// entry's (non-)ownership must keep this context and its version.
    pub(crate) fn unowned() -> Self {
        Self::from_hashes([0xA1; 16], [0xA2; 16])
    }

    pub(crate) fn with_provider_projection(mut self, target: &Self) -> Self {
        self.provider_policy_identity = ProviderPolicyIdentity(target.project_identity.0);
        self
    }

    pub(crate) fn with_external_provider_projection(
        mut self,
        result: &crate::types::ResolveResult,
    ) -> Self {
        fn write_field(buffer: &mut Vec<u8>, value: &str) {
            buffer.extend_from_slice(&(value.len() as u64).to_le_bytes());
            buffer.extend_from_slice(value.as_bytes());
        }

        let mut identity = Vec::with_capacity(
            result.source_id.len()
                + result.provider_id.len()
                + result.provider_specifier.len()
                + 64,
        );
        identity.extend_from_slice(b"verter:external-provider-projection:v1");
        write_field(&mut identity, &result.source_id);
        write_field(&mut identity, &result.provider_id);
        write_field(&mut identity, &result.provider_specifier);
        identity.push(match result.provider_target {
            crate::types::ProviderTarget::SourceFile => 0,
            crate::types::ProviderTarget::CarrierPublicApi => 1,
            crate::types::ProviderTarget::ShadowSourceFile => 2,
        });
        identity.push(match result.resolution_kind {
            crate::types::ResolutionKind::Relative => 0,
            crate::types::ResolutionKind::TsConfigPath => 1,
            crate::types::ResolutionKind::ProjectReference => 2,
            crate::types::ResolutionKind::NodeModules => 3,
            crate::types::ResolutionKind::PackageExports => 4,
            crate::types::ResolutionKind::PackageImports => 5,
            crate::types::ResolutionKind::WorkspaceAlias => 6,
            crate::types::ResolutionKind::Bundler => 7,
            crate::types::ResolutionKind::PlaygroundMap => 8,
        });
        self.provider_policy_identity =
            ProviderPolicyIdentity(xxhash_rust::xxh3::xxh3_128(&identity).to_le_bytes());
        self
    }

    #[cfg(test)]
    pub(crate) fn identity_parts(&self) -> ([u8; 16], [u8; 16], [u8; 16]) {
        (
            self.project_identity.0,
            self.resolver_policy_identity.0,
            self.provider_policy_identity.0,
        )
    }
}

/// Complete semantic identity of one resolution demand.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ResolutionQueryKey {
    entry: ResolutionEntry,
    normalized_specifier: NormalizedSpecifier,
    phase: ResolvePhase,
    request_kind: ResolveRequestKind,
    context: ResolveContextId,
    population: ResolutionPopulation,
}

impl ResolutionQueryKey {
    pub(crate) fn importer(
        importer_id: &str,
        specifier: &str,
        context: ResolutionContext,
        selected: ResolveContextId,
        population: ResolutionPopulation,
    ) -> Self {
        Self {
            entry: ResolutionEntry::Importer(CanonicalResolutionId::new(importer_id)),
            normalized_specifier: NormalizedSpecifier::new(specifier),
            phase: context.phase,
            request_kind: context.kind,
            context: selected,
            population,
        }
    }

    pub(crate) fn explicit_project(
        project: ProjectIdentity,
        specifier: &str,
        context: ResolutionContext,
        selected: ResolveContextId,
        population: ResolutionPopulation,
    ) -> Self {
        Self {
            entry: ResolutionEntry::ExplicitProject(project),
            normalized_specifier: NormalizedSpecifier::new(specifier),
            phase: context.phase,
            request_kind: context.kind,
            context: selected,
            population,
        }
    }

    #[cfg(test)]
    pub(crate) fn context(&self) -> &ResolveContextId {
        &self.context
    }
}

/// Typed path-probe result. Error-tolerant states are not absence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PathProbe {
    File,
    Directory,
    Absent,
    Inaccessible,
    Unknown,
}

/// Immutable request-local overlay input for one Engine-owned resolution
/// batch. `Some(bytes)` is an upsert and `None` is an explicit tombstone.
#[derive(Debug, Clone, Default)]
pub struct ResolutionOverlaySnapshot {
    entries: Arc<HashMap<String, Option<Arc<str>>>>,
}

impl ResolutionOverlaySnapshot {
    #[must_use]
    pub fn new(
        upserts: impl IntoIterator<Item = (String, Arc<str>)>,
        tombstones: impl IntoIterator<Item = String>,
    ) -> Self {
        let mut entries = HashMap::new();
        for (canonical, source) in upserts {
            entries.insert(
                crate::resolver::normalize_canonical_id(&canonical),
                Some(source),
            );
        }
        for canonical in tombstones {
            entries.insert(crate::resolver::normalize_canonical_id(&canonical), None);
        }
        Self {
            entries: Arc::new(entries),
        }
    }

    fn get(&self, canonical_id: &str) -> Option<Option<Arc<str>>> {
        self.entries
            .get(&crate::resolver::normalize_canonical_id(canonical_id))
            .cloned()
    }
}

/// Closed resolution-input taxonomy.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ResolutionFactKey {
    PathProbe {
        canonical: CanonicalResolutionId,
        population: ResolutionPopulation,
    },
    Manifest {
        canonical: CanonicalResolutionId,
        population: ResolutionPopulation,
    },
    Realpath {
        requested: CanonicalResolutionId,
        population: ResolutionPopulation,
    },
    ExactResolution {
        entry: ResolutionEntry,
        specifier: RawSpecifier,
        phase: ResolvePhase,
        kind: ResolveRequestKind,
        population: ResolutionPopulation,
    },
    DirectoryMembers {
        canonical: CanonicalResolutionId,
        population: ResolutionPopulation,
    },
    RecoveryScope {
        canonical_prefix: CanonicalResolutionId,
        population: ResolutionPopulation,
    },
    ContextSelection {
        entry: ResolutionEntry,
        population: ResolutionPopulation,
    },
    /// **Derived node.** The resolution decision for one complete query
    /// identity.
    ///
    /// Its version is the single fact a consumer records instead of the
    /// query's whole transitive leaf set. Its direct dependency edges —
    /// the primitive facts the query itself observed plus the child
    /// decisions it reused — live in [`ResolutionFactRoot`], and a
    /// mutation reaching any of them advances this node through reverse
    /// propagation.
    Decision { query: Box<ResolutionQueryKey> },
    /// **Derived node.** One owner's complete set of direct resolution
    /// decisions.
    ///
    /// Records CHILD DECISIONS, never their leaves, so an owner witness
    /// is bounded by the owner's authored specifier count rather than by
    /// the transitive closure of everything those specifiers resolve
    /// through.
    OwnerResolutionSet {
        owner: CanonicalResolutionId,
        population: ResolutionPopulation,
    },
}

/// How one observation participates in the resolution decision DAG.
///
/// The classification is total over [`FactVersionRef`] (see
/// [`classify_resolution_observation`]), so a new fact variant cannot
/// compile until it has been given one of these three dispositions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResolutionEdgeClass {
    /// A primitive resolution input: a direct dependency edge of the
    /// enclosing decision.
    DirectLeaf,
    /// A derived DAG node reused by this query: a direct dependency edge,
    /// and itself a node whose own edges stay its own business. This is
    /// what replaces flattening a reused child's whole signature.
    DerivedNode,
    /// Not edge-bearing. The observation still roots the witness, but it
    /// stands for a whole compaction domain or for a producer identity
    /// space the resolution root does not own, so it never becomes a
    /// graph edge.
    Terminal,
}

/// **The one observation-to-edge classification.**
///
/// Exhaustive over [`FactVersionRef`] and, inside the resolution arm,
/// over [`ResolutionFactKey`] — no wildcard on either. Adding a variant
/// to either enum is a compile error until its direct-leaf / derived-node
/// / terminal disposition is stated here, which is what makes witness
/// creation and edge classification one operation rather than two that
/// can drift.
pub(crate) fn classify_resolution_observation(fact: &FactVersionRef) -> ResolutionEdgeClass {
    match fact {
        FactVersionRef::ResolveImports(ResolveImportsFactRef::Resolution(fact)) => {
            match &fact.key {
                ResolutionFactKey::PathProbe { .. }
                | ResolutionFactKey::Manifest { .. }
                | ResolutionFactKey::Realpath { .. }
                | ResolutionFactKey::ExactResolution { .. }
                | ResolutionFactKey::DirectoryMembers { .. }
                | ResolutionFactKey::RecoveryScope { .. }
                | ResolutionFactKey::ContextSelection { .. } => ResolutionEdgeClass::DirectLeaf,
                ResolutionFactKey::Decision { .. }
                | ResolutionFactKey::OwnerResolutionSet { .. } => ResolutionEdgeClass::DerivedNode,
            }
        }
        // Another producer's identity space, or an already-terminal
        // whole-domain aggregate. Both root the witness; neither is a
        // resolution-graph edge.
        FactVersionRef::ResolveImports(ResolveImportsFactRef::Semantic { .. })
        | FactVersionRef::FileWholeHash { .. }
        | FactVersionRef::DerivedFactHash { .. }
        | FactVersionRef::Parse(_)
        | FactVersionRef::FileSourceEnv { .. }
        | FactVersionRef::RouteSurface(_)
        | FactVersionRef::ProjectGeneration { .. }
        | FactVersionRef::DomainGeneration(_)
        | FactVersionRef::StrictSelfRootWorld(_) => ResolutionEdgeClass::Terminal,
    }
}

impl ResolutionFactKey {
    /// Build the derived node key for one query identity.
    pub(crate) fn decision(query: ResolutionQueryKey) -> Self {
        Self::Decision {
            query: Box::new(query),
        }
    }

    /// Build the derived node key for one owner's decision set.
    pub(crate) fn owner_resolution_set(owner: &str, population: ResolutionPopulation) -> Self {
        Self::OwnerResolutionSet {
            owner: CanonicalResolutionId::new(crate::resolver::normalize_canonical_id(owner)),
            population,
        }
    }

    /// Whether this key names a derived DAG node rather than a primitive
    /// resolution input.
    pub(crate) fn is_derived_node(&self) -> bool {
        matches!(
            self,
            Self::Decision { .. } | Self::OwnerResolutionSet { .. }
        )
    }

    pub(crate) fn population(&self) -> ResolutionPopulation {
        match self {
            Self::PathProbe { population, .. }
            | Self::Manifest { population, .. }
            | Self::Realpath { population, .. }
            | Self::ExactResolution { population, .. }
            | Self::DirectoryMembers { population, .. }
            | Self::RecoveryScope { population, .. }
            | Self::ContextSelection { population, .. }
            | Self::OwnerResolutionSet { population, .. } => *population,
            // A decision's population is its query's: the query identity
            // already carries one, and two populations for one node
            // would be two answers to the same question.
            Self::Decision { query } => query.population,
        }
    }

    pub(crate) fn in_population(&self, population: ResolutionPopulation) -> Self {
        let mut key = self.clone();
        match &mut key {
            Self::PathProbe {
                population: current,
                ..
            }
            | Self::Manifest {
                population: current,
                ..
            }
            | Self::Realpath {
                population: current,
                ..
            }
            | Self::ExactResolution {
                population: current,
                ..
            }
            | Self::DirectoryMembers {
                population: current,
                ..
            }
            | Self::RecoveryScope {
                population: current,
                ..
            }
            | Self::ContextSelection {
                population: current,
                ..
            }
            | Self::OwnerResolutionSet {
                population: current,
                ..
            } => *current = population,
            Self::Decision { query } => query.population = population,
        }
        key
    }

    pub(crate) fn canonical_id(&self) -> Option<&str> {
        match self {
            Self::PathProbe { canonical, .. }
            | Self::Manifest { canonical, .. }
            | Self::DirectoryMembers { canonical, .. } => Some(&canonical.0),
            Self::Realpath { requested, .. } => Some(&requested.0),
            Self::RecoveryScope {
                canonical_prefix, ..
            } => Some(&canonical_prefix.0),
            Self::OwnerResolutionSet { owner, .. } => Some(&owner.0),
            Self::ExactResolution { entry, .. } | Self::ContextSelection { entry, .. } => {
                match entry {
                    ResolutionEntry::Importer(canonical) => Some(&canonical.0),
                    ResolutionEntry::ExplicitProject(_) => None,
                }
            }
            Self::Decision { query } => match &query.entry {
                ResolutionEntry::Importer(canonical) => Some(&canonical.0),
                ResolutionEntry::ExplicitProject(_) => None,
            },
        }
    }

    /// The owner a `Decision` belongs to, for the owner-set index.
    fn owner_canonical(&self) -> Option<&str> {
        match self {
            Self::Decision { query } => match &query.entry {
                ResolutionEntry::Importer(canonical) => Some(&canonical.0),
                ResolutionEntry::ExplicitProject(_) => None,
            },
            _ => None,
        }
    }

    /// The canonical whose CURRENT DISK STATE is this fact's value, if any.
    ///
    /// Only these three families can be re-observed by re-reading a path:
    /// the typed probe, the realpath, and the manifest fingerprint. A
    /// `RecoveryScope` names an ancestor PREFIX (up to and including `/`) and
    /// is advanced only by an imprecise watcher mutation; `DirectoryMembers`
    /// is advanced through the parent of a path that moved;
    /// `ExactResolution` and `ContextSelection` are table lookups, not disk
    /// reads. Treating any of those as a path to re-read would enumerate
    /// directories the resolver never consulted.
    ///
    /// Exhaustive by construction: a new fact family cannot compile until it
    /// declares which side of this line it is on.
    pub(crate) fn reobservable_path_canonical_id(&self) -> Option<&str> {
        match self {
            Self::PathProbe { canonical, .. } | Self::Manifest { canonical, .. } => {
                Some(&canonical.0)
            }
            Self::Realpath { requested, .. } => Some(&requested.0),
            Self::ExactResolution { .. }
            | Self::DirectoryMembers { .. }
            | Self::RecoveryScope { .. }
            | Self::ContextSelection { .. }
            // A derived node is computed, never read off a path. Handing
            // one to the re-observation walk would re-read a canonical
            // the resolver never probed.
            | Self::Decision { .. }
            | Self::OwnerResolutionSet { .. } => None,
        }
    }

    pub(crate) fn exact_importer(
        importer_id: &str,
        specifier: &str,
        context: ResolutionContext,
        population: ResolutionPopulation,
    ) -> Self {
        Self::ExactResolution {
            entry: ResolutionEntry::Importer(CanonicalResolutionId::new(importer_id)),
            specifier: RawSpecifier::new(specifier),
            phase: context.phase,
            kind: context.kind,
            population,
        }
    }

    pub(crate) fn context_importer(importer_id: &str, population: ResolutionPopulation) -> Self {
        Self::ContextSelection {
            entry: ResolutionEntry::Importer(CanonicalResolutionId::new(importer_id)),
            population,
        }
    }

    pub(crate) fn exact_explicit(
        project: ProjectIdentity,
        specifier: &str,
        context: ResolutionContext,
        population: ResolutionPopulation,
    ) -> Self {
        Self::ExactResolution {
            entry: ResolutionEntry::ExplicitProject(project),
            specifier: RawSpecifier::new(specifier),
            phase: context.phase,
            kind: context.kind,
            population,
        }
    }

    pub(crate) fn context_explicit(
        project: ProjectIdentity,
        population: ResolutionPopulation,
    ) -> Self {
        Self::ContextSelection {
            entry: ResolutionEntry::ExplicitProject(project),
            population,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ResolutionFactRef {
    pub(crate) key: ResolutionFactKey,
    pub(crate) version: ResolutionFactVersion,
}

impl ResolutionFactRef {
    pub(crate) fn new(key: ResolutionFactKey, version: ResolutionFactVersion) -> Self {
        Self { key, version }
    }

    /// Whether this ref names the OWNER-SCOPED resolution set.
    ///
    /// The read-only oracle a consumer crate needs to assert what it
    /// rooted on: the key's own shape stays crate-private, so this is the
    /// only way to ask the question from outside without exposing the
    /// taxonomy.
    #[must_use]
    pub fn is_owner_resolution_set(&self) -> bool {
        matches!(self.key, ResolutionFactKey::OwnerResolutionSet { .. })
    }

    /// Whether this ref names one query's derived decision node.
    ///
    /// Consumer crates use this read-only oracle to verify that a witness
    /// carries bounded DAG nodes without exposing the key's private fields.
    #[must_use]
    pub fn is_decision(&self) -> bool {
        matches!(self.key, ResolutionFactKey::Decision { .. })
    }
}

/// Version ledger for one immutable resolution root.
///
/// The map is point-lookup only — `version` / `advance` / `remove`, never an
/// iteration — so its hasher is free to be chosen for speed. Validating one
/// warm candidate's witness is a `version` lookup per recorded fact, and a
/// `ResolutionFactKey` carries a full canonical path, so the hash of that path
/// is charged on every fact of every candidate of every resolve. Nothing about
/// the ledger's contents, ordering, or the signatures derived from it depends
/// on which hasher produced the buckets: witness canonicalisation orders facts
/// structurally, never by hash.
type FactVersionLedger =
    HashMap<ResolutionFactKey, ResolutionFactVersion, rustc_hash::FxBuildHasher>;

/// One derived node's edge set, or one dependency's dependent set.
type ResolutionEdgeSet = HashSet<ResolutionFactKey, rustc_hash::FxBuildHasher>;

/// Persistent adjacency. `im` maps are HAMTs, so cloning a root shares
/// their nodes structurally and a mutation costs
/// `O(changed keys × log n)` — the bound the decision DAG is sized
/// against.
type ResolutionEdgeMap = HashMap<ResolutionFactKey, ResolutionEdgeSet, rustc_hash::FxBuildHasher>;

#[derive(Debug, Clone, Default)]
pub(crate) struct ResolutionFactRoot {
    versions: FactVersionLedger,
    /// derived node → its COMPLETE direct dependency set.
    forward: ResolutionEdgeMap,
    /// dependency → the derived nodes that directly depend on it.
    reverse: ResolutionEdgeMap,
    /// owner canonical + population -> that owner's published child
    /// decisions. The index exists so an owner-set publication is
    /// `O(owner's decisions)` rather than a scan of the whole ledger.
    owner_decisions:
        HashMap<(String, ResolutionPopulation), ResolutionEdgeSet, rustc_hash::FxBuildHasher>,
    /// Direct fact keys advanced since the enclosing mutation batch
    /// began, drained by [`Self::take_pending_seeds`] at the publication
    /// protocol's propagation step.
    ///
    /// Always empty in a PUBLISHED root: the publication protocol drains
    /// it before the root is stored, so a captured world never carries
    /// mutation-batch scratch state.
    pending_seeds: Vec<ResolutionFactKey>,
}

impl ResolutionFactRoot {
    pub(crate) fn version(&self, key: &ResolutionFactKey) -> ResolutionFactVersion {
        self.versions
            .get(key)
            .copied()
            .unwrap_or(ResolutionFactVersion::INITIAL)
    }

    /// Semantic advance: the fact's observed meaning moved. Records the
    /// key as a propagation seed for the enclosing mutation batch.
    pub(crate) fn advance(&mut self, key: ResolutionFactKey, version: ResolutionFactVersion) {
        self.versions.insert(key.clone(), version);
        self.pending_seeds.push(key);
    }

    /// Copy a base version DOWN into a session root without claiming a
    /// semantic change.
    ///
    /// Deliberately NOT [`Self::advance`]. The composed session view
    /// already answered this key from the base root, so mirroring the
    /// value changes no witness's meaning — seeding propagation from it
    /// would advance every session decision under an unchanged fact on
    /// every overlay open.
    pub(crate) fn mirror_base_version(
        &mut self,
        key: ResolutionFactKey,
        version: ResolutionFactVersion,
    ) {
        self.versions.insert(key, version);
    }

    pub(crate) fn remove(&mut self, key: &ResolutionFactKey) {
        self.versions.remove(key);
        self.pending_seeds.push(key.clone());
    }

    /// **Atomic direct-edge replacement.**
    ///
    /// The node's COMPLETE prior edge set is detached from both maps and
    /// the new one attached in the same mutation, so no reader can ever
    /// observe a node holding a mixture of two computations' edges.
    ///
    /// Publication does NOT mint a version, and that is a correctness
    /// choice. A resolution publishes its own decision, so a minted
    /// version would be one no view captured before that resolution can
    /// hold — every consumer rooting on it would miss against the very
    /// request view it computed under, which is the reuse the DAG exists
    /// to enable. A never-published node reads
    /// [`ResolutionFactVersion::INITIAL`], exactly like every other fact
    /// nothing has advanced, and its version moves only when something
    /// genuinely invalidates it: reverse propagation from a mutated
    /// dependency, or [`Self::remove_derived`].
    ///
    /// Returns whether an EXISTING node's edges were replaced.
    pub(crate) fn publish_derived(
        &mut self,
        node: ResolutionFactKey,
        dependencies: impl IntoIterator<Item = ResolutionFactKey>,
    ) -> bool {
        debug_assert!(
            node.is_derived_node(),
            "only a derived DAG node carries direct edges"
        );
        let replaced = self.forward.contains_key(&node);
        self.detach_edges(&node);
        let mut direct = ResolutionEdgeSet::default();
        for dependency in dependencies {
            // A node is never its own dependency: a recompute of Q that
            // reuses Q's own prior answer is the SAME decision, not a
            // child of itself, and a self-edge would make propagation
            // advance the node that seeded it.
            if dependency == node {
                continue;
            }
            self.reverse
                .entry(dependency.clone())
                .or_default()
                .insert(node.clone());
            direct.insert(dependency);
        }
        if let Some(owner) = node.owner_canonical() {
            self.owner_decisions
                .entry((owner.to_owned(), node.population()))
                .or_default()
                .insert(node.clone());
        }
        self.forward.insert(node, direct);
        replaced
    }

    /// Drop a derived node: ADVANCE its version, then drop its complete
    /// edge set in both directions.
    ///
    /// The advance is what prevents ABA. A node that fell out of the
    /// graph and was later republished must not return to a version a
    /// witness already holds, and publication mints nothing — so the
    /// removal itself is where the tombstone version is minted. Every
    /// witness recorded against the node, at any prior version including
    /// `INITIAL`, stops validating from here on; a reintroduction keeps
    /// the tombstone rather than reverting.
    ///
    /// Nothing is evicted. A dependent cache entry stays exactly where it
    /// is and goes cold only when its own recorded derived version fails
    /// ordinary read-side validation.
    pub(crate) fn remove_derived(
        &mut self,
        node: &ResolutionFactKey,
        version: ResolutionFactVersion,
    ) -> bool {
        if !self.forward.contains_key(node) {
            return false;
        }
        self.detach_edges(node);
        self.forward.remove(node);
        if let Some(owner) = node.owner_canonical() {
            let index_key = (owner.to_owned(), node.population());
            let empty = match self.owner_decisions.get_mut(&index_key) {
                Some(decisions) => {
                    decisions.remove(node);
                    decisions.is_empty()
                }
                None => false,
            };
            if empty {
                self.owner_decisions.remove(&index_key);
            }
        }
        self.advance(node.clone(), version);
        true
    }

    fn detach_edges(&mut self, node: &ResolutionFactKey) {
        let Some(previous) = self.forward.get(node).cloned() else {
            return;
        };
        for dependency in previous {
            let empty = match self.reverse.get_mut(&dependency) {
                Some(dependents) => {
                    dependents.remove(node);
                    dependents.is_empty()
                }
                None => false,
            };
            if empty {
                self.reverse.remove(&dependency);
            }
        }
    }

    /// One owner's currently published child decisions.
    pub(crate) fn owner_child_decisions(
        &self,
        owner: &str,
        population: ResolutionPopulation,
    ) -> Vec<ResolutionFactKey> {
        self.owner_decisions
            .get(&(crate::resolver::normalize_canonical_id(owner), population))
            .map(|decisions| decisions.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// One derived node's complete direct dependency set.
    pub(crate) fn direct_dependencies(
        &self,
        node: &ResolutionFactKey,
    ) -> Option<Vec<ResolutionFactKey>> {
        self.forward
            .get(node)
            .map(|deps| deps.iter().cloned().collect())
    }

    /// Every `ContextSelection` key some derived node depends on.
    ///
    /// The REGISTERED leaves the publication path enumerates for changed
    /// selections. Bounded by the graph's own context edges — never by
    /// the project set, and never by the whole fact ledger.
    pub(crate) fn registered_context_selection_keys(&self) -> Vec<ResolutionFactKey> {
        self.reverse
            .keys()
            .filter(|key| matches!(key, ResolutionFactKey::ContextSelection { .. }))
            .cloned()
            .collect()
    }

    /// Seed this mutation batch's propagation from `key` without claiming
    /// that `key`'s own version moved.
    ///
    /// `ContextSelection` is versioned in the separate `context_versions`
    /// map rather than in the fact ledger, so a publication that changes
    /// an entry's selected context advances no ledger entry to seed from.
    pub(crate) fn seed_propagation(&mut self, key: ResolutionFactKey) {
        self.pending_seeds.push(key);
    }

    /// The derived nodes directly depending on `key`.
    #[cfg(test)]
    pub(crate) fn direct_dependents(&self, key: &ResolutionFactKey) -> Vec<ResolutionFactKey> {
        self.reverse
            .get(key)
            .map(|dependents| dependents.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Drain the direct fact keys this mutation batch advanced.
    pub(crate) fn take_pending_seeds(&mut self) -> Vec<ResolutionFactKey> {
        std::mem::take(&mut self.pending_seeds)
    }

    /// **Reverse-reachable derived propagation, once per node per batch.**
    ///
    /// Advances every `Decision` / `OwnerResolutionSet` reachable from
    /// `seeds` through reverse edges, exactly once, and returns them.
    /// Termination follows from the batch-local visited set over a finite
    /// node set, so a cycle among derived nodes terminates without any
    /// depth bound; the event-minted fresh versions are what make
    /// "advance once per batch" the CORRECT number, not the reason the
    /// walk halts.
    ///
    /// Nothing is evicted. A dependent cache entry stays exactly where it
    /// is and goes cold only when its own recorded derived version fails
    /// ordinary read-side validation.
    pub(crate) fn propagate(
        &mut self,
        seeds: impl IntoIterator<Item = ResolutionFactKey>,
        mut fresh_version: impl FnMut() -> ResolutionFactVersion,
    ) -> Vec<ResolutionFactKey> {
        let mut queue: std::collections::VecDeque<ResolutionFactKey> = seeds.into_iter().collect();
        let mut visited: rustc_hash::FxHashSet<ResolutionFactKey> = queue.iter().cloned().collect();
        let mut advanced = Vec::new();
        while let Some(key) = queue.pop_front() {
            let Some(dependents) = self.reverse.get(&key).cloned() else {
                continue;
            };
            for dependent in dependents {
                if !visited.insert(dependent.clone()) {
                    continue;
                }
                self.versions.insert(dependent.clone(), fresh_version());
                advanced.push(dependent.clone());
                queue.push_back(dependent);
            }
        }
        advanced
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ResolutionSessionRoot {
    pub(crate) id: ResolutionWorldId,
    pub(crate) facts: ResolutionFactRoot,
    pub(crate) overlay_paths: HashSet<String>,
    pub(crate) manifest_fingerprints: HashMap<String, [u8; 16]>,
}

impl ResolutionSessionRoot {
    pub(crate) fn bootstrap(id: ResolutionWorldId) -> Self {
        Self {
            id,
            facts: ResolutionFactRoot::default(),
            overlay_paths: HashSet::new(),
            manifest_fingerprints: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ExactResolutionKey {
    importer_id: String,
    specifier: String,
    phase: ResolvePhase,
    kind: ResolveRequestKind,
}

impl ExactResolutionKey {
    fn new(importer_id: &str, specifier: &str, context: ResolutionContext) -> Self {
        Self {
            importer_id: importer_id.to_owned(),
            specifier: specifier.to_owned(),
            phase: context.phase,
            kind: context.kind,
        }
    }
}

/// Immutable, Arc-published composition used by every Engine transaction.
#[derive(Debug, Clone)]
pub(crate) struct ResolutionWorldRoot {
    pub(crate) id: ResolutionWorldId,
    pub(crate) published: Option<Arc<PublishedRoot>>,
    pub(crate) facts: ResolutionFactRoot,
    pub(crate) path_probes: HashMap<String, PathProbe>,
    /// Recorded base realpath value per requested canonical. `Some(None)`
    /// means the path is known to have no realpath; a missing entry means
    /// the value has never been observed and comparisons must stay
    /// conservative.
    pub(crate) realpaths: HashMap<String, Option<String>>,
    /// Recorded base manifest fingerprint per canonical, on the same shape as
    /// [`Self::realpaths`]: `Some(None)` means the manifest is known to be
    /// absent, a missing entry means never observed. The distinction is
    /// load-bearing — without it a manifest that has never been observed and
    /// one observed as absent are the same state, so the appearance of a
    /// `package.json` reads as a first observation and advances nothing.
    pub(crate) manifest_fingerprints: HashMap<String, Option<[u8; 16]>>,
    context_versions: HashMap<ResolveContextId, ResolutionFactVersion>,
    exact_resolutions: HashMap<ExactResolutionKey, ExactResolution>,
}

impl ResolutionWorldRoot {
    pub(crate) fn bootstrap(id: ResolutionWorldId) -> Self {
        Self {
            id,
            published: None,
            facts: ResolutionFactRoot::default(),
            path_probes: HashMap::new(),
            realpaths: HashMap::new(),
            manifest_fingerprints: HashMap::new(),
            context_versions: HashMap::new(),
            exact_resolutions: HashMap::new(),
        }
    }

    /// Replace the persistent project-selection index while retaining versions
    /// only for context nodes whose complete semantic value remains present.
    /// A context that disappears and is later reintroduced receives a fresh,
    /// globally unique typed version, so a publish cycle cannot create ABA.
    /// A publication can change which context an entry SELECTS without
    /// touching any project's own version, so the registered
    /// `ContextSelection` leaves are enumerated across the swap and the
    /// changed ones seed derived propagation.
    ///
    /// The enumeration costs
    /// `O(registered ContextSelection leaves × one membership evaluation)`
    /// and runs only at publish time. [`PublishedContextSelection`] cannot
    /// reduce it, by construction and by design. By construction: `published`
    /// is a brand-new index whose memo is empty, and every registered leaf
    /// is a distinct path, so each post-swap comparison is a genuine first
    /// walk. By design: a memo that DID answer here would answer with the
    /// previous index's selection — exactly the stale answer that would
    /// make a changed selection compare equal and seed nothing.
    ///
    /// The pre-swap half reads the OUTGOING index and may hit its memo.
    /// That is correct: the old index's answers are still answers about
    /// the old index.
    ///
    /// `also_registered` carries the context leaves registered in the
    /// LIVE SESSION graphs, normalised to the base population. A session
    /// decision's context edge lives in its own root, so a base-local
    /// enumeration would see none of them and a context change would
    /// silently miss every session decision; the seeds are recorded in
    /// the base root and the publication protocol's session fan-out
    /// translates them back.
    pub(crate) fn replace_published(
        &mut self,
        published: Arc<PublishedRoot>,
        also_registered: &[ResolutionFactKey],
        mut fresh_version: impl FnMut() -> ResolutionFactVersion,
    ) {
        let mut registered = self.facts.registered_context_selection_keys();
        registered.extend(also_registered.iter().cloned());
        registered.sort();
        registered.dedup();
        let before: Vec<ResolutionFactVersion> = registered
            .iter()
            .map(|key| self.fact_version(key))
            .collect();

        let mut next_context_versions = HashMap::new();
        let contexts = std::iter::once(ResolveContextId::unowned()).chain(
            published
                .snapshot
                .projects
                .iter()
                .filter_map(|project| context_for_project(&published, project).ok()),
        );
        for context in contexts {
            let version = self
                .context_versions
                .get(&context)
                .copied()
                .unwrap_or_else(&mut fresh_version);
            next_context_versions.insert(context, version);
        }
        self.context_versions = next_context_versions;
        self.published = Some(published);

        for (key, was) in registered.into_iter().zip(before) {
            if self.fact_version(&key) != was {
                self.facts.seed_propagation(key);
            }
        }
    }

    pub(crate) fn exact(
        &self,
        importer_id: &str,
        specifier: &str,
        context: ResolutionContext,
    ) -> Option<&ExactResolution> {
        self.exact_resolutions
            .get(&ExactResolutionKey::new(importer_id, specifier, context))
    }

    pub(crate) fn replace_owner_exacts(
        &mut self,
        importer_id: &str,
        resolutions: &[ExactResolution],
    ) {
        self.exact_resolutions
            .retain(|key, _| key.importer_id != importer_id);
        for resolution in resolutions {
            self.exact_resolutions.insert(
                ExactResolutionKey {
                    importer_id: importer_id.to_owned(),
                    specifier: resolution.specifier.clone(),
                    phase: resolution.phase,
                    kind: resolution.kind,
                },
                resolution.clone(),
            );
        }
    }

    pub(crate) fn owner_exact_fact_keys(&self, importer_id: &str) -> Vec<ResolutionFactKey> {
        self.exact_resolutions
            .keys()
            .filter(|key| key.importer_id == importer_id)
            .map(|key| {
                ResolutionFactKey::exact_importer(
                    importer_id,
                    &key.specifier,
                    ResolutionContext {
                        phase: key.phase,
                        kind: key.kind,
                    },
                    ResolutionPopulation::Base,
                )
            })
            .collect()
    }

    pub(crate) fn exact_owners_under(&self, prefix: &str) -> Vec<String> {
        let mut owners = self
            .exact_resolutions
            .keys()
            .filter(|key| crate::path_matches_prefix(&key.importer_id, prefix))
            .map(|key| key.importer_id.clone())
            .collect::<Vec<_>>();
        owners.sort();
        owners.dedup();
        owners
    }

    pub(crate) fn owner_exacts_equal(
        &self,
        importer_id: &str,
        resolutions: &[ExactResolution],
    ) -> bool {
        let stored = self
            .exact_resolutions
            .iter()
            .filter(|(key, _)| key.importer_id == importer_id)
            .collect::<Vec<_>>();
        if stored.len() != resolutions.len() {
            return false;
        }
        resolutions.iter().all(|resolution| {
            self.exact_resolutions.get(&ExactResolutionKey {
                importer_id: importer_id.to_owned(),
                specifier: resolution.specifier.clone(),
                phase: resolution.phase,
                kind: resolution.kind,
            }) == Some(resolution)
        })
    }
}

/// Genuine provenance gaps: an observation the captured immutable root
/// cannot complete. "No owning project" is deliberately NOT here — a
/// complete read of a complete index returning "none" is a real observed
/// value and selects [`ResolveContextId::unowned`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContextProvenanceError {
    NoPublishedRoot,
    ProjectProjectionMissing,
    ProjectIdentityMissing,
    ResolveEnvironmentMissing,
}

/// Maximum number of per-path context-membership rows one published index
/// retains before an overflowing insert clears the table.
///
/// Same shape and rationale as [`crate::workspace_snapshot::OWNERS_MEMO_CAP`]:
/// the rows are a cost mechanism, never an authority, so dropping them is
/// always correct and costs only a recompute. A clear is COUNTED
/// ([`PublishedContextSelection::table_clears`]) because it also restarts the
/// per-path tally, and a tally that silently restarted would read
/// "evaluated once" for a path this index evaluated many times.
pub(crate) const CONTEXT_MEMBERSHIP_TABLE_CAP: usize = 16 * 1024;

/// One published index's record for one canonical path.
#[derive(Debug, Default)]
struct ContextMembershipRow {
    /// Membership walks this index performed for the row's path.
    ///
    /// A true tally, not a memo-presence flag: it counts walks, so a
    /// memo that computed an answer and then failed to keep it makes this
    /// climb instead of resting at one.
    evaluations: u64,
    /// The memoized answer, TYPED: a provenance error and an unowned
    /// selection are recorded exactly as computed, never collapsed into
    /// absence and never retried per demand.
    selected: Option<Result<ResolveContextId, ContextProvenanceError>>,
}

/// One published index's context-selection memo.
///
/// [`selected_context_for_path`] is a pure function of the published index
/// and the requested canonical id: it reads the index's resolver
/// membership, its project list and its two per-project tables, and
/// nothing else. Its answer is therefore valid for exactly one index's
/// lifetime, which is why this memo lives ON [`PublishedRoot`] — an index
/// that is replaced takes its answers with it, and there is no
/// cross-generation invalidation to get wrong.
///
/// It is deliberately not owned by `ResolutionWorldRoot`, which is cloned
/// on every mutation, and not by `WorkspaceSnapshot`, which an LSP
/// view-only rebuild reuses across a [`PublishedRoot`] whose per-project
/// identity and environment tables were recomposed — the selected context
/// depends on those tables, so a snapshot-scoped answer could outlive its
/// own inputs.
///
/// # What it cannot do
///
/// - **It cannot reduce the publish-time context enumeration.**
///   `ResolutionWorldRoot::replace_published` compares every registered
///   `ContextSelection` leaf across the swap, and the index it compares
///   against is brand new, so its memo is empty by construction. Each
///   registered leaf is a distinct path and therefore a genuine first
///   walk. That enumeration stays `O(registered leaves × one walk)`, and
///   a memo that DID answer there would answer with the previous index's
///   selection — the exact stale answer that makes a changed selection
///   invisible to propagation.
/// - **It does not cover the direct membership callers.** The memo sits
///   at `selected_context_for_path`. `ProjectResolver::nearest_config_for_path`
///   and `effective_configs_for_path` are also called directly — seven
///   times inside `resolver.rs` itself and five times from `verter_lsp`
///   (`provider_sync.rs` ×2, `background_drain_decl_closure.rs`,
///   `background_init.rs`, `server_utils.rs`). Those walk the resolver
///   unmemoized. Extending the memo to them needs their own measurement:
///   they take a different value out of the walk (`&IdeProjectConfig`, a
///   candidate list), not a `ResolveContextId`.
pub(crate) struct PublishedContextSelection {
    rows: dashmap::DashMap<Box<str>, ContextMembershipRow>,
    cap: usize,
    table_clears: std::sync::atomic::AtomicU64,
}

impl PublishedContextSelection {
    /// Service bounded at `cap` rows. Production uses
    /// [`CONTEXT_MEMBERSHIP_TABLE_CAP`] via [`Default`]; the overflow test
    /// uses a tiny cap.
    pub(crate) fn with_cap(cap: usize) -> Self {
        Self {
            rows: dashmap::DashMap::new(),
            cap,
            table_clears: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// Membership walks this index performed for `canonical_id`.
    pub(crate) fn evaluations(&self, canonical_id: &str) -> u64 {
        self.rows
            .get(canonical_id)
            .map(|row| row.evaluations)
            .unwrap_or(0)
    }

    /// How often the row table was cleared for capacity. Non-zero means
    /// [`Self::evaluations`] counts only since the last clear.
    pub(crate) fn table_clears(&self) -> u64 {
        self.table_clears.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// This index's selection for `canonical_id`, walking membership only
    /// on the first demand.
    ///
    /// `evaluate` runs under the row's write guard, so concurrent demands
    /// for one path collapse onto one walk. It must not re-enter this
    /// memo — the membership walk reads published state only, and holds
    /// no row guard of its own.
    fn selected(
        &self,
        canonical_id: &str,
        evaluate: impl FnOnce() -> Result<ResolveContextId, ContextProvenanceError>,
    ) -> Result<ResolveContextId, ContextProvenanceError> {
        if let Some(row) = self.rows.get(canonical_id) {
            if let Some(selected) = row.selected.as_ref() {
                return selected.clone();
            }
        }
        // The capacity clear takes every shard: no row guard may be held.
        if self.rows.len() >= self.cap {
            self.rows.clear();
            self.table_clears
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        let mut row = self.rows.entry(Box::from(canonical_id)).or_default();
        if let Some(selected) = row.selected.as_ref() {
            return selected.clone();
        }
        let selected = evaluate();
        row.evaluations += 1;
        row.selected = Some(selected.clone());
        selected
    }
}

impl Default for PublishedContextSelection {
    fn default() -> Self {
        Self::with_cap(CONTEXT_MEMBERSHIP_TABLE_CAP)
    }
}

impl std::fmt::Debug for PublishedContextSelection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PublishedContextSelection")
            .field("rows", &self.rows.len())
            .field("cap", &self.cap)
            .field("table_clears", &self.table_clears())
            .finish()
    }
}

fn project_for_config<'a>(
    published: &'a PublishedRoot,
    config: &crate::resolver::IdeProjectConfig,
) -> Option<&'a crate::workspace_snapshot::OwnershipProject> {
    let root = crate::resolver::normalize_canonical_id(&config.root);
    let workspace_root = crate::resolver::normalize_canonical_id(&config.workspace_root);
    let tsconfig = config
        .tsconfig_path
        .as_deref()
        .map(crate::resolver::normalize_canonical_id);
    published.snapshot.projects.iter().find(|project| {
        if project.root.as_str() != root || project.workspace_root.as_str() != workspace_root {
            return false;
        }
        match (&project.payload, tsconfig.as_deref()) {
            (
                crate::workspace_snapshot::ProjectPayload::Configured { tsconfig_path, .. },
                Some(expected),
            ) => tsconfig_path.as_str() == expected,
            (crate::workspace_snapshot::ProjectPayload::Fallback { .. }, None) => true,
            _ => false,
        }
    })
}

fn context_for_project(
    published: &PublishedRoot,
    project: &crate::workspace_snapshot::OwnershipProject,
) -> Result<ResolveContextId, ContextProvenanceError> {
    let project_identity = published
        .project_identity_hashes
        .get(&project.id)
        .copied()
        .ok_or(ContextProvenanceError::ProjectIdentityMissing)?;
    let resolve_env_hash = published
        .env_hashes_by_project
        .get(&project.id)
        .map(|hashes| hashes[1])
        .ok_or(ContextProvenanceError::ResolveEnvironmentMissing)?;
    Ok(ResolveContextId::from_hashes(
        project_identity,
        resolve_env_hash,
    ))
}

/// The context an entry selects in `world`'s published index.
///
/// The membership-evaluation boundary the `RESOLVE_CONTEXT_SELECT` probe
/// names, memoized per path on the index it walked. Every walk is
/// tallied, so repeated evaluation of one path against one index stays
/// observable through [`PublishedRoot::context_membership_evaluations`].
///
/// A world with NO published index answers before the boundary: there is
/// no index to walk, none to memoize against, and none to tally on.
pub(crate) fn selected_context_for_path(
    world: &ResolutionWorldRoot,
    canonical_id: &str,
) -> Result<ResolveContextId, ContextProvenanceError> {
    let published = world
        .published
        .as_ref()
        .ok_or(ContextProvenanceError::NoPublishedRoot)?;
    published.context_selection().selected(canonical_id, || {
        evaluate_selected_context(published, canonical_id)
    })
}

/// One membership walk over `published`. Pure in `(published, canonical_id)`.
fn evaluate_selected_context(
    published: &PublishedRoot,
    canonical_id: &str,
) -> Result<ResolveContextId, ContextProvenanceError> {
    if let Some(config) = published
        .snapshot
        .resolver
        .nearest_config_for_path(canonical_id)
    {
        let project = project_for_config(published, config)
            .ok_or(ContextProvenanceError::ProjectProjectionMissing)?;
        return context_for_project(published, project);
    }

    // Package-backed sources under a project root still select that
    // project even though resolver membership excludes `node_modules/`.
    let canonical = crate::CanonicalPath::new(canonical_id);
    let mut selected = None;
    for project in &published.snapshot.projects {
        if !canonical.starts_with_dir(&project.root) {
            continue;
        }
        let suffix = canonical
            .as_str()
            .strip_prefix(project.root.as_str())
            .unwrap_or_default();
        if !crate::engine::suffix_crosses_node_modules(suffix) {
            continue;
        }
        if selected.is_none_or(|current: &crate::workspace_snapshot::OwnershipProject| {
            project.root.as_str().len() > current.root.as_str().len()
        }) {
            selected = Some(project);
        }
    }
    match selected {
        Some(project) => context_for_project(published, project),
        // A complete read of the complete published index returning "no
        // owning project" is a real observed value, not a provenance gap.
        None => Ok(ResolveContextId::unowned()),
    }
}

pub(crate) fn explicit_context(
    world: &ResolutionWorldRoot,
    owner: &crate::types::ProjectOwnership,
) -> Result<(ProjectIdentity, ResolveContextId), ContextProvenanceError> {
    let published = world
        .published
        .as_ref()
        .ok_or(ContextProvenanceError::NoPublishedRoot)?;
    let normalized_root = crate::resolver::normalize_canonical_id(&owner.project_root);
    let normalized_tsconfig = owner
        .tsconfig_path
        .as_deref()
        .map(crate::resolver::normalize_canonical_id);
    let project = published
        .snapshot
        .projects
        .iter()
        .find(|project| {
            if project.root.as_str() != normalized_root {
                return false;
            }
            match (&project.payload, normalized_tsconfig.as_deref()) {
                (
                    crate::workspace_snapshot::ProjectPayload::Configured { tsconfig_path, .. },
                    Some(expected),
                ) => tsconfig_path.as_str() == expected,
                (crate::workspace_snapshot::ProjectPayload::Fallback { .. }, None) => true,
                _ => false,
            }
        })
        .ok_or(ContextProvenanceError::ProjectProjectionMissing)?;
    let context = context_for_project(published, project)?;
    Ok((context.project_identity, context))
}

impl ResolutionWorldRoot {
    fn context_version(&self, entry: &ResolutionEntry) -> ResolutionFactVersion {
        let context = match entry {
            ResolutionEntry::Importer(canonical) => selected_context_for_path(self, &canonical.0),
            ResolutionEntry::ExplicitProject(identity) => {
                let published = self
                    .published
                    .as_ref()
                    .ok_or(ContextProvenanceError::NoPublishedRoot);
                published.and_then(|published| {
                    let project = published
                        .project_identity_hashes
                        .iter()
                        .find_map(|(project_id, hash)| {
                            (*hash == identity.0)
                                .then(|| &published.snapshot.projects[project_id.0 as usize])
                        })
                        .ok_or(ContextProvenanceError::ProjectProjectionMissing)?;
                    context_for_project(published, project)
                })
            }
        };
        context
            .ok()
            .as_ref()
            .and_then(|context| self.context_versions.get(context).copied())
            .unwrap_or(ResolutionFactVersion::INITIAL)
    }

    pub(crate) fn fact_version(&self, key: &ResolutionFactKey) -> ResolutionFactVersion {
        match key {
            ResolutionFactKey::ContextSelection { entry, .. } => self.context_version(entry),
            _ => self.facts.version(key),
        }
    }
}

/// O(1) composition of a base root with an optional session overlay root.
///
/// The composition is the ONLY validity authority for
/// [`ResolveImportsFactRef::Resolution`] facts. It is immutable: every
/// version it reports comes from the `Arc`-pinned roots captured at
/// construction, never from the Engine's live registry, so a consumer that
/// captured this world validates against exactly the world it captured.
///
/// The carrier is sealed. Every field is crate-private and there is no
/// public constructor: the only way to obtain one is
/// [`crate::traits::WorkspaceRead::capture_resolution_world`], which mints it
/// from the Engine's published roots under the resolution-world epoch fence.
/// A consumer cannot manufacture a world from a direct probe, an overlay
/// lookup, or a normalized string.
#[derive(Debug, Clone)]
pub struct CapturedResolutionWorld {
    pub(crate) base: Arc<ResolutionWorldRoot>,
    pub(crate) session: Option<Arc<ResolutionSessionRoot>>,
    pub(crate) population: ResolutionPopulation,
}

impl CapturedResolutionWorld {
    /// Validate one resolve-imports fact against this captured world.
    ///
    /// The borrowing entry point for consumers holding a
    /// [`ResolveImportsFactRef`] (the session `StoreView` per-domain
    /// dispatch) — same body, same authority as the
    /// [`FactVersionValidator`] impl, which delegates here. A `Semantic`
    /// fact belongs to the session's own resolve-imports producer and is
    /// never this world's to validate.
    #[must_use]
    pub fn validates_resolve_imports_fact(&self, fact: &ResolveImportsFactRef) -> bool {
        match fact {
            ResolveImportsFactRef::Resolution(fact) => {
                self.answers_for_population(fact.key.population())
                    && self.fact_version(&fact.key) == fact.version
            }
            ResolveImportsFactRef::Semantic { .. } => false,
        }
    }

    /// `RC-4`: whether this world has AUTHORITY over `population`.
    ///
    /// A base world composes no overlay, and one session's world knows
    /// nothing of another's, so neither can answer for a session
    /// population that is not its own. Answering anyway would settle the
    /// question with [`ResolutionFactVersion::INITIAL`] — the value a
    /// never-advanced fact carries — so every session witness that
    /// happened to record an unadvanced fact would validate against a
    /// base capture. Authority is checked BEFORE the version comparison
    /// precisely so "this world cannot answer" never presents as "the
    /// fact has not moved".
    fn answers_for_population(&self, population: ResolutionPopulation) -> bool {
        match population {
            ResolutionPopulation::Base => true,
            ResolutionPopulation::Session(fingerprint) => {
                self.population == ResolutionPopulation::Session(fingerprint)
                    && self.session.is_some()
            }
        }
    }

    /// The stamp a [`CompactionDomain::Resolution`] aggregate is minted
    /// from and validated against: this captured world's ROOT IDENTITY.
    ///
    /// Root identity, not a ledger counter. `ContextSelection` is
    /// versioned in a separate `context_versions` map that
    /// [`Self::fact_version`] reads INSTEAD of the fact ledger, so a
    /// counter advanced only by ledger mutation would be blind to a
    /// published-context replacement and let a context change stale-serve.
    /// Both root ids advance on publication, which is the boundary
    /// `replace_published` crosses — so the stamp covers the whole domain,
    /// `ContextSelection` included.
    ///
    /// `None` for a population this world cannot answer for, which is what
    /// stops a session aggregate validating against a base world (or
    /// against a DIFFERENT session's world). A session stamp pins BOTH
    /// roots because a session-population fact composes both:
    /// [`Self::fact_version`] falls back to the base root when the session
    /// root holds no entry.
    #[must_use]
    pub fn resolution_stamp(&self, population: ResolutionPopulation) -> Option<AggregateStamp> {
        match population {
            ResolutionPopulation::Base => Some(AggregateStamp::ResolutionRoots {
                base: self.base.id,
                session: None,
            }),
            ResolutionPopulation::Session(fingerprint) => {
                if self.population != ResolutionPopulation::Session(fingerprint) {
                    return None;
                }
                let session = self.session.as_ref()?;
                Some(AggregateStamp::ResolutionRoots {
                    base: self.base.id,
                    session: Some(session.id),
                })
            }
        }
    }

    /// This world's OWN root-identity stamp — [`Self::resolution_stamp`]
    /// for the population the world was captured under.
    ///
    /// The seam a COMPOSITE stamp in another domain uses to pin "the
    /// resolved-import world has not been republished". A composite is
    /// minted under a VIEW population, which is a different identity
    /// space from [`ResolutionPopulation`] and cannot be translated into
    /// one — so the component is the world's own stamp, read through the
    /// same single derivation on both the producer and the validator
    /// side.
    #[must_use]
    pub fn own_resolution_stamp(&self) -> Option<AggregateStamp> {
        self.resolution_stamp(self.population)
    }

    pub(crate) fn fact_version(&self, key: &ResolutionFactKey) -> ResolutionFactVersion {
        match key.population() {
            ResolutionPopulation::Base => self.base.fact_version(key),
            ResolutionPopulation::Session(fingerprint) => {
                if self.population != ResolutionPopulation::Session(fingerprint) {
                    return ResolutionFactVersion::INITIAL;
                }
                if let Some(session) = self.session.as_ref() {
                    let version = session.facts.version(key);
                    if version != ResolutionFactVersion::INITIAL {
                        return version;
                    }
                }
                self.base
                    .fact_version(&key.in_population(ResolutionPopulation::Base))
            }
        }
    }
}

impl FactVersionValidator for CapturedResolutionWorld {
    fn validates_fact_version(&self, fact: &FactVersionRef) -> bool {
        match fact {
            FactVersionRef::ResolveImports(fact) => self.validates_resolve_imports_fact(fact),
            // The resolution domain's terminal aggregate: this world is
            // its only authority, exactly as it is for the precise facts
            // the aggregate replaced. Every other domain's aggregate
            // belongs to a producer this world knows nothing about.
            FactVersionRef::DomainGeneration(aggregate) => {
                aggregate.domain == crate::fact_cache::CompactionDomain::Resolution
                    && match aggregate.population {
                        crate::fact_cache::AggregatePopulation::Resolution(population) => {
                            self.resolution_stamp(population) == Some(aggregate.stamp)
                        }
                        // A VIEW population is another producer's identity
                        // space entirely — overlay installation, which this
                        // world knows nothing about. An aggregate claiming
                        // the resolution domain under one is malformed, and
                        // vouching for it would let a view-scoped claim be
                        // settled by a resolution-world stamp.
                        crate::fact_cache::AggregatePopulation::View(_) => false,
                    }
            }
            _ => false,
        }
    }
}

/// Durable diagnostics for higher-level owner-edge observation.
#[derive(Debug, Clone, Default)]
pub struct ResolutionCurrencyTrace {
    rejected_exact_targets: Arc<[Option<String>]>,
    recomputed: bool,
    published: bool,
    reused: bool,
}

impl ResolutionCurrencyTrace {
    pub fn rejected_exact_targets(&self) -> &[Option<String>] {
        &self.rejected_exact_targets
    }

    pub fn recomputed(&self) -> bool {
        self.recomputed
    }

    pub fn published(&self) -> bool {
        self.published
    }

    pub fn reused(&self) -> bool {
        self.reused
    }
}

/// Final result of the sealed Engine resolution transaction.
#[derive(Debug, Clone)]
pub struct ResolutionOutcome {
    result: Option<ResolveResult>,
    pub(crate) admission: SignatureAdmission,
    trace: ResolutionCurrencyTrace,
}

/// Typed refusal returned when a resolution-derived value reaches a persistent
/// sink without an admitted fact signature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolutionPublicationRefusal {
    reason: verter_audit::NonAdmissionReason,
}

impl ResolutionPublicationRefusal {
    #[must_use]
    pub fn new(reason: verter_audit::NonAdmissionReason) -> Self {
        Self { reason }
    }

    #[must_use]
    pub fn reason(self) -> verter_audit::NonAdmissionReason {
        self.reason
    }
}

/// Resolution-derived value whose complete observation signature was admitted.
///
/// The result stays behind this carrier so a durable sink cannot accept a
/// transient [`ResolveResult`] by accident. An admitted `None` is a witnessed
/// miss and remains distinct from [`ResolutionPublication::Refused`].
#[must_use = "an admitted resolution must be consumed at an explicit publication boundary"]
#[derive(Debug, Clone)]
pub struct AdmittedResolution<T = ResolveResult> {
    result: Option<T>,
    /// The complete observation signature the sealed transaction admitted
    /// for this result. A persistent sink roots its entry on THIS signature
    /// so the entry revalidates through the resolve-imports fact rail; it
    /// travels with the carrier and is never re-derivable from the result.
    signature: crate::ReadSetSignature,
}

impl<T> AdmittedResolution<T> {
    #[must_use]
    pub fn result(&self) -> Option<&T> {
        self.result.as_ref()
    }

    #[must_use]
    pub fn into_result(self) -> Option<T> {
        self.result
    }

    /// The admitted observation signature this resolution was minted with.
    ///
    /// A durable consumer roots its cache entry on these facts, which
    /// validate against a captured [`CapturedResolutionWorld`] through the
    /// existing `FactVersionRef::ResolveImports` rail.
    #[must_use]
    pub fn signature(&self) -> &crate::ReadSetSignature {
        &self.signature
    }

    /// Replace the projected value while retaining this transaction's
    /// admission capability. No admitted carrier can be constructed outside
    /// the Engine-owned transaction boundary.
    pub fn replace_result<U>(self, result: Option<U>) -> ResolutionPublication<U> {
        ResolutionPublication::Admitted(AdmittedResolution {
            result,
            signature: self.signature,
        })
    }
}

/// Admitted identity is the projected value. The observation signature is
/// provenance carried alongside it — two carriers projecting the same value
/// under different witnesses stay equal, exactly as they were before the
/// signature travelled with the carrier.
impl<T: PartialEq> PartialEq for AdmittedResolution<T> {
    fn eq(&self, other: &Self) -> bool {
        self.result == other.result
    }
}

impl<T: Eq> Eq for AdmittedResolution<T> {}

/// Typed outcome at a durable resolution-derived publication boundary.
///
/// This is intentionally not `Result<Option<_>, _>`: generic projections such
/// as `.ok().flatten()` made a refusal indistinguishable from an admitted miss.
#[must_use = "a resolution refusal must be propagated to the final publication boundary"]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolutionPublication<T = ResolveResult> {
    Admitted(AdmittedResolution<T>),
    Refused(ResolutionPublicationRefusal),
}

impl<T> ResolutionPublication<T> {
    /// Whether this is an admitted, witnessed miss.
    ///
    /// A refusal is deliberately neither `None` nor `Some`.
    #[must_use]
    pub fn is_none(&self) -> bool {
        matches!(self, Self::Admitted(admitted) if admitted.result().is_none())
    }

    /// Whether this is an admitted value.
    ///
    /// A refusal is deliberately neither `None` nor `Some`.
    #[must_use]
    pub fn is_some(&self) -> bool {
        matches!(self, Self::Admitted(admitted) if admitted.result().is_some())
    }

    /// Construct a typed refusal for a higher-level resolution-derived batch
    /// whose final publication fence failed.
    pub fn refused(reason: verter_audit::NonAdmissionReason) -> Self {
        Self::Refused(ResolutionPublicationRefusal { reason })
    }

    /// Transform an admitted result while preserving admitted misses and
    /// publication refusals as distinct states.
    pub fn map_result<U>(self, map: impl FnOnce(T) -> U) -> ResolutionPublication<U> {
        match self {
            Self::Admitted(admitted) => {
                let signature = admitted.signature.clone();
                ResolutionPublication::Admitted(AdmittedResolution {
                    result: admitted.into_result().map(map),
                    signature,
                })
            }
            Self::Refused(refusal) => ResolutionPublication::Refused(refusal),
        }
    }

    /// Transform the complete admitted optional product. Unlike a public
    /// constructor, this requires an existing Engine-minted admission carrier.
    pub fn map_admitted<U>(
        self,
        map: impl FnOnce(Option<T>) -> Option<U>,
    ) -> ResolutionPublication<U> {
        match self {
            Self::Admitted(admitted) => {
                let signature = admitted.signature.clone();
                let result = map(admitted.into_result());
                ResolutionPublication::Admitted(AdmittedResolution { result, signature })
            }
            Self::Refused(refusal) => ResolutionPublication::Refused(refusal),
        }
    }

    /// Evaluate an alternate admitted source only after an admitted miss.
    /// Refusals never fall through.
    pub fn or_else_publication(
        self,
        alternate: impl FnOnce() -> ResolutionPublication<T>,
    ) -> ResolutionPublication<T> {
        match self {
            Self::Admitted(admitted) if admitted.result().is_none() => alternate(),
            other => other,
        }
    }
}

impl<T: std::ops::Deref> ResolutionPublication<T> {
    /// Borrow the admitted result through its dereference target while
    /// preserving refusal as a typed refusal.
    pub fn as_deref(&self) -> ResolutionPublication<&T::Target> {
        match self {
            Self::Admitted(admitted) => ResolutionPublication::Admitted(AdmittedResolution {
                result: admitted.result().map(T::deref),
                signature: admitted.signature.clone(),
            }),
            Self::Refused(refusal) => ResolutionPublication::Refused(*refusal),
        }
    }
}

impl<T: PartialEq> PartialEq<Option<T>> for ResolutionPublication<T> {
    fn eq(&self, other: &Option<T>) -> bool {
        matches!(self, Self::Admitted(admitted) if admitted.result() == other.as_ref())
    }
}

impl<T: PartialEq> PartialEq<ResolutionPublication<T>> for Option<T> {
    fn eq(&self, other: &ResolutionPublication<T>) -> bool {
        other == self
    }
}

impl ResolutionOutcome {
    pub(crate) fn refused(
        result: Option<ResolveResult>,
        reason: verter_audit::NonAdmissionReason,
    ) -> Self {
        Self::new(
            result,
            SignatureAdmission::NonCacheable(reason),
            Vec::new(),
            true,
            false,
            false,
        )
    }

    pub(crate) fn adapter_return_only(result: Option<ResolveResult>) -> Self {
        Self::new(
            result,
            SignatureAdmission::NonCacheable(
                verter_audit::NonAdmissionReason::ResolutionUntrackedBackend,
            ),
            Vec::new(),
            true,
            false,
            false,
        )
    }

    pub(crate) fn new(
        result: Option<ResolveResult>,
        admission: SignatureAdmission,
        rejected_exact_targets: Vec<Option<String>>,
        recomputed: bool,
        published: bool,
        reused: bool,
    ) -> Self {
        Self {
            result,
            admission,
            trace: ResolutionCurrencyTrace {
                rejected_exact_targets: rejected_exact_targets.into(),
                recomputed,
                published,
                reused,
            },
        }
    }

    pub fn result(&self) -> Option<&ResolveResult> {
        self.result.as_ref()
    }

    /// Consume the transaction outcome for a caller that will not retain any
    /// resolution-derived state.
    ///
    /// This deliberately names the lifetime contract. Persistent consumers
    /// must use [`Self::into_publication`] so ReturnOnly outcomes cannot
    /// cross a sink boundary through an ambiguous result projection.
    pub fn into_transient_result(self) -> Option<ResolveResult> {
        self.result
    }

    /// Consume the transaction outcome at a persistent sink boundary.
    ///
    /// A result is publishable only when the sealed resolution transaction
    /// admitted its complete fact signature. ReturnOnly results remain useful
    /// through [`Self::into_transient_result`] but can never leave this method.
    pub fn into_publication(self) -> ResolutionPublication {
        match self.admission {
            SignatureAdmission::Cacheable(signature) => {
                ResolutionPublication::Admitted(AdmittedResolution {
                    result: self.result,
                    signature,
                })
            }
            SignatureAdmission::NonCacheable(reason) => {
                ResolutionPublication::Refused(ResolutionPublicationRefusal { reason })
            }
        }
    }

    pub fn trace(&self) -> &ResolutionCurrencyTrace {
        &self.trace
    }

    pub fn is_cacheable(&self) -> bool {
        matches!(self.admission, SignatureAdmission::Cacheable(_))
    }

    pub fn non_admission_reason(&self) -> Option<verter_audit::NonAdmissionReason> {
        match &self.admission {
            SignatureAdmission::Cacheable(_) => None,
            SignatureAdmission::NonCacheable(reason) => Some(*reason),
        }
    }
}

/// Raw observed values one attempt retained so the Engine can fold them
/// into the world's recorded evidence baseline through the mutation
/// protocol at the admission fence.
///
/// TOTALITY: this carries every REOBSERVABLE fact family — exactly the
/// families [`ResolutionFactKey::reobservable_path_canonical_id`] names, and
/// exactly the families [`LiveResolutionObservation`] re-reads. A family
/// missing here is a family whose baseline is never recorded, so its first
/// re-observation is indistinguishable from a first observation and can
/// never advance a fact. That is not an optimisation detail; it is the
/// difference between a witness that heals and one that validates forever.
#[derive(Debug, Default)]
pub(crate) struct ObservedResolutionValues {
    /// Observed path-probe outcomes, in observation order.
    pub(crate) path_probes: Vec<(String, PathProbe)>,
    /// Observed realpath values (`None` = the requested path resolved to
    /// no realpath), in observation order.
    pub(crate) realpaths: Vec<(String, Option<String>)>,
    /// Observed manifest fingerprints (`None` = no manifest at that path),
    /// in observation order.
    pub(crate) manifests: Vec<(String, Option<[u8; 16]>)>,
}

impl ObservedResolutionValues {
    pub(crate) fn is_empty(&self) -> bool {
        self.path_probes.is_empty() && self.realpaths.is_empty() && self.manifests.is_empty()
    }
}

/// One canonical's resolution-visible values, read LIVE.
///
/// The single shape every live evidence read produces and every evidence
/// consumer compares against, so freeze-time revalidation and reuse-time
/// re-observation cannot describe "what the filesystem currently says" in two
/// different vocabularies — which is precisely how the two paths drifted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveResolutionObservation {
    /// Effective typed path probe.
    pub probe: PathProbe,
    /// Effective realpath, normalized; `None` when the path resolves to no
    /// realpath.
    pub realpath: Option<String>,
    /// `None` when the canonical is not a package-manifest path at all;
    /// `Some(None)` when it is one and no manifest is present.
    pub manifest: Option<Option<[u8; 16]>>,
}

/// **The one live-observation primitive for resolution evidence.**
///
/// Reads a canonical's resolution-visible values — typed probe, realpath,
/// manifest fingerprint — from the live source, and returns them. Every
/// evidence consumer (freeze-time revalidation of a recorded observation,
/// reuse-time re-observation of a retained witness) goes through this one
/// read, so "what the source currently says" has exactly one implementation
/// per backend and the consumers cannot drift apart.
///
/// **Live means every cache whose invalidation depends on the audited event
/// channel is bypassed.** An evidence read exists to detect the changes the
/// event stream missed, so answering it out of an event-invalidated cache — a
/// file snapshot, a directory index, a realpath memo, a parsed-manifest
/// cache — can only ever confirm the cache. An overlay is the one exemption:
/// it is authoritative state, not a copy of state, so its content IS the live
/// value.
///
/// `recorded` is the value the caller currently believes, supplied so the
/// implementor can repair its OWN memos when the live read disagrees — and
/// only then. Repair is backend-internal; the value-sensitive fact advance,
/// the publication and the retry all stay in the Engine's mutation protocol.
/// `None` means the caller holds no belief yet, which is never a disagreement.
///
/// Returning `None` means this source genuinely cannot observe the canonical
/// at all — unstable I/O, or a target with no live filesystem behind it. A
/// `None` is never stamped as verified and never folded into a baseline: a
/// read that did not happen must not certify anything. An `Inaccessible` or
/// `Unknown` probe is NOT that case: those are observed VALUES, and returning
/// them as values is what lets a candidate whose target became unreadable die
/// instead of validating forever.
pub(crate) trait LiveResolutionEvidence {
    fn observe_live_resolution_evidence(
        &self,
        canonical_id: &str,
        recorded: Option<&RecordedResolutionBaseline>,
    ) -> Option<LiveResolutionObservation>;
}

/// The evidence capability a resolution request resolves under, stated by the
/// backend at the Engine entry point it calls.
///
/// This is deliberately NOT a [`crate::traits::WorkspaceRead`] hook. A reader
/// hook is forwarded by every delegating wrapper, and a wrapper that forgets
/// one silently inherits the default — three consecutive review rounds landed
/// a fix on one reader while production used another. A required parameter on
/// the Engine's resolution entry cannot be forwarded, stripped or forgotten:
/// the backend that owns the Engine states its capability once, and no reader
/// composed on top of it participates at all.
#[derive(Clone, Copy)]
pub(crate) enum ResolutionEvidenceSource<'a> {
    /// **Fail closed.** No live source: nothing is re-observed, no baseline is
    /// folded, and no canonical is ever stamped verified. A caller that cannot
    /// state a capability gets this, and it can only ever fail to heal — it
    /// can never certify stale state as freshly verified.
    // Constructed on `wasm32` — where `FilesystemWorkspace` has no live
    // filesystem behind its caches — and by the fail-closed contract test.
    // Neither is the native lib build, so the native build sees no
    // constructor for it.
    #[allow(dead_code)]
    Inert,
    /// This reader's OWN reads are the truth: there is no event-invalidated
    /// cache standing behind them, so reading through it IS a live read. An
    /// in-memory workspace is the production case — its overlay and snapshot
    /// are authoritative state rather than a copy of state.
    ///
    /// A backend with any cache behind `probe_path` / `realpath` /
    /// `read_file` must NOT use this arm: it would read that backend's caches
    /// back to it and stamp them as freshly verified.
    ReaderAuthoritative,
    /// Some resolver-visible change reaches this backend with NO event — a
    /// package installed into an unwatched `node_modules` is the case — so
    /// reused evidence is re-observed through this live source once per
    /// content transition, bounded by the candidate's own witness.
    // Mirror of the `Inert` note above: on `wasm32` there is no live
    // filesystem, so nothing constructs this arm on that target.
    #[allow(dead_code)]
    Uncovered(&'a dyn LiveResolutionEvidence),
}

/// What the recorded world currently believes about one canonical, PER
/// FAMILY.
///
/// Every field is `Option`al over the family's own value type, and the outer
/// `None` means "never observed" — which is categorically different from an
/// observed absence. Collapsing the two is how a live read that agrees with
/// everything recorded still reads as a disagreement: an unrecorded realpath
/// is not "this path has no realpath", and an unrecorded manifest is not
/// "there is no manifest here". Both mis-readings fire memo repairs on every
/// tick, for canonicals nothing has changed about.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecordedResolutionBaseline {
    pub probe: Option<PathProbe>,
    pub realpath: Option<Option<String>>,
    pub manifest: Option<Option<[u8; 16]>>,
}

impl RecordedResolutionBaseline {
    /// `true` when no family has a recorded value — nothing a live read can
    /// contradict.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.probe.is_none() && self.realpath.is_none() && self.manifest.is_none()
    }

    /// Which families a live observation CONTRADICTS. A family with no
    /// recorded value contradicts nothing, in either direction.
    #[must_use]
    pub fn disagreements(&self, live: &LiveResolutionObservation) -> EvidenceDisagreement {
        EvidenceDisagreement {
            probe: self.probe.is_some_and(|probe| probe != live.probe),
            realpath: self
                .realpath
                .as_ref()
                .is_some_and(|realpath| realpath != &live.realpath),
            manifest: self
                .manifest
                .zip(live.manifest)
                .is_some_and(|(recorded, live)| recorded != live),
        }
    }
}

/// Per-family contradiction flags from
/// [`RecordedResolutionBaseline::disagreements`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EvidenceDisagreement {
    pub probe: bool,
    pub realpath: bool,
    pub manifest: bool,
}

impl EvidenceDisagreement {
    #[must_use]
    pub fn any(self) -> bool {
        self.probe || self.realpath || self.manifest
    }
}

/// Whether `canonical_id` names a package manifest, i.e. whether the
/// `Manifest` fact family applies to it. One predicate, so the observation
/// rail and the fact rail cannot disagree about which paths carry manifests.
#[must_use]
pub fn is_package_manifest_path(canonical_id: &str) -> bool {
    canonical_id.ends_with("/package.json")
}

/// The resolution-SEMANTIC projection of a `package.json`'s bytes.
///
/// Two manifests with the same projection resolve identically, so a rewrite
/// that changes only `description` or `scripts` moves no resolution fact.
#[must_use]
pub fn manifest_resolution_fingerprint(source: &str) -> [u8; 16] {
    let manifest = crate::package_index::parse_package_json(source);
    manifest_fingerprint_of(&manifest)
}

/// [`manifest_resolution_fingerprint`] for an already-parsed manifest, so a
/// reader that answers with a `PackageManifest` never re-parses to be
/// fingerprinted.
#[must_use]
pub fn manifest_fingerprint_of(manifest: &crate::types::PackageManifest) -> [u8; 16] {
    let semantic = serde_json::json!({
        "name": manifest.name,
        "main": manifest.main,
        "module": manifest.module,
        "types": manifest.types,
        "typings": manifest.typings,
        "exports": manifest.exports,
        "imports": manifest.imports,
    });
    let bytes = serde_json::to_vec(&semantic)
        .expect("resolution manifest projection is always JSON-serializable");
    xxhash_rust::xxh3::xxh3_128(&bytes).to_le_bytes()
}

/// Private capability which records and finalises one attempt exactly once.
pub(crate) struct ResolutionTransaction {
    root: Arc<CapturedResolutionWorld>,
    /// Facts this attempt observed itself, in first-observation order, with
    /// no key recorded twice — see [`Self::observed_keys`].
    observations: Vec<FactVersionRef>,
    /// Every key already present in [`Self::observations`].
    ///
    /// A resolver attempt re-reads the same inputs constantly: each path
    /// probe and each realpath observes its whole ancestor recovery chain,
    /// so probing many candidates inside one directory re-observes that
    /// directory's ancestors once per candidate. Measured on a 200-SFC
    /// corpus, four out of every five observations recorded were a key the
    /// attempt had already recorded.
    ///
    /// Recording a key once is not a weaker read set. `root` is an immutable
    /// captured world, so `fact_version` is a pure function of the key for
    /// this transaction's whole life: a repeat observation produces a
    /// byte-identical `FactVersionRef`, and finalisation's sort + dedup
    /// discarded it anyway. Suppressing it at the recording point drops the
    /// redundant world lookup, the redundant key clone, and the sort work
    /// they created, while leaving the finalised set — and the merge with
    /// any absorbed canonical run — exactly as it was.
    observed_keys: rustc_hash::FxHashSet<ResolutionFactKey>,
    /// This attempt's COMPLETE direct dependency set, in first-observation
    /// order: the primitive resolution facts it observed itself plus the
    /// child decisions it reused. Nothing transitive — a child's own
    /// edges are the child's business.
    ///
    /// Filled by the same operation that records the witness
    /// ([`Self::observe_fact`]), from
    /// [`classify_resolution_observation`], so a fact can never enter the
    /// witness without its edge role having been decided.
    direct_edges: Vec<ResolutionFactKey>,
    observed_values: ObservedResolutionValues,
    non_admission: Option<verter_audit::NonAdmissionReason>,
    query: Option<ResolutionQueryKey>,
    /// Per-domain generations this transaction compacts against. Only the
    /// resolution domain is populated — see [`Self::new`].
    aggregate_basis: crate::fact_cache::AggregateGenerations,
}

/// Resolver-facing reader that makes every filesystem/config observation enter
/// the enclosing transaction. It is private, so Engine callers cannot resolve
/// with an untracked reader by accident.
pub(crate) struct TransactionReader<'a> {
    inner: &'a dyn crate::traits::WorkspaceRead,
    transaction: &'a Mutex<ResolutionTransaction>,
}

/// Immutable request-local overlay composed over an Engine-backed workspace
/// reader. The wrapper never publishes into the shared resolution cache/edge
/// stores; its admitted product is valid only for the enclosing batch.
pub(crate) struct OverlaySnapshotReader<'a> {
    inner: &'a dyn crate::traits::WorkspaceRead,
    overlay: &'a ResolutionOverlaySnapshot,
}

impl<'a> OverlaySnapshotReader<'a> {
    pub(crate) fn new(
        inner: &'a dyn crate::traits::WorkspaceRead,
        overlay: &'a ResolutionOverlaySnapshot,
    ) -> Self {
        Self { inner, overlay }
    }
}

impl crate::traits::WorkspaceRead for OverlaySnapshotReader<'_> {
    fn read_file(&self, canonical_id: &str) -> Option<Arc<str>> {
        self.overlay
            .get(canonical_id)
            .unwrap_or_else(|| self.inner.read_file(canonical_id))
    }

    fn take_last_read_file_trace_detail(&self, canonical_id: &str) -> Option<String> {
        self.inner.take_last_read_file_trace_detail(canonical_id)
    }

    fn file_exists(&self, canonical_id: &str) -> bool {
        matches!(
            self.probe_path(canonical_id),
            PathProbe::File | PathProbe::Directory
        )
    }

    fn probe_path(&self, canonical_id: &str) -> PathProbe {
        match self.overlay.get(canonical_id) {
            Some(Some(_)) => PathProbe::File,
            Some(None) => PathProbe::Absent,
            None => self.inner.probe_path(canonical_id),
        }
    }

    fn resolution_event_bridge_complete(&self) -> bool {
        self.inner.resolution_event_bridge_complete()
    }

    fn resolution_snapshot_is_request_local(&self) -> bool {
        true
    }

    fn take_resolution_directory_observations(&self) -> Vec<String> {
        self.inner.take_resolution_directory_observations()
    }

    fn resolution_population(&self) -> ResolutionPopulation {
        self.inner.resolution_population()
    }

    fn realpath(&self, canonical_id: &str) -> Option<String> {
        match self.overlay.get(canonical_id) {
            Some(Some(_)) => Some(crate::resolver::normalize_canonical_id(canonical_id)),
            Some(None) => None,
            None => self.inner.realpath(canonical_id),
        }
    }

    fn read_package_manifest(&self, canonical_id: &str) -> Option<crate::types::PackageManifest> {
        match self.overlay.get(canonical_id) {
            Some(Some(source)) => Some(crate::package_index::parse_package_json(source.as_ref())),
            Some(None) => None,
            None => self.inner.read_package_manifest(canonical_id),
        }
    }

    fn is_workspace_owned(&self, canonical_id: &str) -> bool {
        self.inner.is_workspace_owned(canonical_id)
    }

    fn is_package_backed(&self, canonical_id: &str) -> bool {
        self.inner.is_package_backed(canonical_id)
    }

    fn content_generation(&self) -> u64 {
        self.inner.content_generation()
    }

    fn resolution_fact_generation(&self) -> u64 {
        self.inner.resolution_fact_generation()
    }

    fn last_content_transition_generation(&self, canonical_id: &str) -> u64 {
        self.inner.last_content_transition_generation(canonical_id)
    }

    fn vfs_provenance_snapshot(&self) -> crate::types::VfsProvenanceSnapshot {
        self.inner.vfs_provenance_snapshot()
    }

    fn resource_snapshot(&self) -> crate::traits::WorkspaceResourceSnapshot {
        self.inner.resource_snapshot()
    }

    fn reverse_deps_for(&self, canonical_id: &str) -> Vec<String> {
        self.inner.reverse_deps_for(canonical_id)
    }

    fn forward_deps_for(&self, canonical_id: &str) -> Vec<String> {
        self.inner.forward_deps_for(canonical_id)
    }

    fn dependency_snapshot(&self, canonical_id: &str) -> Option<crate::DependencySnapshotView> {
        self.inner.dependency_snapshot(canonical_id)
    }

    fn read_dir(&self, dir: &str) -> Result<Vec<crate::error::DirEntry>, crate::error::VfsError> {
        self.inner.read_dir(dir)
    }

    fn walk(
        &self,
        root: &str,
        filter_dir: &dyn Fn(&str) -> bool,
        filter_file: &dyn Fn(&str) -> bool,
    ) -> Result<Vec<String>, crate::error::VfsError> {
        self.inner.walk(root, filter_dir, filter_file)
    }

    fn is_dir(&self, path: &str) -> bool {
        match self.overlay.get(path) {
            Some(_) => false,
            None => self.inner.is_dir(path),
        }
    }
}

impl<'a> TransactionReader<'a> {
    pub(crate) fn new(
        inner: &'a dyn crate::traits::WorkspaceRead,
        transaction: &'a Mutex<ResolutionTransaction>,
    ) -> Self {
        // Evidence is per-thread and per resolver-facing operation. Discard
        // anything left by an earlier non-transactional read before this
        // transaction starts.
        let _ = inner.take_resolution_directory_observations();
        if !inner.resolution_event_bridge_complete() {
            transaction.lock().mark_untracked_backend();
        }
        Self { inner, transaction }
    }

    fn capture_directory_observations<T>(&self, operation: impl FnOnce() -> T) -> T {
        let _ = self.inner.take_resolution_directory_observations();
        let result = operation();
        let observations = self.inner.take_resolution_directory_observations();
        if !observations.is_empty() {
            let mut transaction = self.transaction.lock();
            for canonical in observations {
                transaction.observe_directory(&canonical);
            }
        }
        result
    }
}

impl crate::traits::WorkspaceRead for TransactionReader<'_> {
    fn read_file(&self, canonical_id: &str) -> Option<Arc<str>> {
        let source = self.capture_directory_observations(|| self.inner.read_file(canonical_id));
        if is_package_manifest_path(canonical_id) {
            let fingerprint = source
                .as_deref()
                .map(crate::resolution_currency::manifest_resolution_fingerprint);
            self.transaction
                .lock()
                .observe_manifest(canonical_id, fingerprint);
        }
        source
    }

    fn take_last_read_file_trace_detail(&self, canonical_id: &str) -> Option<String> {
        self.inner.take_last_read_file_trace_detail(canonical_id)
    }

    fn file_exists(&self, canonical_id: &str) -> bool {
        matches!(
            self.probe_path(canonical_id),
            PathProbe::File | PathProbe::Directory
        )
    }

    fn probe_path(&self, canonical_id: &str) -> PathProbe {
        let outcome = self.capture_directory_observations(|| self.inner.probe_path(canonical_id));
        self.transaction.lock().observe_path(canonical_id, outcome);
        outcome
    }

    fn resolution_event_bridge_complete(&self) -> bool {
        true
    }

    fn resolution_population(&self) -> ResolutionPopulation {
        self.transaction.lock().population()
    }

    fn realpath(&self, canonical_id: &str) -> Option<String> {
        let resolved = self.capture_directory_observations(|| self.inner.realpath(canonical_id));
        self.transaction
            .lock()
            .observe_realpath(canonical_id, resolved.as_deref());
        resolved
    }

    fn read_package_manifest(&self, canonical_id: &str) -> Option<crate::types::PackageManifest> {
        let manifest =
            self.capture_directory_observations(|| self.inner.read_package_manifest(canonical_id));
        self.transaction
            .lock()
            .observe_manifest(canonical_id, manifest.as_ref().map(manifest_fingerprint_of));
        manifest
    }

    fn classify_file(&self, canonical_id: &str) -> verter_language::FileLanguage {
        self.inner.classify_file(canonical_id)
    }

    fn is_workspace_owned(&self, canonical_id: &str) -> bool {
        self.capture_directory_observations(|| self.inner.is_workspace_owned(canonical_id))
    }

    fn is_package_backed(&self, canonical_id: &str) -> bool {
        self.capture_directory_observations(|| self.inner.is_package_backed(canonical_id))
    }

    fn content_generation(&self) -> u64 {
        self.inner.content_generation()
    }

    fn resolution_fact_generation(&self) -> u64 {
        self.inner.resolution_fact_generation()
    }

    fn last_content_transition_generation(&self, canonical_id: &str) -> u64 {
        self.inner.last_content_transition_generation(canonical_id)
    }

    fn vfs_provenance_snapshot(&self) -> crate::types::VfsProvenanceSnapshot {
        self.inner.vfs_provenance_snapshot()
    }

    fn resource_snapshot(&self) -> crate::traits::WorkspaceResourceSnapshot {
        self.inner.resource_snapshot()
    }

    fn reverse_deps_for(&self, canonical_id: &str) -> Vec<String> {
        self.inner.reverse_deps_for(canonical_id)
    }

    fn forward_deps_for(&self, canonical_id: &str) -> Vec<String> {
        self.inner.forward_deps_for(canonical_id)
    }

    fn known_canonicals(&self) -> Vec<String> {
        self.inner.known_canonicals()
    }

    fn dependency_snapshot(
        &self,
        canonical_id: &str,
    ) -> Option<crate::exact_resolution::DependencySnapshotView> {
        self.inner.dependency_snapshot(canonical_id)
    }

    fn read_dir(&self, dir: &str) -> Result<Vec<crate::error::DirEntry>, crate::error::VfsError> {
        self.transaction.lock().observe_directory(dir);
        self.capture_directory_observations(|| self.inner.read_dir(dir))
    }

    fn walk(
        &self,
        root: &str,
        filter_dir: &dyn Fn(&str) -> bool,
        filter_file: &dyn Fn(&str) -> bool,
    ) -> Result<Vec<String>, crate::error::VfsError> {
        self.transaction.lock().observe_directory(root);
        self.capture_directory_observations(|| self.inner.walk(root, filter_dir, filter_file))
    }

    fn is_dir(&self, path: &str) -> bool {
        matches!(self.probe_path(path), PathProbe::Directory)
    }

    fn read_ambient_lib(
        &self,
        stable_key: crate::project_key::ProjectStableKey,
        canonical_id: &str,
    ) -> Option<Arc<str>> {
        self.inner.read_ambient_lib(stable_key, canonical_id)
    }

    fn ambient_virtual_canonical_id(
        &self,
        stable_key: crate::project_key::ProjectStableKey,
        canonical_id: &str,
    ) -> Arc<str> {
        self.inner
            .ambient_virtual_canonical_id(stable_key, canonical_id)
    }

    fn project_stable_key(
        &self,
        project_id: crate::workspace_snapshot::ProjectId,
    ) -> Option<crate::project_key::ProjectStableKey> {
        self.inner.project_stable_key(project_id)
    }

    fn lookup_ambient_symbol(
        &self,
        consumer_project: crate::project_key::ProjectStableKey,
        symbol: &str,
    ) -> Option<crate::ambient_lib::AmbientSymbolHit> {
        self.inner.lookup_ambient_symbol(consumer_project, symbol)
    }

    fn ambient_libs_view(&self) -> Arc<crate::ambient_lib::AmbientLibsByProject> {
        self.inner.ambient_libs_view()
    }

    fn published_root(&self) -> Option<Arc<PublishedRoot>> {
        self.inner.published_root()
    }
}

impl ResolutionTransaction {
    pub(crate) fn new(root: Arc<CapturedResolutionWorld>) -> Self {
        // A resolution transaction observes exactly one compaction domain,
        // and its captured world is that domain's producer AND validator —
        // so the basis is derivable here, with no plumbing from the
        // caller. Every other domain is left absent and stays precise.
        let resolution = root.resolution_stamp(root.population);
        Self {
            aggregate_basis: crate::fact_cache::AggregateGenerations {
                resolution,
                ..Default::default()
            },
            root,
            observations: Vec::new(),
            observed_keys: rustc_hash::FxHashSet::default(),
            direct_edges: Vec::new(),
            observed_values: ObservedResolutionValues::default(),
            non_admission: None,
            query: None,
        }
    }

    /// Move the retained raw observed values out of the transaction so the
    /// Engine can fold them into the recorded evidence baseline at the
    /// admission fence.
    pub(crate) fn take_observed_values(&mut self) -> ObservedResolutionValues {
        std::mem::take(&mut self.observed_values)
    }

    pub(crate) fn observe(&mut self, key: ResolutionFactKey) {
        if self.observed_keys.contains(&key) {
            crate::probe_tally!(OBS_SUPPRESSED, 1);
            return;
        }
        let version = self.root.fact_version(&key);
        self.observed_keys.insert(key.clone());
        self.observe_fact(FactVersionRef::ResolveImports(
            ResolveImportsFactRef::Resolution(ResolutionFactRef { key, version }),
        ));
    }

    /// **The one operation that records a witness entry and classifies its
    /// edge role.** Every observation this transaction makes goes through
    /// here, so there is no path on which a fact enters the witness
    /// without a direct-leaf / derived-node / terminal disposition.
    fn observe_fact(&mut self, fact: FactVersionRef) {
        match (classify_resolution_observation(&fact), &fact) {
            (
                ResolutionEdgeClass::DirectLeaf | ResolutionEdgeClass::DerivedNode,
                FactVersionRef::ResolveImports(ResolveImportsFactRef::Resolution(resolution)),
            ) => self.direct_edges.push(resolution.key.clone()),
            (ResolutionEdgeClass::DirectLeaf | ResolutionEdgeClass::DerivedNode, other) => {
                // Structurally unreachable: only the resolution arm of
                // the classification yields an edge-bearing class, and
                // only a resolution fact carries a graph key. Stated so a
                // future classification change fails loudly here instead
                // of silently minting an edge with no key.
                unreachable!("edge-bearing class on a non-resolution fact: {other:?}")
            }
            (ResolutionEdgeClass::Terminal, _) => {}
        }
        self.observations.push(fact);
    }

    /// This attempt's complete direct dependency set, deduped, in
    /// first-observation order.
    pub(crate) fn direct_edges(&self) -> Vec<ResolutionFactKey> {
        self.direct_edges.clone()
    }

    /// Stage a fact minted by ANOTHER domain's producer.
    ///
    /// Every production resolution observation is a resolution key, so
    /// this is the fixture seam that exercises the TERMINAL
    /// (non-edge-bearing) classification arm and the cross-domain
    /// signature shapes `SIG-3` asserts.
    #[cfg(test)]
    pub(crate) fn observe_foreign_fact_for_test(&mut self, fact: FactVersionRef) {
        self.observe_fact(fact);
    }

    pub(crate) fn population(&self) -> ResolutionPopulation {
        self.root.population
    }

    pub(crate) fn mark_incomplete_provenance(&mut self) {
        self.non_admission = Some(verter_audit::NonAdmissionReason::ResolutionIncompleteProvenance);
    }

    pub(crate) fn mark_untracked_backend(&mut self) {
        self.non_admission = Some(verter_audit::NonAdmissionReason::ResolutionUntrackedBackend);
    }

    pub(crate) fn observe_path(&mut self, canonical: &str, outcome: PathProbe) {
        let canonical = crate::resolver::normalize_canonical_id(canonical);
        self.observed_values
            .path_probes
            .push((canonical.clone(), outcome));
        self.observe(ResolutionFactKey::PathProbe {
            canonical: CanonicalResolutionId::new(canonical.clone()),
            population: self.population(),
        });
        self.observe_recovery_chain(&canonical);
        match outcome {
            PathProbe::Inaccessible => {
                self.non_admission =
                    Some(verter_audit::NonAdmissionReason::ResolutionInaccessiblePath);
            }
            PathProbe::Unknown => {
                self.non_admission = Some(verter_audit::NonAdmissionReason::ResolutionUnknownPath);
            }
            PathProbe::File | PathProbe::Directory | PathProbe::Absent => {}
        }
    }

    /// Observe the `Manifest` fact for `canonical` together with the value
    /// that was read, so the admission fold can record a baseline for it.
    /// A manifest observed as ABSENT records `None` — that is a value, not
    /// an absence of one.
    pub(crate) fn observe_manifest(&mut self, canonical: &str, fingerprint: Option<[u8; 16]>) {
        let canonical = crate::resolver::normalize_canonical_id(canonical);
        self.observed_values
            .manifests
            .push((canonical.clone(), fingerprint));
        self.observe(ResolutionFactKey::Manifest {
            canonical: CanonicalResolutionId::new(canonical),
            population: self.population(),
        });
    }

    pub(crate) fn observe_realpath(&mut self, requested: &str, resolved: Option<&str>) {
        let requested = crate::resolver::normalize_canonical_id(requested);
        self.observed_values.realpaths.push((
            requested.clone(),
            resolved.map(crate::resolver::normalize_canonical_id),
        ));
        self.observe(ResolutionFactKey::Realpath {
            requested: CanonicalResolutionId::new(requested.clone()),
            population: self.population(),
        });
        self.observe_recovery_chain(&requested);
        if let Some(resolved) = resolved {
            self.observe_recovery_chain(&crate::resolver::normalize_canonical_id(resolved));
        }
    }

    pub(crate) fn observe_directory(&mut self, canonical: &str) {
        self.observe(ResolutionFactKey::DirectoryMembers {
            canonical: CanonicalResolutionId::new(crate::resolver::normalize_canonical_id(
                canonical,
            )),
            population: self.population(),
        });
    }

    fn observe_recovery_chain(&mut self, canonical: &str) {
        for prefix in ancestor_scopes(canonical) {
            self.observe(ResolutionFactKey::RecoveryScope {
                canonical_prefix: CanonicalResolutionId::new(prefix),
                population: self.population(),
            });
        }
    }

    pub(crate) fn set_query(&mut self, query: ResolutionQueryKey) {
        if self.query.is_some() {
            self.non_admission =
                Some(verter_audit::NonAdmissionReason::ResolutionIncompleteProvenance);
            return;
        }
        self.query = Some(query);
    }

    pub(crate) fn query(&self) -> Option<&ResolutionQueryKey> {
        self.query.as_ref()
    }

    pub(crate) fn finish(self) -> SignatureAdmission {
        if self.query.is_none() {
            return SignatureAdmission::NonCacheable(
                verter_audit::NonAdmissionReason::ResolutionIncompleteProvenance,
            );
        }
        if let Some(reason) = self.non_admission {
            return SignatureAdmission::NonCacheable(reason);
        }
        crate::probe_tally!(OBS_PRE_DEDUP, self.observations.len());
        let mut facts = FactReadSet::new();
        facts.set_aggregate_basis(self.aggregate_basis);
        {
            crate::probe_scope!(FINISH_COLLECT);
            facts.observe_borrowed_signature(&self.observations);
        }
        SignatureAdmission::from_finalise(facts.finalise())
    }
}

pub(crate) fn ancestor_scopes(path: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = path;
    while let Some(index) = current.rfind('/') {
        let prefix = if index == 0 { "/" } else { &current[..index] };
        out.push(prefix.to_owned());
        if prefix == "/" {
            break;
        }
        current = prefix;
    }
    out
}

#[cfg(test)]
mod transaction_contract_tests {
    use super::*;

    fn captured_world() -> Arc<CapturedResolutionWorld> {
        Arc::new(CapturedResolutionWorld {
            base: Arc::new(ResolutionWorldRoot::bootstrap(ResolutionWorldId::fresh(1))),
            session: None,
            population: ResolutionPopulation::Base,
        })
    }

    fn query() -> ResolutionQueryKey {
        ResolutionQueryKey::importer(
            "/p/main.ts",
            "./dep",
            ResolutionContext {
                phase: ResolvePhase::ProviderGraph,
                kind: ResolveRequestKind::EsmImport,
            },
            ResolveContextId::from_hashes([0xA1; 16], [0xA2; 16]),
            ResolutionPopulation::Base,
        )
    }

    #[test]
    fn missing_or_duplicate_query_provenance_fails_closed() {
        let missing = ResolutionTransaction::new(captured_world()).finish();
        assert!(matches!(
            missing,
            SignatureAdmission::NonCacheable(
                verter_audit::NonAdmissionReason::ResolutionIncompleteProvenance
            )
        ));

        let mut duplicate = ResolutionTransaction::new(captured_world());
        duplicate.set_query(query());
        duplicate.set_query(query());
        assert!(matches!(
            duplicate.finish(),
            SignatureAdmission::NonCacheable(
                verter_audit::NonAdmissionReason::ResolutionIncompleteProvenance
            )
        ));

        // Mutation recipe: restore a debug_assert-only query check or discard
        // the query before finalisation. Release builds then mint a cacheable
        // empty signature and this typed refusal disappears.
    }

    /// `SIG-1`: signature CARDINALITY alone must never refuse admission.
    ///
    /// An over-cap observation set confined to ONE compaction domain lifts
    /// that domain's precise bucket to its terminal aggregate and admits.
    /// Written against existing API only, so it is a runnable red: at the
    /// pre-change tree it fails with
    /// `NonCacheable(SignatureOverflow)`.
    #[test]
    fn resolution_over_cap_single_domain_admits_instead_of_refusing() {
        let mut transaction = ResolutionTransaction::new(captured_world());
        transaction.set_query(query());
        for index in 0..=crate::FACT_SIGNATURE_CAP {
            transaction.observe(ResolutionFactKey::PathProbe {
                canonical: CanonicalResolutionId::new(format!("/p/{index}.ts")),
                population: ResolutionPopulation::Base,
            });
        }

        assert!(
            matches!(transaction.finish(), SignatureAdmission::Cacheable(_)),
            "SIG-1: an over-cap observation set confined to one compaction domain must \
             compact that domain and ADMIT — signature cardinality is never a refusal reason"
        );
    }

    /// `SIG-3`: compaction is DOMAIN-WISE. The over-cap domain lifts to a
    /// single terminal aggregate carrying its population; every other
    /// domain in the same signature stays precise.
    ///
    /// Replaces the former `resolution_overflow_uses_the_shared_fact_signature_cap`,
    /// which pinned the refusal this inverts.
    ///
    /// Mutation recipe: make `compact_domains` replace the WHOLE
    /// observation set instead of only the over-threshold buckets — the
    /// unrelated precise fact disappears and the last assertion fails.
    #[test]
    fn resolution_over_cap_lifts_only_its_own_domain() {
        let mut transaction = ResolutionTransaction::new(captured_world());
        transaction.set_query(query());
        for index in 0..=crate::FACT_SIGNATURE_CAP {
            transaction.observe(ResolutionFactKey::PathProbe {
                canonical: CanonicalResolutionId::new(format!("/p/{index}.ts")),
                population: ResolutionPopulation::Base,
            });
        }
        // A second domain, well under its own threshold. It must survive
        // the resolution domain's lifting untouched.
        let unrelated = FactVersionRef::ProjectGeneration { generation: 42 };
        transaction.observe_foreign_fact_for_test(unrelated.clone());

        let SignatureAdmission::Cacheable(signature) = transaction.finish() else {
            panic!("an over-cap single-domain observation set must compact, not refuse");
        };

        let aggregates: Vec<_> = signature
            .facts
            .iter()
            .filter_map(|fact| match fact {
                FactVersionRef::DomainGeneration(fact) => Some(*fact),
                _ => None,
            })
            .collect();
        assert_eq!(
            aggregates.len(),
            1,
            "exactly one terminal aggregate must stand in for the lifted domain; got \
             {aggregates:?}"
        );
        assert_eq!(
            aggregates[0].domain,
            crate::fact_cache::CompactionDomain::Resolution
        );
        assert_eq!(
            aggregates[0].population,
            crate::fact_cache::AggregatePopulation::Resolution(ResolutionPopulation::Base),
            "the aggregate must carry the population of the bucket it replaced"
        );
        assert!(
            signature.facts.contains(&unrelated),
            "SIG-3: lifting one domain must leave every OTHER domain precise — a \
             whole-signature collapse would coarsen this entry's workspace-shape \
             dependency and destroy warm reuse across unrelated edits"
        );
        assert!(
            !signature.facts.iter().any(|fact| matches!(
                fact,
                FactVersionRef::ResolveImports(ResolveImportsFactRef::Resolution(_))
            )),
            "the lifted domain's precise facts must be GONE, not merely joined by an \
             aggregate — otherwise nothing is bounded"
        );
    }

    /// Further observations in an already-lifted domain do not regrow its
    /// precise bucket: the aggregate absorbs them.
    ///
    /// Mutation recipe: run `compact_domains` BEFORE the dedup/merge in
    /// `canonicalise` instead of after — the already-present aggregate
    /// then sits beside reintroduced precise facts and this fails.
    #[test]
    fn a_lifted_resolution_domain_does_not_regrow() {
        let mut first = ResolutionTransaction::new(captured_world());
        first.set_query(query());
        for index in 0..=crate::FACT_SIGNATURE_CAP {
            first.observe(ResolutionFactKey::PathProbe {
                canonical: CanonicalResolutionId::new(format!("/p/{index}.ts")),
                population: ResolutionPopulation::Base,
            });
        }
        let SignatureAdmission::Cacheable(lifted) = first.finish() else {
            panic!("the first attempt must compact and admit");
        };
        assert_eq!(lifted.facts.len(), 1);

        // A later attempt carries that already-lifted aggregate AND
        // observes a handful of its own precise resolution facts.
        let mut second = ResolutionTransaction::new(captured_world());
        second.set_query(query());
        for fact in lifted.facts.iter() {
            second.observe_foreign_fact_for_test(fact.clone());
        }
        for index in 0..4 {
            second.observe(ResolutionFactKey::PathProbe {
                canonical: CanonicalResolutionId::new(format!("/p/late-{index}.ts")),
                population: ResolutionPopulation::Base,
            });
        }
        let SignatureAdmission::Cacheable(signature) = second.finish() else {
            panic!("the reuse attempt must admit");
        };
        assert_eq!(
            signature.facts.len(),
            1,
            "an already-lifted domain must not regrow a precise bucket beside its \
             aggregate; got {:?}",
            signature.facts
        );
        assert!(matches!(
            &signature.facts[0],
            FactVersionRef::DomainGeneration(fact)
                if fact.domain == crate::fact_cache::CompactionDomain::Resolution
        ));
    }

    /// `RC-4`: a session aggregate cannot validate against a base world,
    /// and a base aggregate cannot validate against a foreign session.
    ///
    /// Mutation recipe: drop the population equality check in
    /// `CapturedResolutionWorld::resolution_stamp` (return the base stamp
    /// for any session) — the cross-population assertions fail.
    #[test]
    fn resolution_aggregate_never_validates_across_populations() {
        use crate::fact_cache::{
            AggregatePopulation, CompactionDomain, DomainGenerationFact, FactVersionValidator,
        };

        let base_world = captured_world();
        let base_stamp = base_world
            .resolution_stamp(ResolutionPopulation::Base)
            .expect("a base world always answers for the base population");

        let base_aggregate = FactVersionRef::DomainGeneration(DomainGenerationFact {
            domain: CompactionDomain::Resolution,
            population: AggregatePopulation::Resolution(ResolutionPopulation::Base),
            stamp: base_stamp,
        });
        assert!(
            base_world.validates_fact_version(&base_aggregate),
            "a base aggregate must validate against the world that minted it"
        );

        let foreign_session = FactVersionRef::DomainGeneration(DomainGenerationFact {
            domain: CompactionDomain::Resolution,
            population: AggregatePopulation::Resolution(ResolutionPopulation::Session(
                SessionFingerprint::fresh(7),
            )),
            // Deliberately the numerically-matching generation: population
            // identity, not the number, is what must reject it.
            stamp: base_stamp,
        });
        assert!(
            !base_world.validates_fact_version(&foreign_session),
            "RC-4: a SESSION aggregate must not validate against a base world merely \
             because the generation number coincides"
        );

        let other_domain = FactVersionRef::DomainGeneration(DomainGenerationFact {
            domain: CompactionDomain::Content,
            population: AggregatePopulation::Resolution(ResolutionPopulation::Base),
            stamp: base_stamp,
        });
        assert!(
            !base_world.validates_fact_version(&other_domain),
            "the resolution world is authority for the RESOLUTION domain only — it must \
             never vouch for another domain's aggregate"
        );
    }

    /// The resolution world is authority over the RESOLUTION identity
    /// space only. An aggregate claiming its domain under a VIEW
    /// population — overlay installation, a space this world knows
    /// nothing about — is malformed and must be refused outright, not
    /// settled by a resolution-world stamp that happens to be current.
    ///
    /// Mutation recipe: in `CapturedResolutionWorld::validates_fact_version`,
    /// replace the `AggregatePopulation::View(_) => false` arm with
    /// `AggregatePopulation::View(_) => true`. Both assertions below fail;
    /// nothing else in the workspace suite does.
    #[test]
    fn a_view_population_aggregate_is_refused_by_the_resolution_world() {
        use crate::fact_cache::{
            AggregatePopulation, CompactionDomain, DomainGenerationFact, FactVersionValidator,
            SessionOverlayFingerprint, ViewPopulation,
        };

        let base_world = captured_world();
        let base_stamp = base_world
            .resolution_stamp(ResolutionPopulation::Base)
            .expect("a base world always answers for the base population");

        for view in [
            ViewPopulation::Base,
            ViewPopulation::SessionOverlay(
                SessionOverlayFingerprint::new(0x0BAD_CAFE).expect("non-zero"),
            ),
        ] {
            let malformed = FactVersionRef::DomainGeneration(DomainGenerationFact {
                domain: CompactionDomain::Resolution,
                population: AggregatePopulation::View(view),
                // The CURRENT stamp: only the population may reject it.
                stamp: base_stamp,
            });
            assert!(
                !base_world.validates_fact_version(&malformed),
                "a resolution-domain aggregate carrying a VIEW population is malformed — the \
                 resolution world must refuse it rather than vouch for an identity space it \
                 does not own; got acceptance for {view:?}"
            );
        }
    }

    fn path_probe(name: &str) -> ResolutionFactKey {
        ResolutionFactKey::PathProbe {
            canonical: CanonicalResolutionId::new(name.to_string()),
            population: ResolutionPopulation::Base,
        }
    }

    fn finished_signature(transaction: ResolutionTransaction) -> crate::ReadSetSignature {
        match transaction.finish() {
            SignatureAdmission::Cacheable(signature) => signature,
            other => panic!("expected a cacheable witness, got {other:?}"),
        }
    }

    /// `RC-2`: a reused warm candidate contributes exactly ONE typed
    /// decision fact, and every attempt-local observation survives beside
    /// it.
    ///
    /// Both halves matter. Without the first, the witness would restate
    /// the child's whole transitive leaf set — the growth this DAG
    /// exists to remove. Without the second, the witness would stop
    /// being path-precise for THIS demand.
    #[test]
    fn a_reused_child_decision_contributes_one_fact_and_keeps_local_observations() {
        // The child decision a previously admitted candidate published,
        // and the leaves it was computed from.
        let child_leaves = ["/p/b.ts", "/p/d.ts", "/p/f.ts"];
        let child = query();

        let mut attempt = ResolutionTransaction::new(captured_world());
        attempt.set_query(query());
        attempt.observe(path_probe("/p/a.ts"));
        attempt.observe(path_probe("/p/e.ts"));
        attempt.observe(ResolutionFactKey::decision(child.clone()));
        attempt.observe(path_probe("/p/z.ts"));
        let witness = finished_signature(attempt);

        let decisions: Vec<_> = witness
            .facts
            .iter()
            .filter(|fact| {
                matches!(
                    fact,
                    FactVersionRef::ResolveImports(ResolveImportsFactRef::Resolution(fact))
                        if matches!(fact.key, ResolutionFactKey::Decision { .. })
                )
            })
            .collect();
        assert_eq!(
            decisions.len(),
            1,
            "reusing a child decision must record exactly one derived fact; got {decisions:?}"
        );
        for leaf in child_leaves {
            assert!(
                !witness
                    .facts
                    .iter()
                    .any(|fact| fact.canonical_id() == Some(leaf)),
                "RC-2: the reused child's leaf `{leaf}` must NOT appear in the parent's \
                 witness — a decision records direct dependencies only, never a child's \
                 flattened signature"
            );
        }
        for local in ["/p/a.ts", "/p/e.ts", "/p/z.ts"] {
            assert!(
                witness
                    .facts
                    .iter()
                    .any(|fact| fact.canonical_id() == Some(local)),
                "the attempt's own observation of `{local}` must survive beside the child \
                 decision"
            );
        }

        // Mutation recipe: make `observe` fan a `Decision` key out into
        // the leaves the child recorded instead of recording the node.
        // The leaf-absence assertions fail immediately.
    }

    /// Observation order must not change the witness: finalisation is a
    /// set union under one canonical order, not an append.
    #[test]
    fn a_child_decision_witness_is_independent_of_observation_order() {
        let child = query();

        let mut decision_first = ResolutionTransaction::new(captured_world());
        decision_first.set_query(query());
        decision_first.observe(ResolutionFactKey::decision(child.clone()));
        decision_first.observe(path_probe("/p/c.ts"));
        decision_first.observe(path_probe("/p/a.ts"));

        let mut decision_last = ResolutionTransaction::new(captured_world());
        decision_last.set_query(query());
        decision_last.observe(path_probe("/p/a.ts"));
        decision_last.observe(path_probe("/p/c.ts"));
        decision_last.observe(ResolutionFactKey::decision(child.clone()));

        assert_eq!(
            finished_signature(decision_first).facts.as_ref(),
            finished_signature(decision_last).facts.as_ref()
        );
    }

    /// `DAG-1`: the observation operation classifies every fact it
    /// records. A direct leaf and a reused child decision both become
    /// direct edges; a fact minted by another domain's producer roots the
    /// witness and becomes no edge at all.
    ///
    /// Mutation recipe: give the `Terminal` arm of
    /// `classify_resolution_observation` the `DirectLeaf` disposition.
    /// The foreign observation then reaches the edge-bearing arm of
    /// `observe_fact`, whose `unreachable!` states the invariant, and
    /// this test panics.
    #[test]
    fn observation_classification_decides_every_recorded_fact_exactly_once() {
        let mut attempt = ResolutionTransaction::new(captured_world());
        attempt.set_query(query());
        attempt.observe(path_probe("/p/a.ts"));
        attempt.observe(ResolutionFactKey::decision(query()));
        attempt.observe_foreign_fact_for_test(FactVersionRef::ProjectGeneration { generation: 42 });

        let edges = attempt.direct_edges();
        assert_eq!(
            edges.len(),
            2,
            "exactly the direct leaf and the child decision are edges — a terminal \
             observation roots the witness and enters no edge; got {edges:?}"
        );
        assert!(edges.contains(&path_probe("/p/a.ts")));
        assert!(edges.iter().any(ResolutionFactKey::is_derived_node));

        let witness = finished_signature(attempt);
        assert!(
            witness
                .facts
                .contains(&FactVersionRef::ProjectGeneration { generation: 42 }),
            "a terminal observation must still root the witness"
        );
    }

    /// Every key that differs in ANY component is a distinct observation.
    ///
    /// Suppressing a repeat observation is only sound while the suppression
    /// is keyed on the WHOLE `ResolutionFactKey`. These keys agree on their
    /// most conspicuous component and differ elsewhere — same canonical with
    /// a different variant, a different population, a prefix relationship,
    /// an `ExactResolution` differing only in phase or in request kind — so a
    /// suppression keyed on canonical, on variant, or on any proper subset of
    /// the key collapses at least one pair and shortens the witness.
    fn distinguishing_keys() -> Vec<ResolutionFactKey> {
        let session = ResolutionPopulation::Session(SessionFingerprint::fresh(7));
        vec![
            ResolutionFactKey::PathProbe {
                canonical: CanonicalResolutionId::new("/p/a.ts"),
                population: ResolutionPopulation::Base,
            },
            // Same canonical, different population.
            ResolutionFactKey::PathProbe {
                canonical: CanonicalResolutionId::new("/p/a.ts"),
                population: session,
            },
            // Same canonical and population, different variant.
            ResolutionFactKey::Manifest {
                canonical: CanonicalResolutionId::new("/p/a.ts"),
                population: ResolutionPopulation::Base,
            },
            ResolutionFactKey::DirectoryMembers {
                canonical: CanonicalResolutionId::new("/p/a.ts"),
                population: ResolutionPopulation::Base,
            },
            ResolutionFactKey::Realpath {
                requested: CanonicalResolutionId::new("/p/a.ts"),
                population: ResolutionPopulation::Base,
            },
            ResolutionFactKey::RecoveryScope {
                canonical_prefix: CanonicalResolutionId::new("/p/a.ts"),
                population: ResolutionPopulation::Base,
            },
            // Prefix relationship with the canonical above.
            ResolutionFactKey::PathProbe {
                canonical: CanonicalResolutionId::new("/p/a.ts.map"),
                population: ResolutionPopulation::Base,
            },
            ResolutionFactKey::ContextSelection {
                entry: ResolutionEntry::Importer(CanonicalResolutionId::new("/p/a.ts")),
                population: ResolutionPopulation::Base,
            },
            // Three exact-resolution keys agreeing on entry+specifier and
            // differing only in phase, then only in request kind.
            ResolutionFactKey::ExactResolution {
                entry: ResolutionEntry::Importer(CanonicalResolutionId::new("/p/a.ts")),
                specifier: RawSpecifier::new("./dep"),
                phase: ResolvePhase::ProviderGraph,
                kind: ResolveRequestKind::EsmImport,
                population: ResolutionPopulation::Base,
            },
            ResolutionFactKey::ExactResolution {
                entry: ResolutionEntry::Importer(CanonicalResolutionId::new("/p/a.ts")),
                specifier: RawSpecifier::new("./dep"),
                phase: ResolvePhase::CodegenBlocker,
                kind: ResolveRequestKind::EsmImport,
                population: ResolutionPopulation::Base,
            },
            ResolutionFactKey::ExactResolution {
                entry: ResolutionEntry::Importer(CanonicalResolutionId::new("/p/a.ts")),
                specifier: RawSpecifier::new("./dep"),
                phase: ResolvePhase::ProviderGraph,
                kind: ResolveRequestKind::TypeImport,
                population: ResolutionPopulation::Base,
            },
        ]
    }

    /// A key observed many times is recorded once, and the witness is
    /// byte-identical to the one a caller that never repeated itself mints.
    ///
    /// Both halves matter. Without the second, a suppression that dropped a
    /// DISTINCT key would still pass the "records once" half.
    #[test]
    fn repeat_observations_collapse_to_the_same_witness_as_observing_each_key_once() {
        let keys = distinguishing_keys();

        // Every key observed once, in order.
        let mut once = ResolutionTransaction::new(captured_world());
        once.set_query(query());
        for key in &keys {
            once.observe(key.clone());
        }
        let once = finished_signature(once);
        assert_eq!(
            once.facts.len(),
            keys.len(),
            "each key in the fixture must survive as its own observation"
        );

        // The same keys, each observed several times, interleaved and in a
        // different order — the shape a recovery-chain walk produces.
        let mut repeated = ResolutionTransaction::new(captured_world());
        repeated.set_query(query());
        for round in 0..4 {
            for key in keys.iter().rev().skip(round % 3).chain(keys.iter()) {
                repeated.observe(key.clone());
            }
        }
        let repeated = finished_signature(repeated);

        assert_eq!(
            repeated.facts.as_ref(),
            once.facts.as_ref(),
            "repeating an observation must change neither the witness contents nor its order"
        );

        // Mutation recipe: key the suppression on the fact's canonical id, on
        // its variant, or on any proper subset of the key. The near-miss
        // pairs above then collapse and `once.facts.len()` drops below the
        // fixture size.
    }

    /// Record-time suppression must not disturb the DIRECT EDGE set: a key
    /// observed many times contributes exactly one edge, and a repeated
    /// child-decision observation contributes exactly one derived edge.
    ///
    /// Mutation recipe: push into `direct_edges` before the
    /// already-observed check in `observe` — the edge set then grows with
    /// every repeat and both length assertions fail.
    #[test]
    fn repeat_observations_do_not_disturb_the_direct_edge_set() {
        let keys = distinguishing_keys();

        let mut attempt = ResolutionTransaction::new(captured_world());
        attempt.set_query(query());
        for round in 0..4 {
            for key in keys.iter().rev().skip(round % 3).chain(keys.iter()) {
                attempt.observe(key.clone());
            }
            attempt.observe(ResolutionFactKey::decision(query()));
        }
        let edges = attempt.direct_edges();
        let witness = finished_signature(attempt);

        assert_eq!(
            edges.len(),
            keys.len() + 1,
            "every distinct key contributes exactly one direct edge, plus the one child \
             decision; got {edges:?}"
        );
        assert_eq!(
            witness.facts.len(),
            keys.len() + 1,
            "the witness and the edge set agree on cardinality — they are produced by the \
             same operation"
        );
    }
}

#[cfg(test)]
mod root_graph_tests {
    use super::*;

    fn leaf(name: &str) -> ResolutionFactKey {
        ResolutionFactKey::PathProbe {
            canonical: CanonicalResolutionId::new(name.to_string()),
            population: ResolutionPopulation::Base,
        }
    }

    fn node(specifier: &str) -> ResolutionFactKey {
        ResolutionFactKey::decision(ResolutionQueryKey::importer(
            "/p/main.ts",
            specifier,
            ResolutionContext {
                phase: ResolvePhase::ProviderGraph,
                kind: ResolveRequestKind::EsmImport,
            },
            ResolveContextId::from_hashes([0xA1; 16], [0xA2; 16]),
            ResolutionPopulation::Base,
        ))
    }

    fn sorted(mut keys: Vec<ResolutionFactKey>) -> Vec<ResolutionFactKey> {
        keys.sort();
        keys
    }

    /// A minting counter that mirrors the Engine's: strictly increasing,
    /// never `INITIAL`.
    fn minter() -> impl FnMut() -> ResolutionFactVersion {
        let mut next = 0_u64;
        move || {
            next += 1;
            ResolutionFactVersion::fresh(next)
        }
    }

    /// **Publication replaces the COMPLETE edge set atomically and mints
    /// nothing**, so a node a request just published is rootable against
    /// that request's own captured view.
    ///
    /// Mutation recipe: make `publish_derived` merge into the existing
    /// edge set instead of replacing it (skip `detach_edges` and seed
    /// `direct` from the current set). The dropped-dependency assertion
    /// fails.
    #[test]
    fn republishing_a_decision_replaces_its_whole_edge_set_and_advances_nothing() {
        let mut root = ResolutionFactRoot::default();
        let decision = node("./dep");

        assert!(!root.publish_derived(decision.clone(), [leaf("/p/a.ts"), leaf("/p/b.ts")]));
        assert_eq!(
            root.version(&decision),
            ResolutionFactVersion::INITIAL,
            "publication mints no version: a resolution publishes its OWN decision, so a \
             minted version would be one no view captured before that resolution can hold, \
             and every consumer rooting on it would miss against the very request view it \
             computed under"
        );
        assert_eq!(
            sorted(root.direct_dependencies(&decision).expect("edges")),
            sorted(vec![leaf("/p/a.ts"), leaf("/p/b.ts")])
        );

        // A recomputation over a DIFFERENT edge set.
        assert!(root.publish_derived(decision.clone(), [leaf("/p/b.ts"), leaf("/p/c.ts")]));

        assert_eq!(
            sorted(root.direct_dependencies(&decision).expect("edges")),
            sorted(vec![leaf("/p/b.ts"), leaf("/p/c.ts")]),
            "the replacement is of the COMPLETE edge set, not a union with the prior one"
        );
        assert!(
            root.direct_dependents(&leaf("/p/a.ts")).is_empty(),
            "the dropped dependency's reverse edge must be detached in the same operation \
             — a stale reverse edge would keep propagating into a node that no longer \
             depends on it"
        );
        assert_eq!(
            root.direct_dependents(&leaf("/p/c.ts")),
            vec![decision.clone()],
            "the new dependency's reverse edge must be attached in the same operation"
        );
        assert_eq!(root.direct_dependents(&leaf("/p/b.ts")), vec![decision]);
    }

    /// A node is never recorded as its own dependency.
    ///
    /// The resolve path reaches this whenever an attempt that reused its
    /// own prior answer goes on to republish: the reused answer is the
    /// SAME decision, not a child of itself. A self-edge would make the
    /// node a dependent of the very key that seeds its own propagation.
    ///
    /// Mutation recipe: replace the `if dependency == node { continue; }`
    /// guard in `publish_derived` with `if false { continue; }`. Both
    /// assertions fail.
    #[test]
    fn a_decision_is_never_recorded_as_its_own_dependency() {
        let mut root = ResolutionFactRoot::default();
        let decision = node("./dep");

        root.publish_derived(decision.clone(), [leaf("/p/a.ts"), decision.clone()]);

        assert_eq!(
            root.direct_dependencies(&decision).expect("edges"),
            vec![leaf("/p/a.ts")],
            "a self-dependency must be dropped, not recorded"
        );
        assert!(
            root.direct_dependents(&decision).is_empty(),
            "and no reverse self-edge may be attached"
        );
    }

    /// **Removal ADVANCES the version and drops both edge directions.**
    ///
    /// The advance is the ABA rail: publication mints nothing, so a node
    /// that left the graph and was later republished would otherwise
    /// return to a version a witness already holds. The tombstone means
    /// every witness recorded at any prior version — `INITIAL`
    /// included — stops validating, and a reintroduction keeps the
    /// tombstone rather than reverting to it.
    ///
    /// Mutation recipe: make `remove_derived` skip
    /// `self.advance(node.clone(), version)`. The tombstone assertions
    /// fail.
    #[test]
    fn removing_a_decision_advances_its_version_and_drops_both_edge_directions() {
        let mut mint = minter();
        let mut root = ResolutionFactRoot::default();
        let decision = node("./dep");
        root.publish_derived(decision.clone(), [leaf("/p/a.ts")]);
        assert_eq!(root.version(&decision), ResolutionFactVersion::INITIAL);

        assert!(root.remove_derived(&decision, mint()));
        let tombstone = root.version(&decision);
        assert_ne!(
            tombstone,
            ResolutionFactVersion::INITIAL,
            "a removed node must NOT fall back to INITIAL — that is the version a witness \
             recorded before the node was ever published holds, so reverting to it would \
             re-validate a witness the removal must invalidate"
        );
        assert!(root.direct_dependencies(&decision).is_none());
        assert!(
            root.direct_dependents(&leaf("/p/a.ts")).is_empty(),
            "removal detaches the reverse edges too, so a later mutation of the leaf \
             propagates into nothing"
        );
        assert!(
            !root.remove_derived(&decision, mint()),
            "removing an absent node reports no removal"
        );

        // Reintroduction keeps the tombstone: no ABA.
        root.publish_derived(decision.clone(), [leaf("/p/a.ts")]);
        assert_eq!(
            root.version(&decision),
            tombstone,
            "a reintroduced node keeps its tombstone version — a witness recorded before \
             the removal must never validate again"
        );
    }

    /// **Propagation advances each reachable derived node exactly once
    /// per batch, and terminates on a cycle.**
    ///
    /// Mutation recipe: drop the `visited.insert(..)` guard in
    /// `propagate`. The cycle case then does not terminate.
    #[test]
    fn resolution_decision_cycle_advances_each_node_once() {
        let mut mint = minter();
        let mut root = ResolutionFactRoot::default();
        let a = node("./a");
        let b = node("./b");
        let owner = node("./owner-set");

        // a -> leaf, b -> a, owner -> {a, b}, and a -> owner closes a cycle.
        root.publish_derived(a.clone(), [leaf("/p/dep.ts"), owner.clone()]);
        root.publish_derived(b.clone(), [a.clone()]);
        root.publish_derived(owner.clone(), [a.clone(), b.clone()]);

        let before: Vec<_> = [&a, &b, &owner].map(|key| root.version(key)).into();
        let advanced = root.propagate([leaf("/p/dep.ts")], &mut mint);

        assert_eq!(
            sorted(advanced.clone()),
            sorted(vec![a.clone(), b.clone(), owner.clone()]),
            "every reachable derived node advances"
        );
        assert_eq!(
            advanced.len(),
            3,
            "and each advances exactly ONCE per batch, despite the cycle; got {advanced:?}"
        );
        for (key, was) in [&a, &b, &owner].into_iter().zip(before) {
            assert_ne!(root.version(key), was, "{key:?} must advance");
        }
    }

    /// A mutation of an unrelated leaf advances nothing.
    ///
    /// Mutation recipe: seed `propagate` with every key in the reverse
    /// map instead of the batch's own seeds. This fails.
    #[test]
    fn resolution_decision_unrelated_leaf_advance_reaches_no_node() {
        let mut mint = minter();
        let mut root = ResolutionFactRoot::default();
        let decision = node("./dep");
        root.publish_derived(decision.clone(), [leaf("/p/a.ts")]);
        let before = root.version(&decision);

        let advanced = root.propagate([leaf("/p/unrelated.ts")], &mut mint);

        assert!(advanced.is_empty(), "got {advanced:?}");
        assert_eq!(root.version(&decision), before);
    }

    /// A `RecoveryScope` is a DIRECT LEAF, not a derived node: it is a
    /// coarse ancestor input a decision depends on, and an imprecise
    /// watcher mutation of it must reach every decision beneath it.
    #[test]
    fn only_the_two_derived_families_classify_as_derived_nodes() {
        let derived = [
            node("./dep"),
            ResolutionFactKey::OwnerResolutionSet {
                owner: CanonicalResolutionId::new("/p/main.ts"),
                population: ResolutionPopulation::Base,
            },
        ];
        let leaves = [
            leaf("/p/a.ts"),
            ResolutionFactKey::Manifest {
                canonical: CanonicalResolutionId::new("/p/package.json"),
                population: ResolutionPopulation::Base,
            },
            ResolutionFactKey::Realpath {
                requested: CanonicalResolutionId::new("/p/a.ts"),
                population: ResolutionPopulation::Base,
            },
            ResolutionFactKey::DirectoryMembers {
                canonical: CanonicalResolutionId::new("/p"),
                population: ResolutionPopulation::Base,
            },
            ResolutionFactKey::RecoveryScope {
                canonical_prefix: CanonicalResolutionId::new("/p"),
                population: ResolutionPopulation::Base,
            },
            ResolutionFactKey::context_importer("/p/main.ts", ResolutionPopulation::Base),
            ResolutionFactKey::exact_importer(
                "/p/main.ts",
                "./dep",
                ResolutionContext {
                    phase: ResolvePhase::ProviderGraph,
                    kind: ResolveRequestKind::EsmImport,
                },
                ResolutionPopulation::Base,
            ),
        ];
        for key in derived {
            assert!(key.is_derived_node(), "{key:?} must be a derived node");
            assert_eq!(
                classify_resolution_observation(&FactVersionRef::ResolveImports(
                    ResolveImportsFactRef::Resolution(ResolutionFactRef {
                        key: key.clone(),
                        version: ResolutionFactVersion::INITIAL,
                    })
                )),
                ResolutionEdgeClass::DerivedNode
            );
        }
        for key in leaves {
            assert!(!key.is_derived_node(), "{key:?} must be a direct leaf");
            assert_eq!(
                classify_resolution_observation(&FactVersionRef::ResolveImports(
                    ResolveImportsFactRef::Resolution(ResolutionFactRef {
                        key: key.clone(),
                        version: ResolutionFactVersion::INITIAL,
                    })
                )),
                ResolutionEdgeClass::DirectLeaf,
                "{key:?}"
            );
        }
    }

    /// The row table is BOUNDED, and it says so when it drops rows.
    ///
    /// The tally behind `CTX-1` lives in the same rows as the memoized
    /// answers, so a silent clear would restart it and a path this index
    /// walked many times would read "walked once". Every clear is
    /// counted, and every `CTX-1` assertion reads that count first.
    ///
    /// Mutation recipe: drop the `table_clears` increment beside
    /// `self.rows.clear()`. The clear then happens invisibly and this
    /// fails.
    #[test]
    fn context_selection_memo_is_bounded_and_reports_its_clears() {
        let memo = PublishedContextSelection::with_cap(2);
        let unowned = || Ok(ResolveContextId::unowned());

        assert_eq!(
            memo.selected("/a.ts", unowned),
            Ok(ResolveContextId::unowned())
        );
        assert_eq!(
            memo.selected("/b.ts", unowned),
            Ok(ResolveContextId::unowned())
        );
        assert_eq!(memo.table_clears(), 0, "two rows fit inside a cap of two");
        assert_eq!(memo.evaluations("/a.ts"), 1);

        // The third distinct path overflows: the table is dropped whole.
        assert_eq!(
            memo.selected("/c.ts", unowned),
            Ok(ResolveContextId::unowned())
        );
        assert_eq!(
            memo.table_clears(),
            1,
            "an overflowing insert must COUNT the clear — an uncounted one makes every \
             per-path tally read as if the index had walked once"
        );
        assert_eq!(
            memo.evaluations("/a.ts"),
            0,
            "and the dropped rows really are gone, so the count is honest about what it \
             no longer knows"
        );

        // Dropping rows costs a recompute and nothing else: the answer is
        // recomputed, identical, and memoized again.
        assert_eq!(
            memo.selected("/a.ts", unowned),
            Ok(ResolveContextId::unowned())
        );
        assert_eq!(memo.evaluations("/a.ts"), 1);
    }
}

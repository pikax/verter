//! Resolution-fact identity vocabulary: the closed key/entry/context/query
//! taxonomy `ResolutionFactRef`/`FactVersionRef::ResolveImports(Resolution)`
//! carries.
//!
//! This is immutable identity IR only — cache authority (the fact ledger,
//! mutation propagation, version counters, `ResolutionTransaction`, replay
//! ledgers, validators, invalidation, publication) stays workspace/
//! session-owned.

use crate::resolution::dto::{
    ProviderTarget, ResolutionContext, ResolutionKind, ResolvePhase, ResolveRequestKind,
    ResolveResult,
};
use crate::resolution::resolution_world_identity::ResolutionPopulation;

use std::sync::Arc;

/// A canonical path a resolution fact names.
///
/// Shared text: cloning a fact that names it, which every read-set merge
/// does, shares the one allocation instead of copying it. Equality,
/// ordering and hashing are the text's.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CanonicalResolutionId(Arc<str>);

impl CanonicalResolutionId {
    pub fn new(value: impl Into<Arc<str>>) -> Self {
        Self(value.into())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A normalized specifier, as shared text (see [`CanonicalResolutionId`]).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NormalizedSpecifier(Arc<str>);

impl NormalizedSpecifier {
    /// Normalizes a raw specifier: a relative specifier (leading `.`/`\`,
    /// or an embedded `/./`/`\.\`) drops a single trailing slash; anything
    /// else has backslashes rewritten to forward slashes only.
    pub fn new(value: impl AsRef<str>) -> Self {
        Self(normalize_specifier(value.as_ref()))
    }
}

/// A raw specifier, as shared text (see [`CanonicalResolutionId`]).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RawSpecifier(Arc<str>);

impl RawSpecifier {
    pub fn new(value: impl Into<Arc<str>>) -> Self {
        Self(value.into())
    }
}

fn normalize_specifier(value: &str) -> Arc<str> {
    if value.starts_with('.')
        || value.starts_with('\\')
        || value.contains("/./")
        || value.contains("\\.\\")
    {
        // Trailing-slash trim for a relative specifier only — a bare
        // string transform with no filesystem/workspace access, kept
        // narrow rather than routed through `verter_workspace`'s audited
        // `normalize_relative_specifier` so this module names no
        // workspace type.
        if value.len() > 1 && value.ends_with('/') {
            Arc::from(&value[..value.len() - 1])
        } else {
            Arc::from(value)
        }
    } else {
        Arc::from(value.replace('\\', "/"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProjectIdentity(pub [u8; 16]);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ResolverPolicyIdentity(pub [u8; 16]);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProviderPolicyIdentity(pub [u8; 16]);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ResolveEnvHash(pub [u8; 16]);

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
    pub fn from_hashes(project_identity: [u8; 16], resolve_env_hash: [u8; 16]) -> Self {
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

    #[must_use]
    pub const fn project_identity(&self) -> ProjectIdentity {
        self.project_identity
    }

    /// Stable context identity for an entry no configured project owns.
    ///
    /// "No owning project" is a complete observation of the published
    /// project-selection index — a real observed value, not a missing
    /// observation. The identity is a fixed constant so it never derives
    /// from the project set: a project republish that does not change the
    /// entry's (non-)ownership must keep this context and its version.
    #[must_use]
    pub fn unowned() -> Self {
        Self::from_hashes([0xA1; 16], [0xA2; 16])
    }

    #[must_use]
    pub fn with_provider_projection(mut self, target: &Self) -> Self {
        self.provider_policy_identity = ProviderPolicyIdentity(target.project_identity.0);
        self
    }

    #[must_use]
    pub fn with_external_provider_projection(mut self, result: &ResolveResult) -> Self {
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
            ProviderTarget::SourceFile => 0,
            ProviderTarget::CarrierPublicApi => 1,
            ProviderTarget::ShadowSourceFile => 2,
        });
        identity.push(match result.resolution_kind {
            ResolutionKind::Relative => 0,
            ResolutionKind::TsConfigPath => 1,
            ResolutionKind::ProjectReference => 2,
            ResolutionKind::NodeModules => 3,
            ResolutionKind::PackageExports => 4,
            ResolutionKind::PackageImports => 5,
            ResolutionKind::WorkspaceAlias => 6,
            ResolutionKind::Bundler => 7,
            ResolutionKind::PlaygroundMap => 8,
        });
        self.provider_policy_identity =
            ProviderPolicyIdentity(xxhash_rust::xxh3::xxh3_128(&identity).to_le_bytes());
        self
    }

    #[must_use]
    pub fn identity_parts(&self) -> ([u8; 16], [u8; 16], [u8; 16]) {
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
    pub(crate) entry: ResolutionEntry,
    normalized_specifier: NormalizedSpecifier,
    phase: ResolvePhase,
    request_kind: ResolveRequestKind,
    context: ResolveContextId,
    pub(crate) population: ResolutionPopulation,
}

impl ResolutionQueryKey {
    pub fn importer(
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

    pub fn explicit(
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

    #[must_use]
    pub fn context(&self) -> &ResolveContextId {
        &self.context
    }
}

/// Version of one resolution-observable fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ResolutionFactVersion(u64);

impl ResolutionFactVersion {
    pub const INITIAL: Self = Self(0);

    /// Mint a real (non-zero) fact version. The host's fact ledger is the
    /// sole production caller.
    ///
    /// # Panics
    /// Panics if `raw` is `0` — resolution fact versions must be non-zero.
    pub fn fresh(raw: u64) -> Self {
        assert_ne!(raw, 0, "resolution fact versions must be non-zero");
        Self(raw)
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
    /// decisions it reused — live in the workspace-owned resolution-fact
    /// root, and a mutation reaching any of them advances this node
    /// through reverse propagation (cache authority, not this module's
    /// concern).
    ///
    /// The identity is shared, so cloning the key shares it.
    Decision { query: Arc<ResolutionQueryKey> },
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

impl ResolutionFactKey {
    /// Build the derived node key for one query identity.
    pub fn decision(query: ResolutionQueryKey) -> Self {
        Self::Decision {
            query: Arc::new(query),
        }
    }

    /// Build the derived node key for one owner's decision set. Takes an
    /// already-normalized owner canonical id — normalization is a
    /// workspace-owned path concern, not this module's.
    pub fn owner_resolution_set(
        owner_canonical: CanonicalResolutionId,
        population: ResolutionPopulation,
    ) -> Self {
        Self::OwnerResolutionSet {
            owner: owner_canonical,
            population,
        }
    }

    /// Heap this key owns beyond its inline bytes: its canonical and
    /// specifier texts and a decision's shared query identity, with that
    /// identity's own texts, each once per `aliases` (see
    /// [`crate::retention::allocation::FactMeasure`]).
    #[must_use]
    pub fn owned_heap_bytes(&self, aliases: &mut crate::retention::allocation::AliasSet) -> usize {
        self.measure_heap(&mut crate::retention::allocation::FactMeasure::new(aliases))
    }

    /// [`Self::owned_heap_bytes`] within one fact measurement.
    pub(crate) fn measure_heap<M: crate::retention::allocation::FactMeter>(
        &self,
        m: &mut crate::retention::allocation::FactMeasure<'_, M>,
    ) -> usize {
        let entry = |entry: &ResolutionEntry,
                     m: &mut crate::retention::allocation::FactMeasure<'_, M>| {
            match entry {
                ResolutionEntry::Importer(canonical) => m.text(&canonical.0),
                ResolutionEntry::ExplicitProject(_) => 0,
            }
        };
        match self {
            Self::PathProbe { canonical, .. }
            | Self::Manifest { canonical, .. }
            | Self::DirectoryMembers { canonical, .. } => m.text(&canonical.0),
            Self::Realpath { requested, .. } => m.text(&requested.0),
            Self::RecoveryScope {
                canonical_prefix, ..
            } => m.text(&canonical_prefix.0),
            Self::OwnerResolutionSet { owner, .. } => m.text(&owner.0),
            Self::ExactResolution {
                entry: owner,
                specifier,
                ..
            } => entry(owner, m) + m.text(&specifier.0),
            Self::ContextSelection { entry: owner, .. } => entry(owner, m),
            Self::Decision { query } => {
                if !m.first_owned(query) {
                    return 0;
                }
                crate::retention::allocation::arc_block_bytes(
                    std::mem::size_of::<ResolutionQueryKey>(),
                ) + entry(&query.entry, m)
                    + m.text(&query.normalized_specifier.0)
            }
        }
    }

    /// Whether this key names a derived DAG node rather than a primitive
    /// resolution input.
    #[must_use]
    pub fn is_derived_node(&self) -> bool {
        matches!(
            self,
            Self::Decision { .. } | Self::OwnerResolutionSet { .. }
        )
    }

    #[must_use]
    pub fn population(&self) -> ResolutionPopulation {
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

    #[must_use]
    pub fn in_population(&self, population: ResolutionPopulation) -> Self {
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
            Self::Decision { query } => Arc::make_mut(query).population = population,
        }
        key
    }

    #[must_use]
    pub fn canonical_id(&self) -> Option<&str> {
        match self {
            Self::PathProbe { canonical, .. }
            | Self::Manifest { canonical, .. }
            | Self::DirectoryMembers { canonical, .. } => Some(canonical.as_str()),
            Self::Realpath { requested, .. } => Some(requested.as_str()),
            Self::RecoveryScope {
                canonical_prefix, ..
            } => Some(canonical_prefix.as_str()),
            Self::OwnerResolutionSet { owner, .. } => Some(owner.as_str()),
            Self::ExactResolution { entry, .. } | Self::ContextSelection { entry, .. } => {
                match entry {
                    ResolutionEntry::Importer(canonical) => Some(canonical.as_str()),
                    ResolutionEntry::ExplicitProject(_) => None,
                }
            }
            Self::Decision { query } => match &query.entry {
                ResolutionEntry::Importer(canonical) => Some(canonical.as_str()),
                ResolutionEntry::ExplicitProject(_) => None,
            },
        }
    }

    /// The importer canonical owned by a derived decision node.
    #[must_use]
    pub fn owner_canonical(&self) -> Option<&CanonicalResolutionId> {
        match self {
            Self::Decision { query } => match &query.entry {
                ResolutionEntry::Importer(canonical) => Some(canonical),
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
    #[must_use]
    pub fn reobservable_path_canonical_id(&self) -> Option<&str> {
        match self {
            Self::PathProbe { canonical, .. } | Self::Manifest { canonical, .. } => {
                Some(canonical.as_str())
            }
            Self::Realpath { requested, .. } => Some(requested.as_str()),
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

    pub fn exact_importer(
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

    pub fn context_importer(importer_id: &str, population: ResolutionPopulation) -> Self {
        Self::ContextSelection {
            entry: ResolutionEntry::Importer(CanonicalResolutionId::new(importer_id)),
            population,
        }
    }

    pub fn exact_explicit(
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

    pub fn context_explicit(project: ProjectIdentity, population: ResolutionPopulation) -> Self {
        Self::ContextSelection {
            entry: ResolutionEntry::ExplicitProject(project),
            population,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ResolutionFactRef {
    pub key: ResolutionFactKey,
    pub version: ResolutionFactVersion,
}

impl ResolutionFactRef {
    pub fn new(key: ResolutionFactKey, version: ResolutionFactVersion) -> Self {
        Self { key, version }
    }

    /// Whether this ref names the OWNER-SCOPED resolution set.
    #[must_use]
    pub fn is_owner_resolution_set(&self) -> bool {
        matches!(self.key, ResolutionFactKey::OwnerResolutionSet { .. })
    }

    /// Whether this ref names a `Decision` derived node.
    #[must_use]
    pub fn is_decision(&self) -> bool {
        matches!(self.key, ResolutionFactKey::Decision { .. })
    }
}

#[cfg(test)]
#[path = "resolution_tests.rs"]
mod tests;

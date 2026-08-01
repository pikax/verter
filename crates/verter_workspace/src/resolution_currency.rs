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
    FactReadSet, FactVersionRef, FactVersionValidator, ResolveImportsFactRef, SignatureAdmission,
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
}

impl ResolutionFactKey {
    pub(crate) fn population(&self) -> ResolutionPopulation {
        match self {
            Self::PathProbe { population, .. }
            | Self::Manifest { population, .. }
            | Self::Realpath { population, .. }
            | Self::ExactResolution { population, .. }
            | Self::DirectoryMembers { population, .. }
            | Self::RecoveryScope { population, .. }
            | Self::ContextSelection { population, .. } => *population,
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
            } => *current = population,
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
            Self::ExactResolution { entry, .. } | Self::ContextSelection { entry, .. } => {
                match entry {
                    ResolutionEntry::Importer(canonical) => Some(&canonical.0),
                    ResolutionEntry::ExplicitProject(_) => None,
                }
            }
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
            | Self::ContextSelection { .. } => None,
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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ResolutionFactRef {
    pub(crate) key: ResolutionFactKey,
    pub(crate) version: ResolutionFactVersion,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ResolutionFactRoot {
    versions: HashMap<ResolutionFactKey, ResolutionFactVersion>,
}

impl ResolutionFactRoot {
    pub(crate) fn version(&self, key: &ResolutionFactKey) -> ResolutionFactVersion {
        self.versions
            .get(key)
            .copied()
            .unwrap_or(ResolutionFactVersion::INITIAL)
    }

    pub(crate) fn advance(&mut self, key: ResolutionFactKey, version: ResolutionFactVersion) {
        self.versions.insert(key, version);
    }

    pub(crate) fn remove(&mut self, key: &ResolutionFactKey) {
        self.versions.remove(key);
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
    pub(crate) fn replace_published(
        &mut self,
        published: Arc<PublishedRoot>,
        mut fresh_version: impl FnMut() -> ResolutionFactVersion,
    ) {
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

pub(crate) fn selected_context_for_path(
    world: &ResolutionWorldRoot,
    canonical_id: &str,
) -> Result<ResolveContextId, ContextProvenanceError> {
    let published = world
        .published
        .as_ref()
        .ok_or(ContextProvenanceError::NoPublishedRoot)?;
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
            ResolveImportsFactRef::Resolution(fact) => self.fact_version(&fact.key) == fact.version,
            ResolveImportsFactRef::Semantic { .. } => false,
        }
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
        let FactVersionRef::ResolveImports(fact) = fact else {
            return false;
        };
        self.validates_resolve_imports_fact(fact)
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
    observations: Vec<FactVersionRef>,
    observed_values: ObservedResolutionValues,
    non_admission: Option<verter_audit::NonAdmissionReason>,
    query: Option<ResolutionQueryKey>,
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
        Self {
            root,
            observations: Vec::new(),
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
        let version = self.root.fact_version(&key);
        self.observations.push(FactVersionRef::ResolveImports(
            ResolveImportsFactRef::Resolution(ResolutionFactRef { key, version }),
        ));
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

    pub(crate) fn absorb(&mut self, signature: &crate::ReadSetSignature) {
        self.observations.extend(signature.facts.iter().cloned());
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
        let mut facts = FactReadSet::new();
        facts.observe_borrowed_signature(&self.observations);
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

    #[test]
    fn resolution_overflow_uses_the_shared_fact_signature_cap() {
        let mut transaction = ResolutionTransaction::new(captured_world());
        transaction.set_query(query());
        for index in 0..=crate::FACT_SIGNATURE_CAP {
            transaction.observe(ResolutionFactKey::PathProbe {
                canonical: CanonicalResolutionId::new(format!("/p/{index}.ts")),
                population: ResolutionPopulation::Base,
            });
        }

        assert!(matches!(
            transaction.finish(),
            SignatureAdmission::NonCacheable(verter_audit::NonAdmissionReason::SignatureOverflow)
        ));

        // Mutation recipe: add a resolution-specific cap/fallback or convert
        // overflow into ReadSetSignature::empty(). This assertion then becomes
        // cacheable instead of using the one shared overflow convention.
    }
}

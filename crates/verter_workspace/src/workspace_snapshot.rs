//! Immutable workspace snapshot: single-authority ownership, resolution, and membership.
//!
//! All generation-dependent workspace facts live in ONE immutable [`WorkspaceSnapshot`],
//! published atomically via a single `ArcSwap::store()` on the VFS Engine.
//!
//! # Invariants
//!
//! - A request observes only one snapshot (loaded once, used for entire lifetime)
//! - No partial publication is ever visible (one ArcSwap, one store)
//! - Exactly one ownership authority (the published snapshot)
//! - Configured ownership comes from immutable materialized project file sets
//! - Overlap ambiguity is preserved — no synthetic primary owner
//! - `owners_for_file()` ordering is precomputed during snapshot build

use smallvec::SmallVec;

use crate::canonical_path::CanonicalPath;
use crate::membership::{ConfiguredMembership, FallbackMembership};
use crate::resolver::{IdeProjectCompilerOptions, ProjectResolver, WorkspaceAlias};

/// Index into [`WorkspaceSnapshot::projects`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ProjectId(pub u32);

/// Monotonic generation counter for published snapshots.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct SnapshotGeneration(pub u64);

impl SnapshotGeneration {
    pub fn next(self) -> Self {
        Self(self.0 + 1)
    }
}

/// An immutable workspace snapshot. All ownership queries, resolution, and
/// membership are consistent within a single snapshot.
#[derive(Debug)]
pub struct WorkspaceSnapshot {
    /// All projects, pre-sorted by precedence (longest root first,
    /// Configured before Fallback at same root, alphabetical tiebreak).
    pub projects: Vec<OwnershipProject>,
    /// Import resolver built from all project configs.
    pub resolver: ProjectResolver,
    /// Monotonic generation counter.
    pub generation: SnapshotGeneration,
}

/// A single project in the workspace snapshot.
#[derive(Debug)]
pub struct OwnershipProject {
    /// Index in the parent `WorkspaceSnapshot.projects` vec.
    pub id: ProjectId,
    /// Project root directory (canonical).
    pub root: CanonicalPath,
    /// Workspace root that discovered this project.
    pub workspace_root: CanonicalPath,
    /// Configured vs Fallback — type-enforced invariant.
    pub payload: ProjectPayload,
}

/// Type-enforced project kind: configured (tsconfig-backed) or fallback.
///
/// This encodes the "fallback never implies configured settings" invariant
/// directly in the type system. Code that needs compiler options must match
/// `Configured { .. }` — there is no way to accidentally read options from
/// a fallback project.
#[derive(Debug)]
pub enum ProjectPayload {
    Configured {
        tsconfig_path: CanonicalPath,
        membership: ConfiguredMembership,
        compiler_options: IdeProjectCompilerOptions,
        references: Vec<CanonicalPath>,
        workspace_aliases: Vec<WorkspaceAlias>,
    },
    Fallback {
        membership: FallbackMembership,
    },
}

/// Result of querying configured owners for a file.
#[derive(Debug, PartialEq, Eq)]
pub enum ConfiguredOwnerResolution {
    /// No configured project claims this file.
    None,
    /// Exactly one configured project claims this file.
    Unique(ProjectId),
    /// Multiple configured projects claim this file (real overlap).
    Ambiguous(SmallVec<[ProjectId; 2]>),
}

impl WorkspaceSnapshot {
    /// All projects claiming this file via exact snapshot membership.
    ///
    /// Ordered by pre-sorted project precedence (precomputed during build,
    /// no per-query sorting). Most specific root first, Configured before
    /// Fallback at same root.
    ///
    /// Pure function — depends only on snapshot state.
    pub fn owners_for_file(&self, canonical_id: &str) -> SmallVec<[ProjectId; 2]> {
        let path = CanonicalPath::new(canonical_id);
        let mut result = SmallVec::new();
        let mut has_configured_owner = false;

        for project in &self.projects {
            match &project.payload {
                ProjectPayload::Configured { membership, .. } => {
                    if membership.contains(&path) {
                        result.push(project.id);
                        has_configured_owner = true;
                    }
                }
                ProjectPayload::Fallback { membership } => {
                    // Fallback only claims if no configured project claimed the file
                    if !has_configured_owner && membership.contains(&path) {
                        result.push(project.id);
                    }
                }
            }
        }

        result
    }

    /// Exact configured-owner resolution.
    ///
    /// Never invents a synthetic primary: ambiguous overlap stays ambiguous.
    /// This is the sole entry point for consumers that want a single configured owner.
    pub fn configured_owner_resolution_for_file(
        &self,
        canonical_id: &str,
    ) -> ConfiguredOwnerResolution {
        let owners = self.owners_for_file(canonical_id);
        let configured: SmallVec<[ProjectId; 2]> = owners
            .into_iter()
            .filter(|id| {
                matches!(
                    self.projects[id.0 as usize].payload,
                    ProjectPayload::Configured { .. }
                )
            })
            .collect();

        // Nearest-root effective ownership: a configured candidate whose root
        // is a STRICT ANCESTOR of another matching candidate's root loses —
        // `extends`/breadth at an ancestor root must not make a descendant
        // package file ambiguous when a descendant configured project also
        // claims it. After pruning ancestors, what remains is either exactly
        // one config (UNIQUE), or multiple configs at the same root / at
        // incomparable roots (genuine overlap → AMBIGUOUS).
        let effective: SmallVec<[ProjectId; 2]> = configured
            .iter()
            .copied()
            .filter(|candidate| {
                let candidate_root = &self.projects[candidate.0 as usize].root;
                // Keep the candidate only if no OTHER matching candidate has a
                // strictly-deeper root that contains this candidate's root.
                !configured.iter().any(|other| {
                    if other == candidate {
                        return false;
                    }
                    let other_root = &self.projects[other.0 as usize].root;
                    // `other` strictly under `candidate` ⇒ candidate is an
                    // ancestor ⇒ drop the ancestor candidate.
                    other_root.starts_with_dir(candidate_root)
                        && other_root.as_str().len() > candidate_root.as_str().len()
                })
            })
            .collect();

        match effective.len() {
            0 => ConfiguredOwnerResolution::None,
            1 => ConfiguredOwnerResolution::Unique(effective[0]),
            _ => ConfiguredOwnerResolution::Ambiguous(effective),
        }
    }

    /// Resolve a file to a single owner only when that owner is unambiguous.
    ///
    /// - Unique configured owner -> `Some(ProjectId)`
    /// - Ambiguous configured owners -> `None`
    /// - No configured owners -> a single fallback owner if exactly one exists
    pub fn single_owner_for_file(&self, canonical_id: &str) -> Option<ProjectId> {
        match self.configured_owner_resolution_for_file(canonical_id) {
            ConfiguredOwnerResolution::Unique(id) => Some(id),
            ConfiguredOwnerResolution::Ambiguous(_) => None,
            ConfiguredOwnerResolution::None => {
                let owners = self.owners_for_file(canonical_id);
                (owners.len() == 1).then(|| owners[0])
            }
        }
    }

    /// Get a project by ID.
    pub fn project(&self, id: ProjectId) -> &OwnershipProject {
        &self.projects[id.0 as usize]
    }

    /// Check if a project is configured (tsconfig-backed).
    pub fn is_configured(&self, id: ProjectId) -> bool {
        matches!(
            self.projects[id.0 as usize].payload,
            ProjectPayload::Configured { .. }
        )
    }

    /// Get the tsconfig path for a project, if configured.
    pub fn tsconfig_path(&self, id: ProjectId) -> Option<&CanonicalPath> {
        match &self.projects[id.0 as usize].payload {
            ProjectPayload::Configured { tsconfig_path, .. } => Some(tsconfig_path),
            ProjectPayload::Fallback { .. } => None,
        }
    }
}

impl OwnershipProject {
    /// Whether this project is configured (tsconfig-backed).
    pub fn is_configured(&self) -> bool {
        matches!(self.payload, ProjectPayload::Configured { .. })
    }

    /// Whether this project is a fallback (no tsconfig).
    pub fn is_fallback(&self) -> bool {
        matches!(self.payload, ProjectPayload::Fallback { .. })
    }
}

/// Canonical precedence spec for project ordering.
///
/// Applied once during snapshot construction:
/// 1. Root specificity: longest root prefix first
/// 2. Kind: Configured before Fallback at the same root
/// 3. Deterministic: alphabetical by tsconfig path
pub(crate) fn compare_project_precedence(
    a: &OwnershipProject,
    b: &OwnershipProject,
) -> std::cmp::Ordering {
    // Longest root first
    b.root
        .as_str()
        .len()
        .cmp(&a.root.as_str().len())
        .then_with(|| {
            // Configured before Fallback at same root
            let a_configured = a.is_configured() as u8;
            let b_configured = b.is_configured() as u8;
            b_configured.cmp(&a_configured) // true (1) > false (0), so reverse
        })
        .then_with(|| {
            // Alphabetical by tsconfig path for determinism
            let a_path = match &a.payload {
                ProjectPayload::Configured { tsconfig_path, .. } => tsconfig_path.as_str(),
                ProjectPayload::Fallback { .. } => "",
            };
            let b_path = match &b.payload {
                ProjectPayload::Configured { tsconfig_path, .. } => tsconfig_path.as_str(),
                ProjectPayload::Fallback { .. } => "",
            };
            a_path.cmp(b_path)
        })
}

#[cfg(test)]
#[path = "workspace_snapshot_tests.rs"]
mod tests;

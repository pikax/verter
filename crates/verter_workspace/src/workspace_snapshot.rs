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
//! - `owners_for_file()` results are memoized per snapshot ([`OwnersMemo`]:
//!   bounded, negative-caching, owned by — and dying with — the snapshot)

use dashmap::DashMap;
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
    /// Bounded memo for [`Self::owners_for_file`] — see [`OwnersMemo`].
    pub owners_memo: OwnersMemo,
}

/// Maximum number of memoized [`WorkspaceSnapshot::owners_for_file`] entries
/// per snapshot before an overflowing insert clears the memo.
pub const OWNERS_MEMO_CAP: usize = 16 * 1024;

/// Bounded, snapshot-owned memo for [`WorkspaceSnapshot::owners_for_file`].
///
/// `owners_for_file` is a pure function of immutable snapshot state, so its
/// results are memoizable for exactly the snapshot's lifetime: the memo lives
/// ON the snapshot and dies with it — a new published snapshot starts cold,
/// so no cross-generation invalidation exists, by construction.
///
/// Properties:
/// - **Negative caching**: empty owner sets are cached too — "no owner" costs
///   the same precedence walk + glob matching as a positive answer.
/// - **Bounded**: at most `cap` entries; an overflowing insert clears the map
///   first (approximate under concurrency — this is a memo, not an authority,
///   so dropping entries is always correct and only costs a recompute).
/// - **Randomized hashing**: the default `RandomState` (SipHash) `DashMap`
///   hasher — keys are caller-influenced path strings, so a deterministic
///   hasher would be a collision hazard.
pub struct OwnersMemo {
    map: DashMap<Box<str>, SmallVec<[ProjectId; 2]>>,
    cap: usize,
}

impl OwnersMemo {
    /// Memo bounded at `cap` entries. Production uses [`OWNERS_MEMO_CAP`]
    /// via [`Default`]; tests use tiny caps to exercise clear-on-overflow.
    pub fn with_cap(cap: usize) -> Self {
        Self {
            map: DashMap::new(),
            cap,
        }
    }

    fn get(&self, canonical_id: &str) -> Option<SmallVec<[ProjectId; 2]>> {
        self.map
            .get(canonical_id)
            .map(|entry| entry.value().clone())
    }

    fn insert(&self, canonical_id: &str, owners: SmallVec<[ProjectId; 2]>) {
        if self.map.len() >= self.cap {
            self.map.clear();
        }
        self.map.insert(Box::from(canonical_id), owners);
    }
}

impl Default for OwnersMemo {
    fn default() -> Self {
        Self::with_cap(OWNERS_MEMO_CAP)
    }
}

impl std::fmt::Debug for OwnersMemo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OwnersMemo")
            .field("entries", &self.map.len())
            .field("cap", &self.cap)
            .finish()
    }
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
    /// Pure function — depends only on snapshot state. Results (including
    /// empty owner sets) are served from the snapshot-owned [`OwnersMemo`]
    /// after first compute, so repeated classification traffic skips the
    /// per-call path canonicalization and glob matching.
    pub fn owners_for_file(&self, canonical_id: &str) -> SmallVec<[ProjectId; 2]> {
        if let Some(memoized) = self.owners_memo.get(canonical_id) {
            return memoized;
        }
        let owners = self.compute_owners_for_file(canonical_id);
        self.owners_memo.insert(canonical_id, owners.clone());
        owners
    }

    /// The uncached owner walk backing [`Self::owners_for_file`].
    fn compute_owners_for_file(&self, canonical_id: &str) -> SmallVec<[ProjectId; 2]> {
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

        // Deterministic graph pruning: a configured candidate is dropped when
        // another co-claiming candidate DOMINATES it. Domination is either:
        //
        //   (a) nearest-root — the other candidate has a STRICTLY-DEEPER root
        //       that contains this candidate's root. `extends`/breadth at an
        //       ancestor root must not make a descendant package file ambiguous
        //       when a descendant configured project also claims it.
        //   (b) solution-graph — this candidate transitively `references` the
        //       other candidate, i.e. it is a solution aggregator / referenced
        //       non-leaf pulling in the real leaf owner. TypeScript solution
        //       style: the referenced leaf owns the file, the aggregator only
        //       pulls it in through project references.
        //
        // After pruning, what remains is either exactly one config (UNIQUE), or
        // multiple configs at the same / incomparable roots with no reference
        // relationship (a genuine overlap → AMBIGUOUS). Selection is never
        // lexical / scan-order / load-order — only the deterministic graph
        // rules above decide.
        let effective: SmallVec<[ProjectId; 2]> = configured
            .iter()
            .copied()
            .filter(|candidate| {
                let candidate_root = &self.projects[candidate.0 as usize].root;
                !configured.iter().any(|other| {
                    if other == candidate {
                        return false;
                    }
                    let other_root = &self.projects[other.0 as usize].root;
                    // (a) `other` strictly under `candidate` ⇒ candidate is an
                    // ancestor ⇒ drop the ancestor candidate.
                    if other_root.starts_with_dir(candidate_root)
                        && other_root.as_str().len() > candidate_root.as_str().len()
                    {
                        return true;
                    }
                    // (b) SOLUTION-GRAPH domination: candidate transitively
                    // `references` the co-claiming `other`, i.e. candidate is a
                    // solution aggregator / referenced non-leaf pulling in the real
                    // leaf owner ⇒ drop the aggregator in favour of the leaf. This
                    // fires ONLY for a STRICT reference relationship (a DAG edge),
                    // never a cyclic tie: a malformed `A references B` / `B
                    // references A` pair puts both configs in ONE reference SCC where
                    // each transitively reaches the other, so neither strictly
                    // dominates. Condensing the SCC — requiring `other` NOT to
                    // reference `candidate` back — keeps a genuine cycle `Ambiguous`
                    // over both candidates rather than collapsing it (each dropping
                    // the other) to `None`.
                    match (
                        &self.projects[candidate.0 as usize].payload,
                        &self.projects[other.0 as usize].payload,
                    ) {
                        (
                            ProjectPayload::Configured {
                                tsconfig_path: candidate_tsconfig,
                                ..
                            },
                            ProjectPayload::Configured {
                                tsconfig_path: other_tsconfig,
                                ..
                            },
                        ) => {
                            self.configured_references_transitively(*candidate, other_tsconfig)
                                && !self
                                    .configured_references_transitively(*other, candidate_tsconfig)
                        }
                        _ => false,
                    }
                })
            })
            .collect();

        match effective.len() {
            0 => ConfiguredOwnerResolution::None,
            1 => ConfiguredOwnerResolution::Unique(effective[0]),
            _ => ConfiguredOwnerResolution::Ambiguous(effective),
        }
    }

    /// Whether the configured project `from` reaches `target_tsconfig` through
    /// the project-`references` graph (directly or transitively).
    ///
    /// Used by [`Self::configured_owner_resolution_for_file`] to drop a
    /// solution aggregator / referenced non-leaf in favour of the co-claiming
    /// leaf it pulls in. The reference graph is a DAG for valid TypeScript
    /// configs; the `visited` set makes traversal terminate even on a malformed
    /// cyclic reference set.
    fn configured_references_transitively(
        &self,
        from: ProjectId,
        target_tsconfig: &CanonicalPath,
    ) -> bool {
        let mut visited: SmallVec<[ProjectId; 8]> = SmallVec::new();
        let mut stack: SmallVec<[ProjectId; 8]> = SmallVec::new();
        stack.push(from);
        while let Some(id) = stack.pop() {
            if visited.contains(&id) {
                continue;
            }
            visited.push(id);
            let ProjectPayload::Configured { references, .. } =
                &self.projects[id.0 as usize].payload
            else {
                continue;
            };
            for reference in references {
                if reference == target_tsconfig {
                    return true;
                }
                if let Some(next) = self.configured_project_by_tsconfig(reference) {
                    if !visited.contains(&next) {
                        stack.push(next);
                    }
                }
            }
        }
        false
    }

    /// Find the configured project whose tsconfig path equals `tsconfig`.
    fn configured_project_by_tsconfig(&self, tsconfig: &CanonicalPath) -> Option<ProjectId> {
        self.projects
            .iter()
            .find_map(|project| match &project.payload {
                ProjectPayload::Configured { tsconfig_path, .. } if tsconfig_path == tsconfig => {
                    Some(project.id)
                }
                _ => None,
            })
    }

    /// The single FALLBACK (tsconfig-less) owner for a file, or `None` when there is
    /// no fallback owner or the fallback ownership is itself ambiguous (>1).
    ///
    /// This is the SEPARATE candidate-preserving fallback resolution the non-carrier
    /// LSP context (per-project linter view / `projectRootPath` / SSR detection) and
    /// the intrinsic-projection cache anchor consult ONLY after an authoritative
    /// configured-`None`. It NEVER returns a configured project (carrier ownership is
    /// `verter_session`'s `CarrierOwnershipResolution`), and it never invents a winner
    /// for overlapping fallbacks. `owners_for_file` already suppresses fallbacks when
    /// any configured project claims the file, so on a configured-`None` the owners it
    /// reports are exactly the fallback owners.
    pub fn single_fallback_owner_for_file(&self, canonical_id: &str) -> Option<ProjectId> {
        let owners = self.owners_for_file(canonical_id);
        (owners.len() == 1).then(|| owners[0])
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

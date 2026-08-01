//! The snapshot-backed [`ConfiguredOwnerAuthority`] — the one place a provider's
//! per-file project binding (root + owning config) is decided.
//!
//! A workspace FOLDER is an editor concept; a configured PROJECT is a
//! TypeScript one. They coincide only in the single-package layout. In a pnpm
//! monorepo opened as one folder, every nested package is its own configured
//! project with its own `tsconfig.json` and its own `node_modules` — so a
//! project root derived from folder membership names `/ws` for
//! `/ws/packages/app/src/App.vue.tsx`, and a provider that resolves TypeScript
//! from that root reports `packages/app/node_modules/typescript` absent.
//!
//! Ownership therefore comes from
//! [`WorkspaceSnapshot::default_configured_owner_for_file`] — the shared,
//! provider-neutral tsgo `GetDefaultProject` decision the tsserver,
//! managed-tsgo, and shared-tsgo carrier routes already consume. This adapter
//! adds no second owner-selection engine; its ONLY addition is to reverse-map a
//! generated carrier COMPANION (`Foo.vue.tsx`, `Foo.d.vue.ts`,
//! `Foo.vue.verter.ts`, …) to its real carrier SOURCE through the descriptor
//! authority, because a generated companion is not itself a program file and the
//! snapshot answers about the source.
//!
//! When the snapshot names no owner the answer is the TERMINAL
//! [`ProjectOwnership::NoProject`] — never a nearest-ancestor or folder-derived
//! substitute. A file no configured program contains is precisely the contract's
//! `NoProject` state, and answering it with the deepest configured project that
//! happens to sit above it on disk invents membership the snapshot denied: that
//! project's `include`/`files` do not cover the file, so its compiler options,
//! its `node_modules`, and its path aliases are not the ones that apply. The
//! other carrier routes fail closed there; so does this one.

use std::sync::Arc;

use verter_session::framework::descriptor::classify_carrier_companion;
use verter_type_runtime::traits::{ConfiguredOwner, ConfiguredOwnerAuthority, ProjectOwnership};
use verter_workspace::workspace_snapshot::SnapshotGeneration;
use verter_workspace::{PublishedRoot, WorkspaceSnapshot};

/// The ownership generations observed by one request.
///
/// This is deliberately feature-neutral: any provider-backed operation whose
/// safety proof depends on a published ownership root can capture it before an
/// await and validate it before consuming the provider response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OwnershipGenerationWitness {
    published_root: SnapshotGeneration,
    provider_authority: Option<SnapshotGeneration>,
}

/// Tracks the exact snapshot generation most recently handed to the provider.
///
/// Background initialization marks the new generation immediately before the
/// synchronous provider-authority swap. That ordering makes the marker lead the
/// provider by at most one call instruction: a request may conservatively refuse
/// just before the swap, but it can never observe the provider on a newer graph
/// while the marker still vouches for the old one.
#[derive(Debug, Default)]
pub(crate) struct OwnershipGenerationFence {
    provider_authority: parking_lot::RwLock<Option<SnapshotGeneration>>,
}

impl OwnershipGenerationFence {
    /// Begin installing `generation` as the provider's ownership authority.
    pub(crate) fn begin_provider_install(&self, generation: SnapshotGeneration) {
        *self.provider_authority.write() = Some(generation);
    }

    /// Capture a coherent ready-root/provider pair.
    ///
    /// `None` means ownership is either bootstrap-cold or currently crossing the
    /// provider/root publication boundary. An untracked provider is allowed for
    /// provider-free sessions and direct unit fixtures; if background init later
    /// installs one, validation of the captured `None` witness fails.
    pub(crate) fn capture(&self, published: &PublishedRoot) -> Option<OwnershipGenerationWitness> {
        if !published.ownership_ready {
            return None;
        }
        let provider_authority = *self.provider_authority.read();
        if provider_authority.is_some_and(|generation| generation != published.snapshot.generation)
        {
            return None;
        }
        Some(OwnershipGenerationWitness {
            published_root: published.snapshot.generation,
            provider_authority,
        })
    }

    /// Whether `witness` still names the live ready-root/provider pair.
    pub(crate) fn validates(
        &self,
        witness: OwnershipGenerationWitness,
        published: Option<&PublishedRoot>,
    ) -> bool {
        let Some(published) = published else {
            return false;
        };
        published.ownership_ready
            && published.snapshot.generation == witness.published_root
            && *self.provider_authority.read() == witness.provider_authority
    }
}

/// [`ConfiguredOwnerAuthority`] backed by the published workspace snapshot.
pub struct SnapshotOwnerAuthority {
    snapshot: Arc<WorkspaceSnapshot>,
}

impl SnapshotOwnerAuthority {
    pub fn new(snapshot: Arc<WorkspaceSnapshot>) -> Self {
        Self { snapshot }
    }

    /// The configured project whose PROGRAM contains `file`.
    fn program_owner(&self, file: &str) -> Option<ConfiguredOwner> {
        let id = self.snapshot.default_configured_owner_for_file(file)?;
        let project = self.snapshot.project(id);
        let verter_workspace::workspace_snapshot::ProjectPayload::Configured {
            tsconfig_path, ..
        } = &project.payload
        else {
            // `default_configured_owner_for_file` only ever names a configured
            // project; a fallback row here would mean the snapshot changed shape.
            return None;
        };
        Some(ConfiguredOwner {
            root: project.root.as_str().to_string(),
            config_path: tsconfig_path.as_str().to_string(),
        })
    }
}

impl ConfiguredOwnerAuthority for SnapshotOwnerAuthority {
    fn configured_owner(&self, canonical_id: &str) -> ProjectOwnership {
        let file = verter_span::path::canonicalize_path(canonical_id);

        // A generated companion is not a program file; its OWNER is its source's
        // owner. Reverse-map through the descriptor authority, never a suffix
        // guess.
        let source = classify_carrier_companion(&file).map(|companion| companion.source);

        if let Some(source) = source.as_deref() {
            if let Some(owner) = self.program_owner(source) {
                return ProjectOwnership::Owned(owner);
            }
        }
        match self.program_owner(&file) {
            Some(owner) => ProjectOwnership::Owned(owner),
            // No configured program contains it. Terminal.
            None => ProjectOwnership::NoProject,
        }
    }
}

#[cfg(test)]
#[path = "configured_owner_tests.rs"]
mod tests;

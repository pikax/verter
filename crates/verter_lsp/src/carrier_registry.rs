//! The live wiring of the `CarrierRegistry` seam onto the provider-surface store.
//!
//! The `CarrierRegistry` trait + its `CarrierArtifact` DTO are defined in
//! `verter_session::external_ts`, but the authoritative content store
//! ([`ProviderSurfaceStore`]) lives in `verter_lsp` (which depends ON
//! `verter_session`), so the contract trait cannot reach the live store from its
//! own crate. This module closes that gap: [`StoreBackedCarrierRegistry`]
//! implements the `verter_session::external_ts::CarrierRegistry` trait BACKED BY
//! the existing [`ProviderSurfaceStore`] — NO second content store. It is a thin
//! read-through: it maps a source URI to its companion provider path (via a
//! descriptor-derived path resolver the caller supplies) and reads the store's
//! CURRENT snapshot. The registry is read-only and never mutates the store.

use std::sync::Arc;

use verter_session::external_ts::{CarrierArtifact, CarrierRegistry, CarrierRole};

use crate::provider_surface_store::{
    ProviderSurfaceKind, ProviderSurfaceSnapshot, ProviderSurfaceStore,
};

/// Maps a source URI to the companion provider path the engine type-checks it at
/// (the descriptor-owned `{name}.vue.tsx` / `{name}.vue.verter.ts` identity). The
/// caller supplies this (it owns the `VirtualFileNaming` descriptor authority);
/// the registry stays framework-agnostic by not hardcoding a suffix.
pub trait CarrierPathResolver {
    /// The provider path for `source_uri` at `role`, or `None` if the source has
    /// no carrier of that role.
    fn provider_path_for(&self, source_uri: &str, role: CarrierRole) -> Option<String>;
}

impl<F> CarrierPathResolver for F
where
    F: Fn(&str, CarrierRole) -> Option<String>,
{
    fn provider_path_for(&self, source_uri: &str, role: CarrierRole) -> Option<String> {
        self(source_uri, role)
    }
}

/// A [`CarrierRegistry`] backed by the single [`ProviderSurfaceStore`]. Resolves
/// `source_uri` → companion provider path → the store's current snapshot →
/// [`CarrierArtifact`]. The default role served is [`CarrierRole::CarrierIde`]
/// (the bare-import-probed interactive surface); [`Self::carrier_for_role`] reads
/// a specific role.
pub struct StoreBackedCarrierRegistry<R: CarrierPathResolver> {
    store: ProviderSurfaceStore,
    path_resolver: R,
}

impl<R: CarrierPathResolver> StoreBackedCarrierRegistry<R> {
    /// Build the registry over a (cheap `Arc`-clone of the) store and a
    /// descriptor-derived path resolver.
    #[must_use]
    pub fn new(store: ProviderSurfaceStore, path_resolver: R) -> Self {
        Self {
            store,
            path_resolver,
        }
    }

    /// The carrier artifact for `source_uri` at a SPECIFIC role, or `None` if the
    /// source has no carrier of that role currently in the store.
    ///
    /// `CarrierBatch` is MERGED into `CarrierIde` (the measurement found no
    /// material cold-perf gain and no valid diagnostic-preserving minimal codegen
    /// profile): there is NO distinct `CarrierBatch` storage slot, so a
    /// `CarrierBatch` request reads the `CarrierIde` surface — the cold TSC path
    /// and the interactive path resolve to the SAME stored carrier. The returned
    /// artifact reports the REQUESTED `role` (the caller asked for batch) while
    /// reading the IDE-stored content; they are one surface.
    ///
    /// FAIL CLOSED on every uncertainty: no companion path, no current snapshot, a
    /// stored kind that does not match the effective read role, a snapshot whose
    /// `source_canonical` does not match the requested `source_uri` (a resolver /
    /// path bug must never serve a DIFFERENT source's carrier), or a snapshot with
    /// no recorded `project_owner` (a project-bound contract result must not be
    /// served from a surface that was not recorded under a resolved project
    /// binding) — each returns `None`.
    #[must_use]
    pub fn carrier_for_role(&self, source_uri: &str, role: CarrierRole) -> Option<CarrierArtifact> {
        // Merge alias: CarrierBatch reads the CarrierIde surface (one surface).
        let read_role = effective_read_role(role);
        let provider_path = self
            .path_resolver
            .provider_path_for(source_uri, read_role)?;
        let snapshot = self.store.current_snapshot(&provider_path)?;
        // The snapshot's recorded kind must match the EFFECTIVE read role — the
        // store is the single authority, so a kind mismatch means there is no
        // carrier of that role at that path (fail closed).
        if !role_matches_kind(read_role, snapshot.kind) {
            return None;
        }
        // Source-identity gate: the stored surface must belong to the requested
        // source. A resolver / path-mapping bug that pointed at another source's
        // companion would otherwise leak the wrong carrier into the project-bound
        // contract (fail closed).
        if &*snapshot.source_canonical != source_uri {
            return None;
        }
        // Project-bound gate: a `CarrierArtifact` served into the project-bound
        // contract must come from a surface recorded under a resolved project
        // binding (it carries a project owner). A legacy `project_owner: None`
        // rename-mapping record must NOT leak into the contract (fail closed).
        snapshot.project_owner.as_ref()?;
        // The artifact reports the REQUESTED role (batch stays batch) over the
        // IDE-stored content under the merge.
        Some(artifact_from_snapshot(role, &provider_path, &snapshot))
    }
}

/// The store role a request for `requested` actually READS. `CarrierBatch` is
/// merged into `CarrierIde`, so a batch request reads the `CarrierIde` slot; every
/// other role reads its own slot. This is the single merge-alias point.
#[must_use]
fn effective_read_role(requested: CarrierRole) -> CarrierRole {
    match requested {
        CarrierRole::CarrierBatch => CarrierRole::CarrierIde,
        other => other,
    }
}

impl<R: CarrierPathResolver> CarrierRegistry for StoreBackedCarrierRegistry<R> {
    fn carrier_for(&self, source_uri: &str) -> Option<CarrierArtifact> {
        // The default surface a bare import resolves to is the interactive IDE
        // carrier.
        self.carrier_for_role(source_uri, CarrierRole::CarrierIde)
    }
}

/// Whether a contract [`CarrierRole`] matches a stored [`ProviderSurfaceKind`].
/// The two enums are kept distinct (the contract DTO does not depend on the store
/// enum); this is the single mapping point.
#[must_use]
fn role_matches_kind(role: CarrierRole, kind: ProviderSurfaceKind) -> bool {
    matches!(
        (role, kind),
        (CarrierRole::CarrierIde, ProviderSurfaceKind::CarrierIde)
            | (CarrierRole::CarrierApi, ProviderSurfaceKind::CarrierApi)
            | (CarrierRole::CarrierBatch, ProviderSurfaceKind::CarrierBatch)
            | (CarrierRole::Shadow, ProviderSurfaceKind::Shadow)
            | (CarrierRole::Real, ProviderSurfaceKind::Real)
    )
}

/// Build a [`CarrierArtifact`] from a current store snapshot. The content hash is
/// the snapshot's 16-byte content identity; `map_hash` is the recorded source-map
/// identity; `version` is the snapshot generation.
#[must_use]
fn artifact_from_snapshot(
    role: CarrierRole,
    provider_path: &str,
    snapshot: &ProviderSurfaceSnapshot,
) -> CarrierArtifact {
    CarrierArtifact {
        provider_uri: Arc::from(provider_path),
        role,
        content: Arc::clone(&snapshot.provider_content),
        content_hash: snapshot.stamp.content_hash.to_hash16(),
        map_hash: snapshot.stamp.map_hash,
        version: snapshot.stamp.generation,
    }
}

#[cfg(test)]
#[path = "carrier_registry_tests.rs"]
mod tests;

//! The capability-limited, immutable observation interface used by the
//! resolver kernel.
//!
//! This is a separate sealed trait. It does not extend
//! `verter_session::resolver_core::ResolverContext` and cannot name
//! `VerterHost` or any scheduler type, because `verter_semantic`'s
//! dependency closure cannot reach those types (they live in a crate that
//! depends on this one, never the reverse).
//!
//! Every method returns [`crate::resolver_core::AttemptOutcome`] — never a
//! bare value, a `Result`, or a call that can block. A non-conforming method
//! therefore fails at compile time rather than escaping a sampled runtime
//! test.

use crate::analysis::flow::FunctionBodySkeleton;
use crate::analysis::types::Hash16;
use crate::resolver_core::{
    AttemptOutcome, AugmentationTargetKey, CanonicalId, EnvHashes, FlowFunctionObservationKey,
    LoweredTypeDecl, LoweredValueDecl, ModuleAugmentationIndexObservation,
    ResolutionPackageManifest, StoreViewProjectIdentity,
};
use std::sync::Arc;

/// Private marker `ResolverObservation` is sealed against. Only types
/// inside `verter_semantic` (or, later, a dependency-neutral test double in
/// this crate's own test tree) can implement `ResolverObservation`.
///
/// `pub(crate)` (not fully private) so a sibling module within
/// `verter_semantic` — e.g. [`crate::resolver_core::ResolverAttemptView`]
/// — can implement [`Sealed`] too. This does NOT weaken the seal against
/// EXTERNAL crates: `pub(crate)` is still crate-restricted, so an
/// out-of-crate implementor still cannot name this module.
///
/// This is a separate seal from `verter_session::resolver_core`'s
/// `sealed::Sealed` — the two traits are unrelated types with independent
/// authority, not a shared hierarchy.
pub(crate) mod sealed {
    pub trait Sealed {}
}

/// The I/O-free, host-free observation interface used by
/// [`crate::resolver_core::ModuleResolverCore`].
///
/// Sealed via [`sealed::Sealed`]; implementable only from inside this
/// crate. Every method returns `AttemptOutcome<T>`. The exhaustive test
/// double lives at
/// `resolver_core::observation::observation_tests::TestDouble` — every
/// method added here must be implemented there for the crate to compile,
/// which is the actual coverage proof.
pub trait ResolverObservation: sealed::Sealed {
    /// Env-hash bundle (R21) for `canonical`, or the project-default
    /// bundle when `canonical` is `None`. `EnvHashes` is a dependency-neutral
    /// value containing four plain `Hash16` fields.
    fn env_hashes(&self, canonical: Option<&str>) -> AttemptOutcome<EnvHashes>;

    /// Workspace-default project identity (R21) for `canonical`, or the
    /// project-default identity when `canonical` is `None`.
    fn project_identity(&self, canonical: Option<&str>)
        -> AttemptOutcome<StoreViewProjectIdentity>;

    /// The tracked whole-content hash for `canonical`. `Complete(None)` is
    /// the honest "genuinely untracked" structural fact (contract §7's
    /// stable-missing case — never itself a reason to request more
    /// inputs); `NeedInputs` means trackedness itself is not yet known
    /// from the attempt's current observation set.
    fn whole_hash(&self, canonical: &str) -> AttemptOutcome<Option<Hash16>>;

    /// Whether `canonical` is package-backed per the workspace's
    /// resolver-classification (NOT a path-substring check on
    /// `node_modules` — CLAUDE.md's Macro Type Traversal Rule). True only
    /// when the realpath sits under `node_modules/` AND no registered
    /// project root claims the file.
    fn workspace_is_package_backed(&self, canonical: &str) -> AttemptOutcome<bool>;

    /// Ambient-library declaration lookup for `symbol`, scoped to
    /// `consumer_project`. Both the hit and project key are semantic-owned,
    /// dependency-neutral resolver vocabulary.
    fn lookup_ambient_symbol(
        &self,
        consumer_project: crate::resolver_core::ProjectStableKey,
        symbol: &str,
    ) -> AttemptOutcome<Option<crate::resolver_core::AmbientSymbolHit>>;

    /// The live project-shape/config/env/identity generation counter
    /// (`ProjectTypeStore::project_generation`) — one of `project_type_store()`'s
    /// five real sub-accessors; a plain `u64`,
    /// never the whole `Arc<ProjectTypeStore>` (which does NOT cross this
    /// interface).
    fn project_generation(&self) -> AttemptOutcome<u64>;

    /// The lazily lowered body of the TYPE declaration `name` in
    /// `canonical`, owned by `owner`. Backed by
    /// `verter_session::decl_body_memo::DeclBodyMemo::peek_type_decl` — a
    /// non-blocking peek that NEVER triggers the worker-thread lowering
    /// rendezvous. `Complete(None)` covers both "not inventoried" and
    /// "already demanded, committed empty" (both stable, cacheable facts);
    /// `NeedInputs` covers "not yet demanded" and asks the driver to supply
    /// the typed missing observation.
    fn type_decl(
        &self,
        canonical: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        name: &str,
    ) -> AttemptOutcome<Option<Arc<LoweredTypeDecl>>>;

    /// The value-space mirror of [`Self::type_decl`].
    fn value_decl(
        &self,
        canonical: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        name: &str,
    ) -> AttemptOutcome<Option<Arc<LoweredValueDecl>>>;

    /// The module-augmentation contributor index for `target` — every
    /// augmenter file (`declare module`/`declare global`) currently known
    /// to contribute at this key.
    ///
    /// Backed by `verter_session::file_artifact_store::
    /// FileArtifactStore::get_augmenter_set` — a non-blocking `DashMap`
    /// peek that NEVER triggers `ensure_augmentation_index_populated`'s
    /// cold scan/publish. `Complete(ModuleAugmentationIndexObservation {
    /// contributors: [], .. })` is the stable "genuinely zero augmenters"
    /// fact (cache-lifecycle contract) — never itself a reason to request more inputs;
    /// `NeedInputs` covers "this target's index has not been scanned yet
    /// in this content generation." Population and writes stay
    /// session-only; only this narrow read view crosses).
    fn module_augmentation_index(
        &self,
        target: &AugmentationTargetKey,
    ) -> AttemptOutcome<ModuleAugmentationIndexObservation>;

    /// The memoized [`FunctionBodySkeleton`] of one content-pinned function
    /// — the arena-free, span-relative-to-function-start flow substrate a
    /// demand-sliced flow evaluation is built from.
    ///
    /// Backed by a non-blocking peek at
    /// `verter_session::cache_runtime::flow_slice_node::
    /// FunctionFlowGraphStore`'s existing once-per-content-version memo —
    /// NEVER by driving `RetainedSnapshotSkeletonSource`'s cold build
    /// (which reaches `ensure_indexed_ready_serve` and
    /// `DeclLoweringService::acquire_lease`'s worker-thread rendezvous, the
    /// exact blocking shape `ResolverObservation` must never take).
    /// `Complete(None)` covers a demanded-but-genuinely-absent position
    /// (the pinned content version does not serve this function, or a
    /// live entry's hash no longer matches — a typed miss, never a
    /// skeleton of a different content version"); `NeedInputs` covers
    /// "not yet built for this content version" — the graph store has not
    /// consulted the skeleton producer for this exact key yet.
    fn function_body_skeleton(
        &self,
        key: &FlowFunctionObservationKey,
    ) -> AttemptOutcome<Option<Arc<FunctionBodySkeleton>>>;

    /// Path-existence/kind classification for `path` — the
    /// `WorkspaceRead::probe_path` shape. `Complete(probe)` covers every stable classification,
    /// including `PathProbe::Unknown`/`Inaccessible` (a backend I/O-error
    /// classification is itself a stable, cacheable fact distinct from
    /// "not yet queried" — see `WorkspaceRead::probe_path`'s own doc
    /// comment on why `file_exists`'s boolean-folding is forbidden
    /// resolver-side); `NeedInputs` covers "not yet in this attempt's
    /// observation view." `PathProbe` is semantic-owned resolver
    /// vocabulary, so no workspace dependency crosses the observation
    /// boundary.
    fn path_probe(&self, path: &str) -> AttemptOutcome<crate::resolver_core::PathProbe>;

    /// Symlink-resolved real path for `path`. `Complete(None)` is the stable
    /// "no symlink to resolve" fact; `NeedInputs` covers "not yet
    /// observed." A caller requests this only after `path_probe` on the
    /// same path is already known positive (`File`/`Directory`) — never
    /// speculatively ahead of that — though this bare peek method does not
    /// method itself does not enforce that ordering.
    fn real_path(&self, path: &str) -> AttemptOutcome<Option<CanonicalId>>;

    /// The narrow [`ResolutionPackageManifest`] projection for the
    /// `package.json` at `directory`. Takes a directory
    /// (matching [`crate::resolver_core::InputKey::PackageManifest`]'s
    /// existing `directory` field), not a full `package.json` file path.
    /// The workspace driver joins `"package.json"` at the read point,
    /// keeping the two key identities distinct. `Complete(None)` is the stable "no manifest
    /// at this directory" fact; `NeedInputs` covers "not yet observed."
    fn package_manifest(
        &self,
        directory: &str,
    ) -> AttemptOutcome<Option<Arc<ResolutionPackageManifest>>>;
}

#[cfg(test)]
#[path = "observation_tests.rs"]
mod observation_tests;

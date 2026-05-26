//! Published workspace state: atomic snapshot + consumer extension.
//!
//! [`PublishedRoot`] is a concrete struct wrapping an immutable
//! [`WorkspaceSnapshot`] with an opaque consumer extension (e.g., LSP views).
//! It is published atomically via `ArcSwapOption::store()`.
//!
//! # Boot-time semantics
//!
//! `Engine::new()` eagerly publishes an empty bootstrap snapshot, so
//! `ArcSwapOption` is `Some` immediately after construction. The
//! bootstrap snapshot has `ownership_ready: false` — ownership queries
//! are not yet authoritative. After `background_init` builds the full
//! project graph, a real snapshot with `ownership_ready: true` is
//! published.
//!
//! # Env-hash tables
//!
//! [`PublishedRoot`] carries two project-keyed env-hash tables computed
//! once at snapshot-build time in `engine.rs`'s rebuild path:
//!
//! - [`env_hashes_by_project`](PublishedRoot::env_hashes_by_project) —
//!   per-project `[parse, resolve, type, lib]` env-hash arrays. Looked
//!   up at query time as `O(1)` map access. No re-composition per query.
//! - [`project_identity_hashes`](PublishedRoot::project_identity_hashes) —
//!   per-project identity hashes. Caller wraps as
//!   `verter_session::ProjectIdentity` when consumed at the session boundary.
//!
//! Both tables swap atomically with the rest of the snapshot via
//! `ArcSwapOption<PublishedRoot>`: a reader loading a single `PublishedRoot`
//! sees the tables and the underlying [`WorkspaceSnapshot`] from the same
//! generation.

use std::any::Any;
use std::sync::Arc;

use rustc_hash::FxHashMap;

use verter_scheduler::invalidation::Hash16;

use crate::workspace_snapshot::{ProjectId, WorkspaceSnapshot};

/// Per-project four-array env-hash layout `[parse, resolve, type_, lib]`.
///
/// Index 0 is `parse_env_hash`, index 1 is `resolve_env_hash`, index 2 is
/// `type_env_hash`, index 3 is `lib_env_hash`. Session-side consumers
/// unpack into [`verter_session::session_view::EnvHashes`] by reading
/// these indices in order.
pub type ProjectEnvHashArray = [Hash16; 4];

/// The published workspace root: snapshot + consumer extension + env-hash tables.
///
/// Published atomically via a single `ArcSwap::store()`. Consumers
/// load via `load()` / `load_full()` and see exactly one generation
/// per request.
///
/// # Consumer extension
///
/// `consumer_ext` is opaque — the VFS layer doesn't know what it
/// contains. The LSP downcasts it to `LspViews` (defined in verter_lsp).
/// Non-LSP consumers set `consumer_ext: None`.
///
/// `Arc<WorkspaceSnapshot>` enables LSP-view-only rebuilds: reuse the
/// existing snapshot Arc, build new views, publish new `PublishedRoot`.
///
/// # Env-hash tables
///
/// The two env-hash tables are computed ONCE at snapshot-build time and
/// looked up at `O(1)` cost per query. They are keyed by [`ProjectId`]
/// (NOT canonical id) so workspaces with overlapping projects can keep
/// distinct cache identities for the same canonical when queried with an
/// explicit project context (see `WorkspaceSnapshot::owners_for_file`).
pub struct PublishedRoot {
    /// The immutable workspace snapshot.
    pub snapshot: Arc<WorkspaceSnapshot>,
    /// Opaque consumer extension. LSP downcasts to `LspViews`.
    /// `None` for non-LSP consumers (VFS-only publication).
    pub consumer_ext: Option<Box<dyn Any + Send + Sync>>,
    /// `false` during bootstrap (empty project graph, ownership queries unreliable).
    /// `true` after a real snapshot with the full project graph is published
    /// (e.g., after `background_init` completes).
    pub ownership_ready: bool,
    /// Per-project env-hash arrays `[parse, resolve, type_, lib]`. Empty on
    /// bootstrap snapshots before the project graph is published; populated
    /// in `engine.rs::rebuild_and_publish()` once the snapshot's projects
    /// are known. Lookup is `O(1)` map access; no re-composition per query.
    pub env_hashes_by_project: FxHashMap<ProjectId, ProjectEnvHashArray>,
    /// Per-project identity hashes. Empty on bootstrap snapshots; populated
    /// alongside [`env_hashes_by_project`].
    pub project_identity_hashes: FxHashMap<ProjectId, Hash16>,
}

impl PublishedRoot {
    /// Create a VFS-only published root (no consumer extension).
    ///
    /// `ownership_ready` defaults to `false` — this is a bootstrap snapshot
    /// where ownership queries are not yet authoritative. Env-hash tables
    /// default to empty.
    pub fn new_vfs_only(snapshot: Arc<WorkspaceSnapshot>) -> Self {
        Self {
            snapshot,
            consumer_ext: None,
            ownership_ready: false,
            env_hashes_by_project: FxHashMap::default(),
            project_identity_hashes: FxHashMap::default(),
        }
    }

    /// Create a published root with a consumer extension.
    ///
    /// `ownership_ready` defaults to `true` — the consumer extension (e.g.,
    /// `LspViews`) implies the project graph has been fully built. Env-hash
    /// tables default to empty; the engine rebuild path populates them
    /// before publishing.
    pub fn with_ext(snapshot: Arc<WorkspaceSnapshot>, ext: Box<dyn Any + Send + Sync>) -> Self {
        Self {
            snapshot,
            consumer_ext: Some(ext),
            ownership_ready: true,
            env_hashes_by_project: FxHashMap::default(),
            project_identity_hashes: FxHashMap::default(),
        }
    }

    /// Construct a `PublishedRoot` for an in-progress rebuild that carries
    /// pre-computed env-hash tables alongside the snapshot.
    ///
    /// The engine rebuild path uses this constructor so the snapshot and
    /// its env-hash tables ship atomically. `ownership_ready` follows the
    /// usual rule: VFS-only (no consumer extension) starts at `false`.
    pub fn with_env_hash_tables(
        snapshot: Arc<WorkspaceSnapshot>,
        env_hashes_by_project: FxHashMap<ProjectId, ProjectEnvHashArray>,
        project_identity_hashes: FxHashMap<ProjectId, Hash16>,
    ) -> Self {
        Self {
            snapshot,
            consumer_ext: None,
            ownership_ready: false,
            env_hashes_by_project,
            project_identity_hashes,
        }
    }

    /// Try to downcast the consumer extension to a concrete type.
    pub fn ext<T: 'static>(&self) -> Option<&T> {
        self.consumer_ext
            .as_ref()
            .and_then(|ext| ext.downcast_ref::<T>())
    }
}

impl std::fmt::Debug for PublishedRoot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PublishedRoot")
            .field("generation", &self.snapshot.generation)
            .field("has_ext", &self.consumer_ext.is_some())
            .field("ownership_ready", &self.ownership_ready)
            .field(
                "env_hashes_by_project_count",
                &self.env_hashes_by_project.len(),
            )
            .field(
                "project_identity_hashes_count",
                &self.project_identity_hashes.len(),
            )
            .finish()
    }
}

#[cfg(test)]
#[path = "published_state_tests.rs"]
mod tests;

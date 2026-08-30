use std::collections::BTreeSet;
use std::sync::Arc;

use verter_audit::files::{FileAudit, FileRole};
use verter_audit::origin_graph::VfsLayer;
use verter_audit::payloads::WorkspacePayload;
use verter_audit::{
    RequestAuditRecord, RequestKind, RequestKindPayload, RequestMemoryAudit, RequestStoreAudit,
    RequestTargetIdentity, RequestTimingAudit, WorkspaceOp,
};

use verter_scheduler::invalidation::Hash16;

use crate::ambient_lib::{AmbientLibError, AmbientLibSpec, AmbientLibsByProject};
use crate::exact_resolution::DependencySnapshotView;
use crate::published_state::ProjectEnvHashArray;
use crate::types::{ExactResolution, ExactResolutionResult, PackageManifest, ParsedEdge};
use crate::workspace_snapshot::ProjectId;
use verter_language::FileLanguage;
use verter_semantic::resolver_core::ProjectStableKey;
use verter_semantic::resolver_core::{
    AmbientSymbolHit, ProjectOwnership, ResolutionContext, ResolvePhase, ResolveRequestKind,
    ResolveResult,
};

/// Lightweight resource snapshot for first-class Rust audit.
#[derive(Debug, Clone, Default)]
pub struct WorkspaceResourceSnapshot {
    pub overlay_entries: usize,
    pub overlay_bytes: u64,
    pub snapshot_entries: usize,
    pub snapshot_bytes: u64,
    pub edge_file_count: usize,
    pub reverse_dep_bucket_count: usize,
    pub package_manifest_count: usize,
    pub published_project_count: usize,
}

/// Read-only view of the workspace authority.
///
/// `WorkspaceRead` carries every method that does NOT mutate workspace state
/// (file reads, resolution, ownership, generation, queries, ambient-lib
/// lookups). It is the public surface of the workspace exposed to external
/// crates via [`VerterHost::workspace_read`](`crate::WorkspaceRead`); the
/// mutator surface lives on [`WorkspaceAccess`] (which extends
/// `WorkspaceRead`) and is gated behind `pub(crate) workspace()`.
///
/// **Trait-upcasting requires Rust 1.86.** The workspace `Cargo.toml`
/// declares `rust-version = "1.86"`; this lets `Arc<dyn WorkspaceAccess>`
/// upcast to `Arc<dyn WorkspaceRead>` without a manual `as_read_arc`
/// conversion.
///
/// # Implementation note
///
/// Concrete workspaces (`FilesystemWorkspace`, `MemoryWorkspace`, etc.)
/// implement BOTH traits. The split is mechanical: read-method bodies live
/// on `impl WorkspaceRead for X`, mutator-method bodies on
/// `impl WorkspaceAccess for X`. The `WorkspaceAccess: WorkspaceRead`
/// supertrait bound ensures any `&dyn WorkspaceAccess` can be implicitly
/// used as a `&dyn WorkspaceRead`.
pub trait WorkspaceRead: Send + Sync {
    // ── File reads ──

    /// Read file content. Returns overlay content if set, otherwise
    /// snapshot/disk content. Returns `None` if the file doesn't exist.
    fn read_file(&self, canonical_id: &str) -> Option<Arc<str>>;

    /// Trace-only detail about the most recent `read_file()` on the current
    /// thread for this canonical path, if the workspace can provide it.
    ///
    /// This is intended for high-level trace events that want to preserve the
    /// concrete VFS layer/cache result without re-reading the file.
    fn take_last_read_file_trace_detail(&self, _canonical_id: &str) -> Option<String> {
        None
    }

    /// Check whether a file exists. In Filesystem mode, probes disk on miss.
    fn file_exists(&self, canonical_id: &str) -> bool;

    /// Classify a path without collapsing I/O uncertainty into absence.
    ///
    /// The ONLY path-classification seam resolution may use: no
    /// resolver-side code path may reach [`Self::file_exists`], because
    /// that boolean folds
    /// [`PathProbe::Inaccessible`](verter_semantic::resolver_core::PathProbe::Inaccessible)
    /// and [`PathProbe::Unknown`](verter_semantic::resolver_core::PathProbe::Unknown)
    /// into `false` — laundering an I/O error into a witnessable
    /// `Absent`, the one outcome a resolution witness may never cache.
    ///
    /// Every backend with an error channel answers for itself:
    /// `FilesystemWorkspace` classifies through `NativeFs`, and the
    /// recorder / frozen / overlay / transaction readers each carry the
    /// typed outcome through unchanged. The provided body exists ONLY
    /// for error-channel-free adapters (in-memory and fixture readers),
    /// where "not present" is total information and no laundering is
    /// possible.
    fn probe_path(&self, canonical_id: &str) -> verter_semantic::resolver_core::PathProbe {
        if self.file_exists(canonical_id) {
            verter_semantic::resolver_core::PathProbe::File
        } else if self.is_dir(canonical_id) {
            verter_semantic::resolver_core::PathProbe::Directory
        } else {
            verter_semantic::resolver_core::PathProbe::Absent
        }
    }

    /// Report that a resolver traversal terminated on a stack-safety /
    /// compute budget rather than on proven absence.
    ///
    /// A bounded walk that runs out of budget abandons branches it never
    /// looked at, so the `None` it hands back is NOT a witness of absence and
    /// its fact signature never observed the inputs the walk skipped. Any
    /// reader that carries a resolution transaction must therefore refuse
    /// cache admission for the attempt: publishing the negative would cache a
    /// wrong answer under a signature no later edit to the unwalked inputs
    /// can invalidate.
    ///
    /// This is the exact seam [`Self::probe_path`] already uses to route the
    /// sibling case — an I/O outcome that could not prove absence — to typed
    /// non-admission. The provided body is a no-op for readers with no
    /// transaction to refuse on; those callers already produce no admission.
    fn note_resolution_budget_exhausted(&self) {}

    /// Record the exact rejected prospective budget action, then retain the
    /// existing non-admission behavior for implementations that only need the
    /// coarse hook.
    fn note_input_resolution_budget_exhausted(
        &self,
        _event: verter_semantic::resolver_core::InputResolutionBudgetExhaustion,
    ) {
        self.note_resolution_budget_exhausted();
    }

    /// Mark a bounded-load integrity failure as non-admissible. Readers with
    /// no transaction retain the no-op default.
    fn note_input_resolution_terminal_failure(&self) {}

    fn note_input_load_integrity_failure(&self) {
        self.note_input_resolution_terminal_failure();
    }

    /// Reserve the exact supported input delta without reading or parsing an
    /// unrestricted payload. The default is deliberately incapable: custom
    /// readers never fall back to the legacy unrestricted read methods.
    fn preflight_resolution_inputs_bounded(
        &self,
        keys: &[verter_semantic::resolver_core::InputKey],
        _basis: verter_semantic::resolver_core::ResolutionBasis,
    ) -> Result<
        crate::resolver::ResolutionInputReservationBatch,
        verter_semantic::resolver_core::AttemptFailure,
    > {
        if let Some(failure) = keys
            .iter()
            .find_map(crate::resolver::unsupported_input_failure)
        {
            return Err(failure);
        }
        Err(
            verter_semantic::resolver_core::AttemptFailure::InputLoadUnavailable {
                key: Box::new(keys.first().cloned().unwrap_or_else(|| {
                    verter_semantic::resolver_core::InputKey::PathProbe {
                        path: Arc::from("<empty-bounded-preflight>"),
                    }
                })),
            },
        )
    }

    /// Load exactly a prior bounded reservation. The default never delegates
    /// to unrestricted reads and therefore cannot turn an unsupported custom
    /// reader into a zero-byte loader.
    fn load_preflighted_resolution_inputs(
        &self,
        reservation: &crate::resolver::ResolutionInputReservationBatch,
    ) -> Result<
        crate::resolver::LoadedResolutionInputBatch,
        verter_semantic::resolver_core::AttemptFailure,
    > {
        Err(
            verter_semantic::resolver_core::AttemptFailure::InputLoadUnavailable {
                key: Box::new(reservation.keys().first().cloned().unwrap_or_else(|| {
                    verter_semantic::resolver_core::InputKey::PathProbe {
                        path: Arc::from("<empty-bounded-load>"),
                    }
                })),
            },
        )
    }

    /// Publish loader-local payload and manifest observations only after the
    /// enclosing resolution operation has passed its final integrity and
    /// conditional-commit fences. Readers without shared load caches keep the
    /// default no-op.
    fn commit_loaded_resolution_inputs(&self, _entries: &[crate::resolver::LoadedResolutionInput]) {
    }

    /// Whether every resolver-visible backend mutation is serialized through
    /// this workspace's resolution-world publisher.
    ///
    /// Backends that cannot make that guarantee still return correct results,
    /// but Engine transactions conservatively refuse cache admission.
    fn resolution_event_bridge_complete(&self) -> bool {
        false
    }

    /// Whether this reader's answers are SCOPED to one request rather than
    /// to the population its cache key names.
    ///
    /// A request-local reader may admit a result after its own final fence,
    /// but must neither reuse nor populate the shared resolution
    /// caches/edges: its answers are correct only inside the batch that
    /// composed it. The overlay snapshot reader is the case — its answers
    /// are overlay-effective while the enclosing cache key names the
    /// underlying population, so a published candidate would be served to
    /// requests that cannot see the overlay.
    ///
    /// This is NOT the admission question. A reader that observes the shared
    /// population but cannot guarantee event-bridge coverage answers
    /// [`Self::resolution_event_bridge_complete`] `false` instead: it may
    /// still READ a warm candidate (and should — that is the memo), it
    /// simply cannot admit one. Conflating the two disables the resolution
    /// memo wholesale for the backend that answers `true` here, which is a
    /// silent, suite-invisible cold-path regression rather than a
    /// correctness fence.
    fn resolution_snapshot_is_request_local(&self) -> bool {
        false
    }

    // The live evidence capability is deliberately NOT a hook on this trait.
    // A reader hook is forwarded by every delegating wrapper, and a wrapper
    // that forgets one silently inherits the default. The capability is a
    // required parameter on the Engine's resolution entry points instead,
    // stated once by the backend that owns the Engine — see
    // `crate::resolution_currency::ResolutionEvidenceSource`. Nothing composed
    // on top of a reader can strip it.

    /// Drain exact directory enumerations performed internally by the most
    /// recent resolver-facing read on the current thread.
    ///
    /// Some backends answer a typed path probe by enumerating its parent
    /// directory. [`crate::resolution_currency::TransactionReader`] consumes
    /// this evidence so the corresponding `DirectoryMembers` fact enters the
    /// transaction signature. Implementations that never enumerate
    /// directories internally keep the empty default.
    fn take_resolution_directory_observations(&self) -> Vec<String> {
        Vec::new()
    }

    /// Resolution population visible through this reader.
    ///
    /// Standalone readers observe the base population. Engine-backed editor
    /// workspaces override this with their independently fenced overlay
    /// session.
    fn resolution_population(&self) -> verter_semantic::resolver_core::ResolutionPopulation {
        verter_semantic::resolver_core::ResolutionPopulation::Base
    }

    /// Capture this reader's immutable resolution world for the population
    /// it observes, as the validity root a consumer view retains.
    ///
    /// O(1) for an Engine-backed workspace: the composition of the current
    /// published base root and, for a session population, that session's
    /// overlay root. A consumer captures it ONCE and validates every
    /// resolution fact against that capture, never against the live
    /// registry.
    ///
    /// `None` means this reader publishes no resolution world — an adapter
    /// with no Engine behind it, or a capture that never observed a settled
    /// world. A consumer holding `None` validates no resolution fact at all,
    /// so the absence is a fail-closed miss and never an optimistic accept.
    fn capture_resolution_world(
        &self,
    ) -> Option<std::sync::Arc<crate::resolution_currency::CapturedResolutionWorld>> {
        None
    }

    /// Resolve symlinks to real path.
    fn realpath(&self, canonical_id: &str) -> Option<String>;

    /// Read and parse a `package.json` manifest.
    ///
    /// Concrete workspaces can override this to add caching. The default
    /// implementation reads the file through `read_file()` and parses it
    /// directly.
    fn read_package_manifest(&self, canonical_id: &str) -> Option<PackageManifest> {
        let source = self.read_file(canonical_id)?;
        Some(crate::package_index::parse_package_json(&source))
    }

    /// Classify a file through the PURE static extension registry.
    ///
    /// Workspace-level classification is static-only: project-gated
    /// candidate rows resolve to their ungated fallback here. Host-gated
    /// classification (capability-resolved rows) is composed at the
    /// session level and reaches the scheduler through the
    /// session-implemented `SourceLoader` seam, never from this crate.
    fn classify_file(&self, canonical_id: &str) -> FileLanguage {
        verter_language::LanguageRegistry::global()
            .classify_static(canonical_id)
            .static_resolution()
    }

    // ── Resolution ──

    /// Resolve an import specifier with full context.
    ///
    /// The `ctx` determines which target a specifier resolves to. Different
    /// `(phase, kind)` combinations produce different package.json condition
    /// chains and legacy-field lookups. Host never does its own heuristic
    /// resolution when this returns `None`.
    ///
    /// Default: `None` (no resolution).
    fn resolve_import(
        &self,
        _importer_id: &str,
        _specifier: &str,
        _ctx: ResolutionContext,
    ) -> Option<ResolveResult> {
        None
    }

    /// Resolve with the Engine transaction's fact-signature admission product.
    ///
    /// Adapter backends that do not own an Engine are conservatively
    /// ReturnOnly; concrete Engine-backed workspaces override this method.
    fn resolve_import_outcome(
        &self,
        importer_id: &str,
        specifier: &str,
        ctx: ResolutionContext,
    ) -> crate::resolution_currency::ResolutionOutcome {
        crate::resolution_currency::ResolutionOutcome::adapter_return_only(self.resolve_import(
            importer_id,
            specifier,
            ctx,
        ))
    }

    /// Resolve through this workspace's Engine with an immutable request-local
    /// overlay snapshot layered over the workspace.
    ///
    /// Concrete Engine-backed workspaces override this bridge. Adapter
    /// workspaces cannot mint an admitted publication and therefore retain the
    /// ordinary ReturnOnly behavior.
    fn resolve_import_outcome_with_overlay(
        &self,
        _overlay: &crate::resolution_currency::ResolutionOverlaySnapshot,
        importer_id: &str,
        specifier: &str,
        ctx: ResolutionContext,
    ) -> crate::resolution_currency::ResolutionOutcome {
        self.resolve_import_outcome(importer_id, specifier, ctx)
    }

    /// Resolve against one explicitly captured published root.
    ///
    /// Engine-backed workspaces admit only if this root is still the root
    /// captured by the transaction. Adapter backends remain ReturnOnly.
    fn resolve_import_at_published(
        &self,
        _published: &Arc<crate::published_state::PublishedRoot>,
        importer_id: &str,
        specifier: &str,
        ctx: ResolutionContext,
    ) -> crate::resolution_currency::ResolutionOutcome {
        crate::resolution_currency::ResolutionOutcome::adapter_return_only(self.resolve_import(
            importer_id,
            specifier,
            ctx,
        ))
    }

    /// Resolve an import specifier against an explicit owning project.
    ///
    /// This is used for project-scoped lookups that are not naturally rooted at
    /// a real source file, such as resolving `vue/jsx` for fallthrough
    /// intrinsics. Implementations should honor the same project-level tsconfig,
    /// alias, and package resolution rules as `resolve_import()`, without
    /// fabricating an importer path.
    fn resolve_import_for_project(
        &self,
        _owner: &ProjectOwnership,
        _specifier: &str,
        _ctx: ResolutionContext,
    ) -> Option<ResolveResult> {
        None
    }

    /// Explicit-project counterpart of [`Self::resolve_import_outcome`].
    fn resolve_import_for_project_outcome(
        &self,
        owner: &ProjectOwnership,
        specifier: &str,
        ctx: ResolutionContext,
    ) -> crate::resolution_currency::ResolutionOutcome {
        crate::resolution_currency::ResolutionOutcome::adapter_return_only(
            self.resolve_import_for_project(owner, specifier, ctx),
        )
    }

    /// Whether `canonical_id` is a workspace-owned source file.
    ///
    /// Routes through the resolver's existing ownership classification:
    /// - True when the file (or its `realpath` resolution) sits inside
    ///   any registered project's `root`. This includes:
    ///   - regular workspace packages,
    ///   - workspace packages that happen to live under `node_modules/`,
    ///   - pnpm-symlink hops where `realpath()` resolves a
    ///     `node_modules/.pnpm/...` path back to a workspace location.
    /// - False otherwise (third-party `node_modules` packages, paths
    ///   outside every registered project).
    ///
    /// Callers MUST NOT substitute `path.contains("/node_modules/")`
    /// for this method — that heuristic mis-classifies every
    /// pnpm-symlink and workspace-inside-node_modules case.
    ///
    /// Default: `false` (no project ownership).
    fn is_workspace_owned(&self, _canonical_id: &str) -> bool {
        false
    }

    /// Whether `canonical_id` is backed by a third-party package
    /// installation (i.e., reachable through `node_modules` and NOT
    /// claimed by any registered workspace project).
    ///
    /// Routes through the resolver's existing ownership classification:
    /// - True when the realpath sits under `node_modules/` AND no
    ///   registered project root claims the file.
    /// - False for workspace sources, pnpm-symlink hops that resolve
    ///   into a workspace project, and paths outside any
    ///   `node_modules` directory.
    ///
    /// Default: `false` (nothing is package-backed without a resolver).
    fn is_package_backed(&self, _canonical_id: &str) -> bool {
        false
    }

    /// Monotonic content generation. Bumped when workspace file content or
    /// overlays change, so long-lived consumers can invalidate cached reads.
    fn content_generation(&self) -> u64 {
        0
    }

    /// Monotonic authority for strict structural self-root validation.
    ///
    /// This is distinct from content and resolution generations because the
    /// strict artifact-only lane also consults live trackedness inputs such as
    /// `file_exists` and the session's derived-state presence. `None` means
    /// this workspace cannot vouch for a terminal strict-self-root witness.
    fn strict_self_root_generation(&self) -> Option<u64> {
        None
    }

    /// Stable process-unique identity of the authority that owns
    /// [`Self::strict_self_root_generation`]. A workspace replacement must
    /// not alias the prior workspace merely because both counters started at
    /// the same value.
    fn strict_self_root_authority_id(&self) -> Option<u64> {
        None
    }

    /// Whether any writer is currently changing an input to strict
    /// self-root validation. A generation alone cannot represent overlapping
    /// writers without an aliasing window, so witnesses fail closed while
    /// this is true.
    fn strict_self_root_transition_active(&self) -> bool {
        true
    }

    /// Monotonic count of resolution FACT VERSIONS minted by this
    /// workspace's resolution world.
    ///
    /// Advances by exactly one per
    /// [`crate::resolution_currency::ResolutionFactVersion`] mint — i.e.
    /// once per fact whose OBSERVED VALUE actually moved. Recording a
    /// first-observation baseline for a path the world had never seen
    /// mints no version and does not advance it, so a cold compute's own
    /// discovery does not churn this dimension.
    ///
    /// It exists for consumers that RETAIN a captured
    /// [`crate::resolution_currency::CapturedResolutionWorld`] and answer
    /// resolution-fact validity out of that capture. Validity itself stays
    /// fact-precise and is never decided by this counter (world identity
    /// is barred from being a cross-root warm-validity oracle). What the
    /// counter decides is whether a RETAINED capture is still the right
    /// snapshot to answer from: while it is unchanged, no fact version has
    /// moved, so the capture and the live world agree on every fact.
    /// Without it a cached view could keep serving pre-mutation fact
    /// versions after a resolution-visible change that moved no other
    /// dimension.
    ///
    /// Default `0` for adapters with no resolution world (they capture no
    /// world and validate no resolution fact either).
    fn resolution_fact_generation(&self) -> u64 {
        0
    }

    /// The `content_generation` recorded at `canonical_id`'s most recent
    /// per-canonical content transition (overlay write/clear, snapshot
    /// inject/remove, disk write/copy/delete); `0` when the canonical has
    /// never transitioned. The workspace is the sole content authority,
    /// so this ledger is the AUTHORITATIVE per-canonical freshness rail
    /// for consumers retaining content-derived artifacts: an artifact
    /// built at generation `G` is provably content-fresh only while
    /// `G >= last_content_transition_generation(canonical)`. Recorded at
    /// the workspace mutation chokepoints, so mutators that bypass any
    /// host-level wrapper (a direct embedder `notify_upsert`,
    /// `write_file`, `copy_file`) are covered by construction. Default
    /// `0` (reader-only impls never transition content).
    fn last_content_transition_generation(&self, _canonical_id: &str) -> u64 {
        0
    }

    /// Record a content transition for `canonical_id` at the current generation
    /// WITHOUT a byte change — the explicit marker for consumers that detect a
    /// content-derived artifact went stale while the source bytes were not
    /// re-upserted through a mutating chokepoint (e.g. the carrier-sync
    /// admission gate's equal-key differing-artifact refusal: the conflict
    /// itself proves the artifact rail under-counted). Records at
    /// `current + 1` exactly like the engine's invalidation-time record, so a
    /// later read of `last_content_transition_generation` is strictly newer
    /// than the refused key. Default no-op (reader-only impls never transition
    /// content).
    fn record_content_transition(&self, _canonical_id: &str) {}

    /// Point-in-time VFS provenance counters for observability and benchmarks.
    fn vfs_provenance_snapshot(&self) -> crate::types::VfsProvenanceSnapshot {
        crate::types::VfsProvenanceSnapshot::default()
    }

    /// Point-in-time resource snapshot for native audit.
    fn resource_snapshot(&self) -> WorkspaceResourceSnapshot {
        WorkspaceResourceSnapshot::default()
    }

    /// Compute the preferred alias-based import specifier for a target file.
    ///
    /// Returns the shortest tsconfig-path or workspace-alias specifier that
    /// round-trips back to `target_id` via `resolve_import()`. Returns `None`
    /// if no alias matches or the importer is unowned.
    fn preferred_specifier(&self, _importer_id: &str, _target_id: &str) -> Option<String> {
        None
    }

    /// Query reverse deps (files that import this file). Returns the union
    /// of canonical-axis and stem-axis hits, with the queried target
    /// stripped longest-suffix-first against the workspace's configured
    /// extension list.
    fn reverse_deps_for(&self, canonical_id: &str) -> Vec<String>;

    /// Query forward deps (files this file imports). Union of all
    /// canonical-axis dep classes (parsed + exact + lazy + ambient +
    /// semantic_transitive). Stems are NOT included.
    fn forward_deps_for(&self, canonical_id: &str) -> Vec<String>;

    /// Enumerate every canonical the workspace currently knows about — the
    /// program-member set (open/upserted overlay buffers + injected/published
    /// snapshot content + configured root files). Used for ambient-module
    /// program-completeness: an ambient `declare module "<bare>"` declarer may
    /// be a program-root `.d.ts` that NOTHING imports, reachable only through
    /// program membership rather than the import graph. The default returns
    /// empty — a workspace with no membership notion contributes nothing (and
    /// external string-literal augmentation falls back to import-graph-reachable
    /// declarers only).
    fn known_canonicals(&self) -> Vec<String> {
        Vec::new()
    }

    /// R22 contract: transitive importers of `edited`. The reverse
    /// import graph serves reachability GC + LSP affected-files
    /// reporting + diagnostics; it is **never** wired to cache
    /// invalidation. This BFS walks the canonical reverse axis and
    /// returns the transitive closure of files that (directly or
    /// indirectly) import `edited`, sorted for stable order.
    ///
    /// Default implementation walks via [`Self::reverse_deps_for`]. The
    /// `edited` file itself is NOT included in the result; cycles
    /// terminate via the visited set.
    fn affected_canonicals(&self, edited: &str) -> Vec<String> {
        let mut out: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        let mut frontier: Vec<String> = self.reverse_deps_for(edited);
        while let Some(next) = frontier.pop() {
            if next == edited {
                continue;
            }
            if !out.insert(next.clone()) {
                continue;
            }
            for parent in self.reverse_deps_for(&next) {
                if parent != edited && !out.contains(&parent) {
                    frontier.push(parent);
                }
            }
        }
        out.into_iter().collect()
    }

    /// Inspection: snapshot of an owner's dependency state.
    fn dependency_snapshot(&self, canonical_id: &str) -> Option<DependencySnapshotView>;

    // ── Directory queries ──

    /// List entries in a directory.
    /// Default: `Err(UnsupportedOperation)`.
    fn read_dir(&self, _dir: &str) -> Result<Vec<crate::error::DirEntry>, crate::error::VfsError> {
        Err(crate::error::VfsError::UnsupportedOperation("read_dir"))
    }

    /// Recursively walk a directory tree, filtering directories and files.
    /// Returns canonical paths of matching files.
    /// Default: `Err(UnsupportedOperation)`.
    fn walk(
        &self,
        _root: &str,
        _filter_dir: &dyn Fn(&str) -> bool,
        _filter_file: &dyn Fn(&str) -> bool,
    ) -> Result<Vec<String>, crate::error::VfsError> {
        Err(crate::error::VfsError::UnsupportedOperation("walk"))
    }

    /// Check whether a path is a directory.
    fn is_dir(&self, _path: &str) -> bool {
        false
    }

    // ── Ambient TypeScript lib reads ──

    /// Read an ambient lib's source by `(stable_key, canonical_id)`.
    ///
    /// Returns `None` when the canonical_id is shadowed by a non-ambient user
    /// file (overlay or snapshot) — see A5 user-wins shadowing.
    ///
    /// Default: `None`.
    fn read_ambient_lib(
        &self,
        _stable_key: ProjectStableKey,
        _canonical_id: &str,
    ) -> Option<Arc<str>> {
        None
    }

    /// Compute the project-scoped ambient virtual canonical id for a
    /// `(stable_key, canonical_id)` pair (`ambient:/<tag>/<canonical>`).
    ///
    /// Default falls through to the helper in [`crate::ambient_lib`] so all
    /// backends produce the same id for the same inputs.
    fn ambient_virtual_canonical_id(
        &self,
        stable_key: ProjectStableKey,
        canonical_id: &str,
    ) -> Arc<str> {
        crate::ambient_lib::ambient_virtual_canonical_id(stable_key, canonical_id)
    }

    /// Resolve a `ProjectId` (snapshot index) to its stable key for ambient
    /// lookups. Returns `None` when the workspace is not yet published or the
    /// id is unknown. Default: `None`.
    fn project_stable_key(&self, _project_id: ProjectId) -> Option<ProjectStableKey> {
        None
    }

    /// O(1) symbol-name lookup against the registered ambient libs for a
    /// given consumer project. Used by the bare-name resolver as a fallback
    /// when a symbol is not present in scope or in the import graph.
    ///
    /// Default: `None`.
    fn lookup_ambient_symbol(
        &self,
        _consumer_project: ProjectStableKey,
        _symbol: &str,
    ) -> Option<AmbientSymbolHit> {
        None
    }

    /// Lock-free read of the engine's ambient lib registry, used by host-side
    /// fact validators that resolve an ambient virtual id's current
    /// `FileWholeHash` (e.g., the `WholeHash` arm of the
    /// `validates_fact_signature` walk on the `StoreView`).
    /// Backends that don't support ambient libs return an empty registry.
    fn ambient_libs_view(&self) -> Arc<AmbientLibsByProject> {
        Arc::new(AmbientLibsByProject::default())
    }

    /// Currently-published workspace root, if any.
    ///
    /// Returns `Some` once `Engine::new()`'s bootstrap publication runs;
    /// returns `None` for adapter backends that do not maintain a
    /// published snapshot (e.g., test stubs). Session-side consumers
    /// read `published_root()` to map canonical ids to owning projects
    /// via `snapshot.owners_for_file(canonical).first()`.
    ///
    /// Default: `None`.
    fn published_root(&self) -> Option<Arc<crate::published_state::PublishedRoot>> {
        None
    }
}

/// Mutating view of the workspace authority — extends [`WorkspaceRead`]
/// with edge recording, exact-resolution writes, overlay notifications,
/// audit-sink registry, and ambient-lib mutators.
///
/// All workspace I/O (reads, writes, walks, resolution) goes through this
/// trait hierarchy. There is no separate partial resolver or config-file
/// reader capability. The resolver, config parser, and host all take
/// `&dyn WorkspaceAccess` (or the narrower `&dyn WorkspaceRead` for
/// read-only consumers).
///
/// # Implementors
///
/// - [`FilesystemWorkspace`] — disk-backed with overlay/snapshot cache
/// - [`MemoryWorkspace`] — fully in-memory (tests, WASM, playground)
/// - Lightweight adapters (LSP readers) that delegate to a host's workspace
///
/// # sub-
///
/// `WorkspaceAccess` is no longer the public read API for external crates;
/// it is gated behind `pub(crate) VerterHost::workspace()`. Read consumers
/// outside `verter_session` use `VerterHost::workspace_read()` →
/// `Arc<dyn WorkspaceRead>`. Mutators (`notify_close`, `notify_upsert`,
/// `set_exact_resolutions`, `configure_resolver`) are reachable only via
/// host wrappers that run the cache-cascade discipline.
pub trait WorkspaceAccess: WorkspaceRead {
    /// Open and close a strict-self-root authority write bracket. Backends
    /// that expose a generation override these together with the active-
    /// writer read above.
    fn begin_strict_self_root_transition(&self) {}

    fn end_strict_self_root_transition(&self) {}

    // ── Reverse-graph authority methods (R6: NO DEFAULTS) ──
    //
    // Every WorkspaceAccess impl MUST explicitly implement these. A future
    // impl that forgets to override would have silently dropped edges under
    // a default-no-op design; R6's compile-time enforcement makes that
    // impossible.

    /// Record parsed edges from a file's imports. Eagerly resolves
    /// `Relative` and `ExternalSrc` edges via the parsed-edge resolver
    /// (which bypasses `exact_resolutions` per R5). Stores `Bare` specifiers.
    /// Per R4 lifecycle: clears `exact_resolutions`, `exact_resolved`,
    /// `lazy_resolved`, and `semantic_transitive` for the file. **Does NOT
    /// clear `ambient_resolved`.**
    fn record_parsed_edges(&self, canonical_id: &str, edges: &[ParsedEdge]);

    /// Replace bundler-injected exact resolutions for a file. The active
    /// stem set is recomputed AFTER the exact mutation; parsed-unresolved
    /// entries are NOT destroyed (active-stem model).
    fn set_exact_resolutions(
        &self,
        canonical_id: &str,
        resolutions: Vec<ExactResolution>,
    ) -> ExactResolutionResult;

    /// Record parsed edges AND re-apply bundler exact resolutions as ONE
    /// atomic edge-store mutation. Semantics are
    /// [`record_parsed_edges`](Self::record_parsed_edges) followed by
    /// [`set_exact_resolutions`](Self::set_exact_resolutions), but no
    /// intermediate state (parsed edges recorded, exacts still cleared)
    /// is ever observable to a concurrent resolver — a two-call sequence
    /// exposes an exacts-empty window in which a cold flight resolves
    /// against the half-applied table and publishes a wrong-but-current
    /// route surface with no generation moved. Required (no default) so
    /// every impl makes an explicit atomicity decision — the same
    /// compile-time-enforcement rationale as the other edge mutators.
    fn record_parsed_edges_with_exact_resolutions(
        &self,
        canonical_id: &str,
        edges: &[ParsedEdge],
        resolutions: Vec<ExactResolution>,
    ) -> ExactResolutionResult;

    /// Replace owner's transitive-semantic dep set. Always fires regardless
    /// of `cc.dependencies` union equality.
    fn replace_semantic_transitive(&self, canonical_id: &str, deps: BTreeSet<String>);

    /// Set the workspace's reverse-dep-stripping extension list. Merges
    /// with `probe_extensions()` and sorts longest-first at set-time.
    fn set_default_resolve_extensions(&self, host_extensions: Vec<String>);

    /// Monotonic generation of the SOURCE-ENV compaction domain — the
    /// counter behind every `FileSourceEnv` observation
    /// (`parse_env_hash` / `parse_key` / `file_language_id`).
    ///
    /// Deliberately NOT folded into
    /// [`WorkspaceRead::content_generation`]: the production paths that
    /// move those dimensions — `publish_snapshot`, `rebuild_and_publish`
    /// (both reached through `configure_projects` / `configure_resolver`)
    /// and `WorkspaceChange::ConfigChanged` — do not bump the content
    /// generation, so a source-env fact standing behind a content stamp
    /// would survive a parse-env or file-language change.
    ///
    /// `None` means this workspace tracks NO source-env generation, and
    /// is the honest answer for a workspace with no producer: it disarms
    /// the domain rather than handing out a constant a consumer could
    /// mistake for a live one. A stamp that never advances is a witness
    /// nothing can invalidate, so "no producer" must be representable
    /// and must not be spelled `0`.
    ///
    /// It lives on `WorkspaceAccess` rather than [`WorkspaceRead`]
    /// because only the host-level session seam reads it; the
    /// resolution-time readers (transaction, overlay snapshot, frozen
    /// snapshot) have no source-env concern, and putting it on the read
    /// trait would force the frozen reader to choose between reporting a
    /// captured value and a live one for a dimension it does not
    /// revalidate.
    fn source_env_generation(&self) -> Option<u64> {
        None
    }

    /// Reset VFS provenance counters.
    fn reset_vfs_provenance(&self) {}

    /// Notify the workspace that a file was upserted into the host.
    ///
    /// Sets an overlay so the VFS resolver can find open/in-memory files that
    /// may not yet exist on disk. Called by the host during `upsert()`.
    /// Default: no-op (MemoryWorkspace manages its own snapshot).
    /// Publish one owner's `OwnerResolutionSet` node — the single
    /// bounded, owner-scoped resolution root — and return the fact ref a
    /// consumer observes.
    ///
    /// The node records the owner's CHILD DECISIONS, never their leaves,
    /// so an owner witness is one fact per resolved specifier rather than
    /// the union of everything those specifiers transitively reach.
    ///
    /// `None` when this workspace publishes no resolution world, or when
    /// the owner has no published decision for the node to stand for. A
    /// consumer holding `None` roots nothing owner-scoped, which is
    /// fail-closed.
    ///
    /// **One publisher.** The owner import surface is the single
    /// authority for this node; the session-side call site is a private
    /// function in the module that owns that surface, so no other module
    /// can reach it.
    fn publish_owner_resolution_set(
        &self,
        _owner_canonical: &str,
    ) -> Option<crate::fact_cache::FactVersionRef> {
        None
    }

    fn notify_upsert(&self, _canonical_id: &str, _source: Arc<str>) {}

    /// Notify the workspace that an editor buffer was closed.
    ///
    /// Clears the overlay AND invalidates the snapshot cache so the next
    /// read falls through to disk (picking up any saves made while the
    /// overlay was active). Default: no-op.
    fn notify_close(&self, _canonical_id: &str) {}

    /// Notify the workspace that a file was deleted.
    ///
    /// Clears overlay, removes snapshot, and removes edge-store data so
    /// the file is no longer resolvable or tracked. Default: no-op.
    fn notify_delete(&self, _canonical_id: &str) {}

    /// Configure semantic module resolution from a list of project configs.
    /// Called by the host when `configure_projects()` is used.
    /// Default: no-op. Concrete workspaces override to rebuild the semantic core.
    fn configure_resolver(&self, _projects: Vec<verter_semantic::resolver_core::IdeProjectConfig>) {
    }

    // ── Directory and mutation operations ──

    /// Write file content. Creates parent directories as needed.
    /// Default: `Err(UnsupportedOperation)`.
    fn write_file(&self, _path: &str, _content: &str) -> Result<(), crate::error::VfsError> {
        Err(crate::error::VfsError::UnsupportedOperation("write_file"))
    }

    /// Create a directory and all parent directories.
    /// Default: `Err(UnsupportedOperation)`.
    fn create_dir_all(&self, _path: &str) -> Result<(), crate::error::VfsError> {
        Err(crate::error::VfsError::UnsupportedOperation(
            "create_dir_all",
        ))
    }

    /// Delete a file.
    /// Default: `Err(UnsupportedOperation)`.
    fn delete_file(&self, _path: &str) -> Result<(), crate::error::VfsError> {
        Err(crate::error::VfsError::UnsupportedOperation("delete_file"))
    }

    /// Delete a directory and all its contents.
    /// Default: `Err(UnsupportedOperation)`.
    fn delete_dir_all(&self, _path: &str) -> Result<(), crate::error::VfsError> {
        Err(crate::error::VfsError::UnsupportedOperation(
            "delete_dir_all",
        ))
    }

    /// Copy a file from `src` to `dst`.
    /// Default: `Err(UnsupportedOperation)`.
    fn copy_file(&self, _src: &str, _dst: &str) -> Result<(), crate::error::VfsError> {
        Err(crate::error::VfsError::UnsupportedOperation("copy_file"))
    }

    // ── Package-backed types-entry resolution ──

    /// Locate the manifest `types` / `typings` entry for a package-backed
    /// runtime-script target.
    ///
    /// Returns the canonical path of the resolved types entry when
    /// `canonical_id` is package-backed AND its effective target is a
    /// runtime script (`.js`, `.cjs`, `.mjs`, `.jsx`). Returns `None` for
    /// workspace-owned files, for declaration files (`.d.ts`, `.d.cts`,
    /// `.d.mts`), for TypeScript sources (`.ts`, `.tsx`), and when the
    /// package manifest declares no `types` / `typings` entry.
    ///
    /// Concrete workspaces may override this to add caching. The default
    /// implementation walks up to the package root (the segment immediately
    /// after the last `node_modules/` boundary, expanded for scoped packages),
    /// reads `package.json`, and resolves the `types` or `typings` field
    /// against the package directory.
    fn manifest_types_entry_for(&self, canonical_id: &str) -> Option<String> {
        if !self.is_package_backed(canonical_id) {
            return None;
        }
        if !is_runtime_script_target(canonical_id) {
            return None;
        }
        let package_dir = package_dir_for_resolved_target(canonical_id)?;
        let package_json_path = format!("{package_dir}/package.json");
        let manifest = self.read_package_manifest(&package_json_path)?;
        let type_targets = [manifest.types.clone(), manifest.typings.clone()];
        type_targets.into_iter().flatten().find_map(|target| {
            let candidate = if let Some(rest) = target.strip_prefix("./") {
                format!("{package_dir}/{rest}")
            } else if target.starts_with('/') {
                target
            } else {
                format!("{package_dir}/{target}")
            };
            // Typed probe, never the boolean: this runs inside the
            // resolver's manifest lane, so an `Inaccessible` / `Unknown`
            // manifest entry must reach the transaction as itself rather
            // than as a witnessable absence.
            if self.probe_path(&candidate) != verter_semantic::resolver_core::PathProbe::File {
                return None;
            }
            Some(self.realpath(&candidate).unwrap_or(candidate))
        })
    }

    // ── Audit sink registry ──

    /// Register a VFS audit sink. The returned handle is deregister-able
    /// via [`deregister_audit_sink`]. Default: `NotSupported`.
    /// Concrete workspaces override to maintain a per-sink registry.
    fn register_audit_sink(
        &self,
        _sink: Arc<dyn crate::audit_sink::VfsAuditSink>,
    ) -> Result<crate::audit_sink::SinkHandle, crate::audit_sink::AuditSinkError> {
        Err(crate::audit_sink::AuditSinkError::NotSupported)
    }

    /// Deregister a previously-registered VFS audit sink. Default:
    /// `NotSupported`. Concrete workspaces override to complete the
    /// RAII-style registration lifecycle.
    fn deregister_audit_sink(
        &self,
        _handle: crate::audit_sink::SinkHandle,
    ) -> Result<(), crate::audit_sink::AuditSinkError> {
        Err(crate::audit_sink::AuditSinkError::NotSupported)
    }

    // ── Ambient TypeScript lib registry mutations ──

    /// Register an ambient TypeScript lib (e.g. `lib.es5.d.ts`) for a project.
    ///
    /// Idempotent on `(project, canonical_id, content_hash)`. New content for
    /// the same canonical_id replaces the existing entry and bumps the content
    /// generation so dep-fact validators re-execute.
    ///
    /// Default: `Err(NotBootstrapped)`.
    fn register_ambient_lib(&self, _spec: AmbientLibSpec) -> Result<(), AmbientLibError> {
        Err(AmbientLibError::NotBootstrapped)
    }

    /// Unregister an ambient lib by `(stable_key, canonical_id)`.
    /// Default: `Err(NotBootstrapped)`.
    fn unregister_ambient_lib(
        &self,
        _stable_key: ProjectStableKey,
        _canonical_id: &str,
    ) -> Result<(), AmbientLibError> {
        Err(AmbientLibError::NotBootstrapped)
    }

    /// Record a session-side reverse-dep edge from a consumer file to the
    /// ambient virtual id. Routes to the dedicated `ambient_resolved`
    /// dependency class (ambient deps survive parse re-records).
    /// Re-registration of the lib bumps the content generation so the
    /// fact-rail self-root validators reject downstream caches that
    /// pinned the prior content.
    /// **R6: no default; every workspace impl must override.**
    fn record_ambient_dependency(&self, consumer: &str, virtual_id: &str);

    // ── Project-scoped env-hash API ──
    //
    // Five-dimensional env-hash composition (R21) is keyed by `ProjectId`,
    // not by canonical id, so workspaces with overlapping projects can
    // hold distinct cache identities for a file claimed by multiple
    // projects. Session-side queries map canonical → ProjectId via
    // `WorkspaceSnapshot::owners_for_file(canonical).first()`.
    //
    // The tables live inside the published `PublishedRoot` snapshot (see
    // `crate::published_state::PublishedRoot::env_hashes_by_project` and
    // `project_identity_hashes`) so the snapshot and its env-hash tables
    // swap atomically on `ArcSwapOption<PublishedRoot>` republish. Lookup
    // is `O(1)` map access; the tables are computed ONCE at snapshot-build
    // time in `engine.rs::rebuild_and_publish()`.

    /// Env-hash array `[parse, resolve, type_, lib]` for a published
    /// project.
    ///
    /// Returns `None` when `project_id` is not present in the currently
    /// published snapshot (e.g., dropped on workspace bump, or no snapshot
    /// has been published yet). Callers fall back to
    /// [`Self::workspace_default_env_hash_array`] for canonicals with no
    /// owning project.
    ///
    /// Default body returns `None` — concrete workspaces override to read
    /// from their published snapshot.
    fn env_hash_array_for_project(&self, _project_id: ProjectId) -> Option<ProjectEnvHashArray> {
        None
    }

    /// Project-identity hash for a published project.
    ///
    /// Session callers wrap the returned `Hash16` as
    /// `verter_session::ProjectIdentity`. Returns `None` when the project
    /// is not present in the currently published snapshot.
    ///
    /// Default body returns `None` — concrete workspaces override.
    fn project_identity_hash_for_project(&self, _project_id: ProjectId) -> Option<Hash16> {
        None
    }

    /// Workspace-wide default env-hash array for canonicals with no
    /// owning project (e.g., cross-project sweeps over scratch / ambient
    /// canonicals).
    ///
    /// The default body returns all-zero `Hash16`s; concrete workspaces
    /// override to mix workspace-config + SDK fingerprint into a stable
    /// non-zero default. Session-side validators that observe an all-zero
    /// project identity treat it as "no owning project" rather than
    /// "default project".
    fn workspace_default_env_hash_array(&self) -> ProjectEnvHashArray {
        [[0u8; 16]; 4]
    }

    /// Workspace-wide default project-identity hash for canonicals with
    /// no owning project. See [`Self::workspace_default_env_hash_array`]
    /// for the rationale on the all-zero default.
    fn workspace_default_project_identity_hash(&self) -> Hash16 {
        [0u8; 16]
    }

    // ── Audit producer ──

    /// Drive a workspace [`WorkspaceOp`] under audit and produce a
    /// [`RequestAuditRecord`] describing the work.
    ///
    /// The default body executes the operation through this trait's
    /// own read methods (`resolve_import`, `forward_deps_for`,
    /// `resolve_import_for_project`) so every concrete backend
    /// inherits a real producer that walks live workspace state —
    /// it is NOT a stub.
    ///
    /// **Reachable-only invariant.** The traversal uses ONLY the
    /// `from`-importer's resolution surface (for `AuditResolve`),
    /// the BFS root's forward-dep edges (for `DepGraphTraverse`),
    /// or the project-scoped resolver (for `ResolverWalk`). Files
    /// outside the requested operation's reach do NOT appear in
    /// `record.files` — this enforces the macro-traversal
    /// MUST-NOT-walk-unrelated-imports invariant
    /// (see `CLAUDE.md` "Macro Type Traversal Rule").
    ///
    /// **TLS install.** The session-level callsite (`VerterHost`)
    /// wraps `audit_op` with an `AuditRequestRegistration::new`
    /// (`Active` / `Noop`) so the consumer-filter / records-store
    /// lifecycle is honored. The trait method itself is purely a
    /// producer: it does not enter the active-request registry.
    /// Per-request id is read from
    /// [`verter_scheduler::request_context::current_request_id`]
    /// so a registration installed by the host will already be
    /// visible when the trait method runs.
    fn audit_op(&self, op: WorkspaceOp) -> RequestAuditRecord {
        let request_id = verter_scheduler::request_context::current_request_id().unwrap_or(0);
        let (canonical_id, target_identity) = match &op {
            WorkspaceOp::AuditResolve { from, .. } => (
                from.clone(),
                RequestTargetIdentity::registered(from.clone()),
            ),
            WorkspaceOp::DepGraphTraverse { root } => (
                root.clone(),
                RequestTargetIdentity::registered(root.clone()),
            ),
            WorkspaceOp::ResolverWalk { .. } => {
                (String::new(), RequestTargetIdentity::NotApplicable)
            }
        };

        let start = std::time::Instant::now();
        let mut files: Vec<FileAudit> = Vec::new();
        let mut dep_edges_traversed: u64 = 0;

        match &op {
            WorkspaceOp::AuditResolve { specifier, from } => {
                let ctx = ResolutionContext {
                    phase: ResolvePhase::CodegenBlocker,
                    kind: ResolveRequestKind::EsmImport,
                };
                if let Some(result) = self
                    .resolve_import_outcome(from, specifier, ctx)
                    .into_transient_result()
                {
                    files.push(workspace_audit_file_entry(
                        &result.source_id,
                        FileRole::DirectImport,
                    ));
                }
            }
            WorkspaceOp::DepGraphTraverse { root } => {
                let mut visited: BTreeSet<String> = BTreeSet::new();
                let mut frontier: Vec<String> = vec![root.clone()];
                while let Some(current) = frontier.pop() {
                    if !visited.insert(current.clone()) {
                        continue;
                    }
                    let role = if current == *root {
                        FileRole::Entry
                    } else {
                        FileRole::TransitiveImport
                    };
                    files.push(workspace_audit_file_entry(&current, role));
                    let forward = self.forward_deps_for(&current);
                    dep_edges_traversed += forward.len() as u64;
                    for dep in forward {
                        if !visited.contains(&dep) {
                            frontier.push(dep);
                        }
                    }
                }
            }
            WorkspaceOp::ResolverWalk { specifier } => {
                // Project-scoped resolution: walk the workspace's
                // resolver surface for the specifier. The default
                // body uses the bare `resolve_import` surface with an
                // empty importer; backends that publish a project
                // graph hit `resolve_import_for_project` for each
                // owner via `is_workspace_owned`.
                let ctx = ResolutionContext {
                    phase: ResolvePhase::CodegenBlocker,
                    kind: ResolveRequestKind::EsmImport,
                };
                if let Some(result) = self
                    .resolve_import_outcome("", specifier, ctx)
                    .into_transient_result()
                {
                    files.push(workspace_audit_file_entry(
                        &result.source_id,
                        FileRole::ResolverWalk,
                    ));
                }
            }
        }

        let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
        let files_touched: u32 = files.len().min(u32::MAX as usize) as u32;
        let payload = WorkspacePayload {
            op: op.clone(),
            files_touched,
            ms: elapsed_ms,
            dep_edges_traversed,
        };

        RequestAuditRecord {
            request_id,
            canonical_id,
            target_identity: Some(target_identity),
            kind: RequestKind::Workspace { op },
            parent_request_id: None,
            from_cache: false,
            timings: RequestTimingAudit {
                total_ms: elapsed_ms,
                ..RequestTimingAudit::default()
            },
            memory: RequestMemoryAudit::default(),
            store: RequestStoreAudit::default(),
            footprint: None,
            scheduler: None,
            files,
            waits: None,
            kind_payload: RequestKindPayload::Workspace(payload),
            capture_state: verter_audit::AuditCaptureState::ActiveStored,
            trace_id: String::new(),
        }
    }
}

/// Construct a [`FileAudit`] entry recording a workspace-side touch
/// of `canonical_id` with the given role. Used by the default body
/// of [`WorkspaceAccess::audit_op`] to attribute every file the
/// workspace operation visited. Bytes/timing are zero because
/// `audit_op` does not load file content; the bytes/timing surfaces
/// belong to the read-loop producers (Slices 3.A/3.B/3.C).
fn workspace_audit_file_entry(canonical_id: &str, role: FileRole) -> FileAudit {
    FileAudit {
        canonical_id: canonical_id.to_string(),
        role,
        layer: VfsLayer::Snapshot,
        bytes_read: 0,
        cache_hit: true,
        triggered_by_this_request: false,
        read_ms: None,
        parse_ms: None,
        lower_ms: None,
    }
}

/// Whether `canonical_id`'s effective target is a runtime-script extension
/// (`.js`, `.cjs`, `.mjs`, `.jsx`).
///
/// Module-private helper for [`WorkspaceAccess::manifest_types_entry_for`].
fn is_runtime_script_target(canonical_id: &str) -> bool {
    canonical_id.ends_with(".js")
        || canonical_id.ends_with(".jsx")
        || canonical_id.ends_with(".mjs")
        || canonical_id.ends_with(".cjs")
}

/// Locate the package directory for a canonical_id that lives inside
/// `node_modules/`. Walks back from the last `/node_modules/` boundary
/// to capture the package segment, expanding scoped packages
/// (`@scope/name`) into two segments.
///
/// Returns `None` if `canonical_id` does not contain a `/node_modules/`
/// segment.
///
/// Module-private helper for [`WorkspaceAccess::manifest_types_entry_for`].
fn package_dir_for_resolved_target(canonical_id: &str) -> Option<String> {
    let normalized = canonical_id.replace('\\', "/");
    let marker = "/node_modules/";
    let marker_index = normalized.rfind(marker)?;
    let package_start = marker_index + marker.len();
    let package_path = &normalized[package_start..];
    let mut segments = package_path.split('/');
    let first = segments.next()?;
    let package_suffix = if first.starts_with('@') {
        format!("{first}/{}", segments.next()?)
    } else {
        first.to_string()
    };
    Some(format!("{}{package_suffix}", &normalized[..package_start]))
}

// ── Scheduler-oriented traits ──

/// Read-only file loading interface for the scheduler's I/O pool.
///
/// Implementations check overlay first, then fall back to disk (or memory).
/// All methods are sync — they run on the scheduler's bounded I/O pool,
/// isolated from the CPU pool.
///
/// Unlike [`WorkspaceAccess`], this trait has no resolution, edge recording,
/// or mutation methods. It is the minimal interface needed for the scheduler's
/// Source stage to load file content.
pub trait SourceLoader: Send + Sync {
    /// Load file content by canonical ID. Returns `None` if the file doesn't exist.
    fn load(&self, canonical_id: &str) -> Option<Arc<str>>;

    /// Check whether a file exists.
    fn exists(&self, canonical_id: &str) -> bool;

    /// Classify a file through the static extension registry.
    fn classify(&self, canonical_id: &str) -> FileLanguage;

    /// Resolve symlinks to real path.
    fn realpath(&self, canonical_id: &str) -> Option<String>;
}

/// Read-only snapshot of project resolution state.
///
/// Provides import resolution and project ownership queries without
/// mutation capabilities. The scheduler holds this via `ArcSwap` so it
/// can be atomically replaced when project configuration changes.
///
/// Implementor: `EmptyResolverSnapshot` (for standalone/test hosts).
pub trait ResolverSnapshot: Send + Sync {
    /// Resolve an import specifier in context.
    fn resolve_import(
        &self,
        importer: &str,
        specifier: &str,
        ctx: ResolutionContext,
    ) -> Option<ResolveResult>;

    /// Compute the preferred alias-based specifier for auto-imports.
    fn preferred_specifier(&self, importer: &str, target: &str) -> Option<String>;

    /// Find the owning project for a file.
    fn owner_for_file(&self, id: &str) -> Option<ProjectOwnership>;

    /// Monotonic generation counter. Bumped when project configuration changes.
    fn generation(&self) -> u64;
}

/// Empty resolver that resolves nothing. Used by standalone hosts and tests.
pub struct EmptyResolverSnapshot;

impl ResolverSnapshot for EmptyResolverSnapshot {
    fn resolve_import(
        &self,
        _importer: &str,
        _specifier: &str,
        _ctx: ResolutionContext,
    ) -> Option<ResolveResult> {
        None
    }

    fn preferred_specifier(&self, _importer: &str, _target: &str) -> Option<String> {
        None
    }

    fn owner_for_file(&self, _id: &str) -> Option<ProjectOwnership> {
        None
    }

    fn generation(&self) -> u64 {
        0
    }
}

#[cfg(test)]
mod ambient_default_tests {
    //! / A1 default-trait surface tests.
    //!
    //! These confirm that `WorkspaceAccess` has the ambient lib registration
    //! API and that backends without ambient support return
    //! `Err(NotBootstrapped)` / `None` from the defaults. These are
    //! discriminating: pre-change tree (no ambient methods on the trait) does
    //! not even compile.
    use std::collections::BTreeSet;
    use std::sync::Arc;

    use super::{DependencySnapshotView, WorkspaceAccess, WorkspaceRead};
    use crate::ambient_lib::{AmbientLibError, AmbientLibSpec};
    use crate::types::{ExactResolution, ExactResolutionResult, ParsedEdge};
    use verter_semantic::resolver_core::ProjectStableKey;

    /// Minimal backend that opts out of ambient lib support — exercises the
    /// trait defaults.
    struct StubWs;

    impl WorkspaceRead for StubWs {
        fn read_file(&self, _id: &str) -> Option<Arc<str>> {
            None
        }
        fn file_exists(&self, _id: &str) -> bool {
            false
        }
        fn realpath(&self, _id: &str) -> Option<String> {
            None
        }
        fn reverse_deps_for(&self, _id: &str) -> Vec<String> {
            Vec::new()
        }
        fn forward_deps_for(&self, _id: &str) -> Vec<String> {
            Vec::new()
        }
        fn dependency_snapshot(&self, _id: &str) -> Option<DependencySnapshotView> {
            None
        }
    }

    impl WorkspaceAccess for StubWs {
        // Reader-only stub overrides (R6/R7). Rationale (§2.16b):
        // `StubWs` lives inside a `#[cfg(test)]` ambient_default_tests module;
        // constructed only by trait-default coverage tests that don't invoke
        // VerterHost or any dep-flow path.
        fn record_parsed_edges(&self, _id: &str, _edges: &[ParsedEdge]) {}
        fn set_exact_resolutions(
            &self,
            _id: &str,
            _resolutions: Vec<ExactResolution>,
        ) -> ExactResolutionResult {
            ExactResolutionResult::default()
        }
        fn record_parsed_edges_with_exact_resolutions(
            &self,
            _id: &str,
            _edges: &[ParsedEdge],
            _resolutions: Vec<ExactResolution>,
        ) -> ExactResolutionResult {
            ExactResolutionResult::default()
        }
        fn replace_semantic_transitive(&self, _id: &str, _deps: BTreeSet<String>) {}
        fn set_default_resolve_extensions(&self, _host_extensions: Vec<String>) {}
        fn record_ambient_dependency(&self, _consumer: &str, _virtual_id: &str) {}
    }

    #[test]
    fn default_register_ambient_lib_returns_not_bootstrapped() {
        let ws = StubWs;
        let spec = AmbientLibSpec {
            project_id: None,
            canonical_id: Arc::from("lib.es5.d.ts"),
            source: Arc::from("export {};"),
        };
        let err = ws.register_ambient_lib(spec).unwrap_err();
        assert_eq!(
            err,
            AmbientLibError::NotBootstrapped,
            "default impl MUST surface NotBootstrapped"
        );
    }

    #[test]
    fn default_read_ambient_lib_returns_none() {
        let ws = StubWs;
        let key = ProjectStableKey::Configured([0u8; 16]);
        assert!(
            ws.read_ambient_lib(key, "lib.es5.d.ts").is_none(),
            "default impl MUST return None"
        );
    }

    #[test]
    fn default_lookup_ambient_symbol_returns_none() {
        let ws = StubWs;
        let key = ProjectStableKey::Configured([0u8; 16]);
        assert!(
            ws.lookup_ambient_symbol(key, "Pick").is_none(),
            "default impl MUST return None"
        );
    }

    #[test]
    fn default_ambient_libs_view_is_empty() {
        let ws = StubWs;
        let view = ws.ambient_libs_view();
        assert!(
            view.by_project.is_empty(),
            "default impl MUST return empty registry"
        );
    }

    /// The resolver's manifest lane classifies its `types` candidate
    /// through the TYPED probe, never the boolean.
    ///
    /// Discriminating by construction: `TypedProbeWs::file_exists`
    /// reports `true` for the `types` candidate while `probe_path`
    /// reports `Inaccessible`. Against the pre-change body
    /// (`if !self.file_exists(&candidate) { return None; }`) the entry
    /// is accepted and `manifest_types_entry_for` answers
    /// `Some(".../index.d.ts")` — the exact laundering
    /// `.DECISION.md` §2 forbids, because an I/O error would be
    /// witnessed as a stable positive resolution. Against the typed
    /// body it answers `None`, so the outcome stays with the
    /// transaction's non-admission rail instead.
    struct TypedProbeWs;

    const TYPED_PROBE_PACKAGE_MAIN: &str = "/w/node_modules/pkg/index.js";
    const TYPED_PROBE_TYPES_CANDIDATE: &str = "/w/node_modules/pkg/index.d.ts";

    impl WorkspaceRead for TypedProbeWs {
        fn read_file(&self, id: &str) -> Option<Arc<str>> {
            (id == "/w/node_modules/pkg/package.json")
                .then(|| Arc::from(r#"{"types":"./index.d.ts"}"#))
        }
        fn file_exists(&self, id: &str) -> bool {
            id == TYPED_PROBE_TYPES_CANDIDATE
        }
        fn probe_path(&self, id: &str) -> verter_semantic::resolver_core::PathProbe {
            if id == TYPED_PROBE_TYPES_CANDIDATE {
                // The one divergence: occupancy says "there", typed
                // classification says "the answer is unknowable".
                verter_semantic::resolver_core::PathProbe::Inaccessible
            } else {
                verter_semantic::resolver_core::PathProbe::Absent
            }
        }
        fn is_package_backed(&self, _id: &str) -> bool {
            true
        }
        fn realpath(&self, id: &str) -> Option<String> {
            Some(id.to_string())
        }
        fn reverse_deps_for(&self, _id: &str) -> Vec<String> {
            Vec::new()
        }
        fn forward_deps_for(&self, _id: &str) -> Vec<String> {
            Vec::new()
        }
        fn dependency_snapshot(&self, _id: &str) -> Option<DependencySnapshotView> {
            None
        }
    }

    impl WorkspaceAccess for TypedProbeWs {
        fn record_parsed_edges(&self, _id: &str, _edges: &[ParsedEdge]) {}
        fn set_exact_resolutions(
            &self,
            _id: &str,
            _resolutions: Vec<ExactResolution>,
        ) -> ExactResolutionResult {
            ExactResolutionResult::default()
        }
        fn record_parsed_edges_with_exact_resolutions(
            &self,
            _id: &str,
            _edges: &[ParsedEdge],
            _resolutions: Vec<ExactResolution>,
        ) -> ExactResolutionResult {
            ExactResolutionResult::default()
        }
        fn replace_semantic_transitive(&self, _id: &str, _deps: BTreeSet<String>) {}
        fn set_default_resolve_extensions(&self, _host_extensions: Vec<String>) {}
        fn record_ambient_dependency(&self, _consumer: &str, _virtual_id: &str) {}
    }

    #[test]
    fn manifest_types_entry_declines_an_inaccessible_candidate_the_boolean_calls_present() {
        let ws = TypedProbeWs;
        assert!(
            ws.file_exists(TYPED_PROBE_TYPES_CANDIDATE),
            "fixture precondition: the boolean rail must report the candidate present, \
             otherwise this test cannot discriminate the typed conversion",
        );
        assert_eq!(
            ws.manifest_types_entry_for(TYPED_PROBE_PACKAGE_MAIN),
            None,
            "an Inaccessible types candidate must not be laundered into a positive \
             manifest types entry",
        );
    }

    #[test]
    fn default_ambient_virtual_canonical_id_uses_helper() {
        let ws = StubWs;
        let key = ProjectStableKey::Configured([0xAB; 16]);
        let virt = ws.ambient_virtual_canonical_id(key, "lib.es5.d.ts");
        let s: &str = &virt;
        assert!(s.starts_with("ambient:/C"), "got {s}");
        assert!(s.ends_with("/lib.es5.d.ts"), "got {s}");
    }
}

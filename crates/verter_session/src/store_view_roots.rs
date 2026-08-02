//! The sealed root token a [`HostStoreView`](crate::resolver_store::HostStoreView)
//! is built from, and the lazy per-canonical resolution that reads through it.
//!
//! # Why roots and not snapshots
//!
//! A store view answers "what did this world look like when my request
//! started?". The obsolete way to answer that was to COPY the answer for
//! every owner the host knew about into per-canonical maps at build time —
//! `whole_hashes`, `file_facts`, `derived_hashes`, `source_envs`,
//! augmentation fingerprints. That made view construction O(files) on the
//! keystroke path and, worse, it was still not a snapshot: a copied
//! `(canonical, content_hash)` pair NAMES an artifact but does not KEEP it,
//! so the store could free the thing the view had captured.
//!
//! The replacement is a fixed-size token of immutable ROOTS. Each root both
//! names an epoch and leases it: while the view lives, the owning store may
//! not physically reclaim any version visible at that epoch. Capture is a
//! fixed number of scalar reads and `Arc` clones — independent of how many
//! files the host tracks. Every per-canonical answer is then derived on
//! demand by an exact POINT LOOKUP through the roots; nothing enumerates.
//!
//! # What is in the token, and what may never be
//!
//! Present: the scheduler source root, the artifact-membership root
//! (which also carries the augmentation index), the project-env root, the
//! captured resolution world, the session overlay root, and two live
//! candidate stores (`ResolvedImportFactsDb`, `RouteDb`) whose every
//! returned candidate is validated through the captured roots and the R26
//! fact-signature authority before it is believed.
//!
//! Absent by construction: any per-owner copy of a whole hash, file-facts
//! handle, derived hash, source-env identity or augmentation fingerprint;
//! any owner list; any fallback enumeration when a point lookup misses. A
//! miss is a miss.
//!
//! The two read-through handles (`artifact_reader`, `workspace`) are NOT
//! live-state oracles: they are the stores the roots ADDRESS. Every read
//! through them is root-relative (`*_at_root`), so it answers for the
//! view's epoch, not for now. The two exceptions — the artifact-only
//! authority gate — are documented at their use site and are strictly
//! conservative.
//!
//! # How "the build enumerates nothing" is held
//!
//! Two mechanisms, one structural and one dynamic, because either alone
//! degrades into a claim nobody checks.
//!
//! Structurally, the builder's reachable vocabulary contains no enumerable
//! handle: it does not take `&VerterHost`, and the one store-bearing thing
//! it does receive is a [`RootCapture`] whose fields are private to this
//! module and whose only operation is [`StoreViewRoots::seal`]. Neither
//! surviving root type exposes an iteration API. So a build that walked
//! the host's owners is not a rule violation to be detected — it is an
//! expression that does not compile.
//!
//! Dynamically, [`store_view_owner_visits`] counts every read through the
//! root read surface while a [`StoreViewBuildScope`] is active, and the
//! gate requires zero. That backstops the read surface the builder DOES
//! hold after sealing, and it is the leg that keeps working if the
//! structural shape is later loosened. Its own anti-vacuity control lives
//! with the gate that asserts on it.

use std::cell::Cell;
use std::sync::Arc;

use rustc_hash::FxHashMap;
use verter_scheduler::invalidation::Hash16;

use crate::file_artifact_store::{FileFacts, ProjectIdentity};
use crate::resolver_store::SourceEnvIdentity;

// ── Owner-visit instrumentation ──

thread_local! {
    /// Nesting depth of [`StoreViewBuildScope`] on this thread.
    static BUILD_SCOPE_DEPTH: Cell<u32> = const { Cell::new(0) };
    /// Owner accesses observed through captured roots on this thread
    /// while [`BUILD_SCOPE_DEPTH`] was non-zero.
    static OWNER_VISITS_IN_BUILD: Cell<u64> = const { Cell::new(0) };
}

/// Owner accesses this thread made through a view's captured roots while a
/// store-view BUILD scope was active, since the last
/// [`reset_store_view_owner_visits`].
///
/// The contract requires this to stay ZERO: capture is a fixed number of
/// scalar reads and `Arc` clones, so a build that touches an owner is a
/// re-introduced N-term. It is deliberately THREAD-LOCAL rather than
/// process-global — a build runs to completion on the calling thread, so a
/// thread-local reading is immune to whatever else the shared test process
/// is doing in parallel, in the way
/// [`store_view_coherent_build_sweeps`](crate::resolver_store::store_view_coherent_build_sweeps)
/// is not.
///
/// A zero here is only meaningful because two independent things hold:
/// the counter has a LIVE producer (proved by the anti-vacuity control in
/// `store_view_marginal_admit_tests`, which performs a real owner read
/// inside a scope and requires this to move), and the builder cannot reach
/// any enumerable handle at all (see [`RootCapture`]).
///
/// The READER is `cfg(test)` because the gate is its only consumer, and a
/// public accessor with no caller is API surface pretending to be
/// observability. The WRITER is not: [`note_owner_visit`] and the scope
/// entered by `HostStoreView::build` compile into every configuration, so
/// what the gate measures is the production build path itself and not a
/// test-only variant of it.
#[cfg(test)]
pub(crate) fn store_view_owner_visits() -> u64 {
    OWNER_VISITS_IN_BUILD.with(Cell::get)
}

/// Zero this thread's owner-visit counter. Every measurement is a delta
/// around one window, so the reader resets first and reads after.
#[cfg(test)]
pub(crate) fn reset_store_view_owner_visits() {
    OWNER_VISITS_IN_BUILD.with(|cell| cell.set(0));
}

/// RAII marker: while one of these is alive on this thread, root reads are
/// attributed to a store-view BUILD rather than to demand-time validation.
///
/// Demand-time reads through the roots are correct and expected — they are
/// the whole point of deferring the per-canonical answer. What must never
/// happen is a read during CAPTURE, because that is the cost that scales
/// with the host. The scope is what separates the two.
pub(crate) struct StoreViewBuildScope {
    _not_send: std::marker::PhantomData<*const ()>,
}

impl StoreViewBuildScope {
    pub(crate) fn enter() -> Self {
        BUILD_SCOPE_DEPTH.with(|depth| depth.set(depth.get() + 1));
        Self {
            _not_send: std::marker::PhantomData,
        }
    }
}

impl Drop for StoreViewBuildScope {
    fn drop(&mut self) {
        BUILD_SCOPE_DEPTH.with(|depth| depth.set(depth.get().saturating_sub(1)));
    }
}

/// Record one owner access through the captured roots.
///
/// Called at the two entry points that make up the ENTIRE root read
/// surface — [`StoreViewRoots::resolve_canonical`] and
/// [`StoreViewRoots::augmentation_fingerprint`]. Every other read in this
/// module is a private helper reachable only through one of them, so
/// instrumenting the pair covers the boundary exhaustively rather than
/// covering a hand-picked call site.
fn note_owner_visit() {
    if BUILD_SCOPE_DEPTH.with(Cell::get) > 0 {
        OWNER_VISITS_IN_BUILD.with(|cell| cell.set(cell.get().saturating_add(1)));
    }
}

/// A per-canonical override supplied by a layer ABOVE the captured roots.
///
/// `Inherit` leaves the root-relative answer alone. `Absent` states that
/// this layer knows the answer to be missing — a session overlay whose
/// artifact has not been materialised yet must NOT fall through to the
/// base artifact, whose facts describe the pre-overlay bytes. `Value`
/// replaces it outright.
#[derive(Debug, Clone)]
pub(crate) enum Override<T> {
    /// Leave the root-relative answer alone. Constructed only by the
    /// crate's test seams, which override ONE dimension in isolation; the
    /// production overlay always replaces every dimension at once.
    #[cfg(test)]
    Inherit,
    Absent,
    Value(T),
}

impl<T> Override<T> {
    /// Fold this override onto a root-relative answer.
    fn apply(&self, base: &mut Option<T>)
    where
        T: Clone,
    {
        match self {
            #[cfg(test)]
            Self::Inherit => {}
            Self::Absent => *base = None,
            Self::Value(value) => *base = Some(value.clone()),
        }
    }
}

/// One canonical's session-overlay override set.
#[derive(Debug, Clone)]
pub(crate) struct OverlayCanonical {
    pub(crate) whole_hash: Override<Hash16>,
    pub(crate) file_facts: Override<Arc<FileFacts>>,
    pub(crate) route_hash: Override<Hash16>,
    pub(crate) source_env: Override<SourceEnvIdentity>,
}

impl OverlayCanonical {
    /// An overlay-Upsert override: the session serves different bytes for
    /// this canonical, so every per-canonical answer is replaced.
    pub(crate) fn upsert(
        whole_hash: Hash16,
        file_facts: Option<Arc<FileFacts>>,
        route_hash: Option<Hash16>,
    ) -> Self {
        Self {
            whole_hash: Override::Value(whole_hash),
            file_facts: match file_facts {
                Some(facts) => Override::Value(facts),
                None => Override::Absent,
            },
            route_hash: match route_hash {
                Some(hash) => Override::Value(hash),
                None => Override::Absent,
            },
            // The base source-env identity no longer describes the
            // artifact this session serves for the canonical; drop it so
            // a recorded `FileSourceEnv` fact rejects strictly (miss +
            // recompute) instead of validating against the base identity.
            source_env: Override::Absent,
        }
    }

    fn apply(&self, base: &mut CanonicalView) {
        self.whole_hash.apply(&mut base.whole_hash);
        self.file_facts.apply(&mut base.file_facts);
        self.route_hash.apply(&mut base.route_hash);
        self.source_env.apply(&mut base.source_env);
        // The session is this canonical's content authority now, so a
        // base-layer withdrawal no longer describes the answer.
        base.whole_hash_withdrawn &= base.whole_hash.is_none();
    }
}

/// The session's override layer — O(overlay set), never O(owners).
///
/// Empty on a base view. Populated only by
/// [`HostStoreView::with_session_overlay`](crate::resolver_store::HostStoreView::with_session_overlay)
/// and by the crate's test seams, both of which enumerate the session's
/// own overlay/tombstone canonicals and nothing else.
#[derive(Debug, Clone, Default)]
pub(crate) struct SessionOverlayRoot {
    /// Canonicals the session has DELETED. Kept distinguishable from a
    /// genuinely-untracked canonical: the `FileWholeHash` /
    /// `DirectSource` arms reject a tombstoned canonical before the lazy
    /// untracked-accept rule.
    pub(crate) tombstones: std::collections::HashSet<String>,
    pub(crate) canonicals: FxHashMap<String, OverlayCanonical>,
}

impl SessionOverlayRoot {
    fn is_empty(&self) -> bool {
        self.tombstones.is_empty() && self.canonicals.is_empty()
    }
}

/// The project-env root: the R21 env-hash bundle, the project identity and
/// the project generation the view was captured under, plus the workspace's
/// immutable published snapshot so per-canonical project selection answers
/// for the captured world rather than the live one.
///
/// Capture is three scalar reads and one `ArcSwap` load.
#[derive(Clone)]
pub(crate) struct ProjectEnvRoot {
    pub(crate) env_hashes: crate::session_view::EnvHashes,
    pub(crate) project_identity: ProjectIdentity,
    pub(crate) project_generation: u64,
    published: Option<Arc<verter_workspace::published_state::PublishedRoot>>,
    /// Test-only live parse-env override, captured with everything else so
    /// a root-relative read reproduces `host_view_env_hashes_for` exactly.
    #[cfg(test)]
    parse_env_override: Option<Hash16>,
}

impl std::fmt::Debug for ProjectEnvRoot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProjectEnvRoot")
            .field("project_generation", &self.project_generation)
            .field("published", &self.published.is_some())
            .finish_non_exhaustive()
    }
}

impl Default for ProjectEnvRoot {
    fn default() -> Self {
        Self {
            env_hashes: crate::session_view::EnvHashes::default(),
            project_identity: ProjectIdentity([0u8; 16]),
            project_generation: 0,
            published: None,
            #[cfg(test)]
            parse_env_override: None,
        }
    }
}

impl ProjectEnvRoot {
    pub(crate) fn capture(
        env_hashes: crate::session_view::EnvHashes,
        project_identity: ProjectIdentity,
        project_generation: u64,
        published: Option<Arc<verter_workspace::published_state::PublishedRoot>>,
        #[cfg(test)] parse_env_override: Option<Hash16>,
    ) -> Self {
        Self {
            env_hashes,
            project_identity,
            project_generation,
            published,
            #[cfg(test)]
            parse_env_override,
        }
    }

    /// The `parse_env_hash` dimension for `canonical` AS OF this root.
    ///
    /// Mirrors `VerterHost::host_view_env_hashes_for` — owning project
    /// from the published snapshot, its per-project env array, else the
    /// captured workspace default — but reads the CAPTURED published root
    /// so the answer cannot drift under a live re-publication.
    pub(crate) fn parse_env_hash_for(&self, canonical: &str) -> Hash16 {
        let resolved = self.published.as_ref().and_then(|root| {
            let project = root.snapshot.owners_for_file(canonical).first().copied()?;
            let array = root.env_hashes_by_project.get(&project).copied()?;
            Some(array[0])
        });
        match resolved {
            Some(parse_env_hash) => self.apply_override(parse_env_hash),
            None => self.env_hashes.parse_env_hash,
        }
    }

    #[cfg(test)]
    fn apply_override(&self, parse_env_hash: Hash16) -> Hash16 {
        self.parse_env_override.unwrap_or(parse_env_hash)
    }

    #[cfg(not(test))]
    fn apply_override(&self, parse_env_hash: Hash16) -> Hash16 {
        parse_env_hash
    }
}

/// The sealed token a [`HostStoreView`](crate::resolver_store::HostStoreView)
/// is built from.
///
/// Every field is either an immutable leased root or a candidate store
/// whose results are validated through those roots. Nothing here is sized
/// by the number of files the host tracks, and nothing here is a per-owner
/// copy of an answer.
///
/// The `Option`s are `None` on exactly one value — the detached
/// [`HostStoreView::default`](crate::resolver_store::HostStoreView::default),
/// which snapshots no host and therefore leases nothing. Every view built
/// from a host carries all of them.
#[derive(Clone, Default)]
pub(crate) struct StoreViewRoots {
    /// Scheduler SOURCE membership as of capture. `lookup` is an as-of
    /// read sealed to the root's epoch; there is no path to the live
    /// directory through it.
    pub(crate) source_root: Option<Arc<verter_scheduler::source_root::SchedulerSourceRoot>>,
    /// Artifact membership as of capture — exact artifact keys, the
    /// canonical→keys index, AND the module-augmentation index.
    pub(crate) artifact_root: Option<Arc<crate::file_artifact_store::FileArtifactRoot>>,
    /// Env / identity / generation + the published project graph.
    pub(crate) project_env_root: Arc<ProjectEnvRoot>,
    /// The workspace's immutable published resolution world. The
    /// resolve-imports `Resolution` arm validates against THIS composition
    /// and never the Engine's live registry.
    pub(crate) resolution_root: Option<Arc<verter_workspace::CapturedResolutionWorld>>,
    /// The session's per-canonical override layer, `None` on a base view.
    pub(crate) session_root: Option<Arc<SessionOverlayRoot>>,
    /// Live candidate store. Every candidate it returns is admitted only
    /// after `ReadSetSignature` validation through this same view, so a
    /// live handle is sound here in a way it is not for a per-owner answer.
    pub(crate) resolved_import_facts:
        Option<Arc<crate::resolved_import_facts::ResolvedImportFactsDb>>,
    /// Live candidate store, same contract as `resolved_import_facts`.
    pub(crate) route_db: Option<Arc<crate::resolver_core::route_db::RouteDb>>,
    /// The store the artifact root ADDRESSES. Reads through it are
    /// root-relative (`artifacts_at_root` / `artifact_keys_at_root` /
    /// `augmenter_set_at_root`), so they answer for the captured epoch.
    pub(crate) artifact_reader: Option<Arc<crate::project_type_store::ProjectTypeStore>>,
    /// The workspace instance captured at build time. Used ONLY by the
    /// artifact-only authority gate (see
    /// [`StoreViewRoots::artifact_only_whole_hash_at`]).
    pub(crate) workspace: Option<Arc<dyn verter_workspace::WorkspaceAccess>>,
}

impl std::fmt::Debug for StoreViewRoots {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoreViewRoots")
            .field(
                "source_epoch",
                &self.source_root.as_ref().map(|root| root.epoch()),
            )
            .field(
                "artifact_epoch",
                &self.artifact_root.as_ref().map(|root| root.epoch()),
            )
            .field("project_env", &self.project_env_root)
            .field("has_resolution_root", &self.resolution_root.is_some())
            .field("has_session_root", &self.session_root.is_some())
            .finish_non_exhaustive()
    }
}

/// Every per-canonical answer a store view can give, resolved once through
/// the roots.
///
/// Computed by exact point lookup on first demand for a canonical and
/// memoized per view — O(distinct queried canonicals), never O(owners).
/// The memo is a convenience: dropping it changes cost, never answers.
#[derive(Debug, Clone, Default)]
pub(crate) struct CanonicalView {
    pub(crate) whole_hash: Option<Hash16>,
    pub(crate) source_env: Option<SourceEnvIdentity>,
    pub(crate) file_facts: Option<Arc<FileFacts>>,
    pub(crate) route_hash: Option<Hash16>,
    /// Program-analysis-domain authority: the view-current
    /// `IndexedReady` for the canonical's tracked content. The
    /// `FlowBody` validator reads the artifact's `FunctionProgramIndex`
    /// through this handle — a structural index read, never a re-lower.
    pub(crate) flow_body_indexed: Option<Arc<crate::project_type_store::IndexedReady>>,
    /// The view's world CONTAINED this canonical, but its content
    /// authority withdrew the answer — see [`WholeHashAuthority`].
    ///
    /// The distinction is load-bearing on exactly one rail: the
    /// `FileWholeHash` / `DirectSource` validator arms accept an ABSENT
    /// hash optimistically (a dependency loaded after the snapshot was
    /// taken is not stale, it is new). A withdrawal is not an absence —
    /// the canonical was known here, and its recorded hash can no longer
    /// be confirmed — so those arms must REJECT instead of accept.
    pub(crate) whole_hash_withdrawn: bool,
}

/// Why [`StoreViewRoots::base_whole_hash`] has no hash for a canonical.
enum WholeHashAuthority {
    /// The canonical resolves to this content hash as of the view.
    Present(Hash16),
    /// No authority in this view's world places the canonical at all:
    /// the scheduler never published it as of the captured epoch and no
    /// artifact for it was visible at the captured root either. This is
    /// the genuinely-untracked case the optimistic accept exists for.
    Untracked,
    /// The view's world DID contain an artifact for this canonical, but
    /// the artifact-only authority gate withdrew it — the scheduler took
    /// content authority for it, or the file is gone.
    ///
    /// The gate's two remaining legs (`derived_raw_cache` presence,
    /// `file_exists`) are LIVE reads of state the sealed root cannot
    /// capture per-canonical in O(1). Live is tolerable only because a
    /// withdrawal now reports itself: a canonical the view knew about
    /// stops validating instead of silently degrading into "untracked,
    /// therefore fine".
    Withdrawn,
}

/// Everything a store-view build needs from the host, read in ONE window
/// and sealed so the builder cannot read anything back out of it.
///
/// # Why this type exists at all
///
/// The builder's contract is that it enumerates NOTHING. A comment cannot
/// hold that, and neither can a source scanner looking for the names of
/// today's enumeration APIs. What holds it is the builder's reachable
/// vocabulary: [`HostStoreView::build`](crate::resolver_store::HostStoreView)
/// does not take `&VerterHost`, so the host, the scheduler, the artifact
/// store, the workspace and every candidate store are simply not names in
/// its scope. All it receives is the pre-build token capture, and the only
/// store-bearing member of that capture is one of these — whose fields are
/// PRIVATE to this module and whose sole operation is
/// [`StoreViewRoots::seal`], which consumes it into the sealed token.
///
/// The two roots that do travel onward expose no iteration API of their
/// own: [`FileArtifactRoot`](crate::file_artifact_store::FileArtifactRoot)
/// offers only `epoch()`, and
/// [`SchedulerSourceRoot`](verter_scheduler::source_root::SchedulerSourceRoot)
/// offers `epoch()` plus a single-canonical `lookup()`. So there is no
/// expression a builder could write that walks the host's owners, and
/// `store_view_owner_visits` is the dynamic backstop over the read surface
/// that remains.
#[derive(Clone)]
pub(crate) struct RootCapture {
    source_root: Arc<verter_scheduler::source_root::SchedulerSourceRoot>,
    artifact_root: Arc<crate::file_artifact_store::FileArtifactRoot>,
    resolution_world: Option<Arc<verter_workspace::CapturedResolutionWorld>>,
    published: Option<Arc<verter_workspace::published_state::PublishedRoot>>,
    artifact_reader: Arc<crate::project_type_store::ProjectTypeStore>,
    workspace: Arc<dyn verter_workspace::WorkspaceAccess>,
    resolved_import_facts: Arc<crate::resolved_import_facts::ResolvedImportFactsDb>,
    route_db: Arc<crate::resolver_core::route_db::RouteDb>,
    #[cfg(test)]
    parse_env_override: Option<Hash16>,
}

impl std::fmt::Debug for RootCapture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RootCapture")
            .field("source_epoch", &self.source_root.epoch())
            .field("artifact_epoch", &self.artifact_root.epoch())
            .finish_non_exhaustive()
    }
}

impl RootCapture {
    /// Capture every root and store handle the view will address.
    ///
    /// O(1) in the host's size: two lease captures (one publication-lock
    /// acquisition and one scalar read each), two `ArcSwap` loads and four
    /// `Arc` clones. Nothing here enumerates owners, artifacts,
    /// augmentation targets or scheduler nodes.
    pub(crate) fn capture(host: &crate::VerterHost) -> Self {
        Self {
            source_root: host.scheduler().capture_source_root(),
            artifact_root: host.project_type_store().indexed().capture_root(),
            resolution_world: host.ws().capture_resolution_world(),
            published: host.workspace().published_root(),
            artifact_reader: Arc::clone(host.project_type_store()),
            workspace: host.workspace(),
            resolved_import_facts: Arc::clone(
                host.project_type_store().resolved_import_facts_handle(),
            ),
            route_db: host.project_type_store().routes_handle(),
            #[cfg(test)]
            parse_env_override: *host.parse_env_override.lock(),
        }
    }

    /// Test-only: the workspace this capture addresses, for the mid-build
    /// mutation injection that must fire AFTER the token is sealed.
    ///
    /// Deliberately `cfg(test)`: production build code has no path from a
    /// capture to a workspace handle, which is what keeps enumeration
    /// unexpressible there.
    #[cfg(test)]
    pub(crate) fn workspace_for_test_injection(
        &self,
    ) -> &Arc<dyn verter_workspace::WorkspaceAccess> {
        &self.workspace
    }
}

impl StoreViewRoots {
    /// Seal a capture into the view's root token.
    ///
    /// This is the ONLY way a host-derived `StoreViewRoots` comes into
    /// existence (the other constructor is `Default`, which snapshots no
    /// host and leases nothing). It is a move of already-captured handles:
    /// it reads no store and touches no owner.
    pub(crate) fn seal(
        capture: &RootCapture,
        env_hashes: crate::session_view::EnvHashes,
        project_identity: ProjectIdentity,
        project_generation: u64,
    ) -> Self {
        Self {
            source_root: Some(Arc::clone(&capture.source_root)),
            artifact_root: Some(Arc::clone(&capture.artifact_root)),
            project_env_root: Arc::new(ProjectEnvRoot::capture(
                env_hashes,
                project_identity,
                project_generation,
                capture.published.clone(),
                #[cfg(test)]
                capture.parse_env_override,
            )),
            resolution_root: capture.resolution_world.clone(),
            session_root: None,
            resolved_import_facts: Some(Arc::clone(&capture.resolved_import_facts)),
            route_db: Some(Arc::clone(&capture.route_db)),
            artifact_reader: Some(Arc::clone(&capture.artifact_reader)),
            workspace: Some(Arc::clone(&capture.workspace)),
        }
    }

    /// The leased artifact-membership root. Every artifact, canonical→keys
    /// and augmentation read below goes through it, so all of them answer
    /// for the captured epoch rather than for now.
    pub(crate) fn artifact_root(
        &self,
    ) -> Option<&Arc<crate::file_artifact_store::FileArtifactRoot>> {
        self.artifact_root.as_ref()
    }

    /// The leased scheduler source root — the authority for a canonical's
    /// tracked whole hash.
    pub(crate) fn source_root(
        &self,
    ) -> Option<&Arc<verter_scheduler::source_root::SchedulerSourceRoot>> {
        self.source_root.as_ref()
    }

    /// Is this view's session layer holding a tombstone for `canonical`?
    pub(crate) fn is_tombstoned(&self, canonical: &str) -> bool {
        self.session_root
            .as_ref()
            .is_some_and(|session| session.tombstones.contains(canonical))
    }

    /// Attach a session override layer, replacing any previous one.
    pub(crate) fn with_session(&mut self, session: SessionOverlayRoot) {
        self.session_root = if session.is_empty() {
            None
        } else {
            Some(Arc::new(session))
        };
    }

    /// The complete per-canonical answer as of this token.
    ///
    /// Order: tombstone (nothing survives a session delete) → root-relative
    /// base answer → session override fold.
    pub(crate) fn resolve_canonical(
        &self,
        canonical: &str,
        content_generation: u64,
    ) -> CanonicalView {
        note_owner_visit();
        if self.is_tombstoned(canonical) {
            return CanonicalView::default();
        }
        let mut view = self.base_canonical_view(canonical, content_generation);
        if let Some(overlay) = self
            .session_root
            .as_ref()
            .and_then(|session| session.canonicals.get(canonical))
        {
            overlay.apply(&mut view);
        }
        view
    }

    /// The root-relative base answer: one source-root point lookup, then at
    /// most one canonical→keys point lookup and one artifact read per key
    /// that matches the canonical's tracked content. No enumeration of the
    /// store, no owner list, no fallback scan on a miss.
    fn base_canonical_view(&self, canonical: &str, content_generation: u64) -> CanonicalView {
        let whole_hash = match self.base_whole_hash(canonical, content_generation) {
            WholeHashAuthority::Present(whole_hash) => whole_hash,
            WholeHashAuthority::Untracked => return CanonicalView::default(),
            WholeHashAuthority::Withdrawn => {
                return CanonicalView {
                    whole_hash_withdrawn: true,
                    ..CanonicalView::default()
                }
            }
        };
        let mut view = CanonicalView {
            whole_hash: Some(whole_hash),
            ..CanonicalView::default()
        };
        let (Some(reader), Some(root)) = (self.artifact_reader.as_ref(), self.artifact_root())
        else {
            return view;
        };
        let store = reader.indexed();
        let parse_env_hash = self.project_env_root.parse_env_hash_for(canonical);
        for key in store.artifact_keys_at_root(root, canonical) {
            // Only the variant serving the canonical's tracked content can
            // answer for this view. Stale candidates from prior content
            // generations coexist in the multi-candidate store per R20 but
            // must never back a validator: a path-precise consumer observed
            // against the live content, so validation consults that
            // content's artifact or nothing at all.
            if key.content_hash != whole_hash {
                continue;
            }
            let Some(artifacts) = store.artifacts_at_root(root, &key) else {
                continue;
            };
            // Base keys only for the source-env identity: an overlay-scoped
            // key carries a session discriminator in its `parse_env_hash`
            // dimension and must never seed the base identity.
            if key.is_base() && view.source_env.is_none() {
                view.source_env = Some(SourceEnvIdentity {
                    parse_env_hash: crate::locator_identity::ParseEnvHash::from_env_hash(
                        parse_env_hash,
                    ),
                    parser_version: key.parser_version,
                    file_language_id: key.file_language_id.clone(),
                });
            }
            if view.file_facts.is_none() {
                view.file_facts = Some(Arc::clone(&artifacts.facts));
            }
            if view.flow_body_indexed.is_none() {
                view.flow_body_indexed = Some(Arc::clone(&artifacts.indexed));
            }
            if view.route_hash.is_none() {
                let indexed = &artifacts.indexed;
                // The route-surface authority is a current-content artifact
                // built under the canonical's live parse env — the same
                // gate the producer applies (`indexed_surface_is_current`).
                if indexed.parse_env_hash == parse_env_hash && indexed.whole_hash == whole_hash {
                    view.route_hash = indexed.route_surface_hash();
                }
            }
        }
        view
    }

    /// The canonical's tracked whole-content hash as of this token.
    ///
    /// Scheduler-owned canonicals answer from the source root. A canonical
    /// the scheduler never owned (a package-backed `.d.ts`, an ambient lib)
    /// answers from its artifact — the artifact-only lane.
    fn base_whole_hash(&self, canonical: &str, content_generation: u64) -> WholeHashAuthority {
        let Some(source_root) = self.source_root() else {
            return WholeHashAuthority::Untracked;
        };
        if source_root.is_exhausted() {
            // The epoch line is exhausted, so no root addresses a
            // consistent world any more. Withdraw rather than degrade
            // into the optimistic untracked accept.
            return WholeHashAuthority::Withdrawn;
        }
        match source_root.lookup(canonical) {
            verter_scheduler::source_root::SourceStateAt::Present { whole_hash, .. } => {
                return WholeHashAuthority::Present(whole_hash)
            }
            verter_scheduler::source_root::SourceStateAt::Absent { .. }
            | verter_scheduler::source_root::SourceStateAt::Unknown => {}
        }
        self.artifact_only_whole_hash_at(canonical, content_generation)
    }

    /// The artifact-only lane's whole hash, gated by the single authority
    /// predicate.
    ///
    /// Two of the three gate legs are root-relative by construction (the
    /// source root said the scheduler does not own this canonical; the
    /// artifact root supplies the artifact). The remaining two are live
    /// reads of the captured stores:
    ///
    /// * `derived_raw_cache` presence means the scheduler is the content
    ///   authority for the canonical, so it is not artifact-only.
    /// * `file_exists` covers a deleted / closed / never-present file,
    ///   whose artifact must never serve.
    ///
    /// Neither can be captured per-canonical at build time without
    /// re-introducing an O(owners) enumeration, so both stay live — but a
    /// live read inside a sealed view is only sound while its outcome
    /// cannot be MORE permissive than the captured world. Withdrawing a
    /// candidate is not automatically conservative: on the
    /// `FileWholeHash` / `DirectSource` rail an absent hash is accepted
    /// optimistically, so a live withdrawal would turn a canonical the
    /// view had tracked into one whose stale recorded hashes all
    /// validate. That is why a withdrawal over a canonical the captured
    /// ROOT still holds an artifact for reports
    /// [`WholeHashAuthority::Withdrawn`]: known-here-but-unconfirmable,
    /// which rejects. A canonical with no artifact at the captured root
    /// was never part of this view's world, so it stays
    /// [`WholeHashAuthority::Untracked`] and keeps the optimistic accept
    /// for dependencies loaded after the snapshot.
    ///
    /// The content-transition leg is CLAMPED to this view's captured
    /// `content_generation`: a transition recorded after the view was
    /// captured is not part of the view's world, so clamping reproduces the
    /// capture-time comparison instead of letting a later edit silently
    /// untrack a canonical the view had already resolved.
    fn artifact_only_whole_hash_at(
        &self,
        canonical: &str,
        content_generation: u64,
    ) -> WholeHashAuthority {
        let (Some(reader), Some(workspace), Some(root)) = (
            self.artifact_reader.as_ref(),
            self.workspace.as_ref(),
            self.artifact_root(),
        ) else {
            return WholeHashAuthority::Untracked;
        };
        let store = reader.indexed();
        if reader.derived_raw_cache().entries().contains_key(canonical)
            || !workspace.file_exists(canonical)
        {
            return self.withdrawn_or_untracked(canonical);
        }
        let transition = workspace
            .last_content_transition_generation(canonical)
            .min(content_generation);
        for key in store.artifact_keys_at_root(root, canonical) {
            let Some(artifacts) = store.artifacts_at_root(root, &key) else {
                continue;
            };
            if artifacts.indexed.built_at_content_generation >= transition {
                return WholeHashAuthority::Present(artifacts.indexed.whole_hash);
            }
        }
        WholeHashAuthority::Untracked
    }

    /// Classify a withdrawal: did the CAPTURED root hold an artifact for
    /// this canonical at all?
    ///
    /// One point lookup on the withdrawal path only. A canonical the root
    /// knew about is `Withdrawn` (its recorded facts must stop
    /// validating); one it never knew about is `Untracked`.
    fn withdrawn_or_untracked(&self, canonical: &str) -> WholeHashAuthority {
        let (Some(reader), Some(root)) = (self.artifact_reader.as_ref(), self.artifact_root())
        else {
            return WholeHashAuthority::Untracked;
        };
        if reader
            .indexed()
            .artifact_keys_at_root(root, canonical)
            .is_empty()
        {
            WholeHashAuthority::Untracked
        } else {
            WholeHashAuthority::Withdrawn
        }
    }

    /// Root-relative augmentation-index fingerprint for one target shape.
    ///
    /// Composes the full [`AugmentationTargetKey`](crate::file_artifact_store::AugmentationTargetKey)
    /// — including the project / resolve-env / lib-env dimensions the
    /// consumer's fact does not carry — from this view's project-env root,
    /// then performs ONE point lookup through the artifact root.
    pub(crate) fn augmentation_fingerprint(
        &self,
        target: crate::file_artifact_store::AugmentationTargetKind,
        population: crate::file_artifact_store::AugmentationPopulation,
    ) -> Option<Hash16> {
        note_owner_visit();
        let reader = self.artifact_reader.as_ref()?;
        let root = self.artifact_root()?;
        let key = crate::file_artifact_store::AugmentationTargetKey {
            project_identity: self.project_env_root.project_identity,
            resolve_env_hash: self.project_env_root.env_hashes.resolve_env_hash,
            lib_env_hash: self.project_env_root.env_hashes.lib_env_hash,
            population,
            target,
        };
        reader
            .indexed()
            .augmenter_set_at_root(root, &key)
            .map(|set| set.fingerprint)
    }
}

/// Per-view memo of resolved canonicals.
///
/// Correctness does not depend on it: every entry is a pure function of the
/// view's roots and the canonical. It exists so a fact signature naming the
/// same canonical N times pays the point lookups once, keeping validation
/// O(`ReadSetSignature`) rather than O(signature × root depth).
///
/// Lock rank: LEAF. The lock is never held across a store or workspace
/// read — compute happens outside it, and a lost race just recomputes a
/// value that is equal by construction.
#[derive(Debug, Default)]
pub(crate) struct StoreViewMemo {
    entries: parking_lot::RwLock<FxHashMap<String, Arc<CanonicalView>>>,
}

impl StoreViewMemo {
    pub(crate) fn get(&self, canonical: &str) -> Option<Arc<CanonicalView>> {
        self.entries.read().get(canonical).map(Arc::clone)
    }

    pub(crate) fn insert(&self, canonical: &str, value: &Arc<CanonicalView>) {
        self.entries
            .write()
            .entry(canonical.to_owned())
            .or_insert_with(|| Arc::clone(value));
    }

    /// Number of canonicals this view has actually resolved. The
    /// request-footprint bound the complexity contract allows.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.read().len()
    }
}

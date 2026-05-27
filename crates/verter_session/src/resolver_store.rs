use crate::types::Hash16;
use crate::VerterHost;
use dashmap::DashMap;
use std::panic::Location;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

/// Per-call-site counter for [`HostStoreView::from_host`] invocations.
///
/// **Per-call-site instrumentation.** `HostStoreView::from_host` rebuilds
/// the entire workspace snapshot on every call; the dominant cost
/// surfaces as `host_store_view_from_host_builds` per-Button counts in
/// the audit-record diagnostic counters. To attribute those builds
/// back to specific warm-hit validator call sites (the Bug 2 hypothesis
/// codex's 3-way consult identified), every entry into `from_host`
/// records `std::panic::Location::caller()` and bumps a per-site
/// counter. The `#[track_caller]` rail on `from_host`,
/// `VerterHost::resolver_store_view`, the
/// `impl ResolverContext::resolver_store_view` trait impls, and the
/// `fact_signature_helpers::validate_fact_signature*` helpers
/// propagates the location all the way back to the warm-hit cache
/// validator that triggered the build — so the dump attributes builds
/// to the actual cache layer paying for them, not to the deepest
/// `from_host` body call site.
///
/// **Cost is negligible:** each call performs one `DashMap` lookup
/// (sub-µs) vs the multi-ms workspace sweep `from_host` itself does,
/// so the counter stays production-on. The map is keyed by
/// `&'static Location<'static>` — `track_caller` locations are
/// `'static` by language guarantee, so pointer identity is stable and
/// the key set is bounded by the number of distinct call sites in the
/// linked binary.
///
/// Read via [`dump_from_host_call_sites`] (sorted descending by count).
static FROM_HOST_BY_SITE: OnceLock<DashMap<&'static Location<'static>, AtomicU64>> =
    OnceLock::new();

#[inline]
fn from_host_site_table() -> &'static DashMap<&'static Location<'static>, AtomicU64> {
    FROM_HOST_BY_SITE.get_or_init(DashMap::new)
}

/// Record one entry into [`HostStoreView::from_host`] under the
/// `#[track_caller]`-propagated call site. Bumped on every call;
/// thread-safe; no allocation when the site already has an entry
/// (the common case after the first call from each site).
#[inline]
fn record_from_host_call(loc: &'static Location<'static>) {
    let table = from_host_site_table();
    if let Some(entry) = table.get(loc) {
        entry.fetch_add(1, Ordering::Relaxed);
        return;
    }
    // First call from this site — insert a fresh counter at 1. Two
    // racing first-calls may both take the insert arm; the second's
    // entry overwrites the first's 1-count with another 1-count, which
    // is acceptable for diagnostic accounting (lost at most ~N
    // first-call counts where N = number of racing threads at startup).
    table.insert(loc, AtomicU64::new(1));
}

/// Reset the per-call-site counter table — only useful for tests / benches
/// that want a clean delta. Production callers never invoke this; the
/// table accumulates across the process lifetime.
pub fn reset_from_host_call_sites() {
    from_host_site_table().clear();
}

/// Snapshot the per-call-site counter table, sorted by count descending.
/// Each tuple is `(file_line, call_count)` where `file_line` is the
/// canonical `file:line:col` `Location` debug string.
///
/// **Diagnostic accessor.** The bench example dumps this at
/// the end of each pass to attribute `HostStoreView::from_host` builds
/// to specific warm-hit validator call sites. The `#[track_caller]`
/// rail on `from_host`, `VerterHost::resolver_store_view`, the trait
/// `resolver_store_view` impls, and the `validate_fact_signature*`
/// helpers reflects the location back to the cache layer triggering
/// the build.
#[must_use]
pub fn dump_from_host_call_sites() -> Vec<(String, u64)> {
    let table = from_host_site_table();
    let mut rows: Vec<(String, u64)> = table
        .iter()
        .map(|entry| {
            let loc = *entry.key();
            let count = entry.value().load(Ordering::Relaxed);
            let formatted = format!("{}:{}:{}", loc.file(), loc.line(), loc.column());
            (formatted, count)
        })
        .collect();
    rows.sort_by(|a, b| b.1.cmp(&a.1));
    rows
}

/// Per-request component-meta store counters captured by
/// [`VerterHost::component_meta_audit_store_snapshot`]. The fields
/// live on [`crate::component_meta_audit::ComponentMetaPayload`]
/// rather than the generic
/// [`crate::component_meta_audit::RequestStoreAudit`] envelope; this
/// struct is the cross-call carrier between the snapshot site and
/// the audit-builder finalisation.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ComponentMetaStoreCounters {
    pub materialize_structure_calls: u64,
    pub materialize_structure_cache_hits: u64,
    pub node_arena_lock_acquisitions: u64,
    pub family_map_lock_acquisitions: u64,
    pub dep_signature_merges: u64,
    pub dep_signature_intern_hits: u64,
}
use rustc_hash::FxHashMap;
use std::hash::{Hash, Hasher};

// WASM-only: scheduler is unavailable on web; see CLAUDE.md "Scheduler as Sole Compile Authority".

const STORE_VIEW_SNAPSHOT_RETRY_ATTEMPTS: usize = 3;

#[derive(Debug, Clone)]
pub struct HostStoreView {
    compat_token: crate::resolver_core::StoreViewCompatToken,
    mutation_epoch: u64,
    session_id: Option<u64>,
    whole_hashes: FxHashMap<String, Hash16>,
    derived_hashes: FxHashMap<(String, crate::resolver_core::DerivedFactKind), Hash16>,
    import_routes: FxHashMap<(String, String), crate::types::DependencyResolution>,
    /// Route-surface-domain snapshot — augmentation-index fingerprints
    /// keyed by a structural representation of the
    /// `(target_kind_tag, target_payload)` shape. Validation against
    /// `RouteSurfaceFactRef::ModuleAugmentationIndexShape` consults
    /// this map (R29 + G1 + R26).
    ///
    /// The key shape mirrors the one the
    /// `FactKey::ModuleAugmentationIndexShape` variant carries; see
    /// [`route_surface_index_key`] for the canonical mapping. An
    /// absent key means the augmentation-index entry has not yet
    /// been populated — the validator returns `false` so the
    /// downstream cache misses.
    route_surface_index_fingerprints: FxHashMap<RouteSurfaceIndexShapeKey, Hash16>,
    /// Parse-domain snapshot (R26): per-canonical `Arc<FileFacts>`
    /// captured at view-build time. The validator for
    /// `ParseFactRef` reads through this map; one `Arc::clone` per
    /// tracked file at build time, wait-free hash compares
    /// thereafter. Files not present in the snapshot are treated as
    /// untracked (validator returns `false` — a path-precise
    /// consumer expected its fact to be in the registry).
    file_facts: FxHashMap<String, std::sync::Arc<crate::file_artifact_store::FileFacts>>,
    /// Resolve-imports-domain handle (R26): `Arc` clone of the
    /// project store's `ResolvedImportFactsDb`. The validator for
    /// `ResolveImportsFactRef` composes
    /// `ResolvedImportFactsKey { canonical, content_hash,
    /// parse_env_hash, resolve_env_hash, resolver_version,
    /// known_miss_generation }` from the fact's `canonical_id`, this
    /// view's tracked `whole_hashes[canonical]`,
    /// `resolved_import_facts_known_miss_tags[canonical]`, and
    /// `env_hashes`, then looks up the matching
    /// `Arc<ResolvedImportFacts>` and compares the per-fact
    /// `semantic_hash` / `display_hash` of the stored
    /// `ResolvedImportClauseEntry.fact` /
    /// `ResolvedReexportBindingEntry.fact` (per `fact.lane`) against
    /// `expected_hash`.
    ///
    /// One `Arc` clone at view-build time; reads thereafter are
    /// wait-free against concurrent writers because `DashMap` shards
    /// per key.
    resolved_import_facts:
        Option<std::sync::Arc<crate::resolved_import_facts::ResolvedImportFactsDb>>,
    /// Per-canonical known-miss generation tag captured at view-build
    /// time. Folds the owner's
    /// `DerivedRawState::import_routes_known_miss_recorded_at_generation`
    /// map through
    /// [`crate::resolved_import_facts::compute_known_miss_generation_tag`]
    /// so the validator composes the same `known_miss_generation`
    /// key dimension the producer
    /// (`admit_resolved_import_facts_for_owner`) admitted under.
    /// Absent entries fall back to `[0u8; 16]` (owners with no
    /// recorded known-misses or canonicals whose route resolution
    /// never ran). Codex P2.2 fix.
    resolved_import_facts_known_miss_tags: FxHashMap<String, Hash16>,
    /// Route-surface-domain handle (R26): `Arc` clone of the
    /// project store's `RouteDb`. The validator for
    /// `RouteSurfaceFactRef` with `FactKey::EffectiveExportSet`
    /// composes
    /// `EffectiveExportSetKey { provider_canonical, project_identity,
    /// resolve_env_hash, lib_env_hash }` from the fact's
    /// `canonical_id` plus the view's `project_identity` and
    /// `env_hashes`, then compares the cached entry's
    /// `augmenter_set_fingerprint` to `expected_hash`.
    ///
    /// One `Arc` clone at view-build time; reads thereafter are
    /// wait-free against concurrent writers.
    route_db: Option<std::sync::Arc<crate::resolver_core::route_db::RouteDb>>,
    /// Env-hash bundle (R21) captured at view-build time.
    /// `env_hashes.parse_env_hash` + `env_hashes.resolve_env_hash`
    /// participate in `ResolvedImportFactsKey` composition;
    /// `env_hashes.resolve_env_hash` + `env_hashes.lib_env_hash`
    /// participate in `EffectiveExportSetKey` composition.
    env_hashes: crate::session_view::EnvHashes,
    /// Project identity captured at view-build time. Participates in
    /// `EffectiveExportSetKey` composition (R21).
    project_identity: crate::file_artifact_store::ProjectIdentity,
    /// Monotonic project generation captured at view-build time. The
    /// validator for `FactVersionRef::ProjectGeneration` compares a
    /// fact's observed generation against this snapshot: a cached
    /// value rooted on generation `g` validates iff `g` still equals
    /// the current generation. The generation advances on `tsconfig`,
    /// path-alias, SDK, workspace-folder, and project-graph changes
    /// (never on a pure file-content edit).
    project_generation: u64,
    /// Canonicals the active session has TOMBSTONED (overlay-Deleted).
    ///
    /// [`Self::with_session_overlay`] drops a tombstoned canonical's
    /// per-canonical snapshots from `whole_hashes` / `file_facts` /
    /// `derived_hashes` (see [`Self::drop_tombstoned_canonical_snapshots`])
    /// so the *strict* self-root validator rejects an entry self-rooted
    /// on the deleted file. But that removal also makes the canonical
    /// look UNTRACKED to the *lazy* [`StoreView::validates`]
    /// `FileWholeHash` arm — whose untracked branch optimistically
    /// returns `true` for a genuine cross-file dependency loaded after
    /// the view snapshot. A tombstoned canonical is NOT a
    /// genuinely-untracked dependency: it is a file the session
    /// deleted, so any content dependency on it is invalid.
    ///
    /// This set keeps a tombstoned canonical distinguishable from a
    /// genuinely-untracked one: the `FileWholeHash` / `DirectSource`
    /// validator arms reject a tombstoned canonical before the lazy
    /// untracked-accept rule. Empty on a base (non-session) view —
    /// only `with_session_overlay` populates it.
    tombstoned_canonicals: std::collections::HashSet<String>,
}

/// Structural key for snapshotting `ModuleAugmentationIndexShape`
/// fingerprints into [`HostStoreView`]. Mirrors the parallel
/// optional fields of `FactKey::ModuleAugmentationIndexShape`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct RouteSurfaceIndexShapeKey {
    pub target_kind_tag: verter_semantic::facts::registry::AugmentationTargetKindTag,
    pub external_specifier: Option<String>,
    pub resolved_relative_canonical: Option<String>,
    pub wildcard_pattern: Option<String>,
}

impl Default for HostStoreView {
    fn default() -> Self {
        Self {
            compat_token: crate::resolver_core::StoreViewCompatToken {
                epoch: 0,
                session: None,
            },
            mutation_epoch: 0,
            session_id: None,
            whole_hashes: FxHashMap::default(),
            derived_hashes: FxHashMap::default(),
            import_routes: FxHashMap::default(),
            route_surface_index_fingerprints: FxHashMap::default(),
            file_facts: FxHashMap::default(),
            resolved_import_facts: None,
            resolved_import_facts_known_miss_tags: FxHashMap::default(),
            route_db: None,
            env_hashes: crate::session_view::EnvHashes::default(),
            project_identity: crate::file_artifact_store::ProjectIdentity([0u8; 16]),
            project_generation: 0,
            tombstoned_canonicals: std::collections::HashSet::new(),
        }
    }
}

// Test-only thread-local counter incremented every time
// `HostStoreView::from_host` is called. The discriminating tests for
// The per-request hoist read this counter to assert that a
// single component-meta request builds the view exactly once instead
// of 8-12+ times. Thread-local so parallel `cargo test` execution
// does not cross-pollute counts. Production builds do not pay for
// the increment (gated under `#[cfg(test)]`).
#[cfg(test)]
thread_local! {
    pub(crate) static HOST_STORE_VIEW_FROM_HOST_BUILDS: std::cell::Cell<u64> = const {
        std::cell::Cell::new(0)
    };
}

impl HostStoreView {
    #[track_caller]
    pub(crate) fn from_host(host: &VerterHost) -> Self {
        // Per-call-site instrumentation: record the
        // `#[track_caller]`-propagated location so the bench can
        // attribute `from_host` builds back to specific warm-hit
        // validator call sites. The location flows back through the
        // `#[track_caller]` rail on `VerterHost::resolver_store_view`,
        // the trait `resolver_store_view` impls, and the
        // `validate_fact_signature*` helpers — so the recorded site
        // is the cache layer paying for the build, not the deepest
        // `from_host` body.
        record_from_host_call(Location::caller());
        #[cfg(test)]
        HOST_STORE_VIEW_FROM_HOST_BUILDS.with(|c| c.set(c.get().saturating_add(1)));
        // Block 7.5 diagnostic counter: bump the per-request
        // `host_store_view_from_host_builds` counter so the bench
        // surfaces how many owned-view rebuilds the request paid for.
        // The bump is a noop when no `RequestContext` is installed
        // (synthesised tests, non-audited callers).
        if let Some(ctx) = crate::request_context::current_request_context() {
            ctx.cache_counters
                .bypass_diagnostics
                .host_store_view_from_host_builds
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        for _ in 0..STORE_VIEW_SNAPSHOT_RETRY_ATTEMPTS {
            let snapshot_epoch = host.current_store_view_epoch();
            let view = Self::build(host, snapshot_epoch, None);
            if host.current_store_view_epoch() == snapshot_epoch {
                return view;
            }
        }

        let snapshot_epoch = host.current_store_view_epoch();
        Self::build(host, snapshot_epoch, None)
    }

    /// Build a session-scoped store view from a raw session id.
    ///
    /// The compat token includes the session identity so that two sessions
    /// with different overlays but the same epoch never coalesce into the
    /// same singleflight lane.
    ///
    /// This entry point replaces an earlier `from_session(view: &SessionView,
    /// host)` overload. The old overload took a session-scoped
    /// `SessionView` epoch carrier; under R17 the per-session
    /// overlay-mutation machinery is gone, so the singleflight
    /// lane identity is the raw `session_id` plumbed through the
    /// caller; the runtime-side epoch carrier no longer exists.
    pub(crate) fn from_session_id(session_id: u64, host: &VerterHost) -> Self {
        for _ in 0..STORE_VIEW_SNAPSHOT_RETRY_ATTEMPTS {
            let snapshot_epoch = host.current_store_view_epoch();
            let sv = Self::build(host, snapshot_epoch, Some(session_id));
            if host.current_store_view_epoch() == snapshot_epoch {
                return sv;
            }
        }

        let snapshot_epoch = host.current_store_view_epoch();
        Self::build(host, snapshot_epoch, Some(session_id))
    }

    /// Drop every per-canonical / per-domain snapshot for a
    /// session-deleted (tombstoned) canonical — there is no current
    /// content for it. Removing its `whole_hashes`, `file_facts`, and
    /// `derived_hashes` entries makes strict validation reject any warm
    /// entry rooted on the now-deleted file (`validates_self_root_whole_hash`
    /// rejects an untracked self-root; `validates_parse_domain` rejects
    /// a real fact hash for an untracked file; the `derived_hashes`
    /// validators reject an absent entry), so the consumer recomputes.
    ///
    /// The canonical is also recorded in [`Self::tombstoned_canonicals`].
    /// Removal from `whole_hashes` alone makes the canonical look
    /// *untracked* to the lazy [`StoreView::validates`] `FileWholeHash`
    /// / `DirectSource` arms — whose untracked branch optimistically
    /// accepts a genuine cross-file dependency loaded after the view
    /// snapshot. A tombstoned canonical is a *deleted* file, not a
    /// genuinely-untracked dependency, so a cross-file `FileWholeHash`
    /// dependency on it MUST be rejected; the tombstone set lets
    /// `validates` distinguish the two.
    fn drop_tombstoned_canonical_snapshots(&mut self, canonical: &str) {
        self.whole_hashes.remove(canonical);
        self.file_facts.remove(canonical);
        for kind in [
            crate::resolver_core::DerivedFactKind::Route,
            crate::resolver_core::DerivedFactKind::ImportRoute,
            crate::resolver_core::DerivedFactKind::DirectSource,
        ] {
            self.derived_hashes.remove(&(canonical.to_owned(), kind));
        }
        self.tombstoned_canonicals.insert(canonical.to_owned());
    }

    /// Re-root this view against a [`SessionView`]'s overlay so
    /// warm-read validation observes the session's CURRENT content
    /// identity rather than the base host's — across **every**
    /// per-canonical / per-domain snapshot, not just `whole_hashes`.
    ///
    /// `HostStoreView::build` snapshots every per-canonical field from
    /// the scheduler / `FileArtifactStore` — i.e. the **base** content
    /// of every tracked canonical. A query executed under a
    /// [`crate::resolver_core::SessionResolverContext`] roots its
    /// cached values (semantic-graph `MemoEntry` self-roots, the
    /// path-precise fact rail, the legacy whole-hash rail) on the
    /// **overlay** content for every overlay-bearing canonical —
    /// `ensure_indexed_ready` under a session resolves the overlay
    /// `IndexedReady`, and parse facts pin to the overlay content
    /// version. A warm read whose validation routed through the base
    /// view would compare overlay-rooted facts against base snapshots
    /// and miss on every call.
    ///
    /// Per-canonical / per-domain field treatment for the session's
    /// overlay canonicals:
    ///
    /// - **`whole_hashes`** — overlay-Upsert: set to
    ///   [`SessionView::overlay_content_hash_for`]; tombstone: removed.
    ///   The self-root `FileWholeHash` validator (`validates` /
    ///   `validates_self_root_whole_hash`) and the `DirectSource`
    ///   `DerivedFactHash` arm read this map; re-rooting it closes
    ///   them. It is also the `content_hash` dimension the
    ///   `resolve-imports` validator composes its
    ///   `ResolvedImportFactsKey` from, so re-rooting steers that
    ///   content-addressed `DashMap` lookup at the overlay slot.
    /// - **`file_facts`** — overlay-Upsert: refreshed from the overlay
    ///   `FileArtifacts` (via
    ///   [`OverlayArtifactIdentity::lookup_overlay_artifacts`](crate::host_manage::overlay_materialize::OverlayArtifactIdentity::lookup_overlay_artifacts),
    ///   which rebuilds the exact overlay-scoped key — raw-owner hash +
    ///   discriminator, normalised analysis canonical — and is
    ///   content-pinned); tombstone: removed.
    ///   `validates_parse_domain` reads this per-canonical
    ///   `Arc<FileFacts>` snapshot — a `Parse` fact pinned to the
    ///   overlay version validates against the overlay's `FileFacts`.
    /// - **`derived_hashes`** (`Route` / `ImportRoute`) — overlay-Upsert:
    ///   refreshed from the overlay `IndexedReady`
    ///   (`hash_route_surface` over the overlay `shallow_state`, and the
    ///   overlay `import_route_hash`); tombstone: removed alongside the
    ///   `DirectSource` entry. `validates` reads these per-`(canonical,
    ///   kind)` hashes; refreshing keeps an overlay-rooted
    ///   `DerivedFactHash` validating against overlay content.
    /// - **`resolved_import_facts`** / **`route_db`** — `Arc` clones of
    ///   the project store's content-addressed `DashMap`s. They are
    ///   shared and hold both the base and the overlay candidates; the
    ///   overlay candidate is reached because `whole_hashes` (the
    ///   `content_hash` key dimension) is re-rooted above. No
    ///   per-canonical re-root needed on the handle itself.
    /// - **`resolved_import_facts_known_miss_tags`** — the
    ///   `known_miss_generation` key dimension is generation-scoped, not
    ///   content-scoped; a pure overlay content edit does not advance
    ///   the project generation, so the base snapshot is correct.
    /// - **`route_surface_index_fingerprints`** — keyed by the
    ///   structural augmentation-target shape, not by canonical /
    ///   content hash. The augmentation index this snapshot mirrors is
    ///   base-only: it has no base/session population identity, so an
    ///   overlay that edits a `declare module` block would still be
    ///   summarised by the BASE augmenter set. The base snapshot is
    ///   carried unchanged here only because `EffectiveExportSet`
    ///   consumption is itself base-only —
    ///   `RouteDb::get_or_compute_effective_export_set` fails closed on
    ///   a session view, so no session consumer reads these
    ///   fingerprints. Session-correct augmentation stitching lands with
    ///   the overlay-aware augmentation-index schema.
    /// - **`import_routes`** — populated by `build` but read by no
    ///   `HostStoreView` validator; nothing to re-root.
    /// - **`env_hashes`** / **`project_identity`** / **`project_generation`**
    ///   / **`compat_token`** / **`mutation_epoch`** / **`session_id`** —
    ///   view-level identity, not per-canonical content; untouched.
    ///
    /// The override is **not** a blanket accept: every refreshed
    /// snapshot validates against the session's CURRENT overlay
    /// content. An entry rooted on a *superseded* overlay version, or
    /// on the *base* content while an overlay now covers the canonical,
    /// still misses — exactly as the un-overlaid view validates against
    /// the base's current content.
    ///
    /// A canonical the session TOMBSTONED (overlay-Deleted) has its
    /// base per-canonical snapshots dropped — see
    /// [`Self::drop_tombstoned_canonical_snapshots`]. Tombstones are
    /// reported by [`SessionView::tombstoned_canonicals`], iterated
    /// independently of [`SessionView::overlay_canonicals`]: a session
    /// can delete a file without re-upserting it (so it has no overlay
    /// source), while a canonical re-upserted after a delete appears in
    /// `overlay_canonicals` and is treated as an overlay-Upsert.
    ///
    /// Non-overlay, non-tombstoned canonicals are untouched — they keep
    /// their base snapshots, so a session that overlays or deletes one
    /// file still validates every other canonical against base content.
    #[must_use]
    pub(crate) fn with_session_overlay(
        mut self,
        host: &VerterHost,
        view: &dyn crate::session_view::SessionView,
    ) -> Self {
        // Tombstone-only canonicals: deleted by the session and never
        // re-upserted, so absent from `overlay_canonicals()`. This is
        // the delete-case analogue of the overlay-Upsert re-rooting
        // below — without it a warm entry rooted on a session-deleted
        // file's BASE content would still validate.
        for canonical in view.tombstoned_canonicals() {
            self.drop_tombstoned_canonical_snapshots(&canonical);
        }

        for canonical in view.overlay_canonicals() {
            if view.is_tombstoned(&canonical) {
                // Both an overlay-source key AND tombstoned — the
                // tombstone wins over a stale overlay-source entry.
                self.drop_tombstoned_canonical_snapshots(&canonical);
                continue;
            }
            let Some(overlay_hash) = view.overlay_content_hash_for(&canonical) else {
                continue;
            };
            // Re-root the self-root whole-hash rail.
            self.whole_hashes.insert(canonical.clone(), overlay_hash);

            // Refresh the per-domain parse-fact + derived-fact
            // snapshots from the overlay artifact. `canonical` is the
            // RAW overlay owner (from `overlay_canonicals()`);
            // `lookup_overlay_artifacts` builds the exact overlay
            // artifact key — the raw-owner overlay hash + discriminator
            // with the NORMALISED `analysis_canonical` as
            // `FileArtifactKey.canonical` — so it returns the overlay
            // `FileArtifacts` candidate (not the base one) even when
            // `normalize(raw) != raw`.
            let overlay_identity = host.overlay_artifact_identity(&canonical);
            match overlay_identity.lookup_overlay_artifacts(host, view) {
                Some(overlay_artifacts) => {
                    self.file_facts.insert(
                        canonical.clone(),
                        std::sync::Arc::clone(&overlay_artifacts.facts),
                    );
                    let overlay_indexed = &overlay_artifacts.indexed;
                    if overlay_indexed.shallow_state.has_resolvable_surface() {
                        self.derived_hashes.insert(
                            (
                                canonical.clone(),
                                crate::resolver_core::DerivedFactKind::Route,
                            ),
                            hash_route_surface(&overlay_indexed.shallow_state),
                        );
                    } else {
                        self.derived_hashes.remove(&(
                            canonical.clone(),
                            crate::resolver_core::DerivedFactKind::Route,
                        ));
                    }
                    match overlay_indexed.import_route_hash {
                        Some(hash) => {
                            self.derived_hashes.insert(
                                (
                                    canonical.clone(),
                                    crate::resolver_core::DerivedFactKind::ImportRoute,
                                ),
                                hash,
                            );
                        }
                        None => {
                            self.derived_hashes.remove(&(
                                canonical.clone(),
                                crate::resolver_core::DerivedFactKind::ImportRoute,
                            ));
                        }
                    }
                }
                None => {
                    // The overlay artifact has not been materialised
                    // yet. The base per-domain snapshots are stale
                    // relative to the overlay content; drop them so
                    // `validates_parse_domain` / the `DerivedFactHash`
                    // validator reject any entry rooted on the overlay
                    // and the consumer cold-recomputes (the correct R3
                    // outcome under stale producer state — same shape
                    // as an absent base snapshot).
                    self.file_facts.remove(&canonical);
                    self.derived_hashes.remove(&(
                        canonical.clone(),
                        crate::resolver_core::DerivedFactKind::Route,
                    ));
                    self.derived_hashes.remove(&(
                        canonical.clone(),
                        crate::resolver_core::DerivedFactKind::ImportRoute,
                    ));
                }
            }
        }
        self
    }

    fn build(host: &VerterHost, snapshot_epoch: u64, session_id: Option<u64>) -> Self {
        let mut view = Self {
            mutation_epoch: snapshot_epoch,
            session_id,
            ..Self::default()
        };

        {
            let mut canonical_ids = host.scheduler.node_ids();
            canonical_ids.extend(host.compile_cache().iter().map(|entry| entry.key().clone()));
            canonical_ids.sort();
            canonical_ids.dedup();

            for canonical_id in canonical_ids {
                if let Some(source) = host.scheduler.try_get_source(&canonical_id) {
                    view.whole_hashes
                        .insert(canonical_id.clone(), source.whole_hash);
                }

                if !view.whole_hashes.contains_key(&canonical_id) {
                    if let Some(state) = host.effective_file_state(&canonical_id, None) {
                        view.whole_hashes
                            .insert(canonical_id.clone(), state.whole_hash);
                    }
                }

                // import_routes lives on DerivedRawState (D48 split).
                // The known-miss generation sidecar (Codex P2.2 /
                // Codex-P2.2 fix) lives alongside it; capture both
                // under the same `derived_raw_cache().get(...)` so
                // the validator can compose
                // `ResolvedImportFactsKey.known_miss_generation`
                // identically to the producer.
                if let Some(entry) = host.derived_raw_cache().get(&canonical_id) {
                    for (specifier, resolution) in entry.import_routes.iter() {
                        view.import_routes.insert(
                            (canonical_id.clone(), specifier.clone()),
                            resolution.clone(),
                        );
                    }
                    let tag = crate::resolved_import_facts::compute_known_miss_generation_tag(
                        &entry.import_routes_known_miss_recorded_at_generation,
                    );
                    view.resolved_import_facts_known_miss_tags
                        .insert(canonical_id.clone(), tag);
                }
            }
        }

        // WASM-only: scheduler is unavailable on web; see CLAUDE.md "Scheduler as Sole Compile Authority".

        // Canonicals that have a current-content `IndexedReady`
        // artifact (`indexed.whole_hash == tracked`). The
        // `route_owned_shallow` snapshot MUST NOT contribute a `Route`
        // hash for any such canonical: the current-content indexed
        // artifact is the sole route-surface authority, whether or not
        // its surface is route-resolvable. A route-owned-shallow entry
        // that lingers past an `IndexedReady` materialisation (the
        // route-owned producer declines to publish a new entry once a
        // content-matching `IndexedReady` exists, but a prior entry
        // persists) would otherwise publish a route hash the producer
        // authority `current_route_surface_hash` — which returns `None`
        // as soon as a current indexed artifact exists — does not.
        let mut indexed_route_canonicals: rustc_hash::FxHashSet<String> =
            rustc_hash::FxHashSet::default();

        // Snapshot FileArtifactStore entries into the store view.
        for (canonical_id, indexed) in host.project_type_store.indexed().snapshot_all() {
            let canonical_str = canonical_id.as_ref().to_owned();
            // The tracked current whole hash for this canonical: the
            // value seeded earlier from `effective_file_state`, or
            // `indexed.whole_hash` when no current state was tracked.
            let tracked_whole_hash = *view
                .whole_hashes
                .entry(canonical_str.clone())
                .or_insert(indexed.whole_hash);
            // A current-content `IndexedReady` (`indexed.whole_hash ==
            // tracked`) is the route-surface authority for this
            // canonical. Mark it in `indexed_route_canonicals` so the
            // route-owned-shallow loop below suppresses any lingering
            // fallback entry — whether or not the indexed surface is
            // route-resolvable: `current_route_surface_hash` returns
            // `None` (no route-owned fallback) the moment a current
            // indexed artifact exists, route-resolvable or not, and the
            // store-view validator side must match. A stale
            // `IndexedReady` retained in `snapshot_all()` (whose
            // `whole_hash` no longer matches `tracked`) is NOT marked,
            // so a canonical whose current content is route-owned-only
            // still gets its route hash from the fallback loop. The
            // `Route` derived fact itself is contributed only when the
            // current indexed surface is route-resolvable.
            if indexed.whole_hash == tracked_whole_hash {
                indexed_route_canonicals.insert(canonical_str.clone());
                if indexed.shallow_state.has_resolvable_surface() {
                    view.derived_hashes.insert(
                        (
                            canonical_str.clone(),
                            crate::resolver_core::DerivedFactKind::Route,
                        ),
                        hash_route_surface(&indexed.shallow_state),
                    );
                }
            }
            // The `ImportRoute` derived fact must reflect the
            // generation-current import-target surface. A file with
            // an unresolvable specifier carries a known-miss in its
            // content-pinned `IndexedReady.import_route_hash`; that
            // snapshot would otherwise be served unchanged after a
            // new file satisfies the specifier (the importer's
            // content, hence its `IndexedReady`, does not change), so
            // a dependent cache entry would validate against a stale
            // miss. `generation_current_import_route_hash`
            // re-resolves the miss specifiers against the current
            // workspace so the validator observes the appearance.
            if let Some(hash) = host.generation_current_import_route_hash(&canonical_str) {
                view.derived_hashes.insert(
                    (
                        canonical_str,
                        crate::resolver_core::DerivedFactKind::ImportRoute,
                    ),
                    hash,
                );
            }
        }

        // Snapshot the route-only shallow cache's `Route` hashes — but
        // ONLY for canonicals that have no live current-content
        // `IndexedReady` route fact. The current-content `IndexedReady`
        // artifact is the single canonical route-surface authority: its
        // `Route` hash was inserted by the loop above. A
        // route-owned-shallow entry is the fallback shape for a
        // route-only file the indexed store has not (yet) materialised;
        // the route-owned producer itself declines to publish a new
        // entry once a content-matching `IndexedReady` exists. A
        // route-owned entry that LINGERED past an `IndexedReady`
        // materialisation must not overwrite the indexed `Route` hash —
        // a cold-compute observing the indexed surface would record a
        // hash this view could not reproduce, producing a false stale
        // miss. Centralised source order: indexed `Route` first, the
        // route-owned `Route` only for canonicals the indexed loop did
        // not cover.
        for snapshot in host.snapshot_route_owned_shallow_cache_entries() {
            if indexed_route_canonicals.contains(&snapshot.canonical_id) {
                continue;
            }
            let tracked_whole_hash = *view
                .whole_hashes
                .entry(snapshot.canonical_id.clone())
                .or_insert(snapshot.whole_hash);
            if tracked_whole_hash == snapshot.whole_hash {
                if let Some(route_hash) = snapshot.route_hash {
                    view.derived_hashes.insert(
                        (
                            snapshot.canonical_id.clone(),
                            crate::resolver_core::DerivedFactKind::Route,
                        ),
                        route_hash,
                    );
                }
            }
        }

        view.snapshot_tracked_import_route_hashes(host);
        view.snapshot_augmentation_index(host.project_type_store.indexed());
        view.snapshot_file_facts(host.project_type_store.indexed());
        // R26 per-domain producer handles captured at view-build
        // time. Cheap `Arc::clone` per snapshot; reads through the
        // handles are wait-free against concurrent writers because
        // both `ResolvedImportFactsDb` and `RouteDb` shard by key
        // (DashMap-backed).
        view.resolved_import_facts = Some(std::sync::Arc::clone(
            host.project_type_store.resolved_import_facts_handle(),
        ));
        view.route_db = Some(host.project_type_store.routes_handle());
        // R21 env-hash + project-identity capture. Required for
        // `ResolvedImportFactsKey` + `EffectiveExportSetKey`
        // composition inside the per-domain validators.
        view.env_hashes = host.host_view_env_hashes();
        view.project_identity = host.host_view_project_identity();
        // Project-generation capture for the
        // `FactVersionRef::ProjectGeneration` validator. The host /
        // workspace layer owns this counter; the view records it
        // unchanged so a warm read rejects a value rooted on a
        // superseded generation.
        view.project_generation = host.project_type_store.project_generation();
        view.compat_token = view.compute_compat_token();
        view
    }

    /// Snapshot `Arc<FileFacts>` per canonical from the indexed
    /// store. One refcount bump per tracked file at view-build time;
    /// parse-domain validation reads through these handles
    /// wait-free against concurrent writers because each entry is
    /// immutable.
    ///
    /// If multiple `(content_hash, parse_env_hash)` variants coexist
    /// for one canonical (the multi-candidate cache shape under R20),
    /// the first one encountered wins — subsequent variants do not
    /// overwrite. The view's `whole_hashes` map records the canonical
    /// content hash; a path-precise consumer that observed against
    /// a parse-env-hash variant outside this snapshot will miss
    /// validation and recompute against the current variant.
    fn snapshot_file_facts(&mut self, store: &crate::file_artifact_store::FileArtifactStore) {
        // Snapshot ONLY the `FileFacts` variant whose `content_hash`
        // matches the view's tracked `whole_hashes[canonical]` —
        // that is the source-of-truth content hash for the
        // canonical under this view. Other variants (stale
        // candidates from prior content generations) coexist in
        // the multi-candidate store per R20 but must NOT back the
        // parse-domain validator: a path-precise consumer observed
        // against the live content, so its validation MUST consult
        // the live content's facts.
        //
        // When the artifact store has not yet been refreshed for
        // the new content (lazy `ensure_indexed_ready` has not run
        // yet), the `file_facts` entry for that canonical stays
        // ABSENT. The parse-domain validator interprets absence as
        // a miss (`validates_parse_domain` returns `false` for any
        // observed real-hash fact under an absent entry) — the
        // consumer falls through to cold recompute, which is the
        // correct R3 outcome under stale producer state.
        for (key, artifacts) in store.snapshot_artifacts() {
            let canonical_str = key.canonical.as_ref().to_owned();
            let matches_live = match self.whole_hashes.get(&canonical_str) {
                Some(h) => key.content_hash == *h,
                None => false,
            };
            if matches_live {
                self.file_facts
                    .insert(canonical_str, std::sync::Arc::clone(&artifacts.facts));
            }
        }
    }

    fn snapshot_tracked_import_route_hashes(&mut self, host: &VerterHost) {
        let canonical_ids: Vec<String> = self.whole_hashes.keys().cloned().collect();
        let empty_import_routes = FxHashMap::default();
        let empty_import_route_hash = hash_import_route_targets(&empty_import_routes);

        for canonical_id in canonical_ids {
            if self.derived_hashes.contains_key(&(
                canonical_id.clone(),
                crate::resolver_core::DerivedFactKind::ImportRoute,
            )) {
                continue;
            }

            // Generation-current `ImportRoute` fact for files not
            // covered by the `IndexedReady` snapshot loop above —
            // re-resolves known-miss specifiers against the current
            // workspace so a previously-unresolvable dependency's
            // appearance is observable by the validator.
            let import_route_hash = host.generation_current_import_route_hash(&canonical_id);

            self.derived_hashes.insert(
                (
                    canonical_id.clone(),
                    crate::resolver_core::DerivedFactKind::ImportRoute,
                ),
                import_route_hash.unwrap_or(empty_import_route_hash),
            );
        }
    }

    /// Snapshot the augmentation-index fingerprints from a
    /// [`FileArtifactStore`] into this view (R29 + G1). Called by
    /// `build` when the host's project-type-store is reachable, and
    /// directly from tests that construct a view over a standalone
    /// `FileArtifactStore`.
    pub(crate) fn snapshot_augmentation_index(
        &mut self,
        artifact_store: &crate::file_artifact_store::FileArtifactStore,
    ) {
        for (key, fingerprint) in artifact_store.snapshot_augmentation_index_fingerprints() {
            let snap_key = RouteSurfaceIndexShapeKey {
                target_kind_tag: augmentation_target_kind_tag_for(&key.target),
                external_specifier: augmentation_target_external_specifier(&key.target),
                resolved_relative_canonical: augmentation_target_resolved_relative_canonical(
                    &key.target,
                ),
                wildcard_pattern: augmentation_target_wildcard_pattern(&key.target),
            };
            self.route_surface_index_fingerprints
                .insert(snap_key, fingerprint);
        }
    }

    pub(crate) fn mutation_epoch(&self) -> u64 {
        self.mutation_epoch
    }

    #[allow(dead_code)]
    pub(crate) fn whole_hash(&self, canonical_id: &str) -> Option<Hash16> {
        self.whole_hashes.get(canonical_id).copied()
    }

    #[allow(dead_code)]
    pub(crate) fn derived_hash(
        &self,
        canonical_id: &str,
        kind: crate::resolver_core::DerivedFactKind,
    ) -> Option<Hash16> {
        self.derived_hashes
            .get(&(canonical_id.to_string(), kind))
            .copied()
    }

    pub(crate) fn invalid_fact_details(
        &self,
        facts: &[crate::resolver_core::FactVersionRef],
        limit: usize,
    ) -> Vec<String> {
        facts
            .iter()
            .filter_map(|fact| self.describe_invalid_fact(fact))
            .take(limit)
            .collect()
    }

    fn describe_invalid_fact(&self, fact: &crate::resolver_core::FactVersionRef) -> Option<String> {
        match fact {
            crate::resolver_core::FactVersionRef::FileWholeHash { canonical_id, hash } => {
                match self.whole_hashes.get(canonical_id) {
                    Some(current) if current == hash => None,
                    Some(current) => Some(format!(
                        "FileWholeHash mismatch canonical={} expected={hash:?} actual={current:?}",
                        canonical_id
                    )),
                    None => Some(format!(
                        "FileWholeHash missing canonical={} expected={hash:?}",
                        canonical_id
                    )),
                }
            }
            crate::resolver_core::FactVersionRef::DerivedFactHash {
                canonical_id,
                kind,
                hash,
            } => {
                let current = match kind {
                    crate::resolver_core::DerivedFactKind::DirectSource => {
                        self.whole_hashes.get(canonical_id)
                    }
                    _ => self.derived_hashes.get(&(canonical_id.clone(), *kind)),
                };
                match current {
                    Some(current) if current == hash => None,
                    Some(current) => Some(format!(
                        "DerivedFactHash mismatch canonical={} kind={kind:?} expected={hash:?} actual={current:?}",
                        canonical_id
                    )),
                    None => Some(format!(
                        "DerivedFactHash missing canonical={} kind={kind:?} expected={hash:?}",
                        canonical_id
                    )),
                }
            }
            // R26 per-domain variants — per-domain producers populate
            // the matching stores and produce structured diagnostics
            // there. `HostStoreView` does not observe them directly,
            // so the diagnostic shape is a generic "domain fact not
            // validated yet" string.
            crate::resolver_core::FactVersionRef::Parse(p) => Some(format!(
                "ParseFactRef canonical={} key={:?} lane={:?} expected={:?}",
                p.canonical_id, p.key, p.lane, p.expected_hash
            )),
            crate::resolver_core::FactVersionRef::ResolveImports(r) => Some(format!(
                "ResolveImportsFactRef canonical={} key={:?} lane={:?} expected={:?}",
                r.canonical_id, r.key, r.lane, r.expected_hash
            )),
            crate::resolver_core::FactVersionRef::RouteSurface(r) => Some(format!(
                "RouteSurfaceFactRef canonical={} key={:?} lane={:?} expected={:?}",
                r.canonical_id, r.key, r.lane, r.expected_hash
            )),
            crate::resolver_core::FactVersionRef::ProjectGeneration { generation } => {
                if self.project_generation == *generation {
                    None
                } else {
                    Some(format!(
                        "ProjectGeneration mismatch expected={generation} actual={}",
                        self.project_generation
                    ))
                }
            }
        }
    }

    fn compute_compat_token(&self) -> crate::resolver_core::StoreViewCompatToken {
        crate::resolver_core::StoreViewCompatToken {
            epoch: self.mutation_epoch,
            session: self.session_id,
        }
    }

    /// Overlay-aware variant of
    /// [`crate::resolver_core::StoreView::validates_resolve_imports_domain`]:
    /// composes the `ResolvedImportFactsKey` against the supplied
    /// `content_hash` rather than `self.whole_hashes[canonical]`. Used
    /// by [`crate::resolver_core::RequestStoreView`] when a canonical
    /// was promoted into the per-request completion overlay after the
    /// base view was built (codex re-review B6.C-rfx2). All other key
    /// dimensions (`parse_env_hash`, `resolve_env_hash`,
    /// `resolver_version`, `known_miss_generation`) compose against
    /// the base view's snapshot unchanged.
    pub(crate) fn validates_resolve_imports_domain_for_content_hash(
        &self,
        fact: &crate::resolver_core::ResolveImportsFactRef,
        content_hash: Hash16,
    ) -> bool {
        use verter_semantic::facts::registry::FactLane;
        use verter_semantic::facts::FactKey;
        const ZERO_HASH: Hash16 = [0u8; 16];

        let facts_db = match self.resolved_import_facts.as_ref() {
            Some(db) => db,
            None => return false,
        };

        // `known_miss_generation` (Codex P2.2 fix):
        // captured at view-build time from
        // `DerivedRawState::import_routes_known_miss_recorded_at_generation`.
        // Absent entries → `[0u8; 16]` so an owner that never had
        // `set_import_dependencies` called still composes the same
        // key value the producer admitted under (the producer also
        // reads `[0u8; 16]` when there is no `DerivedRawState`
        // entry yet).
        let known_miss_generation = self
            .resolved_import_facts_known_miss_tags
            .get(fact.canonical_id.as_str())
            .copied()
            .unwrap_or(ZERO_HASH);

        let key = crate::resolved_import_facts::ResolvedImportFactsKey {
            canonical: std::sync::Arc::from(fact.canonical_id.as_str()),
            content_hash,
            parse_env_hash: self.env_hashes.parse_env_hash,
            resolve_env_hash: self.env_hashes.resolve_env_hash,
            resolver_version: crate::resolved_import_facts::RESOLVED_IMPORT_FACTS_RESOLVER_VERSION,
            known_miss_generation,
        };

        let facts = match facts_db.get(&key) {
            Some(f) => f,
            // Cache slot absent — the consumer observed a real fact
            // hash but the resolve-imports producer has not yet
            // populated the entry under this view. Reject so the
            // caller recomputes through the producer (which will
            // populate the cache + re-emit).
            None => return fact.expected_hash == ZERO_HASH,
        };

        // Pick the lane that the consumer observed under.
        let pick_lane = |f: &std::sync::Arc<verter_semantic::facts::registry::Fact>| match fact.lane
        {
            FactLane::Semantic => f.semantic_hash,
            FactLane::Display => f.display_hash,
        };

        match &fact.key {
            FactKey::ResolvedImportClause {
                specifier,
                binding,
                space,
                resolved_canonical,
                resolved_source_name,
            } => facts.import_clauses.iter().any(|entry| {
                entry.specifier == *specifier
                    && entry.binding == *binding
                    && entry.space == *space
                    && entry.resolved_canonical.as_ref().map(|c| c.as_ref())
                        == Some(resolved_canonical.as_ref())
                    && entry.resolved_source_name == *resolved_source_name
                    && pick_lane(&entry.fact) == fact.expected_hash
            }),
            FactKey::ResolvedReexportBinding {
                specifier,
                source_name,
                target_name,
                space,
                resolved_canonical,
                resolved_source_name,
            } => facts.reexport_bindings.iter().any(|entry| {
                entry.specifier == *specifier
                    && entry.source_name == *source_name
                    && entry.target_name == *target_name
                    && entry.space == *space
                    && entry.resolved_canonical.as_ref().map(|c| c.as_ref())
                        == Some(resolved_canonical.as_ref())
                    && entry.resolved_source_name == *resolved_source_name
                    && pick_lane(&entry.fact) == fact.expected_hash
            }),
            // Non-resolve-imports FactKey shapes do not belong to
            // the resolve-imports domain. The dispatch layer routes
            // by `FactDomain` so this arm is defensive.
            _ => false,
        }
    }
}

pub(crate) fn hash_import_route_targets(
    resolutions: &FxHashMap<String, crate::types::DependencyResolution>,
) -> Hash16 {
    let mut entries: Vec<_> = resolutions.iter().collect();
    entries.sort_by(|a, b| a.0.cmp(b.0));

    hash16_from_sorted(|hasher| {
        for (specifier, resolution) in &entries {
            0u8.hash(hasher);
            specifier.hash(hasher);
            resolution
                .resolved_canonical_id
                .clone()
                .or_else(|| resolution.effective_target().map(str::to_string))
                .hash(hasher);
        }
    })
}

pub(crate) fn hash_route_surface(state: &crate::resolver_core::ShallowFileState) -> Hash16 {
    hash16_from_sorted(|hasher| {
        // Hash sorted export names.
        let mut export_names: Vec<&str> = state.exports.keys().map(|s| s.as_str()).collect();
        export_names.sort_unstable();
        for name in &export_names {
            name.hash(hasher);
        }

        // Hash wildcard reexport source specifiers in declaration order.
        for wildcard in &state.wildcard_reexports {
            wildcard.source_specifier.hash(hasher);
            wildcard.canonical_id.hash(hasher);
        }

        // Hash the file content hash.
        state.whole_hash.hash(hasher);
    })
}

fn hash16_from_sorted(f: impl Fn(&mut rustc_hash::FxHasher)) -> Hash16 {
    let mut left = rustc_hash::FxHasher::default();
    0u8.hash(&mut left);
    f(&mut left);

    let mut right = rustc_hash::FxHasher::default();
    1u8.hash(&mut right);
    f(&mut right);

    let mut out = [0u8; 16];
    out[..8].copy_from_slice(&left.finish().to_le_bytes());
    out[8..].copy_from_slice(&right.finish().to_le_bytes());
    out
}

/// Map an [`AugmentationTargetKind`] into the parallel-fields shape
/// the parse-domain [`FactKey::ModuleAugmentationIndexShape`] +
/// audit-event variants use.
pub(crate) fn augmentation_target_kind_tag_for(
    target: &crate::file_artifact_store::AugmentationTargetKind,
) -> verter_semantic::facts::registry::AugmentationTargetKindTag {
    use crate::file_artifact_store::AugmentationTargetKind;
    use verter_semantic::facts::registry::AugmentationTargetKindTag;
    match target {
        AugmentationTargetKind::ExternalSpecifier(_) => {
            AugmentationTargetKindTag::ExternalSpecifier
        }
        AugmentationTargetKind::ResolvedRelativeCanonical(_) => {
            AugmentationTargetKindTag::ResolvedRelativeCanonical
        }
        AugmentationTargetKind::WildcardAmbient(_) => AugmentationTargetKindTag::WildcardAmbient,
        AugmentationTargetKind::GlobalAugmentation => AugmentationTargetKindTag::GlobalAugmentation,
    }
}

pub(crate) fn augmentation_target_external_specifier(
    target: &crate::file_artifact_store::AugmentationTargetKind,
) -> Option<String> {
    use crate::file_artifact_store::AugmentationTargetKind;
    match target {
        AugmentationTargetKind::ExternalSpecifier(spec) => Some(spec.as_ref().to_owned()),
        _ => None,
    }
}

pub(crate) fn augmentation_target_resolved_relative_canonical(
    target: &crate::file_artifact_store::AugmentationTargetKind,
) -> Option<String> {
    use crate::file_artifact_store::AugmentationTargetKind;
    match target {
        AugmentationTargetKind::ResolvedRelativeCanonical(canon) => Some(canon.as_ref().to_owned()),
        _ => None,
    }
}

pub(crate) fn augmentation_target_wildcard_pattern(
    target: &crate::file_artifact_store::AugmentationTargetKind,
) -> Option<String> {
    use crate::file_artifact_store::AugmentationTargetKind;
    match target {
        AugmentationTargetKind::WildcardAmbient(pat) => Some(pat.as_ref().to_owned()),
        _ => None,
    }
}

impl crate::resolver_core::StoreView for HostStoreView {
    fn compat_token(&self) -> crate::resolver_core::StoreViewCompatToken {
        self.compat_token
    }

    fn validates(&self, fact: &crate::resolver_core::FactVersionRef) -> bool {
        match fact {
            crate::resolver_core::FactVersionRef::FileWholeHash { canonical_id, hash } => {
                // Session-tombstoned canonical: the file is DELETED in
                // this session. A cross-file `FileWholeHash` dependency
                // on a deleted file is invalid — reject before the lazy
                // untracked-accept rule below. `with_session_overlay`
                // removed the canonical from `whole_hashes`, so without
                // this guard it would fall into the `None => true`
                // untracked branch and a parent entry depending on the
                // deleted file would still validate.
                if self.tombstoned_canonicals.contains(canonical_id) {
                    return false;
                }
                match self.whole_hashes.get(canonical_id) {
                    Some(current) => current == hash,
                    // File not tracked by this store view — it was loaded as a
                    // dependency AFTER the view snapshot was taken. Accept it:
                    // the facts were just materialized from current disk/workspace
                    // state and are valid. This avoids forcing every dependency
                    // access through the expensive permissive fallback path in
                    // `ensure_indexed_ready`.
                    None => true,
                }
            }
            crate::resolver_core::FactVersionRef::DerivedFactHash {
                canonical_id,
                kind,
                hash,
            } => match kind {
                crate::resolver_core::DerivedFactKind::DirectSource => {
                    // `DirectSource` is a content-hash alias for
                    // `FileWholeHash` (it reads `whole_hashes`) — apply
                    // the same tombstone rejection so the
                    // removal-makes-it-look-untracked window cannot be
                    // re-exploited on the `DirectSource` rail.
                    if self.tombstoned_canonicals.contains(canonical_id) {
                        return false;
                    }
                    match self.whole_hashes.get(canonical_id) {
                        Some(current) => current == hash,
                        // Untracked dependency file — accept (same reasoning
                        // as FileWholeHash above).
                        None => true,
                    }
                }
                _ => self
                    .derived_hashes
                    .get(&(canonical_id.clone(), *kind))
                    .is_some_and(|current| current == hash),
            },
            // R26 per-domain variants — route to the per-domain
            // validators. `HostStoreView` participates in the
            // legacy whole-hash regime today; the per-domain
            // validators are populated by their respective
            // producers. Default impls (returning `false`) are
            // inherited from the trait until per-domain producers
            // wire actual validation through this view.
            // R26 per-domain variants — route to the per-domain
            // validators (which return `false` by trait default;
            // per-domain producers override).
            crate::resolver_core::FactVersionRef::Parse(p) => {
                crate::resolver_core::StoreView::validates_parse_domain(self, p)
            }
            crate::resolver_core::FactVersionRef::ResolveImports(r) => {
                crate::resolver_core::StoreView::validates_resolve_imports_domain(self, r)
            }
            crate::resolver_core::FactVersionRef::RouteSurface(r) => {
                crate::resolver_core::StoreView::validates_route_surface_domain(self, r)
            }
            // Project-generation fact: the cached value observed the
            // project-wide generation `generation`. It validates iff
            // the generation snapshotted at this view's build time
            // still matches — a project-shape change (`tsconfig`,
            // path-alias, SDK, workspace-folder, project-graph) bumps
            // the counter and rejects the entry.
            crate::resolver_core::FactVersionRef::ProjectGeneration { generation } => {
                self.project_generation == *generation
            }
        }
    }

    fn tracks_file(&self, canonical_id: &str) -> bool {
        self.whole_hashes.contains_key(canonical_id)
    }

    /// Direct read of the snapshotted `DerivedFactHash` for a
    /// `(canonical, kind)` pair. Used by per-rejection attribution
    /// helpers to discriminate "entry absent" from "entry present,
    /// hash differs" without re-probing the validator.
    fn derived_hash_for(
        &self,
        canonical_id: &str,
        kind: crate::resolver_core::DerivedFactKind,
    ) -> Option<crate::resolver_core::ResolverHash16> {
        self.derived_hashes
            .get(&(canonical_id.to_owned(), kind))
            .copied()
    }

    /// Strict self-root `FileWholeHash` validation.
    ///
    /// Unlike the [`Self::validates`] `FileWholeHash` arm — whose
    /// untracked-file branch optimistically returns `true` so a
    /// dependency loaded after the view snapshot is not forced through
    /// a permissive recheck — this strict variant returns `false` for
    /// an untracked keyed canonical. A self-root names a query-identity
    /// cache entry's OWN keyed canonical; if that file is untracked by
    /// the live view its content is unknown here, which must invalidate
    /// the entry (a same-canonical content edit must not survive). A
    /// tracked canonical is validated by exact hash equality, identical
    /// to the [`Self::validates`] tracked arm.
    fn validates_self_root_whole_hash(
        &self,
        canonical_id: &str,
        hash: &crate::resolver_core::ResolverHash16,
    ) -> bool {
        match self.whole_hashes.get(canonical_id) {
            Some(current) => current == hash,
            // Untracked self-root canonical — the entry's own file is
            // not in this view. Reject: the warm read misses and
            // recomputes against current content.
            None => false,
        }
    }

    /// Parse-domain validator (R26).
    ///
    /// Look up `fact.key` against the file's `FileFacts` registry and
    /// compare the stored fact's `semantic_hash` / `display_hash`
    /// (per `fact.lane`) to the observed `expected_hash`. The lookup
    /// resolves the current `FileArtifacts` for `canonical_id` from
    /// the project type store; the view snapshot's `whole_hashes`
    /// already pins the parse-env-hash slice the artifacts derive
    /// from, so this read is wait-free against concurrent writers.
    ///
    /// `None` outcomes — file untracked, artifacts absent, key not
    /// in registry — all signal "no longer there", which under R3
    /// must invalidate the consumer's warm hit. The validator
    /// therefore returns `false` rather than the optimistic-accept
    /// shape used for `FileWholeHash` untracked files: a path-precise
    /// `Member`/`MemberPresence` consumer expects the fact to BE in
    /// the registry it recorded, so absence is a discriminating miss.
    fn validates_parse_domain(&self, fact: &crate::resolver_core::ParseFactRef) -> bool {
        const ZERO_HASH: Hash16 = [0u8; 16];
        let facts = match self.file_facts.get(fact.canonical_id.as_str()) {
            Some(f) => f,
            // Untracked file — accept if the observed hash was the
            // zero sentinel (producer saw the file as unavailable
            // and recorded the sentinel; absence is consistent).
            // Otherwise reject — the consumer observed a real fact
            // hash but the file has dropped out of the index.
            None => return fact.expected_hash == ZERO_HASH,
        };
        match facts.lookup(&fact.key) {
            Some(stored) => {
                let stored_hash = match fact.lane {
                    verter_semantic::facts::registry::FactLane::Semantic => stored.semantic_hash,
                    verter_semantic::facts::registry::FactLane::Display => stored.display_hash,
                };
                stored_hash == fact.expected_hash
            }
            // Fact absent in registry — accept iff observed was the
            // zero sentinel (consistent absence — see
            // `fact_signature_helpers::parse_fact_ref`).
            None => fact.expected_hash == ZERO_HASH,
        }
    }

    /// Resolve-imports-domain validator (R26).
    ///
    /// Compose `ResolvedImportFactsKey { canonical, content_hash,
    /// parse_env_hash, resolve_env_hash, resolver_version,
    /// known_miss_generation }` from the fact's `canonical_id`, the
    /// view's tracked `whole_hashes[canonical]`,
    /// `resolved_import_facts_known_miss_tags[canonical]`, and the
    /// view's `env_hashes`. Look up the matching
    /// `Arc<ResolvedImportFacts>` from the captured
    /// `ResolvedImportFactsDb` handle and compare the per-binding
    /// `semantic_hash` / `display_hash` (per `fact.lane`) of the
    /// matching `ResolvedImportClauseEntry` or
    /// `ResolvedReexportBindingEntry` against `expected_hash`.
    ///
    /// Outcomes:
    /// - Handle missing (view built without a resolved-import-facts
    ///   snapshot) → reject. A consumer that observed a real fact
    ///   under no producer is a bug; the caller falls back to cold
    ///   compute, which will re-emit through the producer.
    /// - File untracked under the view (no `whole_hashes[canonical]`
    ///   entry) → accept the optimistic content-hash sentinel
    ///   (`expected_hash == ZERO_HASH`); reject any real fact hash
    ///   for an untracked file (same shape as
    ///   `validates_parse_domain`).
    /// - Cache slot absent for the composed key → reject. The cache
    ///   was the recording site; absence means the consumer
    ///   observed a stale slice.
    /// - Binding present and hash matches → accept; hash differs →
    ///   reject (cosmetic-only edit invalidates display-lane
    ///   consumers but not semantic-lane consumers, per the lane
    ///   discriminator).
    fn validates_resolve_imports_domain(
        &self,
        fact: &crate::resolver_core::ResolveImportsFactRef,
    ) -> bool {
        const ZERO_HASH: Hash16 = [0u8; 16];

        // R26 producer: untracked-file optimistic-accept window. A
        // path-precise resolve-imports consumer that observed against
        // a sentinel hash (`ZERO_HASH`) means "this file produced no
        // value at observation time"; accept that observation against
        // an untracked file (still produces no value).
        let content_hash = match self.whole_hashes.get(fact.canonical_id.as_str()) {
            Some(h) => *h,
            None => return fact.expected_hash == ZERO_HASH,
        };

        self.validates_resolve_imports_domain_for_content_hash(fact, content_hash)
    }

    /// Route-surface-domain validator (R26 + R29 + G1).
    ///
    /// `ModuleAugmentationIndexShape` → consult the snapshot of
    /// augmentation-index fingerprints captured at view-build time
    /// (R29 / G1 producer state).
    ///
    /// `EffectiveExportSet` → compose
    /// `EffectiveExportSetKey { provider_canonical,
    /// project_identity, resolve_env_hash, lib_env_hash }` from the
    /// fact's `canonical_id` plus the view's `project_identity` +
    /// `env_hashes`, look up the cached entry in the captured
    /// `RouteDb` handle, and compare the entry's
    /// `augmenter_set_fingerprint` to `fact.expected_hash`.
    fn validates_route_surface_domain(
        &self,
        fact: &crate::resolver_core::RouteSurfaceFactRef,
    ) -> bool {
        use verter_semantic::facts::FactKey;
        match &fact.key {
            FactKey::ModuleAugmentationIndexShape {
                target_kind_tag,
                external_specifier,
                resolved_relative_canonical,
                wildcard_pattern,
            } => {
                let key = RouteSurfaceIndexShapeKey {
                    target_kind_tag: *target_kind_tag,
                    external_specifier: external_specifier.as_ref().map(|s| s.as_ref().to_owned()),
                    resolved_relative_canonical: resolved_relative_canonical
                        .as_ref()
                        .map(|s| s.as_ref().to_owned()),
                    wildcard_pattern: wildcard_pattern.as_ref().map(|s| s.as_ref().to_owned()),
                };
                match self.route_surface_index_fingerprints.get(&key) {
                    Some(current) => current == &fact.expected_hash,
                    // Absent from the snapshot — the augmentation
                    // index has not been populated under this view.
                    // Refuse the candidate so the consumer recomputes
                    // through the cold path (which will populate the
                    // index).
                    None => false,
                }
            }
            FactKey::EffectiveExportSet => {
                let route_db = match self.route_db.as_ref() {
                    Some(db) => db,
                    None => return false,
                };
                // Compose the `EffectiveExportSetKey` from the fact's
                // `canonical_id` (provider) + view env. Then walk the
                // cache slot for `provider_canonical`; we cannot call
                // `get_effective_export_set(_, view)` here because we
                // ARE the view — that would recurse on validation.
                // Permissive cache-state snapshot via `snapshot_all`
                // is acceptable: the validator only needs to find a
                // candidate whose `augmenter_set_fingerprint` matches
                // the consumer's `expected_hash` under the matching
                // `(provider, project, resolve_env, lib_env)`
                // quadruple.
                let target_key = crate::resolver_core::route_db::EffectiveExportSetKey {
                    provider_canonical: fact.canonical_id.clone(),
                    project_identity: self.project_identity,
                    resolve_env_hash: self.env_hashes.resolve_env_hash,
                    lib_env_hash: self.env_hashes.lib_env_hash,
                };
                route_db.lookup_effective_export_set_fingerprint(&target_key)
                    == Some(fact.expected_hash)
            }
            // Other parse-domain / resolve-domain keys do not belong
            // to the route-surface domain; the dispatch layer guards
            // against this so the match is exhaustive defensively.
            _ => false,
        }
    }
}

impl crate::resolver_core::ResolverStore for VerterHost {
    type View = HostStoreView;

    fn snapshot_view(&self) -> Self::View {
        self.resolver_store_view()
    }
}

impl VerterHost {
    #[track_caller]
    pub(crate) fn resolver_store_view(&self) -> HostStoreView {
        HostStoreView::from_host(self)
    }

    pub(crate) fn component_meta_audit_store_snapshot(
        &self,
        store_view: Option<&HostStoreView>,
    ) -> (
        crate::component_meta_audit::RequestStoreAudit,
        ComponentMetaStoreCounters,
    ) {
        // Entry count and byte sum MUST be drawn from the SAME
        // population. `FileArtifactStore::len` counts every keyed
        // artifact (base + overlay-scoped); the byte sum therefore
        // routes through `snapshot_artifacts()`, which enumerates that
        // same full keyed set. `snapshot_all()` is base-only (it
        // filters to `FileArtifactKey::is_legacy` keys), so summing
        // bytes over it while counting entries via `len()` would report
        // two different populations in a session that materialised
        // overlay artifacts.
        let artifacts = self.project_type_store.indexed().snapshot_artifacts();
        let indexed_entries = artifacts.len() as u32;
        let indexed_bytes = artifacts
            .iter()
            .map(|(key, file_artifacts)| {
                key.canonical.len() as u64
                    + file_artifacts.indexed.raw_source.len() as u64
                    + file_artifacts.indexed.eval_source.len() as u64
            })
            .sum::<u64>();

        let prepared_bundles = self
            .resolver_runtime()
            .prepared_decl_bundles
            .cached_values();
        let prepared_type_decls = prepared_bundles.iter().fold(0u32, |count, bundle| {
            count.saturating_add(bundle.prepared_type_decls.len() as u32)
        });
        let prepared_value_decls = prepared_bundles.iter().fold(0u32, |count, bundle| {
            count.saturating_add(bundle.prepared_value_decls.len() as u32)
        });

        // Pull per-request materialiser/storage counters off the
        // active `RequestContext` (zero ops when no context is
        // installed; the audit pipeline always installs one before
        // taking this snapshot). These counters move into the
        // component-meta payload — they are kind-specific and do
        // not belong on the generic `RequestStoreAudit`.
        let component_meta_counters = match crate::request_context::current_request_context() {
            Some(ctx) => ComponentMetaStoreCounters {
                materialize_structure_calls: ctx
                    .materialize_structure_calls
                    .load(std::sync::atomic::Ordering::Relaxed),
                materialize_structure_cache_hits: ctx
                    .materialize_structure_cache_hits
                    .load(std::sync::atomic::Ordering::Relaxed),
                node_arena_lock_acquisitions: ctx
                    .node_arena_lock_acquisitions
                    .load(std::sync::atomic::Ordering::Relaxed),
                family_map_lock_acquisitions: ctx
                    .family_map_lock_acquisitions
                    .load(std::sync::atomic::Ordering::Relaxed),
                dep_signature_merges: ctx
                    .dep_signature_merges
                    .load(std::sync::atomic::Ordering::Relaxed),
                dep_signature_intern_hits: ctx
                    .dep_signature_intern_hits
                    .load(std::sync::atomic::Ordering::Relaxed),
            },
            None => ComponentMetaStoreCounters::default(),
        };

        let store_audit = crate::component_meta_audit::RequestStoreAudit {
            store_view_hits: u32::from(store_view.is_some()),
            store_view_misses: u32::from(store_view.is_none()),
            structural_merges: 0,
            imported_dependency_entries: indexed_entries,
            imported_dependency_bytes: indexed_bytes,
            prepared_type_decls,
            prepared_value_decls,
            cache_layers: Default::default(),
            bypass_diagnostics: crate::component_meta_audit::snapshot_bypass_diagnostics_from_tls(),
        };
        (store_audit, component_meta_counters)
    }

    pub(crate) fn component_meta_audit_memory_bytes(&self) -> (u64, u64) {
        let host_cache_bytes: u64 = self
            .project_type_store
            .indexed()
            .snapshot_all()
            .iter()
            .map(|(id, indexed)| {
                id.len() as u64 + indexed.raw_source.len() as u64 + indexed.eval_source.len() as u64
            })
            .sum();

        let workspace = self.workspace();
        let workspace_snapshot = workspace.resource_snapshot();
        let workspace_bytes = workspace_snapshot.overlay_bytes + workspace_snapshot.snapshot_bytes;

        (host_cache_bytes, workspace_bytes)
    }
}

#[cfg(test)]
impl HostStoreView {
    /// Test-only constructor: a view that tracks exactly the supplied
    /// `whole_hashes` map and is otherwise [`HostStoreView::default`].
    ///
    /// `whole_hashes` is a private field, so the unit tests in the
    /// sibling `resolver_store_tests` module cannot build the view via
    /// a struct literal — they construct it through this helper.
    pub(crate) fn with_whole_hashes_for_tests(whole_hashes: FxHashMap<String, Hash16>) -> Self {
        Self {
            whole_hashes,
            ..Self::default()
        }
    }

    /// Test-only: forget that `canonical` was tracked. The view loses
    /// its `whole_hashes` entry (so the `FileWholeHash` / resolve-
    /// imports validators see it as untracked) but retains all other
    /// snapshot state (`resolved_import_facts`, `env_hashes`, etc.).
    /// Used by the codex re-review B6.C-rfx2 P2 #2 discriminating
    /// test to simulate a base view that pre-dates a mid-request
    /// `ensure_loaded` promotion of the canonical.
    pub(crate) fn forget_whole_hash_for_tests(&mut self, canonical: &str) {
        self.whole_hashes.remove(canonical);
    }

    /// Test-only: peek the view's `whole_hashes` entry for a canonical
    /// id. The codex re-review B6.C-rfx2 P2 #2 discriminating test
    /// reads the owner's authoritative content hash here so it can
    /// stage the overlay's `whole_hashes` entry with the same hash
    /// the producer admitted under.
    pub(crate) fn whole_hashes_get_for_tests(&self, canonical: &str) -> Option<Hash16> {
        self.whole_hashes.get(canonical).copied()
    }
}

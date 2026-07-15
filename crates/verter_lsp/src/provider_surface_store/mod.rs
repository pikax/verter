//! The authoritative, generation-stamped store of the provider FILE SURFACES
//! (`{carrier}.tsx` IDE, `{carrier}.ts` PUBLIC-API, shadow, and real) synced to
//! the type provider, keyed by their VIRTUAL provider path.
//!
//! ## Why this exists (fail-closed cross-file rename mapping)
//!
//! When the provider (tsserver/tsgo) renames a cross-file Vue prop, it reports
//! the edit against the imported component's `{carrier}.ts` PUBLIC-API surface
//! (e.g. `Child.vue.ts`). Those offsets index whatever content was LAST SYNCED
//! to the provider under that path, and the merge must map them back onto the
//! `.vue` source through the EXACT `CodeTransform` source map that produced them.
//!
//! Provider sync is asynchronous and arrives through MANY paths; tsserver's
//! `open`/`updateOpen` are no-response notifications. The carrier `.vue` may be
//! CLOSED in the editor. A wrong mapping silently CORRUPTS the user's `.vue`, so
//! the mapping must be fail-closed: map only through the precise content the
//! offsets were produced against, or drop.
//!
//! ## The mechanism: immutable, generation-stamped snapshots
//!
//! Every successful sync of a provider surface RECORDS an immutable
//! [`ProviderSurfaceSnapshot`] under a fresh monotonic GENERATION. The store
//! keeps the snapshots HISTORICAL — keyed by `(provider_path, generation)` — and
//! tracks the CURRENT generation per path. A cross-file rename:
//!
//! 1. captures the CURRENT snapshot set (cheap `Arc` clones) under a fence,
//! 2. queries the provider,
//! 3. interprets the returned offsets ONLY against the captured snapshot's exact
//!    generation (looked up by stamp), and
//! 4. maps through that snapshot's own source map, or DROPS.
//!
//! Because snapshots are immutable and historical, a concurrent sync that
//! advances the generation, or a CLOSE that retires the ACTIVE generation, can
//! NEVER retroactively change a snapshot an in-flight request already captured.
//! This is the property the prior "latest-only identity gate" lacked: it checked
//! the LATEST identity, not the generation the offsets were produced against, so
//! it both over-dropped (a fresher latest entry) and admitted a residual race.

use std::collections::HashMap;
use std::sync::Arc;

use dashmap::DashMap;
use parking_lot::RwLock;

use verter_semantic::analysis::types::Hash16;

use crate::carrier_cache::{EngineRecheckState, RegenKey};

use crate::documents::line_index::LineIndex;
use crate::documents::provider_projection::ProviderPositionMapper;

/// What kind of provider surface a snapshot represents.
///
/// [`CarrierApi`](Self::CarrierApi) is the kind whose returned `{carrier}.ts`
/// location maps back onto a carrier source for cross-file rename; the rename
/// capture path ([`ProviderSurfaceStore::capture_current_carrier_api_set`]) is
/// `CarrierApi`-specific by design (a non-`CarrierApi` `Current` path captures as
/// `KnownNonMappable`). The [`CarrierIde`](Self::CarrierIde),
/// [`Shadow`](Self::Shadow), and
/// [`Real`](Self::Real) variants are recordable surfaces with the same
/// generation-stamped / content-addressed identity and the extended
/// owner columns (project owner, `map_hash`, regen key, engine-recheck state); the store is the
/// SINGLE record of all provider content/maps/ownership across every role (no
/// second store). `map_hash` is set on the live path; the project owner / regen key
/// / engine-recheck columns the §2.7 split cache (regeneration skip +
/// dependency-driven engine re-check) reads stay unset until the producer-wiring
/// follow-on. These roles are not part of the `CarrierApi`-only rename-mapping capture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderSurfaceKind {
    /// `{carrier}.tsx` IDE surface (template + script TSX projection) — the
    /// bare-import-probed interactive component identity.
    CarrierIde,
    /// `{carrier}.ts` macro-derived PUBLIC-API surface (the `$props`/`new(props?)`
    /// declaration a cross-file prop rename resolves against).
    CarrierApi,
    /// A self-file shadow / rune-module surface.
    Shadow,
    /// A real, non-carrier source file synced verbatim.
    Real,
}

/// A content-addressed identity for a synced surface's exact content.
///
/// BLAKE3 over the exact bytes synced to the provider. A 256-bit digest is used
/// (not a 64-bit `Hash`) because a collision here could map a rename edit through
/// a DIFFERENT-content source map and corrupt the user's `.vue`; the fail-closed
/// invariant demands a cryptographically strong identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContentHash(blake3::Hash);

impl ContentHash {
    /// Compute the content hash of the exact bytes.
    #[must_use]
    pub fn of(content: &str) -> Self {
        ContentHash(blake3::hash(content.as_bytes()))
    }

    /// The first 16 bytes of the digest as a [`Hash16`] — the env-hash
    /// representation the project-bound contract's `CarrierArtifact` carries. The
    /// full 256-bit digest remains the store's internal fail-closed identity;
    /// this truncation is only for the contract DTO's content-hash field.
    #[must_use]
    pub fn to_hash16(self) -> Hash16 {
        let mut out = [0u8; 16];
        out.copy_from_slice(&self.0.as_bytes()[..16]);
        out
    }
}

/// The generation-stamped identity of a captured provider surface.
///
/// `generation` is a session-monotonic counter advanced on every record (and on
/// every close, so a retired path's generation can never be silently re-used).
/// `(provider_path, generation)` is the exact key an in-flight request pins, and
/// the pinned snapshot — captured under the rename fence — IS the generation the
/// provider's offsets were produced against. Cross-file rename classification maps
/// through THAT captured snapshot's own source map; it never re-checks the stamp
/// against the live store.
///
/// The two content hashes are NOT consulted during rename classification. They
/// back the defense-in-depth diagnostic oracle [`ProviderSurfaceStore::captured_snapshot_still_honored`]
/// (exercised by the store's unit tests, off the classify path): a captured snapshot
/// matches the live current generation only when BOTH sides are identical — the
/// provider `{carrier}.ts` text the offsets index (`content_hash`) AND the carrier
/// `.vue` the source map maps INTO (`source_hash`). The provider text can be
/// byte-identical while the carrier `.vue` changed (e.g. a comment inserted before
/// `<script setup>`, or template text edited — shifts `.vue` byte offsets while
/// leaving the lifted `$props` public-API text identical); comparing on
/// `content_hash` alone would equate two materially different captures, so the oracle
/// requires both.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderSurfaceStamp {
    pub provider_path: Arc<str>,
    pub generation: u64,
    /// BLAKE3 over the exact provider `{carrier}.ts` content the offsets index.
    pub content_hash: ContentHash,
    /// BLAKE3 over the carrier `.vue` source the source map maps INTO. The carrier
    /// half of the diagnostic-oracle identity: a stale carrier source is a distinct
    /// capture even when the provider content is byte-identical.
    pub source_hash: ContentHash,
    /// The `CodeTransform` source-map identity (§2.7). Part of the version-gate
    /// identity: a `map_hash` change invalidates every cached MAPPED result keyed
    /// by the old map. `[0; 16]` when the surface carries no source map.
    pub map_hash: Hash16,
}

/// An immutable, fully self-contained capture of one synced provider surface.
///
/// Holds everything needed to map a returned provider offset back onto the
/// carrier source WITHOUT re-reading the host/VFS or the live `get_public_api()`
/// at merge time, so the mapping is immune to any change that lands after
/// capture. `Arc`-shared for cheap in-flight capture.
///
/// Not `Debug` — `ProviderPositionMapper` (held as `source_map`) is not `Debug`,
/// and a snapshot is an internal mapping artifact, never logged structurally.
pub struct ProviderSurfaceSnapshot {
    pub stamp: ProviderSurfaceStamp,
    pub kind: ProviderSurfaceKind,
    /// The carrier canonical id (`/src/Child.vue`) that owns this surface.
    pub source_canonical: Arc<str>,
    /// The exact provider content synced under `stamp.provider_path`.
    pub provider_content: Arc<str>,
    /// UTF-16 line index over `provider_content` — the source-map's generated
    /// column space.
    pub provider_utf16_line_index: LineIndex,
    /// The source map parsed from the SAME `provider_content` (the bytes the
    /// provider's offsets were produced against). `None` when the surface
    /// carries no map (the mapping then fails closed).
    pub source_map: Option<Arc<ProviderPositionMapper>>,
    /// The carrier `.vue` source captured at record time (from the doc or, for a
    /// CLOSED carrier, from host/VFS). The mapped-into target.
    pub carrier_source: Arc<str>,
    /// UTF-16 line index over `carrier_source` — the source-map's source column
    /// space. The negotiated-encoding re-emission is derived at merge time.
    pub carrier_utf16_line_index: LineIndex,
    /// Content hash of `carrier_source`.
    pub source_hash: ContentHash,
    /// The owning configured project (tsconfig URI) this surface is a member of
    /// — the project-owner column. On the WORKING live record path
    /// (`RecordSurface::carrier_legacy`) this is always `None`; only the
    /// owner-bearing `record_carrier_surface` producer sets it, and that producer is
    /// reserved/unwired until the §2.7 producer-wiring follow-on. The store carries
    /// the column so that, once wired, it is the single record of provider ownership
    /// with no second store.
    pub project_owner: Option<Arc<str>>,
    /// The self-content carrier-regeneration key (§2.7(a)): if unchanged, the
    /// carrier text is byte-stable and need not be regenerated/re-sent. `None`
    /// for surfaces recorded without the producer env dims (legacy `CarrierApi`
    /// path). The regeneration-skip lever, distinct from the engine-recheck
    /// decision below.
    pub regen_key: Option<RegenKey>,
    /// The dependency-driven engine-recheck state (§2.7(b)): the resolved import
    /// signature + dependency-closure generation the surface was last published
    /// under. The engine is re-notified when EITHER advances — NEVER suppressed by
    /// carrier-text stability. `None` for surfaces recorded without dependency
    /// data (legacy `CarrierApi` path).
    pub engine_recheck: Option<EngineRecheckState>,
}

/// Inputs to [`ProviderSurfaceStore::record`] — the data captured for one synced
/// surface, before the store stamps it with a generation.
pub struct RecordSurface {
    pub provider_path: String,
    pub kind: ProviderSurfaceKind,
    pub source_canonical: String,
    pub provider_content: Arc<str>,
    pub source_map: Option<ProviderPositionMapper>,
    pub carrier_source: Arc<str>,
    /// The `CodeTransform` source-map identity (§2.7). `[0; 16]` when the
    /// surface carries no source map. Recorded into the stamp so a `map_hash`
    /// change is a distinct capture.
    pub map_hash: Hash16,
    /// The owning configured project (tsconfig URI), if recorded under a resolved
    /// project binding. `None` preserves the legacy (pre-live-contract) record
    /// path.
    pub project_owner: Option<Arc<str>>,
    /// The self-content regeneration key (§2.7(a)), when the producer env dims are
    /// in scope.
    pub regen_key: Option<RegenKey>,
    /// The dependency-driven engine-recheck state (§2.7(b)), when dependency data
    /// is in scope.
    pub engine_recheck: Option<EngineRecheckState>,
}

impl RecordSurface {
    /// Build a `RecordSurface` for the `CarrierApi` rename-mapping record path —
    /// the surface kind/content/map the existing choke point already captures, with
    /// the owner columns (project owner, regen key, engine-recheck state) left UNSET
    /// (`None`). This is the WORKING live record path; the owner-bearing
    /// `record_carrier_surface` producer that would set those columns has no live
    /// producer and stays reserved until the §2.7 producer-wiring follow-on (the
    /// surface-store carrier-ownership deferral).
    #[must_use]
    pub fn carrier_api_legacy(
        provider_path: String,
        source_canonical: String,
        provider_content: Arc<str>,
        source_map: Option<ProviderPositionMapper>,
        carrier_source: Arc<str>,
    ) -> Self {
        Self::carrier_legacy(
            ProviderSurfaceKind::CarrierApi,
            provider_path,
            source_canonical,
            provider_content,
            source_map,
            carrier_source,
        )
    }

    /// Build a `RecordSurface` for ANY carrier role under the WORKING capture (the
    /// owner columns — project owner, regen key, engine-recheck state — left UNSET
    /// (`None`); the owner-bearing `record_carrier_surface` path that would set them
    /// stays reserved/unwired, the §2.7 producer-wiring follow-on).
    /// Generalises [`carrier_api_legacy`](Self::carrier_api_legacy) over `kind` so
    /// the publish path can record the IDE role (not only the API role) through the
    /// same generation-stamped store — the IDE surface MUST be recorded so its
    /// generation (the plugin's `getScriptVersion`) advances on every content
    /// change instead of staying pinned at the `unwrap_or(1)` fallback.
    #[must_use]
    pub fn carrier_legacy(
        kind: ProviderSurfaceKind,
        provider_path: String,
        source_canonical: String,
        provider_content: Arc<str>,
        source_map: Option<ProviderPositionMapper>,
        carrier_source: Arc<str>,
    ) -> Self {
        Self {
            provider_path,
            kind,
            source_canonical,
            provider_content,
            source_map,
            carrier_source,
            map_hash: [0u8; 16],
            project_owner: None,
            regen_key: None,
            engine_recheck: None,
        }
    }
}

/// One per-path lifecycle state. A known virtual surface is EITHER live
/// ([`Current`](Self::Current), with its active generation) OR closing
/// ([`Closing`](Self::Closing), stamped with the close EPOCH that owns the
/// retire) — never both, never neither. Absent from the lifecycle map ⇒ the path
/// is fully unknown (a genuinely real on-disk file the store never synced).
///
/// `Current` is the former "in the current map"; `Closing` is the former
/// "tombstoned". Folding the two loosely-coupled maps into one per-path state
/// under one lock makes the MONOTONIC-KNOWN invariant trivially atomic — a single
/// map under one lock can never be observed "in neither set".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProviderPathState {
    /// The path is LIVE: `generation` is its active snapshot generation.
    Current { generation: u64 },
    /// The path is CLOSING: its provider close has started under `epoch` but is
    /// not yet confirmed. Only a [`ProviderCloseToken`] carrying this exact epoch
    /// may finalize (clear) it — see [`ProviderSurfaceStore::finalize_close`].
    Closing { epoch: u64 },
}

/// The single per-path lifecycle map plus its session-monotonic event counter,
/// guarded by ONE lock.
#[derive(Default)]
struct Lifecycle {
    /// Session-monotonic counter assigning BOTH record generations and close
    /// epochs from ONE sequence, so every generation/epoch is a unique linearized
    /// event id. Read+incremented UNDER the lifecycle write lock at the SAME
    /// linearization point as the `paths` mutation it stamps (the architect's
    /// load-bearing caveat — assigning before the lock would let an "old
    /// generation committed after a newer forget" reorder survive).
    next_epoch: u64,
    /// Per-path lifecycle state. Present ⇒ known virtual surface (either state);
    /// absent ⇒ fully unknown.
    paths: HashMap<Arc<str>, ProviderPathState>,
}

/// Returned by [`ProviderSurfaceStore::forget`]; the ONLY key that can finalize
/// the close it began. Carries the exact close EPOCH so a stale finalize — whose
/// path was REOPENED (a newer `record` minted a fresh `Current`), or RETIRED
/// AGAIN by a newer close (a fresh `Closing` under a newer epoch) — is a
/// guaranteed no-op rather than an unconditional erase of fresh state.
///
/// `#[must_use]`: a `forget` whose token is dropped on the floor leaves the path
/// `Closing` forever (fail closed), so the caller must consume the token to
/// finalize after a confirmed provider close.
#[must_use]
pub struct ProviderCloseToken {
    provider_path: Arc<str>,
    epoch: u64,
}

/// The authoritative provider-surface store. Shared (`Clone` over inner `Arc`s)
/// across the server and the sync coordinator so EVERY sync/close site records
/// into the same authority.
#[derive(Clone, Default)]
pub struct ProviderSurfaceStore {
    inner: Arc<StoreInner>,
}

#[derive(Default)]
struct StoreInner {
    /// Immutable historical snapshots, keyed by `(provider_path, generation)`.
    snapshots: DashMap<(Arc<str>, u64), Arc<ProviderSurfaceSnapshot>>,
    /// The single per-path lifecycle map (live `Current` / closing `Closing`)
    /// plus the shared generation/epoch counter, under ONE lock. Replaces the
    /// former two loosely-coupled maps (`current` + `tombstones`) and the separate
    /// generation counter: one lock makes every known→known transition observe
    /// atomic, and lets a close be epoch-stamped so a reopen during an older
    /// close's await window can never have its fresh snapshot erased by the stale
    /// finalize.
    lifecycle: RwLock<Lifecycle>,
}

impl ProviderSurfaceStore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a freshly-synced surface under a NEW generation, mark its path
    /// `Current` (reopening it if it was `Closing`), and return the immutable
    /// snapshot.
    ///
    /// Building the UTF-16 line indexes and content hashes here (once, at record
    /// time) keeps the snapshot self-contained: the merge never re-measures the
    /// live content. They are computed BEFORE the lifecycle lock to keep the
    /// critical section short.
    ///
    /// LINEARIZATION: the generation is read+assigned from `next_epoch` INSIDE the
    /// `lifecycle.write()` section — at the SAME linearization point as the
    /// `paths` mutation — so an "old generation committed after a newer forget"
    /// reorder cannot survive (assigning the generation before the lock would let
    /// it). The snapshot's `stamp.generation` is the value assigned under the lock.
    ///
    /// MONOTONIC-KNOWN: setting `Current` here overwrites any prior `Closing`, so a
    /// path being re-synced from a closing state transitions `Closing → Current`
    /// under ONE lock — [`Self::is_known_virtual_surface`] is observably `true` at
    /// every instant (the single map can never be observed "in neither set").
    pub fn record(&self, surface: RecordSurface) -> Arc<ProviderSurfaceSnapshot> {
        let provider_path: Arc<str> = Arc::from(surface.provider_path.as_str());

        // Compute the expensive derived data OUTSIDE the lifecycle lock.
        let provider_utf16_line_index = LineIndex::new_utf16(&surface.provider_content);
        let carrier_utf16_line_index = LineIndex::new_utf16(&surface.carrier_source);
        let content_hash = ContentHash::of(&surface.provider_content);
        let source_hash = ContentHash::of(&surface.carrier_source);
        let source_map = surface.source_map.map(Arc::new);
        let source_canonical: Arc<str> = Arc::from(surface.source_canonical.as_str());

        let mut lifecycle = self.inner.lifecycle.write();
        // Assign the generation at the SAME linearization point as the state
        // mutation (see LINEARIZATION above).
        let generation = lifecycle.next_epoch;
        lifecycle.next_epoch += 1;

        let snapshot = Arc::new(ProviderSurfaceSnapshot {
            stamp: ProviderSurfaceStamp {
                provider_path: Arc::clone(&provider_path),
                generation,
                content_hash,
                source_hash,
                map_hash: surface.map_hash,
            },
            kind: surface.kind,
            source_canonical,
            provider_content: surface.provider_content,
            provider_utf16_line_index,
            source_map,
            carrier_source: surface.carrier_source,
            carrier_utf16_line_index,
            source_hash,
            project_owner: surface.project_owner,
            regen_key: surface.regen_key,
            engine_recheck: surface.engine_recheck,
        });

        // Publish the snapshot BEFORE pointing the lifecycle state at the new
        // generation so a reader observing `Current { generation }` can always
        // resolve the snapshot.
        self.inner.snapshots.insert(
            (Arc::clone(&provider_path), generation),
            Arc::clone(&snapshot),
        );
        // A fresh sync re-activates the path: `Current` overwrites any prior
        // `Closing` (reopen) or `Current` (re-sync).
        lifecycle
            .paths
            .insert(provider_path, ProviderPathState::Current { generation });
        drop(lifecycle);
        snapshot
    }

    /// Retire the ACTIVE generation for a provider path (its surface is CLOSING)
    /// under a fresh close EPOCH, marking the path a known-but-unsafe virtual API
    /// surface until its provider close is confirmed, and return the
    /// [`ProviderCloseToken`] that owns this close.
    ///
    /// IDEMPOTENT for an already-`Closing` path: if the path is already `Closing`,
    /// REUSES that in-flight close's existing epoch (returns a token for it) instead
    /// of minting a fresh epoch and overwriting the state; a fresh epoch is minted
    /// ONLY when transitioning from `Current` or from ABSENT. This makes a DUPLICATE
    /// close of an already-retired surface (two close drivers `forget` the same path
    /// with no intervening `record`) terminate cleanly: both closers hold a token for
    /// the SAME epoch, so whichever close confirms `Ok` first finalizes the matching
    /// `Closing` and clears it — the path can never be stranded `Closing` forever
    /// under an epoch whose only owner's close errored. Historical snapshots are
    /// PRESERVED untouched — an in-flight request that captured the prior generation
    /// keeps mapping correctly. ALWAYS returns a token (even when the path was absent
    /// from the map — a close of an untracked path conservatively becomes
    /// known-virtual = fail-closed).
    ///
    /// `Closing` is the fail-closed half of the close lifecycle: retiring the
    /// current snapshot BEFORE the provider close means a cross-file rename racing
    /// the close finds the path absent from its capture, and the provider close can
    /// FAIL (or its notification be dropped) leaving tsserver LIVE for the virtual
    /// path. `Closing` makes [`Self::is_known_virtual_surface`] keep returning
    /// `true`, so the rename classifies the absent path `VirtualDrop` (drop) rather
    /// than editing a same-named real file with virtual offsets. The `Closing`
    /// state clears ONLY via [`Self::finalize_close`] passed THIS token after a
    /// SUCCESSFUL provider close.
    ///
    /// LINEARIZATION: the epoch is read (and, when minting, assigned) from
    /// `next_epoch` INSIDE the `lifecycle.write()` section — at the SAME
    /// linearization point as the state mutation — so the close epoch and any racing
    /// `record` generation are totally ordered. A freshly-minted epoch (≥ every prior
    /// generation/epoch) means the retired path can never be re-stamped with a
    /// generation a captured snapshot already references; the idempotent reuse branch
    /// keeps the EXISTING in-flight close epoch (which already satisfies that
    /// property), so it does not mint.
    ///
    /// MONOTONIC-KNOWN: this `Current → Closing` (or `Closing → Closing` idempotent)
    /// transition keeps [`Self::is_known_virtual_surface`] observably `true` at every
    /// instant — the single lifecycle map under one lock is never observed "in
    /// neither set".
    pub fn forget(&self, provider_path: &str) -> ProviderCloseToken {
        let path: Arc<str> = Arc::from(provider_path);
        let mut lifecycle = self.inner.lifecycle.write();
        // Read/assign the epoch at the SAME linearization point as the state mutation
        // (see LINEARIZATION above).
        let epoch = match lifecycle.paths.get(&path) {
            // Already closing: REUSE the in-flight close's epoch (idempotent) — do NOT
            // mint a fresh epoch and do NOT overwrite, so a DUPLICATE close of an
            // already-retired surface cannot strand the path in Closing under an epoch
            // whose only owner's close errored. Both duplicate closers thus hold a
            // token for the SAME epoch.
            Some(ProviderPathState::Closing { epoch }) => *epoch,
            // Current or absent: mint a FRESH epoch and (re)enter Closing.
            _ => {
                let minted = lifecycle.next_epoch;
                lifecycle.next_epoch += 1;
                minted
            }
        };
        lifecycle
            .paths
            .insert(Arc::clone(&path), ProviderPathState::Closing { epoch });
        drop(lifecycle);
        ProviderCloseToken {
            provider_path: path,
            epoch,
        }
    }

    /// Finalize a retired path's close after a SUCCESSFUL provider close, using the
    /// [`ProviderCloseToken`] returned by the [`Self::forget`] that began it: clear
    /// the path's `Closing` state IFF it is STILL `Closing` under the token's exact
    /// epoch, so the path is no longer a known virtual surface (a genuinely real
    /// same-named file then classifies `NotVirtual` and is edited in place).
    /// Returns `true` iff it cleared.
    ///
    /// EPOCH-SCOPED no-op (the core correctness property): if the path was REOPENED
    /// during this close's await window — a newer `record` minted a fresh `Current`
    /// — or RETIRED AGAIN by a newer close — a fresh `Closing` under a newer epoch —
    /// the state no longer matches the token's epoch, so this is a guaranteed NO-OP.
    /// It NEVER removes a `Current` (a fresh reopened snapshot is preserved) and
    /// NEVER clears a `Closing` of a different epoch (a newer close owns that
    /// retire). The bare unconditional clear it replaces could erase a fresh reopen.
    ///
    /// Called ONLY when the provider's `close_dts` returned `Ok`. On a close ERROR
    /// the caller does NOT finalize (drops the token), so the `Closing` state
    /// persists and the path keeps classifying `VirtualDrop` — the fail-closed
    /// choice for a path whose provider surface may still be live.
    ///
    /// This is the ONLY legitimate transition to fully-unknown. It runs under the
    /// one lifecycle lock, so a concurrent reader sees the path either fully known
    /// (before) or fully unknown (after), never a skew.
    pub fn finalize_close(&self, token: ProviderCloseToken) -> bool {
        let mut lifecycle = self.inner.lifecycle.write();
        match lifecycle.paths.get(&token.provider_path) {
            Some(ProviderPathState::Closing { epoch }) if *epoch == token.epoch => {
                lifecycle.paths.remove(&token.provider_path);
                true
            }
            // Reopened (now `Current`), retired again by a newer close (`Closing`
            // under a different epoch), or already finalized (absent): the stale
            // finalize is a no-op — it must never erase fresh state.
            _ => false,
        }
    }

    /// Whether the store positively knows `provider_path` to be a virtual API
    /// surface: its lifecycle state is present in EITHER form — `Current` (still
    /// synced) OR `Closing` (retired, close not yet confirmed). Distinguishes a
    /// path the store is responsible for as a virtual surface from a
    /// genuinely-unknown path (a real on-disk file the store never synced, absent
    /// from the map).
    ///
    /// The cross-file rename resolver consults this for a path ABSENT from its
    /// in-flight capture: known ⇒ `VirtualDrop` (the provider may still be live for
    /// the virtual surface; never edit a real file with virtual offsets), unknown ⇒
    /// `NotVirtual` (edit its own real file in place).
    ///
    /// MONOTONIC-KNOWN (concurrency contract): the single lifecycle map under one
    /// lock makes the read atomic — a `present` check over one map can never
    /// observe the path "in neither set". [`Self::record`] (→ `Current`) and
    /// [`Self::forget`] (→ `Closing`) each replace one present state with another
    /// under the same lock, so a path that is virtual BEFORE and AFTER a transition
    /// is present throughout; the reader observes `true` and can never catch an
    /// in-neither-set window that would mis-route a captured-miss rename to a real
    /// same-named file. (The only transition to fully-unknown is the matching-epoch
    /// branch of [`Self::finalize_close`].)
    #[must_use]
    pub fn is_known_virtual_surface(&self, provider_path: &str) -> bool {
        self.inner
            .lifecycle
            .read()
            .paths
            .contains_key(provider_path)
    }

    /// The CURRENT active snapshot for a provider path, if one is synced (its
    /// lifecycle state is `Current`). A `Closing` or absent path resolves to
    /// `None`. Used to CAPTURE the in-flight pinned set.
    #[must_use]
    pub fn current_snapshot(&self, provider_path: &str) -> Option<Arc<ProviderSurfaceSnapshot>> {
        let generation = match self.inner.lifecycle.read().paths.get(provider_path) {
            Some(ProviderPathState::Current { generation }) => *generation,
            _ => return None,
        };
        self.inner
            .snapshots
            .get(&(Arc::from(provider_path), generation))
            .map(|e| Arc::clone(e.value()))
    }

    /// Whether a previously-captured snapshot still agrees with the path's CURRENT
    /// live state. A defense-in-depth / diagnostic oracle that is NOT on the rename
    /// classify path: [`classify_captured_api_surface`] reads ONLY the captured
    /// [`ProviderQuerySnapshot`] and performs ZERO live-store reads, so it never calls
    /// this. The captured snapshot, pinned under the rename fence, already IS the
    /// generation the offsets were produced against; re-checking it against live state
    /// would reintroduce the very TOCTOU the snapshot-only classify closes. This oracle
    /// is exercised directly by the store's unit tests (which characterize the
    /// generation / content-hash agreement rules below).
    ///
    /// Agrees when the captured path still has a current generation AND either
    /// (a) that current generation EQUALS the captured one, OR (b) ALL THREE
    /// identities match — the current provider `{carrier}.ts` `content_hash`
    /// EQUALS the captured one, the current carrier `.vue` `source_hash` EQUALS
    /// the captured one, AND the current `map_hash` EQUALS the captured one.
    /// Branch (b) captures the byte-IDENTICAL background re-sync case — a fresh
    /// generation for the same bytes and the same mapping — where the captured
    /// offsets would still map correctly. The generation-match arm (a) is
    /// inherently exact (same recorded surface) and needs no per-field compare.
    ///
    /// The carrier `source_hash` is load-bearing, not redundant: the provider
    /// `{carrier}.ts` text can be byte-identical across two generations while the
    /// carrier `.vue` source CHANGED (a comment inserted before `<script setup>`, or
    /// template text edited — shifts `.vue` byte offsets while leaving the lifted
    /// `$props` public-API text identical). Comparing on `content_hash` alone would
    /// then equate the OLD carrier source map with the NEW live `.vue`.
    ///
    /// The `map_hash` is equally load-bearing on arm (b): a map-only re-sync
    /// (same provider bytes, same carrier source, CHANGED mapping) must NOT keep
    /// honoring the captured snapshot — a result mapped through the superseded
    /// mapper would be WRONG, not stale. A path with no current snapshot
    /// (closed/forgotten), a differing provider content, a differing carrier
    /// source, OR a differing map identity does NOT agree.
    #[must_use]
    pub fn captured_snapshot_still_honored(&self, captured: &ProviderSurfaceSnapshot) -> bool {
        let Some(current) = self.current_snapshot(&captured.stamp.provider_path) else {
            return false;
        };
        current.stamp.generation == captured.stamp.generation
            || (current.stamp.content_hash == captured.stamp.content_hash
                && current.stamp.source_hash == captured.stamp.source_hash
                && current.stamp.map_hash == captured.stamp.map_hash)
    }

    /// The owning configured project (tsconfig URI) of `provider_path`'s CURRENT
    /// surface — the project-owner column. `None` when the path has no current
    /// snapshot, or its surface was recorded outside a resolved project binding (on
    /// the working live path, always — the owner column is unset until the §2.7
    /// producer-wiring follow-on). The store is the SINGLE record of provider
    /// ownership; the (reserved/unwired) owner-bound path reads this accessor rather
    /// than a second ownership map.
    #[must_use]
    pub fn project_owner_of(&self, provider_path: &str) -> Option<Arc<str>> {
        self.current_snapshot(provider_path)
            .and_then(|s| s.project_owner.clone())
    }

    /// Every CURRENT (`Current`-state) provider path whose surface is owned by the
    /// configured project `project` (its recorded `project_owner` equals
    /// `project`). The store is the SINGLE record of provider ownership, so this is
    /// the authoritative project-scoped surface set the (reserved/unwired)
    /// owner-bound sync layer will capture BEFORE a request so a multi-file result
    /// can be validated against every project surface, not only the queried file (no
    /// second ownership map). Dormant today: with the owner column unset on the live
    /// path it returns empty until the §2.7 producer-wiring follow-on lands.
    ///
    /// ATOMICITY: the lifecycle read guard is held for the whole scan, and each
    /// `Current` path's snapshot is resolved UNDER THAT SAME GUARD (sound because
    /// [`Self::record`] publishes the snapshot into `snapshots` BEFORE pointing the
    /// lifecycle state at its generation). A `Closing` path is excluded (it has no
    /// current snapshot). Result order is unspecified; callers compare by set
    /// membership, not order.
    #[must_use]
    pub fn current_project_surface_paths(&self, project: &str) -> Vec<Arc<str>> {
        let lifecycle = self.inner.lifecycle.read();
        let mut out: Vec<Arc<str>> = Vec::new();
        for (path, state) in lifecycle.paths.iter() {
            let ProviderPathState::Current { generation } = state else {
                continue;
            };
            if let Some(entry) = self.inner.snapshots.get(&(Arc::clone(path), *generation)) {
                if entry
                    .value()
                    .project_owner
                    .as_deref()
                    .is_some_and(|owner| owner == project)
                {
                    out.push(Arc::clone(path));
                }
            }
        }
        out
    }

    /// The CURRENT surface's `map_hash` for `provider_path`, or `None` if no
    /// current snapshot OR the current surface carries no usable source map. The
    /// mapped-result-cache identity (§2.7): a returned span mapped through a map
    /// whose hash no longer matches the current surface must be dropped.
    ///
    /// FAIL CLOSED on a surface with no parsed source map: a snapshot whose
    /// `source_map` is `None` (the map JSON was absent or failed to parse) has NO
    /// usable mapper, so there is no map identity any cached mapped result could
    /// be valid against — return `None` rather than a (possibly zero or stale)
    /// `map_hash` that could falsely validate a mapped result against a missing
    /// map.
    #[must_use]
    pub fn current_map_hash(&self, provider_path: &str) -> Option<Hash16> {
        self.current_snapshot(provider_path).and_then(|s| {
            // No usable mapper ⇒ no map identity to validate against (fail closed).
            s.source_map.as_ref()?;
            Some(s.stamp.map_hash)
        })
    }

    /// Whether mapped results previously produced for `provider_path` under
    /// `cached_map_hash` are still valid against the CURRENT surface's map
    /// (§2.7). `false` (drop) when the path has no current snapshot or the
    /// current `map_hash` differs — never remap a stale diagnostic through a new
    /// map.
    #[must_use]
    pub fn mapped_results_valid(&self, provider_path: &str, cached_map_hash: Hash16) -> bool {
        self.current_map_hash(provider_path)
            .is_some_and(|live| crate::carrier_cache::mapped_results_valid(cached_map_hash, live))
    }

    /// Whether the carrier text for `provider_path` is regeneration-fresh against
    /// `live` self-content env dims (§2.7(a)): `true` ⇒ reuse the cached carrier,
    /// no re-codegen / re-send. `false` when the path has no current snapshot, the
    /// current surface carries no regen key (legacy record), or any self-content
    /// dimension changed. This is the (a) lever ONLY — it does NOT assert the
    /// engine result is still valid (see [`Self::carrier_needs_engine_recheck`]).
    #[must_use]
    pub fn carrier_regeneration_is_fresh(&self, provider_path: &str, live: &RegenKey) -> bool {
        self.current_snapshot(provider_path)
            .and_then(|s| s.regen_key)
            .is_some_and(|cached| RegenKey::carrier_regeneration_is_fresh(&cached, live))
    }

    /// Whether the engine MUST be re-notified to re-check `provider_path` given
    /// the `live` dependency-driven recheck state (§2.7(b)). Returns `true` when
    /// EITHER the resolved import signature changed OR the dependency-closure
    /// generation advanced — NEVER suppressed by carrier-text stability. A path
    /// with no current snapshot, or whose current surface carries no recheck state
    /// (legacy record), conservatively returns `true` (re-check rather than risk a
    /// stale result — fail toward correctness, the no-suppress invariant).
    #[must_use]
    pub fn carrier_needs_engine_recheck(
        &self,
        provider_path: &str,
        live: &EngineRecheckState,
    ) -> bool {
        match self
            .current_snapshot(provider_path)
            .and_then(|s| s.engine_recheck)
        {
            Some(cached) => crate::carrier_cache::needs_engine_recheck(&cached, live),
            // No recorded recheck state ⇒ we cannot prove the dependent is fresh
            // ⇒ re-check (never suppress an engine re-check the design requires).
            None => true,
        }
    }

    /// The exact historical snapshot for `(provider_path, generation)`, if it was
    /// ever recorded. Used at MERGE time to interpret a returned offset against
    /// the precise generation the pinned request captured — independent of any
    /// later sync or close.
    #[must_use]
    pub fn snapshot_at(
        &self,
        provider_path: &str,
        generation: u64,
    ) -> Option<Arc<ProviderSurfaceSnapshot>> {
        self.inner
            .snapshots
            .get(&(Arc::from(provider_path), generation))
            .map(|e| Arc::clone(e.value()))
    }

    /// Capture EVERY tracked path's lifecycle state — the immutable in-flight
    /// pinned set a cross-file rename holds across its provider query, and the SOLE
    /// authority [`classify_captured_api_surface`] routes on (it never reads the
    /// live store afterward).
    ///
    /// COMPLETENESS (condition b): the returned set distinguishes the three states
    /// classify needs, so it can classify + map WITHOUT any later live read:
    /// - a `Current` `CarrierApi` path → [`CapturedPathState::Current`] with its
    ///   full immutable snapshot (maps onto the `.vue` or, if it has no source map,
    ///   drops);
    /// - a `Closing` path, OR a `Current` path that is not `CarrierApi`, OR a
    ///   `Current` path whose snapshot Arc is somehow absent → captured as
    ///   [`CapturedPathState::KnownNonMappable`] (known-virtual-but-not-mappable →
    ///   `VirtualDrop`, never `NotVirtual`). A `Closing` path is INCLUDED here (the
    ///   prior capture SKIPPED it, forcing a live re-consult — the third TOCTOU);
    /// - a path ABSENT from the map → absent from the capture too (a genuinely real
    ///   file → `NotVirtual`).
    ///
    /// ATOMICITY (condition a): the whole capture runs under ONE `lifecycle.read()`
    /// guard held for the entire loop — no drop-and-re-acquire, no second lock. The
    /// `snapshots` DashMap lookup for a `Current` path's generation is performed
    /// WHILE STILL HOLDING that guard, so the captured `(state, snapshot)` pair
    /// cannot be torn by a concurrent `record`/`forget`/`finalize_close`. This is
    /// sound because [`Self::record`] PUBLISHES the snapshot into `snapshots` BEFORE
    /// pointing the lifecycle state at its generation: holding the lifecycle read
    /// guard and then reading `snapshots` for the generation the guard observed
    /// always yields a consistent pair (the snapshot for the observed generation is
    /// already present). A concurrent writer is blocked on the lifecycle write lock
    /// for the whole loop.
    ///
    /// Because every captured value is immutable (an `Arc` snapshot, or a state tag),
    /// a concurrent background sync that advances a path's generation, or a close
    /// that retires/finalizes it, AFTER this capture can never change what a captured
    /// entry resolves to — the no-race property the third-TOCTOU fix completes.
    #[must_use]
    pub fn capture_current_carrier_api_set(&self) -> ProviderQuerySnapshot {
        self.capture_current_set_of_kind(ProviderSurfaceKind::CarrierApi)
    }

    /// Capture EVERY tracked path's lifecycle state with `CarrierIde` snapshots
    /// as the mappable role — the immutable in-flight pinned set a navigation
    /// handler holds across its provider query so a returned FOREIGN carrier
    /// IDE location maps through the surface captured when the request began,
    /// never whatever surface is current at merge time. Same atomicity and
    /// completeness contract as [`Self::capture_current_carrier_api_set`].
    #[must_use]
    pub fn capture_current_carrier_ide_set(&self) -> ProviderQuerySnapshot {
        self.capture_current_set_of_kind(ProviderSurfaceKind::CarrierIde)
    }

    fn capture_current_set_of_kind(&self, kind: ProviderSurfaceKind) -> ProviderQuerySnapshot {
        // ONE read guard for the ENTIRE capture (atomicity, condition a).
        let lifecycle = self.inner.lifecycle.read();
        let mut by_path: HashMap<Arc<str>, CapturedPathState> =
            HashMap::with_capacity(lifecycle.paths.len());
        for (path, state) in lifecycle.paths.iter() {
            let captured = match state {
                // `Closing` (retired-but-known): no mappable snapshot, but the store
                // knew it as virtual → KnownNonMappable so it drops, never falls
                // through to NotVirtual. (This is the path the OLD capture skipped.)
                ProviderPathState::Closing { .. } => CapturedPathState::KnownNonMappable,
                // `Current`: resolve its snapshot Arc UNDER THIS SAME READ GUARD so
                // the (state, snapshot) pair is consistent and cannot tear (see
                // ATOMICITY above). A snapshot of the requested role is mappable;
                // anything else (a different role, or a missing snapshot) is
                // known-virtual but not mappable → drop.
                ProviderPathState::Current { generation } => {
                    match self.inner.snapshots.get(&(Arc::clone(path), *generation)) {
                        Some(entry) if entry.value().kind == kind => {
                            CapturedPathState::Current(Arc::clone(entry.value()))
                        }
                        _ => CapturedPathState::KnownNonMappable,
                    }
                }
            };
            by_path.insert(Arc::clone(path), captured);
        }
        ProviderQuerySnapshot { by_path }
    }

    /// Whether `provider_path` is CURRENTLY synced (lifecycle state `Current`).
    /// Diagnostics / tests only; the mapping path goes through the captured
    /// snapshot, never a live tracked-check.
    #[cfg(test)]
    #[must_use]
    pub fn is_tracked(&self, provider_path: &str) -> bool {
        matches!(
            self.inner.lifecycle.read().paths.get(provider_path),
            Some(ProviderPathState::Current { .. })
        )
    }

    /// Whether `provider_path` is currently CLOSING (retired, close not yet
    /// finalized — lifecycle state `Closing`). Tests only — production consults
    /// `is_known_virtual_surface`.
    #[cfg(test)]
    #[must_use]
    pub fn is_tombstoned(&self, provider_path: &str) -> bool {
        matches!(
            self.inner.lifecycle.read().paths.get(provider_path),
            Some(ProviderPathState::Closing { .. })
        )
    }
}

/// The per-path lifecycle state CAPTURED at the rename fence — the SOLE input to
/// [`classify_captured_api_surface`]. Captured atomically under one `lifecycle`
/// read guard so it is a consistent point-in-time snapshot of the store's view of
/// every path, immune to any live mutation after capture.
///
/// Three cases are explicit; a path ABSENT from [`ProviderQuerySnapshot::by_path`]
/// is the fourth (a genuinely real on-disk file the store did not know as virtual
/// at capture → `NotVirtual`, edit in place).
pub enum CapturedPathState {
    /// A `Current` `CarrierApi` path with its full immutable snapshot — the only
    /// case that can map a returned `{carrier}.ts` offset onto the `.vue`. The
    /// merge maps ONLY through this captured generation's own source map.
    Current(Arc<ProviderSurfaceSnapshot>),
    /// A path the store KNEW as virtual at capture but for which there is NO
    /// mappable snapshot: it was `Closing` (a close in flight), or `Current` but
    /// not a `CarrierApi` surface, or its snapshot Arc was somehow absent. Its
    /// offsets index VIRTUAL content, so it MUST classify `VirtualDrop` (fail
    /// closed) — never fall through to the real-file branch and edit a same-named
    /// real file with virtual offsets.
    KnownNonMappable,
}

/// An immutable, point-in-time capture of EVERY tracked path's lifecycle state,
/// pinned by a cross-file rename across its provider query.
///
/// This captured set is the SOLE merge authority: [`classify_captured_api_surface`]
/// resolves every returned provider location by looking its path up HERE, never the
/// live store. A path captured [`CapturedPathState::Current`] maps through that
/// captured generation's own snapshot; a path captured
/// [`CapturedPathState::KnownNonMappable`] drops; a path ABSENT from the capture is
/// a genuinely real on-disk file (edited in place). A path whose live generation or
/// lifecycle later changes is irrelevant: this capture is the state the provider's
/// offsets were produced against.
#[derive(Default)]
pub struct ProviderQuerySnapshot {
    by_path: HashMap<Arc<str>, CapturedPathState>,
}

impl ProviderQuerySnapshot {
    /// The captured per-path lifecycle state for `provider_path`, or `None` if the
    /// path was ABSENT from the capture (the store did not know it as virtual at
    /// capture → a genuinely real file). This is the lookup classify routes on.
    #[must_use]
    pub fn captured_state_for(&self, provider_path: &str) -> Option<&CapturedPathState> {
        self.by_path.get(provider_path)
    }

    /// The captured MAPPABLE snapshot for `provider_path`, if it was a `Current`
    /// `CarrierApi` surface at capture time. Returns `None` for a
    /// [`CapturedPathState::KnownNonMappable`] path (e.g. `Closing` at capture) and
    /// for an absent path alike — only a path with a mappable snapshot can vouch a
    /// `.vue` edit.
    #[must_use]
    pub fn snapshot_for(&self, provider_path: &str) -> Option<&Arc<ProviderSurfaceSnapshot>> {
        match self.by_path.get(provider_path) {
            Some(CapturedPathState::Current(snapshot)) => Some(snapshot),
            _ => None,
        }
    }

    /// Whether the captured set is empty (no tracked path at all — neither a
    /// mappable `Current` surface nor a known-non-mappable one).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_path.is_empty()
    }
}

mod producers;
pub use producers::*;

#[cfg(test)]
#[path = "../provider_surface_store_tests.rs"]
mod tests;

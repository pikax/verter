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

use verter_session::VerterHost;

use crate::documents::line_index::LineIndex;
use crate::documents::position_map::PositionMapper;
use crate::documents::provider_projection::ProviderPositionMapper;
use crate::documents::DocumentRegistry;

/// What kind of provider surface a snapshot represents.
///
/// Only [`ProviderSurfaceKind::CarrierApi`] surfaces map a returned `{carrier}.ts`
/// location back onto a `.vue` carrier, and `CarrierApi` is the ONLY kind
/// production currently records/vouches (every record choke point synthesises a
/// `CarrierApi` snapshot). The [`CarrierIde`](Self::CarrierIde),
/// [`Shadow`](Self::Shadow), and [`Real`](Self::Real) variants are reserved for a
/// future extension of the store to the full set of synced virtual paths; they are
/// NOT yet wired to any record site, so the store is the complete authority over
/// `CarrierApi` surfaces specifically (a captured snapshot is always `CarrierApi`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderSurfaceKind {
    /// `{carrier}.tsx` IDE surface (template + script TSX projection).
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
            },
            kind: surface.kind,
            source_canonical,
            provider_content: surface.provider_content,
            provider_utf16_line_index,
            source_map,
            carrier_source: surface.carrier_source,
            carrier_utf16_line_index,
            source_hash,
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
    /// (a) that current generation EQUALS the captured one, OR (b) BOTH content
    /// identities match — the current provider `{carrier}.ts` `content_hash` EQUALS
    /// the captured one AND the current carrier `.vue` `source_hash` EQUALS the
    /// captured one. Branch (b) captures the byte-IDENTICAL background re-sync case —
    /// a fresh generation for the same bytes — where identical provider content over
    /// an identical carrier source means an identical source map AND an identical map
    /// target, so the captured offsets would still map correctly.
    ///
    /// The carrier `source_hash` is load-bearing, not redundant: the provider
    /// `{carrier}.ts` text can be byte-identical across two generations while the
    /// carrier `.vue` source CHANGED (a comment inserted before `<script setup>`, or
    /// template text edited — shifts `.vue` byte offsets while leaving the lifted
    /// `$props` public-API text identical). Comparing on `content_hash` alone would
    /// then equate the OLD carrier source map with the NEW live `.vue`. A path with no
    /// current snapshot (closed/forgotten), a differing provider content, OR a
    /// differing carrier source does NOT agree.
    #[must_use]
    pub fn captured_snapshot_still_honored(&self, captured: &ProviderSurfaceSnapshot) -> bool {
        let Some(current) = self.current_snapshot(&captured.stamp.provider_path) else {
            return false;
        };
        current.stamp.generation == captured.stamp.generation
            || (current.stamp.content_hash == captured.stamp.content_hash
                && current.stamp.source_hash == captured.stamp.source_hash)
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
                // ATOMICITY above). A `CarrierApi` snapshot is mappable; anything
                // else (non-CarrierApi kind, or a missing snapshot) is known-virtual
                // but not mappable → drop.
                ProviderPathState::Current { generation } => {
                    match self.inner.snapshots.get(&(Arc::clone(path), *generation)) {
                        Some(entry) if entry.value().kind == ProviderSurfaceKind::CarrierApi => {
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

/// Build the merge-time [`ExternalIdeContext`](crate::type_provider::merge::ExternalIdeContext)
/// for a carrier PUBLIC-API surface from a PINNED, immutable snapshot — the
/// fail-closed bridge behind cross-file rename, anchored to the EXACT generation
/// the provider's offsets were produced against.
///
/// EVERYTHING comes from the snapshot, never a live `get_public_api()` / open
/// document: the provider (API) UTF-16 line index, the source map (parsed from
/// the SAME bytes), and the carrier `.vue` UTF-16 index. The carrier source —
/// captured at sync time (open buffer or host/VFS for a CLOSED carrier) — is
/// re-measured in the NEGOTIATED encoding so the merge re-emits the mapped
/// UTF-16 carrier range in that encoding. A snapshot with no source map fails
/// closed (`None`).
#[must_use]
pub fn external_ide_context_from_snapshot(
    snapshot: &ProviderSurfaceSnapshot,
    negotiated_encoding: tower_lsp_server::ls_types::PositionEncodingKind,
) -> Option<crate::type_provider::merge::ExternalIdeContext> {
    let mapper = snapshot.source_map.as_ref()?;
    let carrier_negotiated_line_index =
        LineIndex::new(&snapshot.carrier_source, negotiated_encoding);
    Some(crate::type_provider::merge::ExternalIdeContext {
        tsx_line_index: snapshot.provider_utf16_line_index.clone(),
        mapper: (**mapper).clone(),
        carrier_line_index: snapshot.carrier_utf16_line_index.clone(),
        carrier_negotiated_line_index: Some(carrier_negotiated_line_index),
    })
}

/// Locate the byte range of a child component's prop identifier IN the captured
/// `{carrier}.ts` PUBLIC-API content, keyed by the prop's TYPED `.vue` declaration
/// identity (its `.vue` decl span + name) — never a text scan of the API content.
///
/// This is the one piece a provider-agnostic cross-file Vue-prop rename needs that
/// a provider's `textDocument/rename` may not itself enumerate (tgo does not): the
/// child-declaration rename leg, synthesized by Verter as a [`RenameLocation`] whose
/// `start..end` is the prop name's byte range in the SAME captured API content the
/// merge maps through. The merge then maps that range back onto the `.vue` via the
/// snapshot's own source map — byte-identically to how it maps a provider's real
/// carrier location — so the result dedups against the provider's location by the
/// final `.vue` range.
///
/// IDENTITY-DRIVEN, not text-scanned: the API generator emits the prop name through
/// the SAME `push_mapped(name, vue_decl_span)` token that seeds this snapshot's
/// source map, so querying the map for the prop's `.vue` decl-span START yields the
/// API position the generator wrote the name at. The byte range is then
/// `[start, start + name.len())` because the generator writes the name VERBATIM
/// (a position-preserving run), so the API slice equals the name exactly.
///
/// FAIL CLOSED (`None`) when any of:
/// - the snapshot carries no source map (an unmappable surface),
/// - the prop's `.vue` decl-span start does not resolve to a `.vue` position, or
///   the map does not map that `.vue` position into the API content (the prop name
///   was not emitted with a mapped token — e.g. a `defineProps<ImportedType>()`
///   surface whose props are a bare type ref, not inline members),
/// - the resolved API position does not convert to a byte offset, or
/// - the API slice at the resolved range is NOT byte-equal to the prop name (the
///   correctness tripwire: a wrong/mis-ranged mapping must never emit an edit that
///   could corrupt the `.vue`).
///
/// A `None` here means the caller must NOT synthesize the child-declaration leg
/// (and, per the fail-closed rename ruling, must not ship a usage-only partial).
#[must_use]
pub fn locate_prop_decl_range_in_carrier_api(
    snapshot: &ProviderSurfaceSnapshot,
    prop_decl_span: verter_span::Span,
    prop_name: &str,
) -> Option<(u32, u32)> {
    use tower_lsp_server::ls_types::Position;
    use verter_span::LspPosition;

    // The map is parsed from the SAME bytes as `provider_content`; no map ⇒ no
    // identity-keyed lookup is possible ⇒ fail closed.
    let mapper = snapshot.source_map.as_ref()?;

    // The prop's `.vue` decl span is a file-absolute `.vue` byte span (the same
    // span analysis hands to `location_from_span`). Convert its START to a `.vue`
    // UTF-16 position — the source map's source column space.
    let vue_pos = snapshot
        .carrier_utf16_line_index
        .offset_to_position(prop_decl_span.start)?;

    // Map the `.vue` decl-span start INTO the API content (source → generated).
    // This is the inverse of the merge's API→`.vue` hop and lands on the API
    // position the generator wrote the prop name at (strict in-run lookup; a `.vue`
    // position the map does not cover returns `None` ⇒ fail closed).
    let api_pos = mapper
        .carrier_to_tsx(LspPosition::new(vue_pos.line, vue_pos.character))?
        .pos;

    // The API UTF-16 position → API byte offset (the `RenameLocation` coordinate
    // space, encoding-neutral bytes the merge re-derives positions from).
    let start = snapshot
        .provider_utf16_line_index
        .position_to_offset(&Position {
            line: api_pos.line,
            character: api_pos.character,
        })?;
    // The generator writes the name verbatim, so the API byte length equals the
    // name's byte length. (`end` is exclusive.)
    let end = start + prop_name.len() as u32;

    // Correctness tripwire — fail closed, NOT the lookup mechanism. The lookup is
    // the structured-offset hop above; this only VALIDATES that the resolved range
    // actually spells the prop name in the API content. A mismatch (mis-ranged
    // mapping, or a caller name that does not match what the resolved range spells)
    // must never emit an edit that could corrupt the `.vue` → fail closed.
    let slice = snapshot
        .provider_content
        .get(start as usize..end as usize)?;
    if slice != prop_name {
        return None;
    }

    Some((start, end))
}

/// Classify a returned carrier PUBLIC-API path into the fail-closed 3-state
/// [`ApiSurfaceResolution`](crate::type_provider::merge::ApiSurfaceResolution) — the
/// SINGLE authority the cross-file rename merge routes on. The production rename
/// closure is a thin adapter over this; pinning the policy here keeps the decision
/// testable without a live provider and prevents a second, divergent classifier.
///
/// ZERO-LIVE-READ INVARIANT (the class-closing property): every decision is read
/// from the CAPTURED snapshot ([`ProviderQuerySnapshot`]) pinned at the rename
/// fence — NEVER the live store. Between [`ProviderSurfaceStore::capture_current_carrier_api_set`]
/// returning and the merge finishing there is ZERO read of mutable store state
/// (`is_known_virtual_surface`, `captured_snapshot_still_honored`, `current_snapshot`,
/// `snapshot_at`, any `lifecycle`/`snapshots` access). This closes the third TOCTOU:
/// a path `Closing` at capture, returned by the provider, then `finalize_close`d by a
/// background close driver (which does NOT hold the rename fence) BEFORE classify
/// could previously consult the now-cleared live store and mis-classify `NotVirtual`
/// → edit a same-named real file with virtual offsets → corruption. The captured
/// snapshot is the exact generation the provider's offsets were produced against and
/// IS the merge authority; a legitimate background re-sync between capture and
/// classify must NOT change how those pinned offsets map, so re-validating against
/// live state would be both a live read AND wrong.
///
/// (The merge's `carrier_source_exists` / `source_reader` host-VFS reads for the
/// `NotVirtual` real-file branch read a genuinely-real on-disk file AFTER this
/// decision — they are not reads of mutable store state and are out of scope.)
///
/// The decision, over the CAPTURED state for `api_path`:
///
/// 1. **Captured [`CapturedPathState::Current`] (a `CarrierApi` surface at capture),
///    context builds** → `Vouched(ctx)`: map the API-surface offsets onto the `.vue`
///    through THAT captured generation's own source map.
/// 2. **Captured [`CapturedPathState::Current`] but no context (no source map)** →
///    `VirtualDrop` (fail closed).
/// 3. **Captured [`CapturedPathState::KnownNonMappable`]** (was `Closing`, or a
///    non-`CarrierApi`/snapshot-less `Current` at capture) → `VirtualDrop`. The store
///    knew the path as virtual; its offsets index VIRTUAL content, so it must NEVER
///    fall through to the real-file branch.
/// 4. **ABSENT from the capture** → a genuinely real file (a hand-written
///    `Child.vue.ts` next to `Child.vue`) the store did not know as virtual at
///    capture: `NotVirtual` (edit it in place).
#[must_use]
pub fn classify_captured_api_surface(
    captured: &ProviderQuerySnapshot,
    api_path: &str,
    negotiated_encoding: tower_lsp_server::ls_types::PositionEncodingKind,
) -> crate::type_provider::merge::ApiSurfaceResolution {
    use crate::type_provider::merge::ApiSurfaceResolution;

    match captured.captured_state_for(api_path) {
        // Mappable captured surface: build the context from THE CAPTURED snapshot
        // (its own provider/carrier indexes + source map). A snapshot with no source
        // map fails closed.
        Some(CapturedPathState::Current(snapshot)) => {
            match external_ide_context_from_snapshot(snapshot, negotiated_encoding) {
                Some(ctx) => ApiSurfaceResolution::Vouched(ctx),
                None => ApiSurfaceResolution::VirtualDrop,
            }
        }
        // Known-virtual-but-not-mappable at capture (e.g. Closing): drop, never
        // edit a same-named real file with virtual offsets.
        Some(CapturedPathState::KnownNonMappable) => ApiSurfaceResolution::VirtualDrop,
        // Absent from the capture: the store did not know it as virtual → a real
        // on-disk file, edit in place.
        None => ApiSurfaceResolution::NotVirtual,
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

/// Resolve the carrier `.vue` source for `canonical_id`, working for OPEN and
/// CLOSED carriers alike.
///
/// Prefers the open editor buffer (the authoritative in-memory edit state — an
/// unsaved edit differs from the on-disk file, and the rename maps INTO this
/// source) when a `DocumentRegistry` is supplied and the carrier is open; falls
/// back to the host/VFS source, which is the workspace authority for a CLOSED
/// carrier. This is the closed-carrier resolution the design mandates: the
/// carrier `.vue` source is captured WITHOUT requiring an open document.
#[must_use]
pub fn resolve_carrier_source(
    documents: Option<&DocumentRegistry>,
    host: &VerterHost,
    canonical_id: &str,
) -> Option<Arc<str>> {
    if let Some(documents) = documents {
        if let Some(uri) = documents.canonical_id_to_uri(canonical_id) {
            if let Some(doc) = documents.get(&uri) {
                return Some(doc.source.clone());
            }
        }
    }
    host.get_source(canonical_id)
}

/// THE single record choke point every API-surface sync site funnels through.
///
/// Captures an immutable [`ProviderSurfaceSnapshot`] of the `{carrier}.ts` API
/// surface just synced to the provider — the EXACT `api_code` (the provider's
/// offsets index it), its source map (parsed from the SAME bytes), and the
/// carrier `.vue` source (open buffer or host/VFS — works closed) — under a
/// fresh generation. A cross-file rename later interprets the provider's offsets
/// against this precise generation.
///
/// Pass `documents = Some(..)` from the live/server/coordinator paths (so an
/// open carrier's unsaved buffer wins); pass `None` from the host-only
/// background workspace scanner. Routing every sync path through this one helper
/// makes completeness STRUCTURAL: a sync site that records is correct by calling
/// this; the only way to miss a generation is to not call it — auditable by a
/// single grep over call sites.
pub fn record_carrier_api_surface(
    store: &ProviderSurfaceStore,
    documents: Option<&DocumentRegistry>,
    host: &VerterHost,
    canonical_id: &str,
    provider_path: &str,
    api_code: &str,
    source_map_json: Option<&str>,
) {
    let Some(carrier_source) = resolve_carrier_source(documents, host, canonical_id) else {
        // No carrier source to map INTO → recording a snapshot would be useless
        // (the merge would fail closed anyway). Skip the record; the path simply
        // has no current generation, so a returned offset fails closed.
        return;
    };
    let source_map = source_map_json
        .and_then(|json| PositionMapper::from_json(json).ok())
        .map(ProviderPositionMapper::source_map);
    store.record(RecordSurface {
        provider_path: provider_path.to_string(),
        kind: ProviderSurfaceKind::CarrierApi,
        source_canonical: canonical_id.to_string(),
        provider_content: Arc::from(api_code),
        source_map,
        carrier_source,
    });
}

/// Record an API-surface snapshot when only the synced `api_code` is in scope
/// (no source map at hand). Fetches the live `get_public_api()` source map and
/// attaches it ONLY when the live code byte-matches `api_code`, so the snapshot
/// never pairs the synced offsets with a map produced against drifted content.
pub fn record_carrier_api_surface_code_only(
    store: &ProviderSurfaceStore,
    documents: Option<&DocumentRegistry>,
    host: &VerterHost,
    canonical_id: &str,
    provider_path: &str,
    api_code: &str,
) {
    let owned_map: Option<Arc<str>> = host
        .get_public_api(canonical_id)
        .filter(|api| &*api.code == api_code)
        .and_then(|api| api.source_map.clone());
    record_carrier_api_surface(
        store,
        documents,
        host,
        canonical_id,
        provider_path,
        api_code,
        owned_map.as_deref(),
    );
}

#[cfg(test)]
#[path = "provider_surface_store_tests.rs"]
mod tests;

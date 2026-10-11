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
//! [`ProviderSurfaceSnapshot`] under a fresh monotonic GENERATION, keyed by
//! `(provider_path, generation)`, and tracks the CURRENT generation per path. A
//! cross-file rename:
//!
//! 1. captures the CURRENT snapshot set (cheap `Arc` clones) under a fence,
//! 2. queries the provider,
//! 3. interprets the returned offsets ONLY against the `Arc` it captured, and
//! 4. maps through that snapshot's own source map, or DROPS.
//!
//! Because a snapshot is immutable and the capture holds it by `Arc`, a
//! concurrent sync that advances the generation, or a CLOSE that retires the
//! ACTIVE generation, can NEVER retroactively change a snapshot an in-flight
//! request already captured. This is the property the prior "latest-only identity
//! gate" lacked: it checked the LATEST identity, not the generation the offsets
//! were produced against, so it both over-dropped (a fresher latest entry) and
//! admitted a residual race.
//!
//! ## Retention is bounded by REACHABILITY, never by history
//!
//! The capture pins what it needs — the `Arc`, not the map slot — so the store's
//! own map never has to keep a superseded generation alive for it. It therefore
//! does not: a `record` that supersedes a path's current generation, and a
//! `forget` that retires one, each DROP the store's reference to the generation
//! they displaced. The map holds at most ONE entry per live path (its `Current`
//! generation); a superseded generation survives exactly as long as some in-flight
//! capture still holds it, and not one instant longer.
//!
//! This is load-bearing, not tidiness. A record happens on every provider sync —
//! i.e. on every edit of every open carrier. An insert-only map would retain every
//! version of every document ever synced for the life of the session: each entry
//! holding the full provider text, the full carrier source and a UTF-16 line index
//! over each. Over a long editing session that is unbounded growth with no
//! reachable reader.
//!
//! Every retained snapshot is CHARGED, for as long as it lives, to the one
//! process-local [`SemanticRetentionAccount`](verter_session_query::retention::SemanticRetentionAccount) — the same aggregate byte ceiling
//! the semantic caches admit against, so provider-surface bytes and semantic-cache
//! bytes cannot each claim the ceiling independently. The charge is
//! [`ChargeClass::Pinned`](verter_session_query::retention::ChargeClass::Pinned):
//! a synced surface is an obligation, not a policy choice — refusing to retain one
//! would leave the provider holding content this store could no longer map back,
//! which is the silent-corruption outcome the whole module exists to prevent. The
//! charge rides INSIDE the snapshot, so the RAII release happens exactly when the
//! last owner — the map slot or an in-flight capture, whichever outlives the other
//! — drops it.

use std::collections::HashMap;
use std::sync::Arc;
use verter_span::path::InjectedPathKey;

use dashmap::DashMap;
use parking_lot::RwLock;

use verter_session_query::analysis::types::Hash16;
use verter_session_query::retention::{
    RetainedFootprint, RetentionCharge, StoreAccount, ENTRY_OVERHEAD_BYTES,
};

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
    /// The path's content epoch: equal across records of one path exactly while
    /// the provider bytes, the carrier source and the map identity all stay the
    /// same. An identical re-record keeps it; any change mints a fresh one from
    /// the session-monotonic sequence, so a surface that changes and changes
    /// back (A→B→A) never returns to the epoch it started from.
    pub content_epoch: u64,
    /// The path's surface incarnation: kept by every record of a live path and
    /// minted fresh by the first record after the path was absent or closing,
    /// so a close and identical reopen is a distinct incarnation.
    pub incarnation: u64,
    /// The path's project-owner epoch: kept while successive records name the
    /// same owning project and minted fresh whenever the owner changes, so an
    /// owner that changes and changes back (A→B→A) never returns to the epoch
    /// it started from. Independent of [`Self::content_epoch`]: an owner move
    /// with identical bytes keeps the content epoch.
    pub owner_epoch: u64,
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
/// The immutable CONTENT payload of a provider surface: every byte the mapping
/// reads, and every byte the retention account charges.
///
/// Split out from [`ProviderSurfaceSnapshot`] so it can be SHARED. A provider
/// re-sync is not necessarily a content change — a background re-sync, a
/// re-open, or a request-driven resync of an unedited carrier all re-record the
/// same bytes — and each of those must still mint a FRESH generation, because
/// the generation is the capture's basis identity and a captured snapshot is
/// only honored across a bump when the content is proven identical. Rebuilding
/// the payload for an identical re-record would allocate both UTF-16 line
/// indexes (each roughly its source again), re-hash both texts, and re-charge
/// the aggregate account, to arrive at a byte-for-byte duplicate of a payload
/// the store already owns.
///
/// So an identical re-record clones this `Arc` instead. The fresh generation is
/// on the snapshot; the payload underneath is the one already in memory, charged
/// ONCE — the charge lives here, so the bytes are released exactly when the last
/// snapshot sharing the payload drops.
pub struct ProviderSurfacePayload {
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
    /// Content hash of `provider_content`.
    pub content_hash: ContentHash,
    /// Content hash of `carrier_source`.
    pub source_hash: ContentHash,
    /// The `CodeTransform` source-map identity the payload was built under.
    pub map_hash: Hash16,
    /// The aggregate-account reservation covering THIS payload's bytes, held for
    /// exactly as long as the payload itself.
    ///
    /// Private and never read: its whole job is `Drop`. Because the charge lives
    /// inside the `Arc`-shared payload, the reservation is released precisely
    /// when the LAST owner goes away — the store's map slot when no capture
    /// outlived it, the final in-flight capture when one did, or the last
    /// snapshot sharing the payload across an identical re-record. There is no
    /// release call for an early-return path to skip, no way to release twice,
    /// and a shared payload is charged ONCE rather than once per generation.
    _retention: RetentionCharge,
    /// Whether the serving provider has ever acknowledged THESE bytes — the
    /// recorded-and-delivered half of the surface. See [`DeliveryCell`].
    delivery: DeliveryCell,
}

/// The delivery acknowledgement a payload carries: whether the serving
/// provider's own evidence has ever shown it holding exactly these bytes, and
/// through which delivery model.
///
/// Set only from serving-side evidence observed by
/// [`ProviderSurfaceStore::delivery_of`] — the engine's per-incarnation
/// application receipt, or the gateway's committed membership publication —
/// never from the record itself. It is monotonic (unset → acknowledged) and
/// lives on the payload, so an identical re-record, which shares the payload,
/// keeps it, while a reopen or a content change, which builds a new payload,
/// starts unacknowledged. The acknowledgement alone never makes a surface
/// servable: currency against the serving incarnation is re-read on every
/// verdict. It only tells a surface that was never delivered (a record that ran
/// ahead of its delivery) from one whose delivery was lost or overtaken.
/// It stores neither an engine incarnation nor a delivery sequence: equal
/// bytes after replay or an unrecorded A→B→A delivery reuse this acknowledgement.
///
/// One byte inline in the payload: an acknowledgement adds no allocation to a
/// record.
#[derive(Default)]
struct DeliveryCell(std::sync::atomic::AtomicU8);

impl DeliveryCell {
    const UNACKNOWLEDGED: u8 = 0;
    const APPLIED: u8 = 1;
    const PUBLISHED: u8 = 2;

    fn acknowledged(&self) -> bool {
        self.0.load(std::sync::atomic::Ordering::Acquire) != Self::UNACKNOWLEDGED
    }

    fn acknowledge(&self, model: u8) {
        let _ = self.0.compare_exchange(
            Self::UNACKNOWLEDGED,
            model,
            std::sync::atomic::Ordering::AcqRel,
            std::sync::atomic::Ordering::Acquire,
        );
    }
}

/// What the serving provider can prove it holds for one recorded surface —
/// the answer a [`ProviderDeliveryWitness`] reads from the provider's own
/// ledger, locally and without a provider round trip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServingDelivery {
    /// The serving engine incarnation accepted exactly these bytes for the
    /// surface's path (its application receipt).
    Applied(Arc<str>),
    /// The serving engine incarnation holds no bytes for the path: never
    /// delivered, or lost to an engine restart, an ownership exclusion or a
    /// failed delivery.
    NotApplied,
    /// The engine reads the surface through the carrier membership
    /// publication, and the committed publication attests exactly this
    /// surface.
    Published,
    /// The engine reads the surface through the carrier membership
    /// publication, and no committed publication attests this surface.
    Unpublished,
    /// The engine keeps no application ledger the store could consult: it
    /// cannot say which bytes it holds, so nothing it answers is attributable
    /// to a recorded surface.
    Uncertified,
}

/// The serving provider's delivery ledger, as the store consults it. Bound
/// once per server by [`ProviderSurfaceStore::bind_delivery_witness`]; every
/// call is a local ledger read — it never issues a provider request.
pub trait ProviderDeliveryWitness: Send + Sync {
    /// What the serving provider holds for `surface`'s provider path.
    fn serving_delivery(&self, surface: &ProviderSurfaceSnapshot) -> ServingDelivery;
}

/// The typed delivery state of one recorded surface at the ledger observation:
/// whether the provider's evidence attests the recorded bytes at that moment.
/// This is not an identity of the delivery a particular query evaluated.
///
/// Only [`Self::Delivered`] may serve a provider answer: a record is never
/// evidence of its own delivery. Every other state is a signal of its own —
/// never a diagnostics outcome — and a foreground request meets it by
/// repairing the requested file's surface before dispatch, or by answering
/// without the provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceDelivery {
    /// Recorded and delivered: the serving provider holds exactly these bytes.
    Delivered,
    /// Recorded, but the serving provider has never acknowledged these bytes:
    /// the record ran ahead of its delivery, or the delivery never happened.
    AwaitingDelivery,
    /// The bytes were acknowledged once, but the serving engine now holds
    /// different bytes at the path: a newer delivery ran ahead of its record,
    /// or the engine fell behind.
    EngineDiverged,
    /// The bytes were acknowledged once, but the serving provider no longer
    /// holds them: an engine restart, an ownership exclusion or a failed
    /// delivery.
    DeliveryLost,
    /// No serving-side ledger exists to consult (no provider is bound, or it
    /// cannot certify application): nothing proves the engine holds these
    /// bytes, so the record alone never serves a provider answer.
    Unwitnessed,
}

impl SurfaceDelivery {
    /// Whether this state satisfies the delivery prerequisite for decoding.
    /// The verdict alone does not bind a query to a particular delivery.
    #[must_use]
    pub const fn is_servable(self) -> bool {
        matches!(self, Self::Delivered)
    }
}

impl ProviderSurfacePayload {
    /// Whether this payload is byte-for-byte what `surface` would produce, so a
    /// re-record can share it instead of rebuilding it.
    ///
    /// Compares the SOURCE bytes, not the derived hashes, because the derived
    /// hashes are exactly what rebuilding would recompute. `Arc::ptr_eq` short-
    /// circuits the common case (the producer handed back the same `Arc`); the
    /// byte compare is the fallback, and both are far cheaper than the two line
    /// indexes this decides whether to build.
    ///
    /// `map_hash` is the map's identity: identical provider content under a
    /// DIFFERENT map identity is a different payload (the map is what the
    /// offsets travel through), so it is compared rather than derived.
    fn matches(&self, surface: &RecordSurface) -> bool {
        self.map_hash == surface.map_hash
            && self.source_map.is_some() == surface.source_map.is_some()
            && str_eq(&self.provider_content, &surface.provider_content)
            && str_eq(&self.carrier_source, &surface.carrier_source)
    }
}

fn str_eq(left: &Arc<str>, right: &Arc<str>) -> bool {
    Arc::ptr_eq(left, right) || **left == **right
}

/// An immutable, fully self-contained capture of one synced provider surface.
///
/// Holds everything needed to map a returned provider offset back onto the
/// carrier source WITHOUT re-reading the host/VFS or the live `get_public_api()`
/// at merge time, so the mapping is immune to any change that lands after
/// capture. `Arc`-shared for cheap in-flight capture.
///
/// The content half lives behind [`ProviderSurfacePayload`], reached by `Deref`
/// so every reader still writes `snapshot.provider_content`. What the split buys
/// is that two generations of byte-identical content are two snapshots over ONE
/// payload.
///
/// Not `Debug` — `ProviderPositionMapper` (held as `source_map`) is not `Debug`,
/// and a snapshot is an internal mapping artifact, never logged structurally.
pub struct ProviderSurfaceSnapshot {
    pub stamp: ProviderSurfaceStamp,
    pub kind: ProviderSurfaceKind,
    /// The carrier canonical id (`/src/Child.vue`) that owns this surface.
    pub source_canonical: Arc<str>,
    /// The immutable content payload, shared with any other generation whose
    /// bytes are identical.
    pub payload: Arc<ProviderSurfacePayload>,
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

impl std::ops::Deref for ProviderSurfaceSnapshot {
    type Target = ProviderSurfacePayload;

    fn deref(&self) -> &Self::Target {
        &self.payload
    }
}

/// Bytes a [`LineIndex`] retains for a source of `source_len` bytes.
///
/// A line index is NOT a view: it keeps its OWN owned copy of the whole source
/// text plus a `u32` line-start per line. So each index costs roughly the source
/// again; the line-start vector is allowed for at a conservative one line per 16
/// source bytes.
const fn line_index_footprint_bytes(source_len: usize) -> usize {
    source_len + (source_len / 16) * std::mem::size_of::<u32>()
}

/// Bytes the parsed source map retains, estimated from the generated text it maps.
///
/// [`ProviderPositionMapper`] exposes no size, and walking it on every record
/// would put an O(mappings) scan on the sync path to refine a figure that only
/// ever decides admission. A mapping table scales with the generated content it
/// indexes, so a fraction of the provider text is the cheap conservative stand-in.
const fn source_map_footprint_bytes(provider_len: usize) -> usize {
    provider_len / 2
}

impl RetainedFootprint for ProviderSurfaceSnapshot {
    /// Estimated bytes this snapshot keeps alive on its own.
    ///
    /// Counts BOTH copies of each text: the `Arc<str>` the snapshot owns and the
    /// duplicate the corresponding line index owns. Under-reporting the duplicate
    /// would make the account's figure a comfortable fiction rather than a bound.
    fn retained_footprint_bytes(&self) -> usize {
        let provider_len = self.provider_content.len();
        let carrier_len = self.carrier_source.len();
        ENTRY_OVERHEAD_BYTES
            + std::mem::size_of::<Self>()
            + self.stamp.provider_path.len()
            + self.source_canonical.len()
            + provider_len
            + line_index_footprint_bytes(provider_len)
            + carrier_len
            + line_index_footprint_bytes(carrier_len)
            + self
                .source_map
                .as_ref()
                .map_or(0, |_| source_map_footprint_bytes(provider_len))
    }
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
    /// The immutable root every foreground capture shares: the same per-path
    /// states as `paths`, resolved to their snapshots and keyed by filesystem
    /// identity. Rewritten under the lifecycle WRITE lock in the same critical
    /// section as each `paths` mutation, so a capture never observes the two
    /// apart.
    root: ProviderLifecycleRoot,
}

impl Lifecycle {
    /// Publish `path`'s new state into the shared root (`None` ⇒ fully
    /// unknown). Called under the lifecycle write lock by every `paths`
    /// mutation. A capture already holding the previous root keeps it unchanged:
    /// the persistent map copies only the nodes on this one path's route.
    fn publish(&mut self, path: &Arc<str>, state: Option<CapturedPathState>) {
        let identity = InjectedPathKey::new(path);
        let mut spellings: Vec<(Arc<str>, CapturedPathState)> = self
            .root
            .by_identity
            .get(&identity)
            .map(|spellings| {
                spellings
                    .iter()
                    .filter(|(spelling, _)| **spelling != **path)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        if let Some(state) = state {
            spellings.push((Arc::clone(path), state));
        }
        if spellings.is_empty() {
            self.root.by_identity.remove(&identity);
        } else {
            self.root.by_identity.insert(identity, spellings.into());
        }
    }
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
    /// The REACHABLE snapshots, keyed by `(provider_path, generation)`. At most
    /// one entry per live path: a `record` or `forget` that displaces a path's
    /// current generation removes the displaced entry, leaving in-flight captures
    /// (which hold the `Arc`, not the slot) as its only remaining owners. See the
    /// module docs — this is the retention bound, not an optimization.
    snapshots: DashMap<(Arc<str>, u64), Arc<ProviderSurfaceSnapshot>>,
    /// The single per-path lifecycle map (live `Current` / closing `Closing`)
    /// plus the shared generation/epoch counter, under ONE lock. Replaces the
    /// former two loosely-coupled maps (`current` + `tombstones`) and the separate
    /// generation counter: one lock makes every known→known transition observe
    /// atomic, and lets a close be epoch-stamped so a reopen during an older
    /// close's await window can never have its fresh snapshot erased by the stale
    /// finalize.
    lifecycle: RwLock<Lifecycle>,
    /// The aggregate byte account every retained snapshot charges. `StoreAccount`
    /// has no account-less variant, so "this store retains surfaces but consumes
    /// no aggregate headroom" is unrepresentable; its `Default` is the ONE
    /// process-local account, never a private per-store quota.
    account: StoreAccount,
    /// The serving provider's delivery ledger, bound once the server has a
    /// provider. Read outside every store lock.
    witness: RwLock<Option<Arc<dyn ProviderDeliveryWitness>>>,
}

impl ProviderSurfaceStore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Bind an EXPLICIT retention account instead of the process-local one.
    ///
    /// Production always uses [`Self::new`]; this exists so a test can drive and
    /// observe retention deterministically, without the rest of the process's
    /// occupancy moving underneath its assertions. Test-only: an isolated
    /// account can be minted only under test support.
    #[cfg(test)]
    #[must_use]
    pub fn with_account(
        account: Arc<verter_session_query::retention::SemanticRetentionAccount>,
    ) -> Self {
        Self {
            inner: Arc::new(StoreInner {
                account: StoreAccount::new(account),
                ..StoreInner::default()
            }),
        }
    }

    /// How many snapshots the store itself currently holds a reference to.
    ///
    /// The retention bound stated as a number: it never exceeds the count of live
    /// (`Current`) paths, regardless of how many generations those paths have been
    /// through. A snapshot still pinned by an in-flight capture is NOT counted —
    /// the store no longer owns it.
    #[must_use]
    pub fn retained_surface_count(&self) -> usize {
        self.inner.snapshots.len()
    }

    /// Weak handles to every snapshot the store currently owns, plus each one's
    /// provider path.
    ///
    /// Tests only. A WEAK handle observes an allocation without owning it, which
    /// is exactly the question an acceptance criterion about RELEASED handles has
    /// to ask: after the store has moved on and the request that captured a
    /// generation is gone, is that generation still alive anywhere in the
    /// process? A byte total cannot answer it (another participant's activity
    /// moves the same number), and a strong handle would itself be the owner
    /// keeping the answer alive.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn weak_handles_to_retained_surfaces(
        &self,
    ) -> Vec<(Arc<str>, std::sync::Weak<ProviderSurfaceSnapshot>)> {
        self.inner
            .snapshots
            .iter()
            .map(|entry| (Arc::clone(&entry.key().0), Arc::downgrade(entry.value())))
            .collect()
    }

    /// Estimated bytes of the snapshots the store still owns.
    #[must_use]
    pub fn retained_surface_bytes(&self) -> usize {
        self.inner
            .snapshots
            .iter()
            .map(|e| e.value().retained_footprint_bytes())
            .sum()
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
    ///
    /// BOUNDED: the generation this record DISPLACES is dropped from the map, in
    /// the same critical section, AFTER the new one is published and the lifecycle
    /// points at it. A record happens on every sync of every open carrier, so
    /// keeping the displaced entry would retain every version of every document
    /// for the life of the session. Dropping it is safe because a capture pins the
    /// `Arc`, not the slot: an in-flight request that already captured the
    /// displaced generation keeps it alive by itself and maps exactly as before —
    /// and the drop happens under the lifecycle WRITE lock, which every capture
    /// takes for read, so no capture can be mid-scan while it happens.
    pub fn record(&self, mut surface: RecordSurface) -> Arc<ProviderSurfaceSnapshot> {
        let provider_path: Arc<str> = Arc::from(surface.provider_path.as_str());

        // REUSE: a re-sync is not necessarily a content change. Before building
        // anything, ask whether the path's CURRENT payload is already byte-for-
        // byte what this record would produce; if it is, the new generation
        // shares it. That skips both UTF-16 line indexes, both content hashes,
        // the source-map `Arc`, and — because the charge lives inside the shared
        // payload — a second reservation for bytes the account is already
        // charging. The generation is still FRESH: basis identity is the
        // snapshot's, and only the content underneath is shared.
        // An absent or `Closing` path simply means "nothing to reuse" — never a
        // fallback that could vouch content.
        let reusable = self
            .current_snapshot(&provider_path)
            .filter(|current| {
                current.kind == surface.kind
                    && *current.source_canonical == *surface.source_canonical
                    && current.payload.matches(&surface)
            })
            .map(|current| Arc::clone(&current.payload));

        let payload = match reusable {
            Some(payload) => payload,
            None => Arc::new(self.build_payload(
                &provider_path,
                surface.source_canonical.len(),
                Arc::clone(&surface.provider_content),
                surface.source_map.take(),
                Arc::clone(&surface.carrier_source),
                surface.map_hash,
            )),
        };
        let source_canonical: Arc<str> = Arc::from(surface.source_canonical.as_str());

        let mut lifecycle = self.inner.lifecycle.write();
        // Assign the generation at the SAME linearization point as the state
        // mutation (see LINEARIZATION above).
        let generation = lifecycle.next_epoch;
        lifecycle.next_epoch += 1;
        // The content epoch and incarnation continue from the snapshot this
        // record displaces, read under the same write lock, so two racing
        // records cannot both inherit from one predecessor.
        let displaced_snapshot = match lifecycle.paths.get(&provider_path) {
            Some(ProviderPathState::Current {
                generation: current,
            }) => self
                .inner
                .snapshots
                .get(&(Arc::clone(&provider_path), *current))
                .map(|entry| Arc::clone(entry.value())),
            Some(ProviderPathState::Closing { .. }) | None => None,
        };
        let (content_epoch, incarnation, owner_epoch) = match displaced_snapshot {
            Some(displaced) => {
                let same_content = displaced.kind == surface.kind
                    && *displaced.source_canonical == *surface.source_canonical
                    && displaced.stamp.content_hash == payload.content_hash
                    && displaced.stamp.source_hash == payload.source_hash
                    && displaced.stamp.map_hash == surface.map_hash
                    && displaced.source_map.is_some() == payload.source_map.is_some();
                let content_epoch = if same_content {
                    displaced.stamp.content_epoch
                } else {
                    generation
                };
                let owner_epoch = if displaced.project_owner == surface.project_owner {
                    displaced.stamp.owner_epoch
                } else {
                    generation
                };
                (content_epoch, displaced.stamp.incarnation, owner_epoch)
            }
            None => (generation, generation, generation),
        };

        let snapshot = Arc::new(ProviderSurfaceSnapshot {
            stamp: ProviderSurfaceStamp {
                provider_path: Arc::clone(&provider_path),
                generation,
                content_hash: payload.content_hash,
                source_hash: payload.source_hash,
                map_hash: surface.map_hash,
                content_epoch,
                incarnation,
                owner_epoch,
            },
            kind: surface.kind,
            source_canonical,
            payload,
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
        let displaced = lifecycle.paths.insert(
            Arc::clone(&provider_path),
            ProviderPathState::Current { generation },
        );
        lifecycle.publish(
            &provider_path,
            Some(CapturedPathState::Current(Arc::clone(&snapshot))),
        );
        // Release the store's hold on the generation this record displaced, AFTER
        // the lifecycle already points at the new one (so no reader can observe
        // `Current { displaced }` and then miss its snapshot) and still under the
        // write lock (so no capture can be mid-scan). A `Closing` displacement has
        // no live generation to drop — `forget` already released it.
        if let Some(ProviderPathState::Current {
            generation: displaced_generation,
        }) = displaced
        {
            self.inner
                .snapshots
                .remove(&(provider_path, displaced_generation));
        }
        drop(lifecycle);
        snapshot
    }

    /// Build the immutable payload for a surface whose content the store does not
    /// already hold.
    ///
    /// The UTF-16 line indexes and content hashes are computed HERE, once, so the
    /// payload is self-contained: the merge never re-measures the live content.
    /// All of it happens BEFORE the lifecycle lock, to keep the critical section
    /// short.
    ///
    /// The reservation's class is `Pinned`, so it is unconditional and cannot
    /// fail: a surface the provider is already holding is an obligation this
    /// store must be able to map back, never a discretionary cache entry to
    /// decline. It still CONSUMES aggregate headroom, which is the point —
    /// provider-surface bytes push back on discretionary semantic retention
    /// instead of being invisible to it.
    fn build_payload(
        &self,
        provider_path: &Arc<str>,
        source_canonical_len: usize,
        provider_content: Arc<str>,
        source_map: Option<ProviderPositionMapper>,
        carrier_source: Arc<str>,
        map_hash: Hash16,
    ) -> ProviderSurfacePayload {
        let provider_utf16_line_index = LineIndex::new_utf16(&provider_content);
        let carrier_utf16_line_index = LineIndex::new_utf16(&carrier_source);
        let content_hash = ContentHash::of(&provider_content);
        let source_hash = ContentHash::of(&carrier_source);
        let source_map = source_map.map(Arc::new);

        // The estimate is taken from the inputs, which are exactly the fields the
        // payload is about to own.
        let footprint = ENTRY_OVERHEAD_BYTES
            + std::mem::size_of::<ProviderSurfaceSnapshot>()
            + std::mem::size_of::<ProviderSurfacePayload>()
            + provider_path.len()
            + source_canonical_len
            + provider_content.len()
            + line_index_footprint_bytes(provider_content.len())
            + carrier_source.len()
            + line_index_footprint_bytes(carrier_source.len())
            + source_map
                .as_ref()
                .map_or(0, |_| source_map_footprint_bytes(provider_content.len()));
        let retention = self.inner.account.get().pin(footprint);

        ProviderSurfacePayload {
            provider_content,
            provider_utf16_line_index,
            source_map,
            carrier_source,
            carrier_utf16_line_index,
            content_hash,
            source_hash,
            map_hash,
            _retention: retention,
            delivery: DeliveryCell::default(),
        }
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
    /// under an epoch whose only owner's close errored. The store RELEASES its own
    /// reference to the retired generation (nothing can reach it through the store
    /// again — a `Closing` path has no current snapshot and captures as
    /// `KnownNonMappable`), while an in-flight request that captured it keeps
    /// mapping correctly from its own `Arc`. ALWAYS returns a token (even when the
    /// path was absent from the map — a close of an untracked path conservatively
    /// becomes known-virtual = fail-closed).
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
        let mut retired_generation = None;
        let epoch = match lifecycle.paths.get(&path) {
            // Already closing: REUSE the in-flight close's epoch (idempotent) — do NOT
            // mint a fresh epoch and do NOT overwrite, so a DUPLICATE close of an
            // already-retired surface cannot strand the path in Closing under an epoch
            // whose only owner's close errored. Both duplicate closers thus hold a
            // token for the SAME epoch. The first `forget` already released the
            // retired generation, so this arm has nothing left to release.
            Some(ProviderPathState::Closing { epoch }) => *epoch,
            // Current or absent: mint a FRESH epoch and (re)enter Closing.
            state => {
                if let Some(ProviderPathState::Current { generation }) = state {
                    retired_generation = Some(*generation);
                }
                let minted = lifecycle.next_epoch;
                lifecycle.next_epoch += 1;
                minted
            }
        };
        lifecycle
            .paths
            .insert(Arc::clone(&path), ProviderPathState::Closing { epoch });
        lifecycle.publish(&path, Some(CapturedPathState::KnownNonMappable));
        // Release the store's hold on the generation this close retired, AFTER the
        // lifecycle already reads `Closing` and still under the write lock. This is
        // the other half of the retention bound: without it, every closed document's
        // last synced surface would stay resident for the life of the session.
        if let Some(generation) = retired_generation {
            self.inner
                .snapshots
                .remove(&(Arc::clone(&path), generation));
        }
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
                // Nothing to release here: the `forget` that began this close
                // already dropped the retired generation, and any `record` since
                // would have reopened the path to `Current` and made this finalize
                // a no-op. A fully-closed path therefore holds no snapshot — see
                // `close_after_capture_preserves_captured_snapshot`, which reads
                // `retained_surface_count()` across exactly this sequence.
                lifecycle.paths.remove(&token.provider_path);
                lifecycle.publish(&token.provider_path, None);
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
    ///
    /// The generation is read and resolved to its snapshot under ONE lifecycle
    /// read guard. [`Self::record`] drops the generation it displaces under the
    /// lifecycle WRITE lock, so a reader that released the guard between the two
    /// could find that generation already gone and answer "no current surface"
    /// for a path that was `Current` at every instant — which every
    /// byte-identical background re-sync of an open carrier would expose.
    #[must_use]
    pub fn current_snapshot(&self, provider_path: &str) -> Option<Arc<ProviderSurfaceSnapshot>> {
        let lifecycle = self.inner.lifecycle.read();
        let (path, ProviderPathState::Current { generation }) =
            lifecycle.paths.get_key_value(provider_path)?
        else {
            return None;
        };
        self.inner
            .snapshots
            .get(&(Arc::clone(path), *generation))
            .map(|entry| Arc::clone(entry.value()))
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

    /// Whether the path's CURRENT surface is still `captured`'s content epoch,
    /// incarnation and project owner — the bracket a foreground request closes
    /// around every surface whose provider answer it decodes, at that decode
    /// and again at settlement.
    ///
    /// Stricter than [`Self::captured_snapshot_still_honored`]: the content and
    /// owner epochs move on every change and never return, so a surface whose
    /// content or owner changed and changed back during the request fails the
    /// bracket even though it ends where it began. An identical re-record keeps
    /// both epochs and passes.
    ///
    /// The bracket also requires the surface to be DELIVERED
    /// ([`Self::delivery_of`]): the serving provider must still hold exactly
    /// the captured bytes. Equal epochs alone cannot say so — a record can run
    /// ahead of its delivery, a delivery can run ahead of its record, and an
    /// engine can restart — and an answer decoded through bytes the engine did
    /// not evaluate maps newer host offsets into older provider text.
    /// These observations do not detect an unrecorded A→B→A delivery between
    /// them or identify a same-byte replay after an engine restart. The path's
    /// incarnation above is distinct from the serving engine's incarnation.
    #[must_use]
    pub fn captured_surface_is_current(&self, captured: &ProviderSurfaceSnapshot) -> bool {
        self.current_snapshot(&captured.stamp.provider_path)
            .is_some_and(|current| {
                current.stamp.content_epoch == captured.stamp.content_epoch
                    && current.stamp.incarnation == captured.stamp.incarnation
                    && current.stamp.owner_epoch == captured.stamp.owner_epoch
                    && current.project_owner == captured.project_owner
            })
            && self.delivery_of(captured).is_servable()
    }

    /// Bind the serving provider's delivery ledger. Called once, by the server
    /// that owns the provider; a store with no bound witness answers
    /// [`SurfaceDelivery::Unwitnessed`].
    pub fn bind_delivery_witness(&self, witness: Arc<dyn ProviderDeliveryWitness>) {
        *self.inner.witness.write() = Some(witness);
    }

    /// The typed delivery state of `surface`: whether the serving provider
    /// holds exactly the bytes it records.
    ///
    /// Reads the bound witness — a local ledger read, never a provider round
    /// trip — outside every store lock. Serving-side proof of these exact bytes
    /// acknowledges the payload ([`DeliveryCell`]); the acknowledgement then
    /// tells a surface whose delivery was lost or overtaken from one that was
    /// never delivered, but it never vouches for currency on its own: every
    /// verdict re-reads what the serving incarnation holds now.
    #[must_use]
    pub fn delivery_of(&self, surface: &ProviderSurfaceSnapshot) -> SurfaceDelivery {
        let witness = self.inner.witness.read().clone();
        delivery_verdict(witness.as_deref(), surface)
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

    /// Capture the store's immutable lifecycle root — the shared handle a
    /// foreground request takes ONCE and derives every captured view from
    /// ([`ProviderLifecycleRoot::carrier_api_set`],
    /// [`ProviderLifecycleRoot::carrier_ide_set`]).
    ///
    /// CONSTANT COST: the capture clones one persistent-map handle under the
    /// lifecycle read guard. It visits no path, copies no entry and takes no
    /// reference on any snapshot, so warm capture work, request-owned entries and
    /// lock hold time are independent of how many unrelated paths the workspace
    /// tracks. The snapshots the root reaches stay alive only while a request
    /// holds it; dropping the request (completion or cancellation) releases them.
    ///
    /// ATOMICITY: every lifecycle mutation rewrites the root under the lifecycle
    /// WRITE lock in the same critical section as the per-path state it mirrors,
    /// so the captured root is one consistent point-in-time view of every path's
    /// state and snapshot — no `(state, snapshot)` pair can tear, and a later
    /// `record`/`forget`/`finalize_close` never changes what it resolves to.
    #[must_use]
    pub fn capture_lifecycle_root(&self) -> ProviderLifecycleRoot {
        let mut root = self.inner.lifecycle.read().root.clone();
        root.witness = self.inner.witness.read().clone();
        root
    }

    /// The captured carrier-API view of every tracked path — the immutable
    /// in-flight set a cross-file rename holds across its provider query, and the
    /// SOLE authority [`classify_captured_api_surface`] routes on (it never reads
    /// the live store afterward). A request that also needs the IDE view captures
    /// [`Self::capture_lifecycle_root`] once and derives both from it.
    #[must_use]
    pub fn capture_current_carrier_api_set(&self) -> ProviderQuerySnapshot {
        self.capture_lifecycle_root().carrier_api_set()
    }

    /// The captured carrier-IDE view of every tracked path — the immutable
    /// in-flight set a navigation handler holds across its provider query so a
    /// returned FOREIGN carrier IDE location maps through the surface captured
    /// when the request began, never whatever surface is current at merge time.
    #[must_use]
    pub fn capture_current_carrier_ide_set(&self) -> ProviderQuerySnapshot {
        self.capture_lifecycle_root().carrier_ide_set()
    }

    /// The account this store charges. Tests only — production never needs to
    /// reach past the store to the account.
    #[cfg(test)]
    #[must_use]
    pub fn account(&self) -> &Arc<verter_session_query::retention::SemanticRetentionAccount> {
        self.inner.account.get()
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

/// The per-path lifecycle state CAPTURED at the request fence — the SOLE input to
/// [`classify_captured_api_surface`]. Read from one captured
/// [`ProviderLifecycleRoot`], so it is a consistent point-in-time view of the
/// store, immune to any live mutation after capture.
///
/// Three cases are explicit; a path ABSENT from the captured root is the fourth (a
/// genuinely real on-disk file the store did not know as virtual at capture →
/// `NotVirtual`, edit in place).
#[derive(Clone)]
pub enum CapturedPathState {
    /// A `Current` path of the view's role with its full immutable snapshot —
    /// the only case that can map a returned `{carrier}.ts` offset onto the
    /// `.vue`. The merge maps ONLY through this captured generation's own source
    /// map.
    Current(Arc<ProviderSurfaceSnapshot>),
    /// A path the store KNEW as virtual at capture but for which there is NO
    /// mappable snapshot: it was `Closing` (a close in flight), or `Current` but
    /// of another role than the view's. Its offsets index VIRTUAL content, so it
    /// MUST classify `VirtualDrop` (fail closed) — never fall through to the
    /// real-file branch and edit a same-named real file with virtual offsets.
    KnownNonMappable,
}

/// The answer for a known path that has no mappable snapshot in a view.
static KNOWN_NON_MAPPABLE: CapturedPathState = CapturedPathState::KnownNonMappable;

/// The immutable provider lifecycle root: every tracked path's lifecycle state,
/// resolved to its snapshot, keyed by filesystem identity
/// ([`InjectedPathKey`], so a provider-returned slash, drive-letter or — on a
/// case-folding host — case spelling resolves the surface it names).
///
/// The store publishes a new root on every lifecycle mutation; a foreground
/// request captures the current one with
/// [`ProviderSurfaceStore::capture_lifecycle_root`]. Cloning a root is
/// constant-cost and shares every node, so a captured root costs the request
/// nothing per tracked path, and pins old snapshots only for as long as the
/// request lives.
///
/// Each identity carries every raw spelling the store tracks under it. Two
/// tracked spellings of one file are never mappable through a third spelling:
/// a lookup that does not match one of them exactly classifies
/// [`CapturedPathState::KnownNonMappable`] (fail closed), never an arbitrary
/// pick.
#[derive(Clone, Default)]
pub struct ProviderLifecycleRoot {
    by_identity: imbl::HashMap<InjectedPathKey, IdentitySpellings>,
    /// The serving provider's delivery ledger, bound to a CAPTURED root so a
    /// decode through it observes delivery like every other capture does.
    /// Never set on the store's own published root.
    witness: Option<Arc<dyn ProviderDeliveryWitness>>,
}

/// Every raw spelling tracked under one filesystem identity, with its state.
type IdentitySpellings = Arc<[(Arc<str>, CapturedPathState)]>;

/// A captured root names, for every path it holds a current surface of, the
/// exact provider bytes a returned location in that path is mapped through: a
/// provider query carrying it refuses to decode a foreign location through any
/// other bytes.
impl verter_type_runtime::provider_query::IntendedTargets for ProviderLifecycleRoot {
    fn intended(&self, path: &str) -> Option<Arc<str>> {
        match self.state_for(path)? {
            CapturedPathState::Current(snapshot) => Some(Arc::clone(&snapshot.provider_content)),
            CapturedPathState::KnownNonMappable => None,
        }
    }
}

/// A captured view names, for every path it holds a mappable surface of, the
/// exact provider bytes a returned location in that path is mapped through.
impl verter_type_runtime::provider_query::IntendedTargets for ProviderQuerySnapshot {
    fn intended(&self, path: &str) -> Option<Arc<str>> {
        self.snapshot_for(path)
            .map(|snapshot| Arc::clone(&snapshot.provider_content))
    }
}

impl ProviderSurfaceSnapshot {
    /// The delivered-surface identity a provider query intending this surface
    /// carries.
    #[must_use]
    pub fn delivered_surface_id(&self) -> verter_type_runtime::provider_query::DeliveredSurfaceId {
        verter_type_runtime::provider_query::DeliveredSurfaceId {
            generation: self.stamp.generation,
            content_epoch: self.stamp.content_epoch,
            incarnation: self.stamp.incarnation,
        }
    }

    /// A provider query whose request position was computed against this
    /// surface: the adapter binds it only to these exact bytes, so every
    /// offset it answers is one this surface's map can carry back.
    #[must_use]
    pub fn provider_query(&self) -> verter_type_runtime::provider_query::ProviderQuery {
        let query = verter_type_runtime::provider_query::ProviderQuery::intending(
            &*self.stamp.provider_path,
            self.delivered_surface_id(),
            Arc::clone(&self.provider_content),
        );
        crate::documents::ForegroundRequest::bracket_query(&query);
        query
    }
}

impl ProviderLifecycleRoot {
    /// This root viewed with `CarrierApi` snapshots as the mappable role.
    #[must_use]
    pub fn carrier_api_set(&self) -> ProviderQuerySnapshot {
        self.view(ProviderSurfaceKind::CarrierApi)
    }

    /// This root viewed with `CarrierIde` snapshots as the mappable role.
    #[must_use]
    pub fn carrier_ide_set(&self) -> ProviderQuerySnapshot {
        self.view(ProviderSurfaceKind::CarrierIde)
    }

    fn view(&self, kind: ProviderSurfaceKind) -> ProviderQuerySnapshot {
        ProviderQuerySnapshot {
            root: self.clone(),
            kind,
        }
    }

    /// The state captured for `provider_path`'s filesystem identity, or `None`
    /// when the store did not know it as virtual.
    fn state_for(&self, provider_path: &str) -> Option<&CapturedPathState> {
        let spellings = self.by_identity.get(&InjectedPathKey::new(provider_path))?;
        Some(match &**spellings {
            [(_, state)] => state,
            spellings => spellings
                .iter()
                .find(|(spelling, _)| **spelling == *provider_path)
                .map_or(&KNOWN_NON_MAPPABLE, |(_, state)| state),
        })
    }
}

/// One role's view of a captured [`ProviderLifecycleRoot`], pinned by a
/// provider-backed request across its provider query.
///
/// This captured view is the SOLE merge authority: [`classify_captured_api_surface`]
/// resolves every returned provider location by looking its path up HERE, never the
/// live store. A path captured [`CapturedPathState::Current`] maps through that
/// captured generation's own snapshot; a path captured
/// [`CapturedPathState::KnownNonMappable`] drops; a path ABSENT from the capture is
/// a genuinely real on-disk file (edited in place). A path whose live generation or
/// lifecycle later changes is irrelevant: this capture is the state the provider's
/// offsets were produced against.
#[derive(Clone)]
pub struct ProviderQuerySnapshot {
    root: ProviderLifecycleRoot,
    kind: ProviderSurfaceKind,
}

impl ProviderQuerySnapshot {
    /// The captured per-path lifecycle state for `provider_path`, or `None` if the
    /// path was ABSENT from the capture (the store did not know it as virtual at
    /// capture → a genuinely real file). This is the lookup classify routes on. A
    /// `Current` surface of another role than this view's is known but not
    /// mappable.
    #[must_use]
    pub fn captured_state_for(&self, provider_path: &str) -> Option<&CapturedPathState> {
        Some(match self.root.state_for(provider_path)? {
            CapturedPathState::Current(snapshot) if snapshot.kind != self.kind => {
                &KNOWN_NON_MAPPABLE
            }
            state => state,
        })
    }

    /// The captured MAPPABLE snapshot for `provider_path`, if it was a `Current`
    /// surface of this view's role at capture time. Returns `None` for a
    /// [`CapturedPathState::KnownNonMappable`] path (e.g. `Closing` at capture) and
    /// for an absent path alike — only a path with a mappable snapshot can vouch a
    /// `.vue` edit.
    #[must_use]
    pub fn snapshot_for(&self, provider_path: &str) -> Option<&Arc<ProviderSurfaceSnapshot>> {
        match self.captured_state_for(provider_path) {
            Some(CapturedPathState::Current(snapshot)) => Some(snapshot),
            _ => None,
        }
    }

    /// The typed delivery state of a surface this view captured, read fresh
    /// from the serving provider's ledger bound at capture — a local read,
    /// never a provider round trip. Only [`SurfaceDelivery::Delivered`] may
    /// decode a provider answer through the surface.
    #[must_use]
    pub fn delivery_of(&self, surface: &ProviderSurfaceSnapshot) -> SurfaceDelivery {
        delivery_verdict(self.root.witness.as_deref(), surface)
    }

    /// Whether the captured set is empty (no tracked path at all — neither a
    /// mappable `Current` surface nor a known-non-mappable one).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.root.by_identity.is_empty()
    }
}

/// The typed delivery state of `surface` under `witness` — the one verdict
/// [`ProviderSurfaceStore::delivery_of`] and a captured
/// [`ProviderQuerySnapshot::delivery_of`] both answer with.
fn delivery_verdict(
    witness: Option<&dyn ProviderDeliveryWitness>,
    surface: &ProviderSurfaceSnapshot,
) -> SurfaceDelivery {
    let Some(witness) = witness else {
        return SurfaceDelivery::Unwitnessed;
    };
    let cell = &surface.payload.delivery;
    let unproven = |acknowledged: bool, lost: SurfaceDelivery| {
        if acknowledged {
            lost
        } else {
            SurfaceDelivery::AwaitingDelivery
        }
    };
    match witness.serving_delivery(surface) {
        ServingDelivery::Applied(bytes) if str_eq(&bytes, &surface.provider_content) => {
            cell.acknowledge(DeliveryCell::APPLIED);
            SurfaceDelivery::Delivered
        }
        ServingDelivery::Published => {
            cell.acknowledge(DeliveryCell::PUBLISHED);
            SurfaceDelivery::Delivered
        }
        ServingDelivery::Applied(_) => {
            unproven(cell.acknowledged(), SurfaceDelivery::EngineDiverged)
        }
        ServingDelivery::NotApplied | ServingDelivery::Unpublished => {
            unproven(cell.acknowledged(), SurfaceDelivery::DeliveryLost)
        }
        ServingDelivery::Uncertified => SurfaceDelivery::Unwitnessed,
    }
}

mod producers;
pub use producers::*;

#[cfg(test)]
#[path = "../provider_surface_store_tests.rs"]
mod tests;

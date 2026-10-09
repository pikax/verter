//! Query-bound provider coordinates.
//!
//! An external engine answers a query against the bytes it holds when it
//! dequeues that query, and every position in the answer names a line and
//! column in those bytes. Converting the request offset against one read of a
//! live content cache and decoding the answer against a second read taken after
//! the await mixes two documents whenever an edit lands in between; mapping the
//! answer through a requester's captured surface that is not the one the engine
//! evaluated mixes two documents the same way.
//!
//! One capability carries a query from its requester to the engine and back:
//!
//! - The requester mints a [`ProviderQuery`] from the provider surface it
//!   computed the request position against ([`ProviderQuery::intending`]) and
//!   the surfaces it will map any foreign location through
//!   ([`IntendedTargets`]). A requester with no captured surface mints
//!   [`ProviderQuery::at_engine_surface`], which binds to whatever the engine
//!   holds.
//! - The hub that serves the query stamps the serving incarnation and the
//!   project the query was admitted into ([`QueryAdmission`]).
//! - The adapter binds it at the exact wire position of its request frame
//!   against a [`DeliveryLedger`], which records for one engine incarnation the
//!   bytes each file carries at the engine in the order the engine receives
//!   them. Binding refuses with a [`ProviderQueryConflict`] when the engine
//!   holds bytes other than the ones the requester intends, converts the request
//!   position against exactly those bytes, and retains them — plus an immutable
//!   snapshot of every other delivered file at that position — in a
//!   [`BoundQuery`]. Every range in the answer decodes through that binding;
//!   nothing reads the live ledger or a content cache for coordinates after the
//!   await.
//!
//! Bytes an engine reads out of band (a store its plugin reads while it
//! evaluates) have no wire position. They are recorded as
//! [`DeliveryOrder::OutOfBand`]. When the engine's publisher is known
//! ([`SurfacePublications`]), a query observes the publisher's position before
//! it is dispatched and, once the answer is in, requires every out-of-band file
//! it decoded through to be attested by the publisher with exactly the retained
//! bytes at a publication no later than that position
//! ([`DeliveryLedger::settle`]); a file no publisher names is read by the
//! engine from disk and must be unmodified since before dispatch. Any other
//! outcome is a [`ProviderQueryConflict`], never a decode against bytes the
//! engine may not have evaluated.
//!
//! A file the engine was never handed is read by the engine from disk. A
//! request on it converts against its disk bytes, which the binding retains
//! with the file's observed length and modification time; a foreign target the
//! engine read from disk decodes through its disk bytes only when the file was
//! last modified before the query was dispatched.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::SystemTime;

/// How an engine receives one file's bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeliveryOrder {
    /// A protocol frame carries the bytes, so the engine applies them at that
    /// frame's position in its input stream.
    Wire,
    /// The engine reads the bytes out of band while it evaluates, so no wire
    /// position orders them against a query.
    OutOfBand,
}

/// One file's bytes as one engine incarnation holds them.
#[derive(Clone, Debug)]
struct DeliveredBytes {
    bytes: Arc<str>,
    /// Minted from the ledger's own counter on every record, so a re-record of
    /// identical bytes is still a different delivery.
    stamp: u64,
    order: DeliveryOrder,
}

/// One change to what an engine holds, recorded when it takes effect.
#[derive(Clone, Debug)]
pub enum SurfaceEffect {
    /// The engine now holds `bytes` for `path`.
    Deliver { path: String, bytes: Arc<str> },
    /// The engine no longer holds an overlay for `path`.
    Withdraw { path: String },
}

impl SurfaceEffect {
    #[must_use]
    pub fn deliver(path: impl Into<String>, bytes: Arc<str>) -> Self {
        Self::Deliver {
            path: path.into(),
            bytes,
        }
    }

    #[must_use]
    pub fn withdraw(path: impl Into<String>) -> Self {
        Self::Withdraw { path: path.into() }
    }
}

/// Why a query's coordinates could not be bound to the bytes the engine
/// evaluated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConflictKind {
    /// The engine holds bytes for the file other than the ones the requester
    /// computed its position against, or will map the location through.
    IntendedSurface,
    /// The file's delivered bytes were replaced between the query's binding
    /// and its decode.
    Moved,
    /// The engine's publisher does not attest the retained bytes as published
    /// since before the query was dispatched.
    Publication,
    /// The file the engine reads from disk is not provably the bytes it held
    /// when the query was dispatched.
    Disk,
}

/// The typed outcome of a query whose coordinates cannot be bound to the bytes
/// the engine evaluated. No answer was decoded; the requester may bind a fresh
/// query against the surface it now holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderQueryConflict {
    path: String,
    kind: ConflictKind,
}

impl ProviderQueryConflict {
    fn new(path: &str, kind: ConflictKind) -> Self {
        Self {
            path: path.to_string(),
            kind,
        }
    }

    /// The file whose delivered bytes could not be bound.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Why it could not be bound.
    #[must_use]
    pub fn kind(&self) -> ConflictKind {
        self.kind
    }
}

impl std::fmt::Display for ProviderQueryConflict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let reason = match self.kind {
            ConflictKind::IntendedSurface => "the engine holds other bytes than the query intends",
            ConflictKind::Moved => "its delivered bytes moved under the query",
            ConflictKind::Publication => {
                "its publisher does not attest the bytes the query decoded through"
            }
            ConflictKind::Disk => "its disk bytes changed after the query was dispatched",
        };
        write!(
            f,
            "provider query coordinates conflict on {}: {reason}",
            self.path
        )
    }
}

// ── the requester-minted capability ─────────────────────────────────────────

/// The identity of the delivered provider surface a requester computed its
/// request position against, as its own surface store recorded it. Opaque to
/// the runtime: carried so the binding names exactly the surface the requester
/// will map the answer through.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DeliveredSurfaceId {
    pub generation: u64,
    pub content_epoch: u64,
    pub incarnation: u64,
}

/// The surface a query's request position was computed against.
#[derive(Clone, Debug)]
pub struct IntendedSurface {
    id: DeliveredSurfaceId,
    bytes: Arc<str>,
}

impl IntendedSurface {
    #[must_use]
    pub fn id(&self) -> DeliveredSurfaceId {
        self.id
    }

    #[must_use]
    pub fn bytes(&self) -> &Arc<str> {
        &self.bytes
    }
}

/// The surfaces a requester will map foreign answer locations through.
pub trait IntendedTargets: Send + Sync {
    /// The exact provider bytes a location in `path` will be interpreted
    /// against, or `None` when the requester captured no surface for `path`.
    fn intended(&self, path: &str) -> Option<Arc<str>>;
}

/// The serving incarnation and project a query was admitted under, stamped by
/// the hub and router that serve it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct QueryAdmission {
    /// The serving engine incarnation the query was dispatched to.
    pub incarnation: Option<verter_identity::identity::ProviderEpoch>,
    /// The configured project the query was admitted into.
    pub project: Option<Arc<str>>,
}

struct QueryRequest {
    path: Box<str>,
    intended: Option<IntendedSurface>,
    targets: Option<Arc<dyn IntendedTargets>>,
}

/// The capability one provider query carries from its requester through the
/// hub and router to the adapter that dispatches it. Cloning shares the
/// requester's intent; the admission is per clone, so a hub stamps its own
/// incarnation without disturbing the requester's handle. The adapter binds it
/// into a [`BoundQuery`] at its request frame's wire position.
#[derive(Clone)]
pub struct ProviderQuery {
    request: Arc<QueryRequest>,
    admission: QueryAdmission,
}

impl std::fmt::Debug for ProviderQuery {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderQuery")
            .field("path", &self.request.path)
            .field(
                "intended",
                &self.request.intended.as_ref().map(IntendedSurface::id),
            )
            .field("admission", &self.admission)
            .finish_non_exhaustive()
    }
}

impl ProviderQuery {
    /// A query whose request position was computed against `bytes`, the
    /// provider surface `id` the requester captured for `path`.
    #[must_use]
    pub fn intending(path: impl AsRef<str>, id: DeliveredSurfaceId, bytes: Arc<str>) -> Self {
        Self::new(
            path.as_ref().into(),
            Some(IntendedSurface { id, bytes }),
            None,
        )
    }

    /// A query whose requester captured no surface for `path`: it binds to the
    /// bytes the engine holds when its frame is placed, and its answer decodes
    /// through those.
    #[must_use]
    pub fn at_engine_surface(path: impl AsRef<str>) -> Self {
        Self::new(path.as_ref().into(), None, None)
    }

    fn new(
        path: Box<str>,
        intended: Option<IntendedSurface>,
        targets: Option<Arc<dyn IntendedTargets>>,
    ) -> Self {
        Self {
            request: Arc::new(QueryRequest {
                path,
                intended,
                targets,
            }),
            admission: QueryAdmission::default(),
        }
    }

    /// This query, mapping foreign locations through `targets`.
    #[must_use]
    pub fn with_targets(self, targets: Arc<dyn IntendedTargets>) -> Self {
        Self::new(
            self.request.path.clone(),
            self.request.intended.clone(),
            Some(targets),
        )
    }

    /// The file the request names.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.request.path
    }

    /// The surface the requester computed the request position against.
    #[must_use]
    pub fn intended(&self) -> Option<&IntendedSurface> {
        self.request.intended.as_ref()
    }

    /// The admission stamped on this query.
    #[must_use]
    pub fn admission(&self) -> &QueryAdmission {
        &self.admission
    }

    /// This query admitted to the serving `incarnation`.
    #[must_use]
    pub fn admitted_to(&self, incarnation: verter_identity::identity::ProviderEpoch) -> Self {
        let mut admitted = self.clone();
        admitted.admission.incarnation = Some(incarnation);
        admitted
    }

    /// This query admitted into `project`.
    #[must_use]
    pub fn admitted_into(&self, project: Arc<str>) -> Self {
        let mut admitted = self.clone();
        admitted.admission.project = Some(project);
        admitted
    }

    /// Check that `held` — the bytes an adapter without a delivery ledger is
    /// about to convert this query's request position against for `path` — is
    /// exactly the surface the requester intends.
    ///
    /// # Errors
    /// [`ConflictKind::IntendedSurface`] when the query intends other bytes.
    pub fn check_intended(
        &self,
        path: &str,
        held: Option<&Arc<str>>,
    ) -> Result<(), ProviderQueryConflict> {
        match self.intended() {
            Some(intended) if !held.is_some_and(|held| same_bytes(held, &intended.bytes)) => Err(
                ProviderQueryConflict::new(path, ConflictKind::IntendedSurface),
            ),
            _ => Ok(()),
        }
    }

    fn intended_target(&self, path: &str) -> Option<Arc<str>> {
        self.request.targets.as_ref()?.intended(path)
    }
}

// ── the engine's publisher ──────────────────────────────────────────────────

/// A publisher's commit position: its store instance and the commit counter
/// within it. Positions of one instance are totally ordered and never reused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublicationPosition {
    pub instance: Arc<str>,
    pub epoch: u64,
}

impl PublicationPosition {
    /// Whether a row published at `self` was already published at `observed`.
    #[must_use]
    pub fn at_or_before(&self, observed: &PublicationPosition) -> bool {
        self.instance == observed.instance && self.epoch <= observed.epoch
    }
}

/// What a publisher records for one file, checked against retained bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Attestation {
    /// Every row naming the file carries exactly the bytes; `published` is the
    /// latest position at which any of those rows was published with them.
    Attested(PublicationPosition),
    /// No row names the file: the engine reads it from disk.
    Unpublished,
    /// A row names the file with other bytes.
    Contradicted,
    /// The publisher's record cannot be read.
    Unreadable,
}

/// The authority that publishes the bytes an engine reads out of band — every
/// writer of the store the engine's plugin reads, not only this process.
pub trait SurfacePublications: Send + Sync {
    /// The publisher's current position, or `None` when nothing was published
    /// or its record cannot be read.
    fn position(&self) -> Option<PublicationPosition>;

    /// What the publisher records for `path` now, against `bytes`.
    fn attest(&self, path: &str, bytes: &str) -> Attestation;
}

// ── disk evidence ───────────────────────────────────────────────────────────

/// The length and modification time a file had when it was read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FileObservation {
    len: u64,
    modified: Option<SystemTime>,
}

impl FileObservation {
    fn of(path: &str) -> std::io::Result<Self> {
        let metadata = std::fs::metadata(path)?;
        Ok(Self {
            len: metadata.len(),
            modified: metadata.modified().ok(),
        })
    }

    /// Whether the file was last modified strictly before `at`.
    fn predates(&self, at: SystemTime) -> bool {
        self.modified.is_some_and(|modified| modified < at)
    }
}

/// One read of a file the engine reads from disk itself.
enum DiskRead {
    Missing,
    /// The file changed while it was read.
    Unstable,
    Read(Arc<str>, FileObservation),
}

fn read_disk(path: &str) -> DiskRead {
    let Ok(before) = FileObservation::of(path) else {
        return DiskRead::Missing;
    };
    let Ok(bytes) = std::fs::read_to_string(path) else {
        return DiskRead::Missing;
    };
    match FileObservation::of(path) {
        Ok(after) if after == before && after.len == bytes.len() as u64 => {
            DiskRead::Read(Arc::from(bytes), after)
        }
        _ => DiskRead::Unstable,
    }
}

fn same_bytes(left: &Arc<str>, right: &Arc<str>) -> bool {
    Arc::ptr_eq(left, right) || **left == **right
}

// ── the ledger ──────────────────────────────────────────────────────────────

#[derive(Default)]
struct LedgerState {
    files: imbl::HashMap<String, DeliveredBytes>,
    next_stamp: u64,
}

impl LedgerState {
    fn apply(&mut self, effect: SurfaceEffect, order: DeliveryOrder) {
        match effect {
            SurfaceEffect::Deliver { path, bytes } => {
                if order == DeliveryOrder::OutOfBand {
                    match self.files.get(&path) {
                        // A buffer the engine holds through its own protocol
                        // outranks bytes it would otherwise read out of band.
                        Some(held) if held.order == DeliveryOrder::Wire => return,
                        // Identical out-of-band bytes place every position the
                        // same, so they are the same delivery.
                        Some(held) if held.bytes == bytes => return,
                        _ => {}
                    }
                }
                self.next_stamp += 1;
                self.files.insert(
                    path,
                    DeliveredBytes {
                        bytes,
                        stamp: self.next_stamp,
                        order,
                    },
                );
            }
            SurfaceEffect::Withdraw { path } => {
                if order == DeliveryOrder::OutOfBand
                    && self
                        .files
                        .get(&path)
                        .is_some_and(|held| held.order == DeliveryOrder::Wire)
                {
                    return;
                }
                self.files.remove(&path);
            }
        }
    }
}

/// The bytes one query's request position was converted against.
#[derive(Clone, Debug)]
struct RequestedBytes {
    bytes: Option<Arc<str>>,
    /// The ledger entry those bytes came from; `None` when the engine was never
    /// handed the file and the bytes are its disk content.
    entry: Option<(u64, DeliveryOrder)>,
    /// The disk observation the fallback bytes were read with.
    disk: Option<FileObservation>,
}

/// A query prepared for dispatch: the evidence gathered before its frame is
/// placed, outside every lock.
pub struct PreparedQuery {
    query: ProviderQuery,
    path: String,
    fallback: Option<(Arc<str>, FileObservation)>,
    /// The publisher's position observed before dispatch.
    position: Option<PublicationPosition>,
    /// The local stamp of the out-of-band request entry the publisher attested.
    attested: Option<u64>,
}

/// Why a prepared query's frame was not placed.
pub enum DispatchRefusal<E> {
    /// The bytes the engine holds cannot be bound to the query.
    Conflict(ProviderQueryConflict),
    /// The frame builder declined or failed.
    Unplaced(E),
}

static NEXT_LEDGER: AtomicU64 = AtomicU64::new(1);

/// The delivered bytes of one engine incarnation, in the order the engine
/// receives them. Owned by that incarnation's transport and dropped with it, so
/// a replacement engine starts from an empty surface its replay refills.
pub struct DeliveryLedger {
    state: parking_lot::Mutex<LedgerState>,
    /// Distinct per ledger, so a binding names the engine incarnation it was
    /// dispatched to.
    incarnation: u64,
    /// The publisher of the bytes this engine reads out of band, when its
    /// topology has one.
    publications: Option<Arc<dyn SurfacePublications>>,
}

impl Default for DeliveryLedger {
    fn default() -> Self {
        Self::new(None)
    }
}

impl DeliveryLedger {
    /// A ledger whose out-of-band bytes are attested by `publications`, or by
    /// their local record alone when `None`.
    #[must_use]
    pub fn new(publications: Option<Arc<dyn SurfacePublications>>) -> Self {
        Self {
            state: parking_lot::Mutex::new(LedgerState::default()),
            incarnation: NEXT_LEDGER.fetch_add(1, Ordering::Relaxed),
            publications,
        }
    }

    /// Record `effects` that take effect at the engine at the current wire
    /// position, running `put_on_wire` (the synchronous enqueue of the frame
    /// that carries them) under the same lock, so no query can bind between
    /// the frame's position and the record.
    pub fn deliver_with<R>(
        &self,
        effects: impl IntoIterator<Item = SurfaceEffect>,
        put_on_wire: impl FnOnce() -> R,
    ) -> R {
        let mut state = self.state.lock();
        let placed = put_on_wire();
        for effect in effects {
            state.apply(effect, DeliveryOrder::Wire);
        }
        placed
    }

    /// Record effects whose frame position is the caller's current position in
    /// a writer that alone orders the wire.
    pub fn record_wire(&self, effect: SurfaceEffect) {
        self.state.lock().apply(effect, DeliveryOrder::Wire);
    }

    /// Record bytes the engine reads out of band, all under one lock so no
    /// query observes part of one publication. They carry no wire position; a
    /// query decoding through them settles against their evidence. An
    /// out-of-band record never replaces or withdraws a buffer the engine holds
    /// through its protocol, and re-recording identical bytes keeps the stamp.
    pub fn record_out_of_band(&self, effects: impl IntoIterator<Item = SurfaceEffect>) {
        let mut state = self.state.lock();
        for effect in effects {
            state.apply(effect, DeliveryOrder::OutOfBand);
        }
    }

    /// Whether the engine holds delivered bytes for `path`.
    #[must_use]
    pub fn holds(&self, path: &str) -> bool {
        self.state.lock().files.contains_key(path)
    }

    /// Gather `query`'s pre-dispatch evidence for a request on `path` (the
    /// adapter's key for the file): the file's disk bytes when the engine holds
    /// none, and — when the engine reads bytes out of band from a publisher —
    /// the publisher's position and its attestation of the request file.
    ///
    /// # Errors
    /// [`ProviderQueryConflict`] when the request file's disk bytes change
    /// while they are read, or the publisher contradicts the out-of-band bytes
    /// the request would convert against.
    pub fn prepare(
        &self,
        query: &ProviderQuery,
        path: &str,
    ) -> Result<PreparedQuery, ProviderQueryConflict> {
        let held = self.state.lock().files.get(path).cloned();
        let fallback = match &held {
            Some(_) => None,
            None => match read_disk(path) {
                DiskRead::Missing => None,
                DiskRead::Unstable => {
                    return Err(ProviderQueryConflict::new(path, ConflictKind::Disk))
                }
                DiskRead::Read(bytes, observed) => Some((bytes, observed)),
            },
        };
        let mut position = None;
        let mut attested = None;
        if let Some(publications) = &self.publications {
            if let Some(held) = held.filter(|held| held.order == DeliveryOrder::OutOfBand) {
                match publications.attest(path, &held.bytes) {
                    Attestation::Attested(_) | Attestation::Unpublished => {
                        attested = Some(held.stamp);
                    }
                    Attestation::Contradicted | Attestation::Unreadable => {
                        return Err(ProviderQueryConflict::new(path, ConflictKind::Publication));
                    }
                }
            }
            position = publications.position();
        }
        Ok(PreparedQuery {
            query: query.clone(),
            path: path.to_string(),
            fallback,
            position,
            attested,
        })
    }

    /// Bind a prepared query at its wire position: `put_on_wire` converts the
    /// request against the bytes the engine holds for the file (else its disk
    /// bytes) and synchronously enqueues the frame, all under the ledger lock,
    /// so the binding names exactly the surface that frame meets.
    ///
    /// # Errors
    /// [`DispatchRefusal::Conflict`] when the engine holds other bytes than the
    /// query intends, or an out-of-band request file moved after the publisher
    /// attested it; [`DispatchRefusal::Unplaced`] with whatever `put_on_wire`
    /// returns. No frame is placed and nothing is bound in either case.
    pub fn dispatch_with<R, E>(
        &self,
        prepared: PreparedQuery,
        put_on_wire: impl FnOnce(Option<&str>) -> Result<R, E>,
    ) -> Result<(BoundQuery, R), DispatchRefusal<E>> {
        let PreparedQuery {
            query,
            path,
            fallback,
            position,
            attested,
        } = prepared;
        let state = self.state.lock();
        let requested = match state.files.get(&path) {
            Some(entry) => {
                if entry.order == DeliveryOrder::OutOfBand
                    && self.publications.is_some()
                    && attested != Some(entry.stamp)
                {
                    return Err(DispatchRefusal::Conflict(ProviderQueryConflict::new(
                        &path,
                        ConflictKind::Moved,
                    )));
                }
                RequestedBytes {
                    bytes: Some(Arc::clone(&entry.bytes)),
                    entry: Some((entry.stamp, entry.order)),
                    disk: None,
                }
            }
            None => RequestedBytes {
                disk: fallback.as_ref().map(|(_, observed)| *observed),
                bytes: fallback.map(|(bytes, _)| bytes),
                entry: None,
            },
        };
        if let Some(intended) = query.intended() {
            if !requested
                .bytes
                .as_ref()
                .is_some_and(|held| same_bytes(held, &intended.bytes))
            {
                return Err(DispatchRefusal::Conflict(ProviderQueryConflict::new(
                    &path,
                    ConflictKind::IntendedSurface,
                )));
            }
        }
        let dispatched_at = SystemTime::now();
        let placed = put_on_wire(requested.bytes.as_deref()).map_err(DispatchRefusal::Unplaced)?;
        let surface = state.files.clone();
        drop(state);
        Ok((
            BoundQuery {
                engine: self.incarnation,
                query,
                path,
                requested,
                surface,
                dispatched_at,
                position,
            },
            placed,
        ))
    }

    /// Settle an answer decoded through `bound`: every out-of-band file it
    /// decoded through (the requested file and each of `decoded`) must still
    /// carry the local stamp it had at dispatch and, when the engine reads it
    /// from a publisher, be attested by the publisher with exactly the retained
    /// bytes at a publication no later than the position observed before
    /// dispatch — or, when no publisher names it, be unmodified on disk since
    /// before dispatch. An undelivered request file must still carry the disk
    /// bytes it was converted against. Wire-ordered bytes need no check: the
    /// engine evaluated exactly the bytes the binding retained.
    ///
    /// # Errors
    /// [`ProviderQueryConflict`] naming the first file whose evidence failed.
    pub fn settle<'a>(
        &self,
        bound: &BoundQuery,
        decoded: impl IntoIterator<Item = &'a str>,
    ) -> Result<(), ProviderQueryConflict> {
        if let Some(observed) = bound.requested.disk {
            if FileObservation::of(&bound.path).ok() != Some(observed) {
                return Err(ProviderQueryConflict::new(&bound.path, ConflictKind::Disk));
            }
        }
        let mut out_of_band: Vec<(&str, &DeliveredBytes)> = Vec::new();
        let requested_entry;
        if let (Some((stamp, DeliveryOrder::OutOfBand)), Some(bytes)) =
            (bound.requested.entry, &bound.requested.bytes)
        {
            requested_entry = DeliveredBytes {
                bytes: Arc::clone(bytes),
                stamp,
                order: DeliveryOrder::OutOfBand,
            };
            out_of_band.push((&bound.path, &requested_entry));
        }
        let mut seen = HashSet::new();
        for path in decoded {
            if path == bound.path || !seen.insert(path) {
                continue;
            }
            if let Some(entry) = bound.surface.get(path) {
                if entry.order == DeliveryOrder::OutOfBand {
                    out_of_band.push((path, entry));
                }
            }
        }
        if out_of_band.is_empty() {
            return Ok(());
        }
        {
            let state = self.state.lock();
            for (path, entry) in &out_of_band {
                if state.files.get(*path).map(|held| held.stamp) != Some(entry.stamp) {
                    return Err(ProviderQueryConflict::new(path, ConflictKind::Moved));
                }
            }
        }
        let Some(publications) = &self.publications else {
            return Ok(());
        };
        for (path, entry) in out_of_band {
            match publications.attest(path, &entry.bytes) {
                Attestation::Attested(published)
                    if bound
                        .position
                        .as_ref()
                        .is_some_and(|observed| published.at_or_before(observed)) => {}
                Attestation::Unpublished => match read_disk(path) {
                    DiskRead::Read(bytes, observed)
                        if observed.predates(bound.dispatched_at)
                            && same_bytes(&bytes, &entry.bytes) => {}
                    _ => return Err(ProviderQueryConflict::new(path, ConflictKind::Disk)),
                },
                _ => return Err(ProviderQueryConflict::new(path, ConflictKind::Publication)),
            }
        }
        Ok(())
    }
}

/// One query bound at its request frame's wire position: the bytes its
/// request position was converted against and the immutable delivered surface
/// its frame met. Every response range decodes through it.
#[derive(Debug)]
pub struct BoundQuery {
    /// The delivery ledger — one per engine incarnation — it was bound on.
    engine: u64,
    query: ProviderQuery,
    path: String,
    requested: RequestedBytes,
    surface: imbl::HashMap<String, DeliveredBytes>,
    /// Taken under the ledger lock before the frame was placed.
    dispatched_at: SystemTime,
    position: Option<PublicationPosition>,
}

impl BoundQuery {
    /// The engine incarnation (its delivery ledger) this query was bound on.
    #[must_use]
    pub fn engine(&self) -> u64 {
        self.engine
    }

    /// The capability this binding was made for, with the admission it was
    /// dispatched under.
    #[must_use]
    pub fn query(&self) -> &ProviderQuery {
        &self.query
    }

    /// The bytes the request position was converted against, retained for the
    /// decode of every range in the same file.
    #[must_use]
    pub fn requested(&self) -> Option<&Arc<str>> {
        self.requested.bytes.as_ref()
    }

    /// The bytes a location in `path` decodes through: the request file's
    /// retained bytes, another file's bytes as the request frame met them, or
    /// — for a file the engine reads from disk — its disk bytes when the file
    /// was last modified before the query was dispatched. `intended_as` is the
    /// identity the requester maps that location under; when the requester
    /// captured a surface for it, the decode bytes must be exactly that
    /// surface's. `Ok(None)` for a target with no bytes at all: its locations
    /// cannot be decoded and are dropped, never given fabricated offsets.
    ///
    /// # Errors
    /// [`ProviderQueryConflict`] when the target's disk bytes changed after
    /// dispatch, or the decode bytes are not the requester's intended surface.
    pub fn target(
        &self,
        path: &str,
        intended_as: &str,
    ) -> Result<Option<Arc<str>>, ProviderQueryConflict> {
        let bytes = if path == self.path {
            self.requested.bytes.clone()
        } else if let Some(entry) = self.surface.get(path) {
            Some(Arc::clone(&entry.bytes))
        } else {
            match read_disk(path) {
                DiskRead::Missing => None,
                DiskRead::Read(bytes, observed) if observed.predates(self.dispatched_at) => {
                    Some(bytes)
                }
                DiskRead::Read(..) | DiskRead::Unstable => {
                    return Err(ProviderQueryConflict::new(path, ConflictKind::Disk))
                }
            }
        };
        if let Some(intended) = self.query.intended_target(intended_as) {
            if !bytes
                .as_ref()
                .is_some_and(|bytes| same_bytes(bytes, &intended))
            {
                return Err(ProviderQueryConflict::new(
                    intended_as,
                    ConflictKind::IntendedSurface,
                ));
            }
        }
        Ok(bytes)
    }

    /// [`Self::target`] for exactly `paths`, keyed for decoders that take a
    /// path-keyed map; a target with no bytes is omitted and its locations are
    /// dropped by the decoder.
    ///
    /// # Errors
    /// The first target's [`ProviderQueryConflict`].
    pub fn targets(
        &self,
        paths: &HashSet<String>,
        intended_as: impl Fn(&str) -> String,
    ) -> Result<HashMap<String, Arc<str>>, ProviderQueryConflict> {
        let mut resolved = HashMap::with_capacity(paths.len());
        for path in paths {
            if let Some(bytes) = self.target(path, &intended_as(path))? {
                resolved.insert(path.clone(), bytes);
            }
        }
        Ok(resolved)
    }
}

#[cfg(test)]
#[path = "provider_query_tests.rs"]
mod tests;

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
//! evaluates, or an overlay another channel injects) have no wire position.
//! They are recorded as [`DeliveryOrder::OutOfBand`] once the engine is known
//! to hold them; while a delivery is still in flight the file is recorded as
//! unknown ([`SurfaceEffect::unsettle`]) and nothing binds or decodes through
//! it. When the engine's publisher is known ([`SurfacePublications`]), the
//! engine holds a publication only once it has adopted it: the ledger records,
//! at the wire position of the frame that makes the engine re-read its
//! publisher, the publisher position observed before that frame
//! ([`DeliveryLedger::adopt_with`]). A query binds an out-of-band file only
//! while the publisher attests exactly the retained bytes at a publication the
//! engine has adopted, and once the answer is in every out-of-band file it
//! decoded through must still carry its local stamp and that attestation
//! ([`DeliveryLedger::settle`]). A row withdrawn or never published leaves the
//! engine reading the file itself, which is never delivery evidence.
//!
//! A file the engine was never handed — a closed workspace file, a library
//! declaration — is read by the engine itself, from disk or its own bundle,
//! with no wire position and no snapshot. Neither a read of the file nor its
//! timestamps identify the bytes the engine evaluated, so a request on such a
//! file, and an answer locating anything in one, is a
//! [`ProviderQueryConflict`] ([`ConflictKind::Undelivered`]) — never a decode
//! against bytes the engine may not have evaluated.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

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
    /// A delivery of the file is in flight, so which bytes the engine holds is
    /// unknown; `bytes` is meaningless until its outcome is recorded.
    in_flight: bool,
}

/// One change to what an engine holds, recorded when it takes effect.
#[derive(Clone, Debug)]
pub enum SurfaceEffect {
    /// The engine now holds `bytes` for `path`.
    Deliver { path: String, bytes: Arc<str> },
    /// The engine no longer holds an overlay for `path`.
    Withdraw { path: String },
    /// A delivery for `path` may reach the engine from now on, through a
    /// channel the ledger cannot order: until its outcome is recorded the
    /// engine may hold either the old or the new bytes.
    Unsettle { path: String },
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

    #[must_use]
    pub fn unsettle(path: impl Into<String>) -> Self {
        Self::Unsettle { path: path.into() }
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
    /// The engine's publisher does not attest the retained bytes as a
    /// publication the engine had adopted when the query was dispatched.
    Publication,
    /// The engine was never handed the file: it reads the file itself, so the
    /// bytes it evaluated cannot be bound.
    Undelivered,
    /// A delivery of the file was in flight, so which bytes the engine holds
    /// is unknown.
    InFlight,
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
    #[must_use]
    pub fn new(path: &str, kind: ConflictKind) -> Self {
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
            ConflictKind::Undelivered => "the engine was never handed it",
            ConflictKind::InFlight => "a delivery of it is in flight",
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

    /// Check that `held` — the bytes a returned location in `path` is about to
    /// be decoded through — is exactly the surface the requester will map that
    /// location through. A target the requester captured no surface for
    /// decodes through whatever bytes are held.
    ///
    /// # Errors
    /// [`ConflictKind::IntendedSurface`] when the requester intends other bytes.
    pub fn check_intended_target(
        &self,
        path: &str,
        held: Option<&Arc<str>>,
    ) -> Result<(), ProviderQueryConflict> {
        match self
            .request
            .targets
            .as_ref()
            .and_then(|targets| targets.intended(path))
        {
            Some(intended) if !held.is_some_and(|held| same_bytes(held, &intended)) => Err(
                ProviderQueryConflict::new(path, ConflictKind::IntendedSurface),
            ),
            _ => Ok(()),
        }
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

impl Attestation {
    /// Whether the publisher attests exactly the retained bytes at a
    /// publication the engine had adopted at `adopted`.
    fn adopted_at(&self, adopted: Option<&PublicationPosition>) -> bool {
        match self {
            Self::Attested(published) => adopted.is_some_and(|at| published.at_or_before(at)),
            Self::Unpublished | Self::Contradicted | Self::Unreadable => false,
        }
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
    /// The latest publisher position the engine has adopted: every
    /// publication and withdrawal at or before it is what the engine reads.
    adopted: Option<PublicationPosition>,
}

impl LedgerState {
    fn stamp(&mut self) -> u64 {
        self.next_stamp += 1;
        self.next_stamp
    }

    fn apply(&mut self, effect: SurfaceEffect, order: DeliveryOrder) {
        let path = match &effect {
            SurfaceEffect::Deliver { path, .. }
            | SurfaceEffect::Withdraw { path }
            | SurfaceEffect::Unsettle { path } => path,
        };
        let held = self.files.get(path);
        if order == DeliveryOrder::OutOfBand {
            match (held, &effect) {
                // A buffer the engine holds through its own protocol outranks
                // bytes it would otherwise read out of band.
                (Some(held), _) if held.order == DeliveryOrder::Wire => return,
                // Identical out-of-band bytes place every position the same,
                // so they are the same delivery.
                (Some(held), SurfaceEffect::Deliver { bytes, .. })
                    if !held.in_flight && held.bytes == *bytes =>
                {
                    return
                }
                _ => {}
            }
        }
        match effect {
            SurfaceEffect::Deliver { path, bytes } => {
                let stamp = self.stamp();
                self.files.insert(
                    path,
                    DeliveredBytes {
                        bytes,
                        stamp,
                        order,
                        in_flight: false,
                    },
                );
            }
            SurfaceEffect::Unsettle { path } => {
                let stamp = self.stamp();
                self.files.insert(
                    path,
                    DeliveredBytes {
                        bytes: Arc::from(""),
                        stamp,
                        order,
                        in_flight: true,
                    },
                );
            }
            SurfaceEffect::Withdraw { path } => {
                self.files.remove(&path);
            }
        }
    }
}

/// The bytes one query's request position was converted against.
#[derive(Clone, Debug)]
struct RequestedBytes {
    bytes: Arc<str>,
    /// The local stamp of the ledger entry those bytes came from.
    stamp: u64,
    order: DeliveryOrder,
}

/// A query prepared for dispatch: the evidence gathered before its frame is
/// placed, outside every lock.
pub struct PreparedQuery {
    query: ProviderQuery,
    path: String,
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
    ///
    /// Construct it before the engine can read anything from `publications`
    /// (before its process starts): the publisher position observed here is
    /// what the engine adopts by reading lazily, since every row published no
    /// later than it is already in place when the engine first reads it.
    #[must_use]
    pub fn new(publications: Option<Arc<dyn SurfacePublications>>) -> Self {
        let adopted = publications
            .as_ref()
            .and_then(|publications| publications.position());
        Self {
            state: parking_lot::Mutex::new(LedgerState {
                adopted,
                ..LedgerState::default()
            }),
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

    /// Record what the engine holds out of band, all under one lock so no
    /// query observes part of one change: bytes it is known to hold, a file it
    /// no longer holds, or a delivery whose outcome is not yet known
    /// ([`SurfaceEffect::unsettle`]). They carry no wire position; a query
    /// decoding through them settles against their evidence. An out-of-band
    /// record never replaces or withdraws a buffer the engine holds through its
    /// protocol, and re-recording identical bytes keeps the stamp.
    pub fn record_out_of_band(&self, effects: impl IntoIterator<Item = SurfaceEffect>) {
        let mut state = self.state.lock();
        for effect in effects {
            state.apply(effect, DeliveryOrder::OutOfBand);
        }
    }

    /// The publisher's current position, observed before a frame that makes
    /// the engine re-read its publisher is placed; `None` without a publisher.
    #[must_use]
    pub fn publication_position(&self) -> Option<PublicationPosition> {
        self.publications.as_ref()?.position()
    }

    /// Record that the engine has adopted every publication up to `observed`
    /// (a [`Self::publication_position`] taken before the frame that makes the
    /// engine re-read its publisher was placed) at the wire position of the
    /// frame `put_on_wire` synchronously enqueues — the first frame the engine
    /// evaluates after that re-read completes — under the ledger lock.
    pub fn adopt_with<R>(
        &self,
        observed: Option<PublicationPosition>,
        put_on_wire: impl FnOnce() -> R,
    ) -> R {
        let mut state = self.state.lock();
        let placed = put_on_wire();
        if let Some(observed) = observed {
            let newer = state
                .adopted
                .as_ref()
                .is_none_or(|adopted| !observed.at_or_before(adopted));
            if newer {
                state.adopted = Some(observed);
            }
        }
        placed
    }

    /// Whether the engine holds delivered bytes for `path`.
    #[must_use]
    pub fn holds(&self, path: &str) -> bool {
        self.state.lock().files.contains_key(path)
    }

    /// Gather `query`'s pre-dispatch evidence for a request on `path` (the
    /// adapter's key for the file): when the engine reads the file's bytes out
    /// of band from a publisher, the publisher's attestation of them against
    /// the publications the engine has adopted.
    ///
    /// # Errors
    /// [`ProviderQueryConflict`] when the engine was never handed the request
    /// file, a delivery of it is in flight, or the publisher's record of its
    /// out-of-band bytes is not one the engine has adopted.
    pub fn prepare(
        &self,
        query: &ProviderQuery,
        path: &str,
    ) -> Result<PreparedQuery, ProviderQueryConflict> {
        let (held, adopted) = {
            let state = self.state.lock();
            (state.files.get(path).cloned(), state.adopted.clone())
        };
        let Some(held) = held else {
            return Err(ProviderQueryConflict::new(path, ConflictKind::Undelivered));
        };
        if held.in_flight {
            return Err(ProviderQueryConflict::new(path, ConflictKind::InFlight));
        }
        let mut attested = None;
        if let Some(publications) = &self.publications {
            if held.order == DeliveryOrder::OutOfBand {
                if !publications
                    .attest(path, &held.bytes)
                    .adopted_at(adopted.as_ref())
                {
                    return Err(ProviderQueryConflict::new(path, ConflictKind::Publication));
                }
                attested = Some(held.stamp);
            }
        }
        Ok(PreparedQuery {
            query: query.clone(),
            path: path.to_string(),
            attested,
        })
    }

    /// Bind a prepared query at its wire position: `put_on_wire` converts the
    /// request against the bytes the engine holds for the file and
    /// synchronously enqueues the frame, all under the ledger lock, so the
    /// binding names exactly the surface that frame meets.
    ///
    /// # Errors
    /// [`DispatchRefusal::Conflict`] when the engine no longer holds the file,
    /// holds other bytes than the query intends, a delivery of the file is in
    /// flight, or an out-of-band request file moved after the publisher
    /// attested it; [`DispatchRefusal::Unplaced`] with whatever `put_on_wire`
    /// returns. No frame is placed and nothing is bound in either case.
    pub fn dispatch_with<R, E>(
        &self,
        prepared: PreparedQuery,
        put_on_wire: impl FnOnce(&str) -> Result<R, E>,
    ) -> Result<(BoundQuery, R), DispatchRefusal<E>> {
        let PreparedQuery {
            query,
            path,
            attested,
        } = prepared;
        let state = self.state.lock();
        let refuse = |kind| DispatchRefusal::Conflict(ProviderQueryConflict::new(&path, kind));
        let Some(entry) = state.files.get(&path) else {
            return Err(refuse(ConflictKind::Undelivered));
        };
        if entry.in_flight {
            return Err(refuse(ConflictKind::InFlight));
        }
        if entry.order == DeliveryOrder::OutOfBand
            && self.publications.is_some()
            && attested != Some(entry.stamp)
        {
            return Err(refuse(ConflictKind::Moved));
        }
        if query
            .intended()
            .is_some_and(|intended| !same_bytes(&entry.bytes, &intended.bytes))
        {
            return Err(refuse(ConflictKind::IntendedSurface));
        }
        let requested = RequestedBytes {
            bytes: Arc::clone(&entry.bytes),
            stamp: entry.stamp,
            order: entry.order,
        };
        let placed = put_on_wire(&requested.bytes).map_err(DispatchRefusal::Unplaced)?;
        let surface = state.files.clone();
        let adopted = state.adopted.clone();
        drop(state);
        Ok((
            BoundQuery {
                engine: self.incarnation,
                query,
                path,
                requested,
                surface,
                adopted,
            },
            placed,
        ))
    }

    /// Settle an answer decoded through `bound`: every out-of-band file it
    /// decoded through (the requested file and each of `decoded`) must still
    /// carry the local stamp it had at dispatch and, when the engine reads it
    /// from a publisher, be attested by the publisher with exactly the retained
    /// bytes as a publication the engine had adopted at dispatch. Wire-ordered
    /// bytes need no check: the engine evaluated exactly the bytes the binding
    /// retained. Nothing decodes through a file the engine was never handed
    /// ([`BoundQuery::target`] refuses it).
    ///
    /// # Errors
    /// [`ProviderQueryConflict`] naming the first file whose evidence failed.
    pub fn settle<'a>(
        &self,
        bound: &BoundQuery,
        decoded: impl IntoIterator<Item = &'a str>,
    ) -> Result<(), ProviderQueryConflict> {
        let mut out_of_band: Vec<(&str, u64, &Arc<str>)> = Vec::new();
        if bound.requested.order == DeliveryOrder::OutOfBand {
            out_of_band.push((&bound.path, bound.requested.stamp, &bound.requested.bytes));
        }
        let mut seen = HashSet::new();
        for path in decoded {
            if path == bound.path || !seen.insert(path) {
                continue;
            }
            if let Some(entry) = bound.surface.get(path) {
                if entry.order == DeliveryOrder::OutOfBand {
                    out_of_band.push((path, entry.stamp, &entry.bytes));
                }
            }
        }
        if out_of_band.is_empty() {
            return Ok(());
        }
        {
            let state = self.state.lock();
            for (path, stamp, _) in &out_of_band {
                if state.files.get(*path).map(|held| held.stamp) != Some(*stamp) {
                    return Err(ProviderQueryConflict::new(path, ConflictKind::Moved));
                }
            }
        }
        let Some(publications) = &self.publications else {
            return Ok(());
        };
        for (path, _, bytes) in out_of_band {
            if !publications
                .attest(path, bytes)
                .adopted_at(bound.adopted.as_ref())
            {
                return Err(ProviderQueryConflict::new(path, ConflictKind::Publication));
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
    /// The publisher position the engine had adopted when the frame was
    /// placed.
    adopted: Option<PublicationPosition>,
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
    pub fn requested(&self) -> &Arc<str> {
        &self.requested.bytes
    }

    /// The bytes a location in `path` decodes through: the request file's
    /// retained bytes, or another file's bytes as the request frame met them.
    /// `intended_as` is the identity the requester maps that location under;
    /// when the requester captured a surface for it, the decode bytes must be
    /// exactly that surface's.
    ///
    /// # Errors
    /// [`ProviderQueryConflict`] when the engine was never handed the target
    /// (it read the file itself), a delivery of it was in flight at dispatch,
    /// or the decode bytes are not the requester's intended surface.
    pub fn target(&self, path: &str, intended_as: &str) -> Result<Arc<str>, ProviderQueryConflict> {
        let bytes = if path == self.path {
            Arc::clone(&self.requested.bytes)
        } else {
            match self.surface.get(path) {
                Some(entry) if entry.in_flight => {
                    return Err(ProviderQueryConflict::new(path, ConflictKind::InFlight))
                }
                Some(entry) => Arc::clone(&entry.bytes),
                None => return Err(ProviderQueryConflict::new(path, ConflictKind::Undelivered)),
            }
        };
        self.query
            .check_intended_target(intended_as, Some(&bytes))?;
        Ok(bytes)
    }

    /// [`Self::target`] for exactly `paths`, keyed for decoders that take a
    /// path-keyed map.
    ///
    /// # Errors
    /// The first target's [`ProviderQueryConflict`].
    pub fn targets(
        &self,
        paths: &HashSet<String>,
        intended_as: impl Fn(&str) -> String,
    ) -> Result<HashMap<String, Arc<str>>, ProviderQueryConflict> {
        paths
            .iter()
            .map(|path| Ok((path.clone(), self.target(path, &intended_as(path))?)))
            .collect()
    }
}

#[cfg(test)]
#[path = "provider_query_tests.rs"]
mod tests;

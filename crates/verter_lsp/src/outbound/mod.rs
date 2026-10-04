//! Bounded server→client transport policy.
//!
//! Every byte the server sends reaches the editor through tower-lsp's single
//! writer, which interleaves two inputs round-robin: the RESPONSE lane (handler
//! results — at most [`crate::LSP_MAX_CONCURRENCY`] in flight, including the
//! replies to `shutdown` and the `RequestCancelled` errors `$/cancelRequest`
//! produces) and the client channel carrying server-initiated messages.
//! Server-initiated traffic is accounted in two separate classes:
//!
//! * CONTROL — server→client requests and ordered notifications (progress,
//!   messages, lifecycle signals). Never coalesced, reordered or dropped. A
//!   producer that awaits its own send holds at most one message in the client
//!   channel; a producer that cannot await (a synchronous callback) hands its
//!   messages to a [`ControlLane`], which sends them in order one at a time and
//!   accounts every message it holds.
//! * REPLACEABLE — `textDocument/publishDiagnostics`, which replaces a document's
//!   whole diagnostic set. It flows only through [`ReplaceableLane`], never as
//!   split append-like chunks.
//!
//! The catalogued envelope of the replaceable class, for a lane built with
//! budget `B`:
//!
//! * the lane owns every payload offered to it, from the moment it is offered
//!   until the transport accepts it or it is retired, and [`LaneLoad`] reports
//!   all of them: ADMITTED payloads (pending or handed to the transport) and
//!   WAITING ones (offered while the admitted set was full);
//! * at most `B.replaceable_messages` payloads and `B.replaceable_bytes`
//!   serialized bytes are admitted. The one exception is a single payload larger
//!   than the byte budget, which is admitted alone and delivered whole — a
//!   budget never truncates a complete result;
//! * at most one payload per document is admitted or waiting; a newer
//!   publication for the same document retires the older payload at once,
//!   whether it was pending or waiting, and an older one never overtakes a newer
//!   one. Retained payloads are therefore bounded by the admitted budget plus
//!   one waiting payload per document with a live publication;
//! * waiting payloads are admitted strictly in the order they were offered, so a
//!   payload that needs more room than is free (a large one needs the lane to
//!   itself) is never overtaken by smaller ones offered after it;
//! * the transport itself holds at most two replaceable payloads: one lane pump
//!   awaits each send, so only the channel's buffered slot and the pump's own
//!   parked send can be occupied. A slow client therefore delays responses by
//!   at most that bounded backlog, never by the backlog of edits behind it.
//!
//! Withdrawing a waiting or pending publication (dropping its future) removes its
//! payload: it never reaches the client. A payload the pump has already handed to
//! the transport is committed — tower's client channel enqueues a message the
//! moment its send is first polled and offers no way to take it back — so it
//! still lands, always ahead of any newer payload for the same document.
//!
//! A lane's transport task lives only as long as some handle to the lane does:
//! dropping the last handle ends a pump parked on a slow client and releases the
//! payload and client it holds.

mod control;

pub use control::ControlLane;

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;
use serde::Serialize;
use tokio::sync::{oneshot, Notify};
use tower_lsp_server::ls_types::notification::PublishDiagnostics;
use tower_lsp_server::ls_types::PublishDiagnosticsParams;
use tower_lsp_server::Client;

/// Limits of the replaceable-notification class. Control and response traffic
/// are accounted separately (see the module docs) and never share this budget,
/// so a diagnostics backlog cannot consume their capacity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutboundBudget {
    /// Payloads admitted (pending or handed to the transport), across every
    /// document.
    pub replaceable_messages: usize,
    /// Serialized JSON bytes of those payloads.
    pub replaceable_bytes: usize,
}

impl OutboundBudget {
    /// The production envelope: room for one pending publication per open
    /// document in any realistic session, and a byte ceiling well above a
    /// heavily diagnosed large component.
    pub const DEFAULT: Self = Self {
        replaceable_messages: 256,
        replaceable_bytes: 16 * 1024 * 1024,
    };
}

/// A count of messages and their serialized bytes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Load {
    pub messages: usize,
    pub bytes: usize,
}

impl Load {
    fn add(&mut self, bytes: usize) {
        self.messages += 1;
        self.bytes += bytes;
    }

    fn remove(&mut self, bytes: usize) {
        self.messages -= 1;
        self.bytes -= bytes;
    }

    fn max(self, other: Self) -> Self {
        Self {
            messages: self.messages.max(other.messages),
            bytes: self.bytes.max(other.bytes),
        }
    }
}

/// Every payload the replaceable lane owns.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LaneLoad {
    /// Pending payloads plus the one handed to the transport and not yet
    /// accepted by it — the set the budget bounds.
    pub admitted: Load,
    /// Payloads offered while the admitted set was full, at most one per
    /// document.
    pub waiting: Load,
}

impl LaneLoad {
    /// Everything the lane retains.
    pub fn retained(&self) -> Load {
        Load {
            messages: self.admitted.messages + self.waiting.messages,
            bytes: self.admitted.bytes + self.waiting.bytes,
        }
    }
}

/// The largest loads a lane has retained since it was built.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LaneHighWater {
    /// The largest admitted set.
    pub admitted: Load,
    /// The largest retained set, admitted and waiting together.
    pub retained: Load,
}

/// How one publication left the lane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Delivery {
    /// The transport accepted the payload.
    Delivered,
    /// A newer publication for the same document replaced it, or arrived first;
    /// this payload never reaches the client.
    Superseded,
}

/// The per-document versioned replacement coalescer for `publishDiagnostics`.
#[derive(Clone)]
pub struct ReplaceableLane {
    inner: Arc<Inner>,
    _owner: Arc<Owner>,
}

struct Inner {
    budget: OutboundBudget,
    state: Mutex<LaneState>,
    retirement: Arc<Retirement>,
}

#[derive(Default)]
struct LaneState {
    /// Admitted payloads, oldest first; the pump sends from the front.
    pending: VecDeque<Entry>,
    /// Payloads not yet admitted, in the order they were offered.
    waiting: VecDeque<Entry>,
    /// Serialized size of the payload the pump has handed to the transport.
    in_flight: Option<usize>,
    load: LaneLoad,
    /// Ordering state for every document with a live offer. An entry lives
    /// exactly as long as some publication for that document is inside
    /// [`ReplaceableLane::publish_diagnostics`].
    documents: HashMap<String, DocumentOffers>,
    next_ticket: u64,
    pump_running: bool,
    high_water: LaneHighWater,
}

struct DocumentOffers {
    newest_epoch: u64,
    live: usize,
}

struct Entry {
    key: String,
    ticket: u64,
    bytes: usize,
    params: PublishDiagnosticsParams,
    /// Fired once the transport accepts the payload; dropped (resolving the
    /// publisher as superseded) when a newer payload retires this one.
    delivered: oneshot::Sender<()>,
}

impl LaneState {
    /// Whether a new `bytes`-sized payload fits the admitted set. A payload that
    /// would be alone in it always fits.
    fn admits(&self, budget: &OutboundBudget, bytes: usize) -> bool {
        let admitted = self.load.admitted;
        admitted.messages == 0
            || (admitted.messages < budget.replaceable_messages
                && admitted.bytes + bytes <= budget.replaceable_bytes)
    }

    /// Whether a `bytes`-sized payload fits in place of an admitted one of
    /// `replacing` bytes.
    fn admits_in_place(&self, budget: &OutboundBudget, bytes: usize, replacing: usize) -> bool {
        let admitted = self.load.admitted;
        admitted.messages == 1 || admitted.bytes - replacing + bytes <= budget.replaceable_bytes
    }

    /// Take a newly offered payload, retiring any older payload of the same
    /// document first. A newer payload keeps the place of the one it retires
    /// when it fits there; a payload that no longer fits the admitted set heads
    /// the waiting line, ahead of payloads offered after its predecessor was
    /// admitted.
    fn offer(&mut self, budget: &OutboundBudget, entry: Entry) {
        if let Some(index) = self.pending.iter().position(|p| p.key == entry.key) {
            let replacing = self.pending[index].bytes;
            if self.admits_in_place(budget, entry.bytes, replacing) {
                self.load.admitted.bytes = self.load.admitted.bytes - replacing + entry.bytes;
                // Dropping the replaced entry resolves its publisher as superseded.
                drop(std::mem::replace(&mut self.pending[index], entry));
                return;
            }
            let retired = self.pending.remove(index).expect("index was just found");
            self.load.admitted.remove(retired.bytes);
            self.load.waiting.add(entry.bytes);
            self.waiting.push_front(entry);
            return;
        }
        if let Some(index) = self.waiting.iter().position(|p| p.key == entry.key) {
            let replacing = self.waiting[index].bytes;
            self.load.waiting.bytes = self.load.waiting.bytes - replacing + entry.bytes;
            drop(std::mem::replace(&mut self.waiting[index], entry));
            return;
        }
        self.load.waiting.add(entry.bytes);
        self.waiting.push_back(entry);
    }

    /// Admit waiting payloads in offer order while the next one fits.
    fn admit_waiting(&mut self, budget: &OutboundBudget) {
        while let Some(next) = self.waiting.front() {
            if !self.admits(budget, next.bytes) {
                break;
            }
            let next = self.waiting.pop_front().expect("front was just seen");
            self.load.waiting.remove(next.bytes);
            self.load.admitted.add(next.bytes);
            self.pending.push_back(next);
        }
        self.high_water.admitted = self.high_water.admitted.max(self.load.admitted);
        self.high_water.retained = self.high_water.retained.max(self.load.retained());
    }

    /// Remove a pending or waiting payload; `false` if it is no longer held.
    fn withdraw(&mut self, ticket: u64) -> bool {
        if let Some(index) = self.pending.iter().position(|p| p.ticket == ticket) {
            let withdrawn = self.pending.remove(index).expect("index was just found");
            self.load.admitted.remove(withdrawn.bytes);
            return true;
        }
        if let Some(index) = self.waiting.iter().position(|p| p.ticket == ticket) {
            let withdrawn = self.waiting.remove(index).expect("index was just found");
            self.load.waiting.remove(withdrawn.bytes);
            return true;
        }
        false
    }
}

/// One publication's presence in the lane. Dropping it — the publisher returned
/// or its future was cancelled — withdraws a still-pending or waiting payload and
/// releases the document's ordering entry.
struct Offer<'a> {
    inner: &'a Inner,
    key: String,
    ticket: Option<u64>,
}

impl Drop for Offer<'_> {
    fn drop(&mut self) {
        let mut state = self.inner.state.lock();
        if let Some(ticket) = self.ticket.take() {
            if state.withdraw(ticket) {
                state.admit_waiting(&self.inner.budget);
            }
        }
        if let Some(document) = state.documents.get_mut(&self.key) {
            document.live -= 1;
            if document.live == 0 {
                state.documents.remove(&self.key);
            }
        }
    }
}

impl ReplaceableLane {
    pub fn new(budget: OutboundBudget) -> Self {
        let retirement = Arc::new(Retirement::default());
        Self {
            inner: Arc::new(Inner {
                budget,
                state: Mutex::new(LaneState::default()),
                retirement: Arc::clone(&retirement),
            }),
            _owner: Arc::new(Owner(retirement)),
        }
    }

    pub fn budget(&self) -> OutboundBudget {
        self.inner.budget
    }

    /// What the lane retains right now.
    pub fn load(&self) -> LaneLoad {
        self.inner.state.lock().load
    }

    /// The largest loads the lane has retained since it was built.
    pub fn high_water(&self) -> LaneHighWater {
        self.inner.state.lock().high_water
    }

    /// Publish one document's complete diagnostic set at `epoch`, a value that
    /// increases with every newer publication of that document.
    ///
    /// Resolves once the transport accepts the payload, or as soon as a newer
    /// publication for the same document supersedes it. Dropping the future
    /// withdraws a payload the pump has not yet taken.
    pub async fn publish_diagnostics(
        &self,
        client: &Client,
        epoch: u64,
        params: PublishDiagnosticsParams,
    ) -> Delivery {
        let key = params.uri.as_str().to_owned();
        let bytes = serialized_len(&params);
        let (ticket, delivered) = {
            let mut state = self.inner.state.lock();
            let document = state
                .documents
                .entry(key.clone())
                .or_insert(DocumentOffers {
                    newest_epoch: epoch,
                    live: 0,
                });
            if epoch < document.newest_epoch {
                return Delivery::Superseded;
            }
            document.newest_epoch = epoch;
            document.live += 1;
            let (delivered_tx, delivered_rx) = oneshot::channel();
            let ticket = state.next_ticket;
            state.next_ticket += 1;
            state.offer(
                &self.inner.budget,
                Entry {
                    key: key.clone(),
                    ticket,
                    bytes,
                    params,
                    delivered: delivered_tx,
                },
            );
            state.admit_waiting(&self.inner.budget);
            if !state.pump_running && !state.pending.is_empty() {
                state.pump_running = true;
                tokio::spawn(pump(Arc::clone(&self.inner), client.clone()));
            }
            (ticket, delivered_rx)
        };
        let mut offer = Offer {
            inner: &self.inner,
            key,
            ticket: Some(ticket),
        };
        let outcome = match delivered.await {
            Ok(()) => Delivery::Delivered,
            Err(_) => Delivery::Superseded,
        };
        // Delivered or retired: nothing of this offer is held any more.
        offer.ticket = None;
        outcome
    }
}

/// Hand pending payloads to the transport one at a time, oldest first, until the
/// lane is empty or retired. Awaiting each send is what keeps the transport's
/// share of the replaceable class at two payloads.
async fn pump(inner: Arc<Inner>, client: Client) {
    loop {
        let next = {
            let mut state = inner.state.lock();
            let Some(next) = state.pending.pop_front() else {
                state.pump_running = false;
                return;
            };
            state.in_flight = Some(next.bytes);
            next
        };
        tokio::select! {
            biased;
            () = inner.retirement.retired() => return,
            () = client.send_notification::<PublishDiagnostics>(next.params) => {}
        }
        {
            let mut state = inner.state.lock();
            if let Some(bytes) = state.in_flight.take() {
                state.load.admitted.remove(bytes);
            }
            state.admit_waiting(&inner.budget);
        }
        let _ = next.delivered.send(());
    }
}

/// Retirement of a lane's transport task once no handle to the lane remains.
#[derive(Default)]
struct Retirement {
    retired: AtomicBool,
    signal: Notify,
}

impl Retirement {
    fn retire(&self) {
        self.retired.store(true, Ordering::SeqCst);
        self.signal.notify_waiters();
    }

    async fn retired(&self) {
        loop {
            let signalled = self.signal.notified();
            tokio::pin!(signalled);
            signalled.as_mut().enable();
            if self.retired.load(Ordering::SeqCst) {
                return;
            }
            signalled.await;
        }
    }
}

/// Shared by every handle of one lane; the last one dropped retires its task.
struct Owner(Arc<Retirement>);

impl Drop for Owner {
    fn drop(&mut self) {
        self.0.retire();
    }
}

/// The exact serialized size of a payload, counted without allocating it.
fn serialized_len<T: Serialize>(payload: &T) -> usize {
    struct Counter(usize);
    impl std::io::Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 += bytes.len();
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut counter = Counter(0);
    serde_json::to_writer(&mut counter, payload).expect("outbound payloads serialize to JSON");
    counter.0
}

#[cfg(test)]
mod tests;

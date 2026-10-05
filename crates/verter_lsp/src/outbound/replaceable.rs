//! The per-document versioned replacement coalescer for `publishDiagnostics`.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use parking_lot::Mutex;
use serde::Serialize;
use tokio::sync::{oneshot, Notify};
use tower_lsp_server::ls_types::notification::{Notification, PublishDiagnostics};
use tower_lsp_server::ls_types::PublishDiagnosticsParams;

use super::{serialized_len, ClassBudget, ClassHighWater, ClassLoad};

/// How one publication left the lane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Delivery {
    /// The writer wrote the payload.
    Delivered,
    /// A newer publication for the same document replaced it or arrived first,
    /// or it was no longer current when the writer reached it; it never reaches
    /// the client.
    Superseded,
    /// The transport ended before the payload was taken; it never reaches the
    /// client.
    Closed,
}

/// Whether a publication is still the current one for its document. Evaluated
/// once, when the writer takes the payload.
type StillCurrent = Box<dyn Fn() -> bool + Send + Sync>;

/// Coalesces `publishDiagnostics` per document and hands the newest payloads
/// to the transport writer, which pulls one only when it is about to write it.
#[derive(Clone)]
pub struct ReplaceableLane {
    inner: Arc<Inner>,
}

struct Inner {
    budget: ClassBudget,
    state: Mutex<LaneState>,
    /// Wakes the transport writer when a payload is ready to take.
    wake: Notify,
}

#[derive(Default)]
struct LaneState {
    /// Admitted payloads, oldest first; the writer takes from the front.
    pending: VecDeque<Entry>,
    /// Payloads not yet admitted, in the order they were offered.
    waiting: VecDeque<Entry>,
    /// Serialized size of the payload the writer has taken and not finished
    /// writing.
    in_flight: Option<usize>,
    load: ClassLoad,
    /// Ordering state for every document with a live offer. An entry lives
    /// exactly as long as some publication for that document is inside
    /// [`ReplaceableLane::publish_diagnostics`].
    documents: HashMap<String, DocumentOffers>,
    next_ticket: u64,
    high_water: ClassHighWater,
    closed: bool,
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
    still_current: StillCurrent,
    /// Fired once the writer has written the payload; dropped (resolving the
    /// publisher as superseded or closed) when the payload is retired.
    delivered: oneshot::Sender<()>,
}

/// A payload the writer has taken: it is written whole, then completed.
pub(super) struct Taken {
    pub(super) params: PublishDiagnosticsParams,
    delivered: oneshot::Sender<()>,
}

/// The body of a notification frame, serialized without building a JSON value.
#[derive(Serialize)]
pub(super) struct NotificationBody<'a, P> {
    pub(super) jsonrpc: &'static str,
    pub(super) method: &'a str,
    pub(super) params: &'a P,
}

pub(super) fn diagnostics_body(
    params: &PublishDiagnosticsParams,
) -> NotificationBody<'_, PublishDiagnosticsParams> {
    NotificationBody {
        jsonrpc: "2.0",
        method: PublishDiagnostics::METHOD,
        params,
    }
}

impl LaneState {
    /// Whether a `bytes`-sized payload fits in place of an admitted one of
    /// `replacing` bytes.
    fn admits_in_place(&self, budget: &ClassBudget, bytes: usize, replacing: usize) -> bool {
        let admitted = self.load.admitted;
        admitted.messages == 1 || admitted.bytes - replacing + bytes <= budget.bytes
    }

    /// Take a newly offered payload, retiring any older payload of the same
    /// document first. A newer payload keeps the place of the one it retires
    /// when it fits there; a payload that no longer fits the admitted set heads
    /// the waiting line, ahead of payloads offered after its predecessor was
    /// admitted.
    fn offer(&mut self, budget: &ClassBudget, entry: Entry) {
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
    fn admit_waiting(&mut self, budget: &ClassBudget) {
        while let Some(next) = self.waiting.front() {
            if !budget.admits(self.load.admitted, next.bytes) {
                break;
            }
            let next = self.waiting.pop_front().expect("front was just seen");
            self.load.waiting.remove(next.bytes);
            self.load.admitted.add(next.bytes);
            self.pending.push_back(next);
        }
        self.high_water.record(self.load);
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
/// or its future was cancelled — withdraws a payload the writer has not taken
/// and releases the document's ordering entry.
struct Offer<'a> {
    inner: &'a Inner,
    key: String,
    ticket: Option<u64>,
}

impl Drop for Offer<'_> {
    fn drop(&mut self) {
        let mut state = self.inner.state.lock();
        let mut freed = false;
        if let Some(ticket) = self.ticket.take() {
            if state.withdraw(ticket) {
                state.admit_waiting(&self.inner.budget);
                freed = true;
            }
        }
        if let Some(document) = state.documents.get_mut(&self.key) {
            document.live -= 1;
            if document.live == 0 {
                state.documents.remove(&self.key);
            }
        }
        drop(state);
        if freed {
            // A withdrawal can admit a waiting payload.
            self.inner.wake.notify_one();
        }
    }
}

impl ReplaceableLane {
    pub fn new(budget: ClassBudget) -> Self {
        Self {
            inner: Arc::new(Inner {
                budget,
                state: Mutex::new(LaneState::default()),
                wake: Notify::new(),
            }),
        }
    }

    pub fn budget(&self) -> ClassBudget {
        self.inner.budget
    }

    /// What the lane retains right now: admitted payloads (pending, or taken by
    /// the writer and not yet written) and waiting ones.
    pub fn load(&self) -> ClassLoad {
        self.inner.state.lock().load
    }

    /// The largest loads the lane has retained since it was built.
    pub fn high_water(&self) -> ClassHighWater {
        self.inner.state.lock().high_water
    }

    /// Publish one document's complete diagnostic set at `epoch`, a value that
    /// increases with every newer publication of that document.
    ///
    /// `still_current` is asked once, when the writer takes the payload; a
    /// payload that is no longer current is retired there instead of written.
    /// Resolves once the payload is written, or as soon as it is retired.
    /// Dropping the future withdraws a payload the writer has not taken.
    pub async fn publish_diagnostics(
        &self,
        epoch: u64,
        params: PublishDiagnosticsParams,
        still_current: impl Fn() -> bool + Send + Sync + 'static,
    ) -> Delivery {
        let key = params.uri.as_str().to_owned();
        let bytes = serialized_len(&diagnostics_body(&params));
        let (ticket, delivered) = {
            let mut state = self.inner.state.lock();
            if state.closed {
                return Delivery::Closed;
            }
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
                    still_current: Box::new(still_current),
                    delivered: delivered_tx,
                },
            );
            state.admit_waiting(&self.inner.budget);
            (ticket, delivered_rx)
        };
        self.inner.wake.notify_one();
        let mut offer = Offer {
            inner: &self.inner,
            key,
            ticket: Some(ticket),
        };
        let outcome = match delivered.await {
            Ok(()) => Delivery::Delivered,
            Err(_) if self.inner.state.lock().closed => Delivery::Closed,
            Err(_) => Delivery::Superseded,
        };
        // Written or retired: nothing of this offer is held any more.
        offer.ticket = None;
        outcome
    }

    /// Take the oldest admitted payload that is still current, retiring any
    /// that are not. The take and the currency check happen under the lane
    /// lock, so a publication withdrawn or cancelled before the take is never
    /// taken. `still_current` may take its own lock under this one; nothing
    /// takes the lane lock under it.
    pub(super) fn take(&self) -> Option<Taken> {
        let mut state = self.inner.state.lock();
        if state.closed || state.in_flight.is_some() {
            return None;
        }
        while let Some(next) = state.pending.pop_front() {
            if !(next.still_current)() {
                // Dropping the entry resolves its publisher as superseded.
                state.load.admitted.remove(next.bytes);
                state.admit_waiting(&self.inner.budget);
                continue;
            }
            state.in_flight = Some(next.bytes);
            return Some(Taken {
                params: next.params,
                delivered: next.delivered,
            });
        }
        None
    }

    /// The writer finished writing a taken payload.
    pub(super) fn complete(&self, taken: Taken) {
        {
            let mut state = self.inner.state.lock();
            if let Some(bytes) = state.in_flight.take() {
                state.load.admitted.remove(bytes);
            }
            state.admit_waiting(&self.inner.budget);
        }
        let _ = taken.delivered.send(());
    }

    /// The transport ended: retire everything held and refuse new payloads.
    pub(super) fn close(&self) {
        let retired = {
            let mut state = self.inner.state.lock();
            state.closed = true;
            state.load = ClassLoad::default();
            state.in_flight = None;
            let pending = std::mem::take(&mut state.pending);
            let waiting = std::mem::take(&mut state.waiting);
            (pending, waiting)
        };
        // Dropped outside the lock: each resolves its publisher as closed.
        drop(retired);
    }

    pub(super) fn wake(&self) -> &Notify {
        &self.inner.wake
    }
}

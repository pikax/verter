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
//!   messages, lifecycle signals). Never coalesced, reordered or dropped; every
//!   producer awaits its own send, so the client channel holds at most one per
//!   awaiting producer.
//! * REPLACEABLE — `textDocument/publishDiagnostics`, which replaces a document's
//!   whole diagnostic set. It flows only through [`ReplaceableLane`], never as
//!   split append-like chunks.
//!
//! The catalogued envelope of the replaceable class, for a lane built with
//! budget `B`:
//!
//! * at most `B.replaceable_messages` payloads and `B.replaceable_bytes`
//!   serialized bytes are pending or in flight. The one exception is a single
//!   payload larger than the byte budget, which is admitted alone and delivered
//!   whole — a budget never truncates a complete result;
//! * at most one payload per document is pending; a newer publication for the
//!   same document replaces it in place, and an older one never overtakes a
//!   newer one that is pending, in flight or waiting;
//! * the transport itself holds at most two replaceable payloads: one lane pump
//!   awaits each send, so only the channel's buffered slot and the pump's own
//!   parked send can be occupied. A slow client therefore delays responses by
//!   at most that bounded backlog, never by the backlog of edits behind it.
//!
//! When the lane is full, a publication for a document with nothing pending
//! waits for capacity, and that wait is as cancellable as the send it precedes.
//! Withdrawing a waiting or pending publication (dropping its future) removes its
//! payload: a superseded payload never reaches the client.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use parking_lot::Mutex;
use tokio::sync::{oneshot, Notify};
use tower_lsp_server::ls_types::notification::PublishDiagnostics;
use tower_lsp_server::ls_types::PublishDiagnosticsParams;
use tower_lsp_server::Client;

/// Limits of the replaceable-notification class. Control and response traffic
/// are bounded structurally (see the module docs) and never share this budget,
/// so a diagnostics backlog cannot consume their capacity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutboundBudget {
    /// Payloads pending or in flight, across every document.
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

/// What the lane currently retains: pending payloads plus the one handed to the
/// transport and not yet accepted by it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LaneLoad {
    pub messages: usize,
    pub bytes: usize,
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
}

struct Inner {
    budget: OutboundBudget,
    state: Mutex<LaneState>,
    /// Signalled whenever capacity frees or a document's newest epoch advances.
    capacity: Notify,
}

#[derive(Default)]
struct LaneState {
    pending: VecDeque<Pending>,
    pending_bytes: usize,
    /// Serialized size of the payload the pump has handed to the transport.
    in_flight: Option<usize>,
    /// Ordering state for every document with a live offer. An entry lives
    /// exactly as long as some publication for that document is inside
    /// [`ReplaceableLane::publish_diagnostics`].
    documents: HashMap<String, DocumentOffers>,
    next_ticket: u64,
    pump_running: bool,
    high_water: LaneLoad,
}

struct DocumentOffers {
    newest_epoch: u64,
    live: usize,
}

struct Pending {
    key: String,
    ticket: u64,
    bytes: usize,
    params: PublishDiagnosticsParams,
    /// Fired once the transport accepts the payload; dropped (resolving the
    /// publisher as superseded) when a newer payload replaces this one.
    delivered: oneshot::Sender<()>,
}

impl LaneState {
    fn load(&self) -> LaneLoad {
        LaneLoad {
            messages: self.pending.len() + usize::from(self.in_flight.is_some()),
            bytes: self.pending_bytes + self.in_flight.unwrap_or(0),
        }
    }

    /// Whether a `bytes`-sized payload fits, either as a new entry or replacing
    /// the pending entry of `replacing` bytes. A payload that would be alone in
    /// the lane always fits.
    fn admits(&self, budget: &OutboundBudget, bytes: usize, replacing: Option<usize>) -> bool {
        let load = self.load();
        match replacing {
            Some(old) => load.messages == 1 || load.bytes - old + bytes <= budget.replaceable_bytes,
            None => {
                load.messages == 0
                    || (load.messages < budget.replaceable_messages
                        && load.bytes + bytes <= budget.replaceable_bytes)
            }
        }
    }

    fn withdraw(&mut self, ticket: u64) -> bool {
        let Some(index) = self.pending.iter().position(|p| p.ticket == ticket) else {
            return false;
        };
        let withdrawn = self.pending.remove(index).expect("index was just found");
        self.pending_bytes -= withdrawn.bytes;
        true
    }
}

/// One publication's presence in the lane. Dropping it — the publisher returned
/// or its future was cancelled — withdraws a still-pending payload and releases
/// the document's ordering entry.
struct Offer<'a> {
    inner: &'a Inner,
    key: String,
    ticket: Option<u64>,
}

impl Drop for Offer<'_> {
    fn drop(&mut self) {
        let mut state = self.inner.state.lock();
        let freed = self
            .ticket
            .take()
            .is_some_and(|ticket| state.withdraw(ticket));
        if let Some(document) = state.documents.get_mut(&self.key) {
            document.live -= 1;
            if document.live == 0 {
                state.documents.remove(&self.key);
            }
        }
        drop(state);
        if freed {
            self.inner.capacity.notify_waiters();
        }
    }
}

impl ReplaceableLane {
    pub fn new(budget: OutboundBudget) -> Self {
        Self {
            inner: Arc::new(Inner {
                budget,
                state: Mutex::new(LaneState::default()),
                capacity: Notify::new(),
            }),
        }
    }

    pub fn budget(&self) -> OutboundBudget {
        self.inner.budget
    }

    /// What the lane retains right now.
    pub fn load(&self) -> LaneLoad {
        self.inner.state.lock().load()
    }

    /// The largest load the lane has retained since it was built.
    pub fn high_water(&self) -> LaneLoad {
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
        {
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
            let overtakes = epoch > document.newest_epoch;
            document.newest_epoch = epoch;
            document.live += 1;
            drop(state);
            if overtakes {
                // Older publications still waiting for capacity give up now.
                self.inner.capacity.notify_waiters();
            }
        }
        let mut offer = Offer {
            inner: &self.inner,
            key,
            ticket: None,
        };

        let mut params = Some(params);
        let delivered = loop {
            let notified = self.inner.capacity.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            {
                let mut state = self.inner.state.lock();
                if state.documents[&offer.key].newest_epoch > epoch {
                    return Delivery::Superseded;
                }
                let replace = state.pending.iter().position(|p| p.key == offer.key);
                let replacing = replace.map(|index| state.pending[index].bytes);
                if state.admits(&self.inner.budget, bytes, replacing) {
                    let (delivered_tx, delivered_rx) = oneshot::channel();
                    let ticket = state.next_ticket;
                    state.next_ticket += 1;
                    let entry = Pending {
                        key: offer.key.clone(),
                        ticket,
                        bytes,
                        params: params.take().expect("an offer is admitted once"),
                        delivered: delivered_tx,
                    };
                    match replace {
                        // Dropping the replaced entry resolves its publisher as
                        // superseded; the newer payload keeps the queue position.
                        Some(index) => {
                            let replaced = std::mem::replace(&mut state.pending[index], entry);
                            state.pending_bytes = state.pending_bytes - replaced.bytes + bytes;
                        }
                        None => {
                            state.pending.push_back(entry);
                            state.pending_bytes += bytes;
                        }
                    }
                    let load = state.load();
                    state.high_water.messages = state.high_water.messages.max(load.messages);
                    state.high_water.bytes = state.high_water.bytes.max(load.bytes);
                    offer.ticket = Some(ticket);
                    if !state.pump_running {
                        state.pump_running = true;
                        tokio::spawn(pump(Arc::clone(&self.inner), client.clone()));
                    }
                    break delivered_rx;
                }
            }
            notified.await;
        };

        let outcome = match delivered.await {
            Ok(()) => Delivery::Delivered,
            Err(_) => Delivery::Superseded,
        };
        // Delivered or replaced: nothing of this offer is pending any more.
        offer.ticket = None;
        outcome
    }
}

/// Hand pending payloads to the transport one at a time, oldest first, until the
/// lane is empty. Awaiting each send is what keeps the transport's share of the
/// replaceable class at two payloads.
async fn pump(inner: Arc<Inner>, client: Client) {
    loop {
        let next = {
            let mut state = inner.state.lock();
            let Some(next) = state.pending.pop_front() else {
                state.pump_running = false;
                return;
            };
            state.pending_bytes -= next.bytes;
            state.in_flight = Some(next.bytes);
            next
        };
        client
            .send_notification::<PublishDiagnostics>(next.params)
            .await;
        inner.state.lock().in_flight = None;
        let _ = next.delivered.send(());
        inner.capacity.notify_waiters();
    }
}

/// The exact serialized size of the payload, counted without allocating it.
fn serialized_len(params: &PublishDiagnosticsParams) -> usize {
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
    serde_json::to_writer(&mut counter, params)
        .expect("publishDiagnostics params always serialize to JSON");
    counter.0
}

#[cfg(test)]
mod tests;

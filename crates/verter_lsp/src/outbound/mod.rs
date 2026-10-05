//! Bounded server→client transport.
//!
//! Verter owns the transport writer: [`serve`] runs the one task that writes
//! every byte the server sends to the editor. Every message is accounted in one
//! of three classes from the moment it is produced until its write completes,
//! each against its own [`ClassBudget`], so a backlog in one class never consumes
//! another's capacity:
//!
//! * RESPONSE — handler results, including the reply to `shutdown` and the
//!   `RequestCancelled` errors `$/cancelRequest` produces. The writer always
//!   writes a pending response first.
//! * CONTROL — every other server→client message except diagnostics: requests
//!   (whose replies [`serve`] routes back to the waiting producer) and ordered
//!   notifications such as progress, messages and lifecycle signals. Awaiting
//!   producers are never coalesced, reordered or dropped. Detached informational
//!   notifications have a separate waiting ceiling. Written ahead of diagnostics.
//! * REPLACEABLE — `textDocument/publishDiagnostics`, which replaces a
//!   document's whole diagnostic set. It flows only through the
//!   [`ReplaceableLane`], never as split append-like chunks.
//!
//! The catalogued envelope, for a transport built with budget `B`:
//!
//! * each class's ADMITTED set — messages accepted for writing, including the
//!   one being written — holds at most `B.<class>.messages` messages and
//!   `B.<class>.bytes` serialized bytes. The one exception is a single message
//!   larger than its byte budget, which is admitted alone and written whole: a
//!   budget never truncates a complete result;
//! * a control producer that offers a message while the admitted set is full
//!   WAITS in one first-come line, and that line is accounted too: a producer
//!   that awaits its send holds its place until admitted, so those waiting
//!   messages number at most one per suspended producer; a producer that
//!   cannot await ([`Outbound::notify_detached`]) leaves its message in the line
//!   without blocking, and at most `B.control` of those detached messages
//!   wait — a newer one sheds the oldest beyond that; an oversized detached
//!   notification that cannot be admitted immediately is shed instead. Messages are admitted
//!   strictly in the order they were offered;
//! * a response produced while the admitted responses are full WAITS,
//!   accounted, and keeps its handler's concurrency slot until admitted, so
//!   ordinary running handlers and waiting responses together never exceed
//!   [`crate::LSP_MAX_CONCURRENCY`]. Two reserved lifecycle slots handle
//!   cancellation notifications, the first shutdown request and exit independently
//!   of response admission. The shutdown reply has one extra accounted waiting
//!   slot; duplicate shutdown requests use ordinary slots. Waiting never stops
//!   running handlers being polled;
//! * the replaceable class keeps at most one payload per document admitted or
//!   waiting: a newer publication retires the older payload at once, and an
//!   older one never overtakes a newer one;
//! * the writer takes the next message only when it is about to write it, so the
//!   transport itself holds at most the one frame in the middle of being
//!   written. A slow client therefore delays a response by at most that frame,
//!   never by a backlog of control messages or edits behind it.
//!
//! A diagnostics publication stays retractable until the writer takes it: a
//! newer publication for the same document replaces it, and cancelling it — the
//! document closed, or its publication was superseded — withdraws it. The take
//! itself re-checks that the publication is still current, atomically with the
//! publisher's own fence, so a publication cancelled before the take never
//! reaches the client. The one frame already taken may finish writing; it stays
//! accounted until it has.

mod control;
mod replaceable;
mod transport;
#[cfg(test)]
mod wire;

pub use replaceable::{Delivery, ReplaceableLane};
pub use transport::serve;
#[cfg(test)]
pub(crate) use wire::Wire;

use std::sync::Arc;

use serde::Serialize;

/// The limits of one traffic class.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClassBudget {
    /// Messages admitted for writing, the one being written included.
    pub messages: usize,
    /// Serialized JSON bytes of those messages.
    pub bytes: usize,
}

impl ClassBudget {
    /// Whether a `bytes`-sized message fits beside the `admitted` set. A message
    /// that would be alone in it always fits, so a budget never truncates or
    /// refuses a complete result.
    fn admits(&self, admitted: Load, bytes: usize) -> bool {
        admitted.messages == 0
            || (admitted.messages < self.messages && admitted.bytes + bytes <= self.bytes)
    }
}

/// The limits of the three traffic classes, each accounted separately.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutboundBudget {
    pub control: ClassBudget,
    pub response: ClassBudget,
    pub replaceable: ClassBudget,
}

impl OutboundBudget {
    /// The production envelope: responses up to the handler concurrency limit,
    /// room for one pending publication per open document in any realistic
    /// session, and byte ceilings well above the largest complete result or
    /// heavily diagnosed component.
    pub const DEFAULT: Self = Self {
        control: ClassBudget {
            messages: 256,
            bytes: 4 * 1024 * 1024,
        },
        response: ClassBudget {
            messages: crate::LSP_MAX_CONCURRENCY,
            bytes: 64 * 1024 * 1024,
        },
        replaceable: ClassBudget {
            messages: 256,
            bytes: 16 * 1024 * 1024,
        },
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

/// Everything one class retains.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ClassLoad {
    /// Messages accepted for writing, the one being written included — the set
    /// the class budget bounds.
    pub admitted: Load,
    /// Messages offered while the admitted set was full.
    pub waiting: Load,
    /// Messages discarded instead of retained since the transport was built:
    /// detached control notifications shed beyond their ceiling. Not part of
    /// what the class retains.
    pub shed: Load,
}

impl ClassLoad {
    /// Everything the class retains.
    pub fn retained(&self) -> Load {
        Load {
            messages: self.admitted.messages + self.waiting.messages,
            bytes: self.admitted.bytes + self.waiting.bytes,
        }
    }
}

/// The largest loads one class has retained since the transport was built.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ClassHighWater {
    /// The largest admitted set.
    pub admitted: Load,
    /// The largest retained set, admitted and waiting together.
    pub retained: Load,
}

impl ClassHighWater {
    fn record(&mut self, load: ClassLoad) {
        self.admitted = self.admitted.max(load.admitted);
        self.retained = self.retained.max(load.retained());
    }
}

/// What every class retains right now.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OutboundLoad {
    pub control: ClassLoad,
    pub response: ClassLoad,
    pub replaceable: ClassLoad,
}

/// The largest loads every class has retained since the transport was built.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OutboundHighWater {
    pub control: ClassHighWater,
    pub response: ClassHighWater,
    pub replaceable: ClassHighWater,
}

/// The server's handle on its outbound transport: the only route by which
/// server code sends anything to the client. Cheap to clone; every clone
/// shares one transport.
#[derive(Clone)]
pub struct Outbound {
    hub: Arc<Hub>,
}

struct Hub {
    budget: OutboundBudget,
    control: control::ControlQueue,
    responses: transport::ResponseQueue,
    diagnostics: ReplaceableLane,
    writer: parking_lot::Mutex<WriterState>,
}

#[derive(Default)]
struct WriterState {
    attached: bool,
    closed: bool,
}

impl Default for Outbound {
    fn default() -> Self {
        Self::new(OutboundBudget::DEFAULT)
    }
}

impl Outbound {
    pub fn new(budget: OutboundBudget) -> Self {
        Self::assemble(budget, ReplaceableLane::new(budget.replaceable))
    }

    fn assemble(budget: OutboundBudget, diagnostics: ReplaceableLane) -> Self {
        Self {
            hub: Arc::new(Hub {
                budget,
                control: control::ControlQueue::default(),
                responses: transport::ResponseQueue::default(),
                diagnostics,
                writer: parking_lot::Mutex::new(WriterState::default()),
            }),
        }
    }

    /// A transport whose replaceable class is `diagnostics`, a lane built
    /// elsewhere — for tests whose registry was built before its transport.
    #[cfg(test)]
    pub(crate) fn with_diagnostics_lane(
        budget: OutboundBudget,
        diagnostics: ReplaceableLane,
    ) -> Self {
        Self::assemble(budget, diagnostics)
    }

    pub fn budget(&self) -> OutboundBudget {
        self.hub.budget
    }

    /// The replaceable lane diagnostics are published through.
    pub fn diagnostics_lane(&self) -> ReplaceableLane {
        self.hub.diagnostics.clone()
    }

    /// What every class retains right now.
    pub fn load(&self) -> OutboundLoad {
        OutboundLoad {
            control: self.hub.control.load(),
            response: self.hub.responses.load(),
            replaceable: self.hub.diagnostics.load(),
        }
    }

    /// The largest loads every class has retained since the transport was
    /// built.
    pub fn high_water(&self) -> OutboundHighWater {
        OutboundHighWater {
            control: self.hub.control.high_water(),
            response: self.hub.responses.high_water(),
            replaceable: self.hub.diagnostics.high_water(),
        }
    }

    /// Become the transport's one writer. Dropping the attachment ends the
    /// transport: nothing more is accepted, waiting producers are released and
    /// outstanding requests fail.
    fn attach_writer(&self) -> WriterAttachment {
        let mut writer = self.hub.writer.lock();
        assert!(
            !writer.attached && !writer.closed,
            "an outbound transport has exactly one writer"
        );
        writer.attached = true;
        WriterAttachment {
            outbound: self.clone(),
        }
    }

    /// The next message the writer should write, by class priority.
    fn next_outgoing(&self, include_responses: bool) -> Option<Outgoing> {
        if include_responses {
            if let Some(body) = self.hub.responses.take() {
                return Some(Outgoing::Response(body));
            }
        }
        if let Some(body) = self.hub.control.take() {
            return Some(Outgoing::Control(body));
        }
        self.hub.diagnostics.take().map(Outgoing::Replaceable)
    }

    /// Account the completed write of `outgoing`.
    fn complete(&self, outgoing: Outgoing) {
        match outgoing {
            Outgoing::Response(_) => {
                if self.hub.responses.complete(&self.hub.budget.response) {
                    self.mark_initialized();
                }
            }
            Outgoing::Control(_) => self.hub.control.complete(&self.hub.budget.control),
            Outgoing::Replaceable(taken) => self.hub.diagnostics.complete(taken),
        }
    }

    /// Wakes the writer whenever any class gains work.
    fn wake(&self) -> &tokio::sync::Notify {
        self.hub.diagnostics.wake()
    }

    /// Stop accepting control and diagnostics: the client is gone or exiting.
    fn close_server_initiated(&self) {
        self.hub.control.close();
        self.hub.diagnostics.close();
        self.wake().notify_one();
    }
}

/// One message the writer has taken.
enum Outgoing {
    Response(Vec<u8>),
    Control(Vec<u8>),
    Replaceable(replaceable::Taken),
}

/// Held by the transport's one writer; dropping it ends the transport.
struct WriterAttachment {
    outbound: Outbound,
}

impl Drop for WriterAttachment {
    fn drop(&mut self) {
        self.outbound.hub.writer.lock().closed = true;
        self.outbound.close_server_initiated();
        self.outbound.hub.responses.close();
    }
}

/// The exact serialized size of a message, counted without allocating it.
fn serialized_len<T: Serialize>(message: &T) -> usize {
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
    serde_json::to_writer(&mut counter, message).expect("outbound messages serialize to JSON");
    counter.0
}

#[cfg(test)]
mod tests;

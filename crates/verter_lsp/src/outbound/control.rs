//! The control class: every server→client request and every notification
//! other than diagnostics.
//!
//! A control message is serialized once, when it is offered, and accounted from
//! then until the writer has written it. Messages are admitted for writing in
//! the order they were offered while the class budget has room; a message
//! offered while it has none waits in one first-come line, accounted with its
//! exact bytes. A producer that awaits its send holds its place in that line,
//! and nothing it offers is coalesced, reordered or dropped. A producer that
//! cannot await leaves its message there, and those detached messages are
//! bounded in their own right: at most one class budget of them waits, and a
//! detached message that would exceed it sheds the oldest waiting detached
//! messages first. Shedding never reorders what remains and never touches an
//! awaiting producer's message or a request.
//!
//! As with any LSP client connection, a notification offered before the client
//! has been answered `initialize` is not sent (window messages excepted), and a
//! request offered then fails as not initialized.

use std::collections::{HashMap, VecDeque};
use std::fmt::Display;

use parking_lot::Mutex;
use serde::Serialize;
use tokio::sync::oneshot;
use tower_lsp_server::jsonrpc::{self, Error, ErrorCode, Id, Response};
use tower_lsp_server::ls_types::notification::{self, Notification};
use tower_lsp_server::ls_types::request::{self, Request};
use tower_lsp_server::ls_types::{
    error_codes, LogMessageParams, MessageType, Registration, RegistrationParams, ShowMessageParams,
};

use super::replaceable::NotificationBody;
use super::{ClassBudget, ClassHighWater, ClassLoad, Load, Outbound};

#[derive(Default)]
pub(super) struct ControlQueue {
    state: Mutex<ControlState>,
}

#[derive(Default)]
struct ControlState {
    /// Frames admitted for writing, in order.
    admitted: VecDeque<Vec<u8>>,
    /// Frames offered while the admitted set was full, in offer order.
    waiting: VecDeque<Waiting>,
    /// Size of the frame the writer has taken and not finished writing.
    writing: Option<usize>,
    load: ClassLoad,
    /// The waiting frames whose producer does not wait.
    detached: Load,
    high_water: ClassHighWater,
    next_ticket: u64,
    next_request_id: i64,
    /// Server→client requests awaiting the client's reply.
    replies: HashMap<Id, oneshot::Sender<Response>>,
    initialized: bool,
    closed: bool,
}

struct Waiting {
    ticket: u64,
    body: Vec<u8>,
    /// Fired when the frame is admitted; `None` for a producer that does not
    /// wait.
    admitted: Option<oneshot::Sender<()>>,
}

enum Offered {
    Admitted,
    Waiting(u64),
    Refused,
}

/// The body of a request frame, serialized without building a JSON value.
#[derive(Serialize)]
struct RequestBody<'a, P> {
    jsonrpc: &'static str,
    method: &'a str,
    params: &'a P,
    id: &'a Id,
}

impl ControlState {
    fn admit_waiting(&mut self, budget: &ClassBudget) {
        while let Some(next) = self.waiting.front() {
            if !budget.admits(self.load.admitted, next.body.len()) {
                break;
            }
            let next = self.waiting.pop_front().expect("front was just seen");
            let bytes = next.body.len();
            self.load.waiting.remove(bytes);
            self.load.admitted.add(bytes);
            self.admitted.push_back(next.body);
            match next.admitted {
                Some(admitted) => {
                    let _ = admitted.send(());
                }
                None => self.detached.remove(bytes),
            }
        }
        self.high_water.record(self.load);
    }

    /// Make room in the detached line for a `bytes`-sized detached frame by
    /// shedding the oldest waiting detached frames.
    fn shed_detached_for(&mut self, budget: &ClassBudget, bytes: usize) {
        while self.detached.messages >= budget.messages
            || self.detached.bytes + bytes > budget.bytes
        {
            let index = self
                .waiting
                .iter()
                .position(|waiting| waiting.admitted.is_none())
                .expect("the detached line is not empty");
            let shed = self.waiting.remove(index).expect("index was just found");
            self.load.waiting.remove(shed.body.len());
            self.detached.remove(shed.body.len());
            self.load.shed.add(shed.body.len());
            tracing::warn!(
                bytes = shed.body.len(),
                "client is not reading; shedding the oldest detached control message"
            );
        }
    }
}

impl ControlQueue {
    fn offer(
        &self,
        budget: &ClassBudget,
        body: Vec<u8>,
        admitted: Option<oneshot::Sender<()>>,
    ) -> Offered {
        let mut state = self.state.lock();
        if state.closed {
            return Offered::Refused;
        }
        if state.waiting.is_empty() && budget.admits(state.load.admitted, body.len()) {
            state.load.admitted.add(body.len());
            state.admitted.push_back(body);
            let load = state.load;
            state.high_water.record(load);
            return Offered::Admitted;
        }
        if admitted.is_none() {
            if budget.messages == 0 || body.len() > budget.bytes {
                state.load.shed.add(body.len());
                tracing::warn!(
                    bytes = body.len(),
                    "shedding oversized detached control message"
                );
                return Offered::Refused;
            }
            state.shed_detached_for(budget, body.len());
            state.detached.add(body.len());
        }
        let ticket = state.next_ticket;
        state.next_ticket += 1;
        state.load.waiting.add(body.len());
        state.waiting.push_back(Waiting {
            ticket,
            body,
            admitted,
        });
        let load = state.load;
        state.high_water.record(load);
        Offered::Waiting(ticket)
    }

    /// Remove a waiting frame whose producer gave up; `true` if the line moved.
    fn withdraw(&self, budget: &ClassBudget, ticket: u64) -> bool {
        let mut state = self.state.lock();
        let Some(index) = state.waiting.iter().position(|w| w.ticket == ticket) else {
            return false;
        };
        let withdrawn = state.waiting.remove(index).expect("index was just found");
        state.load.waiting.remove(withdrawn.body.len());
        state.admit_waiting(budget);
        true
    }

    pub(super) fn take(&self) -> Option<Vec<u8>> {
        let mut state = self.state.lock();
        if state.writing.is_some() {
            return None;
        }
        let body = state.admitted.pop_front()?;
        state.writing = Some(body.len());
        Some(body)
    }

    pub(super) fn complete(&self, budget: &ClassBudget) {
        let mut state = self.state.lock();
        if let Some(bytes) = state.writing.take() {
            state.load.admitted.remove(bytes);
        }
        state.admit_waiting(budget);
    }

    /// The transport ended: release every producer and fail every request
    /// still awaiting a reply.
    pub(super) fn close(&self) {
        let released = {
            let mut state = self.state.lock();
            state.closed = true;
            state.load = ClassLoad {
                shed: state.load.shed,
                ..ClassLoad::default()
            };
            state.detached = Load::default();
            state.writing = None;
            (
                std::mem::take(&mut state.admitted),
                std::mem::take(&mut state.waiting),
                std::mem::take(&mut state.replies),
            )
        };
        drop(released);
    }

    pub(super) fn load(&self) -> ClassLoad {
        self.state.lock().load
    }

    pub(super) fn high_water(&self) -> ClassHighWater {
        self.state.lock().high_water
    }

    fn initialized(&self) -> bool {
        self.state.lock().initialized
    }

    pub(super) fn mark_initialized(&self) {
        self.state.lock().initialized = true;
    }

    fn register_request(&self) -> Option<(Id, oneshot::Receiver<Response>)> {
        let mut state = self.state.lock();
        if state.closed {
            return None;
        }
        let id = Id::Number(state.next_request_id);
        state.next_request_id += 1;
        let (sender, receiver) = oneshot::channel();
        state.replies.insert(id.clone(), sender);
        Some((id, receiver))
    }

    fn forget_request(&self, id: &Id) {
        self.state.lock().replies.remove(id);
    }

    fn route_reply(&self, response: Response) -> Result<(), Response> {
        let reply = self.state.lock().replies.remove(response.id());
        match reply {
            Some(reply) => {
                let _ = reply.send(response);
                Ok(())
            }
            None => Err(response),
        }
    }
}

/// Withdraws a waiting frame when its producer is cancelled.
struct WaitingPlace<'a> {
    outbound: &'a Outbound,
    ticket: Option<u64>,
}

impl Drop for WaitingPlace<'_> {
    fn drop(&mut self) {
        if let Some(ticket) = self.ticket.take() {
            if self
                .outbound
                .hub
                .control
                .withdraw(&self.outbound.hub.budget.control, ticket)
            {
                self.outbound.wake().notify_one();
            }
        }
    }
}

/// Fails the reply slot of a request whose producer is cancelled.
struct ReplySlot<'a> {
    outbound: &'a Outbound,
    id: Id,
}

impl Drop for ReplySlot<'_> {
    fn drop(&mut self) {
        self.outbound.hub.control.forget_request(&self.id);
    }
}

fn notification_frame<N: Notification>(params: &N::Params) -> Vec<u8> {
    serde_json::to_vec(&NotificationBody {
        jsonrpc: "2.0",
        method: N::METHOD,
        params,
    })
    .expect("notification params serialize to JSON")
}

impl Outbound {
    /// Offer one control frame; resolves once it is admitted for writing, or at
    /// once if the transport has ended. `false` if it never will be written.
    async fn offer_control(&self, body: Vec<u8>) -> bool {
        let (admitted, on_admission) = oneshot::channel();
        match self
            .hub
            .control
            .offer(&self.hub.budget.control, body, Some(admitted))
        {
            Offered::Admitted => {
                self.wake().notify_one();
                true
            }
            Offered::Waiting(ticket) => {
                let mut place = WaitingPlace {
                    outbound: self,
                    ticket: Some(ticket),
                };
                let admitted = on_admission.await.is_ok();
                place.ticket = None;
                admitted
            }
            Offered::Refused => false,
        }
    }

    /// Send a notification, in order behind every control message offered
    /// before it. Resolves once the notification is admitted for writing.
    pub async fn send_notification<N>(&self, params: N::Params)
    where
        N: Notification,
    {
        if !self.hub.control.initialized() {
            tracing::trace!(
                method = N::METHOD,
                "server not initialized, suppressing notification"
            );
            return;
        }
        self.offer_control(notification_frame::<N>(&params)).await;
    }

    /// Send a notification from a producer that cannot await: it joins the
    /// control line in order and is accounted there until written. While the
    /// client is not reading, at most one control budget of such notifications
    /// waits; beyond it the oldest waiting one is shed.
    pub fn notify_detached<N>(&self, params: N::Params)
    where
        N: Notification,
    {
        if !self.hub.control.initialized() {
            tracing::trace!(
                method = N::METHOD,
                "server not initialized, suppressing notification"
            );
            return;
        }
        let body = notification_frame::<N>(&params);
        if let Offered::Admitted = self.hub.control.offer(&self.hub.budget.control, body, None) {
            self.wake().notify_one();
        }
    }

    /// Send a request to the client and await its reply.
    pub async fn send_request<R>(&self, params: R::Params) -> jsonrpc::Result<R::Result>
    where
        R: Request,
    {
        if !self.hub.control.initialized() {
            tracing::trace!(
                method = R::METHOD,
                "server not initialized, suppressing request"
            );
            return Err(Error {
                code: ErrorCode::ServerError(error_codes::SERVER_NOT_INITIALIZED),
                message: "Server not initialized".into(),
                data: None,
            });
        }
        let Some((id, reply)) = self.hub.control.register_request() else {
            return Err(Error::internal_error());
        };
        let _slot = ReplySlot {
            outbound: self,
            id: id.clone(),
        };
        let body = serde_json::to_vec(&RequestBody {
            jsonrpc: "2.0",
            method: R::METHOD,
            params: &params,
            id: &id,
        })
        .expect("request params serialize to JSON");
        if !self.offer_control(body).await {
            return Err(Error::internal_error());
        }
        let Ok(response) = reply.await else {
            return Err(Error::internal_error());
        };
        let (_, result) = response.into_parts();
        result.and_then(|value| {
            serde_json::from_value(value).map_err(|error| Error {
                code: ErrorCode::ParseError,
                message: error.to_string().into(),
                data: None,
            })
        })
    }

    /// `window/showMessage`, which may be sent before initialization.
    pub async fn show_message<M: Display>(&self, typ: MessageType, message: M) {
        let params = ShowMessageParams {
            typ,
            message: message.to_string(),
        };
        self.offer_control(notification_frame::<notification::ShowMessage>(&params))
            .await;
    }

    /// `window/logMessage`, which may be sent before initialization.
    pub async fn log_message<M: Display>(&self, typ: MessageType, message: M) {
        let params = LogMessageParams {
            typ,
            message: message.to_string(),
        };
        self.offer_control(notification_frame::<notification::LogMessage>(&params))
            .await;
    }

    pub async fn register_capability(
        &self,
        registrations: Vec<Registration>,
    ) -> jsonrpc::Result<()> {
        self.send_request::<request::RegisterCapability>(RegistrationParams { registrations })
            .await
    }

    pub async fn semantic_tokens_refresh(&self) -> jsonrpc::Result<()> {
        self.send_request::<request::SemanticTokensRefresh>(())
            .await
    }

    pub async fn inlay_hint_refresh(&self) -> jsonrpc::Result<()> {
        self.send_request::<request::InlayHintRefreshRequest>(())
            .await
    }

    /// The client answered `initialize`: control traffic may flow.
    pub(super) fn mark_initialized(&self) {
        self.hub.control.mark_initialized();
    }

    /// Hand a client reply to the request awaiting it.
    pub(super) fn route_reply(&self, response: Response) {
        if let Err(response) = self.hub.control.route_reply(response) {
            tracing::warn!(id = %response.id(), "reply to an unknown server request");
        }
    }
}

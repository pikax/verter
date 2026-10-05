//! The LSP stdio transport: framing, request dispatch and the one writer.

use std::collections::VecDeque;
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::task::Poll;

use futures_util::future::Either;
use futures_util::stream::FuturesUnordered;
use futures_util::task::AtomicWaker;
use futures_util::{FutureExt as _, StreamExt as _};
use parking_lot::Mutex;
use serde::Deserialize;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;
use tower_lsp_server::jsonrpc::{Error, Id, Request, Response};
use tower_service::Service;

use super::replaceable::diagnostics_body;
use super::{ClassBudget, ClassHighWater, ClassLoad, Outbound, Outgoing};

/// Requests read but not yet started, beyond the ones running concurrently.
const MESSAGE_QUEUE_SIZE: usize = 100;
/// Reserved running slots for lifecycle work, independent of response admission.
const LIFECYCLE_SLOTS: usize = 2;

/// One request's handling: whether it was `initialize`, and its reply (none for
/// a notification).
type Task = Pin<Box<dyn Future<Output = (bool, Option<Response>)> + Send>>;

/// The response class: handler results, held from the moment a handler
/// finishes until the writer has written them.
#[derive(Default)]
pub(super) struct ResponseQueue {
    state: Mutex<ResponseState>,
    /// Woken whenever a waiting response is admitted: the dispatcher may start
    /// another handler.
    slots: AtomicWaker,
}

#[derive(Default)]
struct ResponseState {
    /// Admitted responses, in the order they were admitted.
    queue: VecDeque<Vec<u8>>,
    /// Responses produced while the admitted set was full, in production order.
    waiting: VecDeque<Produced>,
    writing: Option<usize>,
    load: ClassLoad,
    high_water: ClassHighWater,
    /// No further response will be produced.
    finished: bool,
    /// The transport ended; responses are no longer written.
    closed: bool,
}

/// A complete, serialized response not yet admitted.
struct Produced {
    body: Vec<u8>,
    /// It is the successful reply to `initialize`.
    initializes: bool,
    /// Ordinary results keep their handler slot while awaiting admission.
    holds_slot: bool,
}

impl ResponseState {
    /// Admit `produced` when it fits; `true` if it was admitted.
    fn admit(&mut self, budget: &ClassBudget, produced: &Produced) -> bool {
        if !budget.admits(self.load.admitted, produced.body.len()) {
            return false;
        }
        self.load.admitted.add(produced.body.len());
        true
    }

    /// Admit waiting responses in production order while the next one fits;
    /// `true` if one of them answered `initialize`.
    fn admit_waiting(&mut self, budget: &ClassBudget) -> bool {
        let mut initialized = false;
        while let Some(next) = self.waiting.front() {
            if !budget.admits(self.load.admitted, next.body.len()) {
                break;
            }
            let next = self.waiting.pop_front().expect("front was just seen");
            self.load.waiting.remove(next.body.len());
            self.load.admitted.add(next.body.len());
            initialized |= next.initializes;
            self.queue.push_back(next.body);
        }
        self.high_water.record(self.load);
        initialized
    }
}

impl ResponseQueue {
    /// Hold one complete response: admitted when the admitted set has room,
    /// otherwise waiting, accounted either way. Never blocks, so the handlers
    /// still running — a cancellation among them — keep being polled. `true`
    /// if a successful `initialize` reply was admitted.
    fn offer(
        &self,
        budget: &ClassBudget,
        body: Vec<u8>,
        initializes: bool,
        holds_slot: bool,
    ) -> bool {
        let mut state = self.state.lock();
        if state.closed {
            return false;
        }
        let produced = Produced {
            body,
            initializes,
            holds_slot,
        };
        if state.waiting.is_empty() && state.admit(budget, &produced) {
            state.queue.push_back(produced.body);
            let load = state.load;
            state.high_water.record(load);
            return initializes;
        }
        state.load.waiting.add(produced.body.len());
        state.waiting.push_back(produced);
        let load = state.load;
        state.high_water.record(load);
        false
    }

    /// Ordinary responses produced and not yet admitted.
    fn unadmitted(&self) -> usize {
        self.state
            .lock()
            .waiting
            .iter()
            .filter(|p| p.holds_slot)
            .count()
    }

    pub(super) fn take(&self) -> Option<Vec<u8>> {
        let mut state = self.state.lock();
        if state.writing.is_some() {
            return None;
        }
        let body = state.queue.pop_front()?;
        state.writing = Some(body.len());
        Some(body)
    }

    /// The writer finished writing the taken response; `true` if a successful
    /// `initialize` reply was admitted in its place.
    pub(super) fn complete(&self, budget: &ClassBudget) -> bool {
        let initialized = {
            let mut state = self.state.lock();
            if let Some(bytes) = state.writing.take() {
                state.load.admitted.remove(bytes);
            }
            state.admit_waiting(budget)
        };
        self.slots.wake();
        initialized
    }

    fn finish(&self) {
        self.state.lock().finished = true;
    }

    /// Every response has been produced and written.
    fn drained(&self) -> bool {
        let state = self.state.lock();
        state.closed
            || (state.finished
                && state.queue.is_empty()
                && state.waiting.is_empty()
                && state.writing.is_none())
    }

    pub(super) fn close(&self) {
        {
            let mut state = self.state.lock();
            state.closed = true;
            state.queue.clear();
            state.waiting.clear();
            state.writing = None;
            state.load = ClassLoad::default();
        }
        self.slots.wake();
    }

    pub(super) fn load(&self) -> ClassLoad {
        self.state.lock().load
    }

    pub(super) fn high_water(&self) -> ClassHighWater {
        self.state.lock().high_water
    }
}

/// A message read from the client.
#[derive(Deserialize)]
#[serde(untagged)]
enum Incoming {
    Response(Response),
    Request(Request),
}

enum ReadError {
    /// The stream failed or ended mid-message.
    Io(io::Error),
    /// A malformed frame; answered with this error and skipped.
    Malformed(Error),
}

/// Serve `service` over `stdin`/`stdout`, writing every outbound byte through
/// `outbound`'s one writer.
///
/// Ordinary requests use at most [`crate::LSP_MAX_CONCURRENCY`] handler slots;
/// lifecycle work uses two reserved slots. Client replies reach their awaiting
/// server requests. After `exit`
/// (or the end of input) the server finishes the requests it started, writes
/// their responses and returns. Server-initiated traffic stops at that point.
pub async fn serve<I, O, S>(stdin: I, stdout: O, mut service: S, outbound: Outbound)
where
    I: AsyncRead + Unpin,
    O: AsyncWrite + Unpin,
    S: Service<Request, Response = Option<Response>>,
    S::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
    S::Future: Send + 'static,
{
    let attachment = outbound.attach_writer();
    let (tasks_tx, tasks_rx) = mpsc::channel::<Task>(MESSAGE_QUEUE_SIZE);
    let (lifecycle_tx, lifecycle_rx) = mpsc::channel::<Task>(LIFECYCLE_SLOTS);
    let handling = Box::pin(futures_util::future::join(
        read_input(stdin, &mut service, &outbound, tasks_tx, lifecycle_tx),
        dispatch(tasks_rx, lifecycle_rx, &outbound),
    ));
    let writing = Box::pin(write_output(stdout, &outbound));
    match futures_util::future::select(handling, writing).await {
        // Input ended and every handler finished: write what they produced.
        Either::Left((_, writing)) => writing.await,
        // The writer failed before input ended. Nothing more can reach the
        // client, so dropping the handling stops reading and drops every
        // running handler with whatever it holds.
        Either::Right(((), handling)) => drop(handling),
    }
    drop(attachment);
}

async fn read_input<I, S>(
    stdin: I,
    service: &mut S,
    outbound: &Outbound,
    tasks: mpsc::Sender<Task>,
    lifecycle: mpsc::Sender<Task>,
) where
    I: AsyncRead + Unpin,
    S: Service<Request, Response = Option<Response>>,
    S::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
    S::Future: Send + 'static,
{
    let mut reader = FrameReader::new(stdin);
    let mut shutdown_started = false;
    loop {
        match reader.read_message().await {
            Ok(None) => break,
            Ok(Some(Incoming::Request(request))) => {
                if let Err(error) = futures_util::future::poll_fn(|cx| service.poll_ready(cx)).await
                {
                    tracing::error!("{}", error.into());
                    break;
                }
                // The server must exit right after `exit`; some clients never
                // close stdin, so reading stops here.
                let will_exit = request.method() == "exit";
                let initialize = request.method() == "initialize";
                // Notifications cannot accumulate response waiters. Only the
                // first shutdown request gets a reserved response slot; duplicate
                // shutdown requests use ordinary admission.
                let notification = request.id().is_none();
                let first_shutdown =
                    request.method() == "shutdown" && !notification && !shutdown_started;
                let urgent = (notification
                    && matches!(request.method(), "$/cancelRequest" | "exit"))
                    || first_shutdown;
                shutdown_started |= first_shutdown;
                let handled = service.call(request).map(move |result| {
                    let response = result.unwrap_or_else(|error| {
                        tracing::error!("{}", error.into());
                        None
                    });
                    // LSP notifications have no response. Do not allow a
                    // nonconforming generic service to accumulate responses
                    // outside the handler-slot envelope through this route.
                    let response = if notification {
                        if response.is_some() {
                            tracing::warn!("service returned a response to a notification");
                        }
                        None
                    } else {
                        response
                    };
                    (initialize, response)
                });
                let target = if urgent { &lifecycle } else { &tasks };
                if target.send(Box::pin(handled)).await.is_err() || will_exit {
                    break;
                }
            }
            Ok(Some(Incoming::Response(reply))) => outbound.route_reply(reply),
            Err(ReadError::Malformed(error)) => {
                tracing::error!("failed to decode message: {}", error.message);
                let response = Response::from_error(Id::Null, error);
                let answered = std::future::ready((false, Some(response)));
                if tasks.send(Box::pin(answered)).await.is_err() {
                    break;
                }
            }
            Err(ReadError::Io(error)) => {
                tracing::error!("failed to read from the client: {error}");
                break;
            }
        }
    }
    drop(tasks);
    drop(lifecycle);
    outbound.close_server_initiated();
}

/// Run ordinary handlers within [`crate::LSP_MAX_CONCURRENCY`], plus two
/// reserved lifecycle slots, and account every produced response.
///
/// A response produced while the admitted responses are full waits, accounted,
/// and keeps its handler's slot until it is admitted, so no more handlers start
/// than the slots allow. Holding it never stops the polling of the handlers
/// still running. Lifecycle work has reserved slots even when every ordinary
/// slot holds a response waiting for a stalled client.
async fn dispatch(
    mut tasks: mpsc::Receiver<Task>,
    mut lifecycle: mpsc::Receiver<Task>,
    outbound: &Outbound,
) {
    async fn tracked(task: Task, ordinary: bool) -> (bool, bool, Option<Response>) {
        let (initialize, response) = task.await;
        (ordinary, initialize, response)
    }
    let responses = &outbound.hub.responses;
    let budget = &outbound.hub.budget.response;
    let mut running = FuturesUnordered::new();
    let mut ordinary_running = 0;
    let mut lifecycle_running = 0;
    let mut input_open = true;
    let mut lifecycle_open = true;
    futures_util::future::poll_fn(|cx| {
        responses.slots.register(cx.waker());
        loop {
            // Cancellation, shutdown and exit must still run when ordinary
            // handler slots are occupied by produced responses awaiting room.
            while lifecycle_open && lifecycle_running < LIFECYCLE_SLOTS {
                match lifecycle.poll_recv(cx) {
                    Poll::Ready(Some(task)) => {
                        running.push(tracked(task, false));
                        lifecycle_running += 1;
                    }
                    Poll::Ready(None) => lifecycle_open = false,
                    Poll::Pending => break,
                }
            }
            while input_open
                && ordinary_running + responses.unadmitted() < crate::LSP_MAX_CONCURRENCY
            {
                match tasks.poll_recv(cx) {
                    Poll::Ready(Some(task)) => {
                        running.push(tracked(task, true));
                        ordinary_running += 1;
                    }
                    Poll::Ready(None) => input_open = false,
                    Poll::Pending => break,
                }
            }
            match running.poll_next_unpin(cx) {
                Poll::Ready(Some((ordinary, initialize, response))) => {
                    if ordinary {
                        ordinary_running -= 1;
                    } else {
                        lifecycle_running -= 1;
                    }
                    if let Some(response) = response {
                        let initializes = initialize && response.is_ok();
                        if responses.offer(budget, response_body(&response), initializes, ordinary)
                        {
                            outbound.mark_initialized();
                        }
                        outbound.wake().notify_one();
                    }
                }
                Poll::Ready(None) if !input_open && !lifecycle_open => return Poll::Ready(()),
                Poll::Ready(None) | Poll::Pending => return Poll::Pending,
            }
        }
    })
    .await;
    responses.finish();
    outbound.wake().notify_one();
}

async fn write_output<O: AsyncWrite + Unpin>(mut stdout: O, outbound: &Outbound) {
    loop {
        if let Some(outgoing) = outbound.next_outgoing(true) {
            let written = match &outgoing {
                Outgoing::Response(body) | Outgoing::Control(body) => {
                    write_frame(&mut stdout, body).await
                }
                Outgoing::Replaceable(taken) => {
                    let body = serde_json::to_vec(&diagnostics_body(&taken.params))
                        .expect("diagnostics serialize to JSON");
                    write_frame(&mut stdout, &body).await
                }
            };
            if let Err(error) = written {
                tracing::error!("failed to write to the client: {error}");
                outbound.hub.responses.close();
                outbound.close_server_initiated();
                return;
            }
            outbound.complete(outgoing);
            continue;
        }
        if outbound.hub.responses.drained() {
            return;
        }
        outbound.wake().notified().await;
    }
}

async fn write_frame<O: AsyncWrite + Unpin>(stdout: &mut O, body: &[u8]) -> io::Result<()> {
    stdout
        .write_all(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes())
        .await?;
    stdout.write_all(body).await?;
    stdout.flush().await
}

fn response_body(response: &Response) -> Vec<u8> {
    serde_json::to_vec(response).expect("responses serialize to JSON")
}

/// Reads framed messages from the client.
struct FrameReader<R> {
    reader: BufReader<R>,
    /// After a header block with no usable `Content-Length` the body's extent
    /// is unknown: input is skipped up to the next `Content-Length` header.
    resynchronizing: bool,
}

/// Body bytes reserved ahead of their arrival; a larger body grows as it is
/// read, so a declared length never allocates more than has been sent.
const BODY_RESERVE: usize = 64 * 1024;

impl<R: AsyncRead + Unpin> FrameReader<R> {
    fn new(stdin: R) -> Self {
        Self {
            reader: BufReader::new(stdin),
            resynchronizing: false,
        }
    }

    /// Read one framed message; `None` at a clean end of input. Empty bodies
    /// are skipped.
    async fn read_message(&mut self) -> Result<Option<Incoming>, ReadError> {
        loop {
            let mut content_length = None;
            let mut malformed = false;
            let mut started = false;
            let mut line = Vec::new();
            loop {
                line.clear();
                let read = self
                    .reader
                    .read_until(b'\n', &mut line)
                    .await
                    .map_err(ReadError::Io)?;
                if read == 0 {
                    if started {
                        return Err(ReadError::Io(io::ErrorKind::UnexpectedEof.into()));
                    }
                    return Ok(None);
                }
                if self.resynchronizing {
                    // The skipped body has no line ending of its own, so the
                    // next header can share its line.
                    let Some(at) = find_content_length(&line) else {
                        continue;
                    };
                    line.drain(..at);
                    self.resynchronizing = false;
                }
                let Ok(text) = std::str::from_utf8(&line) else {
                    malformed = true;
                    started = true;
                    continue;
                };
                let text = text.trim_end_matches(['\r', '\n']);
                if text.is_empty() {
                    if started {
                        break;
                    }
                    continue;
                }
                started = true;
                let Some((name, value)) = text.split_once(':') else {
                    malformed = true;
                    continue;
                };
                let value = value.trim();
                if name.eq_ignore_ascii_case("Content-Length") {
                    match value.parse::<usize>() {
                        Ok(length) => content_length = Some(length),
                        Err(_) => malformed = true,
                    }
                } else if name.eq_ignore_ascii_case("Content-Type") {
                    let charset = value
                        .split(';')
                        .skip(1)
                        .map(str::trim)
                        .find_map(|param| param.strip_prefix("charset="));
                    if !matches!(charset, Some("utf-8" | "utf8")) {
                        malformed = true;
                    }
                } else {
                    tracing::warn!("encountered unsupported header: {name:?}");
                }
            }
            let Some(length) = content_length else {
                self.resynchronizing = true;
                return Err(ReadError::Malformed(Error::parse_error()));
            };
            let mut body = Vec::with_capacity(length.min(BODY_RESERVE));
            (&mut self.reader)
                .take(length as u64)
                .read_to_end(&mut body)
                .await
                .map_err(ReadError::Io)?;
            if body.len() < length {
                return Err(ReadError::Io(io::ErrorKind::UnexpectedEof.into()));
            }
            if malformed {
                return Err(ReadError::Malformed(Error::parse_error()));
            }
            if body.is_empty() {
                continue;
            }
            return match serde_json::from_slice(&body) {
                Ok(message) => Ok(Some(message)),
                Err(error) if error.is_data() => {
                    Err(ReadError::Malformed(Error::invalid_request()))
                }
                Err(_) => Err(ReadError::Malformed(Error::parse_error())),
            };
        }
    }
}

/// Where a usable `Content-Length: <digits>` header starts, in any letter case.
/// A token inside the skipped JSON body must not hide a later real header.
fn find_content_length(line: &[u8]) -> Option<usize> {
    const NAME: &[u8] = b"content-length";
    line.windows(NAME.len())
        .enumerate()
        .find_map(|(at, window)| {
            if !window.eq_ignore_ascii_case(NAME) {
                return None;
            }
            let suffix = std::str::from_utf8(&line[at + NAME.len()..]).ok()?;
            suffix.strip_prefix(':')?.trim().parse::<usize>().ok()?;
            Some(at)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A generic service's invalid notification replies must not escape the
    /// lifecycle lane or accumulate outside ordinary response admission.
    #[tokio::test(flavor = "current_thread")]
    async fn notification_replies_cannot_escape_the_lifecycle_envelope() {
        struct RepliesToNotifications;
        impl Service<Request> for RepliesToNotifications {
            type Response = Option<Response>;
            type Error = io::Error;
            type Future = std::future::Ready<Result<Self::Response, Self::Error>>;
            fn poll_ready(
                &mut self,
                _: &mut std::task::Context<'_>,
            ) -> Poll<Result<(), Self::Error>> {
                Poll::Ready(Ok(()))
            }
            fn call(&mut self, _: Request) -> Self::Future {
                std::future::ready(Ok(Some(Response::from_ok(
                    Id::Null,
                    serde_json::json!("invalid reply"),
                ))))
            }
        }
        let input = format!(
            "{}{}",
            frame(r#"{"jsonrpc":"2.0","method":"$/cancelRequest","params":{"id":1}}"#),
            frame(r#"{"jsonrpc":"2.0","method":"exit"}"#)
        );
        let outbound = Outbound::default();
        let mut written = Vec::new();
        serve(
            input.as_bytes(),
            &mut written,
            RepliesToNotifications,
            outbound.clone(),
        )
        .await;
        assert!(
            written.is_empty(),
            "notifications must never produce response frames"
        );
        assert_eq!(
            outbound.high_water().response.retained.messages,
            0,
            "notification replies cannot accumulate response waiters"
        );
    }

    fn frame(body: &str) -> String {
        format!("Content-Length: {}\r\n\r\n{body}", body.len())
    }

    fn request(id: i64) -> String {
        frame(&format!(
            r#"{{"jsonrpc":"2.0","id":{id},"method":"test/ping"}}"#
        ))
    }

    fn request_id(message: Option<Incoming>) -> Id {
        match message {
            Some(Incoming::Request(request)) => request.id().cloned().expect("a request id"),
            _ => panic!("expected a request"),
        }
    }

    /// A header block with no usable length is answered once, and reading
    /// resumes at the next frame.
    ///
    /// Discriminating: a reader that returns without skipping the unframed body
    /// misreads every following frame as malformed.
    #[tokio::test(flavor = "current_thread")]
    async fn a_frame_without_a_usable_length_does_not_lose_the_frames_after_it() {
        for header in ["Content-Length: abc\r\n\r\n", "\r\nX-Other: 1\r\n\r\n"] {
            let input = format!(
                "{header}{{\"x\":\"Content-Length\"}}{}{}",
                request(1),
                request(2)
            );
            let mut reader = FrameReader::new(input.as_bytes());
            assert!(
                matches!(reader.read_message().await, Err(ReadError::Malformed(_))),
                "the unframed message is answered as malformed ({header:?})"
            );
            assert_eq!(
                request_id(reader.read_message().await.ok().flatten()),
                Id::Number(1)
            );
            assert_eq!(
                request_id(reader.read_message().await.ok().flatten()),
                Id::Number(2)
            );
            assert!(matches!(reader.read_message().await, Ok(None)));
        }
    }

    /// A declared length allocates only the bytes that arrive: a frame that
    /// claims more than is ever sent ends the stream instead of aborting the
    /// process on the allocation.
    #[tokio::test(flavor = "current_thread")]
    async fn a_huge_declared_length_reads_only_what_arrives() {
        let input = "Content-Length: 99999999999999\r\n\r\n{}";
        let mut reader = FrameReader::new(input.as_bytes());
        assert!(matches!(
            reader.read_message().await,
            Err(ReadError::Io(error)) if error.kind() == io::ErrorKind::UnexpectedEof
        ));
    }
}

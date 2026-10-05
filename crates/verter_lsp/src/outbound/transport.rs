//! The LSP stdio transport: framing, request dispatch and the one writer.

use std::collections::VecDeque;
use std::future::Future;
use std::io;
use std::pin::Pin;

use futures_util::{FutureExt as _, StreamExt as _};
use parking_lot::Mutex;
use serde::Deserialize;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, Notify};
use tower_lsp_server::jsonrpc::{Error, Id, Request, Response};
use tower_service::Service;

use super::replaceable::diagnostics_body;
use super::{ClassBudget, ClassHighWater, ClassLoad, Outbound, Outgoing};

/// Requests read but not yet started, beyond the ones running concurrently.
const MESSAGE_QUEUE_SIZE: usize = 100;

/// One request's handling: whether it was `initialize`, and its reply (none for
/// a notification).
type Task = Pin<Box<dyn Future<Output = (bool, Option<Response>)> + Send>>;

/// The response class: handler results, held from the moment a handler
/// finishes until the writer has written them.
#[derive(Default)]
pub(super) struct ResponseQueue {
    state: Mutex<ResponseState>,
    /// Signalled whenever the admitted set shrinks.
    room: Notify,
}

#[derive(Default)]
struct ResponseState {
    queue: VecDeque<Vec<u8>>,
    writing: Option<usize>,
    load: ClassLoad,
    high_water: ClassHighWater,
    /// No further response will be produced.
    finished: bool,
    /// The transport ended; responses are no longer written.
    closed: bool,
}

impl ResponseQueue {
    /// Admit one serialized response, waiting while the admitted set is full.
    /// Nothing more is taken from the running handlers meanwhile.
    async fn push(&self, budget: &ClassBudget, body: Vec<u8>, wake: &Notify) {
        loop {
            let room = self.room.notified();
            tokio::pin!(room);
            room.as_mut().enable();
            {
                let mut state = self.state.lock();
                if state.closed {
                    return;
                }
                if budget.admits(state.load.admitted, body.len()) {
                    state.load.admitted.add(body.len());
                    state.queue.push_back(body);
                    let load = state.load;
                    state.high_water.record(load);
                    drop(state);
                    wake.notify_one();
                    return;
                }
            }
            room.await;
        }
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

    pub(super) fn complete(&self) {
        {
            let mut state = self.state.lock();
            if let Some(bytes) = state.writing.take() {
                state.load.admitted.remove(bytes);
            }
        }
        self.room.notify_waiters();
    }

    fn finish(&self) {
        self.state.lock().finished = true;
    }

    /// Every response has been produced and written.
    fn drained(&self) -> bool {
        let state = self.state.lock();
        state.closed || (state.finished && state.queue.is_empty() && state.writing.is_none())
    }

    pub(super) fn close(&self) {
        {
            let mut state = self.state.lock();
            state.closed = true;
            state.queue.clear();
            state.writing = None;
            state.load = ClassLoad::default();
        }
        self.room.notify_waiters();
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
/// Requests are handled up to [`crate::LSP_MAX_CONCURRENCY`] at a time, client
/// replies are routed to the server requests awaiting them, and after `exit`
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
    futures_util::join!(
        read_input(stdin, &mut service, &outbound, tasks_tx),
        dispatch(tasks_rx, &outbound),
        write_output(stdout, &outbound),
    );
    drop(attachment);
}

async fn read_input<I, S>(stdin: I, service: &mut S, outbound: &Outbound, tasks: mpsc::Sender<Task>)
where
    I: AsyncRead + Unpin,
    S: Service<Request, Response = Option<Response>>,
    S::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
    S::Future: Send + 'static,
{
    let mut reader = BufReader::new(stdin);
    loop {
        match read_message(&mut reader).await {
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
                let handled = service.call(request).map(move |result| {
                    let response = result.unwrap_or_else(|error| {
                        tracing::error!("{}", error.into());
                        None
                    });
                    (initialize, response)
                });
                if tasks.send(Box::pin(handled)).await.is_err() || will_exit {
                    break;
                }
            }
            Ok(Some(Incoming::Response(reply))) => outbound.route_reply(reply),
            Err(ReadError::Malformed(error)) => {
                tracing::error!("failed to decode message: {}", error.message);
                let response = Response::from_error(Id::Null, error);
                outbound
                    .hub
                    .responses
                    .push(
                        &outbound.hub.budget.response,
                        response_body(&response),
                        outbound.wake(),
                    )
                    .await;
            }
            Err(ReadError::Io(error)) => {
                tracing::error!("failed to read from the client: {error}");
                break;
            }
        }
    }
    drop(tasks);
    outbound.close_server_initiated();
}

async fn dispatch(mut tasks: mpsc::Receiver<Task>, outbound: &Outbound) {
    futures_util::stream::poll_fn(|cx| tasks.poll_recv(cx))
        .buffer_unordered(crate::LSP_MAX_CONCURRENCY)
        .for_each(|(initialize, response)| async move {
            let Some(response) = response else {
                return;
            };
            let succeeded = response.is_ok();
            outbound
                .hub
                .responses
                .push(
                    &outbound.hub.budget.response,
                    response_body(&response),
                    outbound.wake(),
                )
                .await;
            // Queued ahead of any notification it releases: responses are
            // written first.
            if initialize && succeeded {
                outbound.mark_initialized();
            }
        })
        .await;
    outbound.hub.responses.finish();
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

/// Read one framed message; `None` at a clean end of input. Empty bodies are
/// skipped.
async fn read_message<R>(reader: &mut BufReader<R>) -> Result<Option<Incoming>, ReadError>
where
    R: AsyncRead + Unpin,
{
    loop {
        let mut content_length = None;
        let mut malformed = false;
        let mut started = false;
        let mut line = Vec::new();
        loop {
            line.clear();
            let read = reader
                .read_until(b'\n', &mut line)
                .await
                .map_err(ReadError::Io)?;
            if read == 0 {
                if started {
                    return Err(ReadError::Io(io::ErrorKind::UnexpectedEof.into()));
                }
                return Ok(None);
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
            return Err(ReadError::Malformed(Error::parse_error()));
        };
        let mut body = vec![0; length];
        reader.read_exact(&mut body).await.map_err(ReadError::Io)?;
        if malformed {
            return Err(ReadError::Malformed(Error::parse_error()));
        }
        if body.is_empty() {
            continue;
        }
        return match serde_json::from_slice(&body) {
            Ok(message) => Ok(Some(message)),
            Err(error) if error.is_data() => Err(ReadError::Malformed(Error::invalid_request())),
            Err(_) => Err(ReadError::Malformed(Error::parse_error())),
        };
    }
}

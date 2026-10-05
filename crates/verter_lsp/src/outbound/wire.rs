//! An in-process writer for tests that drive a server without a byte stream:
//! it takes the server's control and diagnostics traffic as the transport
//! writer would and yields each message as a JSON-RPC request.

use std::pin::Pin;
use std::task::{Context, Poll};

use futures_util::stream::{BoxStream, Stream};
use futures_util::StreamExt as _;
use tower_lsp_server::jsonrpc::{Request, Response};
use tower_lsp_server::ls_types::notification::{Notification, PublishDiagnostics};

use super::{Outbound, Outgoing, WriterAttachment};

/// The writer of an outbound transport whose client is the test itself.
/// Control messages arrive ahead of diagnostics, as on the wire.
pub(crate) struct Wire {
    outbound: Outbound,
    messages: BoxStream<'static, Request>,
    _attachment: WriterAttachment,
}

impl Outbound {
    /// Attach the test as this transport's writer. As with the real writer,
    /// control traffic flows only once the client has been answered
    /// `initialize`; a test that drives that handshake outside the transport
    /// says so with [`Self::assume_initialized`].
    pub(crate) fn wire(&self) -> Wire {
        let attachment = self.attach_writer();
        let messages = futures_util::stream::unfold(self.clone(), |outbound| async move {
            loop {
                if let Some(outgoing) = outbound.next_outgoing(false) {
                    let message = match &outgoing {
                        Outgoing::Control(body) => serde_json::from_slice(body)
                            .expect("control frames are JSON-RPC requests"),
                        Outgoing::Replaceable(taken) => Request::build(PublishDiagnostics::METHOD)
                            .params(
                                serde_json::to_value(&taken.params)
                                    .expect("diagnostics serialize to JSON"),
                            )
                            .finish(),
                        Outgoing::Response(_) => unreachable!("responses are not taken"),
                    };
                    outbound.complete(outgoing);
                    return Some((message, outbound));
                }
                outbound.wake().notified().await;
            }
        })
        .boxed();
        Wire {
            outbound: self.clone(),
            messages,
            _attachment: attachment,
        }
    }
}

impl Outbound {
    /// The test answered `initialize` itself, outside the transport.
    pub(crate) fn assume_initialized(&self) {
        self.mark_initialized();
    }
}

impl Wire {
    /// Answer a server→client request.
    pub(crate) fn reply(&self, response: Response) {
        self.outbound.route_reply(response);
    }
}

impl Stream for Wire {
    type Item = Request;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Request>> {
        self.messages.as_mut().poll_next(cx)
    }
}

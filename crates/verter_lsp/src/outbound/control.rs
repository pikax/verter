//! The ordered, accounted route for control notifications whose producer cannot
//! await its own send.
//!
//! A synchronous callback that spawned one detached send per message would park
//! one more message in the client channel for every event while the editor is
//! not reading, with nothing counting them. A [`ControlLane`] instead owns the
//! messages, sends them in order one at a time, and reports every message it
//! holds. Control messages are never coalesced or dropped.

use std::collections::VecDeque;
use std::sync::Arc;

use futures_util::future::BoxFuture;
use parking_lot::Mutex;
use tokio::sync::OnceCell;
use tower_lsp_server::ls_types::notification::Notification;
use tower_lsp_server::Client;

use super::{serialized_len, Load, Owner, Retirement};

type QueuedSend = Box<dyn FnOnce(Client) -> BoxFuture<'static, ()> + Send>;

/// Detached control notifications, sent in order through one client.
#[derive(Clone)]
pub struct ControlLane {
    inner: Arc<Inner>,
    _owner: Arc<Owner>,
}

struct Inner {
    client: Arc<OnceCell<Client>>,
    state: Mutex<State>,
    retirement: Arc<Retirement>,
}

#[derive(Default)]
struct State {
    queue: VecDeque<(usize, QueuedSend)>,
    /// Queued messages plus the one handed to the transport and not yet
    /// accepted by it.
    load: Load,
    high_water: Load,
    draining: bool,
}

impl ControlLane {
    /// A lane sending through the client `client` holds once the server is
    /// built; messages offered before then are not sent, as with any
    /// notification an uninitialized server emits.
    pub fn new(client: Arc<OnceCell<Client>>) -> Self {
        let retirement = Arc::new(Retirement::default());
        Self {
            inner: Arc::new(Inner {
                client,
                state: Mutex::new(State::default()),
                retirement: Arc::clone(&retirement),
            }),
            _owner: Arc::new(Owner(retirement)),
        }
    }

    /// Queue one notification behind every message offered before it.
    pub fn notify<N>(&self, params: N::Params)
    where
        N: Notification,
        N::Params: Send + 'static,
    {
        let bytes = serialized_len(&params);
        let send: QueuedSend = Box::new(move |client: Client| {
            Box::pin(async move { client.send_notification::<N>(params).await })
        });
        let mut state = self.inner.state.lock();
        state.queue.push_back((bytes, send));
        state.load.add(bytes);
        state.high_water = state.high_water.max(state.load);
        if !state.draining {
            state.draining = true;
            tokio::spawn(drain(Arc::clone(&self.inner)));
        }
    }

    /// What the lane holds right now.
    pub fn load(&self) -> Load {
        self.inner.state.lock().load
    }

    /// The largest load the lane has held since it was built.
    pub fn high_water(&self) -> Load {
        self.inner.state.lock().high_water
    }
}

async fn drain(inner: Arc<Inner>) {
    loop {
        let (bytes, send) = {
            let mut state = inner.state.lock();
            let Some(next) = state.queue.pop_front() else {
                state.draining = false;
                return;
            };
            next
        };
        if let Some(client) = inner.client.get() {
            tokio::select! {
                biased;
                () = inner.retirement.retired() => return,
                () = send(client.clone()) => {}
            }
        }
        inner.state.lock().load.remove(bytes);
    }
}

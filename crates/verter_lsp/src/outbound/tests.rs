//! Lane semantics observed on a real `Client` whose wire nobody reads until the
//! test says so — the slow editor every case here models.

use std::sync::Arc;

use futures_util::StreamExt as _;
use tower_lsp_server::ls_types::notification::ShowMessage;
use tower_lsp_server::ls_types::{
    Diagnostic, MessageType, PublishDiagnosticsParams, Range, ShowMessageParams, Uri,
};
use tower_lsp_server::{Client, ClientSocket};

use super::{serialized_len, ControlLane, Delivery, Load, OutboundBudget, ReplaceableLane};

/// Await `future`, failing the test instead of hanging when the outcome it
/// waits for never comes.
async fn bounded<F: std::future::Future>(future: F) -> F::Output {
    tokio::time::timeout(std::time::Duration::from_secs(10), future)
        .await
        .expect("the awaited lane outcome never arrived")
}

async fn stalled_client() -> (Client, ClientSocket) {
    let (cell, wire) = crate::test_utils::initialized_client_with_socket().await;
    (
        cell.get().expect("handshake populated the client").clone(),
        wire,
    )
}

fn params(uri: &str, messages: &[&str]) -> PublishDiagnosticsParams {
    PublishDiagnosticsParams::new(
        uri.parse::<Uri>().unwrap(),
        messages
            .iter()
            .map(|m| Diagnostic::new_simple(Range::default(), (*m).to_string()))
            .collect(),
        None,
    )
}

/// A payload of `count` diagnostics, each `width` bytes of message text.
fn wide(uri: &str, tag: &str, count: usize, width: usize) -> PublishDiagnosticsParams {
    let messages: Vec<String> = (0..count)
        .map(|i| format!("{tag}-{i}-{}", "x".repeat(width)))
        .collect();
    let refs: Vec<&str> = messages.iter().map(String::as_str).collect();
    params(uri, &refs)
}

/// Every diagnostics payload the wire carried, as `(uri, joined messages)`,
/// reading until `count` have arrived.
async fn read(wire: &mut ClientSocket, count: usize) -> Vec<(String, String)> {
    let mut received = Vec::new();
    while received.len() < count {
        let message = wire.next().await.expect("the wire stays open");
        let params: PublishDiagnosticsParams =
            serde_json::from_value(message.params().cloned().expect("params"))
                .expect("publishDiagnostics params decode");
        let text = params
            .diagnostics
            .iter()
            .map(|d| d.message.as_str())
            .collect::<Vec<_>>()
            .join(",");
        received.push((params.uri.as_str().to_owned(), text));
    }
    received
}

fn joined(params: &PublishDiagnosticsParams) -> String {
    params
        .diagnostics
        .iter()
        .map(|d| d.message.as_str())
        .collect::<Vec<_>>()
        .join(",")
}

const FILLER: &str = "file:///filler.vue";

/// Occupy the transport: the first payload takes its buffered slot and the
/// second parks the pump, so later offers stay in the lane. Returns the
/// serialized size of the parked payload, which stays admitted.
async fn occupy_transport(lane: &ReplaceableLane, client: &Client) -> usize {
    assert_eq!(
        lane.publish_diagnostics(client, 1, params(FILLER, &["filler-1"]))
            .await,
        Delivery::Delivered
    );
    let second = params(FILLER, &["filler-2"]);
    let bytes = serialized_len(&second);
    let parked = lane.publish_diagnostics(client, 2, second);
    tokio::pin!(parked);
    assert!(futures_util::poll!(&mut parked).is_pending());
    tokio::task::yield_now().await;
    assert_eq!(
        lane.load().admitted,
        Load { messages: 1, bytes },
        "the second filler is handed to the transport and stays in flight"
    );
    // The parked send stays committed to the transport; its publisher may go.
    bytes
}

#[tokio::test(flavor = "current_thread")]
async fn a_newer_publication_replaces_the_pending_payload_in_place() {
    let (client, mut wire) = stalled_client().await;
    let lane = ReplaceableLane::new(OutboundBudget::DEFAULT);
    occupy_transport(&lane, &client).await;

    let older = lane.publish_diagnostics(&client, 1, params("file:///a.vue", &["old"]));
    tokio::pin!(older);
    assert!(futures_util::poll!(&mut older).is_pending());
    let newer = lane.publish_diagnostics(&client, 2, params("file:///a.vue", &["new"]));
    tokio::pin!(newer);
    assert!(futures_util::poll!(&mut newer).is_pending());

    assert_eq!(bounded(older).await, Delivery::Superseded);
    assert_eq!(
        lane.load().retained().messages,
        2,
        "one payload in flight and one pending for the edited document"
    );
    let received = bounded(read(&mut wire, 3)).await;
    assert_eq!(bounded(newer).await, Delivery::Delivered);
    assert_eq!(
        received.last(),
        Some(&("file:///a.vue".to_owned(), "new".to_owned()))
    );
    assert!(
        received.iter().all(|(_, text)| text != "old"),
        "the replaced payload never reaches the client: {received:?}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn an_older_publication_never_overtakes_a_newer_one() {
    let (client, _wire) = stalled_client().await;
    let lane = ReplaceableLane::new(OutboundBudget::DEFAULT);
    occupy_transport(&lane, &client).await;

    let newer = lane.publish_diagnostics(&client, 7, params("file:///a.vue", &["new"]));
    tokio::pin!(newer);
    assert!(futures_util::poll!(&mut newer).is_pending());
    assert_eq!(
        lane.publish_diagnostics(&client, 6, params("file:///a.vue", &["old"]))
            .await,
        Delivery::Superseded,
        "an older epoch is refused without displacing the pending newer payload"
    );
    assert_eq!(lane.load().admitted.messages, 2);
}

/// The message budget backpressures a document with nothing pending, never
/// drops it, counts the payload it holds back, and a withdrawn publication
/// gives its capacity back.
#[tokio::test(flavor = "current_thread")]
async fn a_full_lane_holds_new_documents_back_and_withdrawal_frees_capacity() {
    let (client, mut wire) = stalled_client().await;
    let lane = ReplaceableLane::new(OutboundBudget {
        replaceable_messages: 2,
        replaceable_bytes: usize::MAX,
    });
    occupy_transport(&lane, &client).await;

    // Boxed so the test can drop (withdraw) it mid-flight.
    let mut first = Box::pin(lane.publish_diagnostics(&client, 1, params("file:///b.vue", &["b"])));
    assert!(futures_util::poll!(&mut first).is_pending());
    let held_back = params("file:///c.vue", &["c"]);
    let held_back_bytes = serialized_len(&held_back);
    let waiting = lane.publish_diagnostics(&client, 1, held_back);
    tokio::pin!(waiting);
    assert!(futures_util::poll!(&mut waiting).is_pending());
    let load = lane.load();
    assert_eq!(
        load.admitted.messages, 2,
        "the waiting document is not admitted"
    );
    assert_eq!(
        load.waiting,
        Load {
            messages: 1,
            bytes: held_back_bytes
        },
        "the payload held back is still the lane's, and counted"
    );

    // Withdrawing the pending publication admits the waiting one.
    drop(first);
    assert!(futures_util::poll!(&mut waiting).is_pending());
    assert_eq!(lane.load().admitted.messages, 2);
    assert_eq!(lane.load().waiting, Load::default());
    let (received, delivery) = bounded(async { tokio::join!(read(&mut wire, 3), waiting) }).await;
    assert_eq!(delivery, Delivery::Delivered);
    assert!(
        received.iter().all(|(uri, _)| uri != "file:///b.vue"),
        "a withdrawn payload never reaches the client: {received:?}"
    );
    assert_eq!(received[2], ("file:///c.vue".to_owned(), "c".to_owned()));
    assert_eq!(lane.high_water().admitted.messages, 2);
    assert_eq!(lane.high_water().retained.messages, 3);
}

/// The byte budget backpressures on its own: with message slots to spare, a
/// payload that does not fit the remaining bytes waits, and is delivered whole
/// once the bytes ahead of it drain.
#[tokio::test(flavor = "current_thread")]
async fn the_byte_budget_holds_a_payload_back_while_message_slots_remain() {
    let (client, mut wire) = stalled_client().await;
    let pending = wide("file:///b.vue", "b", 4, 64);
    let heavy = wide("file:///c.vue", "c", 8, 64);
    let in_flight = serialized_len(&params(FILLER, &["filler-2"]));
    let budget = OutboundBudget {
        replaceable_messages: 16,
        replaceable_bytes: in_flight + serialized_len(&pending) + serialized_len(&heavy) - 1,
    };
    let lane = ReplaceableLane::new(budget);
    occupy_transport(&lane, &client).await;

    let first = lane.publish_diagnostics(&client, 1, pending);
    tokio::pin!(first);
    assert!(futures_util::poll!(&mut first).is_pending());
    let expected = joined(&heavy);
    let held_back = lane.publish_diagnostics(&client, 1, heavy);
    tokio::pin!(held_back);
    assert!(futures_util::poll!(&mut held_back).is_pending());
    let load = lane.load();
    assert!(load.admitted.messages < budget.replaceable_messages);
    assert_eq!(
        (load.admitted.messages, load.waiting.messages),
        (2, 1),
        "the heavy payload waits for bytes, not for a message slot"
    );

    let (received, first, held_back) =
        bounded(async { tokio::join!(read(&mut wire, 4), first, held_back) }).await;
    assert_eq!(
        (first, held_back),
        (Delivery::Delivered, Delivery::Delivered)
    );
    assert_eq!(received[3], ("file:///c.vue".to_owned(), expected));
    let high_water = lane.high_water();
    assert!(
        high_water.admitted.bytes <= budget.replaceable_bytes,
        "the admitted set never exceeds the byte budget: {high_water:?}"
    );
}

/// A newer publication retires the document's older pending payload even when
/// the newer one is too large to take its place and has to wait.
#[tokio::test(flavor = "current_thread")]
async fn a_larger_newer_publication_retires_the_older_one_while_it_waits() {
    let (client, mut wire) = stalled_client().await;
    let old = params("file:///a.vue", &["old"]);
    let in_flight = serialized_len(&params(FILLER, &["filler-2"]));
    let lane = ReplaceableLane::new(OutboundBudget {
        replaceable_messages: 16,
        replaceable_bytes: in_flight + serialized_len(&old),
    });
    occupy_transport(&lane, &client).await;

    let older = lane.publish_diagnostics(&client, 1, old);
    tokio::pin!(older);
    assert!(futures_util::poll!(&mut older).is_pending());
    let new = wide("file:///a.vue", "new", 8, 64);
    let new_bytes = serialized_len(&new);
    let expected = joined(&new);
    let newer = lane.publish_diagnostics(&client, 2, new);
    tokio::pin!(newer);
    assert!(futures_util::poll!(&mut newer).is_pending());

    assert_eq!(
        bounded(older).await,
        Delivery::Superseded,
        "the older payload is retired as soon as the newer epoch is offered"
    );
    let load = lane.load();
    assert_eq!(
        load.admitted,
        Load {
            messages: 1,
            bytes: in_flight
        },
        "only the parked filler remains admitted"
    );
    assert_eq!(
        load.waiting,
        Load {
            messages: 1,
            bytes: new_bytes
        }
    );

    let (received, delivery) = bounded(async { tokio::join!(read(&mut wire, 3), newer) }).await;
    assert_eq!(delivery, Delivery::Delivered);
    assert_eq!(received[2], ("file:///a.vue".to_owned(), expected));
    assert!(
        received.iter().all(|(_, text)| text != "old"),
        "the retired payload never reaches the client: {received:?}"
    );
}

/// A payload that needs the lane to itself is admitted before payloads offered
/// after it, however small they are.
#[tokio::test(flavor = "current_thread")]
async fn a_large_waiting_payload_is_not_overtaken_by_smaller_ones() {
    let (client, mut wire) = stalled_client().await;
    let lane = ReplaceableLane::new(OutboundBudget {
        replaceable_messages: 16,
        replaceable_bytes: 512,
    });
    occupy_transport(&lane, &client).await;

    let big = wide("file:///big.vue", "big", 16, 64);
    assert!(serialized_len(&big) > 512);
    let big = lane.publish_diagnostics(&client, 1, big);
    tokio::pin!(big);
    assert!(futures_util::poll!(&mut big).is_pending());
    let small = lane.publish_diagnostics(&client, 1, params("file:///small.vue", &["s"]));
    tokio::pin!(small);
    assert!(futures_util::poll!(&mut small).is_pending());
    assert_eq!(
        (lane.load().admitted.messages, lane.load().waiting.messages),
        (1, 2),
        "the small payload fits the free room but waits behind the large one"
    );

    let (received, big, small) =
        bounded(async { tokio::join!(read(&mut wire, 4), big, small) }).await;
    assert_eq!((big, small), (Delivery::Delivered, Delivery::Delivered));
    let order: Vec<&str> = received.iter().map(|(uri, _)| uri.as_str()).collect();
    assert_eq!(
        &order[2..],
        ["file:///big.vue", "file:///small.vue"],
        "waiting payloads are admitted in offer order"
    );
}

/// A complete result larger than the whole byte budget is delivered intact.
#[tokio::test(flavor = "current_thread")]
async fn a_payload_larger_than_the_byte_budget_is_delivered_whole() {
    let (client, mut wire) = stalled_client().await;
    let lane = ReplaceableLane::new(OutboundBudget {
        replaceable_messages: 4,
        replaceable_bytes: 64,
    });
    let messages: Vec<String> = (0..500).map(|i| format!("diagnostic-{i}")).collect();
    let refs: Vec<&str> = messages.iter().map(String::as_str).collect();
    let (delivery, received) = bounded(async {
        tokio::join!(
            lane.publish_diagnostics(&client, 1, params("file:///big.vue", &refs)),
            read(&mut wire, 1)
        )
    })
    .await;
    assert_eq!(delivery, Delivery::Delivered);
    assert_eq!(received[0].1, refs.join(","));
    assert!(
        lane.high_water().admitted.bytes > 64,
        "the oversize payload was admitted alone"
    );
    assert_eq!(lane.load().retained(), Load::default());
}

/// Dropping the last lane handle ends a pump parked on a slow client and
/// releases everything it held.
#[tokio::test(flavor = "current_thread")]
async fn the_pump_ends_with_the_last_lane_handle() {
    let (client, _wire) = stalled_client().await;
    let lane = ReplaceableLane::new(OutboundBudget::DEFAULT);
    occupy_transport(&lane, &client).await;
    let state = Arc::downgrade(&lane.inner);
    let clone = lane.clone();
    drop(lane);
    tokio::task::yield_now().await;
    assert!(
        state.upgrade().is_some(),
        "a remaining handle keeps the lane and its pump alive"
    );

    drop(clone);
    for _ in 0..16 {
        if state.upgrade().is_none() {
            return;
        }
        tokio::task::yield_now().await;
    }
    panic!("the parked pump still holds the lane after its last handle dropped");
}

/// Notifications from a producer that cannot await reach the client in order,
/// and the lane accounts for every one it holds while the client is not reading.
#[tokio::test(flavor = "current_thread")]
async fn a_control_lane_sends_in_order_and_accounts_what_it_holds() {
    let (cell, mut wire) = crate::test_utils::initialized_client_with_socket().await;
    let control = ControlLane::new(cell);
    let sent: Vec<ShowMessageParams> = (0..5)
        .map(|i| ShowMessageParams {
            typ: MessageType::WARNING,
            message: format!("warning-{i}"),
        })
        .collect();
    for message in &sent {
        control.notify::<ShowMessage>(message.clone());
    }
    let offered = Load {
        messages: sent.len(),
        bytes: sent.iter().map(serialized_len).sum(),
    };
    assert_eq!(control.load(), offered, "every queued message is counted");
    tokio::task::yield_now().await;
    let held = control.load();
    assert!(
        held.messages >= sent.len() - 1 && held.messages < sent.len(),
        "the drain hands one message at a time to the stalled transport: {held:?}"
    );
    assert_eq!(control.high_water(), offered);

    let mut received = Vec::new();
    while received.len() < sent.len() {
        let message = bounded(wire.next()).await.expect("the wire stays open");
        let params: ShowMessageParams =
            serde_json::from_value(message.params().cloned().expect("params"))
                .expect("showMessage params decode");
        received.push(params.message);
    }
    let expected: Vec<String> = sent.into_iter().map(|m| m.message).collect();
    assert_eq!(received, expected, "control messages keep their order");
}

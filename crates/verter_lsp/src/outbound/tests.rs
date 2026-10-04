//! Lane semantics observed on a real `Client` whose wire nobody reads until the
//! test says so — the slow editor every case here models.

use futures_util::StreamExt as _;
use tower_lsp_server::ls_types::{Diagnostic, PublishDiagnosticsParams, Range, Uri};
use tower_lsp_server::{Client, ClientSocket};

use super::{Delivery, LaneLoad, OutboundBudget, ReplaceableLane};

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

/// Occupy the transport: the first payload takes its buffered slot and the
/// second parks the pump, so later offers stay in the lane.
async fn occupy_transport(lane: &ReplaceableLane, client: &Client) {
    let filler = "file:///filler.vue";
    assert_eq!(
        lane.publish_diagnostics(client, 1, params(filler, &["filler-1"]))
            .await,
        Delivery::Delivered
    );
    let parked = lane.publish_diagnostics(client, 2, params(filler, &["filler-2"]));
    tokio::pin!(parked);
    assert!(futures_util::poll!(&mut parked).is_pending());
    tokio::task::yield_now().await;
    assert_eq!(
        lane.load().messages,
        1,
        "the second filler is handed to the transport and stays in flight"
    );
    // The parked send stays committed to the transport; its publisher may go.
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

    assert_eq!(older.await, Delivery::Superseded);
    assert_eq!(
        lane.load().messages,
        2,
        "one payload in flight and one pending for the edited document"
    );
    let received = read(&mut wire, 3).await;
    assert_eq!(newer.await, Delivery::Delivered);
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
    assert_eq!(lane.load().messages, 2);
}

/// The budget backpressures a document with nothing pending, never drops it,
/// and a withdrawn publication gives its capacity back.
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
    let waiting = lane.publish_diagnostics(&client, 1, params("file:///c.vue", &["c"]));
    tokio::pin!(waiting);
    assert!(futures_util::poll!(&mut waiting).is_pending());
    assert_eq!(
        lane.high_water().messages,
        2,
        "the waiting document is not admitted"
    );

    // Withdrawing the pending publication admits the waiting one.
    drop(first);
    assert!(futures_util::poll!(&mut waiting).is_pending());
    assert_eq!(lane.load().messages, 2);
    let (received, delivery) = tokio::join!(read(&mut wire, 3), waiting);
    assert_eq!(delivery, Delivery::Delivered);
    assert!(
        received.iter().all(|(uri, _)| uri != "file:///b.vue"),
        "a withdrawn payload never reaches the client: {received:?}"
    );
    assert_eq!(received[2], ("file:///c.vue".to_owned(), "c".to_owned()));
    assert_eq!(lane.high_water().messages, 2);
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
    let (delivery, received) = tokio::join!(
        lane.publish_diagnostics(&client, 1, params("file:///big.vue", &refs)),
        read(&mut wire, 1)
    );
    assert_eq!(delivery, Delivery::Delivered);
    assert_eq!(received[0].1, refs.join(","));
    assert!(
        lane.high_water().bytes > 64,
        "the oversize payload was admitted alone"
    );
    assert_eq!(lane.load(), LaneLoad::default());
}

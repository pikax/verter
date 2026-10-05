//! Transport semantics observed through the in-process writer, which takes
//! nothing until the test reads — the slow editor every case here models.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use futures_util::{FutureExt as _, StreamExt as _};
use tower_lsp_server::jsonrpc::{Request, Response};
use tower_lsp_server::ls_types::notification::{Notification, PublishDiagnostics, ShowMessage};
use tower_lsp_server::ls_types::request::WorkspaceConfiguration;
use tower_lsp_server::ls_types::{
    ConfigurationParams, Diagnostic, MessageType, PublishDiagnosticsParams, Range,
    ShowMessageParams, Uri,
};

use super::replaceable::{diagnostics_body, NotificationBody};
use super::{
    serialized_len, ClassBudget, Delivery, Load, Outbound, OutboundBudget, ReplaceableLane, Wire,
};

/// Await `future`, failing the test instead of hanging when the outcome it
/// waits for never comes.
async fn bounded<F: std::future::Future>(future: F) -> F::Output {
    tokio::time::timeout(std::time::Duration::from_secs(10), future)
        .await
        .expect("the awaited transport outcome never arrived")
}

/// A transport whose replaceable class has `budget`, and its lane.
fn transport(budget: ClassBudget) -> (Outbound, ReplaceableLane) {
    let outbound = Outbound::new(OutboundBudget {
        replaceable: budget,
        ..OutboundBudget::DEFAULT
    });
    let lane = outbound.diagnostics_lane();
    (outbound, lane)
}

fn current() -> bool {
    true
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

/// The accounted size of a diagnostics payload: its whole notification body.
fn size(params: &PublishDiagnosticsParams) -> usize {
    serialized_len(&diagnostics_body(params))
}

/// A payload of `count` diagnostics, each `width` bytes of message text.
fn wide(uri: &str, tag: &str, count: usize, width: usize) -> PublishDiagnosticsParams {
    let messages: Vec<String> = (0..count)
        .map(|i| format!("{tag}-{i}-{}", "x".repeat(width)))
        .collect();
    let refs: Vec<&str> = messages.iter().map(String::as_str).collect();
    params(uri, &refs)
}

fn joined(params: &PublishDiagnosticsParams) -> String {
    params
        .diagnostics
        .iter()
        .map(|d| d.message.as_str())
        .collect::<Vec<_>>()
        .join(",")
}

/// Everything the writer would write right now, in order, without waiting.
fn written_now(wire: &mut Wire) -> Vec<Request> {
    let mut written = Vec::new();
    while let Some(Some(message)) = wire.next().now_or_never() {
        written.push(message);
    }
    written
}

/// The diagnostics among `messages`, as `(uri, joined messages)`.
fn diagnostics(messages: &[Request]) -> Vec<(String, String)> {
    messages
        .iter()
        .filter(|m| m.method() == PublishDiagnostics::METHOD)
        .map(|m| {
            let params: PublishDiagnosticsParams =
                serde_json::from_value(m.params().cloned().expect("params"))
                    .expect("publishDiagnostics params decode");
            (params.uri.as_str().to_owned(), joined(&params))
        })
        .collect()
}

#[tokio::test(flavor = "current_thread")]
async fn a_newer_publication_replaces_the_pending_payload_in_place() {
    let (outbound, lane) = transport(OutboundBudget::DEFAULT.replaceable);
    let older = lane.publish_diagnostics(1, params("file:///a.vue", &["old"]), current);
    tokio::pin!(older);
    assert!(futures_util::poll!(&mut older).is_pending());
    let newer = lane.publish_diagnostics(2, params("file:///a.vue", &["new"]), current);
    tokio::pin!(newer);
    assert!(futures_util::poll!(&mut newer).is_pending());

    assert_eq!(bounded(older).await, Delivery::Superseded);
    assert_eq!(lane.load().retained().messages, 1);

    let mut wire = outbound.wire();
    assert_eq!(
        diagnostics(&written_now(&mut wire)),
        vec![("file:///a.vue".to_owned(), "new".to_owned())],
        "the replaced payload never reaches the client"
    );
    assert_eq!(bounded(newer).await, Delivery::Delivered);
}

#[tokio::test(flavor = "current_thread")]
async fn an_older_publication_never_overtakes_a_newer_one() {
    let (_outbound, lane) = transport(OutboundBudget::DEFAULT.replaceable);
    let newer = lane.publish_diagnostics(7, params("file:///a.vue", &["new"]), current);
    tokio::pin!(newer);
    assert!(futures_util::poll!(&mut newer).is_pending());
    assert_eq!(
        lane.publish_diagnostics(6, params("file:///a.vue", &["old"]), current)
            .await,
        Delivery::Superseded,
        "an older epoch is refused without displacing the pending newer payload"
    );
    assert_eq!(lane.load().admitted.messages, 1);
}

/// The message budget backpressures a document with nothing pending, never
/// drops it, counts the payload it holds back, and a withdrawn publication
/// gives its capacity back.
#[tokio::test(flavor = "current_thread")]
async fn a_full_lane_holds_new_documents_back_and_withdrawal_frees_capacity() {
    let (outbound, lane) = transport(ClassBudget {
        messages: 2,
        bytes: usize::MAX,
    });
    let first = lane.publish_diagnostics(1, params("file:///a.vue", &["a"]), current);
    tokio::pin!(first);
    assert!(futures_util::poll!(&mut first).is_pending());
    // Boxed so the test can drop (withdraw) it mid-flight.
    let mut second =
        Box::pin(lane.publish_diagnostics(1, params("file:///b.vue", &["b"]), current));
    assert!(futures_util::poll!(&mut second).is_pending());
    let held_back = params("file:///c.vue", &["c"]);
    let held_back_bytes = size(&held_back);
    let waiting = lane.publish_diagnostics(1, held_back, current);
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
    drop(second);
    assert_eq!(lane.load().admitted.messages, 2);
    assert_eq!(lane.load().waiting, Load::default());

    let mut wire = outbound.wire();
    assert_eq!(
        diagnostics(&written_now(&mut wire)),
        vec![
            ("file:///a.vue".to_owned(), "a".to_owned()),
            ("file:///c.vue".to_owned(), "c".to_owned()),
        ],
        "a withdrawn payload never reaches the client"
    );
    assert_eq!(bounded(first).await, Delivery::Delivered);
    assert_eq!(bounded(waiting).await, Delivery::Delivered);
    assert_eq!(lane.high_water().admitted.messages, 2);
    assert_eq!(lane.high_water().retained.messages, 3);
}

/// The byte budget backpressures on its own: with message slots to spare, a
/// payload that does not fit the remaining bytes waits, and is delivered whole
/// once the bytes ahead of it drain.
#[tokio::test(flavor = "current_thread")]
async fn the_byte_budget_holds_a_payload_back_while_message_slots_remain() {
    let pending = wide("file:///b.vue", "b", 4, 64);
    let heavy = wide("file:///c.vue", "c", 8, 64);
    let budget = ClassBudget {
        messages: 16,
        bytes: size(&pending) + size(&heavy) - 1,
    };
    let (outbound, lane) = transport(budget);

    let first = lane.publish_diagnostics(1, pending, current);
    tokio::pin!(first);
    assert!(futures_util::poll!(&mut first).is_pending());
    let expected = joined(&heavy);
    let held_back = lane.publish_diagnostics(1, heavy, current);
    tokio::pin!(held_back);
    assert!(futures_util::poll!(&mut held_back).is_pending());
    let load = lane.load();
    assert_eq!(
        (load.admitted.messages, load.waiting.messages),
        (1, 1),
        "the heavy payload waits for bytes, not for a message slot"
    );

    let mut wire = outbound.wire();
    let written = diagnostics(&written_now(&mut wire));
    assert_eq!(written[1], ("file:///c.vue".to_owned(), expected));
    assert_eq!(
        (bounded(first).await, bounded(held_back).await),
        (Delivery::Delivered, Delivery::Delivered)
    );
    let high_water = lane.high_water();
    assert!(
        high_water.admitted.bytes <= budget.bytes,
        "the admitted set never exceeds the byte budget: {high_water:?}"
    );
}

/// A newer publication retires the document's older pending payload even when
/// the newer one is too large to take its place and has to wait.
#[tokio::test(flavor = "current_thread")]
async fn a_larger_newer_publication_retires_the_older_one_while_it_waits() {
    let other = params("file:///other.vue", &["other"]);
    let old = params("file:///a.vue", &["old"]);
    let (outbound, lane) = transport(ClassBudget {
        messages: 16,
        bytes: size(&other) + size(&old),
    });
    let other_send = lane.publish_diagnostics(1, other.clone(), current);
    tokio::pin!(other_send);
    assert!(futures_util::poll!(&mut other_send).is_pending());
    let older = lane.publish_diagnostics(1, old, current);
    tokio::pin!(older);
    assert!(futures_util::poll!(&mut older).is_pending());
    let new = wide("file:///a.vue", "new", 8, 64);
    let new_bytes = size(&new);
    let expected = joined(&new);
    let newer = lane.publish_diagnostics(2, new, current);
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
            bytes: size(&other)
        },
        "only the other document remains admitted"
    );
    assert_eq!(
        load.waiting,
        Load {
            messages: 1,
            bytes: new_bytes
        }
    );

    let mut wire = outbound.wire();
    let written = diagnostics(&written_now(&mut wire));
    assert_eq!(written[1], ("file:///a.vue".to_owned(), expected));
    assert!(
        written.iter().all(|(_, text)| text != "old"),
        "the retired payload never reaches the client: {written:?}"
    );
    assert_eq!(bounded(newer).await, Delivery::Delivered);
}

/// A payload that needs the lane to itself is admitted before payloads offered
/// after it, however small they are.
#[tokio::test(flavor = "current_thread")]
async fn a_large_waiting_payload_is_not_overtaken_by_smaller_ones() {
    let (outbound, lane) = transport(ClassBudget {
        messages: 16,
        bytes: 512,
    });
    let first = lane.publish_diagnostics(1, params("file:///first.vue", &["f"]), current);
    tokio::pin!(first);
    assert!(futures_util::poll!(&mut first).is_pending());
    let big = wide("file:///big.vue", "big", 16, 64);
    assert!(size(&big) > 512);
    let big = lane.publish_diagnostics(1, big, current);
    tokio::pin!(big);
    assert!(futures_util::poll!(&mut big).is_pending());
    let small = lane.publish_diagnostics(1, params("file:///small.vue", &["s"]), current);
    tokio::pin!(small);
    assert!(futures_util::poll!(&mut small).is_pending());
    assert_eq!(
        (lane.load().admitted.messages, lane.load().waiting.messages),
        (1, 2),
        "the small payload fits the free room but waits behind the large one"
    );

    let mut wire = outbound.wire();
    let order: Vec<String> = diagnostics(&written_now(&mut wire))
        .into_iter()
        .map(|(uri, _)| uri)
        .collect();
    assert_eq!(
        order,
        ["file:///first.vue", "file:///big.vue", "file:///small.vue"],
        "waiting payloads are admitted in offer order"
    );
}

/// A complete result larger than the whole byte budget is delivered intact.
#[tokio::test(flavor = "current_thread")]
async fn a_payload_larger_than_the_byte_budget_is_delivered_whole() {
    let (outbound, lane) = transport(ClassBudget {
        messages: 4,
        bytes: 64,
    });
    let mut wire = outbound.wire();
    let messages: Vec<String> = (0..500).map(|i| format!("diagnostic-{i}")).collect();
    let refs: Vec<&str> = messages.iter().map(String::as_str).collect();
    let (delivery, written) = bounded(async {
        tokio::join!(
            lane.publish_diagnostics(1, params("file:///big.vue", &refs), current),
            wire.next()
        )
    })
    .await;
    assert_eq!(delivery, Delivery::Delivered);
    assert_eq!(
        diagnostics(&[written.expect("the wire stays open")])[0].1,
        refs.join(",")
    );
    assert!(
        lane.high_water().admitted.bytes > 64,
        "the oversize payload was admitted alone"
    );
    assert_eq!(lane.load().retained(), Load::default());
}

/// The writer takes one payload at a time, and the payload stays accounted
/// until its write completes.
#[tokio::test(flavor = "current_thread")]
async fn a_taken_payload_stays_accounted_until_written() {
    let (_outbound, lane) = transport(OutboundBudget::DEFAULT.replaceable);
    let a = params("file:///a.vue", &["a"]);
    let a_bytes = size(&a);
    let first = lane.publish_diagnostics(1, a, current);
    tokio::pin!(first);
    assert!(futures_util::poll!(&mut first).is_pending());
    let second = lane.publish_diagnostics(1, params("file:///b.vue", &["b"]), current);
    tokio::pin!(second);
    assert!(futures_util::poll!(&mut second).is_pending());

    let taken = lane.take().expect("an admitted payload is ready");
    assert_eq!(taken.params.uri.as_str(), "file:///a.vue");
    assert!(
        lane.take().is_none(),
        "nothing more is taken while a payload is being written"
    );
    assert_eq!(lane.load().admitted.messages, 2);
    assert!(lane.load().admitted.bytes >= a_bytes);
    assert!(futures_util::poll!(&mut first).is_pending());

    lane.complete(taken);
    assert_eq!(bounded(first).await, Delivery::Delivered);
    assert_eq!(lane.load().admitted.messages, 1);
}

/// A publication that is no longer current when the writer reaches it is
/// retired there, never written — even though its publisher never withdrew it.
#[tokio::test(flavor = "current_thread")]
async fn a_payload_no_longer_current_at_the_take_is_retired() {
    let (outbound, lane) = transport(OutboundBudget::DEFAULT.replaceable);
    let stale = Arc::new(AtomicBool::new(false));
    let stale_send = lane.publish_diagnostics(1, params("file:///a.vue", &["stale"]), {
        let stale = Arc::clone(&stale);
        move || !stale.load(Ordering::SeqCst)
    });
    tokio::pin!(stale_send);
    assert!(futures_util::poll!(&mut stale_send).is_pending());
    let other = lane.publish_diagnostics(1, params("file:///b.vue", &["b"]), current);
    tokio::pin!(other);
    assert!(futures_util::poll!(&mut other).is_pending());

    stale.store(true, Ordering::SeqCst);
    let mut wire = outbound.wire();
    assert_eq!(
        diagnostics(&written_now(&mut wire)),
        vec![("file:///b.vue".to_owned(), "b".to_owned())]
    );
    assert_eq!(bounded(stale_send).await, Delivery::Superseded);
    assert_eq!(lane.load().retained(), Load::default());
}

/// When the transport ends, every held publication resolves as closed and new
/// ones are refused.
#[tokio::test(flavor = "current_thread")]
async fn an_ended_transport_releases_and_refuses_publications() {
    let (outbound, lane) = transport(OutboundBudget::DEFAULT.replaceable);
    let held = lane.publish_diagnostics(1, params("file:///a.vue", &["a"]), current);
    tokio::pin!(held);
    assert!(futures_util::poll!(&mut held).is_pending());
    drop(outbound.wire());
    assert_eq!(bounded(held).await, Delivery::Closed);
    assert_eq!(
        lane.publish_diagnostics(2, params("file:///a.vue", &["b"]), current)
            .await,
        Delivery::Closed
    );
    assert_eq!(lane.load().retained(), Load::default());
}

fn warning(i: usize) -> ShowMessageParams {
    ShowMessageParams {
        typ: MessageType::WARNING,
        message: format!("warning-{i}"),
    }
}

fn control_size(params: &ShowMessageParams) -> usize {
    serialized_len(&NotificationBody {
        jsonrpc: "2.0",
        method: ShowMessage::METHOD,
        params,
    })
}

fn shown(messages: &[Request]) -> Vec<String> {
    messages
        .iter()
        .filter(|m| m.method() == ShowMessage::METHOD)
        .map(|m| {
            let params: ShowMessageParams =
                serde_json::from_value(m.params().cloned().expect("params"))
                    .expect("showMessage params decode");
            params.message
        })
        .collect()
}

/// A control producer that cannot await joins the line in order and is
/// accounted with its exact bytes, even past the admitted budget, until the
/// writer has written it.
#[tokio::test(flavor = "current_thread")]
async fn detached_control_messages_are_ordered_and_accounted_until_written() {
    let outbound = Outbound::new(OutboundBudget {
        control: ClassBudget {
            messages: 2,
            bytes: usize::MAX,
        },
        ..OutboundBudget::DEFAULT
    });
    outbound.mark_initialized();
    let sent: Vec<ShowMessageParams> = (0..5).map(warning).collect();
    for message in &sent {
        outbound.notify_detached::<ShowMessage>(message.clone());
    }
    let control = outbound.load().control;
    assert_eq!(
        control.admitted,
        Load {
            messages: 2,
            bytes: sent[..2].iter().map(control_size).sum()
        },
        "the admitted set stops at the budget"
    );
    assert_eq!(
        control.waiting,
        Load {
            messages: 3,
            bytes: sent[2..].iter().map(control_size).sum()
        },
        "the rest wait, every byte counted"
    );

    let mut wire = outbound.wire();
    let expected: Vec<String> = sent.into_iter().map(|m| m.message).collect();
    assert_eq!(
        shown(&written_now(&mut wire)),
        expected,
        "control messages keep their order"
    );
    assert_eq!(outbound.load().control.retained(), Load::default());
    assert_eq!(outbound.high_water().control.admitted.messages, 2);
    assert_eq!(outbound.high_water().control.retained.messages, 5);
}

/// A producer that awaits its send waits for room, holding its place in the
/// line with its message accounted; cancelling it withdraws the message.
#[tokio::test(flavor = "current_thread")]
async fn an_awaiting_control_producer_waits_for_room_and_withdraws_on_cancel() {
    let outbound = Outbound::new(OutboundBudget {
        control: ClassBudget {
            messages: 1,
            bytes: usize::MAX,
        },
        ..OutboundBudget::DEFAULT
    });
    outbound.mark_initialized();
    outbound
        .send_notification::<ShowMessage>(warning(0))
        .now_or_never()
        .expect("the first message is admitted at once");

    let mut cancelled = Box::pin(outbound.send_notification::<ShowMessage>(warning(1)));
    assert!(futures_util::poll!(&mut cancelled).is_pending());
    let waiting = outbound.send_notification::<ShowMessage>(warning(2));
    tokio::pin!(waiting);
    assert!(futures_util::poll!(&mut waiting).is_pending());
    assert_eq!(
        outbound.load().control.waiting,
        Load {
            messages: 2,
            bytes: control_size(&warning(1)) + control_size(&warning(2))
        }
    );

    drop(cancelled);
    assert_eq!(outbound.load().control.waiting.messages, 1);

    let mut wire = outbound.wire();
    let first = bounded(wire.next()).await.expect("the wire stays open");
    assert_eq!(shown(&[first]), vec!["warning-0"]);
    bounded(&mut waiting).await;
    assert_eq!(
        shown(&written_now(&mut wire)),
        vec!["warning-2"],
        "the cancelled message never reaches the client"
    );
}

/// Server→client requests carry their own ids, are accounted like any control
/// message, and the client's reply reaches the producer that asked.
#[tokio::test(flavor = "current_thread")]
async fn a_server_request_is_answered_through_the_transport() {
    let outbound = Outbound::default();
    let refused = outbound
        .send_request::<WorkspaceConfiguration>(ConfigurationParams { items: Vec::new() })
        .await;
    assert!(
        refused.is_err(),
        "a request before initialization is refused, as by any LSP client connection"
    );

    let mut wire = outbound.wire();
    outbound.assume_initialized();
    let request =
        outbound.send_request::<WorkspaceConfiguration>(ConfigurationParams { items: Vec::new() });
    tokio::pin!(request);
    assert!(futures_util::poll!(&mut request).is_pending());
    let sent = bounded(wire.next()).await.expect("the wire stays open");
    assert_eq!(sent.method(), "workspace/configuration");
    let id = sent.id().cloned().expect("a request carries an id");
    wire.reply(Response::from_ok(id, serde_json::json!([{"a": 1}])));
    assert_eq!(
        bounded(request).await.expect("the reply is routed back"),
        vec![serde_json::json!({"a": 1})]
    );
    assert_eq!(outbound.load().control.retained(), Load::default());
}

/// Control is written ahead of a diagnostics backlog.
#[tokio::test(flavor = "current_thread")]
async fn control_is_written_ahead_of_diagnostics() {
    let outbound = Outbound::default();
    outbound.mark_initialized();
    let lane = outbound.diagnostics_lane();
    let mut publications = Vec::new();
    for i in 0..8 {
        let mut publication = Box::pin(lane.publish_diagnostics(
            1,
            params(&format!("file:///{i}.vue"), &["d"]),
            current,
        ));
        assert!(futures_util::poll!(&mut publication).is_pending());
        publications.push(publication);
    }
    outbound.notify_detached::<ShowMessage>(warning(0));

    let mut wire = outbound.wire();
    let written = written_now(&mut wire);
    assert_eq!(written[0].method(), ShowMessage::METHOD);
    assert_eq!(diagnostics(&written).len(), 8);
    for publication in publications {
        assert_eq!(bounded(publication).await, Delivery::Delivered);
    }
}

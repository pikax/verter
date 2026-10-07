//! Request pins: the document source and published root a foreground request
//! admits, and the provider surface its query maps through, are released when
//! the client cancels it, whether the request was just admitted or the
//! provider already held its query, and a cancelled request leaves nothing
//! behind that a later one waits on.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;

use super::super::test_support::RequestBarrier;
use super::movement::Handles;
use super::{reference_answer, Fixture, Outcome, Route};

/// Strong owners of each pinnable input: `(provider surface, document source,
/// published root)`.
type PinCounts = (usize, usize, usize);

fn pin_counts(handles: &Handles) -> PinCounts {
    let surface = handles.current_surface();
    let source = handles
        .server
        .documents
        .get(&handles.uri)
        .map(|document| Arc::clone(&document.source))
        .expect("the carrier is open");
    let root = handles
        .server
        .documents
        .host()
        .workspace_read()
        .published_root()
        .expect("the fixture published a root");
    // Each count includes the one this function holds.
    (
        Arc::strong_count(&surface) - 1,
        Arc::strong_count(&source) - 1,
        Arc::strong_count(&root) - 1,
    )
}

async fn assert_cancellation_releases_pins(route: Route, barrier: RequestBarrier) {
    let reference = reference_answer(route).await;
    let fixture = Fixture::new().await;
    let armed = route.arm(&fixture).await;
    let handles = Handles::of(&fixture);
    fixture.provider.clear_calls();
    fixture.barriers.clear();

    let before = pin_counts(&handles);
    let during: Arc<Mutex<Option<PinCounts>>> = Arc::default();
    let reached = Arc::new(AtomicBool::new(false));
    let arrived = Arc::new(tokio::sync::Notify::new());
    {
        let handles = handles.clone();
        let during = Arc::clone(&during);
        let reached = Arc::clone(&reached);
        let arrived = Arc::clone(&arrived);
        fixture.barriers.arm(
            barrier,
            Arc::new(move |_| {
                *during.lock() = Some(pin_counts(&handles));
                reached.store(true, Ordering::SeqCst);
                arrived.notify_one();
                // The client cancels while the request is suspended here.
                Box::pin(std::future::pending())
            }),
        );
    }

    {
        let ask = route.ask(&fixture, &armed);
        tokio::pin!(ask);
        tokio::select! {
            outcome = &mut ask => panic!("{route:?}: the request must suspend at {barrier:?}, got {outcome:?}"),
            () = arrived.notified() => {}
        }
    }
    assert!(reached.load(Ordering::SeqCst));
    let during = during.lock().expect("the barrier measured the pins");
    // The admission pins the document source and the published root; a query
    // the provider holds also pins the surface it maps through.
    let surface_pinned = barrier != RequestBarrier::ProviderDispatch || during.0 > before.0;
    assert!(
        during.1 > before.1 && during.2 > before.2 && surface_pinned,
        "{route:?} at {barrier:?}: the suspended request pins its inputs \
         (before {before:?}, during {during:?})"
    );
    assert_eq!(
        pin_counts(&handles),
        before,
        "{route:?} at {barrier:?}: cancellation releases every pin the request held"
    );

    // Nothing the cancelled request held is waited on by the next one.
    fixture.barriers.clear();
    fixture.provider.clear_calls();
    assert_eq!(
        route.ask(&fixture, &armed).await,
        Outcome::Answered(reference)
    );
    assert_eq!(fixture.dispatches(route), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn cancellation_before_provider_dispatch_releases_request_pins() {
    assert_cancellation_releases_pins(Route::Hover, RequestBarrier::Capture).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn cancellation_after_the_provider_holds_the_query_releases_request_pins() {
    assert_cancellation_releases_pins(Route::Rename, RequestBarrier::ProviderDispatch).await;
}

/// A request whose current-file repair has handed its provider write to the
/// engine is cancelled before the engine acknowledges the write. The request's
/// pins are released at once; the write is not abandoned with the request but
/// settles when the engine acknowledges it, so the next repair finds it applied
/// instead of writing again; and a later request neither waits on the cancelled
/// one nor answers from an unknown provider state.
#[tokio::test(flavor = "multi_thread")]
async fn cancellation_after_an_engine_write_releases_pins_and_the_write_still_settles() {
    use crate::type_provider::mock::MockCall;
    use crate::type_provider::traits::TypeProvider;
    use verter_type_runtime::traits::AppliedContent;

    let route = Route::Hover;
    let reference = reference_answer(route).await;
    let fixture = Fixture::new().await;
    let armed = route.arm(&fixture).await;
    let handles = Handles::of(&fixture);
    let server = fixture.server();
    let ide_path = server
        .active_ide_path_for_uri(&fixture.uri)
        .expect("the carrier has a provider path");
    let writes_of = |content: &str| {
        fixture
            .provider
            .calls()
            .iter()
            .filter(|call| {
                matches!(call, MockCall::UpdateFile { path, content: written }
                    if *path == ide_path && written.contains(content))
            })
            .count()
    };

    // A client edit the provider has not received: the next request's repair
    // writes it.
    handles.edit(2, &super::movement::edited_app());
    fixture.provider.clear_calls();
    fixture.barriers.clear();
    let before = pin_counts(&handles);
    let (write_reached_engine, acknowledge_write) = fixture.provider.block_update_file(&ide_path);

    let during = {
        let ask = route.ask(&fixture, &armed);
        tokio::pin!(ask);
        tokio::select! {
            outcome = &mut ask => panic!("the request must suspend on its engine write, got {outcome:?}"),
            () = write_reached_engine.notified() => {}
        }
        pin_counts(&handles)
    };
    assert!(
        during.1 > before.1 && during.2 > before.2,
        "the request suspended on its engine write pins its admitted inputs \
         (before {before:?}, during {during:?})"
    );
    // The in-flight write holds its own capture until it settles; the request's
    // admission pin is released with the request.
    let cancelled = pin_counts(&handles);
    assert!(
        cancelled.2 < during.2,
        "cancellation releases the request's published-root pin while the write is in flight \
         (during {during:?}, after cancellation {cancelled:?})"
    );

    // The engine acknowledges the write the cancelled request issued; its
    // settlement completes without the request, so the next repair finds the
    // edit applied and writes nothing.
    acknowledge_write.notify_one();
    server.ensure_current_file_synced(&fixture.uri).await;
    assert!(
        matches!(
            fixture.provider.applied_content(&ide_path),
            AppliedContent::Applied(applied) if applied.contains("hello there")
        ),
        "the cancelled request's write settled as applied"
    );
    assert_eq!(
        writes_of("hello there"),
        1,
        "the cancelled request's write was settled, not abandoned and written again"
    );
    // The settled write records a surface over the edited source, which the
    // store now retains; the published root, which only requests and their
    // in-flight writes hold, is back to its unpinned count.
    assert_eq!(
        pin_counts(&handles).2,
        before.2,
        "nothing the cancelled request or its write held outlives the write's settlement"
    );

    // A later request neither waits on the cancelled one nor answers from an
    // unknown provider state: restoring the original text answers the unmoved
    // reference through one dispatch.
    handles.edit(3, super::APP);
    fixture.provider.clear_calls();
    assert_eq!(
        route.ask(&fixture, &armed).await,
        Outcome::Answered(reference)
    );
    assert_eq!(fixture.dispatches(route), 1);
}

/// The current-file repair a request runs before its provider query carries
/// the request's own deadline: every provider write the repair issues is bounded
/// by the instant the request is bounded by, so a write queued behind a stalled
/// engine expires with the request instead of applying after the client gave
/// up, even though the repair runs on its own task.
#[tokio::test(flavor = "multi_thread")]
async fn a_request_repair_writes_under_the_request_deadline() {
    let budget = std::time::Duration::from_secs(60);
    let mut host_config = verter_session::HostConfig::default();
    host_config.lsp_method_timeouts.request_deadlines.hover = budget;
    let route = Route::Hover;
    let fixture = Fixture::with_host_config(host_config).await;
    let armed = route.arm(&fixture).await;
    let handles = Handles::of(&fixture);
    let ide_path = fixture
        .server()
        .active_ide_path_for_uri(&fixture.uri)
        .expect("the carrier has a provider path");

    // A client edit the provider has not received: the request's repair
    // writes it.
    handles.edit(2, &super::movement::edited_app());
    let earlier_writes = fixture.provider.write_deadlines(&ide_path).len();
    let asked_at = tokio::time::Instant::now();
    let _ = route.ask(&fixture, &armed).await;
    let answered_at = tokio::time::Instant::now();

    let repair_writes = fixture.provider.write_deadlines(&ide_path)[earlier_writes..].to_vec();
    assert!(
        !repair_writes.is_empty(),
        "the request repaired the edited carrier before its query"
    );
    for deadline in repair_writes {
        let at = deadline.expect("a request repair write carries the request deadline");
        assert!(
            at >= asked_at + budget && at <= answered_at + budget,
            "the repair write is bounded by the deadline the request opened with its \
             budget, not another bound ({:?} after the request was asked; budget {budget:?})",
            at.saturating_duration_since(asked_at)
        );
    }
}

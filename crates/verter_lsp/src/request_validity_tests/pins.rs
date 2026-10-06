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

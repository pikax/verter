//! Liveness rows: with the document, its project authority, the provider's
//! health and every semantic input fixed, background work that moves only
//! derived state must not cost a request its answer. Every barrier of every
//! attempt moves that state, continuously; the request must still answer the
//! unmoved fixture's NONEMPTY provider answer, with no `ContentModified` and
//! exactly one provider dispatch.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use super::movement::{every, Handles, ALL_BARRIERS};
use super::{reference_answer, Fixture, Outcome, Route};

/// Background work that changes nothing a request answers from.
#[derive(Clone, Copy, Debug)]
pub(super) enum Movement {
    /// The diagnostics generation advances: importer re-arm, identical
    /// background resync, identical recompile, eviction with identical reload.
    DiagnosticsRepublication,
    /// The IDE surface is recorded again with identical bytes and map.
    IdenticalSurfaceRecord,
    /// The workspace root is published again over the unchanged snapshot.
    EquivalentRootPublication,
}

macro_rules! liveness_rows {
    ($route:expr; $($(#[$attr:meta])* $row:ident: $movement:ident),+ $(,)?) => {
        mod liveness {
            use super::super::liveness::{assert_live, Movement};
            use super::Route;

            $(
                #[tokio::test(flavor = "multi_thread")]
                $(#[$attr])*
                async fn $row() {
                    assert_live($route, Movement::$movement).await;
                }
            )+
        }
    };
}
pub(super) use liveness_rows;

impl Movement {
    async fn act(self, handles: &Handles, index: usize) {
        match self {
            Movement::DiagnosticsRepublication => handles.republish_diagnostics(index).await,
            Movement::IdenticalSurfaceRecord => handles.record_current_surface(None),
            Movement::EquivalentRootPublication => handles.republish_equivalent_root(),
        }
    }
}

pub(super) async fn assert_live(route: Route, movement: Movement) {
    let reference = reference_answer(route).await;
    let fixture = Fixture::new().await;
    let armed = route.arm(&fixture).await;
    let handles = Handles::of(&fixture);
    fixture.provider.clear_calls();
    fixture.barriers.clear();
    // One count across every barrier, so consecutive moves rotate through the
    // producers rather than each barrier repeating the same one.
    let moves = Arc::new(AtomicUsize::new(0));
    for barrier in ALL_BARRIERS {
        let moves = Arc::clone(&moves);
        fixture.barriers.arm(
            barrier,
            every(handles.clone(), move |handles, _| {
                let index = moves.fetch_add(1, Ordering::SeqCst);
                Box::pin(async move { movement.act(&handles, index).await })
            }),
        );
    }

    let outcome = route.ask(&fixture, &armed).await;
    let dispatches = fixture.dispatches(route);

    assert_ne!(
        outcome,
        Outcome::ContentModified,
        "{route:?}/{movement:?}: background movement that changes no input must not \
         answer ContentModified (provider dispatches: {dispatches})"
    );
    assert_eq!(
        outcome,
        Outcome::Answered(reference),
        "{route:?}/{movement:?}: the request must answer the unmoved provider answer \
         (provider dispatches: {dispatches})"
    );
    assert_eq!(
        dispatches, 1,
        "{route:?}/{movement:?}: exactly one provider dispatch per request"
    );
}

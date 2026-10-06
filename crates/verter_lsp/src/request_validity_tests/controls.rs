//! Adversarial controls: a request whose inputs genuinely change while it is
//! in flight ends in its defined non-success outcome, or in an answer equal to
//! the one the unmoved fixture gives — never in a range mapped through another
//! revision.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;

use super::super::test_support::RequestBarrier;
use super::movement::{edited_app, shifted_app, Handles};
use super::{reference_answer, Fixture, Outcome, Route, APP, BARRIERS};

/// One way a request's inputs change underneath it.
#[derive(Clone, Copy, Debug)]
pub(super) enum Control {
    /// The document text changes without its version moving.
    SameVersionContentReplacement,
    /// The document closes and reopens with the bytes it had.
    CloseReopenIdenticalBytes,
    /// A fresh workspace replaces the published one; its snapshot repeats the
    /// replaced snapshot's scalar generation.
    WorkspaceReplacementRepeatedGeneration,
    /// The workspace loses the configured project that owned the document.
    ProjectOwnerLoss,
    /// The provider engine restarts under the query: the in-flight delivery is
    /// lost, and the replacement engine serves the bytes the old one held.
    ProviderRestart,
    /// The IDE surface is recorded again with only its map identity changed.
    MapOnlyChange,
    /// Authored text is inserted before every block, moving every carrier
    /// offset while the script and template bodies stay the same.
    SourceOnlyOffsetShift,
    /// The provider's answer is held while the document is edited and the edit
    /// is reverted to the original bytes.
    DelayedProviderDelivery,
    /// Every provider delivery fails.
    FailedProviderDelivery,
    /// The IDE surface moves to different bytes and back to the original
    /// bytes during one request.
    ProviderSurfaceAbaDuringRequest,
}

macro_rules! control_rows {
    ($route:expr) => {
        mod controls {
            use super::super::controls::{assert_control, Control};
            use super::Route;

            #[tokio::test(flavor = "multi_thread")]
            async fn same_version_content_replacement_never_maps_the_old_revision() {
                assert_control($route, Control::SameVersionContentReplacement).await;
            }

            #[tokio::test(flavor = "multi_thread")]
            async fn close_reopen_with_identical_bytes_fails_closed() {
                assert_control($route, Control::CloseReopenIdenticalBytes).await;
            }

            #[tokio::test(flavor = "multi_thread")]
            async fn workspace_replacement_with_a_repeated_generation_fails_closed() {
                assert_control($route, Control::WorkspaceReplacementRepeatedGeneration).await;
            }

            #[tokio::test(flavor = "multi_thread")]
            async fn project_owner_loss_fails_closed() {
                assert_control($route, Control::ProjectOwnerLoss).await;
            }

            #[tokio::test(flavor = "multi_thread")]
            async fn provider_restart_answers_only_from_redelivered_bytes() {
                assert_control($route, Control::ProviderRestart).await;
            }

            #[tokio::test(flavor = "multi_thread")]
            async fn map_only_change_never_maps_through_the_superseded_map() {
                assert_control($route, Control::MapOnlyChange).await;
            }

            #[tokio::test(flavor = "multi_thread")]
            async fn source_only_offset_shift_never_maps_the_old_offsets() {
                assert_control($route, Control::SourceOnlyOffsetShift).await;
            }

            #[tokio::test(flavor = "multi_thread")]
            async fn delayed_provider_delivery_across_an_edit_fails_closed() {
                assert_control($route, Control::DelayedProviderDelivery).await;
            }

            #[tokio::test(flavor = "multi_thread")]
            async fn failed_provider_delivery_answers_without_the_provider() {
                assert_control($route, Control::FailedProviderDelivery).await;
            }

            #[tokio::test(flavor = "multi_thread")]
            async fn provider_surface_a_b_a_never_maps_the_intermediate_surface() {
                assert_control($route, Control::ProviderSurfaceAbaDuringRequest).await;
            }
        }
    };
}
pub(super) use control_rows;

/// For each provider dispatch of one request, in order: whether the answer it
/// returned was produced after the control moved state.
type DispatchLedger = Arc<Mutex<Vec<bool>>>;

impl Control {
    /// The barriers this control moves state at. A provider-side event can
    /// only happen while the provider holds the query.
    fn barriers(self) -> &'static [RequestBarrier] {
        match self {
            Control::ProviderRestart | Control::FailedProviderDelivery => {
                &[RequestBarrier::ProviderDispatch]
            }
            _ => &BARRIERS,
        }
    }

    /// Move the state this control changes, once.
    fn act(self, handles: &Handles) {
        match self {
            Control::SameVersionContentReplacement => handles.edit(1, &edited_app()),
            Control::CloseReopenIdenticalBytes => handles.close_and_reopen(APP),
            Control::WorkspaceReplacementRepeatedGeneration => handles.replace_workspace(true),
            Control::ProjectOwnerLoss => handles.replace_workspace(false),
            Control::ProviderRestart => handles.provider.fail_next_deliveries(1),
            Control::MapOnlyChange => {
                let mut map_hash = handles.current_surface().stamp.map_hash;
                map_hash[0] ^= 0xff;
                handles.record_current_surface(Some(map_hash));
            }
            Control::SourceOnlyOffsetShift => handles.edit(2, &shifted_app()),
            Control::DelayedProviderDelivery => {
                handles.edit(2, &edited_app());
                handles.edit(3, APP);
            }
            Control::FailedProviderDelivery => handles.provider.fail_next_deliveries(usize::MAX),
            Control::ProviderSurfaceAbaDuringRequest => {
                handles.record_drifted_surface();
                handles.record_current_surface(None);
            }
        }
    }

    /// Whether `outcome` is this control's defined result for `route` when the
    /// control moved state at `barrier`. `reference` is the unmoved fixture's
    /// answer; `fresh` says whether the dispatch whose answer the reply carries
    /// was produced after the move.
    fn admits(
        self,
        route: Route,
        barrier: RequestBarrier,
        outcome: &Outcome,
        reference: &str,
        fresh: bool,
    ) -> bool {
        let unmoved = |answer: &String| answer == reference;
        match (self, outcome) {
            // Nothing a client sent moved, so no route may claim it did, and a
            // provider that never answered contributes nothing.
            (Control::FailedProviderDelivery, Outcome::Empty | Outcome::Refused(_)) => true,
            (Control::FailedProviderDelivery, _) => false,
            // A route with bounded recovery re-asks the replacement engine and
            // answers what the unmoved fixture answers; every other route
            // answers without the provider. Neither may claim a content change.
            (Control::ProviderRestart, Outcome::Answered(answer)) => {
                route.recovers_a_lost_delivery() && fresh && unmoved(answer)
            }
            (Control::ProviderRestart, Outcome::Empty) => !route.recovers_a_lost_delivery(),
            (Control::ProviderRestart, _) => false,
            // The surface ends byte- and map-identical to where it began, but it
            // was a different surface in between: only a request that captured
            // its surface after the round trip — moved at admission, before any
            // surface is captured — may answer. Moved while the provider held
            // the query, after the decode or at settlement, the bracket the
            // request keeps until settlement refuses the answer.
            (Control::ProviderSurfaceAbaDuringRequest, Outcome::Answered(answer)) => {
                unmoved(answer) && barrier == RequestBarrier::Capture
            }
            // The map changed: an answer decoded through the map captured before
            // the change may not survive, whether the change landed while the
            // provider held the query, after the decode or at settlement. Only an
            // answer re-asked after the change, or captured after a change at
            // admission, went through the map that is current.
            (Control::MapOnlyChange, Outcome::Answered(answer)) => {
                unmoved(answer) && (fresh || barrier == RequestBarrier::Capture)
            }
            // The document or its workspace changed: only an answer computed
            // entirely after the change describes the current revision, and when
            // the change restores the original bytes it is the unmoved answer.
            (
                Control::CloseReopenIdenticalBytes
                | Control::WorkspaceReplacementRepeatedGeneration
                | Control::ProjectOwnerLoss
                | Control::DelayedProviderDelivery,
                Outcome::Answered(answer),
            ) => fresh && unmoved(answer),
            (
                Control::SameVersionContentReplacement | Control::SourceOnlyOffsetShift,
                Outcome::Answered(_),
            ) => fresh,
            (_, Outcome::Empty | Outcome::ContentModified | Outcome::Refused(_)) => true,
        }
    }
}

/// Run `control` at each of its barriers against a fresh fixture and require
/// its defined outcome every time.
pub(super) async fn assert_control(route: Route, control: Control) {
    let reference = reference_answer(route).await;
    for &barrier in control.barriers() {
        let fixture = Fixture::new().await;
        let armed = route.arm(&fixture).await;
        let handles = Handles::of(&fixture);
        let moved = Arc::new(AtomicBool::new(false));
        let ledger: DispatchLedger = Arc::default();
        fixture.provider.clear_calls();
        fixture.barriers.clear();
        // The provider produces its answer when it is queried, before the
        // dispatch barrier runs, so a dispatch is fresh exactly when the move
        // preceded its barrier arrival.
        let record_dispatch = {
            let ledger = Arc::clone(&ledger);
            let moved = Arc::clone(&moved);
            move || ledger.lock().push(moved.load(Ordering::SeqCst))
        };
        let act = {
            let moved = Arc::clone(&moved);
            move || {
                control.act(&handles);
                moved.store(true, Ordering::SeqCst);
            }
        };
        if barrier == RequestBarrier::ProviderDispatch {
            fixture.barriers.arm(
                barrier,
                Arc::new(move |arrival| {
                    record_dispatch();
                    if arrival == 0 {
                        act();
                    }
                    Box::pin(async {})
                }),
            );
        } else {
            fixture.barriers.arm(
                barrier,
                Arc::new(move |arrival| {
                    if arrival == 0 {
                        act();
                    }
                    Box::pin(async {})
                }),
            );
            fixture.barriers.arm(
                RequestBarrier::ProviderDispatch,
                Arc::new(move |_| {
                    record_dispatch();
                    Box::pin(async {})
                }),
            );
        }
        let outcome = route.ask(&fixture, &armed).await;
        assert!(
            moved.load(Ordering::SeqCst),
            "{route:?}/{control:?}: the request never reached {barrier:?}, so the control did not run"
        );
        let ledger = ledger.lock().clone();
        let fresh = ledger.last().copied().unwrap_or(false);
        assert!(
            control.admits(route, barrier, &outcome, &reference, fresh),
            "{route:?}/{control:?} at {barrier:?}: {outcome:?} is not the defined outcome \
             (unmoved answer {reference:?}; per dispatch, answered after the move: {ledger:?})"
        );
    }
}

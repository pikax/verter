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
use crate::provider_surface_store::SurfaceDelivery;

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
    /// The serving engine incarnation is retired and replaced while the query
    /// is outstanding — while the engine holds it, or after its answer settled
    /// and before the decode: an answer the retired engine selected never
    /// settles, and the replacement holds none of the bytes the retired one
    /// acknowledged until they are delivered to it again.
    ProviderRestart,
    /// The IDE surface is recorded again with only its map identity changed.
    MapOnlyChange,
    /// Authored text is inserted before every block, moving every carrier
    /// offset while the script and template bodies stay the same.
    SourceOnlyOffsetShift,
    /// The document is edited, the production re-sync's file write of the
    /// edited carrier reaches the engine and is held there unacknowledged, and
    /// the edit is reverted. The write is released only after the request has
    /// settled, so the engine holds the original bytes throughout it.
    DelayedProviderDelivery,
    /// The engine loses the requested surface's delivery, and every later file
    /// delivery and provider answer fails.
    FailedProviderDelivery,
    /// The IDE surface moves to different bytes and back to the original
    /// bytes during one request.
    ProviderSurfaceAbaDuringRequest,
    /// A surface with different bytes is recorded and its publication
    /// committed before any engine received them: the record runs ahead of its
    /// delivery.
    RecordBeforeDelivery,
    /// The engine receives different bytes before any surface describing them
    /// is recorded: the delivery runs ahead of its record.
    DeliverBeforeRecord,
    /// The engine restarts and holds none of the recorded surface: the
    /// provider lags behind the store.
    LaggingProvider,
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

            #[tokio::test(flavor = "multi_thread")]
            async fn a_record_ahead_of_its_delivery_never_decodes_an_answer() {
                assert_control($route, Control::RecordBeforeDelivery).await;
            }

            #[tokio::test(flavor = "multi_thread")]
            async fn a_delivery_ahead_of_its_record_never_decodes_an_answer() {
                assert_control($route, Control::DeliverBeforeRecord).await;
            }

            #[tokio::test(flavor = "multi_thread")]
            async fn a_lagging_provider_is_repaired_or_unavailable_never_mismapped() {
                assert_control($route, Control::LaggingProvider).await;
            }
        }
    };
}
pub(super) use control_rows;

/// For each provider dispatch of one request, in order: whether the answer it
/// returned was produced after the control moved state.
type DispatchLedger = Arc<Mutex<Vec<bool>>>;

/// What a control's move left behind for the checks after the request.
#[derive(Default)]
struct Moved {
    /// The carrier surface's delivery state observed right after the move.
    delivery: Option<SurfaceDelivery>,
    /// A file write held at the engine: its release and the task awaiting it.
    held_write: Option<(Arc<tokio::sync::Notify>, tokio::task::JoinHandle<()>)>,
    /// Releases a request that reached the document's repair lane.
    lane_release: Option<Arc<tokio::sync::Notify>>,
    /// Signalled when the request reaches the repair lane while the write is
    /// held: it joins the pending delivery.
    joined: Arc<tokio::sync::Notify>,
}

impl Moved {
    /// Take everything this move holds, to release it.
    fn release(&mut self) -> Held {
        Held {
            write: self.held_write.take(),
            lane: self.lane_release.take(),
        }
    }
}

/// A held write and repair lane, released together.
struct Held {
    write: Option<(Arc<tokio::sync::Notify>, tokio::task::JoinHandle<()>)>,
    lane: Option<Arc<tokio::sync::Notify>>,
}

impl Held {
    /// Release the held write and wait for it to settle, then let a request
    /// waiting at the repair lane proceed.
    async fn settle(self) {
        if let Some((release, delivering)) = self.write {
            release.notify_one();
            tokio::time::timeout(std::time::Duration::from_secs(30), delivering)
                .await
                .expect("the released write settles")
                .expect("the delivering task completes");
        }
        if let Some(lane) = self.lane {
            lane.notify_one();
        }
    }
}

impl Control {
    /// The barriers this control moves state at. A provider-side event can
    /// only happen while the provider holds the query.
    fn barriers(self) -> &'static [RequestBarrier] {
        match self {
            Control::ProviderRestart => &[
                RequestBarrier::ProviderDispatch,
                RequestBarrier::ProviderDecode,
            ],
            Control::FailedProviderDelivery => &[RequestBarrier::ProviderDispatch],
            _ => &BARRIERS,
        }
    }

    /// Move the state this control changes, once, and note what the move
    /// left behind in `moved`.
    async fn act(self, handles: &Handles, moved: &Mutex<Moved>) {
        match self {
            Control::SameVersionContentReplacement => handles.edit(1, &edited_app()),
            Control::CloseReopenIdenticalBytes => handles.close_and_reopen(APP),
            Control::WorkspaceReplacementRepeatedGeneration => handles.replace_workspace(true),
            Control::ProjectOwnerLoss => handles.replace_workspace(false),
            Control::ProviderRestart => handles.provider.replace_engine(),
            Control::MapOnlyChange => {
                let mut map_hash = handles.current_surface().stamp.map_hash;
                map_hash[0] ^= 0xff;
                handles.record_current_surface(Some(map_hash));
            }
            Control::SourceOnlyOffsetShift => handles.edit(2, &shifted_app()),
            Control::DelayedProviderDelivery => {
                let ide_path = handles.current_surface().stamp.provider_path.to_string();
                let (arrived, release) = handles.provider.block_update_file(&ide_path);
                handles.edit(2, &edited_app());
                let delivering = tokio::spawn({
                    let handles = handles.clone();
                    async move { handles.resync_carrier().await }
                });
                tokio::time::timeout(std::time::Duration::from_secs(30), arrived.notified())
                    .await
                    .expect("the edited carrier's file write never reached the engine");
                handles.edit(3, APP);
                let (at_lane, lane_release) = handles
                    .server
                    .pause_next_ide_sync_before_lease(&handles.canonical);
                let joined = Arc::clone(&moved.lock().joined);
                tokio::spawn(async move {
                    at_lane.notified().await;
                    joined.notify_one();
                });
                let mut moved = moved.lock();
                moved.held_write = Some((release, delivering));
                moved.lane_release = Some(lane_release);
            }
            Control::FailedProviderDelivery => {
                handles.provider.set_fail_file_ops(true);
                handles
                    .provider
                    .lose_delivery(&handles.current_surface().stamp.provider_path);
                handles.provider.fail_next_deliveries(usize::MAX);
            }
            Control::ProviderSurfaceAbaDuringRequest => {
                handles.record_drifted_surface();
                handles.record_current_surface(None);
            }
            Control::RecordBeforeDelivery => handles.record_and_commit_undelivered_drift(),
            Control::DeliverBeforeRecord => handles.deliver_unrecorded_drift(),
            Control::LaggingProvider => handles.provider.forget_applied_content(),
        }
        moved.lock().delivery = Some(handles.surface_delivery());
    }

    /// The delivery states the carrier's current IDE surface may be in right
    /// after the move, while the request is still outstanding.
    fn moved_delivery(self) -> Option<&'static [SurfaceDelivery]> {
        match self {
            // The retired incarnation's acknowledgement proves nothing about
            // the replacement.
            Control::ProviderRestart => Some(&[SurfaceDelivery::DeliveryLost]),
            // The unacknowledged write proves nothing: the recorded surface is
            // still the one the engine holds.
            Control::DelayedProviderDelivery => Some(&[SurfaceDelivery::Delivered]),
            // The engine dropped the bytes it acknowledged.
            Control::FailedProviderDelivery => Some(&[SurfaceDelivery::DeliveryLost]),
            _ => None,
        }
    }

    /// The delivery states the carrier's current IDE surface may end the
    /// request in — the typed delivery outcome of each control that moves what
    /// the engine holds.
    fn final_delivery(self) -> Option<&'static [SurfaceDelivery]> {
        match self {
            // Released after the request settled, the stale edited write lands
            // on the engine after the edit was reverted: the engine diverges
            // from the recorded surface, unless the re-sync recorded it.
            Control::DelayedProviderDelivery => {
                Some(&[SurfaceDelivery::EngineDiverged, SurfaceDelivery::Delivered])
            }
            // No delivery can succeed again: the lost surface stays lost, or a
            // surface recorded again for a repair waits on a delivery that
            // keeps failing.
            Control::FailedProviderDelivery => Some(&[
                SurfaceDelivery::DeliveryLost,
                SurfaceDelivery::AwaitingDelivery,
            ]),
            // Re-delivered to the replacement incarnation, or reported lost.
            Control::ProviderRestart => {
                Some(&[SurfaceDelivery::Delivered, SurfaceDelivery::DeliveryLost])
            }
            // Repaired before dispatch, or still awaiting the delivery the
            // record ran ahead of.
            Control::RecordBeforeDelivery => Some(&[
                SurfaceDelivery::Delivered,
                SurfaceDelivery::AwaitingDelivery,
            ]),
            // Repaired before dispatch, or the engine still holds the bytes
            // that were never recorded.
            Control::DeliverBeforeRecord => {
                Some(&[SurfaceDelivery::Delivered, SurfaceDelivery::EngineDiverged])
            }
            // Repaired before dispatch, or the recorded surface is reported
            // lost.
            Control::LaggingProvider => {
                Some(&[SurfaceDelivery::Delivered, SurfaceDelivery::DeliveryLost])
            }
            Control::SameVersionContentReplacement
            | Control::CloseReopenIdenticalBytes
            | Control::WorkspaceReplacementRepeatedGeneration
            | Control::ProjectOwnerLoss
            | Control::MapOnlyChange
            | Control::SourceOnlyOffsetShift
            | Control::ProviderSurfaceAbaDuringRequest => None,
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
            // A recovering route that answers without the provider must have
            // met the replacement without the surface (checked against the
            // surface's final delivery state by the caller).
            (Control::ProviderRestart, Outcome::Answered(answer)) => {
                route.recovers_a_lost_delivery() && fresh && unmoved(answer)
            }
            (Control::ProviderRestart, Outcome::Empty) => true,
            // Retired after the answer settled at the provider, the surface the
            // answer would decode through is lost: the route's settlement
            // bracket refuses it under the route's own supersession contract.
            (Control::ProviderRestart, Outcome::ContentModified | Outcome::Refused(_)) => {
                barrier != RequestBarrier::ProviderDispatch
            }
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
            // The surface and the engine disagreed: only an answer decoded
            // through bytes the engine holds — after the requested surface was
            // repaired before dispatch — may answer, and it is the unmoved one.
            (
                Control::RecordBeforeDelivery
                | Control::DeliverBeforeRecord
                | Control::LaggingProvider,
                Outcome::Answered(answer),
            ) => unmoved(answer),
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
        let left_behind = Arc::new(Mutex::new(Moved::default()));
        let ledger: DispatchLedger = Arc::default();
        fixture.provider.clear_calls();
        fixture.barriers.clear();
        // The provider selects its answer when it is queried, before the
        // dispatch barrier runs, so a dispatch is fresh exactly when the move
        // preceded its barrier arrival.
        let ide_path = handles.current_surface().stamp.provider_path.to_string();
        let record_dispatch = {
            let ledger = Arc::clone(&ledger);
            let moved = Arc::clone(&moved);
            move |fresh: Option<bool>| {
                let fresh = fresh.unwrap_or_else(|| moved.load(Ordering::SeqCst));
                ledger.lock().push(fresh);
            }
        };
        let moved_at_dispatch = Arc::clone(&moved);
        let act = {
            let moved = Arc::clone(&moved);
            let left_behind = Arc::clone(&left_behind);
            let handles = handles.clone();
            move || -> futures_util::future::BoxFuture<'static, ()> {
                let moved = Arc::clone(&moved);
                let left_behind = Arc::clone(&left_behind);
                let handles = handles.clone();
                Box::pin(async move {
                    control.act(&handles, &left_behind).await;
                    moved.store(true, Ordering::SeqCst);
                })
            }
        };
        if barrier == RequestBarrier::ProviderDispatch {
            fixture.barriers.arm(
                barrier,
                Arc::new(move |arrival| {
                    // The answer this dispatch carries was selected before the
                    // move.
                    record_dispatch(Some(moved_at_dispatch.load(Ordering::SeqCst)));
                    if arrival == 0 {
                        act()
                    } else {
                        Box::pin(async {})
                    }
                }),
            );
        } else {
            fixture.barriers.arm(
                barrier,
                Arc::new(move |arrival| {
                    if arrival == 0 {
                        act()
                    } else {
                        Box::pin(async {})
                    }
                }),
            );
            fixture.barriers.arm(
                RequestBarrier::ProviderDispatch,
                Arc::new(move |_| {
                    record_dispatch(None);
                    Box::pin(async {})
                }),
            );
        }
        // A request that needs the requested file's pending delivery joins it
        // at the document's repair lane; the held write is released once it
        // has, so the joined request completes. A request that does not join
        // settles while the write is still held.
        let outcome = {
            let ask = route.ask(&fixture, &armed);
            tokio::pin!(ask);
            let joined = Arc::clone(&left_behind.lock().joined);
            tokio::select! {
                outcome = &mut ask => outcome,
                () = joined.notified() => {
                    let held = left_behind.lock().release();
                    held.settle().await;
                    ask.await
                }
            }
        };
        assert!(
            moved.load(Ordering::SeqCst),
            "{route:?}/{control:?}: the request never reached {barrier:?}, so the control did not run"
        );
        let (moved_delivery, held) = {
            let mut left_behind = left_behind.lock();
            (left_behind.delivery, left_behind.release())
        };
        if let Some(states) = control.moved_delivery() {
            assert!(
                moved_delivery.is_some_and(|delivery| states.contains(&delivery)),
                "{route:?}/{control:?} at {barrier:?}: right after the move the surface was \
                 {moved_delivery:?}, expected one of {states:?}"
            );
        }
        held.settle().await;
        let ledger = ledger.lock().clone();
        let fresh = ledger.last().copied().unwrap_or(false);
        assert!(
            control.admits(route, barrier, &outcome, &reference, fresh),
            "{route:?}/{control:?} at {barrier:?}: {outcome:?} is not the defined outcome \
             (unmoved answer {reference:?}; per dispatch, answered after the move: {ledger:?})"
        );
        // Whatever the control, an answer is decoded only through the bytes the
        // engine evaluated it against — read at the instant the engine selected
        // the answer, not after: the surface the answer settled through is the
        // store's current one, since nothing moves after the control's one move.
        if matches!(outcome, Outcome::Answered(_)) {
            assert_eq!(
                fixture.provider.last_evaluating_incarnation(&ide_path),
                Some(fixture.provider.incarnation()),
                "{route:?}/{control:?} at {barrier:?}: the answer came from a retired engine"
            );
            let settled = handles.current_surface();
            let evaluated = fixture.provider.last_evaluated_bytes(&ide_path).flatten();
            assert!(
                evaluated.as_deref() == Some(&*settled.provider_content),
                "{route:?}/{control:?} at {barrier:?}: the answer was decoded through a surface \
                 the engine did not evaluate (engine held {} bytes, settled surface {} bytes)",
                evaluated.as_ref().map_or(0, |bytes| bytes.len()),
                settled.provider_content.len()
            );
        }
        if let Some(states) = control.final_delivery() {
            let delivery = handles.surface_delivery();
            assert!(
                states.contains(&delivery),
                "{route:?}/{control:?} at {barrier:?}: the surface ended {delivery:?}, \
                 expected one of {states:?}"
            );
            // An answer settles only through a delivered surface, and a route
            // that recovers a lost delivery answers without the provider only
            // when the replacement still lacks the surface.
            if matches!(outcome, Outcome::Answered(_)) {
                assert_eq!(
                    delivery,
                    SurfaceDelivery::Delivered,
                    "{route:?}/{control:?} at {barrier:?}: answered through an undelivered surface"
                );
            }
            if matches!(control, Control::ProviderRestart)
                && matches!(outcome, Outcome::Empty)
                && route.recovers_a_lost_delivery()
            {
                assert_eq!(
                    delivery,
                    SurfaceDelivery::DeliveryLost,
                    "{route:?} at {barrier:?}: the replacement engine holds the surface, so the \
                     recovering route must answer through it"
                );
            }
        }
    }
}

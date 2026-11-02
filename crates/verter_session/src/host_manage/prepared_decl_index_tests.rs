//! A cold IndexedReady build is a pollable computation: it suspends on its
//! owner shard's cold-index job instead of parking, another task claiming
//! the same file follows it, and both resume to the one artifact the build
//! publishes.

use std::future::Future;
use std::pin::{pin, Pin};
use std::sync::{mpsc, Arc};
use std::task::{Context, Poll, Wake, Waker};
use std::time::Duration;

use verter_execution::blocking::WaitProbe;
use verter_execution::tasks::{ExecutionScope, ExecutionTask};

use crate::types::{HostConfig, UpsertRequest};
use crate::VerterHost;

const PROPS: &str = "/src/props.ts";
const DEADLINE: Duration = Duration::from_secs(5);

fn host() -> VerterHost {
    let host = VerterHost::new_standalone(HostConfig::default());
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(PROPS.to_owned()),
            input_id: PROPS.to_owned(),
            source: Arc::from("export interface Base { label: string }\n"),
            file_language: crate::FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("the module registers");
    host
}

struct ChannelWake(parking_lot::Mutex<mpsc::Sender<()>>);

impl Wake for ChannelWake {
    fn wake(self: Arc<Self>) {
        let _ = self.0.lock().send(());
    }
}

fn channel_waker() -> (Waker, mpsc::Receiver<()>) {
    let (tx, rx) = mpsc::channel();
    (
        Waker::from(Arc::new(ChannelWake(parking_lot::Mutex::new(tx)))),
        rx,
    )
}

/// Poll `build` once as `task`, under a forbidding probe: a poll that would
/// park fails instead.
fn poll_as<F: Future>(
    host: &VerterHost,
    task: &ExecutionTask,
    build: Pin<&mut F>,
    waker: &Waker,
) -> Poll<F::Output> {
    let _task = ExecutionScope::install(&host.task_registry, task);
    let _forbid = WaitProbe::forbid();
    build.poll(&mut Context::from_waker(waker))
}

/// IndexedReady materializations so far, where capture keeps the count.
#[cfg(feature = "semantic-observe")]
fn materializes(host: &VerterHost) -> u64 {
    host.provenance().snapshot().indexed_ready_materializes
}

#[test]
fn indexed_load_waits_for_indexing_when_source_is_already_present() {
    use verter_type_engine::resolver_core::request_ports::{IndexedLoadOutcome, IndexedReadiness};

    let host = host();
    assert!(host.scheduler.try_get_source(PROPS).is_some());
    let mut held = host.decl_lowering.hold_workers_for_tests();
    let IndexedReadiness::Loading(mut load) = host.begin_indexed_ready_load(PROPS) else {
        panic!("source readiness must not report unfinished indexing as settled");
    };
    let (waker, wakes) = channel_waker();
    {
        let _forbid = WaitProbe::forbid();
        assert!(load.poll(&waker).is_pending());
        assert!(load.poll(&waker).is_pending());
    }
    held.release();
    wakes
        .recv_timeout(DEADLINE)
        .expect("index completion wakes its load");
    let outcome = {
        let _forbid = WaitProbe::forbid();
        load.poll(&waker)
    };
    assert_eq!(outcome, Poll::Ready(IndexedLoadOutcome::Integrated));
    assert_eq!(load.poll(&waker), outcome, "completion is terminal");
    let indexed = host
        .ensure_indexed_ready_serve(PROPS)
        .expect("the index is published");
    assert!(indexed.store_published);
    assert_eq!(host.task_registry.wait_count_for_tests(), 0);
}

#[test]
fn dropping_or_cancelling_an_indexed_load_releases_its_retained_index_flight() {
    use verter_type_engine::resolver_core::request_ports::{
        IndexedLoadEnd, IndexedLoadOutcome, IndexedReadiness,
    };

    for cancel in [false, true] {
        let host = host();
        let mut held = host.decl_lowering.hold_workers_for_tests();
        let IndexedReadiness::Loading(mut load) = host.begin_indexed_ready_load(PROPS) else {
            panic!("the index is cold");
        };
        let (waker, wakes) = channel_waker();
        {
            let _forbid = WaitProbe::forbid();
            assert!(load.poll(&waker).is_pending());
            if cancel {
                host.cooperative_drive.cancellation().cancel();
                wakes
                    .recv_timeout(DEADLINE)
                    .expect("cancellation wakes the load");
                let outcome =
                    Poll::Ready(IndexedLoadOutcome::NotIntegrated(IndexedLoadEnd::Cancelled));
                assert_eq!(load.poll(&waker), outcome);
                assert_eq!(load.poll(&waker), outcome);
            }
        }
        drop(load);
        assert_eq!(host.task_registry.wait_count_for_tests(), 0);
        held.release();
        assert!(
            host.ensure_indexed_ready_serve(PROPS).is_some(),
            "abandoning a load leaves indexing claimable"
        );
    }
}

#[test]
fn an_unpublished_index_transfers_its_valid_delivery_without_reserving_a_rebuild() {
    use verter_type_engine::resolver_core::request_ports::{IndexedLoadOutcome, IndexedReadiness};
    let host = host();
    host.test_force
        .force_indexed_ready_serve_fence_for_tests
        .store(true, std::sync::atomic::Ordering::Relaxed);
    let mut held = host.decl_lowering.hold_workers_for_tests();
    let IndexedReadiness::Loading(mut load) = host.begin_indexed_ready_load(PROPS) else {
        panic!("the index is cold");
    };
    let (waker, wakes) = channel_waker();
    {
        let _forbid = WaitProbe::forbid();
        assert!(load.poll(&waker).is_pending());
    }
    held.release();
    wakes
        .recv_timeout(DEADLINE)
        .expect("the fenced computation wakes its reader");
    let _forbid = WaitProbe::forbid();
    let expected = Poll::Ready(IndexedLoadOutcome::Integrated);
    assert_eq!(load.poll(&waker), expected);
    assert_eq!(load.poll(&waker), expected, "a fenced index is terminal");
    let serve = load
        .take_indexed_input()
        .expect("the valid fenced delivery is retained");
    assert!(!serve.store_published);
    assert!(
        load.take_indexed_input().is_none(),
        "delivery transfers once"
    );
}

/// With the owner shards held, a cold build's poll answers `Pending`
/// without entering a wait, and a second task's claim of the same file
/// follows it instead of building again. Releasing the shards wakes the
/// leader; both answer the one published artifact, and no wait edge is
/// left.
#[test]
fn a_cold_index_build_suspends_on_its_shard_and_a_second_task_follows_it() {
    let host = host();
    let observation = verter_type_engine::request_observers::CaptureTargets::installed::<
        verter_audit::observe::Uncaptured,
    >();
    #[cfg(feature = "semantic-observe")]
    let before = materializes(&host);
    let (leading, following) = (
        host.task_registry.register_task(),
        host.task_registry.register_task(),
    );
    let (leader_waker, leader_wakes) = channel_waker();
    let (follower_waker, follower_wakes) = channel_waker();
    let mut held = host.decl_lowering.hold_workers_for_tests();
    let mut leader = pin!(host
        .ensure_indexed_ready_serve_located_pollable::<verter_audit::observe::Uncaptured>(
            &observation,
            PROPS,
            None,
            None,
        ));
    let mut follower = pin!(host
        .ensure_indexed_ready_serve_located_pollable::<verter_audit::observe::Uncaptured>(
            &observation,
            PROPS,
            None,
            None,
        ));
    assert!(poll_as(&host, &leading, leader.as_mut(), &leader_waker).is_pending());
    assert!(poll_as(&host, &following, follower.as_mut(), &follower_waker).is_pending());
    assert_eq!(
        host.task_registry.wait_count_for_tests(),
        1,
        "the follower waits on the leading task"
    );
    held.release();
    leader_wakes
        .recv_timeout(DEADLINE)
        .expect("the shard's answer wakes the leader");
    let Poll::Ready(led) = poll_as(&host, &leading, leader.as_mut(), &leader_waker) else {
        panic!("the leader answers once its shard has");
    };
    follower_wakes
        .recv_timeout(DEADLINE)
        .expect("the leader's terminal wakes the follower");
    let Poll::Ready(followed) = poll_as(&host, &following, follower.as_mut(), &follower_waker)
    else {
        panic!("the follower answers the leader's artifact");
    };
    let (led, followed) = (
        led.expect("the module indexes").serve,
        followed.expect("the module indexes").serve,
    );
    assert!(
        Arc::ptr_eq(&led.indexed, &followed.indexed),
        "the follower answers the one artifact the leader built"
    );
    assert!(led.store_published && followed.store_published);
    #[cfg(feature = "semantic-observe")]
    assert_eq!(materializes(&host) - before, 1, "one build for both tasks");
    assert_eq!(host.task_registry.wait_count_for_tests(), 0);
}

/// Dropping a cold build suspended on its shard abandons its lane, so a
/// later claim builds; the abandoned job's lease goes back on the shard.
#[test]
fn dropping_a_suspended_index_build_lets_a_later_claim_build() {
    let host = host();
    let observation = verter_type_engine::request_observers::CaptureTargets::installed::<
        verter_audit::observe::Uncaptured,
    >();
    let task = host.task_registry.register_task();
    let (waker, _wakes) = channel_waker();
    let mut held = host.decl_lowering.hold_workers_for_tests();
    {
        let mut abandoned = Box::pin(
            host.ensure_indexed_ready_serve_located_pollable::<verter_audit::observe::Uncaptured>(
                &observation,
                PROPS,
                None,
                None,
            ),
        );
        assert!(poll_as(&host, &task, abandoned.as_mut(), &waker).is_pending());
    }
    held.release();
    let served = host
        .ensure_indexed_ready_serve(PROPS)
        .expect("a later claim builds the module");
    assert!(served.store_published);
    assert_eq!(host.task_registry.wait_count_for_tests(), 0);
}

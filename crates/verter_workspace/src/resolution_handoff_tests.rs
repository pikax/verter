use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::task::{Poll, Wake, Waker};

use crate::resolution_handoff::*;
use crate::{MemoryWorkspace, WorkspaceAccess, WorkspaceRead};
use verter_execution::cancellation::CancellationToken;
use verter_execution::tasks::TaskRegistry;
use verter_session_query::resolution::{ResolutionContext, ResolvePhase, ResolveRequestKind};

const CONTEXT: ResolutionContext = ResolutionContext {
    phase: ResolvePhase::CodegenBlocker,
    kind: ResolveRequestKind::TypeImport,
};
struct Count(AtomicUsize);
impl Wake for Count {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}
fn waker() -> (Arc<Count>, Waker) {
    let count = Arc::new(Count(AtomicUsize::new(0)));
    (count.clone(), Waker::from(count))
}
fn fixture() -> (Arc<MemoryWorkspace>, TaskRegistry) {
    let registry = TaskRegistry::default();
    let ws = Arc::new(MemoryWorkspace::new(Default::default()));
    ws.bind_execution_registry(registry.clone());
    ws.inject_file("/p/main.ts".into(), Arc::from("export {}"));
    ws.inject_file("/p/dep.ts".into(), Arc::from("export const dep = 1"));
    (ws, registry)
}
fn request() -> ResolutionRequest {
    ResolutionRequest {
        importer: "/p/main.ts".into(),
        specifier: "./dep".into(),
        context: CONTEXT,
        published: None,
        overlay: None,
        project: None,
    }
}
fn begin(
    ws: &Arc<MemoryWorkspace>,
    registry: &TaskRegistry,
    stop: CancellationToken,
) -> WorkspaceResolution {
    ws.clone()
        .begin_resolution(request(), registry.register_task(), stop)
}
fn producer(handoff: &mut WorkspaceResolution, waker: &Waker) -> ResolutionProducer {
    match handoff.poll(waker) {
        Poll::Ready(Ok(ResolutionStep::Produce(producer))) => producer,
        _ => panic!("a cold demand transfers its independent producer"),
    }
}
fn hold(
    producer: &mut ResolutionProducer,
    phase: ResolutionPhase,
    waker: &Waker,
) -> ResolutionWork {
    for _ in 0..128 {
        match producer.poll(waker) {
            Poll::Ready(Some(work)) if work.phase() == phase => return work,
            Poll::Ready(Some(work)) => work.run(),
            _ => panic!("expected runnable work before {phase:?}"),
        }
    }
    panic!("bounded phase progression");
}
fn finish(producer: &mut ResolutionProducer, waker: &Waker) {
    for _ in 0..256 {
        match producer.poll(waker) {
            Poll::Ready(Some(work)) => work.run(),
            Poll::Ready(None) => return,
            Poll::Pending => panic!("all held dependencies have been released"),
        }
    }
    panic!("bounded continuation completion");
}
fn assert_target(handoff: &mut WorkspaceResolution, waker: &Waker) {
    let Poll::Ready(Ok(ResolutionStep::Value(value))) = handoff.poll(waker) else {
        panic!("complete answer")
    };
    assert_eq!(
        value.result().map(|r| r.source_id.as_str()),
        Some("/p/dep.ts")
    );
}

#[test]
fn held_input_cancellation_detaches_one_waiter_without_new_root_admission() {
    for phase in [ResolutionPhase::Preflight, ResolutionPhase::Load] {
        let (ws, registry) = fixture();
        let (count, waker) = waker();
        let stop = CancellationToken::default();
        let mut leader = begin(&ws, &registry, stop.clone());
        let mut owner = producer(&mut leader, &waker);
        let held = hold(&mut owner, phase, &waker);
        let mut sibling = begin(&ws, &registry, CancellationToken::default());
        let mut sibling_owner = producer(&mut sibling, &waker);
        // From here the driver admits no new root; it only runs transferred
        // work and resumes the already admitted continuations.
        for _ in 0..128 {
            match sibling_owner.poll(&waker) {
                Poll::Ready(Some(work)) => work.run(),
                Poll::Pending => break,
                Poll::Ready(None) => panic!("sibling waits for held flight"),
            }
        }
        assert!(leader.poll(&waker).is_pending());
        assert!(sibling.poll(&waker).is_pending());
        let before = registry.wait_graph_occupancy().edges;
        let wakes = count.0.load(Ordering::SeqCst);
        stop.cancel();
        assert!(
            count.0.load(Ordering::SeqCst) > wakes,
            "stop wakes the parked subscriber"
        );
        assert!(matches!(
            leader.poll(&waker),
            Poll::Ready(Err(ResolutionError::Cancelled))
        ));
        assert_eq!(registry.wait_graph_occupancy().edges, before - 1);
        // The leader's independent producer and the sibling survive the stop.
        held.run();
        finish(&mut owner, &waker);
        finish(&mut sibling_owner, &waker);
        assert_target(&mut sibling, &waker);
        drop((owner, sibling_owner, leader, sibling));
        assert_eq!(registry.wait_graph_occupancy().edges, 0);
        assert_eq!(registry.active_task_count_for_tests(), 0);
        assert_eq!(ws.resource_snapshot().resolution_flights, 0);
    }
}

#[test]
fn completion_before_repoll_and_spurious_wakes_preserve_the_answer() {
    let (ws, registry) = fixture();
    let (_, waker) = waker();
    let mut root = begin(&ws, &registry, CancellationToken::default());
    let mut owner = producer(&mut root, &waker);
    let held = hold(&mut owner, ResolutionPhase::Load, &waker);
    waker.wake_by_ref();
    assert!(root.poll(&waker).is_pending());
    assert!(
        owner.poll(&waker).is_pending(),
        "a wake does not duplicate transferred work"
    );
    held.run();
    finish(&mut owner, &waker);
    assert_target(&mut root, &waker);
    assert_eq!(registry.wait_graph_occupancy().edges, 0);
    drop((root, owner));
    assert_eq!(registry.active_task_count_for_tests(), 0);
}

#[test]
fn dropped_and_panicking_external_work_publish_typed_terminal_errors() {
    for panic in [false, true] {
        let (ws, registry) = fixture();
        let (_, waker) = waker();
        let mut root = begin(&ws, &registry, CancellationToken::default());
        let mut owner = producer(&mut root, &waker);
        let held = hold(&mut owner, ResolutionPhase::Load, &waker);
        assert!(root.poll(&waker).is_pending());
        if panic {
            crate::engine::resolution_test_hooks::with_hook(
                crate::engine::resolution_test_hooks::ResolutionPhase::DriverRound,
                || panic!("injected input producer failure"),
                || held.run(),
            );
        } else {
            drop(held);
        }
        let expected = if panic {
            ResolutionError::ProducerPanicked
        } else {
            ResolutionError::ProducerDropped
        };
        assert!(matches!(root.poll(&waker), Poll::Ready(Err(error)) if error == expected));
        drop((owner, root));
        assert_eq!(registry.wait_graph_occupancy().edges, 0);
        assert_eq!(registry.active_task_count_for_tests(), 0);
        assert_eq!(ws.resource_snapshot().resolution_flights, 0);
    }
}

#[test]
fn suspended_input_retains_its_view_and_never_publishes_a_superseded_root() {
    let (ws, registry) = fixture();
    let (_, waker) = waker();
    let mut captured = request();
    captured.published = ws.published_root();
    let mut root = ws.clone().begin_resolution(
        captured,
        registry.register_task(),
        CancellationToken::default(),
    );
    let mut owner = producer(&mut root, &waker);
    let held = hold(&mut owner, ResolutionPhase::Load, &waker);
    ws.set_default_resolve_extensions(vec![".custom".into()]);
    held.run();
    finish(&mut owner, &waker);
    let Poll::Ready(Ok(ResolutionStep::Value(value))) = root.poll(&waker) else {
        panic!("terminal view result")
    };
    assert!(!value.trace().published());
    drop((root, owner));
    assert_eq!(registry.wait_graph_occupancy().edges, 0);
}

#[test]
fn owned_resolution_capabilities_are_send() {
    fn send<T: Send>() {}
    send::<WorkspaceResolution>();
    send::<ResolutionProducer>();
    send::<ResolutionWork>();
}

#[test]
fn publication_gate_suspends_without_parking_and_resumes_after_release() {
    let (ws, registry) = fixture();
    let (count, waker) = waker();
    let mut root = begin(&ws, &registry, CancellationToken::default());
    let mut owner = producer(&mut root, &waker);
    let held = hold(&mut owner, ResolutionPhase::Publish, &waker);
    let gate = ws.engine.hold_publication_for_test();
    held.run();
    assert!(owner.poll(&waker).is_pending());
    assert!(root.poll(&waker).is_pending());
    let wakes = count.0.load(Ordering::SeqCst);
    drop(gate);
    assert!(count.0.load(Ordering::SeqCst) > wakes);
    finish(&mut owner, &waker);
    assert_target(&mut root, &waker);
    drop((owner, root));
    assert_eq!(registry.wait_graph_occupancy().edges, 0);
    assert_eq!(registry.active_task_count_for_tests(), 0);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn filesystem_discovery_and_frozen_replay_survive_migration_between_work_slices() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("main.ts"), "export {}").unwrap();
    std::fs::write(dir.path().join("dep.ts"), "export const dep = 1").unwrap();
    let ws = Arc::new(crate::FilesystemWorkspace::new(Default::default()));
    let registry = TaskRegistry::default();
    ws.bind_execution_registry(registry.clone());
    let mut query = request();
    query.importer = dir
        .path()
        .join("main.ts")
        .to_str()
        .unwrap()
        .replace('\\', "/");
    let expected = dir
        .path()
        .join("dep.ts")
        .to_str()
        .unwrap()
        .replace('\\', "/");
    let (_, waker) = waker();
    let mut root = ws.clone().begin_resolution(
        query,
        registry.register_task(),
        CancellationToken::default(),
    );
    let mut owner = producer(&mut root, &waker);
    let mut phases = Vec::new();
    for _ in 0..256 {
        match owner.poll(&waker) {
            Poll::Ready(Some(work)) => {
                phases.push(work.phase());
                // The admitted work migrates; no new root or worker service
                // is created by the production handoff.
                std::thread::spawn(move || work.run()).join().unwrap();
            }
            Poll::Ready(None) => break,
            Poll::Pending => panic!("filesystem owner has no unreleased dependency"),
        }
    }
    assert!(phases.contains(&ResolutionPhase::Discover));
    assert!(phases.contains(&ResolutionPhase::Freeze));
    assert!(phases.contains(&ResolutionPhase::Load));
    let Poll::Ready(Ok(ResolutionStep::Value(value))) = root.poll(&waker) else {
        panic!("owned filesystem answer")
    };
    assert_eq!(
        value.result().map(|r| r.source_id.as_str()),
        Some(expected.as_str())
    );
    let sync = ws.resolve_import_outcome(
        dir.path().join("main.ts").to_str().unwrap(),
        "./dep",
        CONTEXT,
    );
    assert_eq!(
        sync.result().map(|r| r.source_id.as_str()),
        value.result().map(|r| r.source_id.as_str())
    );
    assert_eq!(sync.non_admission_reason(), value.non_admission_reason());
    drop((owner, root));
    assert_eq!(registry.wait_graph_occupancy().edges, 0);
}

#[test]
fn nested_same_path_resolution_reports_cycle_and_leaves_parent_live() {
    let (ws, registry) = fixture();
    let (_, waker) = waker();
    let mut root = begin(&ws, &registry, CancellationToken::default());
    let mut owner = producer(&mut root, &waker);
    let held = hold(&mut owner, ResolutionPhase::Load, &waker);
    let hook_registry = registry.clone();
    let hook_ws = ws.clone();
    let hook_waker = waker.clone();
    crate::engine::resolution_test_hooks::with_hook(
        crate::engine::resolution_test_hooks::ResolutionPhase::PreAdmissionValidation,
        move || {
            let waker = &hook_waker;
            let current = verter_execution::tasks::ExecutionScope::current(&hook_registry).unwrap();
            let mut nested =
                hook_ws
                    .clone()
                    .begin_resolution(request(), current, CancellationToken::default());
            let mut nested_owner = producer(&mut nested, waker);
            finish(&mut nested_owner, waker);
            assert!(matches!(
                nested.poll(waker),
                Poll::Ready(Err(ResolutionError::Cycle))
            ));
        },
        || held.run(),
    );
    finish(&mut owner, &waker);
    assert_target(&mut root, &waker);
    drop((root, owner));
    assert_eq!(registry.active_task_count_for_tests(), 0);
}

#[test]
fn project_only_lookup_retains_its_native_frame_across_input_loading() {
    let (ws, registry) = fixture();
    ws.configure_resolver(vec![crate::ide_project_config(
        "/p".into(),
        "/p".into(),
        None,
    )]);
    let project = verter_session_query::resolution::ProjectOwnership {
        project_root: "/p".into(),
        tsconfig_path: None,
    };
    let sync = ws.resolve_import_for_project_outcome(&project, "./dep", CONTEXT);
    let (_, waker) = waker();
    let mut root = ws.clone().begin_project_resolution(
        ProjectResolutionRequest {
            project,
            specifier: "./dep".into(),
            context: CONTEXT,
        },
        registry.register_task(),
        CancellationToken::default(),
    );
    let mut owner = producer(&mut root, &waker);
    let held = hold(&mut owner, ResolutionPhase::Load, &waker);
    assert!(root.poll(&waker).is_pending());
    held.run();
    finish(&mut owner, &waker);
    let Poll::Ready(Ok(ResolutionStep::Value(value))) = root.poll(&waker) else {
        panic!("owned project-only answer")
    };
    assert_eq!(
        value.result().map(|r| r.source_id.as_str()),
        Some("/p/dep.ts")
    );
    assert_eq!(value.result(), sync.result());
    assert_eq!(value.non_admission_reason(), sync.non_admission_reason());
    drop((root, owner));
    assert_eq!(registry.active_task_count_for_tests(), 0);
}

#[test]
fn workspace_dependency_uses_its_composed_registry_and_rejects_foreign_waiters() {
    let (ws, registry) = fixture();
    let other = TaskRegistry::default();
    let foreign = other.register_task();
    let _scope = verter_execution::tasks::ExecutionScope::install(&other, &foreign);
    let (_, waker) = waker();
    let mut root = ws
        .clone()
        .begin_resolution(request(), foreign, CancellationToken::default());
    assert!(matches!(
        root.poll(&waker),
        Poll::Ready(Err(ResolutionError::Cycle))
    ));
    assert_eq!(registry.wait_graph_occupancy().edges, 0);
    assert_eq!(registry.active_task_count_for_tests(), 0);
    // An ambient scope from another host cannot replace this workspace's
    // injected authority when its external compatibility entry is called.
    let value = ws.resolve_import_outcome("/p/main.ts", "./dep", CONTEXT);
    assert_eq!(
        value.result().map(|r| r.source_id.as_str()),
        Some("/p/dep.ts")
    );
    assert_eq!(registry.active_task_count_for_tests(), 0);
}

#[test]
fn abandoning_a_publication_wait_withdraws_its_edge_before_the_writer_releases() {
    let (ws, registry) = fixture();
    let (_, waker) = waker();
    let mut root = begin(&ws, &registry, CancellationToken::default());
    let mut owner = producer(&mut root, &waker);
    let held = hold(&mut owner, ResolutionPhase::Publish, &waker);
    let gate = ws.engine.hold_publication_for_test();
    held.run();
    assert!(owner.poll(&waker).is_pending());
    assert!(root.poll(&waker).is_pending());
    assert_eq!(registry.wait_graph_occupancy().edges, 2);
    drop(owner);
    assert!(matches!(
        root.poll(&waker),
        Poll::Ready(Err(ResolutionError::ProducerDropped))
    ));
    assert_eq!(registry.wait_graph_occupancy().edges, 0);
    drop(gate);
    assert_eq!(ws.resource_snapshot().resolution_flights, 0);
    drop(root);
    assert_eq!(registry.active_task_count_for_tests(), 0);
}

#[test]
fn wake_repolling_before_a_running_slice_returns_is_not_lost() {
    struct EagerWake {
        owner: std::sync::Weak<parking_lot::Mutex<ResolutionProducer>>,
        queued: std::sync::Weak<parking_lot::Mutex<Vec<ResolutionWork>>>,
        pending: AtomicUsize,
    }
    impl Wake for EagerWake {
        fn wake(self: Arc<Self>) {
            self.wake_by_ref();
        }
        fn wake_by_ref(self: &Arc<Self>) {
            let owner = self.owner.upgrade().expect("external owner stays live");
            let result = owner.lock().poll(&Waker::from(self.clone()));
            match result {
                Poll::Pending => {
                    self.pending.fetch_add(1, Ordering::SeqCst);
                }
                Poll::Ready(Some(work)) => self.queued.upgrade().unwrap().lock().push(work),
                Poll::Ready(None) => {}
            }
        }
    }
    let (ws, registry) = fixture();
    let mut root = begin(&ws, &registry, CancellationToken::default());
    let owner = Arc::new(parking_lot::Mutex::new(producer(&mut root, Waker::noop())));
    let queued = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let eager = Arc::new(EagerWake {
        owner: Arc::downgrade(&owner),
        queued: Arc::downgrade(&queued),
        pending: AtomicUsize::new(0),
    });
    let waker = Waker::from(eager.clone());
    let Poll::Ready(Some(work)) = owner.lock().poll(&waker) else {
        panic!("initial runnable slice")
    };
    work.run();
    assert_eq!(
        eager.pending.load(Ordering::SeqCst),
        1,
        "the executor re-polled before the slice returned"
    );
    let slices = std::mem::take(&mut *queued.lock());
    let resumed = slices.len();
    drop(slices);
    drop((root, owner));
    assert_eq!(registry.wait_graph_occupancy().edges, 0);
    assert_eq!(
        resumed, 1,
        "publishing the suspended future must wake the re-registered producer poller"
    );
}

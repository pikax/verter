use super::*;

#[test]
fn two_task_wait_cycle_is_detected_before_parking() {
    let graph = TaskRegistry::default();
    let task_a = graph.register_task();
    let task_b = graph.register_task();

    let _a_waits_for_b = graph
        .register_wait(task_a.id(), task_b.id())
        .expect("first edge is acyclic");
    assert!(matches!(
        graph.register_wait(task_b.id(), task_a.id()),
        Err(WaitCycle)
    ));
    assert_eq!(graph.wait_count_for_tests(), 1);
}

#[test]
fn three_task_wait_cycle_is_detected_before_parking() {
    let graph = TaskRegistry::default();
    let task_a = graph.register_task();
    let task_b = graph.register_task();
    let task_c = graph.register_task();

    let _a_waits_for_b = graph
        .register_wait(task_a.id(), task_b.id())
        .expect("first edge is acyclic");
    let _b_waits_for_c = graph
        .register_wait(task_b.id(), task_c.id())
        .expect("second edge is acyclic");
    assert!(matches!(
        graph.register_wait(task_c.id(), task_a.id()),
        Err(WaitCycle)
    ));
    assert_eq!(graph.wait_count_for_tests(), 2);
}

#[test]
fn acyclic_wait_edges_register_and_clean_up_independently() {
    let graph = TaskRegistry::default();
    let task_a = graph.register_task();
    let task_b = graph.register_task();
    let task_c = graph.register_task();

    let a_waits_for_b = graph
        .register_wait(task_a.id(), task_b.id())
        .expect("a -> b is acyclic");
    let b_waits_for_c = graph
        .register_wait(task_b.id(), task_c.id())
        .expect("b -> c is acyclic");
    assert_eq!(graph.wait_count_for_tests(), 2);

    drop(a_waits_for_b);
    assert_eq!(graph.wait_count_for_tests(), 1);
    drop(b_waits_for_c);
    assert_eq!(graph.wait_count_for_tests(), 0);
}

#[test]
fn stale_generation_cleanup_cannot_remove_reused_task_or_wait() {
    let graph = TaskRegistry::default();
    let first = graph.register_task();
    let stale = first.id();
    drop(first);

    let reused = graph.register_task();
    let target = graph.register_task();
    assert_eq!(reused.id().slot(), stale.slot(), "task slot must be reused");
    assert_ne!(
        reused.id().generation(),
        stale.generation(),
        "slot reuse must advance the generation"
    );
    let _wait = graph
        .register_wait(reused.id(), target.id())
        .expect("reused task wait is acyclic");

    graph.retire_for_tests(stale);
    graph.remove_wait_for_tests(stale, target.id());
    assert!(graph.is_active_for_tests(reused.id()));
    assert_eq!(graph.wait_count_for_tests(), 1);
}

#[test]
fn panic_and_cancel_style_early_return_clean_wait_registrations() {
    let graph = TaskRegistry::default();
    let target = graph.register_task();

    let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let waiter = graph.register_task();
        let _wait = graph
            .register_wait(waiter.id(), target.id())
            .expect("panic-path wait is acyclic");
        panic!("simulated task panic");
    }));
    assert!(panicked.is_err());
    assert_eq!(graph.wait_count_for_tests(), 0);
    assert_eq!(graph.active_task_count_for_tests(), 1);

    fn cancelled_early(graph: &TaskRegistry, target: crate::tasks::TaskId) {
        let waiter = graph.register_task();
        let _wait = graph
            .register_wait(waiter.id(), target)
            .expect("cancel-path wait is acyclic");
        // A cancellation return drops both RAII registrations.
    }
    cancelled_early(&graph, target.id());
    assert_eq!(graph.wait_count_for_tests(), 0);
    assert_eq!(graph.active_task_count_for_tests(), 1);
}

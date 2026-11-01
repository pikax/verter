//! Cooperative drive-adapter discriminators.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use verter_scheduler::job::CompletionState;
use verter_scheduler::scheduler::Request;
use verter_scheduler::stage::{Priority, TargetStage};

use super::{
    CooperativeDrive, CooperativePoint, CooperativeSchedulerAdapter, CooperativeSubmit,
    CooperativeYield, YieldDecision,
};
use crate::types::HostConfig;
use crate::VerterHost;

/// A yield hook that cancels the adapter at its first yield point and
/// records how often the scheduler was consulted afterwards.
#[derive(Debug)]
struct CancellingYield {
    fired: AtomicUsize,
}

impl CancellingYield {
    fn fenced_calls(&self) -> usize {
        self.fired.load(Ordering::SeqCst)
    }
}

impl CooperativeYield for CancellingYield {
    fn yield_to_runtime(&self) -> YieldDecision {
        self.fired.fetch_add(1, Ordering::SeqCst);
        YieldDecision::Cancelled
    }
}

fn standalone_host() -> VerterHost {
    VerterHost::new_standalone(HostConfig::default())
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn unchanged_carrier_routes_materialize_after_source_publication_unlocks() {
    let host = Arc::new(standalone_host());
    let canonical = "/work/App.vue";
    let initial = "<script setup lang=\"ts\">defineProps<{ title: string }>()</script><template>{{ title }}</template>";
    let edited = "<script setup lang=\"ts\">defineProps<{ title: string }>()</script><template><h1>{{ title }}</h1></template>";
    let upsert = |source: &str| {
        host.upsert(crate::types::UpsertRequest {
            canonical_id: Some(canonical.into()),
            input_id: canonical.into(),
            source: Arc::from(source),
            file_language: host.language_classifier().classify(canonical),
            aliases: Vec::new(),
        })
        .unwrap()
    };
    assert!(upsert(initial).changed);
    assert!(host
        .ensure_indexed_ready_serve_unobserved(canonical)
        .is_some());
    let observed = Arc::new(AtomicUsize::new(0));
    let locked = Arc::new(AtomicUsize::new(0));
    let weak = Arc::downgrade(&host);
    let observed_hook = observed.clone();
    let locked_hook = locked.clone();
    *host.test_force.indexed_source_capture_seam_hook.0.lock() = Some(Arc::new(move || {
        let host = weak.upgrade().expect("fixture owns host");
        observed_hook.fetch_add(1, Ordering::SeqCst);
        if host
            .block_content
            .admission_fence
            .is_owned_by_current_thread()
        {
            locked_hook.fetch_add(1, Ordering::SeqCst);
        }
    }));
    assert!(!upsert(initial).changed);
    assert_eq!(
        observed.load(Ordering::SeqCst),
        0,
        "a byte-identical upsert stays warm"
    );
    assert!(upsert(edited).changed);
    assert_eq!(
        observed.load(Ordering::SeqCst),
        1,
        "the unchanged route is eagerly re-indexed"
    );
    assert_eq!(
        locked.load(Ordering::SeqCst),
        0,
        "indexing may join a producer that needs source publication"
    );
    let current = host
        .ensure_indexed_ready_serve_unobserved(canonical)
        .unwrap();
    assert!(current.store_published);
    assert_eq!(current.indexed.raw_source.as_ref(), edited);
    assert_eq!(
        observed.load(Ordering::SeqCst),
        1,
        "the published route stays warm"
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn upsert_program_discovery_runs_after_source_publication_unlocks() {
    let workspace = Arc::new(verter_workspace::MemoryWorkspace::new(Default::default()));
    workspace.inject_file(
        "/work/tsconfig.json".into(),
        Arc::from(r#"{"files":["entry.ts","dep.ts"]}"#),
    );
    workspace.inject_file(
        "/work/dep.ts".into(),
        Arc::from("export type Ref = string;"),
    );
    workspace.inject_file(
        "/work/entry.ts".into(),
        Arc::from("import type { Ref } from './dep'; export type Box = Ref;"),
    );
    let host = Arc::new(VerterHost::new(HostConfig::default(), workspace.clone()));
    host.configure_projects(vec![verter_workspace::configured_ide_project(
        &*workspace,
        "/work".into(),
        "/work".into(),
        "/work/tsconfig.json",
    )]);
    let observed = Arc::new(AtomicUsize::new(0));
    let locked = Arc::new(AtomicUsize::new(0));
    let weak = Arc::downgrade(&host);
    let observed_hook = observed.clone();
    let locked_hook = locked.clone();
    *host.test_force.indexed_source_capture_seam_hook.0.lock() = Some(Arc::new(move || {
        let host = weak.upgrade().expect("fixture owns host");
        observed_hook.fetch_add(1, Ordering::SeqCst);
        if host
            .block_content
            .admission_fence
            .is_owned_by_current_thread()
        {
            locked_hook.fetch_add(1, Ordering::SeqCst);
        }
    }));
    register_ts_source(
        &host,
        "/work/entry.ts",
        "import type { Ref } from './dep'; export type Box = { value: Ref };",
    );
    assert_eq!(
        observed.load(Ordering::SeqCst),
        1,
        "the changed program member is indexed"
    );
    assert_eq!(
        locked.load(Ordering::SeqCst),
        0,
        "indexing may join a producer that needs source publication"
    );
}

#[test]
fn owned_load_cancellation_admits_no_source_and_transfer_integrates_externally() {
    use std::task::{Poll, Waker};
    use verter_execution::cancellation::CancellationToken;
    use verter_execution::tasks::TaskRegistry;
    let host = standalone_host();
    let stop = CancellationToken::new();
    stop.cancel();
    let mut load = host.begin_load("/cancel.ts", TaskRegistry::default().register_task(), stop);
    assert!(matches!(
        load.poll(Waker::noop()),
        Poll::Ready(Err(crate::HostLoadError::Cancelled))
    ));
    assert!(host.scheduler.try_get_source("/cancel.ts").is_none());

    register_ts_source(&host, "/ready.ts", "export const n = 1;");
    let mut load = host.begin_load(
        "/ready.ts",
        TaskRegistry::default().register_task(),
        CancellationToken::new(),
    );
    let Poll::Ready(Ok(commit)) = load.poll(Waker::noop()) else {
        panic!("an already integrated load is immediately ready");
    };
    assert!(commit.apply(&host).expect("same host accepts its commit"));
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn owned_load_transfers_integration_and_rejects_another_host() {
    use verter_execution::cancellation::CancellationToken;
    fn assert_send<T: Send>() {}
    assert_send::<crate::HostLoadHandoff>();
    assert_send::<crate::HostLoadCommit>();
    let workspace = Arc::new(verter_workspace::MemoryWorkspace::new(Default::default()));
    workspace.inject_file("/cold.ts".into(), Arc::from("export const n = 1;"));
    let host = VerterHost::new(HostConfig::default(), workspace);
    let scope = host.scheduler.enter_execution_scope();
    let before = host.current_validation_token();
    let mut load = host.begin_load("/cold.ts", scope.task().clone(), CancellationToken::new());
    let commit = verter_execution::blocking::block_on(
        verter_execution::blocking::WaitFamily::SchedulerCompletion,
        None,
        |waker| load.poll(waker),
    )
    .expect("scheduler dependency completes");
    assert!(
        !host.dependency_cache().contains_key("/cold.ts"),
        "polling never integrates host state"
    );
    assert!(commit.apply(&host).expect("matching host"));
    assert!(host.dependency_cache().contains_key("/cold.ts"));
    assert_ne!(before, host.current_validation_token());

    let mut warm = host.begin_load("/cold.ts", scope.task().clone(), CancellationToken::new());
    let commit = verter_execution::blocking::block_on(
        verter_execution::blocking::WaitFamily::SchedulerCompletion,
        None,
        |waker| warm.poll(waker),
    )
    .unwrap();
    let other = standalone_host();
    assert!(matches!(
        commit.apply(&other),
        Err(crate::HostLoadError::WrongHost)
    ));
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn owned_load_commit_revalidates_after_reset_and_same_content_replacement() {
    use std::task::{Poll, Waker};
    use verter_execution::cancellation::CancellationToken;
    for replace in [false, true] {
        let host = standalone_host();
        register_ts_source(&host, "/old.ts", "export const n = 1;");
        let scope = host.scheduler.enter_execution_scope();
        let mut load = host.begin_load("/old.ts", scope.task().clone(), CancellationToken::new());
        let Poll::Ready(Ok(commit)) = load.poll(Waker::noop()) else {
            panic!("warm load");
        };
        host.scheduler.reset();
        if replace {
            register_ts_source(&host, "/old.ts", "export const n = 1;");
        }
        assert!(
            matches!(commit.apply(&host), Err(crate::HostLoadError::Superseded)),
            "a retained commit cannot adopt a removed or replacement incarnation"
        );
    }
}

#[test]
fn owned_load_rechecks_cancellation_after_a_continuing_yield() {
    use verter_execution::cancellation::CancellationToken;
    use verter_scheduler::execution::executor::DefaultExecutor;
    use verter_scheduler::scheduler::{Scheduler, SchedulerConfig};
    let mut host = standalone_host();
    let registry = host
        .scheduler
        .enter_execution_scope()
        .task()
        .registry()
        .clone();
    let config = SchedulerConfig {
        cpu_threads: 1,
        io_threads: 1,
        ..SchedulerConfig::default()
    };
    let loader = Arc::new(verter_scheduler::source_loader::MemorySourceLoader::new());
    loader.insert("/yield.ts".into(), Arc::from("export const n = 1;"));
    // This discriminator controls stage execution itself. A threaded driver
    // could execute Source independently of the adapter's yield decision.
    host.scheduler = Scheduler::new_sync_with_executor_and_task_registry(
        config.clone(),
        loader,
        Arc::new(DefaultExecutor),
        #[cfg(not(target_arch = "wasm32"))]
        verter_scheduler::SchedulerCpuPool::new(1, config.resolved_dag_budget().cpu as usize),
        #[cfg(not(target_arch = "wasm32"))]
        verter_scheduler::SchedulerIoPool::new(1, config.resolved_dag_budget().io as usize),
        registry,
    );
    let stop = CancellationToken::new();
    let adapter = CooperativeSchedulerAdapter::with_yield_hook_and_cancellation(
        Arc::new(CancelTokenAtFirstOffer {
            cancellation: stop.clone(),
        }),
        stop.clone(),
    );
    let scope = host.scheduler.enter_execution_scope();
    let mut load = host.begin_load("/yield.ts", scope.task().clone(), stop);
    assert!(matches!(
        adapter.drive_load(&host.scheduler, &mut load),
        Err(crate::HostLoadError::Cancelled)
    ));
    assert!(
        host.scheduler.try_get_source("/yield.ts").is_none(),
        "cancellation at the yield forbids executing Source"
    );
    assert!(
        host.scheduler.drive_one(),
        "the external driver stopped before executing the queued Source stage"
    );
    assert!(host.scheduler.try_get_source("/yield.ts").is_some());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn owned_load_commit_rejects_eviction_before_external_integration() {
    use verter_execution::cancellation::CancellationToken;
    for warm in [false, true] {
        let workspace = Arc::new(verter_workspace::MemoryWorkspace::new(Default::default()));
        workspace.inject_file("/evicted.ts".into(), Arc::from("export const n = 1;"));
        let host = VerterHost::new(HostConfig::default(), workspace);
        if warm {
            assert!(host.ensure_loaded("/evicted.ts"));
        }
        let scope = host.scheduler.enter_execution_scope();
        let mut load = host.begin_load(
            "/evicted.ts",
            scope.task().clone(),
            CancellationToken::new(),
        );
        let commit = verter_execution::blocking::block_on(
            verter_execution::blocking::WaitFamily::SchedulerCompletion,
            None,
            |waker| load.poll(waker),
        )
        .expect("Analysis completes");
        host.evict("/evicted.ts");
        assert!(
            matches!(commit.apply(&host), Err(crate::HostLoadError::Superseded)),
            "a transferred commit cannot reverse a later eviction (warm={warm})"
        );
        assert!(
            host.derived_raw_cache()
                .get("/evicted.ts")
                .expect("evicted state")
                .evicted
        );
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn owned_load_commit_survives_unrelated_eviction() {
    use verter_execution::cancellation::CancellationToken;
    for warm in [false, true] {
        let workspace = Arc::new(verter_workspace::MemoryWorkspace::new(Default::default()));
        workspace.inject_file("/kept.ts".into(), Arc::from("export const kept = 1;"));
        workspace.inject_file("/other.ts".into(), Arc::from("export const other = 1;"));
        let host = VerterHost::new(HostConfig::default(), workspace);
        assert!(host.ensure_loaded("/other.ts"));
        if warm {
            assert!(host.ensure_loaded("/kept.ts"));
        }
        let scope = host.scheduler.enter_execution_scope();
        let mut load = host.begin_load("/kept.ts", scope.task().clone(), CancellationToken::new());
        let commit = verter_execution::blocking::block_on(
            verter_execution::blocking::WaitFamily::SchedulerCompletion,
            None,
            |waker| load.poll(waker),
        )
        .expect("Analysis completes");
        host.evict("/other.ts");
        assert!(commit
            .apply(&host)
            .expect("another file's eviction cannot supersede this load"));
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn owned_reload_commit_distinguishes_its_original_eviction_from_a_later_one() {
    use verter_execution::cancellation::CancellationToken;
    for evict_again in [false, true] {
        let workspace = Arc::new(verter_workspace::MemoryWorkspace::new(Default::default()));
        workspace.inject_file("/reload.ts".into(), Arc::from("export const n = 1;"));
        let host = VerterHost::new(HostConfig::default(), workspace);
        assert!(host.ensure_loaded("/reload.ts"));
        host.evict("/reload.ts");
        let scope = host.scheduler.enter_execution_scope();
        let mut load =
            host.begin_load("/reload.ts", scope.task().clone(), CancellationToken::new());
        let commit = verter_execution::blocking::block_on(
            verter_execution::blocking::WaitFamily::SchedulerCompletion,
            None,
            |waker| load.poll(waker),
        )
        .expect("reload completes");
        if evict_again {
            host.evict("/reload.ts");
        }
        let result = commit.apply(&host);
        if evict_again {
            assert!(matches!(result, Err(crate::HostLoadError::Superseded)));
        } else {
            assert!(result.expect("the original eviction is the reload's input"));
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn owned_reload_commit_observes_cancellation_at_the_visibility_write() {
    use verter_execution::cancellation::CancellationToken;
    let workspace = Arc::new(verter_workspace::MemoryWorkspace::new(Default::default()));
    workspace.inject_file("/cancelled.ts".into(), Arc::from("export const n = 1;"));
    let host = VerterHost::new(HostConfig::default(), workspace);
    assert!(host.ensure_loaded("/cancelled.ts"));
    host.evict("/cancelled.ts");
    let scope = host.scheduler.enter_execution_scope();
    let stop = CancellationToken::new();
    let mut load = host.begin_load("/cancelled.ts", scope.task().clone(), stop.clone());
    let commit = verter_execution::blocking::block_on(
        verter_execution::blocking::WaitFamily::SchedulerCompletion,
        None,
        |waker| load.poll(waker),
    )
    .expect("reload completes");
    *host.test_force.host_load_publish_seam_hook.0.lock() = Some(Arc::new(move || stop.cancel()));
    assert!(matches!(
        commit.apply(&host),
        Err(crate::HostLoadError::Cancelled)
    ));
    assert!(
        host.derived_raw_cache()
            .get("/cancelled.ts")
            .unwrap()
            .evicted
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn cancelled_cold_load_does_not_publish_an_integrated_marker() {
    use verter_execution::cancellation::CancellationToken;
    let workspace = Arc::new(verter_workspace::MemoryWorkspace::new(Default::default()));
    workspace.inject_file(
        "/cold.ts".into(),
        Arc::from("import { n } from './dep'; export { n };"),
    );
    workspace.inject_file("/dep.ts".into(), Arc::from("export const n = 1;"));
    let host = VerterHost::new(HostConfig::default(), workspace);
    let scope = host.scheduler.enter_execution_scope();
    let stop = CancellationToken::new();
    let mut load = host.begin_load("/cold.ts", scope.task().clone(), stop.clone());
    let commit = verter_execution::blocking::block_on(
        verter_execution::blocking::WaitFamily::SchedulerCompletion,
        None,
        |waker| load.poll(waker),
    )
    .unwrap();
    *host.test_force.host_load_publish_seam_hook.0.lock() = Some(Arc::new(move || stop.cancel()));
    assert!(matches!(
        commit.apply(&host),
        Err(crate::HostLoadError::Cancelled)
    ));
    assert!(host.ensure_loaded("/cold.ts"));
    assert_eq!(
        host.ws()
            .dependency_snapshot("/cold.ts")
            .unwrap()
            .parsed_resolved,
        ["/dep.ts".to_string()].into()
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn superseded_load_preserves_replacement_dependencies_and_aliases() {
    use verter_execution::cancellation::CancellationToken;
    let workspace = Arc::new(verter_workspace::MemoryWorkspace::new(Default::default()));
    workspace.inject_file("/replaced.ts".into(), Arc::from("import './old';"));
    workspace.inject_file("/old.ts".into(), Arc::from("export {};"));
    workspace.inject_file("/new.ts".into(), Arc::from("export {};"));
    let host = Arc::new(VerterHost::new(HostConfig::default(), workspace));
    let scope = host.scheduler.enter_execution_scope();
    let mut load = host.begin_load(
        "/replaced.ts",
        scope.task().clone(),
        CancellationToken::new(),
    );
    let commit = verter_execution::blocking::block_on(
        verter_execution::blocking::WaitFamily::SchedulerCompletion,
        None,
        |waker| load.poll(waker),
    )
    .unwrap();
    let weak = Arc::downgrade(&host);
    *host.test_force.host_load_publish_seam_hook.0.lock() = Some(Arc::new(move || {
        let host = weak.upgrade().unwrap();
        let replacement = std::thread::spawn(move || {
            let _update = host
                .upsert(crate::types::UpsertRequest {
                    canonical_id: Some("/replaced.ts".into()),
                    input_id: "/replacement-alias.ts".into(),
                    source: Arc::from("import './new';"),
                    file_language: verter_language::FileLanguage::script_ts(),
                    aliases: vec!["/replacement-alias.ts".into()],
                })
                .unwrap();
        });
        replacement.join().unwrap();
    }));
    assert!(matches!(
        commit.apply(&host),
        Err(crate::HostLoadError::Superseded)
    ));
    let dep = host.dependency_cache().get("/replaced.ts").unwrap();
    assert_eq!(dep.dependencies, ["/new".to_string()].into());
    assert!(dep.aliases.contains("/replacement-alias.ts"));
    drop(dep);
    assert_eq!(
        host.ws()
            .dependency_snapshot("/replaced.ts")
            .unwrap()
            .parsed_resolved,
        ["/new.ts".to_string()].into()
    );
    assert!(host.ensure_loaded("/replaced.ts"));
    assert_eq!(
        host.dependency_cache()
            .get("/replaced.ts")
            .unwrap()
            .dependencies,
        ["/new".to_string()].into()
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn owned_load_commit_rechecks_its_fence_at_the_visibility_write() {
    use verter_execution::cancellation::CancellationToken;
    #[derive(Clone, Copy)]
    enum Mutation {
        Evict,
        Replace,
        ResetProject,
    }
    for mutation in [Mutation::Evict, Mutation::Replace, Mutation::ResetProject] {
        let workspace = Arc::new(verter_workspace::MemoryWorkspace::new(Default::default()));
        workspace.inject_file("/publish.ts".into(), Arc::from("export const n = 1;"));
        let host = Arc::new(VerterHost::new(HostConfig::default(), workspace));
        let scope = host.scheduler.enter_execution_scope();
        let mut load = host.begin_load(
            "/publish.ts",
            scope.task().clone(),
            CancellationToken::new(),
        );
        let commit = verter_execution::blocking::block_on(
            verter_execution::blocking::WaitFamily::SchedulerCompletion,
            None,
            |waker| load.poll(waker),
        )
        .expect("Analysis completes");
        let weak = Arc::downgrade(&host);
        let fired = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let fired_hook = fired.clone();
        *host.test_force.host_load_publish_seam_hook.0.lock() = Some(Arc::new(move || {
            let host = weak.upgrade().expect("fixture owns host");
            fired_hook.store(true, Ordering::SeqCst);
            match mutation {
                Mutation::Evict => host.evict("/publish.ts"),
                Mutation::Replace => {
                    register_ts_source(&host, "/publish.ts", "export const n = 2;")
                }
                Mutation::ResetProject => {
                    host.project_type_store.bump_project_generation_and_evict();
                }
            }
        }));
        let result = commit.apply(&host);
        assert!(
            fired.load(Ordering::SeqCst),
            "the mutation landed after preflight"
        );
        assert!(matches!(result, Err(crate::HostLoadError::Superseded)));
        if matches!(mutation, Mutation::Evict) {
            assert!(
                host.derived_raw_cache().get("/publish.ts").unwrap().evicted,
                "a racing eviction remains hidden"
            );
        }
    }
}

#[test]
fn owned_warm_load_commit_rejects_project_reset_with_unchanged_scheduler_version() {
    use std::task::{Poll, Waker};
    use verter_execution::cancellation::CancellationToken;
    let host = standalone_host();
    register_ts_source(&host, "/authority.ts", "export const n = 1;");
    let version = host
        .scheduler
        .try_get_source("/authority.ts")
        .unwrap()
        .version();
    let scope = host.scheduler.enter_execution_scope();
    let mut load = host.begin_load(
        "/authority.ts",
        scope.task().clone(),
        CancellationToken::new(),
    );
    let Poll::Ready(Ok(commit)) = load.poll(Waker::noop()) else {
        panic!("warm commit");
    };
    host.project_type_store.bump_project_generation_and_evict();
    assert_eq!(
        host.scheduler
            .try_get_source("/authority.ts")
            .unwrap()
            .version(),
        version
    );
    assert!(matches!(
        commit.apply(&host),
        Err(crate::HostLoadError::Superseded)
    ));
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn owned_load_driver_pumps_a_dependency_on_the_only_cpu_worker() {
    use verter_execution::cancellation::CancellationToken;
    use verter_scheduler::execution::executor::{StageError, StageExecutor};
    use verter_scheduler::node::{AnalysisSnapshot, SourceSnapshot};
    use verter_scheduler::scheduler::{Scheduler, SchedulerConfig};
    struct NestedLoad {
        host: Arc<parking_lot::Mutex<Option<std::sync::Weak<VerterHost>>>>,
        stop: CancellationToken,
        result: std::sync::mpsc::Sender<Result<(), crate::HostLoadError>>,
    }
    impl StageExecutor for NestedLoad {
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
        fn execute_analysis(
            &self,
            canonical: &str,
            _: &SourceSnapshot,
            generation: u64,
        ) -> Result<AnalysisSnapshot, StageError> {
            if canonical == "/outer.ts" {
                let host = self.host.lock().as_ref().unwrap().upgrade().unwrap();
                let scope = host.scheduler.enter_execution_scope();
                let mut load =
                    host.begin_load("/nested.ts", scope.task().clone(), self.stop.clone());
                let adapter = CooperativeSchedulerAdapter::with_yield_hook_and_cancellation(
                    Arc::new(super::InlineYield),
                    self.stop.clone(),
                );
                let _ = self
                    .result
                    .send(adapter.drive_load(&host.scheduler, &mut load).map(|_| ()));
            }
            Ok(AnalysisSnapshot::new_empty(generation))
        }
    }
    let mut host = standalone_host();
    let registry = host
        .scheduler
        .enter_execution_scope()
        .task()
        .registry()
        .clone();
    let config = SchedulerConfig {
        cpu_threads: 1,
        io_threads: 1,
        ..SchedulerConfig::default()
    };
    let loader = Arc::new(verter_scheduler::source_loader::MemorySourceLoader::new());
    loader.insert("/outer.ts".into(), Arc::from("export const outer = 1;"));
    loader.insert("/nested.ts".into(), Arc::from("export const nested = 1;"));
    let slot = Arc::new(parking_lot::Mutex::new(None));
    let stop = CancellationToken::new();
    let (send, receive) = std::sync::mpsc::channel();
    host.scheduler = Scheduler::with_executor_and_task_registry(
        config.clone(),
        loader,
        Arc::new(NestedLoad {
            host: slot.clone(),
            stop: stop.clone(),
            result: send,
        }),
        verter_scheduler::SchedulerCpuPool::new(1, config.resolved_dag_budget().cpu as usize),
        verter_scheduler::SchedulerIoPool::new(1, config.resolved_dag_budget().io as usize),
        registry,
    );
    let host = Arc::new(host);
    *slot.lock() = Some(Arc::downgrade(&host));
    let outer = host.scheduler.submit_request(analysis_request("/outer.ts"));
    let completed = receive.recv_timeout(std::time::Duration::from_secs(2));
    // Always wake a parked regression before awaiting or asserting, so the
    // sole worker and its scheduler are released even when pumping is broken.
    stop.cancel();
    let terminal = outer.wait();
    assert!(matches!(terminal, CompletionState::Ready(_)));
    assert!(
        matches!(completed, Ok(Ok(()))),
        "the held CPU worker must run its nested Analysis before cancellation: {completed:?}"
    );
}

fn analysis_request(canonical: &str) -> Request {
    Request {
        file_id: canonical.to_string(),
        target: TargetStage::Analysis,
        priority: Priority::Interactive,
        source: None,
        file_language: None,
        request_context: None,
    }
}

/// Register one TypeScript source and return its canonical id, so a
/// submitted Analysis request has real scheduler work to drive.
fn register_ts_source(host: &VerterHost, canonical: &str, source: &str) {
    use verter_language::FileLanguage;
    let _update = host
        .upsert(crate::types::UpsertRequest {
            canonical_id: Some(canonical.to_string()),
            input_id: canonical.to_string(),
            source: std::sync::Arc::from(source),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("register source");
}

/// An uncancellable adapter with the inline yield hook drives the
/// existing execution contract to the same terminal state as driving
/// the scheduler directly — cooperative points do not rewrite query
/// outcomes.
#[test]
fn uncancellable_drive_preserves_scheduler_outcome() {
    let host = standalone_host();
    register_ts_source(&host, "/coop/a.ts", "export const a = 1;\n");

    let direct = host
        .scheduler
        .submit_request(analysis_request("/coop/a.ts"));
    let direct_state = host.scheduler.wait_or_drive(&direct);

    let adapter = CooperativeSchedulerAdapter::new();
    let cooperatively = adapter.submit(&host.scheduler, analysis_request("/coop/a.ts"));
    let CooperativeSubmit::Submitted(handle) = cooperatively else {
        panic!("uncancelled adapter must submit");
    };
    let adapted_state = match adapter.drive(&host.scheduler, &handle) {
        CooperativeDrive::Driven(state) => state,
        CooperativeDrive::Cancelled(stop) => panic!("uncancelled drive stopped at {stop:?}"),
    };

    let direct_ready = matches!(direct_state, CompletionState::Ready(_));
    assert!(
        direct_ready,
        "control drive must reach Ready (got {direct_state:?})"
    );
    assert!(
        matches!(adapted_state, CompletionState::Ready(_)),
        "cooperative drive must reach the same Ready terminal (got {adapted_state:?})"
    );
}

/// A cancel before submit refuses without consulting the scheduler:
/// nothing is admitted and no handle exists to leak.
#[test]
fn cancel_before_submit_refuses_without_admission() {
    let host = standalone_host();
    register_ts_source(&host, "/coop/b.ts", "export const b = 2;\n");

    let adapter = CooperativeSchedulerAdapter::new();
    adapter.cancel();
    match adapter.submit(&host.scheduler, analysis_request("/coop/b.ts")) {
        CooperativeSubmit::Refused(stop) => assert_eq!(stop.point, CooperativePoint::BeforeSubmit),
        CooperativeSubmit::Submitted(_) => {
            panic!("cancelled adapter must not admit new work")
        }
    }
}

/// A cancel after submit, before drive, returns the typed cancelled
/// outcome and leaves the handle pending — the drive contributed no
/// stages, so no partial result can be presented as an outcome.
#[test]
fn cancel_before_drive_returns_typed_stop_and_no_result() {
    let host = standalone_host();
    register_ts_source(&host, "/coop/c.ts", "export const c = 3;\n");

    let adapter = CooperativeSchedulerAdapter::new();
    let CooperativeSubmit::Submitted(handle) =
        adapter.submit(&host.scheduler, analysis_request("/coop/c.ts"))
    else {
        panic!("uncancelled adapter must submit");
    };
    adapter.cancel();
    match adapter.drive(&host.scheduler, &handle) {
        CooperativeDrive::Cancelled(stop) => {
            assert_eq!(stop.point, CooperativePoint::BeforeDrive);
        }
        CooperativeDrive::Driven(state) => {
            panic!("cancelled drive must not present an outcome (got {state:?})")
        }
    }
}

/// A yield point that withdraws the worker stops the drive before the
/// scheduler is consulted for it; the pending handle is released by
/// dropping it, and cancellation observed at a yield is reported at the
/// yield point — distinct from both submit- and drive-side stops.
#[test]
fn yield_point_withdrawal_stops_before_the_scheduler_is_consulted() {
    let host = standalone_host();
    register_ts_source(&host, "/coop/d.ts", "export const d = 4;\n");

    let hook = Arc::new(CancellingYield {
        fired: AtomicUsize::new(0),
    });
    let adapter = CooperativeSchedulerAdapter::with_yield_hook(hook.clone());
    let CooperativeSubmit::Submitted(handle) =
        adapter.submit(&host.scheduler, analysis_request("/coop/d.ts"))
    else {
        panic!("uncancelled adapter must submit");
    };

    let driven = adapter.drive(&host.scheduler, &handle);
    assert!(
        matches!(
            &driven,
            CooperativeDrive::Cancelled(stop) if stop.point == CooperativePoint::AtYield
        ),
        "yield withdrawal must stop at the yield point (got {driven:?})"
    );
    assert_eq!(hook.fenced_calls(), 1, "exactly one yield point per drive");
    assert!(
        !driven.driven_ready(),
        "a stopped drive is never a Ready outcome"
    );
    // Releasing the retained handle is the caller's drop; the adapter
    // keeps no second reference to it.
    drop(handle);
}

/// A cancelled drive cannot expose a half-committed snapshot: the
/// cancelled outcome carries no value at the type level, and the
/// committed input behind the request stays fence-coherent.
#[test]
fn cancelled_drive_exposes_no_half_committed_snapshot() {
    use crate::input_handoff::{AcquiredFile, CommittedInputHandoff};

    let host = standalone_host();
    register_ts_source(&host, "/coop/e.ts", "export const e = 5;\n");

    let handoff = CommittedInputHandoff::commit(
        [AcquiredFile {
            canonical: std::sync::Arc::from("/coop/e.ts"),
            content: std::sync::Arc::from("export const e = 5;\n"),
        }],
        [],
    )
    .expect("coherent wave");

    let adapter = CooperativeSchedulerAdapter::new();
    let CooperativeSubmit::Submitted(handle) =
        adapter.submit(&host.scheduler, analysis_request("/coop/e.ts"))
    else {
        panic!("uncancelled adapter must submit");
    };
    adapter.cancel();
    let stopped = adapter.drive(&host.scheduler, &handle);
    assert!(matches!(stopped, CooperativeDrive::Cancelled(_)));

    // The committed basis remains exactly the bound identity: nothing
    // the cancelled drive did (or did not) do tore the snapshot fence,
    // and observing it still answers the committed bytes.
    assert_eq!(handoff.binding().admit_publication(handoff.basis()), Ok(()));
    assert!(matches!(
        handoff.observe("/coop/e.ts"),
        crate::input_handoff::HandoffObserve::File { .. }
    ));
}

/// A yield hook that continues on its first offer (before the first
/// driven stage) and withdraws on its second (between stages).
#[derive(Debug)]
struct ContinueOnceYield {
    offers: AtomicUsize,
}

impl ContinueOnceYield {
    fn offers(&self) -> usize {
        self.offers.load(Ordering::SeqCst)
    }
}

impl CooperativeYield for ContinueOnceYield {
    fn yield_to_runtime(&self) -> YieldDecision {
        if self.offers.fetch_add(1, Ordering::SeqCst) == 0 {
            YieldDecision::Continue
        } else {
            YieldDecision::Cancelled
        }
    }
}

/// The cooperative points sit INSIDE the pump a nonthreaded worker
/// runs, not only in front of it. On a scheduler with no driver
/// thread (the wasm/test inline execution model) the adapter drives
/// one `Scheduler::drive_one` stage per iteration and offers the
/// yield hook between stages: a hook that withdraws on its second
/// offer stops the drive with the Source stage committed but Analysis
/// not yet driven — the worker withdrew mid-request, and the pending
/// handle is all a later drive call needs to resume it.
#[test]
fn yield_point_between_stages_stops_mid_pump() {
    use verter_scheduler::scheduler::Scheduler;
    use verter_scheduler::source_loader::MemorySourceLoader;

    let scheduler = Scheduler::test_new_sync(
        verter_scheduler::scheduler::SchedulerConfig::default(),
        Arc::new(MemorySourceLoader::new()),
    );

    let hook = Arc::new(ContinueOnceYield {
        offers: AtomicUsize::new(0),
    });
    let adapter = CooperativeSchedulerAdapter::with_yield_hook(hook.clone());
    let request = Request {
        file_id: "/coop/f.ts".to_string(),
        target: TargetStage::Analysis,
        priority: Priority::Interactive,
        // Inline source: the Source stage commits without depending
        // on the default executor's reading behaviour.
        source: Some(Arc::from("export const f = 6;\n")),
        file_language: None,
        request_context: None,
    };
    let CooperativeSubmit::Submitted(handle) = adapter.submit(&scheduler, request) else {
        panic!("uncancelled adapter must submit");
    };

    let driven = adapter.drive(&scheduler, &handle);
    assert!(
        matches!(
            &driven,
            CooperativeDrive::Cancelled(stop) if stop.point == CooperativePoint::AtYield
        ),
        "the between-stages yield offer must withdraw the worker (got {driven:?})"
    );
    assert_eq!(
        hook.offers(),
        2,
        "one offer before the first stage, one between stages"
    );
    assert!(
        scheduler.try_get_source("/coop/f.ts").is_some(),
        "the Source stage must have been driven before the withdrawal"
    );
    assert!(
        scheduler.try_get_analysis("/coop/f.ts").is_none(),
        "the withdrawal must stop the pump before the Analysis stage is driven"
    );
    drop(handle);
}

/// Cancellation observed between driven stages stops the pump with the
/// typed between-stages stop: at least one stage ran, no result value
/// is carried, and a re-drive of the same handle through a fresh,
/// uncancellable adapter reaches the scheduler's own terminal state —
/// withdrawal never rewrote the request's outcome.
#[test]
fn cancel_between_stages_stops_the_pump_and_a_re_drive_resumes() {
    use verter_scheduler::scheduler::Scheduler;
    use verter_scheduler::source_loader::MemorySourceLoader;

    let scheduler = Scheduler::test_new_sync(
        verter_scheduler::scheduler::SchedulerConfig::default(),
        Arc::new(MemorySourceLoader::new()),
    );

    // Withdraw after the Source stage by cancelling the adapter's
    // token at the between-stages offer — the embedding runtime's
    // out-of-band withdrawal shape.
    let cancellation = verter_execution::cancellation::CancellationToken::new();
    let hook = Arc::new(CancelTokenAtSecondOffer {
        offers: AtomicUsize::new(0),
        cancellation: cancellation.clone(),
    });
    let adapter = CooperativeSchedulerAdapter::with_yield_hook_and_cancellation(hook, cancellation);
    let request = Request {
        file_id: "/coop/g.ts".to_string(),
        target: TargetStage::Analysis,
        priority: Priority::Interactive,
        source: Some(Arc::from("export const g = 7;\n")),
        file_language: None,
        request_context: None,
    };
    let CooperativeSubmit::Submitted(handle) = adapter.submit(&scheduler, request) else {
        panic!("uncancelled adapter must submit");
    };

    let stopped = adapter.drive(&scheduler, &handle);
    assert!(
        matches!(
            &stopped,
            CooperativeDrive::Cancelled(stop) if stop.point == CooperativePoint::BetweenStages
        ),
        "cancellation observed after a driven stage stops at the \
         between-stages point (got {stopped:?})"
    );
    assert!(
        scheduler.try_get_source("/coop/g.ts").is_some(),
        "exactly the Source stage ran before the cancellation"
    );
    assert!(
        scheduler.try_get_analysis("/coop/g.ts").is_none(),
        "the cancelled pump drove no Analysis stage"
    );

    // The handle stays pending and carries no partial outcome: a
    // later drive (here through a fresh uncancellable adapter, the
    // shape an embedding runtime's resumed continuation takes) pumps
    // the same request to the scheduler's own terminal state.
    let resumed = CooperativeSchedulerAdapter::new();
    match resumed.drive(&scheduler, &handle) {
        CooperativeDrive::Driven(state) => assert!(
            !matches!(
                state,
                CompletionState::Superseded | CompletionState::Shutdown
            ),
            "the resumed drive must reach a real terminal state (got {state:?})"
        ),
        CooperativeDrive::Cancelled(stop) => {
            panic!("an uncancellable resumed drive must not stop (got {stop:?})")
        }
    }
    drop(handle);
}

/// Always continues, but trips the shared cancellation token on its
/// second offer, so the stop is observed at the between-stages point
/// rather than at a yield withdrawal.
#[derive(Debug)]
struct CancelTokenAtSecondOffer {
    offers: AtomicUsize,
    cancellation: verter_execution::cancellation::CancellationToken,
}

impl CooperativeYield for CancelTokenAtSecondOffer {
    fn yield_to_runtime(&self) -> YieldDecision {
        if self.offers.fetch_add(1, Ordering::SeqCst) > 0 {
            self.cancellation.cancel();
        }
        YieldDecision::Continue
    }
}

/// With a driver thread installed, the adapter's drive NEVER pumps
/// `Scheduler::drive_one` on the calling thread: the driver owns the
/// pump, and a host/External caller that dequeues and inline-executes
/// ready stages breaks the dual-pool isolation between host
/// coordinator threads and the scheduler's stage pools. The armed
/// dispatch pause parks the driver after one sacrificial dispatch, so
/// the driven request's stages sit READY and undispatched — an
/// uncancellable drive on this thread cannot complete the work itself
/// (it must park on the driver); only releasing the driver finishes
/// the request.
#[test]
fn threaded_drive_parks_on_the_driver_instead_of_inline_pumping() {
    use std::time::Duration;
    use verter_scheduler::scheduler::Scheduler;
    use verter_scheduler::source_loader::MemorySourceLoader;

    let scheduler = Scheduler::test_new(
        verter_scheduler::scheduler::SchedulerConfig::default(),
        Arc::new(MemorySourceLoader::new()),
    );
    assert!(
        scheduler.has_driver_thread(),
        "precondition: the threaded test scheduler must have a driver"
    );
    scheduler.test_arm_dispatch_pause(0);
    // The sacrificial request gives the driver its one dispatch before
    // it parks, so nothing else is dispatched while it is parked.
    let _sacrificial = scheduler.submit_request(Request {
        file_id: "/coop/h1.ts".to_string(),
        target: TargetStage::Analysis,
        priority: Priority::Interactive,
        source: Some(Arc::from("export const h1 = 1;\n")),
        file_language: None,
        request_context: None,
    });
    scheduler.test_wait_until_dispatch_paused();

    let adapter = CooperativeSchedulerAdapter::new();
    let request = Request {
        file_id: "/coop/h2.ts".to_string(),
        target: TargetStage::Analysis,
        priority: Priority::Interactive,
        source: Some(Arc::from("export const h2 = 2;\n")),
        file_language: None,
        request_context: None,
    };
    let CooperativeSubmit::Submitted(handle) = adapter.submit(&scheduler, request) else {
        panic!("uncancelled adapter must submit");
    };

    let completed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let parked_on = handle.clone();
    let drive_thread = {
        let scheduler = Arc::clone(&scheduler);
        let completed = Arc::clone(&completed);
        std::thread::spawn(move || {
            let driven = adapter.drive(&scheduler, &handle);
            completed.store(true, Ordering::SeqCst);
            assert!(
                matches!(driven, CooperativeDrive::Driven(CompletionState::Ready(_))),
                "the parked-on-driver drive must still reach Ready (got {driven:?})"
            );
        })
    };
    // A drive that parks on the driver blocks in the handle's plain wait; one that pumps
    // (inline, or through the cooperative pump's bounded waits) never parks there. The budget
    // is a hang guard only.
    assert!(
        parked_on.test_wait_until_parked(1, Duration::from_secs(10)),
        "with the driver parked, a threaded drive must park on its handle rather than pump"
    );
    assert!(
        !completed.load(Ordering::SeqCst),
        "with the driver parked and the request's stages ready but \
         undispatched, a threaded drive must not complete the work \
         itself — the calling thread is not the pump"
    );
    scheduler.test_release_dispatch_pause();
    drive_thread
        .join()
        .expect("the drive thread must not panic");
}

/// A yield hook that cancels the adapter's token at its first offer but
/// asks the drive to continue: the cancellation is then observed by the
/// drive's own token check, not reported at the yield point.
#[derive(Debug)]
struct CancelTokenAtFirstOffer {
    cancellation: verter_execution::cancellation::CancellationToken,
}

impl CooperativeYield for CancelTokenAtFirstOffer {
    fn yield_to_runtime(&self) -> YieldDecision {
        self.cancellation.cancel();
        YieldDecision::Continue
    }
}

/// On the driver-thread path the adapter pumps no stage itself, so a
/// cancellation observed at the pre-park check is a before-drive stop:
/// the handle stays pending with the scheduler and no stage of this
/// drive ran. Reporting it as between-stages would tell a consumer that
/// separates "withdrew before any work" from "withdrew mid-request" the
/// wrong point.
#[test]
fn cancellation_observed_before_parking_on_the_driver_is_a_before_drive_stop() {
    use verter_scheduler::scheduler::Scheduler;
    use verter_scheduler::source_loader::MemorySourceLoader;

    let scheduler = Scheduler::test_new(
        verter_scheduler::scheduler::SchedulerConfig::default(),
        Arc::new(MemorySourceLoader::new()),
    );
    assert!(
        scheduler.has_driver_thread(),
        "precondition: the threaded test scheduler must have a driver"
    );
    scheduler.test_arm_dispatch_pause(0);
    let _sacrificial = scheduler.submit_request(Request {
        file_id: "/coop/i1.ts".to_string(),
        target: TargetStage::Analysis,
        priority: Priority::Interactive,
        source: Some(Arc::from("export const i1 = 1;\n")),
        file_language: None,
        request_context: None,
    });
    scheduler.test_wait_until_dispatch_paused();

    let cancellation = verter_execution::cancellation::CancellationToken::new();
    let hook = Arc::new(CancelTokenAtFirstOffer {
        cancellation: cancellation.clone(),
    });
    let adapter = CooperativeSchedulerAdapter::with_yield_hook_and_cancellation(hook, cancellation);
    let request = Request {
        file_id: "/coop/i2.ts".to_string(),
        target: TargetStage::Analysis,
        priority: Priority::Interactive,
        source: Some(Arc::from("export const i2 = 2;\n")),
        file_language: None,
        request_context: None,
    };
    let CooperativeSubmit::Submitted(handle) = adapter.submit(&scheduler, request) else {
        panic!("uncancelled adapter must submit");
    };

    let stopped = adapter.drive(&scheduler, &handle);
    assert!(
        matches!(
            &stopped,
            CooperativeDrive::Cancelled(stop) if stop.point == CooperativePoint::BeforeDrive
        ),
        "a cancellation observed before parking on the driver stops before \
         the drive, no stage of this drive having been pumped (got {stopped:?})"
    );
    assert!(
        scheduler.try_get_source("/coop/i2.ts").is_none(),
        "the parked driver dispatched nothing for the cancelled request"
    );
    scheduler.test_release_dispatch_pause();
    drop(handle);
}

#[test]
fn workspace_resolution_shares_scheduler_authority_on_attach_and_replacement() {
    use std::task::{Poll, Waker};
    use verter_execution::cancellation::CancellationToken;
    use verter_workspace::resolution_handoff::{ResolutionRequest, ResolutionStep};
    let make_workspace = || {
        let workspace = Arc::new(verter_workspace::MemoryWorkspace::new(Default::default()));
        workspace.inject_file("/p/main.ts".into(), Arc::from("export {}"));
        workspace.inject_file("/p/dep.ts".into(), Arc::from("export const n = 1"));
        workspace
    };
    let host = VerterHost::new(HostConfig::default(), make_workspace());
    for replace in [false, true] {
        if replace {
            host.set_workspace(make_workspace());
        }
        let _sibling = VerterHost::new(HostConfig::default(), host.workspace());
        let scope = host.scheduler.enter_execution_scope();
        let mut resolution = host.workspace_read().begin_resolution(
            ResolutionRequest {
                importer: "/p/main.ts".into(),
                specifier: "./dep".into(),
                context: verter_session_query::resolution::ResolutionContext {
                    phase: verter_session_query::resolution::ResolvePhase::CodegenBlocker,
                    kind: verter_session_query::resolution::ResolveRequestKind::TypeImport,
                },
                published: None,
                overlay: None,
                project: None,
            },
            scope.task().clone(),
            CancellationToken::default(),
        );
        let mut producer = None;
        let mut completed = false;
        for _ in 0..256 {
            match resolution.poll(Waker::noop()) {
                Poll::Ready(Ok(ResolutionStep::Produce(owner))) => producer = Some(owner),
                Poll::Ready(Ok(ResolutionStep::Value(value))) => {
                    assert_eq!(
                        value.result().map(|r| r.source_id.as_str()),
                        Some("/p/dep.ts")
                    );
                    completed = true;
                    break;
                }
                Poll::Ready(Err(error)) => panic!("workspace shares host authority: {error:?}"),
                Poll::Pending => {}
            }
            if let Some(owner) = producer.as_mut() {
                match owner.poll(Waker::noop()) {
                    Poll::Ready(Some(work)) => work.run(),
                    Poll::Ready(None) => {}
                    Poll::Pending => panic!("in-memory dependency needs no external writer"),
                }
            }
        }
        assert!(completed, "bounded workspace continuation completes");
        assert_eq!(scope.task().registry().wait_graph_occupancy().edges, 0);
    }
}

/// A file's index flight loads its source under the admission fence, so an
/// upsert that follows the flight while holding the fence is never released.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn ambient_ingestion_does_not_follow_a_flight_under_admission() {
    use std::sync::mpsc;
    use std::time::Duration;

    /// How long a blocked run takes to fail. Only the cycle reaches it: every
    /// wait below ends on a signal.
    const CYCLE_DETECTION: Duration = Duration::from_secs(15);

    let workspace = Arc::new(verter_workspace::MemoryWorkspace::new(Default::default()));
    let host = Arc::new(VerterHost::new(HostConfig::default(), workspace.clone()));
    let contributor = "/shared.d.ts";
    workspace.inject_file(
        contributor.into(),
        Arc::from("declare const shared: number;"),
    );

    let (leader_loaded, await_leader) = mpsc::channel();
    let (admission_held, await_admission) = mpsc::channel();
    let await_admission = parking_lot::Mutex::new(await_admission);
    *host.test_force.host_load_publish_seam_hook.0.lock() = Some(Arc::new(move || {
        leader_loaded.send(()).expect("the test is waiting");
        await_admission
            .lock()
            .recv()
            .expect("the upsert reaches its ingestion");
    }));
    *host.test_force.ambient_ingestion_cursor_hook.0.lock() = Some(Arc::new(move || {
        admission_held.send(()).expect("the leader is waiting");
    }));

    let (finished, await_finished) = mpsc::channel();
    let leader_host = Arc::clone(&host);
    let leader_finished = finished.clone();
    std::thread::spawn(move || {
        let served = leader_host.ensure_indexed_ready_serve_unobserved(contributor);
        leader_finished
            .send(served.is_some())
            .expect("the test is waiting");
    });
    await_leader
        .recv_timeout(CYCLE_DETECTION)
        .expect("the leader claims the flight and loads its source");

    let upsert_host = Arc::clone(&host);
    std::thread::spawn(move || {
        register_ts_source(&upsert_host, "/entry.ts", "export const entry = 1;");
        finished.send(true).expect("the test is waiting");
    });

    for _ in 0..2 {
        assert!(
            await_finished
                .recv_timeout(CYCLE_DETECTION)
                .expect("the upsert and the flight's leader must not wait on each other"),
            "the contributor is served"
        );
    }
    assert!(host
        .project_type_store()
        .indexed()
        .get_any(contributor)
        .is_some());
}

/// Readers treat a pending contributor as an incomplete population, so a
/// contributor stays pending until it is indexed, and it is indexed with the
/// admission fence released.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_contributor_stays_pending_until_it_is_indexed_outside_admission() {
    let workspace = Arc::new(verter_workspace::MemoryWorkspace::new(Default::default()));
    let host = Arc::new(VerterHost::new(HostConfig::default(), workspace.clone()));
    workspace.inject_file(
        "/pending.d.ts".into(),
        Arc::from("declare const pending: number;"),
    );
    let observed = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let weak = Arc::downgrade(&host);
    let observed_hook = observed.clone();
    *host.test_force.indexed_source_capture_seam_hook.0.lock() = Some(Arc::new(move || {
        let host = weak.upgrade().expect("fixture owns host");
        observed_hook.lock().push((
            host.block_content
                .admission_fence
                .is_owned_by_current_thread(),
            host.project_type_store()
                .indexed()
                .global_contributor_index()
                .has_pending_overlay_ambient(),
        ));
    }));

    register_ts_source(&host, "/entry.ts", "export const entry = 1;");

    assert_eq!(
        observed.lock().first(),
        Some(&(false, true)),
        "the contributor indexes first, unfenced and still pending"
    );
    let index = host.project_type_store().indexed();
    assert!(!index
        .global_contributor_index()
        .has_pending_overlay_ambient());
    assert!(index.get_any("/pending.d.ts").is_some());
}

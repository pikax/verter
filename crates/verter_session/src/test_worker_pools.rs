//! Test-support worker substrate shared by fresh [`VerterHost`](crate::VerterHost) shells.
//!
//! The value owns execution resources only. A host constructed from it still
//! receives a fresh scheduler/driver, workspace, caches, audit stores, request
//! counters, and declaration-lowering service. This narrow boundary is what
//! lets table-driven tests amortise OS worker creation without turning one
//! test's semantic state into another test's implicit input.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use crate::types::{HostConfig, PoolSpawn};

/// Process-unique identities of the three shared execution pools.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TestHostWorkerPoolIds {
    /// Scheduler CPU-stage pool identity.
    pub scheduler_cpu: usize,
    /// Scheduler source/I/O pool identity.
    pub scheduler_io: usize,
    /// Host batch-coordinator pool identity.
    pub host_cpu: usize,
}

/// Runtime-owned construction receipt used by table-driven tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TestHostWorkerPoolsReceipt {
    /// Identities of the one-time-created worker pools.
    pub pool_ids: TestHostWorkerPoolIds,
    /// Number of fresh host shells constructed from this substrate.
    pub host_shells_created: usize,
    /// Number of fresh schedulers/drivers constructed from this substrate.
    pub scheduler_shells_created: usize,
    /// Number of host/scheduler shells currently holding the exclusive lease.
    /// Must be zero between logical cases.
    pub active_scheduler_shells: usize,
}

/// Reusable worker pools for test-only, sequential fresh-host cases.
///
/// The I/O transport is sized for the stored scheduler configuration. Callers
/// must not run multiple schedulers from the same value concurrently: each
/// scheduler owns an independent DAG admission ledger, while the shared bounded
/// transport is sized for one ledger. This is enforced by an exclusive lease:
/// constructing a second host before the first is dropped fails immediately.
pub struct TestHostWorkerPools {
    pub(crate) scheduler_config: verter_scheduler::scheduler::SchedulerConfig,
    pub(crate) host_pool_spawn: PoolSpawn,
    pub(crate) host_pool_threads: usize,
    pub(crate) scheduler_cpu_pool: Arc<verter_scheduler::SchedulerCpuPool>,
    pub(crate) scheduler_io_pool: Arc<verter_scheduler::SchedulerIoPool>,
    pub(crate) host_cpu_pool: Arc<verter_scheduler::HostCpuPool>,
    host_shells_created: AtomicUsize,
    scheduler_shells_created: AtomicUsize,
    active_scheduler_shells: AtomicUsize,
}

/// Host-owned exclusive lease over one shared test worker substrate.
///
/// The lease is stored as the final field of `VerterHost`, so it is released
/// only after the fresh scheduler, driver, caches, and worker-pool handles have
/// been dropped. This makes the one-ledger-at-a-time I/O capacity invariant an
/// executable boundary rather than a caller convention.
pub(crate) struct TestHostWorkerPoolLease {
    owner: Arc<TestHostWorkerPools>,
}

impl Drop for TestHostWorkerPoolLease {
    fn drop(&mut self) {
        let previous = self
            .owner
            .active_scheduler_shells
            .fetch_sub(1, Ordering::AcqRel);
        assert_eq!(
            previous, 1,
            "shared test worker-pool lease accounting must release exactly one active shell"
        );
    }
}

impl std::fmt::Debug for TestHostWorkerPools {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TestHostWorkerPools")
            .field("scheduler_config", &self.scheduler_config)
            .field("host_pool_spawn", &self.host_pool_spawn)
            .field("host_pool_threads", &self.host_pool_threads)
            .field("pool_ids", &self.pool_ids())
            .field(
                "host_shells_created",
                &self.host_shells_created.load(Ordering::Relaxed),
            )
            .field(
                "scheduler_shells_created",
                &self.scheduler_shells_created.load(Ordering::Relaxed),
            )
            .field(
                "active_scheduler_shells",
                &self.active_scheduler_shells.load(Ordering::Acquire),
            )
            .finish()
    }
}

impl TestHostWorkerPools {
    /// Construct one execution substrate at the production-resolved pool sizes.
    ///
    /// No thread count is capped or substituted: scheduler sizes come directly
    /// from `scheduler_config`, and the host coordinator uses the exact resolved
    /// [`HostConfig`] policy.
    #[must_use]
    pub fn new(
        config: &HostConfig,
        scheduler_config: verter_scheduler::scheduler::SchedulerConfig,
    ) -> Arc<Self> {
        let scheduler_cpu_pool =
            verter_scheduler::SchedulerCpuPool::new(scheduler_config.cpu_threads);
        let scheduler_io_pool = verter_scheduler::SchedulerIoPool::new(
            scheduler_config.io_threads,
            scheduler_config.resolved_dag_budget().io as usize,
        );
        let host_policy = config.resolved_host_cpu_pool_policy();
        let host_pool_threads = host_policy.size.resolve();
        let host_cpu_pool = match host_policy.spawn {
            PoolSpawn::Eager => verter_scheduler::HostCpuPool::new(host_pool_threads),
            PoolSpawn::LazyOnFirstUse => verter_scheduler::HostCpuPool::new_lazy(host_pool_threads),
        };
        Arc::new(Self {
            scheduler_config,
            host_pool_spawn: host_policy.spawn,
            host_pool_threads,
            scheduler_cpu_pool,
            scheduler_io_pool,
            host_cpu_pool,
            host_shells_created: AtomicUsize::new(0),
            scheduler_shells_created: AtomicUsize::new(0),
            active_scheduler_shells: AtomicUsize::new(0),
        })
    }

    /// Identities proving that fresh hosts use this exact shared substrate.
    #[must_use]
    pub fn pool_ids(&self) -> TestHostWorkerPoolIds {
        TestHostWorkerPoolIds {
            scheduler_cpu: self.scheduler_cpu_pool.pool_id(),
            scheduler_io: self.scheduler_io_pool.pool_id(),
            host_cpu: self.host_cpu_pool.pool_id(),
        }
    }

    /// Snapshot the fresh-shell counters and pool identities.
    #[must_use]
    pub fn receipt(&self) -> TestHostWorkerPoolsReceipt {
        TestHostWorkerPoolsReceipt {
            pool_ids: self.pool_ids(),
            host_shells_created: self.host_shells_created.load(Ordering::Relaxed),
            scheduler_shells_created: self.scheduler_shells_created.load(Ordering::Relaxed),
            active_scheduler_shells: self.active_scheduler_shells.load(Ordering::Acquire),
        }
    }

    pub(crate) fn acquire_scheduler_shell(self: &Arc<Self>) -> TestHostWorkerPoolLease {
        self.active_scheduler_shells
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
            .unwrap_or_else(|active| {
                panic!(
                    "shared test worker pools permit one live scheduler shell at a time; \
                     found {active} active shell(s). Drop the prior host before constructing the next"
                )
            });
        TestHostWorkerPoolLease {
            owner: Arc::clone(self),
        }
    }

    pub(crate) fn assert_compatible(&self, config: &HostConfig) {
        let host_policy = config.resolved_host_cpu_pool_policy();
        assert_eq!(
            host_policy.spawn, self.host_pool_spawn,
            "shared test host pools must preserve the configured host-pool spawn policy"
        );
        assert_eq!(
            host_policy.size.resolve(),
            self.host_pool_threads,
            "shared test host pools must preserve the configured host-pool size"
        );
    }

    pub(crate) fn record_scheduler_shell_created(&self) {
        self.scheduler_shells_created
            .fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_host_shell_created(&self) {
        self.host_shells_created.fetch_add(1, Ordering::Relaxed);
    }
}

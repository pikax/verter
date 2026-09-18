//! Owner-affine pool commands.
//!
//! A [`OwnerCommand`] can be submitted only to the pool that owns its
//! marker. CPU work cannot be enqueued on the I/O pool, I/O work cannot
//! land on the CPU pool, and provider/host-coordinator work cannot land
//! on either scheduler pool. The type system is the sole routing
//! authority — there is no runtime fallback that retargets a command.

use std::marker::PhantomData;

use crate::pool::SchedulerPoolTask;

/// Scheduler CPU-pool owner. Commands of this owner submit only to
/// [`crate::pool::SchedulerCpuPool`].
pub enum Cpu {}

/// Scheduler I/O-pool owner. Commands of this owner submit only to
/// [`crate::pool::SchedulerIoPool`].
pub enum Io {}

/// Host/provider coordinator-pool owner. Commands of this owner are
/// not accepted by scheduler CPU or I/O pools.
pub enum Provider {}

/// Fire-and-forget work that may be submitted only to pool `O`.
pub struct OwnerCommand<O> {
    task: SchedulerPoolTask,
    _owner: PhantomData<fn() -> O>,
}

impl OwnerCommand<Cpu> {
    /// Wrap CPU-owned work.
    #[must_use]
    pub fn cpu(task: SchedulerPoolTask) -> Self {
        Self {
            task,
            _owner: PhantomData,
        }
    }
}

impl OwnerCommand<Io> {
    /// Wrap I/O-owned work.
    #[must_use]
    pub fn io(task: SchedulerPoolTask) -> Self {
        Self {
            task,
            _owner: PhantomData,
        }
    }
}

impl OwnerCommand<Provider> {
    /// Wrap provider/host-coordinator work. Scheduler CPU/I/O pools
    /// have no `try_submit(OwnerCommand<Provider>)` method.
    #[must_use]
    pub fn provider(task: SchedulerPoolTask) -> Self {
        Self {
            task,
            _owner: PhantomData,
        }
    }
}

impl<O> OwnerCommand<O> {
    pub(crate) fn into_task(self) -> SchedulerPoolTask {
        self.task
    }
}

/// Type-level proofs that each pool accepts only its owner command.
/// Adding a `try_submit(OwnerCommand<WrongOwner>)` impl makes these
/// fail to compile.
const _: () = {
    fn cpu_pool_accepts_cpu(
        pool: &crate::pool::SchedulerCpuPool,
        command: OwnerCommand<Cpu>,
    ) -> Result<crate::pool::SchedulerPoolSubmitResult, crate::pool::SchedulerPoolSubmitError> {
        pool.try_submit(command)
    }
    fn io_pool_accepts_io(
        pool: &crate::pool::SchedulerIoPool,
        command: OwnerCommand<Io>,
    ) -> Result<crate::pool::SchedulerPoolSubmitResult, crate::pool::SchedulerPoolSubmitError> {
        pool.try_submit(command)
    }
    let _ = cpu_pool_accepts_cpu;
    let _ = io_pool_accepts_io;
};

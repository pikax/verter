//! Concrete execution adapters: the stage-executor seam and the worker pools.
//!
//! This file is the module root. The scheduler itself owns no execution
//! substrate — it coordinates stages, and everything that actually *runs* a
//! stage or owns the threads it runs on lives behind these submodules:
//!
//! - [`executor`](self::executor) — the [`StageExecutor`](self::executor::StageExecutor)
//!   trait the host injects, plus the [`DefaultExecutor`](self::executor::DefaultExecutor)
//!   the scheduler falls back to when a constructor takes no executor.
//! - [`pool`](self::pool) — the two host-constructed scheduler worker pools
//!   (CPU and I/O). The scheduler owns no pool construction: the host builds
//!   them and injects them into every [`Scheduler`](crate::scheduler::Scheduler)
//!   constructor.
//! - [`owner_command`](self::owner_command) — the type-level owner markers that
//!   make pool submission routing a compile-time fact.
//! - [`cpu_concurrency`](self::cpu_concurrency) — the CPU pool's bounded
//!   transport, sized to dominate the DAG CPU budget so the DAG stays the sole
//!   admission gate.
//! - [`host_cpu_pool`](self::host_cpu_pool) — the host/runtime layer's own
//!   batch-coordinator pool, kept isolated from both scheduler pools.
//!
//! The four pool-side submodules are blocking and native-only, so they are
//! gated `cfg(not(target_arch = "wasm32"))`; the WASM scheduler runs stages
//! inline on the calling thread and has no pools. That gating mirrors the
//! other blocking native-only modules in this crate (`audit_publish`).

#[cfg(not(target_arch = "wasm32"))]
pub mod cpu_concurrency;
pub mod executor;
#[cfg(not(target_arch = "wasm32"))]
pub mod host_cpu_pool;
#[cfg(not(target_arch = "wasm32"))]
pub mod owner_command;
#[cfg(not(target_arch = "wasm32"))]
pub mod pool;

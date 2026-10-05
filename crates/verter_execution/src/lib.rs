//! Scheduler-independent execution primitives shared by every layer that
//! runs work: the concrete scheduler, the workspace, the semantic engine and
//! the session.
//!
//! - [`tasks`]: execution task identity, producer ownership and the
//!   wait-for graph that refuses cycles. One [`tasks::TaskRegistry`] is one
//!   cycle authority; layers whose producers may wait on one another share
//!   one instance.
//! - [`cancellation`]: one-shot and aggregate cancellation tokens and the
//!   current-job cancellation scope.
//! - [`pool_size`]: the worker-count policy a host-owned pool resolves at
//!   construction.
//! - [`request_context`]: the opaque request-context carrier, its TLS slots
//!   and the installation/restoration guards.
//!
//! Concrete queues, worker pools, admission and priorities stay in
//! `verter_scheduler`.

#[macro_use]
extern crate verter_debug_assert;

pub mod cancellation;
pub mod pool_size;
pub mod request_context;
pub mod tasks;

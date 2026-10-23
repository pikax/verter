//! Scheduler-independent execution primitives shared by every layer that
//! runs work: the concrete scheduler, the workspace, the semantic engine and
//! the session.
//!
//! - [`blocking`]: the external-boundary driver that runs a pollable
//!   computation to completion on a synchronous caller's thread.
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
//! - [`sync_cell`]: `RefCell`/`Cell` borrow discipline with an atomic
//!   flag, for request state a suspended computation carries between polling
//!   threads.
//! - [`wake`]: the per-subscriber waker registrations a producer keeps for
//!   the pollable computations waiting on it.
//!
//! Concrete queues, worker pools, admission and priorities stay in
//! `verter_scheduler`.

#![forbid(unsafe_code)]

#[macro_use]
extern crate verter_debug_assert;

pub mod blocking;
pub mod cancellation;
pub mod pool_size;
pub mod request_context;
pub mod sync_cell;
pub mod tasks;
pub mod wake;

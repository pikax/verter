//! Async file scheduler for Verter.
//!
//! Files progress independently through Source → Analysis → Artifact stages.
//! Cross-file blocking is declarative — the scheduler manages wakeups.
//!
//! # Architecture
//!
//! - [`FileNode`](node::FileNode) — per-file state: ArcSwap snapshots, generation counter
//! - [`JobIndex`](queue::JobIndex) — indexed priority structure (scheduler-owned)
//! - [`CompletionHandle`](job::CompletionHandle) — request-scoped handle, resolves to exactly one terminal state
//! - [`Priority`](stage::Priority) — 4-tier scheduling with FIFO within tier and aging across tiers
//! - [`EdgeManager`](edges::EdgeManager) — reverse index + blocker registry
//! - [`OverlayMap`](overlay::OverlayMap) — concurrent editor buffer storage
//! - [`SourceLoader`](source_loader::SourceLoader) — file loading trait (memory/disk)

pub mod driver;
pub mod edges;
pub mod executor;
pub mod invalidation;
pub mod job;
pub mod node;
pub mod overlay;
#[cfg(not(target_arch = "wasm32"))]
pub mod pool;
pub mod queue;
pub mod request_context;
pub mod scheduler;
pub mod source_loader;
pub mod stage;

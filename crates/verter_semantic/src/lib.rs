//! # verter_semantic — Semantic authority for Verter
//!
//! Revision-tracked semantic query engine over immutable workspace, parser,
//! and compiler snapshots. Provides component surface resolution, cross-file
//! symbol identity, binding analysis, and reactivity provenance.
//!
//! ## Design principles
//!
//! - Queries are **pure** over immutable input snapshots keyed by revisions
//! - Queries do **not** perform I/O or block on cross-file wakeups
//! - Dependencies are **declared explicitly** as part of evaluation results
//! - Public APIs use **stable refs** and **revision markers**
//!
//! ## Crate boundaries
//!
//! - `verter_semantic` consumes parser outputs and compiler-owned lowered IR
//! - It does not own a parser or lowering pipeline
//! - `verter_session` orchestrates materialization and scheduling

pub mod analyzers;
pub mod db;
pub mod extract;
pub mod facts;
pub mod input;
pub mod migration;
pub mod profile;
pub mod query;
pub mod refs;
pub mod revision;
pub mod snapshot;

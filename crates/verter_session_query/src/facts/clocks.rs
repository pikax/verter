//! Read-only live clocks a fact basis composes: the workspace's content and
//! source-env generations, the project shape generation, and the bracketed
//! semantic-import and route-surface generations.
//!
//! Each reader samples an existing clock owned elsewhere. None of them can
//! advance, bracket or mutate the clock it reads.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use crate::facts::fact_cache::LiveAggregateCounters;

/// The live workspace generations, read through the workspace's live slot so
/// a workspace replacement is visible to the next read.
pub trait WorkspaceClocks: Send + Sync {
    /// The current workspace content (file-set) generation.
    fn content_generation(&self) -> u64;
    /// The current workspace source-environment generation, `None` when the
    /// workspace does not track one.
    fn source_env_generation(&self) -> Option<u64>;
}

/// A read-only project clock selected from its lifetime owner.
#[derive(Clone)]
pub struct ProjectGenerationRead(Arc<AtomicU64>);
impl ProjectGenerationRead {
    /// Read `clock`, which its owner keeps advancing.
    #[must_use]
    pub fn new(clock: Arc<AtomicU64>) -> Self {
        Self(clock)
    }
    fn current(&self) -> u64 {
        self.0.load(Ordering::Acquire)
    }
}

/// Read-only authority over a bracketed generation; it cannot bracket or
/// mutate. The sequence is odd while a mutation is in flight.
#[derive(Clone)]
pub struct BracketedGenerationRead(Arc<AtomicU64>);
impl BracketedGenerationRead {
    /// Read the bracketed sequence `seq`.
    #[must_use]
    pub fn new(seq: Arc<AtomicU64>) -> Self {
        Self(seq)
    }
    /// The current stable generation, or `None` while a mutation is in
    /// flight.
    #[must_use]
    pub fn stable(&self) -> Option<u64> {
        let seq = self.0.load(Ordering::Acquire);
        seq.is_multiple_of(2).then_some(seq)
    }
}

/// Fixed live-clock sampling authority. The workspace slot remains live so a
/// workspace replacement has the same visibility as the host's original read.
/// No workspace, store, mutation, or callback capability can escape this record.
#[derive(Clone)]
pub struct AggregateClockReader {
    workspace: Arc<dyn WorkspaceClocks>,
    project: ProjectGenerationRead,
    imports: BracketedGenerationRead,
    routes: BracketedGenerationRead,
}
impl AggregateClockReader {
    /// Sample `workspace`, `project`, `imports` and `routes`.
    #[must_use]
    pub fn new(
        workspace: Arc<dyn WorkspaceClocks>,
        project: ProjectGenerationRead,
        imports: BracketedGenerationRead,
        routes: BracketedGenerationRead,
    ) -> Self {
        Self {
            workspace,
            project,
            imports,
            routes,
        }
    }
    /// The live aggregate counters, read in the original order.
    #[must_use]
    pub fn live(&self) -> LiveAggregateCounters {
        // Keep the original two workspace selections and read order. A workspace
        // can be replaced between them; each read observes the live slot.
        let content = self.workspace.content_generation();
        let source_env = self.workspace.source_env_generation();
        LiveAggregateCounters {
            content,
            source_env,
            workspace_shape: self.project.current(),
            semantic_imports: self.imports.stable(),
            route_surface: self.routes.stable(),
        }
    }
}

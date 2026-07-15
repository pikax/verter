#![deny(missing_docs)]
//! Per-`RequestKind` data payloads. Each module owns one payload
//! struct; the producer crate (e.g. `verter_compiler` for
//! [`compile::CompilePayload`]) populates it at request close.
//!
//! Payloads are pure data — no behaviour, no logic dependencies on
//! owning crates. Producers convert their domain types through the
//! stringly-typed mirrors in [`tags`].

pub mod bundler;
pub mod cache_outcomes;
pub mod compile;
pub mod component_meta;
pub mod lsp;
pub mod mcp;
pub mod semantic;
pub mod tags;
pub mod type_resolution;
pub mod typeinfo_graph;
pub mod workspace;

pub use bundler::{BundlerBatchPayload, SlowRecordSummary};
pub use compile::CompilePayload;
pub use component_meta::{AuditDiagnosticEntry, AuditDiagnosticKind, ComponentMetaPayload};
pub use lsp::{LspRequestPayload, PositionInfo};
pub use mcp::McpToolPayload;
pub use semantic::SemanticAnalysisPayload;
pub use type_resolution::TypeResolutionPayload;
pub use typeinfo_graph::{
    ExactnessTag, FrameworkSurfaceKindSupportTag, GraphClosurePolicyTag, GraphOperationTag,
    ReductionDemandTag, TypeInfoDegradationReasonTag, TypeInfoGraphPayload,
};
pub use workspace::{WorkspaceOp, WorkspacePayload};

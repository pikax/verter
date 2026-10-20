#![deny(missing_docs)]
//! `verter_audit` — leaf observability substrate for Verter.
//!
//! This crate holds DTOs for the audit record envelope, the
//! per-`RequestKind` payload data structs, the [`AuditObserver`]
//! trait that lower crates emit through with [`observe!`], and the
//! capture-availability vocabulary in [`observe`].
//!
//! Capture is optional state. Without the `semantic-observe` feature the
//! observer slot, its install guards and every producer emission compile
//! away; the DTOs stay, so a default build can still describe a record as
//! [`AuditCaptureState::Unavailable`].
//!
//! The substrate is intentionally a leaf — it must NOT depend on any
//! other `verter_*` crate apart from [`verter_span`]. Higher layers
//! (notably `verter_session`) own the concrete `HostAuditRuntime`,
//! the `AuditRecordsStore` instance, the per-request accumulator, and
//! the footprint miner. They populate the data types defined here.
//!
//! ## Module layout
//!
//! - [`attribution`] — measurement-only work attribution: the closed
//!   [`attribution::WorkSite`] schema plus, behind the non-default
//!   `semantic-observe` feature, the counter table and its reader. Nothing
//!   in a default build can read a counter, so it cannot become a
//!   semantic authority.
//! - [`record`] — top-level [`record::RequestAuditRecord`] envelope plus
//!   [`record::RequestKind`] / [`record::RequestKindPayload`]
//!   discriminants and the [`record::IncidentalFields`] trait.
//! - [`audited_result`] — [`audited_result::AuditedResult`], the
//!   audit-bearing execution envelope pairing a request outcome with
//!   its [`record::RequestAuditRecord`].
//! - [`files`] — [`files::FileAudit`] / [`files::FileRole`]
//!   per-file attribution attached to the envelope's `files` field.
//! - [`timing`] — [`timing::RequestTimingAudit`].
//! - [`memory`] — [`memory::RequestMemoryAudit`] +
//!   [`memory::current_process_rss`].
//! - [`store`] — [`store::RequestStoreAudit`] (kind-agnostic store
//!   counters; materialiser-specific fields live in
//!   [`payloads::component_meta::ComponentMetaPayload`]).
//! - [`footprint`] — [`footprint::RequestFootprintAudit`] and the
//!   per-record vector types it owns.
//! - [`origin_graph`] — derivation-edge tags and DTOs
//!   ([`origin_graph::OriginEdgeKind`], [`origin_graph::OriginEdgeMetaDto`],
//!   …) plus value-mirror enums consumed by
//!   [`structured_event::StructuredAuditEvent`].
//! - [`structured_event`] — [`structured_event::StructuredAuditEvent`]
//!   enum.
//! - [`observer`] — [`observer::AuditObserver`] trait,
//!   [`observer::AuditEvent`] and, with `semantic-observe`, the
//!   observer TLS slot and its install/clear guards.
//! - [`observe`] — compile-time capture availability, the execution
//!   mode a root entry selects, and zero-size counters for optional
//!   per-request state.
//! - [`config`] — [`config::AuditConfig`] + consumer filter.
//! - [`payloads`] — per-`RequestKind` payload data structs.
//! - [`scheduler`] — [`scheduler::SchedulerAudit`] +
//!   [`scheduler::WorkerPool`] / [`scheduler::SchedulerDepths`].
//! - [`batch`] — [`batch::BatchAuditAggregator`] folds an
//!   [`batch::AuditRecordSource`] into a
//!   [`payloads::BundlerBatchPayload`].

pub mod attribution;
pub mod audited_result;
pub mod batch;
pub mod config;
pub mod files;
pub mod footprint;
pub mod instant;
pub mod memory;
pub mod observe;
pub mod observer;
pub mod origin_graph;
pub mod payloads;
pub mod published_surface;
#[cfg(test)]
mod published_surface_tests;
pub mod record;
pub mod scheduler;
pub mod store;
pub mod structured_event;
pub mod timing;
pub mod waits;

// Common re-exports — keep narrow; consumers `use` the module path
// for everything else.

pub use audited_result::AuditedResult;
pub use batch::{AuditRecordSource, BatchAuditAggregator, SLOWEST_RECORD_LIMIT};
pub use config::{AuditCaps, AuditConfig, AuditConsumerFilter};
pub use files::{FileAudit, FileRole};
pub use footprint::{
    AliasResolveRecord, CacheOutcomeTally, ConditionalRecord, GraphCompletenessReport,
    IndexedReadyBuildRecord, InstantiationRecord, MaterializationRecord, ProjectionRecord,
    RequestFootprintAudit, ResolverHotPathCounters, SharedLoadReuseRecord, SubstitutionRecord,
    TruncationCounters, VfsReadRecord,
};
pub use memory::{current_process_rss, RequestMemoryAudit};
pub use observe::{CaptureAvailability, ObserveMode};
#[cfg(feature = "semantic-observe")]
pub use observer::{clear_observer, current_observer, install_observer, ObserverGuard};
pub use observer::{AuditEvent, AuditObserver};
pub use origin_graph::{
    AuditPropertyKey, AuditTopLevelOwnerKind, AuditUniqueSymbolIdentity, ConditionalBranch,
    DerivationEdgeRaw, DerivationEdgeRecord, DerivationSubgraph, DispatchKeyKind, EdgeId,
    MaterializationScopeAudit, MaterializationSubject, MaterializeSkipReason, MemberEdgeProvenance,
    NamedIdentity, NodeId, NodeRecord, NormalizeKind, OriginEdgeKind, OriginEdgeMetaDto,
    ProjectPathSegment, ProjectionModeAudit, SemanticNodeKind, VfsLayer,
};
pub use published_surface::{
    event_name_to_on_prop_name, names_for_policy, AnalyzedSurface, AnalyzedSurfaceItem,
    PolicyNamesResult, PublishedSurfacePolicy, COMPAT_BLOCKED_SLOT_NAMES, VUE_INTRINSIC_ATTR_NAMES,
};

pub use payloads::cache_outcomes::CacheOutcomeKind;
pub use payloads::tags::{
    AdmissionRefusalReason, AugmentationTargetKindTag, BundlerKindTag, CompileBackendTag,
    CompileProductSetTag, FactKeyKindTag, FactLaneTag, FileArtifactCacheAction,
    FlowSliceBudgetAxisTag, LspMethodTag, ProjectionModeTag,
};
pub use payloads::{
    AuditDiagnosticEntry, AuditDiagnosticKind, BundlerBatchPayload, CompilePayload,
    ComponentMetaPayload, FlowDegradationTag, FlowFailureTag, FlowPartialityTag,
    FlowReturnInferencePayload, LspRequestPayload, McpToolPayload, SemanticAnalysisPayload,
    SlowRecordSummary, TypeResolutionPayload, WorkspaceOp, WorkspacePayload,
};
pub use record::{
    AuditCaptureState, Hash16, IncidentalFields, RequestAuditRecord, RequestKind,
    RequestKindPayload, RequestPhaseAudit, RequestTargetIdentity, WALKER_DEPTH_CAP,
};
pub use scheduler::{SchedulerAudit, SchedulerDepths, WorkerPool};
pub use store::RequestStoreAudit;
pub use structured_event::{NonAdmissionReason, StructuredAuditEvent};
pub use timing::RequestTimingAudit;
pub use waits::WaitAudit;

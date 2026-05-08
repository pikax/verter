#![deny(missing_docs)]
//! Top-level audit record envelope: [`RequestAuditRecord`] +
//! [`RequestKind`] / [`RequestKindPayload`] discriminants + the
//! [`IncidentalFields`] trait used to mask snapshot-flaky payloads.

use serde::{Deserialize, Serialize};

use crate::files::FileAudit;
use crate::footprint::RequestFootprintAudit;
use crate::memory::RequestMemoryAudit;
use crate::payloads::tags::{BundlerKindTag, CompileTargetTag, LspMethodTag};
use crate::payloads::{
    BundlerBatchPayload, CompilePayload, ComponentMetaPayload, LspRequestPayload, McpToolPayload,
    SemanticAnalysisPayload, TypeResolutionPayload, WorkspaceOp, WorkspacePayload,
};
use crate::scheduler::SchedulerAudit;
use crate::store::RequestStoreAudit;
use crate::timing::RequestTimingAudit;
use crate::waits::WaitAudit;

/// 128-bit content / semantic hash, byte array form. Defined here so
/// the substrate has zero cross-crate type dependencies; existing
/// `verter_session::types::Hash16` re-exports this alias.
pub type Hash16 = [u8; 16];

/// Walker depth cap shared by every depth-sensitive consumer.
///
/// Mirrors `verter_session::component_meta_audit::assertions::WALKER_DEPTH_CAP`
/// (`= 256`). The substrate keeps its own definition so type-resolution
/// audit producers (and downstream consumers) can compare
/// `TypeResolutionPayload::depth_high_water` against the cap without
/// taking a `verter_session` dependency. The
/// `walker_depth_cap_substrate_matches_session` test in
/// `verter_session::tests::architecture_guards` keeps the two values
/// pinned in lock-step so a silent drift surfaces as a named failure.
pub const WALKER_DEPTH_CAP: u16 = 256;

/// `serde_with`-style helper for the `u64` decimal-string transport
/// shared across audit DTOs. Encodes a `u64` as a base-10 string in
/// JSON / TS so the value survives a round-trip through engines that
/// would otherwise lose precision past `2^53`.
pub mod u64_as_decimal_string {
    use serde::{Deserialize, Deserializer, Serializer};

    /// Serialise a `u64` as a base-10 string.
    ///
    /// # Errors
    /// Returns the underlying serializer's error type unchanged.
    pub fn serialize<S>(value: &u64, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&value.to_string())
    }

    /// Deserialise a `u64` from a base-10 string.
    ///
    /// # Errors
    /// Returns a serde parse error when the input is not a valid
    /// base-10 unsigned 64-bit integer.
    pub fn deserialize<'de, D>(deserializer: D) -> Result<u64, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        s.parse::<u64>().map_err(serde::de::Error::custom)
    }
}

/// `serde_with`-style helper for `i64` decimal-string transport. Same
/// rationale as [`u64_as_decimal_string`] for signed integers.
pub mod i64_as_decimal_string {
    use serde::{Deserialize, Deserializer, Serializer};

    /// Serialise an `i64` as a base-10 string.
    ///
    /// # Errors
    /// Returns the underlying serializer's error type unchanged.
    pub fn serialize<S>(value: &i64, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&value.to_string())
    }

    /// Deserialise an `i64` from a base-10 string.
    ///
    /// # Errors
    /// Returns a serde parse error when the input is not a valid
    /// base-10 signed 64-bit integer.
    pub fn deserialize<'de, D>(deserializer: D) -> Result<i64, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        s.parse::<i64>().map_err(serde::de::Error::custom)
    }
}

/// Top-level audit record for one logical request — the envelope every
/// consumer (NAPI, WASM, LSP, MCP, bundler) reads.
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct RequestAuditRecord {
    /// Monotonic request id stamped at the public audited entry-point.
    /// Decimal-string transport — non-zero, unique per audited request.
    #[serde(with = "u64_as_decimal_string")]
    #[ts(type = "string")]
    pub request_id: u64,
    /// Canonical file id the request targeted. Empty allowed for
    /// kinds that do not have a single canonical (e.g. some MCP
    /// tool calls).
    pub canonical_id: String,
    /// Discriminant identifying which `RequestKind` this record is
    /// for. Producers populate the matching variant in
    /// [`Self::kind_payload`].
    pub kind: RequestKind,
    /// Optional parent-request correlation id. Promoted to the
    /// envelope so it remains accessible when scheduler data is
    /// absent (e.g. WASM, MCP, LSP fast paths).
    /// Always `None` until producers wire it from
    /// `entry.request_context` at scheduler dispatch.
    pub parent_request_id: Option<String>,
    /// `true` when the audited request was satisfied from the warm
    /// component-meta result cache. Cold cold-resolver runs leave
    /// this `false`. Serde-default for back-compat with payloads
    /// emitted before this field landed.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub from_cache: bool,
    /// Per-phase wall-clock timings (ms).
    pub timings: RequestTimingAudit,
    /// Process memory snapshots (before/after/delta + host/workspace
    /// caches).
    pub memory: RequestMemoryAudit,
    /// Generic store/view counters that apply to every request kind.
    /// Materializer- and ComponentMeta-specific store counters live
    /// in [`crate::payloads::ComponentMetaPayload`].
    pub store: RequestStoreAudit,
    /// Optional semantic footprint (component-meta only). Populated
    /// when `HostConfig::footprint_capture` is true and the
    /// accumulator collected work for this request.
    pub footprint: Option<RequestFootprintAudit>,
    /// Optional scheduler-side attribution captured at first dispatch.
    /// Always `None` on WASM (no scheduler) and `None` on native
    /// requests that did not flow through scheduler dispatch.
    /// Serde-default for back-compat with payloads emitted before
    /// this field landed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scheduler: Option<SchedulerAudit>,
    /// Per-file attribution for every file the request observed.
    /// Deduplicated by `canonical_id` — one entry per canonical file.
    /// The vector is empty when no producer populated per-file
    /// attribution. Read-once-aware: per-entry `*_ms` timings are
    /// `Some` only when this request triggered the work.
    ///
    /// Serde-default for back-compat with audit payloads written
    /// before this field landed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<FileAudit>,
    /// Optional lock + queue contention attribution. Populated only
    /// when `HostConfig::audit_timing_capture = true`. Always `None`
    /// when the timing flag is off so producers short-circuit their
    /// `Instant::now()` capture and the zero-cost path is preserved.
    /// Serde-default for back-compat with payloads emitted before
    /// this field landed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waits: Option<WaitAudit>,
    /// Per-`RequestKind` strongly-typed payload. The variant tag
    /// MUST match [`Self::kind`].
    pub kind_payload: RequestKindPayload,
    /// Per-request correlation id for tracing-span association.
    /// Populated by the audited entry-point when an audit
    /// registration is installed; the matching tracing span emits
    /// the same value as a span field so consumers can join audit
    /// records to log captures by `trace_id`.
    /// Serde-default empty string for back-compat with payloads
    /// emitted before this field landed.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub trace_id: String,
}

impl RequestAuditRecord {
    /// Borrow the record's component-meta payload, if any.
    ///
    /// Returns `None` if [`Self::kind_payload`] is not the
    /// component-meta variant. Use this in tests instead of pattern
    /// matching when a typed accessor is more readable.
    #[must_use]
    pub fn component_meta_payload(&self) -> Option<&ComponentMetaPayload> {
        match &self.kind_payload {
            RequestKindPayload::ComponentMeta(p) => Some(p),
            _ => None,
        }
    }

    /// Borrow the record's type-resolution payload, if any.
    #[must_use]
    pub fn type_resolution_payload(&self) -> Option<&TypeResolutionPayload> {
        match &self.kind_payload {
            RequestKindPayload::TypeResolution(p) => Some(p),
            _ => None,
        }
    }

    /// Borrow the record's compile payload, if any.
    #[must_use]
    pub fn compile_payload(&self) -> Option<&CompilePayload> {
        match &self.kind_payload {
            RequestKindPayload::Compile(p) => Some(p),
            _ => None,
        }
    }

    /// Borrow the record's semantic-analysis payload, if any.
    #[must_use]
    pub fn semantic_analysis_payload(&self) -> Option<&SemanticAnalysisPayload> {
        match &self.kind_payload {
            RequestKindPayload::SemanticAnalysis(p) => Some(p),
            _ => None,
        }
    }

    /// Borrow the record's workspace payload, if any.
    #[must_use]
    pub fn workspace_payload(&self) -> Option<&WorkspacePayload> {
        match &self.kind_payload {
            RequestKindPayload::Workspace(p) => Some(p),
            _ => None,
        }
    }

    /// Borrow the record's LSP payload, if any.
    #[must_use]
    pub fn lsp_payload(&self) -> Option<&LspRequestPayload> {
        match &self.kind_payload {
            RequestKindPayload::Lsp(p) => Some(p),
            _ => None,
        }
    }

    /// Borrow the record's MCP payload, if any.
    #[must_use]
    pub fn mcp_payload(&self) -> Option<&McpToolPayload> {
        match &self.kind_payload {
            RequestKindPayload::Mcp(p) => Some(p),
            _ => None,
        }
    }

    /// Borrow the record's bundler-batch payload, if any.
    #[must_use]
    pub fn bundler_batch_payload(&self) -> Option<&BundlerBatchPayload> {
        match &self.kind_payload {
            RequestKindPayload::BundlerBatch(p) => Some(p),
            _ => None,
        }
    }
}

/// Discriminant naming the producer surface that emitted the record.
///
/// Variants that can be parameterized by a stringly-typed tag
/// (`Compile`, `Workspace`, `Lsp`, `BundlerBatch`) carry a small
/// string-mirror enum from [`crate::payloads::tags`] so the substrate
/// stays decoupled from owning crates' concrete types.
///
/// `Custom` is the open-ended escape hatch; producers that need an
/// ad-hoc kind set the stringly name and document why their concern
/// did not warrant a first-class variant.
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS, PartialEq, Eq, Hash)]
#[ts(export, export_to = "audit.generated.ts")]
pub enum RequestKind {
    /// `getComponentMeta(canonicalId)` — the original audit surface.
    ComponentMeta,
    /// Type-resolution query (resolver_core entry-point).
    TypeResolution,
    /// Semantic-analysis (`AnalysisReady`) build.
    SemanticAnalysis,
    /// Compile request — VDOM or IDE codegen.
    Compile {
        /// Which codegen target ran.
        target: CompileTargetTag,
    },
    /// Workspace-side operation (resolve, dep-graph traverse,
    /// resolver walk).
    Workspace {
        /// Which workspace operation ran.
        op: WorkspaceOp,
    },
    /// LSP request handler.
    Lsp {
        /// Which LSP method ran.
        method: LspMethodTag,
    },
    /// MCP tool invocation.
    Mcp {
        /// Tool name (free-form — plugin authors define these).
        tool: String,
    },
    /// Bundler batch summary.
    BundlerBatch {
        /// Which bundler kind produced the batch (vite, webpack, …).
        kind: BundlerKindTag,
    },
    /// Open-ended escape hatch.
    Custom {
        /// Free-form name describing the custom kind.
        name: String,
    },
}

impl RequestKind {
    /// Match a textual kind filter (e.g. from a JSON-RPC parameter or
    /// a CLI flag) against this `RequestKind`. Returns `true` when
    /// the variant tag matches the filter regardless of the inner
    /// fields. The filter strings mirror the variant names exactly.
    ///
    /// Consumers (NAPI / WASM / LSP / CLI) share this matcher so that
    /// the recognised kind set never drifts between transports.
    #[must_use]
    pub fn matches_filter(&self, filter: &str) -> bool {
        matches!(
            (filter, self),
            ("ComponentMeta", RequestKind::ComponentMeta)
                | ("TypeResolution", RequestKind::TypeResolution)
                | ("SemanticAnalysis", RequestKind::SemanticAnalysis)
                | ("Compile", RequestKind::Compile { .. })
                | ("Workspace", RequestKind::Workspace { .. })
                | ("Lsp", RequestKind::Lsp { .. })
                | ("Mcp", RequestKind::Mcp { .. })
                | ("BundlerBatch", RequestKind::BundlerBatch { .. })
                | ("Custom", RequestKind::Custom { .. })
        )
    }
}

/// Strongly-typed payload paired with [`RequestKind`]. The variant
/// tag matches the kind discriminant; the special `None` variant is
/// used when a producer has not populated a typed payload yet (the
/// `RequestAuditRecord` envelope still carries the generic timing /
/// memory / store / footprint data in that case).
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
#[serde(tag = "kind")]
pub enum RequestKindPayload {
    /// Producer has not populated a typed payload yet. Generic
    /// envelope fields still apply.
    None,
    /// Component-meta payload — materializer-specific store counters
    /// plus solver counters that apply only when the
    /// [`RequestKind`] is `ComponentMeta`.
    ComponentMeta(ComponentMetaPayload),
    /// Type-resolution payload (per-query mode counters).
    TypeResolution(TypeResolutionPayload),
    /// Semantic-analysis payload.
    SemanticAnalysis(SemanticAnalysisPayload),
    /// Compile payload (per-phase timings, codegen counts).
    Compile(CompilePayload),
    /// Workspace payload.
    Workspace(WorkspacePayload),
    /// LSP request payload.
    Lsp(LspRequestPayload),
    /// MCP tool payload.
    Mcp(McpToolPayload),
    /// Bundler batch summary.
    BundlerBatch(BundlerBatchPayload),
}

/// Phase-specific audit data threaded through TLS by the request
/// context guard. Currently only the imported-root-proof phase is
/// instrumented in detail; other phases are timed via the top-level
/// `RequestTimingAudit` blocks.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct RequestPhaseAudit {
    /// Total milliseconds spent inside the imported-root-proof phase
    /// for the current request. Accumulated against the top-of-stack
    /// TLS entry by the session-side `record_imported_root_proof_ms`.
    pub imported_root_proof_ms: f64,
}

/// Contract for audit record types whose fields include
/// timing-incidental payloads that must be cleared before snapshot
/// comparison.
///
/// `incidental_fields()` enumerates the field names that are
/// incidental — fixture snapshots are stable against changes to
/// these fields' contents but not against changes to which fields
/// are listed (adding a field implies pinned snapshots need
/// regeneration). `mask_incidental(&mut self)` clears every payload
/// whose name appears in `incidental_fields()`.
///
/// The `commit_7_snapshots_stable_against_current_incidental_event_names_list`
/// test in `verter_session::tests::corpus_generator_parity` pins
/// each implementor's declared set so a silent expansion surfaces
/// as a named failure rather than flapping snapshots.
///
/// Implementors today: [`crate::footprint::RequestFootprintAudit`]
/// (one incidental field, `vfs_reads`).
pub trait IncidentalFields {
    /// Names of the fields cleared by [`Self::mask_incidental`].
    /// `'static` so callers can compare slices and emit names in
    /// diagnostics without lifetime juggling.
    fn incidental_fields() -> &'static [&'static str];

    /// Clear every payload whose field name is in
    /// [`Self::incidental_fields`]. Implementations must branch on
    /// the listed names — an unknown name is a contract violation
    /// and should panic so the lock-step regression surfaces
    /// immediately.
    fn mask_incidental(&mut self);
}

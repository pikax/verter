//! Typed NAPI entry-points for the audit-record producers.
//!
//! Each function on `NapiVerterHost` here wraps one
//! `VerterHost::*_with_audit` producer and returns the produced
//! `RequestAuditRecord` as a JSON-serialised `Buffer`. Consumers
//! decode the buffer into `@verter/types/audit.generated`'s
//! `RequestAuditRecord` shape.
//!
//! Eight typed exports exposed alongside the existing
//! `MetaSession.getComponentMetaWithAudit` surface:
//!
//! 1. `getComponentMetaWithAudit` (existing, on `MetaSession`).
//! 2. `resolveTypeWithAudit` — wraps
//!    [`verter_session::VerterHost::resolve_type_with_audit`].
//! 3. `compileWithAudit` — wraps
//!    [`verter_session::VerterHost::compile_with_audit`].
//! 4. `analyzeWithAudit` — wraps
//!    [`verter_session::VerterHost::analyze_with_audit`].
//! 5. `auditWorkspaceOp` — wraps
//!    [`verter_session::VerterHost::audit_workspace_op`].
//! 6. `getLastAuditRecord` — drains the most recently published
//!    record from the host's `AuditRecordsStore`.
//! 7. `getAuditRecords` — non-destructive filtered query over the
//!    store (`{ kind?, sinceRequestId?, limit? }`).
//! 8. `getBundlerBatchSummary` — invokes
//!    [`verter_audit::batch::BatchAuditAggregator`] over the store
//!    and returns a `BundlerBatchPayload`.
//!
//! All exports return `Buffer` (JSON-serialised payload) for parity
//! with the existing `getComponentMetaWithAudit` contract.
//!
//! Implementation note: the napi-derive class-registration looks up
//! the `js_name` rename from the impl block's containing module.
//! Because the `NapiVerterHost` struct is declared in `lib.rs`, the
//! `#[napi] impl NapiVerterHost` block carrying the audit methods
//! must live in `lib.rs` too — splitting it into a sibling module
//! breaks the `js_name = "VerterHost"` rename. This file therefore
//! exposes only the helper types / free functions that the inline
//! `lib.rs` impl block consumes.

use napi::bindgen_prelude::*;
use napi::{Error, Status};
use napi_derive::napi;

use verter_audit::{payloads::tags::BundlerKindTag, RequestAuditRecord, RequestKind, WorkspaceOp};
use verter_compiler::compile::CompileTarget;

/// Encode a `RequestAuditRecord` to a `Buffer` (JSON UTF-8).
pub(crate) fn encode_record(record: &RequestAuditRecord) -> Result<Buffer> {
    let bytes = serde_json::to_vec(record).map_err(|e| {
        Error::new(
            Status::GenericFailure,
            format!("audit record serialization error: {e}"),
        )
    })?;
    Ok(Buffer::from(bytes))
}

/// Encode a list of records to a `Buffer` (JSON UTF-8 array).
pub(crate) fn encode_record_list(records: &[RequestAuditRecord]) -> Result<Buffer> {
    let bytes = serde_json::to_vec(records).map_err(|e| {
        Error::new(
            Status::GenericFailure,
            format!("audit record list serialization error: {e}"),
        )
    })?;
    Ok(Buffer::from(bytes))
}

/// Map a string target name (`"BUNDLER"` / `"IDE"` / `"ANALYSIS"` /
/// `"META"` / `"TSX"` / `"TSC"`) to a `CompileTarget`. Returns a
/// NAPI error on unknown names so callers see a clear failure
/// rather than a silent wrong-target compile.
pub(crate) fn parse_compile_target(name: &str) -> Result<CompileTarget> {
    match name {
        "BUNDLER" => Ok(CompileTarget::BUNDLER),
        "IDE" => Ok(CompileTarget::IDE),
        "ANALYSIS" => Ok(CompileTarget::ANALYSIS),
        "META" => Ok(CompileTarget::META),
        "TSX" => Ok(CompileTarget::TSX),
        "TSC" => Ok(CompileTarget::TSC),
        other => Err(Error::new(
            Status::InvalidArg,
            format!(
                "unknown compile target '{other}'; \
                 expected one of: BUNDLER, IDE, ANALYSIS, META, TSX, TSC"
            ),
        )),
    }
}

/// Workspace op argument decoded into a [`WorkspaceOp`]. The
/// camelCase JS object lays the variant-specific fields flat
/// alongside the `type` discriminant; missing fields surface as a
/// NAPI error.
#[napi(object)]
pub struct NapiWorkspaceOp {
    /// Discriminant: `"AuditResolve"`, `"DepGraphTraverse"`, or
    /// `"ResolverWalk"`.
    pub r#type: String,
    /// `AuditResolve` / `ResolverWalk` only — module specifier.
    pub specifier: Option<String>,
    /// `AuditResolve` only — importer canonical id.
    pub from: Option<String>,
    /// `DepGraphTraverse` only — root canonical id to traverse.
    pub root: Option<String>,
}

impl NapiWorkspaceOp {
    pub(crate) fn try_into_workspace_op(self) -> Result<WorkspaceOp> {
        match self.r#type.as_str() {
            "AuditResolve" => {
                let specifier = self.specifier.ok_or_else(|| {
                    Error::new(
                        Status::InvalidArg,
                        "AuditResolve requires `specifier`".to_string(),
                    )
                })?;
                let from = self.from.ok_or_else(|| {
                    Error::new(
                        Status::InvalidArg,
                        "AuditResolve requires `from`".to_string(),
                    )
                })?;
                Ok(WorkspaceOp::AuditResolve { specifier, from })
            }
            "DepGraphTraverse" => {
                let root = self.root.ok_or_else(|| {
                    Error::new(
                        Status::InvalidArg,
                        "DepGraphTraverse requires `root`".to_string(),
                    )
                })?;
                Ok(WorkspaceOp::DepGraphTraverse { root })
            }
            "ResolverWalk" => {
                let specifier = self.specifier.ok_or_else(|| {
                    Error::new(
                        Status::InvalidArg,
                        "ResolverWalk requires `specifier`".to_string(),
                    )
                })?;
                Ok(WorkspaceOp::ResolverWalk { specifier })
            }
            other => Err(Error::new(
                Status::InvalidArg,
                format!(
                    "unknown workspace op type '{other}'; \
                     expected 'AuditResolve', 'DepGraphTraverse', or 'ResolverWalk'"
                ),
            )),
        }
    }
}

/// Filter argument for `getAuditRecords`. Each field is independent
/// — combining them narrows the result set further.
#[napi(object)]
#[derive(Default)]
pub struct NapiAuditRecordFilter {
    /// Discriminant name to filter by. Accepted values:
    /// `"ComponentMeta"`, `"TypeResolution"`, `"SemanticAnalysis"`,
    /// `"Compile"`, `"Workspace"`, `"Lsp"`, `"Mcp"`,
    /// `"BundlerBatch"`, `"Custom"`.
    pub kind: Option<String>,
    /// Minimum request id (exclusive). Decimal string matching the
    /// JSON serialization of `request_id`.
    pub since_request_id: Option<String>,
    /// Cap the returned record count.
    pub limit: Option<u32>,
}

/// Filter argument for `getBundlerBatchSummary`.
#[napi(object)]
#[derive(Default)]
pub struct NapiBundlerBatchSummaryArgs {
    /// Bundler kind tag — `"Vite"`, `"Webpack"`, `"Rollup"`,
    /// `"Esbuild"`, `"Rolldown"`, or any other string for `Other`.
    /// Defaults to `"Vite"`.
    pub kind: Option<String>,
    /// Optional minimum request id watermark (exclusive).
    pub since_request_id: Option<String>,
}

/// Match the textual `kind` filter against a `RequestKind`.
pub(crate) fn kind_matches(filter: &str, kind: &RequestKind) -> bool {
    matches!(
        (filter, kind),
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

/// Map a textual bundler-kind tag.
pub(crate) fn parse_bundler_kind(name: Option<&str>) -> BundlerKindTag {
    match name.unwrap_or("Vite") {
        "Vite" => BundlerKindTag::Vite,
        "Webpack" => BundlerKindTag::Webpack,
        "Rollup" => BundlerKindTag::Rollup,
        "Esbuild" => BundlerKindTag::Esbuild,
        "Rolldown" => BundlerKindTag::Rolldown,
        other => BundlerKindTag::Other(other.to_string()),
    }
}

/// Parse a decimal-string request id (matching the JSON
/// serialization on the record) into a `u64`.
pub(crate) fn parse_request_id_str(s: &str) -> Result<u64> {
    s.parse::<u64>().map_err(|e| {
        Error::new(
            Status::InvalidArg,
            format!("expected decimal request id, got '{s}': {e}"),
        )
    })
}

//! Newline-delimited JSON request/response schema for the differential-baseline
//! bridge.
//!
//! The TS DX runner speaks exactly these messages to the bridge over
//! stdin/stdout. Every response is a NORMALIZED provider output plus provider
//! capability metadata — the runner never sees raw tsgo/tsserver protocol
//! frames, and never re-implements provider discovery or versioned artifact
//! sync. All of that stays on the Rust side of this seam.

use serde::{Deserialize, Serialize};

use verter_type_runtime::protocol as rt;

/// LSP document version (the authored `.vue` URI version). Mirrors the LSP
/// `version` field; `i64` so an absent/`-1` value round-trips cleanly.
pub type Version = i64;

/// Which baseline type provider the bridge drives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderName {
    Tsgo,
    Tsserver,
}

impl ProviderName {
    pub fn as_str(self) -> &'static str {
        match self {
            ProviderName::Tsgo => "tsgo",
            ProviderName::Tsserver => "tsserver",
        }
    }
}

/// Role of a materialized file in the baseline workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileRole {
    /// An editor-open entry artifact (`.vue.tsx`) — drives diagnostics.
    Entry,
    /// A public-API twin (`.vue.ts`) — import-resolution only.
    Api,
    /// A support file (vendored shim, lib, etc.) — import-resolution only.
    Support,
}

/// A single materialized file pushed to the provider.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BaselineFile {
    /// Generated/provider-graph path (e.g. `…/Foo.vue.tsx`, `…/Foo.vue.ts`).
    pub path: String,
    pub content: String,
    pub role: FileRole,
    /// The artifact's `sourceMapIdentity`, when materialization produced a map
    /// for it. Carried on the initial `open` so an edit-0 `requiresSourceMap`
    /// probe is not falsely refused for an artifact that DOES have a map. `None`
    /// records a map-absent artifact (e.g. a support `.d.ts`); only an entry
    /// artifact's map gates the `requiresSourceMap` refusal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_map_identity: Option<String>,
}

/// The pinned, deterministic TypeScript tool root passed in `hello`.
///
/// For `provider = tsserver`, `tsserver_tsdk` + `expected_tsserver_js` are
/// required in strict CI; the bridge passes the explicit tsdk into discovery
/// and refuses any discovered path that is not exactly `expected_tsserver_js`
/// (never silently falling back to global npm).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolRoot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tsserver_tsdk: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_tsserver_js: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tsserver_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tsgo_bin: Option<String>,
}

/// `hello` — handshake; resolves + spawns the provider under strict tool-root
/// enforcement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HelloRequest {
    pub workspace_root: String,
    pub repo_root: String,
    pub provider: ProviderName,
    pub strict_ci: bool,
    pub tool_root: ToolRoot,
}

/// `open` — push the initial set of materialized files at a version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenRequest {
    pub files: Vec<BaselineFile>,
    pub version: Version,
}

/// One `.vue.ts` public-API twin refreshed by an edit, carrying its OWN authored
/// document version.
///
/// LSP document versions are document-local. When an edit to a parent document
/// refreshes a child's public-API twin (an import-closure refresh), the child's
/// twin content changes but the child's own LSP version does not advance to the
/// parent's. Carrying the twin's own version here lets the bridge apply the
/// refreshed content for import resolution while versioning each authored URI
/// independently — a parent edit at v5 never marks a child fresh-through-v5.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangedTwin {
    /// Generated `.vue.ts` twin path.
    pub path: String,
    /// The twin's authored document LSP version (NOT the edited document's).
    pub version: Version,
}

/// `syncArtifacts` — apply the per-edit version-stamped artifact overlay for one
/// authored `.vue` URI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncArtifactsRequest {
    /// Authored `.vue` URI whose edit produced these artifacts.
    pub uri: String,
    /// LSP document version for `uri`. Advances ONLY this authored URI's
    /// probe-gate version — never any sibling artifact in `files`.
    pub version: Version,
    pub files: Vec<BaselineFile>,
    /// Stable hash over emitted map content + compile profile.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_map_identity: Option<String>,
    /// Every `.vue.ts` twin whose content changed due to this edit, each carrying
    /// its OWN authored document version (never the edited document's version).
    /// These advance each named twin's authored URI independently.
    #[serde(default)]
    pub changed_public_api_twins: Vec<ChangedTwin>,
}

/// Provider query method (all single-offset).
///
/// `codeAction` is intentionally absent: the `query` envelope carries a single
/// `offset`, while code actions need a start/end range, so they are not exposed
/// over this single-offset query surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum QueryMethod {
    Completion,
    Hover,
    Definition,
    TypeDefinition,
    References,
}

/// `query` — a single provider query at a generated byte offset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryRequest {
    pub method: QueryMethod,
    /// Authored `.vue` URI (drives the overlay version gate).
    pub uri: String,
    /// Generated/provider-graph path to query.
    pub path: String,
    /// Byte offset in the generated file.
    pub offset: u32,
    pub version: Version,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_character: Option<String>,
    /// When `true`, the probe requires the targeted artifact's source map. If no
    /// source map is present for `uri`, the bridge refuses with
    /// `compiled_code_map_absent` instead of running the provider.
    #[serde(default)]
    pub requires_source_map: bool,
}

/// `resolveCompletion` — re-issue `completionItem/resolve` for ONE completion
/// item's typed provider resolve handle, recovering its auto-import
/// `additionalTextEdits`.
///
/// This is the lazy auto-import-on-accept route: the runner first runs a
/// `completion` query, picks the item carrying an actionable
/// [`rt::CompletionResolveData`] handle (`resolveData` on the
/// [`NormalizedCompletionItem`]), and sends it back here so the SAME real
/// provider (tsgo or tsserver) resolves the import edit. It is the bridge-side
/// surface that lets the differential prove tsserver and tsgo return the SAME
/// resolved edits.
// Not `Eq`: `data` is a `CompletionResolveData` carrying a non-`Eq`
// `serde_json::Value`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveCompletionRequest {
    /// Authored `.vue` URI (drives the overlay version gate, same as `query`).
    pub uri: String,
    /// Generated/provider-graph path the resolve is re-issued against.
    pub path: String,
    pub version: Version,
    /// The provider-pure resolve handle minted on the completion item at list
    /// time (the `resolveData` returned by a prior `completion` query).
    pub data: rt::CompletionResolveData,
}

/// `diagnostics` — pull diagnostics for one generated file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsRequest {
    pub uri: String,
    pub path: String,
    pub version: Version,
    /// When `true`, the probe requires the targeted artifact's source map, and
    /// the bridge refuses with `compiled_code_map_absent` when none is present —
    /// the same map-presence gate `query` applies, so map-absent enforcement is
    /// consistent across probe kinds.
    #[serde(default)]
    pub requires_source_map: bool,
}

/// One bridge request line.
// Not `Eq`: `ResolveCompletion` carries a non-`Eq` resolve handle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Request {
    Hello(HelloRequest),
    Open(OpenRequest),
    SyncArtifacts(SyncArtifactsRequest),
    Query(QueryRequest),
    ResolveCompletion(ResolveCompletionRequest),
    Diagnostics(DiagnosticsRequest),
    Shutdown,
}

// ── Responses ────────────────────────────────────────────────────────────

/// Provider capability metadata surfaced to the runner so a probe's confidence
/// is structural, not an ad-hoc annotation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCapabilities {
    pub provider: ProviderName,
    /// Units of `query.offset` crossing this seam. The `TypeProvider` trait is
    /// byte-offset based (`query.offset` is a generated-file byte offset), so
    /// this is `"utf-8"`. UTF-16 is internal to the downstream tsgo/tsserver LSP
    /// wire only and never crosses this bridge.
    pub position_encoding: String,
    pub diagnostics_push: bool,
    pub completion_resolve: bool,
}

impl ProviderCapabilities {
    /// Capability metadata for a spawned provider. The offsets crossing this seam
    /// are generated-file BYTE offsets (the `TypeProvider` trait contract), so the
    /// advertised encoding is `"utf-8"`. tsgo and tsserver convert to UTF-16 on
    /// their own LSP wire internally, but that conversion never reaches the runner.
    /// Both push diagnostics and support completion resolve.
    pub fn for_provider(provider: ProviderName) -> Self {
        ProviderCapabilities {
            provider,
            position_encoding: "utf-8".to_string(),
            diagnostics_push: true,
            completion_resolve: true,
        }
    }
}

/// `hello` reply.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HelloResponse {
    pub ok: bool,
    pub provider: ProviderName,
    /// `true` when a non-strict run gracefully skipped a missing provider.
    pub skipped: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skip_reason: Option<String>,
    /// The actual tool root the provider ran against. Recorded in the baseline
    /// manifest for every provider run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_tool_root_used: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<ProviderCapabilities>,
}

/// `open` reply.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenResponse {
    pub ok: bool,
    /// Generated paths opened in the provider, in request order.
    pub opened: Vec<String>,
    pub version: Version,
}

/// What the overlay did with one synced file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncAction {
    /// Newly opened editor-entry artifact.
    Opened,
    /// Newly loaded (import-resolution-only) support/api file.
    Loaded,
    /// Updated an already-open file.
    Updated,
}

/// One applied sync entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppliedSync {
    pub path: String,
    pub action: SyncAction,
}

/// `syncArtifacts` reply.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncArtifactsResponse {
    pub ok: bool,
    pub uri: String,
    pub version: Version,
    pub applied: Vec<AppliedSync>,
}

/// Normalized hover output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NormalizedHover {
    pub contents: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range_start: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range_end: Option<u32>,
}

/// Normalized source location.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NormalizedLocation {
    pub path: String,
    pub start: u32,
    pub end: u32,
}

/// Normalized completion item.
// Not `Eq`: `resolve_data` carries a `CompletionResolveData` whose `data` blob is
// a `serde_json::Value` (not `Eq`). `PartialEq` is enough for the round-trip tests.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NormalizedCompletionItem {
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub insert_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort_text: Option<String>,
    /// The provider-pure resolve handle minted on this item at list time, when it
    /// carries one. The runner sends an item's `resolveData` back via
    /// `resolveCompletion` to recover the auto-import edits — the bridge-side
    /// surface for the lazy auto-import-on-accept differential. Omitted for an
    /// item with no resolve handle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolve_data: Option<rt::CompletionResolveData>,
}

/// A provider-neutral editor diagnostic tag on the baseline wire, mirroring
/// [`rt::TypeDiagnosticTag`]. Serialized as a lowercase string so the TS bridge
/// mirror reads `"unnecessary"` / `"deprecated"` directly (the user-visible
/// gray-out / strikethrough contract crosses this seam intact).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NormalizedDiagnosticTag {
    Unnecessary,
    Deprecated,
}

impl From<rt::TypeDiagnosticTag> for NormalizedDiagnosticTag {
    fn from(t: rt::TypeDiagnosticTag) -> Self {
        match t {
            rt::TypeDiagnosticTag::Unnecessary => NormalizedDiagnosticTag::Unnecessary,
            rt::TypeDiagnosticTag::Deprecated => NormalizedDiagnosticTag::Deprecated,
        }
    }
}

/// Normalized diagnostic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NormalizedDiagnostic {
    pub message: String,
    pub severity: String,
    pub start: u32,
    pub end: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    /// Editor-facing tags (unused-symbol fade / deprecation strikethrough). Empty
    /// when the diagnostic carries no tags, so a tagless baseline diagnostic
    /// serializes an empty array rather than an absent field — the dx-harness can
    /// guard the gray-out contract end-to-end.
    #[serde(default)]
    pub tags: Vec<NormalizedDiagnosticTag>,
}

/// Normalized query result, keyed by the kind that produced it.
// Not `Eq`: `Completion` carries `NormalizedCompletionItem`s whose resolve handle
// is not `Eq`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum QueryResult {
    // The enum-level `rename_all` renames variant tags; the per-variant
    // `rename_all` renames each variant's struct fields, so multi-word fields
    // (`is_incomplete` → `isIncomplete`) stay camelCase like the rest of the wire.
    #[serde(rename_all = "camelCase")]
    Hover { hover: Option<NormalizedHover> },
    #[serde(rename_all = "camelCase")]
    Completion {
        items: Vec<NormalizedCompletionItem>,
        is_incomplete: bool,
    },
    #[serde(rename_all = "camelCase")]
    Definition { locations: Vec<NormalizedLocation> },
}

/// `query` reply.
// Not `Eq`: contains a `QueryResult` (non-`Eq` via the completion handle).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryResponse {
    pub method: QueryMethod,
    pub uri: String,
    pub version: Version,
    pub result: QueryResult,
    pub capabilities: ProviderCapabilities,
}

/// A normalized resolved auto-import text edit (generated-file byte offsets) —
/// the bridge-side mirror of [`rt::ResolvedTextEdit`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NormalizedResolvedTextEdit {
    pub start: u32,
    pub end: u32,
    pub new_text: String,
}

/// `resolveCompletion` reply: the resolved auto-import edits for one completion
/// item, plus any lazy detail/documentation enrichment the provider returned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveCompletionResponse {
    pub uri: String,
    pub version: Version,
    /// The auto-import additional text edits in generated-file byte offsets.
    /// Empty when the entry resolved to no edits (a local symbol, or a
    /// drifted/mis-keyed offset — fail-closed, never a wrong import).
    pub additional_text_edits: Vec<NormalizedResolvedTextEdit>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub documentation: Option<String>,
    pub capabilities: ProviderCapabilities,
}

/// `diagnostics` reply.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsResponse {
    pub uri: String,
    pub version: Version,
    pub diagnostics: Vec<NormalizedDiagnostic>,
    pub capabilities: ProviderCapabilities,
}

/// `shutdown` reply.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShutdownResponse {
    pub ok: bool,
    /// Rust-authoritative count of provider probes (query + diagnostics) the
    /// bridge actually executed this session. Required CI fails on `0`.
    pub baseline_ran: u64,
}

/// Typed error category. Serialized snake_case so the wire kinds match the
/// design exactly (`baseline_artifact_stale`, `baseline_tool_root_mismatch`, …).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    /// A probe arrived at version `V` but the overlay has no artifacts for
    /// `uri`, or only artifacts for `< V`.
    BaselineArtifactStale,
    /// Strict CI: tsserver resolved to a path other than `expected_tsserver_js`.
    BaselineToolRootMismatch,
    /// Strict CI: a required tool-root field was missing, or a required tool
    /// (tsgo/node/tsserver) was not found.
    BaselineToolRootMissing,
    /// `requiresSourceMap` probe but verter returned no map at this version.
    CompiledCodeMapAbsent,
    /// The provider process failed the operation.
    ProviderError,
    /// A request arrived before a successful `hello`.
    NotInitialized,
    /// The request could not be parsed or violated a precondition.
    InvalidRequest,
}

/// Error reply.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorResponse {
    pub kind: ErrorKind,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
    /// The version the probe asked for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_version: Option<Version>,
    /// The newest version the overlay actually holds for `uri`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub have_version: Option<Version>,
}

/// One bridge response line.
// Not `Eq`: `Query` carries a `QueryResponse` (non-`Eq` via the completion handle).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Response {
    Hello(HelloResponse),
    Open(OpenResponse),
    SyncArtifacts(SyncArtifactsResponse),
    Query(QueryResponse),
    ResolveCompletion(ResolveCompletionResponse),
    Diagnostics(DiagnosticsResponse),
    Shutdown(ShutdownResponse),
    Error(ErrorResponse),
}

impl Response {
    /// Build a stale-artifact refusal for a probe at `version` on `uri`.
    pub fn stale(uri: &str, requested: Version, have: Option<Version>) -> Self {
        Response::Error(ErrorResponse {
            kind: ErrorKind::BaselineArtifactStale,
            message: format!(
                "baseline artifact for {uri} is stale: requested v{requested}, have {}",
                have.map(|v| v.to_string())
                    .unwrap_or_else(|| "none".to_string())
            ),
            uri: Some(uri.to_string()),
            requested_version: Some(requested),
            have_version: have,
        })
    }

    /// Build a `compiled_code_map_absent` refusal for a `requiresSourceMap`
    /// probe at `version` on `uri` that has no source map.
    pub fn map_absent(uri: &str, requested: Version) -> Self {
        Response::Error(ErrorResponse {
            kind: ErrorKind::CompiledCodeMapAbsent,
            message: format!(
                "requiresSourceMap probe on {uri} at v{requested} but no source map is present"
            ),
            uri: Some(uri.to_string()),
            requested_version: Some(requested),
            have_version: None,
        })
    }

    /// Build a generic typed error.
    pub fn error(kind: ErrorKind, message: impl Into<String>) -> Self {
        Response::Error(ErrorResponse {
            kind,
            message: message.into(),
            uri: None,
            requested_version: None,
            have_version: None,
        })
    }
}

// ── Normalizers from provider-runtime DTOs ─────────────────────────────────

impl From<&rt::HoverInfo> for NormalizedHover {
    fn from(h: &rt::HoverInfo) -> Self {
        NormalizedHover {
            contents: h.contents.clone(),
            range_start: h.range_start,
            range_end: h.range_end,
        }
    }
}

impl From<&rt::TypeLocation> for NormalizedLocation {
    fn from(l: &rt::TypeLocation) -> Self {
        NormalizedLocation {
            path: l.path.clone(),
            start: l.start,
            end: l.end,
        }
    }
}

impl From<&rt::Completion> for NormalizedCompletionItem {
    fn from(c: &rt::Completion) -> Self {
        NormalizedCompletionItem {
            label: c.label.clone(),
            kind: c.kind.map(|k| format!("{k:?}")),
            detail: c.detail.clone(),
            insert_text: c.insert_text.clone(),
            sort_text: c.sort_text.clone(),
            resolve_data: c.data.clone(),
        }
    }
}

impl From<&rt::ResolvedTextEdit> for NormalizedResolvedTextEdit {
    fn from(e: &rt::ResolvedTextEdit) -> Self {
        NormalizedResolvedTextEdit {
            start: e.start,
            end: e.end,
            new_text: e.new_text.clone(),
        }
    }
}

impl From<&rt::TypeDiagnostic> for NormalizedDiagnostic {
    fn from(d: &rt::TypeDiagnostic) -> Self {
        let severity = match d.severity {
            rt::TypeDiagnosticSeverity::Error => "error",
            rt::TypeDiagnosticSeverity::Warning => "warning",
            rt::TypeDiagnosticSeverity::Info => "info",
            rt::TypeDiagnosticSeverity::Hint => "hint",
        }
        .to_string();
        NormalizedDiagnostic {
            message: d.message.clone(),
            severity,
            start: d.start,
            end: d.end,
            code: d.code.clone(),
            tags: d.tags.iter().copied().map(Into::into).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(line: &str) -> Request {
        serde_json::from_str(line).expect("request should parse")
    }

    #[test]
    fn hello_request_decodes_exact_wire_shape() {
        let line = r#"{"type":"hello","workspaceRoot":"/ws","repoRoot":"/repo","provider":"tsserver","strictCi":true,"toolRoot":{"tsserverTsdk":"/repo/node_modules/typescript/lib","expectedTsserverJs":"/repo/node_modules/typescript/lib/tsserver.js","tsserverVersion":"5.7.2"}}"#;
        match parse(line) {
            Request::Hello(h) => {
                assert_eq!(h.workspace_root, "/ws");
                assert_eq!(h.repo_root, "/repo");
                assert_eq!(h.provider, ProviderName::Tsserver);
                assert!(h.strict_ci);
                assert_eq!(
                    h.tool_root.expected_tsserver_js.as_deref(),
                    Some("/repo/node_modules/typescript/lib/tsserver.js")
                );
                assert_eq!(h.tool_root.tsserver_version.as_deref(), Some("5.7.2"));
                // Negative: tsgoBin omitted decodes to None, not "".
                assert_eq!(h.tool_root.tsgo_bin, None);
            }
            other => panic!("expected hello, got {other:?}"),
        }
    }

    #[test]
    fn each_message_type_tag_is_exactly_as_specified() {
        assert!(matches!(
            parse(r#"{"type":"open","files":[],"version":1}"#),
            Request::Open(_)
        ));
        assert!(matches!(
            parse(r#"{"type":"syncArtifacts","uri":"file:///a.vue","version":2,"files":[]}"#),
            Request::SyncArtifacts(_)
        ));
        assert!(matches!(
            parse(
                r#"{"type":"query","method":"hover","uri":"file:///a.vue","path":"/a.vue.tsx","offset":10,"version":2}"#
            ),
            Request::Query(_)
        ));
        assert!(matches!(
            parse(
                r#"{"type":"diagnostics","uri":"file:///a.vue","path":"/a.vue.tsx","version":2}"#
            ),
            Request::Diagnostics(_)
        ));
        assert!(matches!(parse(r#"{"type":"shutdown"}"#), Request::Shutdown));
    }

    #[test]
    fn open_files_carry_role_and_camelcase_fields() {
        let line = r#"{"type":"open","version":3,"files":[{"path":"/a.vue.tsx","content":"x","role":"entry"},{"path":"/a.vue.ts","content":"y","role":"api"},{"path":"/vue.d.ts","content":"z","role":"support"}]}"#;
        match parse(line) {
            Request::Open(o) => {
                assert_eq!(o.version, 3);
                assert_eq!(o.files.len(), 3);
                assert_eq!(o.files[0].role, FileRole::Entry);
                assert_eq!(o.files[1].role, FileRole::Api);
                assert_eq!(o.files[2].role, FileRole::Support);
            }
            other => panic!("expected open, got {other:?}"),
        }
    }

    #[test]
    fn sync_artifacts_optional_fields_default() {
        // sourceMapIdentity + changedPublicApiTwins omitted.
        let line = r#"{"type":"syncArtifacts","uri":"file:///a.vue","version":5,"files":[]}"#;
        match parse(line) {
            Request::SyncArtifacts(s) => {
                assert_eq!(s.version, 5);
                assert_eq!(s.source_map_identity, None);
                assert!(s.changed_public_api_twins.is_empty());
            }
            other => panic!("expected syncArtifacts, got {other:?}"),
        }
    }

    #[test]
    fn query_trigger_character_is_optional() {
        let with = r#"{"type":"query","method":"completion","uri":"file:///a.vue","path":"/a.vue.tsx","offset":1,"version":1,"triggerCharacter":"."}"#;
        let without = r#"{"type":"query","method":"completion","uri":"file:///a.vue","path":"/a.vue.tsx","offset":1,"version":1}"#;
        match parse(with) {
            Request::Query(q) => assert_eq!(q.trigger_character.as_deref(), Some(".")),
            other => panic!("expected query, got {other:?}"),
        }
        match parse(without) {
            Request::Query(q) => assert_eq!(q.trigger_character, None),
            other => panic!("expected query, got {other:?}"),
        }
    }

    #[test]
    fn error_kinds_serialize_to_the_exact_design_strings() {
        let stale = Response::stale("file:///a.vue", 4, Some(2));
        let json = serde_json::to_string(&stale).unwrap();
        assert!(json.contains(r#""type":"error""#), "{json}");
        assert!(
            json.contains(r#""kind":"baseline_artifact_stale""#),
            "{json}"
        );
        assert!(json.contains(r#""requestedVersion":4"#), "{json}");
        assert!(json.contains(r#""haveVersion":2"#), "{json}");

        let mismatch = Response::error(ErrorKind::BaselineToolRootMismatch, "x");
        let json = serde_json::to_string(&mismatch).unwrap();
        assert!(
            json.contains(r#""kind":"baseline_tool_root_mismatch""#),
            "{json}"
        );

        let map_absent = Response::error(ErrorKind::CompiledCodeMapAbsent, "x");
        let json = serde_json::to_string(&map_absent).unwrap();
        assert!(
            json.contains(r#""kind":"compiled_code_map_absent""#),
            "{json}"
        );
    }

    #[test]
    fn hello_response_round_trips_and_uses_camelcase() {
        let resp = Response::Hello(HelloResponse {
            ok: true,
            provider: ProviderName::Tsgo,
            skipped: false,
            skip_reason: None,
            baseline_tool_root_used: Some("/path/to/tsgo".to_string()),
            capabilities: Some(ProviderCapabilities::for_provider(ProviderName::Tsgo)),
        });
        let json = serde_json::to_string(&resp).unwrap();
        assert!(
            json.contains(r#""baselineToolRootUsed":"/path/to/tsgo""#),
            "{json}"
        );
        // The seam advertises byte (utf-8) units for query.offset — never utf-16.
        assert!(json.contains(r#""positionEncoding":"utf-8""#), "{json}");
        assert!(!json.contains(r#""positionEncoding":"utf-16""#), "{json}");
        let back: Response = serde_json::from_str(&json).unwrap();
        assert_eq!(back, resp);
    }

    #[test]
    fn provider_capabilities_advertise_byte_offset_units_not_utf16() {
        // The `TypeProvider` seam is byte-offset (query.offset is a generated
        // byte offset); UTF-16 is the downstream tsgo/tsserver LSP wire only.
        for p in [ProviderName::Tsgo, ProviderName::Tsserver] {
            let caps = ProviderCapabilities::for_provider(p);
            assert_eq!(caps.position_encoding, "utf-8", "{p:?}");
            assert_ne!(caps.position_encoding, "utf-16", "{p:?}");
        }
    }

    #[test]
    fn sync_artifacts_changed_twins_carry_per_twin_version() {
        // A parent edit at v5 names a child twin whose OWN version is 1, not 5.
        let line = r#"{"type":"syncArtifacts","uri":"file:///Parent.vue","version":5,"files":[],"changedPublicApiTwins":[{"path":"/ws/Child.vue.ts","version":1}]}"#;
        match parse(line) {
            Request::SyncArtifacts(s) => {
                assert_eq!(s.version, 5);
                assert_eq!(s.changed_public_api_twins.len(), 1);
                assert_eq!(s.changed_public_api_twins[0].path, "/ws/Child.vue.ts");
                assert_eq!(s.changed_public_api_twins[0].version, 1);
                // Negative: the twin's version is NOT the edited document's version.
                assert_ne!(s.changed_public_api_twins[0].version, s.version);
            }
            other => panic!("expected syncArtifacts, got {other:?}"),
        }
    }

    #[test]
    fn query_requires_source_map_defaults_false_and_parses_true() {
        let without = r#"{"type":"query","method":"hover","uri":"file:///a.vue","path":"/a.vue.tsx","offset":1,"version":1}"#;
        match parse(without) {
            Request::Query(q) => assert!(
                !q.requires_source_map,
                "absent requiresSourceMap must default to false"
            ),
            other => panic!("expected query, got {other:?}"),
        }
        let with = r#"{"type":"query","method":"hover","uri":"file:///a.vue","path":"/a.vue.tsx","offset":1,"version":1,"requiresSourceMap":true}"#;
        match parse(with) {
            Request::Query(q) => assert!(q.requires_source_map),
            other => panic!("expected query, got {other:?}"),
        }
    }

    #[test]
    fn diagnostics_requires_source_map_defaults_false_and_parses_true() {
        let without =
            r#"{"type":"diagnostics","uri":"file:///a.vue","path":"/a.vue.tsx","version":2}"#;
        match parse(without) {
            Request::Diagnostics(d) => assert!(
                !d.requires_source_map,
                "absent requiresSourceMap must default to false"
            ),
            other => panic!("expected diagnostics, got {other:?}"),
        }
        let with = r#"{"type":"diagnostics","uri":"file:///a.vue","path":"/a.vue.tsx","version":2,"requiresSourceMap":true}"#;
        match parse(with) {
            Request::Diagnostics(d) => assert!(d.requires_source_map),
            other => panic!("expected diagnostics, got {other:?}"),
        }
    }

    #[test]
    fn map_absent_response_carries_kind_and_uri() {
        let r = Response::map_absent("file:///a.vue", 3);
        let json = serde_json::to_string(&r).unwrap();
        assert!(
            json.contains(r#""kind":"compiled_code_map_absent""#),
            "{json}"
        );
        assert!(json.contains(r#""uri":"file:///a.vue""#), "{json}");
        assert!(json.contains(r#""requestedVersion":3"#), "{json}");
    }

    #[test]
    fn query_result_kinds_are_tagged() {
        let hover = QueryResult::Hover {
            hover: Some(NormalizedHover {
                contents: "const x: string".to_string(),
                range_start: Some(1),
                range_end: Some(2),
            }),
        };
        let json = serde_json::to_string(&hover).unwrap();
        assert!(json.contains(r#""kind":"hover""#), "{json}");
        let completion = QueryResult::Completion {
            items: vec![],
            is_incomplete: false,
        };
        let json = serde_json::to_string(&completion).unwrap();
        assert!(json.contains(r#""kind":"completion""#), "{json}");
        assert!(json.contains(r#""isIncomplete":false"#), "{json}");
    }

    #[test]
    fn resolve_completion_request_round_trips() {
        // The runner sends back the item's `resolveData` handle (a TsserverEntry).
        let line = r#"{"type":"resolveCompletion","uri":"file:///a.vue","path":"/a.vue.tsx","version":3,"data":{"kind":"tsserver_entry","name":"computed","source":"vue","offset":42}}"#;
        match parse(line) {
            Request::ResolveCompletion(r) => {
                assert_eq!(r.uri, "file:///a.vue");
                assert_eq!(r.path, "/a.vue.tsx");
                assert_eq!(r.version, 3);
                match r.data {
                    rt::CompletionResolveData::TsserverEntry {
                        name,
                        source,
                        offset,
                        ..
                    } => {
                        assert_eq!(name, "computed");
                        assert_eq!(source.as_deref(), Some("vue"));
                        assert_eq!(offset, 42);
                    }
                    other => panic!("expected a TsserverEntry handle, got {other:?}"),
                }
            }
            other => panic!("expected resolveCompletion, got {other:?}"),
        }
    }

    #[test]
    fn resolve_completion_request_carries_lsp_handle() {
        // A TSGO list mints an `Lsp { label, data }` handle.
        let line = r#"{"type":"resolveCompletion","uri":"file:///a.vue","path":"/a.vue.tsx","version":1,"data":{"kind":"lsp","label":"computed","data":{"exportName":"computed"}}}"#;
        match parse(line) {
            Request::ResolveCompletion(r) => match r.data {
                rt::CompletionResolveData::Lsp { label, .. } => assert_eq!(label, "computed"),
                other => panic!("expected an Lsp handle, got {other:?}"),
            },
            other => panic!("expected resolveCompletion, got {other:?}"),
        }
    }

    #[test]
    fn resolve_completion_response_round_trips_and_uses_camelcase() {
        let resp = Response::ResolveCompletion(ResolveCompletionResponse {
            uri: "file:///a.vue".to_string(),
            version: 2,
            additional_text_edits: vec![NormalizedResolvedTextEdit {
                start: 0,
                end: 0,
                new_text: "import { computed } from 'vue'\n".to_string(),
            }],
            detail: Some("(alias) const computed".to_string()),
            documentation: None,
            capabilities: ProviderCapabilities::for_provider(ProviderName::Tsserver),
        });
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains(r#""type":"resolveCompletion""#), "{json}");
        assert!(json.contains(r#""additionalTextEdits""#), "{json}");
        assert!(
            json.contains(r#""newText":"import { computed } from 'vue'\n""#),
            "{json}"
        );
        let back: Response = serde_json::from_str(&json).unwrap();
        assert_eq!(back, resp);
    }

    /// T4: the `resolveData` handle a `completion` query carries on each item is
    /// the ACTUAL Rust-serialized `CompletionResolveData` — pin its bytes so a
    /// serde rename on the shared protocol type can't silently break the
    /// cross-process resolve round-trip (the TS bridge mirror is asserted against
    /// THIS shape in the dx-harness serde fixture test).
    #[test]
    fn normalized_completion_item_serializes_resolve_handle() {
        let item = NormalizedCompletionItem {
            label: "computed".to_string(),
            kind: Some("Function".to_string()),
            detail: None,
            insert_text: None,
            sort_text: Some("11".to_string()),
            resolve_data: Some(rt::CompletionResolveData::TsserverEntry {
                name: "computed".to_string(),
                source: Some("vue".to_string()),
                data: Some(serde_json::json!({ "exportName": "computed" })),
                offset: 42,
            }),
        };
        let json = serde_json::to_value(&item).unwrap();
        assert_eq!(json["label"], "computed");
        // The handle rides as `resolveData` (camelCase) with the snake_case kind tag.
        assert_eq!(json["resolveData"]["kind"], "tsserver_entry");
        assert_eq!(json["resolveData"]["name"], "computed");
        assert_eq!(json["resolveData"]["source"], "vue");
        assert_eq!(json["resolveData"]["offset"], 42);

        // An item with no handle omits `resolveData` entirely.
        let bare = NormalizedCompletionItem {
            label: "x".to_string(),
            kind: None,
            detail: None,
            insert_text: None,
            sort_text: None,
            resolve_data: None,
        };
        let json = serde_json::to_value(&bare).unwrap();
        assert!(
            json.get("resolveData").is_none(),
            "an item with no resolve handle omits resolveData: {json}"
        );
    }

    #[test]
    fn normalizers_map_provider_dtos() {
        let hov = rt::HoverInfo {
            contents: "string".to_string(),
            range_start: Some(3),
            range_end: Some(9),
        };
        let n: NormalizedHover = (&hov).into();
        assert_eq!(n.contents, "string");
        assert_eq!(n.range_start, Some(3));

        let diag = rt::TypeDiagnostic {
            message: "Cannot find name 'x'".to_string(),
            severity: rt::TypeDiagnosticSeverity::Error,
            start: 0,
            end: 1,
            code: Some("2304".to_string()),
            tags: Vec::new(),
            related_information: Vec::new(),
        };
        let nd: NormalizedDiagnostic = (&diag).into();
        assert_eq!(nd.severity, "error");
        assert_eq!(nd.code.as_deref(), Some("2304"));
        // A tagless diagnostic carries no tags.
        assert!(nd.tags.is_empty(), "a plain diagnostic carries no tags");
    }

    /// The baseline `NormalizedDiagnostic` must carry editor tags end-to-end (the
    /// gray-out / strikethrough contract the dx-harness guards). An unused-symbol
    /// `Unnecessary` tag and a `Deprecated` tag both cross the seam and serialize
    /// to their lowercase wire spellings.
    #[test]
    fn normalized_diagnostic_carries_editor_tags() {
        let unused = rt::TypeDiagnostic {
            message: "'msg' is declared but its value is never read.".to_string(),
            severity: rt::TypeDiagnosticSeverity::Hint,
            start: 6,
            end: 9,
            code: Some("6133".to_string()),
            tags: vec![rt::TypeDiagnosticTag::Unnecessary],
            related_information: Vec::new(),
        };
        let nd: NormalizedDiagnostic = (&unused).into();
        assert_eq!(
            nd.tags,
            vec![NormalizedDiagnosticTag::Unnecessary],
            "the Unnecessary tag must survive normalization, got: {:?}",
            nd.tags
        );

        // It serializes to the lowercase wire spelling the TS mirror reads.
        let json = serde_json::to_value(&nd).unwrap();
        assert_eq!(json["tags"], serde_json::json!(["unnecessary"]));

        // A multi-tag diagnostic (unused + deprecated) carries BOTH in order.
        let both = rt::TypeDiagnostic {
            message: "'oldUnused' is declared but its value is never read.".to_string(),
            severity: rt::TypeDiagnosticSeverity::Hint,
            start: 0,
            end: 9,
            code: Some("6133".to_string()),
            tags: vec![
                rt::TypeDiagnosticTag::Unnecessary,
                rt::TypeDiagnosticTag::Deprecated,
            ],
            related_information: Vec::new(),
        };
        let nd: NormalizedDiagnostic = (&both).into();
        assert_eq!(
            nd.tags,
            vec![
                NormalizedDiagnosticTag::Unnecessary,
                NormalizedDiagnosticTag::Deprecated
            ]
        );
        let json = serde_json::to_value(&nd).unwrap();
        assert_eq!(
            json["tags"],
            serde_json::json!(["unnecessary", "deprecated"])
        );

        // Round-trip back to the same value (the bridge re-reads its own frames).
        let back: NormalizedDiagnostic = serde_json::from_value(json).unwrap();
        assert_eq!(back, nd);
    }
}

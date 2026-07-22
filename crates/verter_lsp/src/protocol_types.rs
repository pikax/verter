use serde::{Deserialize, Serialize};
use tower_lsp_server::ls_types::*;

// ── Custom protocol types ──────────────────────────────────────────────

/// Server → client notification: TSGO child process started with given PID.
/// The extension tracks this PID to kill orphaned TSGO processes on restart.
pub enum TsgoStarted {}

impl tower_lsp_server::ls_types::notification::Notification for TsgoStarted {
    type Params = TsgoStartedParams;
    const METHOD: &'static str = "$/verter/tsgoStarted";
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TsgoStartedParams {
    pub pid: u32,
}

/// Server → client notification: type provider child process started.
/// Supersedes `TsgoStarted` — includes the provider kind so the extension
/// can track both TSGO and tsserver processes.
pub enum TypeProviderStarted {}

impl tower_lsp_server::ls_types::notification::Notification for TypeProviderStarted {
    type Params = TypeProviderStartedParams;
    const METHOD: &'static str = "$/verter/typeProviderStarted";
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TypeProviderStartedParams {
    pub pid: u32,
    pub kind: String,
}

/// Server → client heartbeat notification.
/// Sent every 5 seconds to let the extension detect if the server is frozen.
/// If the extension doesn't receive a heartbeat for 30 seconds, it restarts the server.
pub enum Heartbeat {}

impl tower_lsp_server::ls_types::notification::Notification for Heartbeat {
    type Params = HeartbeatParams;
    const METHOD: &'static str = "$/verter/heartbeat";
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HeartbeatParams {
    pub timestamp: u64,
}

/// Server → client notification: LEVEL 1 of the two-level readiness ladder —
/// background initialization is complete and the server answers requests.
///
/// Guaranteed at this point: the project registry is built, the type provider is
/// spawned and configured, and every open document has had diagnostics published.
/// The extension uses this to re-request diagnostics for open docs.
///
/// NOT guaranteed at this point: the workspace scan. It is still running, so
/// cross-file results (barrel re-exports, imported carrier surfaces, project-wide
/// references) may be incomplete for files the scan has not reached. A client that
/// needs project-wide completeness waits for LEVEL 2,
/// [`TypeProviderSyncComplete`], which is never emitted before this notification
/// for the same generation.
pub enum VerterReady {}

impl tower_lsp_server::ls_types::notification::Notification for VerterReady {
    type Params = VerterReadyParams;
    const METHOD: &'static str = "$/verter/ready";
}

#[derive(Debug, Serialize, Deserialize)]
pub struct VerterReadyParams {
    pub gen: u64,
}

/// Server → client notification: LEVEL 2 of the two-level readiness ladder — the
/// workspace scanner has finished syncing all files to the type provider.
/// Cross-file type resolution (barrel re-exports, imported types) is now reliable.
///
/// Ordered strictly after [`VerterReady`] for the same generation, so the ladder a
/// client observes is monotone: a fast scan on a small workspace can finish before
/// background init reaches its ready point, and emitting level 2 first would let a
/// client conclude the project had gone stale again.
pub enum TypeProviderSyncComplete {}

impl tower_lsp_server::ls_types::notification::Notification for TypeProviderSyncComplete {
    type Params = TypeProviderSyncCompleteParams;
    const METHOD: &'static str = "$/verter/typeProviderSyncComplete";
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TypeProviderSyncCompleteParams {
    pub gen: u64,
}

/// Server → client notification: MCP HTTP server is ready.
/// Sent during `initialized()` with the actual bound port (may differ from requested
/// when port 0 is used for OS-assigned dynamic ports).
pub enum McpReady {}

impl tower_lsp_server::ls_types::notification::Notification for McpReady {
    type Params = McpReadyParams;
    const METHOD: &'static str = "$/verter/mcpReady";
}

#[derive(Debug, Serialize, Deserialize)]
pub struct McpReadyParams {
    pub port: u16,
}

/// Server → client notification: a Vite config requires trust for execution.
/// Sent from `background_init()` when static analysis cannot handle a config and
/// the file is not in `trustedFiles`. The extension shows a prompt to the user.
pub enum ViteConfigTrustRequired {}

impl tower_lsp_server::ls_types::notification::Notification for ViteConfigTrustRequired {
    type Params = ViteConfigTrustRequiredParams;
    const METHOD: &'static str = "$/verter/viteConfigTrustRequired";
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ViteConfigTrustRequiredParams {
    pub config_path: String,
    pub workspace_root: String,
    pub reason: String,
}

/// Server → client notification: type provider status.
/// Sent during `initialized()` to inform the extension which type provider is active
/// (or that none could be started, with a reason).
pub enum TypeProviderStatus {}

impl tower_lsp_server::ls_types::notification::Notification for TypeProviderStatus {
    type Params = TypeProviderStatusParams;
    const METHOD: &'static str = "$/verter/typeProviderStatus";
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TypeProviderStatusParams {
    /// Which type provider FAMILY is active: "tsgo", "tsserver",
    /// "editor-tsserver", or "none". Two topologies share the "tsgo" family, so
    /// this alone cannot identify the serving engine — see `topology`.
    pub kind: String,
    /// WHICH engine is serving and who owns it: "shared-tsgo", "managed-tsgo",
    /// "workspace-tsserver", "editor-tsserver", "extension-hosted", or "none".
    ///
    /// The status surface must name the topology, not the family: a working
    /// attach to the editor's own tsgo and a second Verter-spawned tsgo both
    /// reported "tsgo", which made a serving tier indistinguishable from a
    /// broken one. Defaulted so an older client that never sends it still
    /// deserializes.
    #[serde(default)]
    pub topology: String,
    /// Stable provenance for the selected route, or the failure reason for "none".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Present when a different provider is recommended over the active one
    /// (tsgo-preferred model: tsserver-family serving recommends TSGO). The
    /// DECISION is server-owned portable facts; PRESENTATION (notification
    /// style, dismissal persistence, settings affordances) is client-owned.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommendation: Option<ProviderRecommendation>,
}

/// Structured provider recommendation riding `$/verter/typeProviderStatus`.
///
/// Content is editor-agnostic: no client settings keys, no editor-product
/// names — each client renders the facts in its own idiom. Mirrors the
/// `recommendation` field in `packages/language-shared/src/notifications.ts`.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderRecommendation {
    /// The recommended provider kind (currently always "tsgo").
    pub preferred: String,
    /// Portable, human-readable rationale naming the active route.
    pub reason: String,
    /// Honest, tree-evidenced capability gaps of the recommended provider —
    /// never marketing over evidence.
    pub known_gaps: Vec<String>,
}

/// Server → client notification: the resolved per-workspace carrier-store
/// directory the LSP publishes compiled `.vue`/`.svelte` carriers into.
///
/// The extension forwards this dir to VS Code's OWN TypeScript server via
/// `configurePlugin`, so a plain `.ts` opened in VS Code (served by VS Code's TS
/// service, not the LSP-spawned tsserver) reads the same store and gets real types
/// for imported carriers. The LSP is the single source of the
/// `<temp>/verter-carrier-store/<host-version>/<workspace-hash>/` path derivation,
/// which the extension cannot reproduce without mirroring that exact recipe. Mirrors
/// `$/verter/carrierStoreReady` in `packages/language-shared/src/notifications.ts`.
pub enum CarrierStoreReady {}

impl tower_lsp_server::ls_types::notification::Notification for CarrierStoreReady {
    type Params = CarrierStoreReadyParams;
    const METHOD: &'static str = "$/verter/carrierStoreReady";
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CarrierStoreReadyParams {
    /// The absolute, forward-slash-normalized per-workspace carrier-store dir the
    /// LSP publishes carriers into (and the dir the `@verter/typescript-plugin`
    /// reads). Identical to the dir the LSP delivers to its own spawned tsserver
    /// through `VERTER_CARRIER_STORE_DIR`.
    pub carrier_store_dir: String,
}

/// Server → client request: forward a TypeScript query to the extension's
/// in-process `ts.createLanguageService()`. Uses tsserver command format so
/// existing response parsers work unchanged.
pub enum TsQuery {}

impl tower_lsp_server::ls_types::request::Request for TsQuery {
    type Params = TsQueryParams;
    type Result = serde_json::Value;
    const METHOD: &'static str = "$/verter/tsQuery";
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TsQueryParams {
    pub command: String,
    pub arguments: serde_json::Value,
}

/// Params for `$/onDidChangeTsOrJsFile` notification.
#[derive(Debug, Deserialize)]
pub struct OnDidChangeTsOrJsFileParams {
    pub uri: String,
    pub changes: Vec<TextChangeEvent>,
}

#[derive(Debug, Deserialize)]
pub struct TextChangeEvent {
    pub text: String,
    pub range: TextChangeRange,
}

#[derive(Debug, Deserialize)]
pub struct TextChangeRange {
    pub start: TextChangePosition,
    pub end: TextChangePosition,
}

#[derive(Debug, Deserialize)]
pub struct TextChangePosition {
    pub line: u32,
    pub character: u32,
}

/// Params for `$/onFileChanged` notification.
#[derive(Debug, Deserialize)]
pub struct OnFileChangedParams {
    pub uri: String,
    #[serde(rename = "type")]
    pub change_type: String,
}

/// Params for `$/verter/watcherStateChanged` notification.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WatcherStateChangedParams {
    pub workspace_root: String,
    pub reason: String,
}

/// Params for `$/getCompiledCode` request.
#[derive(Debug, Deserialize)]
pub struct GetCompiledCodeParams {
    pub uri: String,
}

/// Response for `$/getCompiledCode` request.
#[derive(Debug, Serialize)]
pub struct CompiledCodeResponse {
    pub js: CompiledBlock,
    pub css: CompiledBlock,
    pub wasm: CompiledBlock,
}

#[derive(Debug, Serialize)]
pub struct CompiledBlock {
    pub code: String,
    pub map: Option<String>,
}

/// Params for `$/verter/getVirtualFiles` request.
#[derive(Debug, Deserialize)]
pub struct GetVirtualFilesParams {
    pub uri: String,
}

/// Params for `$/verter/applyStyleOverrides` request.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyStyleOverridesParams {
    pub uri: String,
    pub overrides: Vec<StyleOverrideParam>,
}

/// A single style override entry from the client.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StyleOverrideParam {
    pub index: u32,
    pub code: String,
    pub source_map: Option<String>,
}

/// Response for `$/verter/applyStyleOverrides` request.
#[derive(Debug, Serialize)]
pub struct ApplyStyleOverridesResponse {
    pub success: bool,
}

/// Params for `$/verter/getAnalysis` (and `$/verter/getBindingTypes`) request.
#[derive(Debug, Deserialize)]
pub struct GetAnalysisParams {
    pub uri: String,
}

/// Params for `$/verter/getComponentParents` request.
#[derive(Debug, Deserialize)]
pub struct GetComponentParentsParams {
    pub uri: String,
}

/// A single parent file that uses a component.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentParentInfo {
    pub file_path: String,
    pub component_name: String,
    pub props: Vec<serde_json::Value>,
    pub slots_used: Vec<String>,
}

/// Response for `$/verter/getComponentParents` request.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentParentsResponse {
    pub component_path: String,
    pub parents: Vec<ComponentParentInfo>,
}

/// A single virtual file entry in the response.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VirtualFileEntry {
    pub kind: String,
    pub code: String,
    pub lang: String,
    pub source_map: Option<String>,
    pub stale: bool,
}

/// Generated code block in the virtual files response.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeBlock {
    pub code: String,
    pub source_map: Option<String>,
    /// `true` when the SFC script is JavaScript (JSX/JSDoc) rather than TypeScript (TSX).
    pub is_js: bool,
}

/// Response for `$/verter/getVirtualFiles` request.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VirtualFilesResponse {
    pub ide: Option<CodeBlock>,
    pub api: Option<CodeBlock>,
    pub virtual_files: Vec<VirtualFileEntry>,
}

/// Params for `$/verter/documentDropEdit` request.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentDropEditParams {
    pub text_document: TextDocumentIdentifier,
    pub position: Position,
    pub dropped_uri: String,
}

/// Params for `$/verter/getStatistics` request.
#[derive(Debug, Deserialize)]
pub struct StatisticsRequestParams {
    #[serde(default)]
    pub include_events: bool,
    pub scope: Option<String>,
}

/// Response for `$/verter/getStatistics` request.
#[derive(Debug, Serialize)]
pub struct StatisticsSnapshot {
    pub enabled: bool,
    pub session: StatisticsSession,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatisticsSession {
    pub by_type: serde_json::Map<String, serde_json::Value>,
    pub by_file: serde_json::Map<String, serde_json::Value>,
}

/// Response for `$/verter/getProjectOverview` request.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectOverviewResponse {
    pub files: Vec<ProjectOverviewFile>,
    pub component_graph: Vec<ProjectOverviewComponentEdge>,
    pub stats: ProjectOverviewStats,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectOverviewFile {
    pub path: String,
    pub kind: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectOverviewComponentEdge {
    pub file: String,
    pub uses_components: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectOverviewStats {
    /// Count of framework CARRIER component files (`.vue`, `.svelte`, …).
    pub total_component_files: usize,
    pub total_components: usize,
    pub total_provide_keys: usize,
    pub total_inject_keys: usize,
    pub files_with_scoped_styles: usize,
}

/// Cached verter diagnostic entry: (document_version, diagnostics_generation, diagnostics).
/// The `diagnostics_generation` comes from `VerterHost::get_diagnostics_generation()` and
/// detects host-driven recompiles (e.g., dependency hydration) without a document version change.
pub(crate) type CachedVerterDiagEntry = (i32, u64, Vec<Diagnostic>);

// ─────────────────────────────────────────────────────────────────
// Component-meta selective API (D32 / D102 / D104 / D113)
// ─────────────────────────────────────────────────────────────────

/// Params for `$/verter/getComponentMeta` request — full Volar-shape payload.
#[derive(Debug, Deserialize)]
pub struct GetComponentMetaParams {
    /// Document URI of the Vue SFC whose component metadata is being requested.
    pub uri: String,
}

/// Params for `$/verter/getComponentMetaSurface` request — selective surface
/// envelope (D102).
#[derive(Debug, Deserialize)]
pub struct GetComponentMetaSurfaceParams {
    pub uri: String,
}

/// Params for `$/verter/getComponentMetaTypeExpansion` request — one-layer
/// `TypeHandle` resolution (D104).
///
/// `handle_bytes` carries the protobuf-encoded `TypeHandle` (D100 wire format).
/// `depth` is accepted and forwarded but currently ignored — the resolver
/// always performs a one-layer expansion.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetComponentMetaTypeExpansionParams {
    pub handle_bytes: Vec<u8>,
    #[serde(default)]
    pub depth: Option<u32>,
}

/// Response envelope for `$/verter/getComponentMetaTypeExpansion`.
///
/// `expansion_bytes` is the protobuf-encoded `TypeExpansion` on success.
/// `error` carries the structured `TypeHandleError` on failure (D104 + D114).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetComponentMetaTypeExpansionResponse {
    /// Encoded `TypeExpansion` proto bytes. Empty on error.
    pub expansion_bytes: Vec<u8>,
    /// Structured handle error (e.g. `projectMismatch`, `staleHandle`).
    /// `None` on success.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<TypeHandleErrorPayload>,
}

/// JSON-side projection of `TypeHandleError`. The wire form keeps a discriminator
/// `kind` to mirror the proto union shape so the TS-side switch can stay typed.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum TypeHandleErrorPayload {
    ProjectMismatch { expected: String, actual: String },
    StaleHandle { reason: String },
    DepthExceeded { cap: u32 },
    Other { message: String },
}

/// Params for `$/verter/audit/getRecord` request.
///
/// `request_id` is encoded as a string because JSON cannot losslessly
/// round-trip 64-bit integers in JavaScript clients. Producers stringify
/// the `u64` request id; the handler parses it back via
/// [`u64::from_str`].
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetAuditRecordParams {
    /// String-encoded `u64` request id.
    pub request_id: String,
}

/// Params for `$/verter/audit/getRecent` request.
///
/// Both fields are optional. `kind` filters records by `RequestKind`
/// variant tag (e.g. `"Lsp"`, `"ComponentMeta"`, `"Compile"`); the
/// matcher is [`verter_audit::RequestKind::matches_filter`]. `limit`
/// caps the number of returned records (default 50, capped at 1024
/// to keep the response payload bounded).
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetAuditRecentParams {
    /// Optional kind filter — variant-name string match.
    pub kind: Option<String>,
    /// Optional cap on returned records.
    pub limit: Option<u32>,
}

#[cfg(test)]
mod component_meta_protocol_tests {
    //! D113 / Tier 5b W7 review: JSON round-trip checks for the three new
    //! component-meta protocol types. These tests guard the wire format
    //! shared with `packages/language-shared/src/request.ts` (the TS LSP
    //! client-side bindings) — any field rename or shape drift breaks JSON
    //! interop with the VS Code extension.

    use super::*;
    use serde_json::json;

    #[test]
    fn get_component_meta_params_decodes_camel_case_uri() {
        let p: GetComponentMetaParams = serde_json::from_value(json!({
            "uri": "file:///test/Comp.vue"
        }))
        .expect("camelCase uri must decode");
        assert_eq!(p.uri, "file:///test/Comp.vue");
    }

    #[test]
    fn get_component_meta_surface_params_decodes_camel_case_uri() {
        let p: GetComponentMetaSurfaceParams = serde_json::from_value(json!({
            "uri": "file:///test/Comp.vue"
        }))
        .expect("camelCase uri must decode");
        assert_eq!(p.uri, "file:///test/Comp.vue");
    }

    #[test]
    fn get_component_meta_type_expansion_params_uses_camel_case_handle_bytes() {
        let p: GetComponentMetaTypeExpansionParams = serde_json::from_value(json!({
            "handleBytes": [10, 0, 18, 9, 47, 116, 101, 115, 116, 46, 118, 117, 101],
            "depth": 2
        }))
        .expect("camelCase handleBytes + depth must decode");
        assert_eq!(p.handle_bytes.len(), 13);
        assert_eq!(p.depth, Some(2));
    }

    #[test]
    fn get_component_meta_type_expansion_params_depth_optional() {
        let p: GetComponentMetaTypeExpansionParams =
            serde_json::from_value(json!({ "handleBytes": [] })).expect("depth must be optional");
        assert!(p.depth.is_none());
    }

    #[test]
    fn type_expansion_response_serializes_with_camel_case_and_skips_none_error() {
        let response = GetComponentMetaTypeExpansionResponse {
            expansion_bytes: vec![1, 2, 3],
            error: None,
        };
        let json = serde_json::to_value(&response).expect("must serialize");
        // expansionBytes camelCase
        assert_eq!(json["expansionBytes"], json!([1, 2, 3]));
        // error skipped on None
        assert!(
            json.get("error").is_none(),
            "error: None must be skipped from JSON"
        );
    }

    #[test]
    fn type_expansion_response_error_project_mismatch_serializes_with_kind_discriminator() {
        let response = GetComponentMetaTypeExpansionResponse {
            expansion_bytes: vec![],
            error: Some(TypeHandleErrorPayload::ProjectMismatch {
                expected: String::new(),
                actual: "foreign-project".to_string(),
            }),
        };
        let json = serde_json::to_value(&response).expect("must serialize");
        let err = &json["error"];
        assert_eq!(err["kind"], "projectMismatch");
        assert_eq!(err["actual"], "foreign-project");
    }

    #[test]
    fn type_expansion_response_error_stale_handle_serializes_with_kind_discriminator() {
        let response = GetComponentMetaTypeExpansionResponse {
            expansion_bytes: vec![],
            error: Some(TypeHandleErrorPayload::StaleHandle {
                reason: "FileDeleted".to_string(),
            }),
        };
        let json = serde_json::to_value(&response).expect("must serialize");
        let err = &json["error"];
        assert_eq!(err["kind"], "staleHandle");
        assert_eq!(err["reason"], "FileDeleted");
    }
}

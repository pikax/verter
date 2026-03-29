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

/// Server → client notification: background initialization complete.
/// Sent after project registry, workspace scanner, and type provider are ready.
/// The extension uses this to re-request diagnostics for open docs.
pub enum VerterReady {}

impl tower_lsp_server::ls_types::notification::Notification for VerterReady {
    type Params = VerterReadyParams;
    const METHOD: &'static str = "$/verter/ready";
}

#[derive(Debug, Serialize, Deserialize)]
pub struct VerterReadyParams {
    pub gen: u64,
}

/// Server → client notification: workspace scanner has finished syncing all files
/// (Phase 1 `.vue` + Phase 2 non-Vue source) to the type provider.
/// Cross-file type resolution (barrel re-exports, imported types) is now reliable.
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
    /// Which type provider is active: "tsgo", "tsserver", or "none".
    pub kind: String,
    /// Why no type provider is available (only set when kind is "none").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
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
    pub total_vue_files: usize,
    pub total_components: usize,
    pub total_provide_keys: usize,
    pub total_inject_keys: usize,
    pub files_with_scoped_styles: usize,
}

/// Cached verter diagnostic entry: (document_version, diagnostics_generation, diagnostics).
/// The `diagnostics_generation` comes from `VerterHost::get_diagnostics_generation()` and
/// detects host-driven recompiles (e.g., dependency hydration) without a document version change.
pub(crate) type CachedVerterDiagEntry = (i32, u64, Vec<Diagnostic>);

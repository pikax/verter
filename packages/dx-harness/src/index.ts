/**
 * `@verter/dx-harness` — the immutable {@link MaterializedWorkspace} scaffold plus
 * the TypeScript orchestration that drives the `verter_dx_baseline` differential
 * baseline.
 *
 * B owns the Verter-facing scaffold (fixture copy + test-anchor strip,
 * deterministic tool roots, the committed vendored Vue shims, workspace settings)
 * and the bridge/materialize clients; C (`crates/verter_dx_baseline`) owns ALL
 * baseline materialization. B CALLS C and reads its emitted source maps as
 * authoritative — it never duplicates C's compilation, twin generation, specifier
 * rewriting, or source-map shifting.
 */

export { canonicalizePath, joinCanonical, offsetToLineChar, type LineChar } from "./paths.js";

export {
  AnchorError,
  addFileAnchors,
  requireAnchor,
  stripAnchors,
  type Anchor,
  type AnchorEncoding,
  type AnchorMap,
  type AnchorPosition,
  type StripResult,
} from "./anchors.js";

export {
  readTypescriptVersionFromDisk,
  resolveToolRoots,
  type ResolveToolRootsOptions,
  type ToolRoots,
} from "./toolRoots.js";

export {
  DX_HARNESS_WORKSPACE_ENV,
  writeWorkspaceSettings,
  type WorkspaceSettings,
  type WriteWorkspaceSettingsOptions,
} from "./workspaceSettings.js";

export {
  VENDORED_VUE_VERSION,
  buildVendorManifest,
  collectVuePackageVersions,
  computeExpectedVueVersion,
  sha256Hex,
  vendorShimsDir,
  type VendorFile,
  type VendorManifest,
  type VuePackageVersion,
} from "./vendorManifest.js";

export { runOneShot, type OneShotOptions, type OneShotResult } from "./baseline/childProcess.js";

export {
  buildMaterializeRequest,
  parseMaterializeResult,
  runMaterialize,
  type MaterializeArtifact,
  type MaterializeCompileError,
  type MaterializeRequestInput,
  type MaterializeResult,
  type MaterializeWireRequest,
  type RunMaterializeOptions,
  type VueVersionWarning,
} from "./baseline/materializeClient.js";

export {
  BridgeClient,
  NewlineFramer,
  decodeResponse,
  diagnosticsFrame,
  encodeRequest,
  helloFrame,
  openFrame,
  queryFrame,
  shutdownFrame,
  syncArtifactsFrame,
  type AppliedSync,
  type BaselineFile,
  type BridgeClientOptions,
  type BridgeRequest,
  type BridgeResponse,
  type ChangedTwin,
  type DiagnosticsInput,
  type DiagnosticsRequest,
  type DiagnosticsResponse,
  type ErrorKind,
  type ErrorResponse,
  type FileRole,
  type HelloRequest,
  type HelloResponse,
  type NormalizedCompletionItem,
  type NormalizedDiagnostic,
  type NormalizedHover,
  type NormalizedLocation,
  type OpenRequest,
  type OpenResponse,
  type ProviderCapabilities,
  type ProviderName,
  type QueryInput,
  type QueryMethod,
  type QueryRequest,
  type QueryResponse,
  type QueryResult,
  type ShutdownRequest,
  type ShutdownResponse,
  type SyncAction,
  type SyncArtifactsInput,
  type SyncArtifactsRequest,
  type SyncArtifactsResponse,
  type ToolRootWire,
} from "./baseline/bridgeClient.js";

export {
  createMaterializedWorkspace,
  disposeMaterializedWorkspace,
  type CreateMaterializedWorkspaceOptions,
  type MaterializedWorkspace,
  type MaterializeRunner,
  type TsconfigSet,
  type VendorReference,
} from "./materializedWorkspace.js";

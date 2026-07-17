/**
 * `@verter/lsp-test-client` — a side-effect-free JSON-RPC LSP stdio client for
 * test and DX harnesses.
 *
 * It drives a real language-server child process over Content-Length-framed
 * JSON-RPC on stdio. Importing this module performs no I/O, reads no config,
 * and never mutates global `console` — it is a pure class plus helpers.
 *
 * Surface:
 *  - {@link LspClient} — spawn, request/notification transport, notification
 *    waiting, buffered stderr, position-encoding negotiation, clean teardown.
 *  - {@link StderrBuffer} — buffered, line-addressable child stderr.
 *  - {@link DocumentPositions} and the position helpers — encoding-aware
 *    conversions between source positions, UTF-8 byte offsets, and LSP
 *    positions in the negotiated `positionEncoding`.
 */
export { LspClient, type LspClientOptions } from "./lspClient.js";
export { StderrBuffer, type StderrBufferOptions } from "./stderrBuffer.js";
export {
  DocumentPositions,
  DEFAULT_POSITION_ENCODING,
  adoptServerEncoding,
  defaultClientPositionEncodings,
  isPositionEncoding,
  withPositionEncodings,
  byteOffsetToPosition,
  positionToByteOffset,
  type PositionEncoding,
  type LspPosition,
  type InitializeParamsLike,
} from "./positionEncoding.js";
export {
  createEditorNeutralContractInventory,
  executeEditorNeutralContractCase,
  resolveContractAnchor,
  type ContractAnchor,
  type EditorNeutralContractCase,
  type EditorNeutralContractDriver,
  type EditorNeutralContractFeature,
  type EditorNeutralContractSurface,
  type EditorNeutralFramework,
  type EditorNeutralProviderRoute,
  type EditorNeutralScriptLanguage,
  type LspCompletionItem,
  type LspCompletionList,
  type LspDiagnostic,
  type LspLocation,
  type LspRange,
  type LspTextDocumentEdit,
  type LspTextEdit,
  type LspWorkspaceEdit,
  type ProviderAttestation,
  type ProviderTopologyAttestation,
} from "./contracts/editorNeutral.js";

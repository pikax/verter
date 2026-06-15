/**
 * Pure LSP-response normalizers: fold raw provider responses into canonical,
 * comparable `Canonical*` forms (LSP line/character coordinates) for the
 * differential and collector layers. Every function is pure, total over
 * `null`/`undefined`/empty, and deterministic.
 *
 * These verter-side forms are distinct from the `verter_dx_baseline` bridge's
 * byte-offset `Normalized*` wire shapes ({@link ../baseline/bridgeClient}); the
 * differential reconciles the two.
 */

// The raw-LSP response-input unions (`CompletionResponse` / `HoverResponse` /
// `DefinitionResponse` / `DiagnosticsResponse`) are the normalizers' INTERNAL input
// shapes and are deliberately NOT re-exported: the public surface is the element
// shapes below plus the `Canonical*` outputs and `normalize*` functions. Keeping the
// response unions private also avoids colliding with the bridge's object-form
// `DiagnosticsResponse` ({@link ../baseline/bridgeClient}) at the package root.
export type {
  CompletionEdit,
  CompletionItem,
  CompletionList,
  Diagnostic,
  Hover,
  InsertReplaceEdit,
  Location,
  LocationLink,
  MarkedString,
  MarkupContent,
  Position,
  Range,
  TextEdit,
} from "./lspTypes.js";

export {
  coerceRange,
  completionItemKindName,
  diagnosticSeverityName,
  isGeneratedUri,
  normalizeEol,
  positionsEqual,
  rangesEqual,
  stableStringify,
} from "./shared.js";

export {
  normalizeCompletion,
  type CanonicalCompletionItem,
  type CanonicalCompletionList,
} from "./completion.js";

export { normalizeHover, type CanonicalHover } from "./hover.js";

export {
  definitionMatchesExpected,
  isDefinitionGeneratedOnly,
  isUnmappedGeneratedOnly,
  normalizeDefinition,
  type CanonicalDefinitionTarget,
  type ExpectedDefinition,
} from "./definition.js";

export {
  isDefaultDiagnosticRange,
  isImpossibleDefaultDiagnostic,
  normalizeDiagnostics,
  type CanonicalDiagnostic,
} from "./diagnostics.js";

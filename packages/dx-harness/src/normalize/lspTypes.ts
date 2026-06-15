/**
 * Minimal STRUCTURAL views of the LSP 3.17 response shapes the normalizers
 * consume.
 *
 * These are deliberately defined here, not imported from
 * `vscode-languageserver-protocol`: that package is not a dependency of this
 * harness (and `@verter/lsp-test-client` types its responses as `any`), so a
 * local structural definition keeps the normalizers hermetic and dependency-free.
 * A real provider response (an `any` from the raw-LSP client) satisfies these
 * interfaces by structure. Each shape mirrors the corresponding
 * `vscode-languageserver-protocol` 3.17 type; only the fields the normalizers read
 * are modelled.
 *
 * Positions are LSP `{ line, character }` (0-based; UTF-16 columns under the
 * default encoding) — NOT byte offsets. The `verter_dx_baseline` bridge emits its
 * own byte-offset `Normalized*` wire shapes on the Rust side; these `Canonical*`
 * forms are the verter-side raw-LSP representation, and the line-0 / `(0,0)`
 * predicates operate on exactly these line/character coordinates.
 */

/** An LSP `Position`: 0-based line and UTF-16 code-unit column. */
export interface Position {
  readonly line: number;
  readonly character: number;
}

/** An LSP `Range`: an inclusive start, exclusive end. */
export interface Range {
  readonly start: Position;
  readonly end: Position;
}

/** An LSP `TextEdit`: replace `range` with `newText`. */
export interface TextEdit {
  readonly range: Range;
  readonly newText: string;
}

/** An LSP `InsertReplaceEdit`: a completion edit carrying both insert and replace ranges. */
export interface InsertReplaceEdit {
  readonly newText: string;
  readonly insert: Range;
  readonly replace: Range;
}

/** The `textEdit` form on a completion item — either shape. */
export type CompletionEdit = TextEdit | InsertReplaceEdit;

/** An LSP `MarkupContent` hover/detail body. */
export interface MarkupContent {
  readonly kind: "plaintext" | "markdown";
  readonly value: string;
}

/** A deprecated LSP `MarkedString`: a bare string or a `{ language, value }` code block. */
export type MarkedString = string | { readonly language: string; readonly value: string };

/** An LSP `CompletionItem`. Only the normalizer-relevant fields are modelled. */
export interface CompletionItem {
  readonly label: string;
  /** The numeric `CompletionItemKind` (1-25); normalized to a name string. */
  readonly kind?: number;
  readonly detail?: string;
  readonly insertText?: string;
  readonly sortText?: string;
  readonly filterText?: string;
  readonly textEdit?: CompletionEdit;
  readonly additionalTextEdits?: readonly TextEdit[];
  readonly data?: unknown;
}

/** An LSP `CompletionList`. */
export interface CompletionList {
  readonly isIncomplete: boolean;
  readonly items: readonly CompletionItem[];
}

/** A raw `textDocument/completion` response. */
export type CompletionResponse = CompletionList | readonly CompletionItem[] | null | undefined;

/** An LSP `Hover`. `contents` may be `MarkupContent`, a `MarkedString`, or an array. */
export interface Hover {
  readonly contents: MarkupContent | MarkedString | readonly MarkedString[];
  readonly range?: Range;
}

/** A raw `textDocument/hover` response. */
export type HoverResponse = Hover | null | undefined;

/** An LSP `Location`. */
export interface Location {
  readonly uri: string;
  readonly range: Range;
}

/** An LSP `LocationLink`. */
export interface LocationLink {
  readonly targetUri: string;
  readonly targetRange: Range;
  readonly targetSelectionRange: Range;
  readonly originSelectionRange?: Range;
}

/** A raw `textDocument/definition` response (all LSP-permitted shapes). */
export type DefinitionResponse =
  | Location
  | readonly Location[]
  | LocationLink
  | readonly LocationLink[]
  | null
  | undefined;

/** An LSP `Diagnostic`. Only the normalizer-relevant fields are modelled. */
export interface Diagnostic {
  readonly range: Range;
  /** The numeric `DiagnosticSeverity` (1=Error … 4=Hint); normalized to a name string. */
  readonly severity?: number;
  readonly code?: string | number;
  readonly source?: string;
  readonly message: string;
  readonly tags?: readonly number[];
}

/** A raw `textDocument/publishDiagnostics` diagnostics array. */
export type DiagnosticsResponse = readonly Diagnostic[] | null | undefined;

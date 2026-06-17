/**
 * Shared LIVE-path machinery for the raw-LSP collectors: the narrow client
 * interface the real `@verter/lsp-test-client` `LspClient` structurally satisfies,
 * plus the document-open / incremental-`didChange` helpers that translate an
 * encoding-agnostic {@link Tick} into LSP notifications in the server's negotiated
 * position encoding.
 *
 * The offset→position conversion reuses the `@verter/lsp-test-client`
 * `DocumentPositions` seam (surrogate-pair and CRLF correct) rather than a second
 * offset walker. These helpers are exercised by the env-gated integration suite; the
 * collectors' decision logic lives in their pure classifiers, tested without a server.
 */

import { DocumentPositions, type PositionEncoding } from "@verter/lsp-test-client";

import type { Position, Range } from "../normalize/index.js";
import type { Tick } from "./editLoop.js";

/**
 * The slice of the `@verter/lsp-test-client` `LspClient` the collectors drive: the
 * negotiated encoding, the server capabilities (read for advertised completion
 * triggers, never hardcoded), request/notification transport, the per-method
 * notification subscription (diagnostics), and the buffered child stderr (logs).
 */
export interface CollectorLspClient {
  readonly positionEncoding: PositionEncoding;
  readonly serverCapabilities: unknown;
  sendRequest<T = unknown>(method: string, params?: unknown, timeout?: number): Promise<T>;
  sendNotification(method: string, params?: unknown): void;
  onNotification(method: string, handler: (params: any) => void): void;
  offNotification(method: string, handler: (params: any) => void): void;
  readonly stderr: { text(): string };
}

/** A UTF-16 offset over `text` → an LSP position in the negotiated `encoding`. */
export function offsetToPosition(
  text: string,
  offset: number,
  encoding: PositionEncoding,
): Position {
  const pos = new DocumentPositions(text).utf16ToPosition(offset, encoding);
  return { line: pos.line, character: pos.character };
}

/** Open a text document on the server (`textDocument/didOpen`). */
export function openDocument(
  client: CollectorLspClient,
  uri: string,
  languageId: string,
  text: string,
  version = 1,
): void {
  client.sendNotification("textDocument/didOpen", {
    textDocument: { uri, languageId, version, text },
  });
}

/** Close a text document on the server (`textDocument/didClose`). */
export function closeDocument(client: CollectorLspClient, uri: string): void {
  client.sendNotification("textDocument/didClose", { textDocument: { uri } });
}

/**
 * Send one tick's incremental `didChange`. The change range is measured against the
 * tick's PRE-change text in the server's negotiated encoding, so a UTF-8-negotiating
 * server (verter-lsp can select UTF-8) receives byte columns, not UTF-16 columns.
 */
export function sendTickChange(client: CollectorLspClient, uri: string, tick: Tick): void {
  const doc = new DocumentPositions(tick.previousText);
  const start = doc.utf16ToPosition(tick.change.startOffset, client.positionEncoding);
  const end = doc.utf16ToPosition(tick.change.endOffset, client.positionEncoding);
  const range: Range = {
    start: { line: start.line, character: start.character },
    end: { line: end.line, character: end.character },
  };
  client.sendNotification("textDocument/didChange", {
    textDocument: { uri, version: tick.version },
    contentChanges: [{ range, text: tick.change.text }],
  });
}

/** The cursor (sample) position of a tick, as an LSP position in the negotiated encoding. */
export function tickCursorPosition(client: CollectorLspClient, tick: Tick): Position {
  return offsetToPosition(tick.text, tick.cursor, client.positionEncoding);
}

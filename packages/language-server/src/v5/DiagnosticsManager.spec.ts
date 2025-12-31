/**
 * @ai-generated - This test file was generated with AI assistance.
 * Covers DiagnosticsManager behavior:
 * - Only Vue documents are processed
 * - `sendDiagnostics` payload does not include internal cancellation token
 * - Requests are skipped when document version changes before processing
 */

import { describe, expect, it, vi } from "vitest";
import { CancellationToken } from "vscode-languageserver";

import { DiagnosticsManager } from "./DiagnosticsManager";
import { VueDocument } from "./documents";

describe("DiagnosticsManager", () => {
  it("does not send diagnostics for unknown documents", () => {
    vi.useFakeTimers();
    const connection = { sendDiagnostics: vi.fn() } as any;
    const verterManager = { getTsService: vi.fn() } as any;
    const documentManager = { getDocument: vi.fn(() => undefined) } as any;

    const manager = new DiagnosticsManager(connection, verterManager, documentManager);
    manager.requestDiagnostics("file:///unknown.vue");
    vi.runAllTimers();

    expect(connection.sendDiagnostics).not.toHaveBeenCalled();
    vi.useRealTimers();
  });

  it("sends diagnostics without internal token field", () => {
    vi.useFakeTimers();
    const connection = { sendDiagnostics: vi.fn() } as any;
    const verterManager = { getTsService: vi.fn() } as any;

    const doc = VueDocument.create("file:///test.vue", "<template/>", 1);
    const documentManager = { getDocument: vi.fn(() => doc) } as any;

    const manager = new DiagnosticsManager(connection, verterManager, documentManager);

    // Avoid running real parsing/LS; this file's tests focus on batching + payload.
    (manager as any).retrieveDiagnostics = vi.fn((_d: any, token: CancellationToken) => {
      return {
        uri: doc.uri,
        diagnostics: [],
        token,
      };
    });

    manager.requestDiagnostics(doc.uri);
    vi.runAllTimers();

    expect(connection.sendDiagnostics).toHaveBeenCalledTimes(1);
    const payload = connection.sendDiagnostics.mock.calls[0][0];
    expect(payload).toEqual({ uri: doc.uri, diagnostics: [] });
    expect("token" in payload).toBe(false);
    vi.useRealTimers();
  });

  it("skips sending diagnostics when document version changes before processing", () => {
    vi.useFakeTimers();
    const connection = { sendDiagnostics: vi.fn() } as any;
    const verterManager = { getTsService: vi.fn() } as any;

    const doc = VueDocument.create("file:///test.vue", "<template/>", 1);
    const documentManager = { getDocument: vi.fn(() => doc) } as any;

    const manager = new DiagnosticsManager(connection, verterManager, documentManager);

    // If version check fails, retrieveDiagnostics should never be called.
    (manager as any).retrieveDiagnostics = vi.fn(() => {
      throw new Error("retrieveDiagnostics should not be called when version mismatches");
    });

    manager.requestDiagnostics(doc.uri);
    // Mutate version before debounced batch runs
    doc.update(doc.getText(), doc.version + 1);
    vi.runAllTimers();

    expect(connection.sendDiagnostics).not.toHaveBeenCalled();
    vi.useRealTimers();
  });
});

/**
 * @ai-generated - Tests VS Code's document-only synchronization of plain TS/JS.
 */

import { describe, expect, it } from "vitest";
import {
  isPlainScriptFileUri,
  PlainScriptDocumentSync,
  shouldSuppressPlainScriptDiagnostics,
  snapshotPlainScriptDocument,
  type LspNotificationSender,
  type PlainScriptDocumentSnapshot,
} from "./plainScriptDocumentSync";

function document(
  uri: string,
  languageId: string,
  version: number,
  text: string,
): PlainScriptDocumentSnapshot {
  return { uri, scheme: "file", languageId, version, text };
}

function sender() {
  const sent: Array<{ method: string; params: unknown }> = [];
  const value: LspNotificationSender = {
    sendNotification(method, params) {
      sent.push({ method, params });
    },
  };
  return { sent, value };
}

describe("PlainScriptDocumentSync", () => {
  // Mutation: call toString(true); the encoded URI assertion fails.
  it("uses encoded document URIs and identifies plain-script diagnostic surfaces", () => {
    const snapshot = snapshotPlainScriptDocument({
      uri: {
        scheme: "file",
        path: "/src/hash # percent %.ts",
        toString: (skipEncoding) =>
          skipEncoding
            ? "file:///src/hash # percent %.ts"
            : "file:///src/hash%20%23%20percent%20%25.ts",
      },
      languageId: "typescript",
      version: 1,
      getText: () => "export {}",
    });

    expect(snapshot.uri).toBe("file:///src/hash%20%23%20percent%20%25.ts");
    expect(isPlainScriptFileUri({ scheme: "file", path: "/src/file.tsx" })).toBe(true);
    expect(isPlainScriptFileUri({ scheme: "file", path: "/src/App.vue" })).toBe(false);
    expect(shouldSuppressPlainScriptDiagnostics({ scheme: "file", path: "/src/file.ts" })).toBe(
      true,
    );
    expect(
      shouldSuppressPlainScriptDiagnostics(
        { scheme: "file", path: "/src/extensionless" },
        "typescript",
      ),
    ).toBe(true);
    expect(shouldSuppressPlainScriptDiagnostics({ scheme: "file", path: "/src/App.vue" })).toBe(
      false,
    );
  });

  // Mutation: omit connect-time replay; this fails by losing the dirty buffer.
  it("replays already-open dirty TS/JS buffers when an LSP client connects", () => {
    const sync = new PlainScriptDocumentSync();
    sync.observeOpen(document("file:///src/dirty.ts", "typescript", 7, "export const value = 2"));
    sync.observeOpen(
      document("file:///src/view.tsx", "typescriptreact", 3, "export const View = 1"),
    );
    sync.observeOpen(document("file:///src/App.vue", "vue", 1, "<template />"));

    const target = sender();
    sync.connect(target.value);

    expect(target.sent).toEqual([
      {
        method: "textDocument/didOpen",
        params: {
          textDocument: {
            uri: "file:///src/dirty.ts",
            languageId: "typescript",
            version: 7,
            text: "export const value = 2",
          },
        },
      },
      {
        method: "textDocument/didOpen",
        params: {
          textDocument: {
            uri: "file:///src/view.tsx",
            languageId: "typescriptreact",
            version: 3,
            text: "export const View = 1",
          },
        },
      },
    ]);
  });

  // Mutation: forward only the incremental fragment; the full-buffer assertion fails.
  it("sends full-buffer changes and opens a changed document if needed", () => {
    const sync = new PlainScriptDocumentSync();
    sync.observeOpen(document("file:///src/dep.ts", "typescript", 1, "export const answer = 4"));
    const target = sender();
    sync.connect(target.value);
    target.sent.length = 0;

    sync.observeChange(document("file:///src/dep.ts", "typescript", 2, "export const answer = 42"));

    expect(target.sent).toEqual([
      {
        method: "textDocument/didChange",
        params: {
          textDocument: { uri: "file:///src/dep.ts", version: 2 },
          contentChanges: [{ text: "export const answer = 42" }],
        },
      },
    ]);

    target.sent.length = 0;
    sync.observeChange(
      document("file:///src/late.js", "javascript", 5, "export const late = true"),
    );
    expect(target.sent).toEqual([
      {
        method: "textDocument/didOpen",
        params: {
          textDocument: {
            uri: "file:///src/late.js",
            languageId: "javascript",
            version: 5,
            text: "export const late = true",
          },
        },
      },
    ]);
  });

  // Mutation: omit didClose or retain the closed snapshot; either close/reconnect assertion fails.
  it("closes discarded buffers and replays only documents still open after restart", () => {
    const sync = new PlainScriptDocumentSync();
    const dep = document("file:///src/dep.js", "javascript", 4, "export const stale = true");
    sync.observeOpen(dep);

    const first = sender();
    sync.connect(first.value);
    sync.observeClose(dep);
    expect(first.sent[first.sent.length - 1]).toEqual({
      method: "textDocument/didClose",
      params: { textDocument: { uri: "file:///src/dep.js" } },
    });

    sync.disconnect();
    const second = sender();
    sync.connect(second.value);
    expect(second.sent).toEqual([]);
  });

  // Mutation: ignore stopped/running transitions; the replacement server receives no didOpen.
  it("replays open buffers after an automatic client restart", () => {
    const sync = new PlainScriptDocumentSync();
    sync.observeOpen(document("file:///src/dep.ts", "typescript", 4, "export const value = 4"));

    const first = sender();
    sync.observeClientState("running", first.value);
    expect(first.sent).toHaveLength(1);

    sync.observeClientState("stopped", first.value);
    const second = sender();
    sync.observeClientState("starting", second.value);
    sync.observeClientState("running", second.value);
    expect(second.sent).toEqual(first.sent);
  });
});

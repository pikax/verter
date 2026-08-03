import { describe, expect, it, vi } from "vitest";

// TE-C-11: CSS diagnostics must FAIL CLOSED on a stale/unavailable structure —
// a non-`available` structure response must never surface as a successful
// empty validation (which would clear last-known real diagnostics).

vi.mock("vscode", () => {
  const textDocuments: Array<{ uri: { toString(): string }; version: number }> = [];
  class Position {
    constructor(
      public line: number,
      public character: number,
    ) {}
  }
  class Range {
    constructor(
      public start: Position,
      public end: Position,
    ) {}
  }
  class Diagnostic {
    source?: string;
    constructor(
      public range: Range,
      public message: string,
      public severity?: number,
    ) {}
  }
  return {
    __textDocuments: textDocuments,
    workspace: { textDocuments },
    window: {
      showWarningMessage: async () => undefined,
    },
    languages: {
      createDiagnosticCollection: () => ({
        set() {},
        delete() {},
        dispose() {},
      }),
    },
    Uri: {
      parse: (value: string) => ({ toString: () => value }),
    },
    Position,
    Range,
    Diagnostic,
    DiagnosticSeverity: { Error: 0, Warning: 1, Information: 2, Hint: 3 },
  };
});

import * as vscode from "vscode";
import { CssService } from "./cssService";
import type { DocumentStructureResponseV1 } from "@verter/language-shared";

const URI = "file:///workspace/App.vue";

function liveDocuments(): Array<{ uri: { toString(): string }; version: number }> {
  return (vscode as unknown as Record<string, unknown>).__textDocuments as Array<{
    uri: { toString(): string };
    version: number;
  }>;
}

function styleSection(source: string) {
  const contentStart = source.indexOf(">") + 1;
  const contentEnd = source.indexOf("</style>");
  const range = (start: number, end: number) => ({ sourceSpaceToken: "s0", start, end });
  return {
    kind: "section",
    markupRootTokens: [],
    section: {
      blockToken: "style-0",
      role: { kind: "style", dialect: "css", scoped: false, module: "none" },
      openingRange: range(0, contentStart),
      contentRange: range(contentStart, contentEnd),
      closingRange: range(contentEnd, source.length),
      fullRange: range(0, source.length),
      attributeInsertionAnchor: range(contentStart - 1, contentStart - 1),
      attributes: [],
    },
  };
}

type Responder = (params: {
  requestToken: string;
  clientOpenEpoch: string;
  expectedClientVersion: number;
}) => DocumentStructureResponseV1 | Promise<DocumentStructureResponseV1>;

function service(respond: Responder): CssService {
  const client = {
    sendRequest: async (_type: unknown, params: unknown) =>
      respond(
        params as {
          requestToken: string;
          clientOpenEpoch: string;
          expectedClientVersion: number;
        },
      ),
  };
  return new CssService(
    () => client as never,
    undefined,
    () => "epoch-1",
  );
}

function available(
  params: { requestToken: string; clientOpenEpoch: string; expectedClientVersion: number },
  source: string,
): DocumentStructureResponseV1 {
  return {
    kind: "available",
    requestToken: params.requestToken,
    clientOpenEpoch: params.clientOpenEpoch,
    expectedClientVersion: params.expectedClientVersion,
    structure: {
      schemaVersion: 1,
      documentRevisionToken: "rev-1",
      artifactToken: "artifact-1",
      blocks: [styleSection(source)],
      markupNodes: [],
    },
  } as unknown as DocumentStructureResponseV1;
}

describe("CssService doValidation availability fail-closed (TE-C-11)", () => {
  it("returns null (publishes nothing) for an unavailable structure", async () => {
    const source = "<style>.x{color:red}</style>";
    liveDocuments().splice(0, liveDocuments().length, { uri: { toString: () => URI }, version: 1 });
    const svc = service((params) => ({
      kind: "unavailable",
      requestToken: params.requestToken,
      clientOpenEpoch: params.clientOpenEpoch,
      expectedClientVersion: params.expectedClientVersion,
      reason: "structureNotReady",
    }));
    const results = await svc.doValidation(URI, source, 1);
    expect(results).toBeNull();
  });

  it("returns null when the transport fails", async () => {
    const source = "<style>.x{color:red}</style>";
    liveDocuments().splice(0, liveDocuments().length, { uri: { toString: () => URI }, version: 1 });
    const svc = service(() => {
      throw new Error("transport down");
    });
    const results = await svc.doValidation(URI, source, 1);
    expect(results).toBeNull();
  });

  it("returns null for a stale response of another version (not admitted)", async () => {
    const source = "<style>.x{color:red}</style>";
    liveDocuments().splice(0, liveDocuments().length, { uri: { toString: () => URI }, version: 2 });
    const svc = service((params) => ({
      kind: "staleClientDocument",
      requestToken: params.requestToken,
      clientOpenEpoch: params.clientOpenEpoch,
      expectedClientVersion: params.expectedClientVersion,
    }));
    const results = await svc.doValidation(URI, source, 2);
    expect(results).toBeNull();
  });

  it("publishes an EMPTY result for an admitted available structure with clean CSS", async () => {
    const source = "<style>.x{color:red}</style>";
    liveDocuments().splice(0, liveDocuments().length, { uri: { toString: () => URI }, version: 1 });
    const svc = service((params) => available(params, source));
    const results = await svc.doValidation(URI, source, 1);
    expect(results).not.toBeNull();
    expect(results).toEqual([]);
  });

  it("keeps real diagnostics distinguishable: dirty CSS yields diagnostics, a later unavailable yields null", async () => {
    const dirty = "<style>.x{color:}</style>";
    liveDocuments().splice(0, liveDocuments().length, { uri: { toString: () => URI }, version: 1 });
    let mode: "available" | "unavailable" = "available";
    const svc = service((params) =>
      mode === "available"
        ? available(params, dirty)
        : ({
            kind: "unavailable",
            requestToken: params.requestToken,
            clientOpenEpoch: params.clientOpenEpoch,
            expectedClientVersion: params.expectedClientVersion,
            reason: "structureNotReady",
          } as DocumentStructureResponseV1),
    );
    const first = await svc.doValidation(URI, dirty, 1);
    expect(first).not.toBeNull();
    expect(first?.length).toBeGreaterThan(0);
    expect(first?.[0].diagnostics.length).toBeGreaterThan(0);

    // A later revision whose structure is unavailable must NOT report success
    // (null tells the publisher to keep the last-known diagnostics).
    mode = "unavailable";
    liveDocuments().splice(0, liveDocuments().length, { uri: { toString: () => URI }, version: 2 });
    const second = await svc.doValidation(URI, dirty, 2);
    expect(second).toBeNull();
  });
});

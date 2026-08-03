import { describe, expect, it, vi } from "vitest";

// TE-C-11: CSS diagnostics must FAIL CLOSED on a stale/unavailable structure —
// a non-`available` structure response must never surface as a successful
// empty validation (which would clear last-known real diagnostics).

vi.mock("vscode", () => {
  const textDocuments: Array<{ uri: { toString(): string }; version: number }> = [];
  const diagnosticSets: Array<{ uri: string; count: number }> = [];
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
    __diagnosticSets: diagnosticSets,
    workspace: { textDocuments },
    window: {
      showWarningMessage: async () => undefined,
    },
    languages: {
      createDiagnosticCollection: () => ({
        set(uri: { toString(): string }, diags: readonly unknown[]) {
          diagnosticSets.push({ uri: uri.toString(), count: diags?.length ?? 0 });
        },
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

const transpileGate: { block?: Promise<void>; started?: () => void } = {};
vi.mock("./transpiler", () => ({
  transpile: async () => {
    transpileGate.started?.();
    if (transpileGate.block) await transpileGate.block;
    return { css: ".compiled { color: blue }", sourceMap: undefined };
  },
}));

import * as vscode from "vscode";
import { CssService } from "./cssService";
import { RequestType } from "@verter/language-shared";
import type { DocumentStructureResponseV1 } from "@verter/language-shared";

const URI = "file:///workspace/App.vue";

function liveDocuments(): Array<{ uri: { toString(): string }; version: number }> {
  return (vscode as unknown as Record<string, unknown>).__textDocuments as Array<{
    uri: { toString(): string };
    version: number;
  }>;
}

function diagnosticSets(): Array<{ uri: string; count: number }> {
  return (vscode as unknown as Record<string, unknown>).__diagnosticSets as Array<{
    uri: string;
    count: number;
  }>;
}

function styleSection(source: string, opts?: { dialect?: string; src?: boolean }) {
  const contentStart = source.indexOf(">") + 1;
  const contentEnd = source.indexOf("</style>");
  const range = (start: number, end: number) => ({ sourceSpaceToken: "s0", start, end });
  const attributes: unknown[] = [];
  if (opts?.src) {
    const srcStart = source.indexOf("src=");
    attributes.push({
      attributeToken: "src-0",
      kind: "named",
      name: { spelling: "src", normalized: "src", range: range(srcStart, srcStart + 3) },
      value: "./theme.css",
      fullRange: range(srcStart, srcStart + 17),
    });
  }
  return {
    kind: "section",
    markupRootTokens: [],
    section: {
      blockToken: "style-0",
      role: { kind: "style", dialect: opts?.dialect ?? "css", scoped: false, module: "none" },
      openingRange: range(0, contentStart),
      contentRange: range(contentStart, contentEnd),
      closingRange: range(contentEnd, source.length),
      fullRange: range(0, source.length),
      attributeInsertionAnchor: range(contentStart - 1, contentStart - 1),
      attributes,
    },
  };
}

type Responder = (params: {
  requestToken: string;
  clientOpenEpoch: string;
  expectedClientVersion: number;
}) => DocumentStructureResponseV1 | Promise<DocumentStructureResponseV1>;

function service(
  respond: Responder,
  requests?: Array<{ type: unknown; params: unknown }>,
): CssService {
  const client = {
    sendRequest: async (type: unknown, params: unknown) => {
      requests?.push({ type, params });
      if (type === RequestType.ApplyStyleOverrides) return { success: true };
      return respond(
        params as {
          requestToken: string;
          clientOpenEpoch: string;
          expectedClientVersion: number;
        },
      );
    },
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
  opts?: { dialect?: string; src?: boolean },
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
      blocks: [styleSection(source, opts)],
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

  it("treats external-src stray inline content as unavailable, not validatable CSS (R2-B-03)", async () => {
    // The stray inline bytes inside a `src` block are framework-IGNORED: the
    // external file replaces the block content. Validating them fabricates
    // diagnostics for content Vue never uses.
    const dirty = '<style src="./theme.css">.bad{color:}</style>';
    liveDocuments().splice(0, liveDocuments().length, { uri: { toString: () => URI }, version: 1 });
    const svc = service((params) => available(params, dirty, { src: true }));
    const results = await svc.doValidation(URI, dirty, 1);
    expect(results).toEqual([]);
  });

  it("sends no style override for an external-src preprocessor block (R2-B-03)", async () => {
    const external = '<style src="./theme.sass" lang="sass"></style>';
    liveDocuments().splice(0, liveDocuments().length, { uri: { toString: () => URI }, version: 1 });
    const requests: Array<{ type: unknown; params: unknown }> = [];
    const svc = service(
      (params) => available(params, external, { dialect: "sass", src: true }),
      requests,
    );
    await svc.doValidation(URI, external, 1);
    expect(requests.filter((r) => r.type === RequestType.ApplyStyleOverrides)).toHaveLength(0);
  });

  it("still sends the override for an INLINE preprocessor block (negative control)", async () => {
    const inline = '<style lang="sass">.a\n  color: red\n</style>';
    liveDocuments().splice(0, liveDocuments().length, { uri: { toString: () => URI }, version: 1 });
    const requests: Array<{ type: unknown; params: unknown }> = [];
    const svc = service((params) => available(params, inline, { dialect: "sass" }), requests);
    await svc.doValidation(URI, inline, 1);
    expect(requests.filter((r) => r.type === RequestType.ApplyStyleOverrides)).toHaveLength(1);
  });

  it("binds the override to the captured revision/artifact tokens (R2-B-04)", async () => {
    const inline = '<style lang="sass">.a\n  color: red\n</style>';
    liveDocuments().splice(0, liveDocuments().length, { uri: { toString: () => URI }, version: 1 });
    const requests: Array<{ type: unknown; params: unknown }> = [];
    const svc = service((params) => available(params, inline, { dialect: "sass" }), requests);
    await svc.doValidation(URI, inline, 1);
    const override = requests.find((r) => r.type === RequestType.ApplyStyleOverrides);
    expect(override).toBeDefined();
    expect(override?.params).toMatchObject({
      uri: URI,
      documentRevisionToken: "rev-1",
      artifactToken: "artifact-1",
    });
  });

  it("drops a transpile result whose document moved before the send (R2-B-04)", async () => {
    const inline = '<style lang="sass">.a\n  color: red\n</style>';
    liveDocuments().splice(0, liveDocuments().length, { uri: { toString: () => URI }, version: 1 });
    const requests: Array<{ type: unknown; params: unknown }> = [];
    const svc = service((params) => available(params, inline, { dialect: "sass" }), requests);
    let startedResolve!: () => void;
    const started = new Promise<void>((resolve) => {
      startedResolve = resolve;
    });
    transpileGate.started = () => startedResolve();
    let release!: () => void;
    transpileGate.block = new Promise((resolve) => {
      release = resolve;
    });
    const pending = svc.doValidation(URI, inline, 1);
    // Wait until revision A's transpile is genuinely in flight (admission
    // already passed), THEN commit revision B and release the transpile.
    await started;
    liveDocuments().splice(0, liveDocuments().length, { uri: { toString: () => URI }, version: 2 });
    release();
    transpileGate.block = undefined;
    transpileGate.started = undefined;
    await pending;
    expect(requests.filter((r) => r.type === RequestType.ApplyStyleOverrides)).toHaveLength(0);
  });

  it("a stale invocation publishes nothing and never overwrites a newer revision's cache (R3-B-04)", async () => {
    const inline = '<style lang="sass">.a\n  color: red\n</style>';
    liveDocuments().splice(0, liveDocuments().length, { uri: { toString: () => URI }, version: 1 });
    const requests: Array<{ type: unknown; params: unknown }> = [];
    const svc = service((params) => available(params, inline, { dialect: "sass" }), requests);

    // Gate revision 1's transpile so its post-await writes run AFTER
    // revision 2 has fully validated and cached.
    let startedResolve!: () => void;
    const started = new Promise<void>((resolve) => {
      startedResolve = resolve;
    });
    transpileGate.started = () => startedResolve();
    let release!: () => void;
    transpileGate.block = new Promise((resolve) => {
      release = resolve;
    });
    const pendingV1 = svc.doValidation(URI, inline, 1);
    await started;
    transpileGate.block = undefined;
    transpileGate.started = undefined;

    // Commit revision 2 and complete ITS validation (ungated transpile).
    liveDocuments().splice(0, liveDocuments().length, { uri: { toString: () => URI }, version: 2 });
    const second = await svc.doValidation(URI, inline, 2);
    expect(second).not.toBeNull();
    const structureRequestsAfterV2 = requests.filter(
      (r) => r.type === RequestType.GetDocumentStructure,
    ).length;
    const diagnosticSetsAfterV2 = diagnosticSets().length;

    // Release revision 1's stale in-flight invocation.
    release();
    await pendingV1;

    // The stale invocation publishes NO diagnostics…
    expect(diagnosticSets().length).toBe(diagnosticSetsAfterV2);
    // …and does NOT overwrite revision 2's cache entry: a fresh revision-2
    // demand is a cache hit (no additional structure request).
    const third = await svc.doValidation(URI, inline, 2);
    expect(third).not.toBeNull();
    expect(requests.filter((r) => r.type === RequestType.GetDocumentStructure)).toHaveLength(
      structureRequestsAfterV2,
    );
  });

  it("re-queries a transient non-available structure on the next demand (R3-B-04)", async () => {
    const dirty = "<style>.x{color:}</style>";
    liveDocuments().splice(0, liveDocuments().length, { uri: { toString: () => URI }, version: 1 });
    let mode: "available" | "unavailable" = "unavailable";
    const requests: Array<{ type: unknown; params: unknown }> = [];
    const svc = service(
      (params) =>
        mode === "available"
          ? available(params, dirty)
          : ({
              kind: "unavailable",
              requestToken: params.requestToken,
              clientOpenEpoch: params.clientOpenEpoch,
              expectedClientVersion: params.expectedClientVersion,
              reason: "structureNotReady",
            } as DocumentStructureResponseV1),
      requests,
    );

    const first = await svc.doValidation(URI, dirty, 1);
    expect(first).toBeNull();

    // The structure host recovered at the SAME (version, openEpoch): the
    // transient non-available must not be sticky — the next demand
    // re-queries and serves real diagnostics.
    mode = "available";
    const second = await svc.doValidation(URI, dirty, 1);
    expect(second).not.toBeNull();
    expect(second?.length).toBeGreaterThan(0);
    expect(requests.filter((r) => r.type === RequestType.GetDocumentStructure)).toHaveLength(2);
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

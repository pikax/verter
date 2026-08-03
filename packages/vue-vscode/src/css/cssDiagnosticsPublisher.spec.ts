import { describe, expect, it } from "vitest";
import { vi } from "vitest";

// B-29: the CSS diagnostics publication path must RE-ADMIT after its await —
// a stale invocation's validation result (computed against an older document
// revision) must never be published against the live document.

vi.mock("vscode", () => {
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
    Position,
    Range,
    Diagnostic,
    DiagnosticSeverity: { Error: 0, Warning: 1, Information: 2, Hint: 3 },
  };
});

import type { Diagnostic as CSSDiagnostic } from "vscode-css-languageservice";
import { createCssDiagnosticsUpdater } from "./cssDiagnosticsPublisher";

interface MutableDocument {
  uri: { toString(): string };
  languageId: string;
  version: number;
  isClosed: boolean;
  getText(): string;
}

function makeDocument(overrides?: Partial<MutableDocument>): MutableDocument {
  return {
    uri: { toString: () => "file:///workspace/App.vue" },
    languageId: "vue",
    version: 1,
    isClosed: false,
    getText: () => "<style>.x{color:}</style>",
    ...overrides,
  };
}

function collector(): {
  sets: Array<{ uri: string; count: number }>;
  set: (u: unknown, d?: readonly unknown[]) => void;
} {
  const sets: Array<{ uri: string; count: number }> = [];
  return {
    sets,
    set(uri: unknown, diags?: readonly unknown[]) {
      sets.push({ uri: String(uri), count: diags?.length ?? 0 });
    },
  };
}

const blockDiagnostic: { blockToken: string; diagnostics: CSSDiagnostic[] } = {
  blockToken: "style-0",
  diagnostics: [
    {
      range: { start: { line: 0, character: 7 }, end: { line: 0, character: 9 } },
      message: "expected color value",
      severity: 1,
    },
  ],
};

describe("createCssDiagnosticsUpdater publication re-admission (B-29)", () => {
  it("publishes an admitted, still-current validation result", async () => {
    const document = makeDocument();
    const sink = collector();
    const update = createCssDiagnosticsUpdater(
      () => ({ doValidation: async () => [blockDiagnostic] }),
      sink,
    );
    await update(document as never);
    expect(sink.sets).toHaveLength(1);
    expect(sink.sets[0].count).toBe(1);
  });

  it("never publishes a stale invocation's return: the document moved during the await", async () => {
    const document = makeDocument();
    const sink = collector();
    const update = createCssDiagnosticsUpdater(
      () => ({
        doValidation: async () => {
          // The document commits a NEWER revision while validation is in
          // flight; the resolved result belongs to the old revision.
          document.version = 2;
          return [blockDiagnostic];
        },
      }),
      sink,
    );
    await update(document as never);
    expect(sink.sets).toHaveLength(0);
  });

  it("never publishes against a document that closed during the await", async () => {
    const document = makeDocument();
    const sink = collector();
    const update = createCssDiagnosticsUpdater(
      () => ({
        doValidation: async () => {
          document.isClosed = true;
          return [blockDiagnostic];
        },
      }),
      sink,
    );
    await update(document as never);
    expect(sink.sets).toHaveLength(0);
  });

  it("publishes nothing for a fail-closed null validation (keeps last-known diagnostics)", async () => {
    const document = makeDocument();
    const sink = collector();
    const update = createCssDiagnosticsUpdater(() => ({ doValidation: async () => null }), sink);
    await update(document as never);
    expect(sink.sets).toHaveLength(0);
  });

  it("publishes a genuinely clean EMPTY result (clears prior diagnostics)", async () => {
    const document = makeDocument();
    const sink = collector();
    const update = createCssDiagnosticsUpdater(() => ({ doValidation: async () => [] }), sink);
    await update(document as never);
    expect(sink.sets).toHaveLength(1);
    expect(sink.sets[0].count).toBe(0);
  });

  it("does not validate non-framework-carrier documents at all", async () => {
    const document = makeDocument({ languageId: "typescript" });
    const sink = collector();
    const doValidation = vi.fn(async () => [blockDiagnostic]);
    const update = createCssDiagnosticsUpdater(() => ({ doValidation }), sink);
    await update(document as never);
    expect(doValidation).not.toHaveBeenCalled();
    expect(sink.sets).toHaveLength(0);
  });
});

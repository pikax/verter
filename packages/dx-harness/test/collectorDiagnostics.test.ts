import { describe, expect, it } from "vitest";

import {
  CollectingSink,
  EditBuffer,
  classifyDiagnosticsSample,
  collectDiagnostics,
  type CollectorLspClient,
} from "../src/collectors/index.js";
import type { CollectorEventKey } from "../src/collectors/index.js";
import { GeneratedDocument, normalizeDiagnostics } from "../src/index.js";
import type { CanonicalDiagnostic, NormalizedDiagnostic, Probe } from "../src/index.js";

const key: CollectorEventKey = {
  scenario: "minimal-member-access",
  editStepIndex: 0,
  driver: "rawLsp",
  provider: "tsgo",
  probe: "diagnostics",
  version: 1,
  anchor: "doc",
};

const diag = (over: Partial<CanonicalDiagnostic> = {}): CanonicalDiagnostic =>
  normalizeDiagnostics([
    {
      range: over.range ?? { start: { line: 2, character: 4 }, end: { line: 2, character: 9 } },
      severity: 1,
      code: over.code ?? "2304",
      message: over.message ?? "Cannot find name 'foo'.",
    },
  ])[0];

describe("classifyDiagnosticsSample — (0,0)-default flagged only when the known span is elsewhere", () => {
  it("flags a (0,0) diagnostic whose KNOWN source span is elsewhere", () => {
    const verter = normalizeDiagnostics([
      {
        range: { start: { line: 0, character: 0 }, end: { line: 0, character: 0 } },
        severity: 1,
        code: "2304",
        message: "x",
      },
    ]);
    const events = classifyDiagnosticsSample({
      key,
      verter,
      knownSourceSpans: {
        "2304": { start: { line: 5, character: 2 }, end: { line: 5, character: 8 } },
      },
    });
    const fail = events.filter((e) => !e.ok);
    expect(fail).toHaveLength(1);
    expect(fail[0].signal).toBe("diagnostics_default_range");
    expect(fail[0].severity).toBe("userVisible");
  });

  it("does NOT flag a (0,0) diagnostic whose KNOWN source span IS at the origin", () => {
    const verter = normalizeDiagnostics([
      {
        range: { start: { line: 0, character: 0 }, end: { line: 0, character: 5 } },
        severity: 1,
        code: "2304",
        message: "x",
      },
    ]);
    const events = classifyDiagnosticsSample({
      key,
      verter,
      knownSourceSpans: {
        "2304": { start: { line: 0, character: 0 }, end: { line: 0, character: 5 } },
      },
    });
    expect(events.filter((e) => !e.ok)).toHaveLength(0);
  });

  it("does NOT flag a precise positive-width LINE-0 diagnostic with no known span", () => {
    const verter = [
      diag({ range: { start: { line: 0, character: 3 }, end: { line: 0, character: 8 } } }),
    ];
    const events = classifyDiagnosticsSample({ key, verter });
    expect(events.filter((e) => !e.ok)).toHaveLength(0);
  });

  it("flags the zero-width (0,0)-(0,0) default sentinel even with no known span (impossible extent)", () => {
    const verter = normalizeDiagnostics([
      {
        range: { start: { line: 0, character: 0 }, end: { line: 0, character: 0 } },
        severity: 1,
        code: "2304",
        message: "x",
      },
    ]);
    const events = classifyDiagnosticsSample({ key, verter });
    expect(events.filter((e) => !e.ok && e.signal === "diagnostics_default_range")).toHaveLength(1);
  });
});

describe("classifyDiagnosticsSample — baseline parity classifies verter-only / baseline-only / range", () => {
  // Emitted TSX: line 0 "const a = 1;\n", line 1 "const b: string = 2;\n". The baseline byte
  // offsets index into this text; the document converts them to generated positions.
  const tsx = "const a = 1;\nconst b: string = 2;\n";
  const document = new GeneratedDocument(tsx);

  it("agrees when verter and the baseline diagnostic share code + range", () => {
    // The "2" on line 1 (the first occurrence after the line-0 newline) — the
    // mistyped value whose byte offset the baseline diagnostic indexes into.
    const start = tsx.indexOf("2", tsx.indexOf("\n"));
    const baseline: NormalizedDiagnostic[] = [
      {
        start,
        end: start + 1,
        severity: "error",
        code: "2322",
        message: "Type 'number' is not assignable to type 'string'.",
      },
    ];
    const gen = document.byteRangeToPosition(start, start + 1);
    const verter = normalizeDiagnostics([
      {
        range: gen,
        severity: 1,
        code: "2322",
        message: "Type 'number' is not assignable to type 'string'.",
      },
    ]);
    const events = classifyDiagnosticsSample({
      key,
      verter,
      baseline: { provider: "tsgo", diagnostics: baseline, document },
    });
    expect(events.every((e) => e.ok)).toBe(true);
  });

  it("flags a verter-only diagnostic the baseline did not emit", () => {
    const verter = normalizeDiagnostics([
      {
        range: { start: { line: 1, character: 6 }, end: { line: 1, character: 7 } },
        severity: 1,
        code: "9999",
        message: "verter-only",
      },
    ]);
    const events = classifyDiagnosticsSample({
      key,
      verter,
      baseline: { provider: "tsgo", diagnostics: [], document },
    });
    const fail = events.filter((e) => !e.ok);
    expect(fail.some((e) => (e.data as { class?: string }).class === "verterOnly")).toBe(true);
  });
});

const oracleProbe: Probe = {
  id: "diag-oracle",
  method: "diagnostics",
  anchor: "doc",
  mappingPolicy: "none",
  confidence: "high",
  dimension: "vueSemanticValidity",
  requiresSourceMap: false,
  requiredDrivers: [],
  capabilityRequirements: [],
};

const TYPE_ERROR = "Type 'number' is not assignable to type 'string'.";

/** Verter's `.vue` 2322 diagnostic (the authored span is irrelevant — the oracle matches by code). */
const verter2322 = (): readonly CanonicalDiagnostic[] =>
  normalizeDiagnostics([
    {
      range: { start: { line: 1, character: 6 }, end: { line: 1, character: 9 } },
      severity: 1,
      code: "2322",
      message: TYPE_ERROR,
    },
  ]);

/** A bridge `.ts` oracle diagnostic at code `2322` (its raw `.ts` byte span is never compared cross-file). */
const oracle2322: NormalizedDiagnostic = {
  start: 0,
  end: 1,
  severity: "error",
  code: "2322",
  message: TYPE_ERROR,
};

describe("classifyDiagnosticsSample — curated vue-semantic-validity oracle (shared classifyOracleDiagnostics)", () => {
  it("agrees (ok) when verter and the `.ts` oracle share a diagnostic code", () => {
    const events = classifyDiagnosticsSample({
      key,
      verter: verter2322(),
      oracle: { probe: oracleProbe, providers: { tsgo: { ok: true, output: [oracle2322] } } },
    });
    const oracleEvents = events.filter((e) => e.signal === "diagnostics_vue_semantic_validity");
    expect(oracleEvents).toHaveLength(1);
    expect(oracleEvents[0].ok).toBe(true);
  });

  it("flags a verter-only diagnostic the `.ts` oracle lacks (a verter false-red)", () => {
    const events = classifyDiagnosticsSample({
      key,
      verter: verter2322(),
      oracle: { probe: oracleProbe, providers: { tsgo: { ok: true, output: [] } } },
    });
    const fail = events.filter((e) => e.signal === "diagnostics_vue_semantic_validity" && !e.ok);
    expect(fail).toHaveLength(1);
    expect(fail[0].severity).toBe("userVisible");
    expect((fail[0].data as { class?: string }).class).toBe("verterOnly");
  });

  it("does NOT emit diagnostics_vue_semantic_validity without an oracle (no spurious emission)", () => {
    const events = classifyDiagnosticsSample({ key, verter: verter2322() });
    expect(events.some((e) => e.signal === "diagnostics_vue_semantic_validity")).toBe(false);
  });
});

/**
 * A fake LSP client that publishes ONE canned `publishDiagnostics` push on the
 * document's `didOpen`, so the LIVE {@link collectDiagnostics} driver — its push
 * accumulation AND the oracle option it threads into {@link classifyDiagnosticsSample}
 * — is verifiable without spawning a server.
 */
class FakeDiagnosticsClient implements CollectorLspClient {
  readonly positionEncoding = "utf-16" as const;
  readonly serverCapabilities = {};
  readonly stderr = { text: (): string => "" };
  private readonly diagHandlers = new Set<(p: unknown) => void>();
  constructor(private readonly toPublish: { uri: string; diagnostics: unknown[] }) {}
  async sendRequest<T = unknown>(): Promise<T> {
    return null as T;
  }
  sendNotification(method: string): void {
    // Push the canned diagnostics once the document opens (handlers already registered).
    if (method === "textDocument/didOpen") {
      for (const handler of this.diagHandlers) handler(this.toPublish);
    }
  }
  onNotification(method: string, handler: (p: unknown) => void): void {
    if (method === "textDocument/publishDiagnostics") this.diagHandlers.add(handler);
  }
  offNotification(method: string, handler: (p: unknown) => void): void {
    if (method === "textDocument/publishDiagnostics") this.diagHandlers.delete(handler);
  }
}

describe("collectDiagnostics — the live oracle option threads the curated `.ts` oracle into the sample", () => {
  it("emits an ok diagnostics_vue_semantic_validity when verter's pushed diagnostic matches the oracle code", async () => {
    const uri = "file:///bad.vue";
    const client = new FakeDiagnosticsClient({
      uri,
      diagnostics: [
        {
          range: { start: { line: 1, character: 6 }, end: { line: 1, character: 9 } },
          severity: 1,
          code: "2322",
          message: TYPE_ERROR,
        },
      ],
    });
    const sink = new CollectingSink();
    await collectDiagnostics({
      client,
      sink,
      uri,
      buffer: new EditBuffer("const bad = 1\n", { doc: 0 }),
      scenario: "hermetic",
      probe: "diagnostics",
      anchor: "doc",
      provider: "tsgo",
      settle: async () => {},
      oracle: { probe: oracleProbe, providers: { tsgo: { ok: true, output: [oracle2322] } } },
    });
    const oracleEvents = sink.events.filter(
      (e) => e.signal === "diagnostics_vue_semantic_validity",
    );
    expect(oracleEvents).toHaveLength(1);
    expect(oracleEvents[0].ok).toBe(true);
  });

  it("does NOT emit diagnostics_vue_semantic_validity without an oracle (no spurious emission)", async () => {
    const uri = "file:///bad.vue";
    const client = new FakeDiagnosticsClient({ uri, diagnostics: [] });
    const sink = new CollectingSink();
    await collectDiagnostics({
      client,
      sink,
      uri,
      buffer: new EditBuffer("const bad = 1\n", { doc: 0 }),
      scenario: "hermetic",
      probe: "diagnostics",
      anchor: "doc",
      provider: "tsgo",
      settle: async () => {},
    });
    expect(sink.events.some((e) => e.signal === "diagnostics_vue_semantic_validity")).toBe(false);
  });
});

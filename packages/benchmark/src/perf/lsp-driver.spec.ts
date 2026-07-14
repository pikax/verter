import { describe, it, expect, beforeAll, afterAll } from "vitest";
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL, fileURLToPath } from "node:url";
import type { EnsuredCorpus } from "./corpus.js";
import {
  assertHoverResult,
  completionItemCount,
  relativizeUri,
  ideQueryLatency,
  warmDependencyEditLatency,
  editToDiagnosticsLatency,
  waitForReady,
  assertTypeProviderTsgo,
  captureTypeProviderStatus,
  publishMatchesTransition,
  DiagnosticsBus,
  type DriverClient,
  type DriverConnect,
  type DiagnosticTransition,
} from "./lsp-driver.js";

// ── A minimal on-disk corpus (one SFC + its sibling type module) ─────────────
let dir: string;
let corpus: EnsuredCorpus;
const SFC =
  '<script setup lang="ts">\nconst props = defineProps<{ x: number }>()\n</script>\n' +
  "<template><div>{{ props.x }}</div></template>\n";
const TYPES =
  "export interface Foo {\n  id: number;\n}\n" +
  "export type Emits = {\n  change: [value: number];\n};\n";

/** A minimal published diagnostic at `line` (the value used by the toggle). */
const diag = (line: number, message = ""): unknown => ({
  range: { start: { line, character: 0 } },
  code: "2322",
  message,
});

beforeAll(() => {
  dir = mkdtempSync(join(tmpdir(), "verter-lsp-driver-spec-"));
  mkdirSync(join(dir, "app"), { recursive: true });
  writeFileSync(join(dir, "app", "Comp.vue"), SFC);
  writeFileSync(join(dir, "app", "types.ts"), TYPES);
  corpus = {
    dir,
    manifest: {} as EnsuredCorpus["manifest"],
    contentHash: "test",
    isGateCorpus: false,
    appTsconfig: "",
    kernelTsconfig: "",
    rootTsconfig: "",
  };
});
afterAll(() => rmSync(dir, { recursive: true, force: true }));

// ── A fake client that lets a test script the server's responses ─────────────
interface FakeBehavior {
  onDidChange?: (params: unknown, emit: (m: string, p: unknown) => void) => void;
  onDidOpen?: (params: unknown, emit: (m: string, p: unknown) => void) => void;
  hover?: (params: unknown) => unknown;
  completion?: (params: unknown) => unknown;
}

class FakeLspClient implements DriverClient {
  private handlers = new Map<string, ((p: unknown) => void)[]>();
  constructor(private readonly behavior: FakeBehavior = {}) {}
  sendNotification(method: string, params?: unknown): void {
    if (method === "textDocument/didChange") this.behavior.onDidChange?.(params, this.emit);
    if (method === "textDocument/didOpen") this.behavior.onDidOpen?.(params, this.emit);
  }
  sendRequest<T>(method: string, params?: unknown): Promise<T> {
    if (method === "textDocument/hover") {
      return Promise.resolve((this.behavior.hover?.(params) ?? null) as T);
    }
    if (method === "textDocument/completion") {
      return Promise.resolve((this.behavior.completion?.(params) ?? null) as T);
    }
    return Promise.resolve(null as T);
  }
  onNotification(method: string, handler: (p: unknown) => void): void {
    const a = this.handlers.get(method) ?? [];
    a.push(handler);
    this.handlers.set(method, a);
  }
  offNotification(method: string, handler: (p: unknown) => void): void {
    const a = this.handlers.get(method);
    if (!a) return;
    const i = a.indexOf(handler);
    if (i >= 0) a.splice(i, 1);
  }
  waitForNotification(
    method: string,
    timeout = 50,
    predicate?: (p: unknown) => boolean,
  ): Promise<unknown> {
    return new Promise((resolve, reject) => {
      const h = (p: unknown): void => {
        if (predicate && !predicate(p)) return;
        this.offNotification(method, h);
        clearTimeout(t);
        resolve(p);
      };
      const t = setTimeout(
        () => {
          this.offNotification(method, h);
          reject(new Error(`fake: '${method}' timed out`));
        },
        Math.min(timeout, 40),
      );
      this.onNotification(method, h);
    });
  }
  kill(): void {
    /* no-op */
  }
  emit = (method: string, params: unknown): void => {
    for (const handler of [...(this.handlers.get(method) ?? [])]) handler(params);
  };
}

const connectWith = (behavior: FakeBehavior): DriverConnect => {
  return async () => ({ client: new FakeLspClient(behavior), rootUri: pathToFileURL(dir).href });
};

// ── The pure result validators ──────────────────────────────────────────────
describe("LSP query-result validators (a no-result query is a failure)", () => {
  it("assertHoverResult throws on a null/empty hover, accepts real contents", () => {
    expect(() => assertHoverResult(null)).toThrow(/contents/i);
    expect(() => assertHoverResult({ contents: null })).toThrow(/contents/i);
    expect(() => assertHoverResult({ contents: "" })).toThrow(/contents/i);
    expect(() => assertHoverResult({ contents: [] })).toThrow(/contents/i);
    expect(() => assertHoverResult({ contents: "Foo: number" })).not.toThrow();
    expect(() => assertHoverResult({ contents: [{ value: "x" }] })).not.toThrow();
  });

  it("assertHoverResult rejects EMPTY rich content (MarkupContent/MarkedString/array-of-empty)", () => {
    // A MarkupContent OBJECT with an empty value is NOT a hit (the repeat-miss
    // class: a no-result hover that returns `{ kind, value: "" }` recorded fast).
    expect(() => assertHoverResult({ contents: { kind: "markdown", value: "" } })).toThrow(
      /contents/i,
    );
    expect(() => assertHoverResult({ contents: { kind: "markdown", value: "   " } })).toThrow(
      /contents/i,
    );
    // An object MarkedString (`{ language, value }`) with an empty value, too.
    expect(() => assertHoverResult({ contents: { language: "ts", value: "" } })).toThrow(
      /contents/i,
    );
    // A non-empty ARRAY of all-empty entries is NOT a hit.
    expect(() => assertHoverResult({ contents: [{ value: "" }] })).toThrow(/contents/i);
    expect(() => assertHoverResult({ contents: ["", "  "] })).toThrow(/contents/i);
    // Real rich content (object value OR array with a non-empty entry) IS a hit.
    expect(() =>
      assertHoverResult({ contents: { kind: "markdown", value: "Foo: number" } }),
    ).not.toThrow();
    expect(() => assertHoverResult({ contents: { language: "ts", value: "x" } })).not.toThrow();
    expect(() => assertHoverResult({ contents: [{ value: "" }, { value: "Foo" }] })).not.toThrow();
  });

  it("completionItemCount throws on an empty completion, counts real labeled items", () => {
    expect(() => completionItemCount(null)).toThrow(/items/i);
    expect(() => completionItemCount([])).toThrow(/items/i);
    expect(() => completionItemCount({ items: [] })).toThrow(/items/i);
    expect(completionItemCount([{ label: "a" }, { label: "b" }, { label: "c" }])).toBe(3);
    expect(completionItemCount({ items: [{ label: "a" }, { label: "b" }] })).toBe(2);
  });

  it("completionItemCount REJECTS content-less items ([{}]/[null]/all-blank-label), accepts a real label", () => {
    // A non-empty array is NOT automatically a hit: a no-op LSP answering fast with
    // shells ([{}]/[null]) or all-blank labels must NOT read as a passing sample
    // (mirrors the hover-contents non-empty rule). Without it, any non-empty array counted.
    expect(() => completionItemCount([{}])).toThrow(/label|content|items/i);
    expect(() => completionItemCount([null])).toThrow(/label|content|items/i);
    expect(() => completionItemCount([{ label: "" }, { label: "   " }])).toThrow(
      /label|content|items/i,
    );
    expect(() => completionItemCount({ items: [{}, null] })).toThrow(/label|content|items/i);
    // A real string label, OR a CompletionItemLabel `{ label }` object, counts.
    expect(completionItemCount([{ label: "foo" }, {}, { label: "bar" }])).toBe(2);
    expect(completionItemCount({ items: [{ label: { label: "baz" } }] })).toBe(1);
  });

  it("relativizeUri returns a corpus-relative path via fileURLToPath (no leading-slash bug)", () => {
    const file = join(dir, "app", "Comp.vue");
    const uri = pathToFileURL(file).href;
    expect(relativizeUri(uri, dir)).toBe("app/Comp.vue");
    // Round-trips on this OS (the discriminator vs the old hand-strip).
    expect(fileURLToPath(uri)).toBe(file);
  });
});

describe("ideQueryLatency does not swallow no-result queries", () => {
  it("FAILS when a hover returns no contents (a broken no-op LSP must not pass)", async () => {
    const connect = connectWith({
      onDidOpen: (_p, emit) =>
        emit("textDocument/publishDiagnostics", { uri: "x", diagnostics: [] }),
      hover: () => ({ contents: null }), // broken: no contents
      completion: () => ({ items: [{ label: "a" }, { label: "b" }, { label: "c" }] }),
    });
    await expect(ideQueryLatency("bin", corpus, { ops: 1 }, connect)).rejects.toThrow(/hover/i);
  });

  it("FAILS when a completion returns zero items", async () => {
    const connect = connectWith({
      hover: () => ({ contents: "Foo: number" }),
      completion: () => ({ items: [] }), // broken: empty
    });
    await expect(ideQueryLatency("bin", corpus, { ops: 1 }, connect)).rejects.toThrow(/items/i);
  });

  it("FAILS when a completion returns only content-less items ([{}]) — a broken no-op LSP", async () => {
    const connect = connectWith({
      hover: () => ({ contents: "Foo: number" }),
      completion: () => ({ items: [{}, null] }), // non-empty but no labels
    });
    await expect(ideQueryLatency("bin", corpus, { ops: 1 }, connect)).rejects.toThrow(
      /label|content|items/i,
    );
  });

  it("RESOLVES with hit/item counts when hover + completion return real labeled results", async () => {
    const connect = connectWith({
      hover: () => ({ contents: "Foo: number" }),
      completion: () => ({
        items: [{ label: "a" }, { label: "b" }, { label: "c" }, { label: "d" }],
      }),
    });
    const s = await ideQueryLatency("bin", corpus, { ops: 2 }, connect);
    expect(s.hoverHits).toBe(2);
    expect(s.completionItems).toBe(4);
    expect(s.hoverLatencies.length).toBe(2);
  });

  it("records EVERY op's completion count (not just the last) — an earlier degraded op pulls the gated scalar down", async () => {
    // Op 0 returns 4 valid items, op 1 returns 1 — capturing only the LAST op's
    // count (1) would hide an op-0 degradation whenever op 0 is not the last op;
    // here the WORST (min) over all ops surfaces the degradation to the
    // candidate/baseline completion_item_parity gate, and the per-op counts are
    // all recorded.
    let n = 0;
    const connect = connectWith({
      hover: () => ({ contents: "Foo: number" }),
      completion: () => {
        const items =
          n === 0
            ? [{ label: "a" }, { label: "b" }, { label: "c" }, { label: "d" }]
            : [{ label: "only" }];
        n++;
        return { items };
      },
    });
    const s = await ideQueryLatency("bin", corpus, { ops: 2 }, connect);
    expect(s.completionItemCounts).toEqual([4, 1]); // every op recorded, in order
    expect(s.completionItems).toBe(1); // the WORST op, not the last-only / the max
  });
});

describe("ideQueryLatency captures hover CONTENT + completion LABEL SET (content, not just counts)", () => {
  it("a candidate whose hover TEXT / completion LABELS diverge (at IDENTICAL counts) is VISIBLE in the captured content", async () => {
    // Two sides return the SAME number of hover hits + completion items, but
    // DIFFERENT content. Capturing only counts (hoverHits / completionItems) would
    // leave a bogus-but-same-count answer invisible. The
    // normalized hover text + completion label SET are captured, so the divergence
    // is visible to the gate's content-equality rail.
    const sideA = connectWith({
      hover: () => ({ contents: "Foo: number" }),
      completion: () => ({ items: [{ label: "alpha" }, { label: "beta" }, { label: "gamma" }] }),
    });
    const sideB = connectWith({
      hover: () => ({ contents: "Bar: string" }), // different TEXT, same hit count
      completion: () => ({ items: [{ label: "alpha" }, { label: "beta" }, { label: "DELTA" }] }), // same item count, one label differs
    });
    const a = await ideQueryLatency("bin", corpus, { ops: 2 }, sideA);
    const b = await ideQueryLatency("bin", corpus, { ops: 2 }, sideB);

    // Counts MATCH on both sides (parity alone would pass — the count-only blind spot).
    expect(a.hoverHits).toBe(b.hoverHits);
    expect(a.completionItems).toBe(b.completionItems);

    // …but the captured CONTENT differs, so a content regression is catchable.
    expect(a.hoverContents.some((c) => c.includes("Foo: number"))).toBe(true);
    expect(b.hoverContents.some((c) => c.includes("Bar: string"))).toBe(true);
    expect(a.hoverContents).not.toEqual(b.hoverContents);

    expect(a.completionLabelSet.some((l) => l.endsWith(":gamma"))).toBe(true);
    expect(b.completionLabelSet.some((l) => l.endsWith(":DELTA"))).toBe(true);
    expect(a.completionLabelSet).not.toEqual(b.completionLabelSet);

    // Identical content on both sides ⇒ identical captured sets (the positive control).
    const c = await ideQueryLatency("bin", corpus, { ops: 2 }, sideA);
    expect(a.hoverContents).toEqual(c.hoverContents);
    expect(a.completionLabelSet).toEqual(c.completionLabelSet);
  });
});

const sfcUriOf = (): string => pathToFileURL(join(dir, "app", "Comp.vue")).toString();
const editTextOf = (params: unknown): string =>
  (params as { contentChanges?: { text?: string }[] }).contentChanges?.[0]?.text ?? "";
const openTextOf = (params: unknown): string =>
  (params as { textDocument?: { text?: string } }).textDocument?.text ?? "";
const didOpenUri = (params: unknown): string =>
  (params as { textDocument?: { uri?: string } }).textDocument?.uri ?? "";

/**
 * The dependent SFC's TS2322 the warm cross-file edit produces: an inline
 * `import("./types").WarmDepProbe<K>` annotation re-pointed at a stable imported
 * string-literal alias `WarmDepProbe<K> = "perfDep<K>"`. The message ECHOES the resolved
 * literal — the unique per-edit fingerprint the wait binds to.
 */
const dep2322 = (token: string): unknown => ({
  range: { start: { line: 38, character: 6 } },
  code: "2322",
  message: `Type '0' is not assignable to type '"${token}"'.`,
});
/** The per-iteration cross-file token the dependent resolves (`WarmDepProbe<K>` → `perfDep<K>`). */
const warmTokenOf = (text: string): string | null => {
  const k = /import\("\.\/types"\)\.WarmDepProbe(\w+)/.exec(text)?.[1];
  return k == null ? null : `perfDep${k}`;
};

/**
 * An `onDidOpen` that publishes the active/dependent SFC's INITIAL (settled)
 * diagnostics so the workload's wait-for-settle resolves before the measured
 * edits. Fires only for the SFC open (not the sibling type-module open). Defaults
 * to a clean (empty) initial set; pass `initialDiags` to model a pre-existing
 * settled state (e.g. a stale error already present before the edit).
 */
const settleOpen =
  (initialDiags: unknown[] = []) =>
  (params: unknown, emit: (m: string, p: unknown) => void): void => {
    if (didOpenUri(params) === sfcUriOf()) {
      setTimeout(
        () =>
          emit("textDocument/publishDiagnostics", { uri: sfcUriOf(), diagnostics: initialDiags }),
        2,
      );
    }
  };

describe("editToDiagnosticsLatency measures the edit's EFFECT, not a stale publish", () => {
  it("FAILS when an edit does NOT change diagnostics (a stale/no-op republish cannot satisfy the wait)", async () => {
    // The server always republishes the SAME (empty) set regardless of the edit, so
    // the active SFC never settles a NON-EMPTY pre-edit baseline (the primed error) —
    // a no-op server is a hard failure, never a fast pass on an empty/queued publish.
    const connect = connectWith({
      onDidOpen: settleOpen(),
      onDidChange: (_p, emit) =>
        setTimeout(
          () => emit("textDocument/publishDiagnostics", { uri: sfcUriOf(), diagnostics: [] }),
          3,
        ),
    });
    await expect(editToDiagnosticsLatency("bin", corpus, { ops: 1 }, connect)).rejects.toThrow(
      /timed out/i,
    );
  });

  it("RESOLVES and records a latency when each edit produces its EXPECTED diagnostic state", async () => {
    const connect = connectWith({
      onDidOpen: settleOpen(),
      onDidChange: (params, emit) => {
        const text = editTextOf(params);
        // The injected error puts the token in TYPE position (`const x: "perf-N" = 0`),
        // so the realistic TS2322 message ("Type '0' is not assignable to type '\"perf-N\"'.")
        // ECHOES the token the edit fingerprint binds to — a genuine per-edit transition that
        // satisfies the unversioned wait. A value-position token would widen to `string` and
        // be absent from the message (the old never-match bug, guarded below).
        const token = /:\s*"(perf-[^"]*)"\s*=\s*0/.exec(text)?.[1];
        setTimeout(
          () =>
            emit("textDocument/publishDiagnostics", {
              uri: sfcUriOf(),
              diagnostics: token
                ? [diag(1, `Type '0' is not assignable to type '"${token}"'.`)]
                : [],
            }),
          3,
        );
      },
    });
    const s = await editToDiagnosticsLatency("bin", corpus, { ops: 2 }, connect);
    expect(s.latencies.length).toBe(2);
  });

  it("FAILS when the 2322 message LACKS this edit's token (value-position widening — the old never-match bug)", async () => {
    // If the injected error sat in VALUE position (`const x: number = "perf-N"`) the literal
    // would widen to `string` and the TS2322 message would be token-free ("Type 'string' is
    // not assignable to type 'number'."). The wait binds messageIncludes: the per-edit token,
    // so a token-less 2322 can NEVER satisfy it — even when the diagnostic SET genuinely
    // changes each edit. Each publish here is at a DISTINCT line (so the set always differs
    // from the pre-edit set), isolating the failure to the missing token, not a stale republish.
    let line = 0;
    const connect = connectWith({
      onDidOpen: settleOpen(),
      onDidChange: (_p, emit) => {
        const at = line++;
        setTimeout(
          () =>
            emit("textDocument/publishDiagnostics", {
              uri: sfcUriOf(),
              diagnostics: [diag(at, "Type 'string' is not assignable to type 'number'.")],
            }),
          3,
        );
      },
    });
    await expect(editToDiagnosticsLatency("bin", corpus, { ops: 1 }, connect)).rejects.toThrow(
      /timed out/i,
    );
  });

  it("FAILS when only a STALE (old-version) opposite-count publish answers the edit (version binding)", async () => {
    // The measured edit is sent above the prime's version; a queued/stale publish
    // that echoes the PRE-edit version (1) must NOT satisfy the measured wait —
    // otherwise a stale publish from before the edit is timed as the edit's effect.
    // The version floor (and the unique-token envelope) reject it and the sample
    // times out.
    const connect = connectWith({
      onDidOpen: settleOpen(),
      onDidChange: (_p, emit) =>
        setTimeout(
          () =>
            emit("textDocument/publishDiagnostics", {
              uri: sfcUriOf(),
              version: 1, // STALE — below the edit's version 2
              diagnostics: [diag(1)],
            }),
          3,
        ),
    });
    await expect(editToDiagnosticsLatency("bin", corpus, { ops: 1 }, connect)).rejects.toThrow(
      /timed out/i,
    );
  });

  it("(i) a stale/queued EMPTY publish at a later iteration does NOT satisfy (times out)", async () => {
    // ops=2: i=0 settles its perf-0 error; i=1's publish is a stale/queued EMPTY set
    // lacking THIS edit's unique token. The odd "clear" iteration must NOT be satisfied
    // by an empty publish (absence treated as proof) — every iteration is a present-
    // with-unique-token transition, so an empty publish cannot satisfy and it times out.
    const connect = connectWith({
      onDidOpen: settleOpen(),
      onDidChange: (params, emit) => {
        const text = editTextOf(params);
        const diags = text.includes("perf-0")
          ? [diag(1, `Type '0' is not assignable to type '"perf-0"'.`)]
          : text.includes("perf-base")
            ? [diag(1, `Type '0' is not assignable to type '"perf-base"'.`)]
            : []; // i=1 (the odd "clear" edit): a stale/queued EMPTY publish
        setTimeout(
          () => emit("textDocument/publishDiagnostics", { uri: sfcUriOf(), diagnostics: diags }),
          3,
        );
      },
    });
    await expect(editToDiagnosticsLatency("bin", corpus, { ops: 2 }, connect)).rejects.toThrow(
      /timed out/i,
    );
  });

  it("(ii) a publish carrying a WRONG (not this edit's) token at a later iteration does NOT satisfy", async () => {
    // ops=2: i=0 settles perf-0; i=1's publish carries a DIFFERENT non-empty token
    // (perf-99), not THIS edit's. The odd "clear" iteration must NOT accept any
    // token-lacking publish — i=1 binds to its own unique token, so a wrong-token
    // publish times out.
    const connect = connectWith({
      onDidOpen: settleOpen(),
      onDidChange: (params, emit) => {
        const text = editTextOf(params);
        const diags = text.includes("perf-0")
          ? [diag(1, `Type '0' is not assignable to type '"perf-0"'.`)]
          : text.includes("perf-base")
            ? [diag(1, `Type '0' is not assignable to type '"perf-base"'.`)]
            : [diag(2, `Type '0' is not assignable to type '"perf-99"'.`)]; // wrong token
        setTimeout(
          () => emit("textDocument/publishDiagnostics", { uri: sfcUriOf(), diagnostics: diags }),
          3,
        );
      },
    });
    await expect(editToDiagnosticsLatency("bin", corpus, { ops: 2 }, connect)).rejects.toThrow(
      /timed out/i,
    );
  });
});

describe("warmDependencyEditLatency binds each warm cross-file edit to THIS edit (unique token + non-empty baseline)", () => {
  // Each measured edit re-points the dependent's inline `import("./types").WarmDepProbe<K>`
  // annotation at the NEXT stable imported string-literal alias; the warm cross-file
  // re-resolution re-errors TS2322 echoing that alias's resolved literal (the per-edit
  // fingerprint). The dependent's OWN buffer is what changes per edit (so an open dependent
  // reliably re-publishes), and it settles a NON-EMPTY pre-edit baseline on open — a
  // queued/empty initial publish can never be timed as an edit's effect, and a publish
  // lacking THIS edit's token cannot satisfy the wait.
  const settleSfcOpen =
    (initial: unknown[]) =>
    (params: unknown, emit: (m: string, p: unknown) => void): void => {
      if (didOpenUri(params) === sfcUriOf()) {
        setTimeout(
          () => emit("textDocument/publishDiagnostics", { uri: sfcUriOf(), diagnostics: initial }),
          2,
        );
      }
    };

  it("hard-fails when the edit never re-publishes the dependent (no swallow)", async () => {
    const connect = connectWith({
      onDidOpen: settleSfcOpen([dep2322("perfDepBase")]), // a non-empty settled baseline
      onDidChange: () => {
        /* broken: never re-publishes ⇒ the measured wait must hard-fail */
      },
    });
    await expect(warmDependencyEditLatency("bin", corpus, { ops: 1 }, connect)).rejects.toThrow(
      /timed out/i,
    );
  });

  it("(i) an EMPTY pre-edit baseline + a queued publish does NOT satisfy (times out)", async () => {
    // The dependent never settles a NON-EMPTY pre-edit baseline (the open publishes empty),
    // so a queued post-edit publish can never be timed as the edit's effect — a non-empty
    // baseline is required, so it times out.
    const connect = connectWith({
      onDidOpen: settleSfcOpen([]), // EMPTY baseline ⇒ no non-empty settle
      onDidChange: (_p, emit) =>
        setTimeout(
          () =>
            emit("textDocument/publishDiagnostics", { uri: sfcUriOf(), diagnostics: [diag(2)] }),
          3,
        ),
    });
    await expect(warmDependencyEditLatency("bin", corpus, { ops: 1 }, connect)).rejects.toThrow(
      /timed out|baseline|non-empty/i,
    );
  });

  it("(ii) a publish LACKING this edit's unique token does NOT satisfy (times out)", async () => {
    // A non-empty baseline IS settled, but the post-edit publish carries a generic 2322
    // LACKING this edit's unique per-iteration token — the wait binds to THIS edit's unique
    // token, so a bare-code publish without it times out.
    const connect = connectWith({
      onDidOpen: settleSfcOpen([dep2322("perfDepBase")]), // non-empty baseline
      onDidChange: (_p, emit) =>
        setTimeout(
          // a publish that DIFFERS from the baseline but lacks the unique edit token
          () =>
            emit("textDocument/publishDiagnostics", { uri: sfcUriOf(), diagnostics: [diag(2)] }),
          3,
        ),
    });
    await expect(warmDependencyEditLatency("bin", corpus, { ops: 1 }, connect)).rejects.toThrow(
      /timed out/i,
    );
  });

  it("a STALE republish EQUAL to the non-empty pre-edit baseline does NOT satisfy (times out)", async () => {
    // The dependent re-publishes the SAME (base-token) set it settled before the edit
    // — a stale republish, not THIS edit's effect — so the set-difference rail rejects it.
    const connect = connectWith({
      onDidOpen: settleSfcOpen([dep2322("perfDepBase")]),
      onDidChange: (_p, emit) =>
        setTimeout(
          () =>
            emit("textDocument/publishDiagnostics", {
              uri: sfcUriOf(),
              diagnostics: [dep2322("perfDepBase")], // equal to the pre-edit baseline
            }),
          3,
        ),
    });
    await expect(warmDependencyEditLatency("bin", corpus, { ops: 1 }, connect)).rejects.toThrow(
      /timed out/i,
    );
  });

  it("(iii) a genuine edit producing the unique cross-file token over a non-empty baseline DOES satisfy", async () => {
    // Each edit re-points the dependent at a unique imported alias (`WarmDepProbeN`); the
    // dependent re-errors TS2322 echoing the resolved literal (`perfDepN`), DIFFERING from
    // the (non-empty) pre-edit baseline AND carrying the unique fingerprint ⇒ the wait resolves.
    const connect = connectWith({
      onDidOpen: settleSfcOpen([dep2322("perfDepBase")]),
      onDidChange: (params, emit) => {
        const token = warmTokenOf(editTextOf(params));
        if (token) {
          setTimeout(
            () =>
              emit("textDocument/publishDiagnostics", {
                uri: sfcUriOf(),
                diagnostics: [dep2322(token)],
              }),
            3,
          );
        }
      },
    });
    const s = await warmDependencyEditLatency("bin", corpus, { ops: 2 }, connect);
    expect(s.latencies.length).toBe(2);
    expect(s.affectedUrisMax).toBeGreaterThanOrEqual(1);
  });
});

describe("publishMatchesTransition binds to a TRUE per-edit transition (robust to unversioned publishes)", () => {
  const uri = "file:///x.vue";
  const rawDiag = (code: string, line: number, character: number, message: string): unknown => ({
    range: { start: { line, character } },
    code,
    message,
  });

  it("a code-only / version-only transition (no envelope) is REJECTED — the full envelope is required", () => {
    // After removing the code-only acceptance path, a transition lacking the full
    // envelope (preEditFingerprints + the unique editFingerprint) is a harness defect,
    // not a satisfiable publish — publishMatchesTransition THROWS rather than binding on
    // a coarse code/version match (a versioned code match must NOT return true here).
    const codeOnly = {
      code: "2322",
      expectPresent: true,
      minVersion: 5,
    } as unknown as DiagnosticTransition;
    expect(() =>
      publishMatchesTransition(
        { uri, version: 5, diagnostics: [rawDiag("2322", 1, 0, "boom perf-1")] },
        uri,
        codeOnly,
      ),
    ).toThrow(/envelope/i);
    // Control: a FULL-envelope transition that genuinely transitions DOES satisfy.
    const full: DiagnosticTransition = {
      code: "2322",
      expectPresent: true,
      minVersion: 5,
      preEditFingerprints: [],
      editFingerprint: { code: "2322", messageIncludes: "perf-1" },
    };
    expect(
      publishMatchesTransition(
        {
          uri,
          version: 5,
          diagnostics: [
            rawDiag("2322", 1, 0, `Type '"perf-1"' is not assignable to type 'number'.`),
          ],
        },
        uri,
        full,
      ),
    ).toBe(true);
  });

  it("VERSIONED + envelope: a NEW version whose set EQUALS the pre-edit set does NOT satisfy (no real transition)", () => {
    // A new document version alone is NOT proof of a transition: the toggled code
    // could already have been present pre-edit. The versioned path must NOT return
    // `codes.includes(code) === expectPresent` while IGNORING the envelope — that
    // would pass a versioned republish of the unchanged pre-edit state; the envelope
    // (set DIFFERS + the edit fingerprint) is required on the versioned path too.
    const pre = ["2322:3:5:Type 'string' is not assignable to type 'number'."];
    const t: DiagnosticTransition = {
      code: "2322",
      expectPresent: true,
      minVersion: 5,
      preEditFingerprints: pre,
      editFingerprint: { code: "2322", messageIncludes: "perf-7" },
    };
    const staleButNewVersion = {
      uri,
      version: 7, // at/above the floor
      diagnostics: [rawDiag("2322", 3, 5, "Type 'string' is not assignable to type 'number'.")],
    };
    expect(publishMatchesTransition(staleButNewVersion, uri, t)).toBe(false);
  });

  it("VERSIONED + envelope: a NEW version carrying the code but NOT this edit's unique fingerprint does NOT satisfy", () => {
    const t: DiagnosticTransition = {
      code: "2322",
      expectPresent: true,
      minVersion: 5,
      preEditFingerprints: [],
      editFingerprint: { code: "2322", messageIncludes: "perf-7" },
    };
    const wrongToken = {
      uri,
      version: 7,
      diagnostics: [rawDiag("2322", 9, 2, `Type '"perf-3"' is not assignable to type 'number'.`)],
    };
    expect(publishMatchesTransition(wrongToken, uri, t)).toBe(false);
  });

  it("VERSIONED + envelope: a NEW version that genuinely transitions AND carries the edit fingerprint satisfies", () => {
    const t: DiagnosticTransition = {
      code: "2322",
      expectPresent: true,
      minVersion: 5,
      preEditFingerprints: [],
      editFingerprint: { code: "2322", messageIncludes: "perf-7" },
    };
    const post = {
      uri,
      version: 7,
      diagnostics: [rawDiag("2322", 9, 2, `Type '"perf-7"' is not assignable to type 'number'.`)],
    };
    expect(publishMatchesTransition(post, uri, t)).toBe(true);
  });

  it("UNVERSIONED stale publish EQUAL to the pre-edit settled set does NOT satisfy (must time out)", () => {
    // The toggled code IS present (so the coarse code check passes), but the
    // published set is identical to the PRE-EDIT settled state: a queued pre-edit
    // republish, not THIS edit's effect. The code-only wait must NOT accept it —
    // it must NOT satisfy (so the measured wait times out).
    const pre = ["2322:3:5:Type 'string' is not assignable to type 'number'."];
    const t: DiagnosticTransition = {
      code: "2322",
      expectPresent: true,
      preEditFingerprints: pre,
      editFingerprint: { code: "2322", messageIncludes: "perf-7" },
    };
    const stale = {
      uri, // NO version field — verter_lsp publishes version: null
      diagnostics: [rawDiag("2322", 3, 5, "Type 'string' is not assignable to type 'number'.")],
    };
    expect(publishMatchesTransition(stale, uri, t)).toBe(false);
  });

  it("UNVERSIONED add: a publish that DIFFERS from pre-edit AND carries the unique fingerprint satisfies", () => {
    const t: DiagnosticTransition = {
      code: "2322",
      expectPresent: true,
      preEditFingerprints: [],
      editFingerprint: { code: "2322", messageIncludes: "perf-7" },
    };
    const post = {
      uri,
      diagnostics: [rawDiag("2322", 9, 2, `Type '"perf-7"' is not assignable to type 'number'.`)],
    };
    expect(publishMatchesTransition(post, uri, t)).toBe(true);
  });

  it("UNVERSIONED add: a publish that differs from pre-edit but LACKS this edit's fingerprint does NOT satisfy", () => {
    // The count rose with a TS2322 — but it is a DIFFERENT diagnostic (wrong
    // token), not the one THIS edit creates. A publish lacking the fingerprint
    // must not satisfy.
    const t: DiagnosticTransition = {
      code: "2322",
      expectPresent: true,
      preEditFingerprints: [],
      editFingerprint: { code: "2322", messageIncludes: "perf-7" },
    };
    const post = {
      uri,
      diagnostics: [rawDiag("2322", 9, 2, `Type '"perf-3"' is not assignable to type 'number'.`)],
    };
    expect(publishMatchesTransition(post, uri, t)).toBe(false);
  });

  it("UNVERSIONED remove: clearing the fingerprint (and differing from pre-edit) satisfies; still carrying it does NOT", () => {
    const pre = [`2322:9:2:Type '"perf-6"' is not assignable to type 'number'.`];
    const t: DiagnosticTransition = {
      code: "2322",
      expectPresent: false,
      preEditFingerprints: pre,
      editFingerprint: { code: "2322", messageIncludes: "perf-6" },
    };
    expect(publishMatchesTransition({ uri, diagnostics: [] }, uri, t)).toBe(true);
    const stillPresent = {
      uri,
      diagnostics: [rawDiag("2322", 9, 2, `Type '"perf-6"' is not assignable to type 'number'.`)],
    };
    // Equal to pre-edit AND still carrying the fingerprint ⇒ no transition.
    expect(publishMatchesTransition(stillPresent, uri, t)).toBe(false);
  });

  it("UNVERSIONED: a transition without the full envelope THROWS (cannot be bound to the edit)", () => {
    // An unversioned publish without the envelope must NOT return false via a
    // code-only fallback — the missing envelope is a harness defect that throws,
    // version-independently — every measured transition must carry the full envelope.
    const t = { code: "2322", expectPresent: true } as unknown as DiagnosticTransition;
    const post = {
      uri,
      diagnostics: [rawDiag("2322", 9, 2, `Type '"perf-7"' is not assignable to type 'number'.`)],
    };
    expect(() => publishMatchesTransition(post, uri, t)).toThrow(/envelope/i);
  });

  it("rejects a publish for a DIFFERENT uri", () => {
    const t: DiagnosticTransition = {
      code: "2322",
      expectPresent: true,
      preEditFingerprints: [],
      editFingerprint: { code: "2322" },
    };
    expect(publishMatchesTransition({ uri: "file:///other.vue", diagnostics: [] }, uri, t)).toBe(
      false,
    );
  });
});

describe("DiagnosticsBus.diagnosticSet() carries the FULL cross-side correctness identity", () => {
  const rawDiag = (
    line: number,
    character: number,
    code: string,
    severity: number,
    message: string,
  ): unknown => ({ range: { start: { line, character } }, code, message, severity });
  const setFor = (d: unknown): string[] => {
    const client = new FakeLspClient();
    const bus = new DiagnosticsBus(client, dir);
    client.emit("textDocument/publishDiagnostics", { uri: sfcUriOf(), diagnostics: [d] });
    const set = bus.diagnosticSet();
    bus.dispose();
    return set;
  };

  it("distinguishes two diagnostics that differ ONLY in MESSAGE (same path/line/char/code/severity)", () => {
    // The key must include the message, not just `path:line:char:code` — with a
    // code-only key a candidate that changes the diagnostic TEXT at the same site
    // would pass cross-side correctness equality.
    const a = setFor(rawDiag(3, 5, "2322", 1, "Type 'string' is not assignable to type 'number'."));
    const b = setFor(
      rawDiag(3, 5, "2322", 1, "Type 'boolean' is not assignable to type 'number'."),
    );
    expect(a).not.toEqual(b);
  });

  it("distinguishes two diagnostics that differ ONLY in SEVERITY (same path/line/char/code/message)", () => {
    // Severity must be part of the key — dropping it would let an error→warning flip
    // at the same site pass correctness equality. (LSP 1=Error, 2=Warning.)
    const err = setFor(rawDiag(3, 5, "2322", 1, "same message"));
    const warn = setFor(rawDiag(3, 5, "2322", 2, "same message"));
    expect(err).not.toEqual(warn);
  });

  it("the set element key carries path:line:char:code:severity:message (full identity, severity mapped to TS vocab)", () => {
    const set = setFor(rawDiag(7, 2, "2322", 1, "boom message"));
    expect(set).toEqual(["app/Comp.vue:7:2:2322:error:boom message"]);
  });

  it("normalizes the message run-stable (collapses a per-side carrier hash so cross-side sets stay logical)", () => {
    // The candidate + baseline run in distinct working trees; a message embedding a
    // per-side carrier hash must collapse so the logical diagnostic set is equal.
    const a = setFor(rawDiag(1, 0, "2304", 1, "Cannot find name in 'Foo_a1b2c3d4.vue.ts'."));
    const b = setFor(rawDiag(1, 0, "2304", 1, "Cannot find name in 'Foo_99887766.vue.ts'."));
    expect(a).toEqual(b);
  });

  it("two identical diagnostics still compare EQUAL (the positive control)", () => {
    const a = setFor(rawDiag(3, 5, "2322", 1, "identical"));
    const b = setFor(rawDiag(3, 5, "2322", 1, "identical"));
    expect(a).toEqual(b);
  });
});

describe("waitForReady fails closed on a missing $/verter/ready when required", () => {
  it("REJECTS when ready never arrives and required=true (full-gate path)", async () => {
    const client = new FakeLspClient({}); // never emits $/verter/ready
    await expect(waitForReady(client, 30, true)).rejects.toThrow(/ready/i);
  });

  it("RESOLVES (explicit fallback) when ready never arrives and required=false", async () => {
    const client = new FakeLspClient({});
    await expect(waitForReady(client, 30, false)).resolves.toBeUndefined();
  });

  it("RESOLVES when ready IS emitted", async () => {
    const client = new FakeLspClient({});
    setTimeout(() => client.emit("$/verter/ready", {}), 3);
    await expect(waitForReady(client, 30, true)).resolves.toBeUndefined();
  });
});

describe("assertTypeProviderTsgo requires an active tsgo provider before a measured run", () => {
  // A measured LSP workload MUST run against the tsgo type engine. The server emits
  // `$/verter/typeProviderStatus` { kind: "tsgo" | "tsserver" | "none", reason? }
  // during `initialized` (crates/verter_lsp/src/server/lifecycle.rs). Without this
  // assertion a server that fell back to verter-only mode (kind:"none") would let the
  // workload silently SKIP instead of failing loud — the gate would read no TS
  // diagnostics / empty hover as "fast", not as "no engine".
  it("RESOLVES when the active provider is tsgo (the measured-path happy case)", () => {
    expect(() => assertTypeProviderTsgo({ kind: "tsgo" }, true)).not.toThrow();
  });

  it("REJECTS kind:'none' BEFORE measurement and surfaces the server-provided reason", () => {
    // The discriminating case: pre-assertion, a verter-only server (no tsgo) silently
    // produced no TS diagnostics and the workload skipped. Now it fails loud, naming
    // the engine it refused to measure without AND echoing the server's reason.
    expect(() =>
      assertTypeProviderTsgo({ kind: "none", reason: "tsgo binary not found" }, true),
    ).toThrow(/tsgo binary not found/);
    expect(() => assertTypeProviderTsgo({ kind: "none", reason: "x" }, true)).toThrow(/tsgo/i);
  });

  it("REJECTS a MISSING status (no $/verter/typeProviderStatus received) on the required path", () => {
    expect(() => assertTypeProviderTsgo(undefined, true)).toThrow(/tsgo|type provider/i);
    expect(() => assertTypeProviderTsgo(null, true)).toThrow(/tsgo|type provider/i);
    expect(() => assertTypeProviderTsgo({}, true)).toThrow(/tsgo|type provider/i);
    expect(() => assertTypeProviderTsgo({ kind: 7 }, true)).toThrow(/tsgo|type provider/i);
  });

  it("REJECTS a WRONG provider (kind:'tsserver') on the required path", () => {
    expect(() => assertTypeProviderTsgo({ kind: "tsserver" }, true)).toThrow(/tsserver/);
    expect(() => assertTypeProviderTsgo({ kind: "tsserver" }, true)).toThrow(/tsgo/i);
  });

  it("TOLERATES a non-tsgo provider when required=false (the explicit smoke/self-check fallback, like waitForReady)", () => {
    expect(() => assertTypeProviderTsgo({ kind: "none", reason: "x" }, false)).not.toThrow();
    expect(() => assertTypeProviderTsgo(undefined, false)).not.toThrow();
    expect(() => assertTypeProviderTsgo({ kind: "tsserver" }, false)).not.toThrow();
  });
});

describe("captureTypeProviderStatus records the provider status the assertion reads", () => {
  it("captures the emitted $/verter/typeProviderStatus so a tsgo status PASSES", () => {
    const client = new FakeLspClient({});
    const cap = captureTypeProviderStatus(client);
    client.emit("$/verter/typeProviderStatus", { kind: "tsgo" });
    expect(() => assertTypeProviderTsgo(cap.current(), true)).not.toThrow();
    cap.dispose();
  });

  it("a captured kind:'none' status REJECTS before measurement with the reason", () => {
    const client = new FakeLspClient({});
    const cap = captureTypeProviderStatus(client);
    client.emit("$/verter/typeProviderStatus", { kind: "none", reason: "engine version mismatch" });
    expect(() => assertTypeProviderTsgo(cap.current(), true)).toThrow(/engine version mismatch/);
    cap.dispose();
  });

  it("no status emitted ⇒ current() is undefined ⇒ the required assertion REJECTS (the silent-skip the gate must catch)", () => {
    const client = new FakeLspClient({});
    const cap = captureTypeProviderStatus(client);
    expect(cap.current()).toBeUndefined();
    expect(() => assertTypeProviderTsgo(cap.current(), true)).toThrow();
    cap.dispose();
  });

  it("dispose() detaches the handler so a later emit does not mutate the captured status", () => {
    const client = new FakeLspClient({});
    const cap = captureTypeProviderStatus(client);
    client.emit("$/verter/typeProviderStatus", { kind: "tsgo" });
    cap.dispose();
    client.emit("$/verter/typeProviderStatus", { kind: "none", reason: "late" });
    expect(() => assertTypeProviderTsgo(cap.current(), true)).not.toThrow();
  });
});

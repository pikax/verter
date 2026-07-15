import { describe, expect, it } from "vitest";

import {
  CollectingSink,
  EditBuffer,
  classifyHoverSample,
  collectHover,
  type CollectorLspClient,
  type HoverSampleInput,
} from "../src/collectors/index.js";
import type { CollectorEventKey } from "../src/collectors/index.js";
import { normalizeHover } from "../src/index.js";
import type { NormalizedHover, Probe } from "../src/index.js";

const key: CollectorEventKey = {
  scenario: "minimal-member-access",
  editStepIndex: 0,
  driver: "rawLsp",
  provider: "tsgo",
  probe: "hover-on-ref",
  version: 1,
  anchor: "ref",
};

const fenced = (label: string): string => "```typescript\n" + label + "\n```";

const sample = (over: Partial<HoverSampleInput> = {}): HoverSampleInput => ({
  key,
  verter: normalizeHover({ contents: fenced("const count: number") }),
  ...over,
});

describe("classifyHoverSample — synthetic regions are tolerated, real misses are user-visible", () => {
  it("tolerates a CONTENTLESS hover in a synthetic region (NOT a failure)", () => {
    const events = classifyHoverSample(
      sample({ verter: null, syntheticRegion: true, baseline: { provider: "tsgo", hover: null } }),
    );
    expect(events.every((e) => e.ok)).toBe(true);
    expect(events.some((e) => e.signal === "hover_synthetic_region_tolerated")).toBe(true);
  });

  it("does NOT flag a synthetic-region miss even when a baseline HAS content", () => {
    const baseline: { provider: string; hover: NormalizedHover | null } = {
      provider: "tsgo",
      hover: { contents: fenced("const count: number") },
    };
    const events = classifyHoverSample(sample({ verter: null, syntheticRegion: true, baseline }));
    expect(events.every((e) => e.ok)).toBe(true);
  });

  it("flags a REAL-region presence mismatch (verter empty, baseline has content) as user-visible", () => {
    const baseline: { provider: string; hover: NormalizedHover | null } = {
      provider: "tsgo",
      hover: { contents: fenced("const count: number") },
    };
    const events = classifyHoverSample(sample({ verter: null, syntheticRegion: false, baseline }));
    const fail = events.filter((e) => !e.ok);
    expect(fail.length).toBeGreaterThan(0);
    expect(fail.some((e) => e.severity === "userVisible")).toBe(true);
    expect(fail.some((e) => (e.data as { class?: string }).class === "hoverPresenceMismatch")).toBe(
      true,
    );
  });

  it("agrees when verter and the baseline carry the same type label", () => {
    const baseline: { provider: string; hover: NormalizedHover | null } = {
      provider: "tsgo",
      hover: { contents: fenced("const count: number") },
    };
    const events = classifyHoverSample(sample({ baseline }));
    expect(events.every((e) => e.ok)).toBe(true);
  });
});

describe("classifyHoverSample — direct Vue-surface invariants", () => {
  it("flags an `excludes` invariant violated by the label (e.g. onClick leaks into a @click hover)", () => {
    const events = classifyHoverSample(
      sample({
        verter: normalizeHover({ contents: fenced("(property) onClick: () => void") }),
        invariants: [
          { id: "no-onclick", assertion: "excludes", value: "onClick" },
          { id: "has-click", assertion: "contains", value: "Click" },
        ],
      }),
    );
    const violations = events.filter((e) => e.signal === "hover_invariant" && !e.ok);
    expect(violations).toHaveLength(1);
    expect(violations[0].severity).toBe("userVisible");
    expect((violations[0].data as { invariant?: string }).invariant).toBe("no-onclick");
  });

  it("passes invariants that hold and does not invent failures", () => {
    const events = classifyHoverSample(
      sample({
        verter: normalizeHover({ contents: fenced("@click handler: (e: MouseEvent) => void") }),
        invariants: [{ id: "has-click", assertion: "contains", value: "@click" }],
      }),
    );
    expect(events.filter((e) => e.signal === "hover_invariant" && !e.ok)).toHaveLength(0);
  });
});

describe("classifyHoverSample — required snippets without a baseline", () => {
  it("flags an absent required snippet and passes a present one", () => {
    const events = classifyHoverSample(sample({ requiredSnippets: ["number", "string"] }));
    const fail = events.filter((e) => !e.ok);
    expect(fail).toHaveLength(1);
    expect((fail[0].data as { snippet?: string }).snippet).toBe("string");
  });
});

const oracleProbe: Probe = {
  id: "hover-oracle",
  method: "hover",
  anchor: "ref",
  mappingPolicy: "none",
  confidence: "high",
  dimension: "vueSemanticValidity",
  requiresSourceMap: false,
  requiredDrivers: [],
  capabilityRequirements: [],
};

describe("classifyHoverSample — curated vue-semantic-validity oracle (shared classifyOracleHover)", () => {
  it("emits a vueSemanticValidity divergence when verter's label disagrees with the .ts oracle", () => {
    const events = classifyHoverSample(
      sample({
        verter: normalizeHover({ contents: fenced("const count: string") }), // wrong type
        oracle: {
          probe: oracleProbe,
          providers: { tsgo: { ok: true, output: { contents: fenced("const count: number") } } },
        },
      }),
    );
    const fail = events.filter((e) => e.signal === "hover_vue_semantic_validity" && !e.ok);
    expect(fail).toHaveLength(1);
    expect(fail[0].severity).toBe("userVisible");
    expect((fail[0].data as { class?: string }).class).toBe("typeLabelMismatch");
  });

  it("emits an ok vueSemanticValidity event when verter agrees with the oracle", () => {
    const events = classifyHoverSample(
      sample({
        oracle: {
          probe: oracleProbe,
          providers: { tsgo: { ok: true, output: { contents: fenced("const count: number") } } },
        },
      }),
    );
    const oracleEvents = events.filter((e) => e.signal === "hover_vue_semantic_validity");
    expect(oracleEvents).toHaveLength(1);
    expect(oracleEvents[0].ok).toBe(true);
  });
});

describe("classifyHoverSample — every probe emits a keyed sample (no empty event list)", () => {
  it("emits exactly one ok:true hover_observed for a contentful hover with no oracle inputs", () => {
    // No baseline, no required snippets, no invariants: a successful hover is still a
    // recorded observation — it must NOT classify to an empty event list.
    const events = classifyHoverSample(sample());
    expect(events).toHaveLength(1);
    expect(events[0].signal).toBe("hover_observed");
    expect(events[0].ok).toBe(true);
  });

  it("emits a distinct ok keyed event for a contentless NON-synthetic hover with no oracle", () => {
    const events = classifyHoverSample(sample({ verter: null }));
    expect(events).toHaveLength(1);
    expect(events[0].signal).toBe("hover_contentless_observed");
    expect(events[0].ok).toBe(true);
  });
});

/**
 * A fake LSP client returning ONE canned hover for the document, so the LIVE
 * {@link collectHover} driver — its open/sample loop AND the oracle option it threads
 * into {@link classifyHoverSample} — is verifiable without spawning a server.
 */
class FakeHoverClient implements CollectorLspClient {
  readonly positionEncoding = "utf-16" as const;
  readonly serverCapabilities = {};
  readonly stderr = { text: (): string => "" };
  constructor(private readonly hoverContents: string) {}
  async sendRequest<T = unknown>(method: string): Promise<T> {
    if (method === "textDocument/hover") {
      return { contents: "```typescript\n" + this.hoverContents + "\n```" } as T;
    }
    return null as T;
  }
  sendNotification(): void {}
  onNotification(): void {}
  offNotification(): void {}
}

const HOVER_VUE = "const count = 1\n";
const identAnchor = { ident: "const ".length }; // the `count` declaration identifier

describe("collectHover — the live oracle option threads the curated .ts oracle into the sample", () => {
  it("emits an ok hover_vue_semantic_validity when the live hover agrees with the supplied oracle", async () => {
    const client = new FakeHoverClient("const count: number");
    const sink = new CollectingSink();
    await collectHover({
      client,
      sink,
      uri: "file:///probe.vue",
      buffer: new EditBuffer(HOVER_VUE, identAnchor),
      scenario: "hermetic",
      probe: "hover",
      anchor: "ident",
      provider: "tsgo",
      oracle: {
        probe: oracleProbe,
        providers: { tsgo: { ok: true, output: { contents: fenced("const count: number") } } },
        requiredSnippets: ["number"],
      },
    });
    const oracleEvents = sink.events.filter((e) => e.signal === "hover_vue_semantic_validity");
    expect(oracleEvents).toHaveLength(1);
    expect(oracleEvents[0].ok).toBe(true);
  });

  it("emits a hover_vue_semantic_validity DIVERGENCE when the live hover disagrees with the oracle", async () => {
    // verter's live hover says `string`; the curated `.ts` oracle says `number`.
    const client = new FakeHoverClient("const count: string");
    const sink = new CollectingSink();
    await collectHover({
      client,
      sink,
      uri: "file:///probe.vue",
      buffer: new EditBuffer(HOVER_VUE, identAnchor),
      scenario: "hermetic",
      probe: "hover",
      anchor: "ident",
      provider: "tsgo",
      oracle: {
        probe: oracleProbe,
        providers: { tsgo: { ok: true, output: { contents: fenced("const count: number") } } },
      },
    });
    const fail = sink.events.filter((e) => e.signal === "hover_vue_semantic_validity" && !e.ok);
    expect(fail).toHaveLength(1);
    expect((fail[0].data as { class?: string }).class).toBe("typeLabelMismatch");
  });

  it("does NOT emit hover_vue_semantic_validity when no oracle is supplied (no spurious emission)", async () => {
    const client = new FakeHoverClient("const count: number");
    const sink = new CollectingSink();
    await collectHover({
      client,
      sink,
      uri: "file:///probe.vue",
      buffer: new EditBuffer(HOVER_VUE, identAnchor),
      scenario: "hermetic",
      probe: "hover",
      anchor: "ident",
      provider: "tsgo",
    });
    expect(sink.events.some((e) => e.signal === "hover_vue_semantic_validity")).toBe(false);
    // The bare observation is still recorded (the live hover had content).
    expect(sink.events.some((e) => e.signal === "hover_observed")).toBe(true);
  });
});

import { describe, it, expect } from "vitest";
import {
  compareMeta,
  normalizeTypeString,
  generateMetaReport,
  type MetaComparisonResult,
  type MetaBenchmarkReport,
} from "./meta-bench-utils.js";

describe("normalizeTypeString", () => {
  it("trims whitespace", () => {
    expect(normalizeTypeString("  string  ")).toBe("string");
  });

  it("normalizes internal whitespace", () => {
    expect(normalizeTypeString("string  |  number")).toBe("string | number");
  });

  it("normalizes undefined to ?", () => {
    expect(normalizeTypeString("string | undefined")).toBe("string | undefined");
  });

  it("handles empty/null/undefined", () => {
    expect(normalizeTypeString("")).toBe("");
    expect(normalizeTypeString(undefined as any)).toBe("");
    expect(normalizeTypeString(null as any)).toBe("");
  });
});

describe("compareMeta", () => {
  it("returns match when props names align", () => {
    const verter = {
      props: [
        { name: "title", type: "string", required: true },
        { name: "count", type: "number", required: false },
      ],
      events: [],
      slots: [],
    };
    const volar = {
      props: [
        { name: "title", type: "string", required: true },
        { name: "count", type: "number", required: false },
      ],
      events: [],
      slots: [],
    };
    const result = compareMeta(verter, volar);
    expect(result.props.status).toBe("match");
    expect(result.props.missing).toEqual([]);
    expect(result.props.extra).toEqual([]);
    expect(result.overall).toBe("match");
  });

  it("detects missing props", () => {
    const verter = {
      props: [{ name: "title", type: "string", required: true }],
      events: [],
      slots: [],
    };
    const volar = {
      props: [
        { name: "title", type: "string", required: true },
        { name: "count", type: "number", required: false },
      ],
      events: [],
      slots: [],
    };
    const result = compareMeta(verter, volar);
    expect(result.props.status).toBe("mismatch");
    expect(result.props.missing).toEqual(["count"]);
    expect(result.overall).toBe("mismatch");
  });

  it("detects extra props", () => {
    const verter = {
      props: [
        { name: "title", type: "string", required: true },
        { name: "extra", type: "boolean", required: false },
      ],
      events: [],
      slots: [],
    };
    const volar = {
      props: [{ name: "title", type: "string", required: true }],
      events: [],
      slots: [],
    };
    const result = compareMeta(verter, volar);
    expect(result.props.status).toBe("mismatch");
    expect(result.props.extra).toEqual(["extra"]);
  });

  it("compares events", () => {
    const verter = {
      props: [],
      events: [{ name: "click", type: "(e: MouseEvent) => void" }],
      slots: [],
    };
    const volar = {
      props: [],
      events: [{ name: "click", type: "(e: MouseEvent) => void" }],
      slots: [],
    };
    const result = compareMeta(verter, volar);
    expect(result.events.status).toBe("match");
  });

  it("compares slots", () => {
    const verter = {
      props: [],
      events: [],
      slots: [{ name: "default" }, { name: "header" }],
    };
    const volar = {
      props: [],
      events: [],
      slots: [{ name: "default" }],
    };
    const result = compareMeta(verter, volar);
    expect(result.slots.status).toBe("mismatch");
    expect(result.slots.extra).toEqual(["header"]);
  });

  it("overall is match only when all categories match", () => {
    const verter = {
      props: [{ name: "a", type: "string", required: false }],
      events: [{ name: "click", type: "void" }],
      slots: [{ name: "default" }],
    };
    const volar = {
      props: [{ name: "a", type: "string", required: false }],
      events: [{ name: "click", type: "void" }],
      slots: [{ name: "default" }],
    };
    const result = compareMeta(verter, volar);
    expect(result.overall).toBe("match");
    expect(result.props.status).toBe("match");
    expect(result.events.status).toBe("match");
    expect(result.slots.status).toBe("match");
  });

  it("type mismatch is flagged but does not cause overall mismatch", () => {
    const verter = {
      props: [{ name: "x", type: "string", required: true }],
      events: [],
      slots: [],
    };
    const volar = {
      props: [{ name: "x", type: "String", required: true }],
      events: [],
      slots: [],
    };
    const result = compareMeta(verter, volar);
    // Names match, so structural comparison passes; type diffs are warnings
    expect(result.props.status).toBe("match");
    expect(result.props.typeDiffs.length).toBe(1);
    expect(result.props.typeDiffs[0]).toEqual({ name: "x", verter: "string", volar: "String" });
  });
});

describe("generateMetaReport", () => {
  it("generates valid report structure", () => {
    const perFile: Array<{
      fixture: string;
      verterMs: number;
      volarMs: number;
      comparison: MetaComparisonResult;
    }> = [
      {
        fixture: "single-prop",
        verterMs: 0.5,
        volarMs: 5.0,
        comparison: {
          props: { status: "match", missing: [], extra: [], typeDiffs: [] },
          events: { status: "match", missing: [], extra: [], typeDiffs: [] },
          slots: { status: "match", missing: [], extra: [], typeDiffs: [] },
          overall: "match",
        },
      },
    ];

    const report = generateMetaReport(perFile);
    expect(report.fixtures).toHaveLength(1);
    expect(report.fixtures[0].fixture).toBe("single-prop");
    expect(report.fixtures[0].speedup).toBe(10);
    expect(report.summary.totalFixtures).toBe(1);
    expect(report.summary.allCorrect).toBe(true);
    expect(report.timestamp).toBeDefined();
    // Should NOT have undefined fields
    expect(report.summary.avgSpeedup).not.toBeNaN();
  });

  it("flags incorrect when comparison has mismatch", () => {
    const perFile = [
      {
        fixture: "broken",
        verterMs: 1,
        volarMs: 2,
        comparison: {
          props: { status: "mismatch" as const, missing: ["x"], extra: [], typeDiffs: [] },
          events: { status: "match" as const, missing: [], extra: [], typeDiffs: [] },
          slots: { status: "match" as const, missing: [], extra: [], typeDiffs: [] },
          overall: "mismatch" as const,
        },
      },
    ];

    const report = generateMetaReport(perFile);
    expect(report.summary.allCorrect).toBe(false);
    expect(report.fixtures[0].correct).toBe(false);
  });
});

import { describe, it, expect } from "vitest";
import {
  parseCompileAttribution,
  aggregateAttribution,
  type OverheadAttribution,
} from "./audit-attribution.js";

/** A complete `RequestAuditRecord` Buffer, with field overrides for the gaps. */
function recordBuf(over: {
  payload?: Record<string, unknown>;
  timings?: Record<string, unknown> | null;
  memory?: Record<string, unknown> | null;
}): Buffer {
  const payload = {
    kind: "Compile",
    target: "Ide",
    parse_ms: 1,
    transform_ms: 2,
    codegen_ms: 3,
    css_analysis_ms: 0,
    sourcemap_ms: 4,
    output_bytes: "100",
    sourcemap_bytes: "50",
    num_directives: 0,
    num_components: 0,
    num_style_blocks: 0,
    num_script_blocks: 1,
    code_transform_ops: 7,
    ...over.payload,
  };
  const rec: Record<string, unknown> = { kind: "Compile", kind_payload: payload };
  if (over.timings !== null) {
    rec.timings = over.timings ?? {
      total_ms: 20,
      capture_inputs_ms: 1,
      store_read_ms: 1,
      store_merge_ms: 1,
      serialize_ms: 1,
    };
  }
  if (over.memory !== null) {
    rec.memory = over.memory ?? {
      process_rss_peak_bytes: "1048576",
      process_rss_after_bytes: "0",
      process_rss_delta_bytes: "0",
      bytes_parsed: "0",
    };
  }
  return Buffer.from(JSON.stringify(rec), "utf-8");
}

describe("parseCompileAttribution surfaces MISSING gated fields as null (never 0)", () => {
  it("parses a complete record into real numbers", () => {
    const a = parseCompileAttribution(recordBuf({}))!;
    expect(a.codegenMs).toBe(3);
    expect(a.sourcemapMs).toBe(4);
    expect(a.codegenSourcemapMs).toBe(3 + 4); // the present codegen+source-map emit aggregate
    expect(a.parseTransformTransportMs).toBe(1 + 2 + (1 + 1 + 1 + 1)); // parse+transform+transport
    expect(a.nonCheckerMs).toBe(3 + 4 + 1 + 2 + 4);
    expect(a.sourceMapBytes).toBe(50);
    expect(a.peakRssBytes).toBe(1048576);
  });

  it("a missing codegen_ms makes codegenSourcemapMs + nonCheckerMs NULL — never 0 (no undercounted sum)", () => {
    // A `null` codegen_ms must propagate as null, never coerce to 0 — coercing it
    // would let nonCheckerMs sum to a smaller-but-present number (a false
    // lower-is-better win) instead of the honest null.
    const a = parseCompileAttribution(recordBuf({ payload: { codegen_ms: null } }))!;
    expect(a.codegenMs).toBeNull();
    expect(a.codegenSourcemapMs).toBeNull();
    expect(a.nonCheckerMs).toBeNull();
  });

  it("codegenSourcemapMs stays PRESENT when only parse/transform/transport is missing (the measurable sub-signal)", () => {
    // An absent timings record sinks parseTransformTransportMs (and therefore the
    // FULL nonCheckerMs aggregate) to null — but the codegen + source-map emit
    // phases are still measured, so the codegen-time sub-signal the axis-A gate
    // reads survives. This is exactly why axis A gates codegenSourcemapMs, not the
    // full non-checker aggregate.
    const a = parseCompileAttribution(recordBuf({ timings: null }))!;
    expect(a.parseTransformTransportMs).toBeNull();
    expect(a.nonCheckerMs).toBeNull();
    expect(a.codegenSourcemapMs).toBe(3 + 4); // codegen + source-map remain present
  });

  it("a malformed/missing sourcemap_bytes yields NULL sourceMapBytes — never 0", () => {
    const a = parseCompileAttribution(recordBuf({ payload: { sourcemap_bytes: "not-a-number" } }))!;
    expect(a.sourceMapBytes).toBeNull();
    const b = parseCompileAttribution(recordBuf({ payload: { sourcemap_bytes: undefined } }))!;
    expect(b.sourceMapBytes).toBeNull();
  });

  it("an absent / zero audit RSS yields NULL peakRssBytes — never 0", () => {
    const a = parseCompileAttribution(recordBuf({ memory: null }))!;
    expect(a.peakRssBytes).toBeNull();
    const z = parseCompileAttribution(recordBuf({ memory: { process_rss_peak_bytes: "0" } }))!;
    expect(z.peakRssBytes).toBeNull();
  });
});

describe("aggregateAttribution propagates nullness (a single missing contributor ⇒ null)", () => {
  const full = (over: Partial<OverheadAttribution> = {}): OverheadAttribution => ({
    codegenMs: 1,
    sourcemapMs: 1,
    codegenSourcemapMs: 2,
    parseTransformTransportMs: 1,
    nonCheckerMs: 3,
    outputBytes: 10,
    sourceMapBytes: 5,
    codeTransformOps: 2,
    peakRssBytes: 100,
    ...over,
  });

  it("sums when every contributor has the field", () => {
    const agg = aggregateAttribution([full(), full()]);
    expect(agg.nonCheckerMs).toBe(6);
    expect(agg.codegenSourcemapMs).toBe(4); // 2 + 2
    expect(agg.sourceMapBytes).toBe(10);
    expect(agg.peakRssBytes).toBe(100); // max
  });

  it("ONE contributor missing nonCheckerMs ⇒ aggregate nonCheckerMs is null (no partial sum)", () => {
    const agg = aggregateAttribution([full(), full({ nonCheckerMs: null })]);
    expect(agg.nonCheckerMs).toBeNull();
    // sourceMapBytes still present on both ⇒ still summed.
    expect(agg.sourceMapBytes).toBe(10);
  });

  it("peakRssBytes is the MAX of PRESENT contributors — a single null contributor does NOT null the batch peak", () => {
    // peakRssBytes is the deliberate exception to null-propagation: it is a PEAK,
    // so a missing per-compile RSS is ignored and the max of the present ones is
    // kept (unlike the summed fields, where ONE null sinks the aggregate). It is
    // deferred from the axis-A gated set, so this never feeds a gate decision.
    const agg = aggregateAttribution([full({ peakRssBytes: 100 }), full({ peakRssBytes: null })]);
    expect(agg.peakRssBytes).toBe(100);
    // A summed field on the SAME inputs DOES propagate the null contributor.
    const aggSummed = aggregateAttribution([full(), full({ sourceMapBytes: null })]);
    expect(aggSummed.sourceMapBytes).toBeNull();
    const agg2 = aggregateAttribution([full({ peakRssBytes: 100 }), full({ peakRssBytes: 250 })]);
    expect(agg2.peakRssBytes).toBe(250);
  });

  it("peakRssBytes is null ONLY when NO contributor reported RSS", () => {
    const agg = aggregateAttribution([full({ peakRssBytes: null }), full({ peakRssBytes: null })]);
    expect(agg.peakRssBytes).toBeNull();
  });
});

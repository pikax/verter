import { describe, expect, it } from "vitest";
import {
  buildSummary,
  formatValidationResult,
  loadTraceSpec,
  normalizePath,
  parseCoreEvent,
  parseTraceLog,
  parseTraceLine,
  validateTrace,
  type TraceSpec,
} from "./trace-validator.js";

// ── Parser tests ──────────────────────────────────────────────────────

describe("parseTraceLine", () => {
  it("parses a start event", () => {
    const line = `[verter-meta-trace] event=start trace=6 span=61 parent=36 request=6 subrequest=61 caller=36 depth=2 thread=ThreadId(1) name="compute_evaluated_types_expand_macros" detail="owner=foo.vue macros=4 bindings=0 store_view=true"`;
    const e = parseTraceLine(line);
    expect(e).not.toBeNull();
    expect(e!.type).toBe("start");
    expect(e!.name).toBe("compute_evaluated_types_expand_macros");
    expect(e!.detail).toBe("owner=foo.vue macros=4 bindings=0 store_view=true");
    expect(e!.depth).toBe(2);
    expect(e!.durMs).toBeUndefined();
  });

  it("parses an end event with duration", () => {
    const line = `[verter-meta-trace] event=end trace=6 span=61 parent=36 request=6 subrequest=61 caller=36 depth=2 thread=ThreadId(1) name="compute_evaluated_types_expand_macros" detail="owner=foo.vue macros=4 bindings=0 store_view=true" dur_ms=1670.263`;
    const e = parseTraceLine(line);
    expect(e).not.toBeNull();
    expect(e!.type).toBe("end");
    expect(e!.durMs).toBeCloseTo(1670.263);
  });

  it("parses a point event", () => {
    const line = `[verter-meta-trace] event=point trace=6 span=9 parent=8 request=6 subrequest=9 caller=8 depth=3 thread=ThreadId(1) name="authoritative_import_route_in_view_result" detail="owner=foo.vue import=reka-ui source=module_facts target=bar.d.ts"`;
    const e = parseTraceLine(line);
    expect(e).not.toBeNull();
    expect(e!.type).toBe("point");
    expect(e!.name).toBe("authoritative_import_route_in_view_result");
  });

  it("returns null for non-trace lines", () => {
    expect(parseTraceLine("core_event name=foo bar=baz")).toBeNull();
    expect(parseTraceLine("")).toBeNull();
    expect(parseTraceLine("Done in 100ms")).toBeNull();
  });
});

describe("parseCoreEvent", () => {
  it("parses a core_event line", () => {
    const line = `core_event name=core_named_resolution file=<unknown> kind=missing cache=miss name=ComponentConfig bindings=0 companions=0`;
    const e = parseCoreEvent(line);
    expect(e).not.toBeNull();
    expect(e!.name).toBe("core_named_resolution");
    expect(e!.detail).toContain("kind=missing");
  });

  it("returns null for non-core lines", () => {
    expect(parseCoreEvent("[verter-meta-trace] event=start")).toBeNull();
  });
});

describe("parseTraceLog", () => {
  it("separates trace events and core events", () => {
    const content = [
      `[verter-meta-trace] event=start trace=1 span=1 parent=- request=1 subrequest=1 caller=- depth=0 thread=ThreadId(1) name="resolve_component_meta" detail="owner=foo.vue"`,
      `core_event name=core_named_resolution file=<unknown> kind=missing cache=miss name=Foo bindings=0 companions=0`,
      `[verter-meta-trace] event=end trace=1 span=1 parent=- request=1 subrequest=1 caller=- depth=0 thread=ThreadId(1) name="resolve_component_meta" detail="owner=foo.vue" dur_ms=100.0`,
    ].join("\n");
    const { events, coreEvents } = parseTraceLog(content);
    expect(events).toHaveLength(2);
    expect(coreEvents).toHaveLength(1);
  });
});

// ── Path normalization tests ──────────────────────────────────────────

describe("normalizePath", () => {
  it("strips pnpm store hashes", () => {
    const raw =
      "d:/project/node_modules/.pnpm/reka-ui@2.9.2_vue@3.5.31_typescript@5.9.3_/node_modules/reka-ui/dist/index.d.ts";
    expect(normalizePath(raw)).toBe("d:/project/node_modules/reka-ui/dist/index.d.ts");
  });

  it("strips workspace root prefix", () => {
    const raw =
      "d:/dev/personal/verter/.integration-tests/repos/nuxt-ui/src/runtime/components/Accordion.vue";
    const normalized = normalizePath(
      raw,
      "d:/dev/personal/verter/.integration-tests/repos/nuxt-ui",
    );
    expect(normalized).toBe("src/runtime/components/Accordion.vue");
  });

  it("converts backslashes to forward slashes", () => {
    expect(normalizePath("d:\\foo\\bar\\baz.ts")).toBe("d:/foo/bar/baz.ts");
  });
});

// ── Validator tests ───────────────────────────────────────────────────

function makeMinimalSpec(overrides: Partial<TraceSpec> = {}): TraceSpec {
  return {
    component: "Test",
    componentPath: "src/Test.vue",
    maxTotalDurationMs: 5000,
    required: [],
    forbidden: [],
    maxCounts: [],
    maxDurations: [],
    ...overrides,
  };
}

const SAMPLE_TRACE = [
  `[verter-meta-trace] event=start trace=1 span=1 parent=- request=1 subrequest=1 caller=- depth=0 thread=ThreadId(1) name="resolve_component_meta" detail="owner=src/Test.vue mode=Expanded"`,
  `[verter-meta-trace] event=start trace=1 span=2 parent=1 request=1 subrequest=2 caller=1 depth=1 thread=ThreadId(1) name="compute_evaluated_types_expand_macros" detail="owner=src/Test.vue macros=2 bindings=0 store_view=true"`,
  `[verter-meta-trace] event=start trace=1 span=3 parent=2 request=1 subrequest=3 caller=2 depth=2 thread=ThreadId(1) name="resolve_imported_type_root" detail="canonical=types/index.ts imported=FooProps"`,
  `[verter-meta-trace] event=end trace=1 span=3 parent=2 request=1 subrequest=3 caller=2 depth=2 thread=ThreadId(1) name="resolve_imported_type_root" detail="canonical=types/index.ts imported=FooProps" dur_ms=50.0`,
  `[verter-meta-trace] event=end trace=1 span=2 parent=1 request=1 subrequest=2 caller=1 depth=1 thread=ThreadId(1) name="compute_evaluated_types_expand_macros" detail="owner=src/Test.vue macros=2 bindings=0 store_view=true" dur_ms=200.0`,
  `core_event name=core_named_resolution file=<unknown> kind=missing cache=miss name=ComponentConfig bindings=0 companions=0`,
  `[verter-meta-trace] event=point trace=1 span=4 parent=1 request=1 subrequest=4 caller=1 depth=1 thread=ThreadId(1) name="resolve_component_meta_result" detail="owner=src/Test.vue mode=Expanded source=flight:leader attempts=1 macros=2 resolved_types=2 has_evaluated_types=true fact_versions=10"`,
  `[verter-meta-trace] event=end trace=1 span=1 parent=- request=1 subrequest=1 caller=- depth=0 thread=ThreadId(1) name="resolve_component_meta" detail="owner=src/Test.vue mode=Expanded" dur_ms=300.0`,
  `[verter-meta-trace] event=point trace=2 span=5 parent=2 request=2 subrequest=5 caller=2 depth=1 thread=ThreadId(1) name="extract_component_meta_declared_surface" detail="owner=src/Test.vue props=5 events=2 slots=3"`,
].join("\n");

describe("validateTrace", () => {
  it("passes with a permissive spec", () => {
    const { events, coreEvents } = parseTraceLog(SAMPLE_TRACE);
    const result = validateTrace(makeMinimalSpec(), events, coreEvents);
    expect(result.passed).toBe(true);
    expect(result.failures).toHaveLength(0);
  });

  it("fails on total duration exceeded", () => {
    const { events, coreEvents } = parseTraceLog(SAMPLE_TRACE);
    const result = validateTrace(makeMinimalSpec({ maxTotalDurationMs: 100 }), events, coreEvents);
    expect(result.passed).toBe(false);
    expect(result.failures.some((f) => f.kind === "total_duration_exceeded")).toBe(true);
  });

  it("fails on missing required event", () => {
    const { events, coreEvents } = parseTraceLog(SAMPLE_TRACE);
    const result = validateTrace(
      makeMinimalSpec({
        required: [
          {
            namePattern: "nonexistent_event",
            note: "this event should appear",
          },
        ],
      }),
      events,
      coreEvents,
    );
    expect(result.passed).toBe(false);
    expect(result.failures[0].kind).toBe("missing_required");
  });

  it("passes when required event exists", () => {
    const { events, coreEvents } = parseTraceLog(SAMPLE_TRACE);
    const result = validateTrace(
      makeMinimalSpec({
        required: [
          {
            namePattern: "resolve_component_meta",
            note: "must resolve component meta",
          },
        ],
      }),
      events,
      coreEvents,
    );
    expect(result.passed).toBe(true);
  });

  it("fails on forbidden event present", () => {
    const { events, coreEvents } = parseTraceLog(SAMPLE_TRACE);
    const result = validateTrace(
      makeMinimalSpec({
        forbidden: [
          {
            namePattern: "core_named_resolution",
            detailPattern: "kind=missing",
            note: "missing type resolution should not happen on fast path",
          },
        ],
      }),
      events,
      coreEvents,
    );
    expect(result.passed).toBe(false);
    expect(result.failures[0].kind).toBe("forbidden_present");
  });

  it("passes when forbidden event absent", () => {
    const { events, coreEvents } = parseTraceLog(SAMPLE_TRACE);
    const result = validateTrace(
      makeMinimalSpec({
        forbidden: [
          {
            namePattern: "legacy_fallback_path",
            note: "legacy should not be used",
          },
        ],
      }),
      events,
      coreEvents,
    );
    expect(result.passed).toBe(true);
  });

  it("fails on count exceeded", () => {
    const { events, coreEvents } = parseTraceLog(SAMPLE_TRACE);
    const result = validateTrace(
      makeMinimalSpec({
        maxCounts: [
          {
            namePattern: "resolve_imported_type_root",
            maxCount: 0,
            note: "should not resolve type roots in this test",
          },
        ],
      }),
      events,
      coreEvents,
    );
    expect(result.passed).toBe(false);
    expect(result.failures[0].kind).toBe("count_exceeded");
  });

  it("fails on duration exceeded", () => {
    const { events, coreEvents } = parseTraceLog(SAMPLE_TRACE);
    const result = validateTrace(
      makeMinimalSpec({
        maxDurations: [
          {
            namePattern: "compute_evaluated_types_expand_macros",
            maxDurationMs: 100,
            note: "macro expansion should be fast",
          },
        ],
      }),
      events,
      coreEvents,
    );
    expect(result.passed).toBe(false);
    expect(result.failures[0].kind).toBe("duration_exceeded");
  });

  it("supports regex patterns", () => {
    const { events, coreEvents } = parseTraceLog(SAMPLE_TRACE);
    const result = validateTrace(
      makeMinimalSpec({
        required: [
          {
            namePattern: "/resolve_.*_meta/",
            note: "regex match for any resolve meta event",
          },
        ],
      }),
      events,
      coreEvents,
    );
    expect(result.passed).toBe(true);
  });

  it("supports detail pattern filtering on counts", () => {
    const { events, coreEvents } = parseTraceLog(SAMPLE_TRACE);
    // There's 1 resolve_imported_type_root with types/index.ts
    const result = validateTrace(
      makeMinimalSpec({
        maxCounts: [
          {
            namePattern: "resolve_imported_type_root",
            detailPattern: "canonical=types/index.ts",
            maxCount: 2,
            note: "should resolve from types/index.ts at most once (start+end = 2 events)",
          },
        ],
      }),
      events,
      coreEvents,
    );
    expect(result.passed).toBe(true);
  });

  it("fails when expectedResult minProps is not met", () => {
    const { events, coreEvents } = parseTraceLog(SAMPLE_TRACE);
    const result = validateTrace(
      makeMinimalSpec({
        expectedResult: {
          minProps: 10,
          requireEvaluatedTypes: true,
          note: "should have at least 10 props",
        },
      }),
      events,
      coreEvents,
    );
    expect(result.passed).toBe(false);
    expect(result.failures.some((f) => f.kind === "result_incorrect")).toBe(true);
    expect(result.failures[0].actual).toContain("5 props");
  });

  it("passes when expectedResult is satisfied", () => {
    const { events, coreEvents } = parseTraceLog(SAMPLE_TRACE);
    const result = validateTrace(
      makeMinimalSpec({
        expectedResult: {
          minProps: 5,
          minEvents: 2,
          minSlots: 3,
          requireEvaluatedTypes: true,
          note: "exact match for sample trace",
        },
      }),
      events,
      coreEvents,
    );
    expect(result.passed).toBe(true);
  });

  it("fails when requireEvaluatedTypes but has_evaluated_types missing", () => {
    // Trace without resolve_component_meta_result has_evaluated_types=true
    const noEvalTrace = [
      `[verter-meta-trace] event=start trace=1 span=1 parent=- request=1 subrequest=1 caller=- depth=0 thread=ThreadId(1) name="resolve_component_meta" detail="owner=src/Test.vue mode=Expanded"`,
      `[verter-meta-trace] event=end trace=1 span=1 parent=- request=1 subrequest=1 caller=- depth=0 thread=ThreadId(1) name="resolve_component_meta" detail="owner=src/Test.vue mode=Expanded" dur_ms=100.0`,
      `[verter-meta-trace] event=point trace=2 span=5 parent=2 request=2 subrequest=5 caller=2 depth=1 thread=ThreadId(1) name="extract_component_meta_declared_surface" detail="owner=src/Test.vue props=5 events=0 slots=0"`,
    ].join("\n");
    const { events, coreEvents } = parseTraceLog(noEvalTrace);
    const result = validateTrace(
      makeMinimalSpec({
        expectedResult: {
          minProps: 1,
          requireEvaluatedTypes: true,
          note: "must have evaluated types",
        },
      }),
      events,
      coreEvents,
    );
    expect(result.passed).toBe(false);
    expect(result.failures.some((f) => f.assertion.includes("requireEvaluatedTypes"))).toBe(true);
  });

  it("fails when no declared surface event exists", () => {
    const noSurfaceTrace = [
      `[verter-meta-trace] event=start trace=1 span=1 parent=- request=1 subrequest=1 caller=- depth=0 thread=ThreadId(1) name="resolve_component_meta" detail="owner=src/Test.vue mode=Expanded"`,
      `[verter-meta-trace] event=end trace=1 span=1 parent=- request=1 subrequest=1 caller=- depth=0 thread=ThreadId(1) name="resolve_component_meta" detail="owner=src/Test.vue mode=Expanded" dur_ms=100.0`,
    ].join("\n");
    const { events, coreEvents } = parseTraceLog(noSurfaceTrace);
    const result = validateTrace(
      makeMinimalSpec({
        expectedResult: {
          minProps: 1,
          note: "must have declared surface",
        },
      }),
      events,
      coreEvents,
    );
    expect(result.passed).toBe(false);
    expect(result.failures[0].actual).toContain("no declared surface event");
  });
});

describe("buildSummary", () => {
  it("computes event counts and durations", () => {
    const { events, coreEvents } = parseTraceLog(SAMPLE_TRACE);
    const summary = buildSummary(events, coreEvents);
    expect(summary.totalDurationMs).toBeCloseTo(300);
    expect(summary.eventCounts.get("resolve_component_meta")).toBe(2); // start + end
    expect(summary.maxDurations.get("resolve_component_meta")).toBeCloseTo(300);
    expect(summary.eventCounts.get("core:core_named_resolution")).toBe(1);
  });

  it("extracts declared surface from trace", () => {
    const { events, coreEvents } = parseTraceLog(SAMPLE_TRACE);
    const summary = buildSummary(events, coreEvents);
    expect(summary.declaredSurface).toEqual({ props: 5, events: 2, slots: 3 });
    expect(summary.hasEvaluatedTypes).toBe(true);
  });
});

describe("loadTraceSpec", () => {
  it("loads a valid spec", () => {
    const json = JSON.stringify({
      component: "Accordion",
      componentPath: "src/runtime/components/Accordion.vue",
      maxTotalDurationMs: 3000,
      required: [{ namePattern: "resolve_component_meta", note: "must resolve" }],
      forbidden: [],
      maxCounts: [],
      maxDurations: [],
    });
    const spec = loadTraceSpec(json);
    expect(spec.component).toBe("Accordion");
    expect(spec.required).toHaveLength(1);
  });

  it("throws on missing required fields", () => {
    expect(() => loadTraceSpec(JSON.stringify({}))).toThrow("Invalid trace spec");
  });

  it("rejects under-specified specs when requireForbidden is set", () => {
    const json = JSON.stringify({
      component: "Weak",
      componentPath: "src/Weak.vue",
      maxTotalDurationMs: 1000,
      forbidden: [],
    });
    expect(() => loadTraceSpec(json, { requireForbidden: true })).toThrow(
      "no forbidden assertions",
    );
  });

  it("rejects under-specified specs when requireMaxCounts is set", () => {
    const json = JSON.stringify({
      component: "Weak",
      componentPath: "src/Weak.vue",
      maxTotalDurationMs: 1000,
      forbidden: [{ namePattern: "legacy", note: "guard" }],
      maxCounts: [],
    });
    expect(() => loadTraceSpec(json, { requireMaxCounts: true })).toThrow("no maxCount assertions");
  });

  it("accepts well-specified specs with requireForbidden", () => {
    const json = JSON.stringify({
      component: "Strong",
      componentPath: "src/Strong.vue",
      maxTotalDurationMs: 1000,
      forbidden: [{ namePattern: "legacy_path", note: "must not use legacy" }],
      maxCounts: [{ namePattern: "expensive_op", maxCount: 10, note: "bounded" }],
    });
    const spec = loadTraceSpec(json, { requireForbidden: true, requireMaxCounts: true });
    expect(spec.forbidden).toHaveLength(1);
    expect(spec.maxCounts).toHaveLength(1);
  });

  it("defaults optional arrays to empty", () => {
    const json = JSON.stringify({
      component: "Test",
      componentPath: "src/Test.vue",
      maxTotalDurationMs: 1000,
    });
    const spec = loadTraceSpec(json);
    expect(spec.required).toHaveLength(0);
    expect(spec.forbidden).toHaveLength(0);
  });
});

describe("formatValidationResult", () => {
  it("formats a passing result", () => {
    const { events, coreEvents } = parseTraceLog(SAMPLE_TRACE);
    const result = validateTrace(makeMinimalSpec(), events, coreEvents);
    const output = formatValidationResult(result);
    expect(output).toContain("[PASS]");
    expect(output).toContain("Test");
  });

  it("formats a failing result with failure details", () => {
    const { events, coreEvents } = parseTraceLog(SAMPLE_TRACE);
    const result = validateTrace(makeMinimalSpec({ maxTotalDurationMs: 10 }), events, coreEvents);
    const output = formatValidationResult(result);
    expect(output).toContain("[FAIL]");
    expect(output).toContain("total_duration_exceeded");
  });
});

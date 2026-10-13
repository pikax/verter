import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import {
  readBaselineManifest,
  validateManifest,
  evaluateGate,
  buildGateErrorReport,
  buildSideContexts,
  runInterleaved,
  type BaselineManifest,
  type WorkloadSpec,
  type GateEvaluationInput,
  type WorkloadEvaluationInput,
  type MetricSource,
} from "./gate.js";
import { ratioDecision, throughputRatioDecision } from "./stats.js";
import {
  axisACodegen,
  type WorkloadSample,
  type WorkloadContext,
  type AxisAChildRunner,
} from "./workloads.js";
import type { AxisAChildSample } from "./axis-a-child.js";
import type { EnsuredCorpus } from "./corpus.js";

const __dirname = dirname(fileURLToPath(import.meta.url));

/**
 * A syntactically-valid pinned baseline: a full 40-hex commit hash. Armed mode
 * REQUIRES this shape — a non-SHA value (a branch, a tag, a placeholder) can
 * never arm the gate (see the "armed mode requires a pinned 40-hex SHA" spec).
 */
const PINNED_BASELINE_SHA = "0123456789abcdef0123456789abcdef01234567";

describe("validateManifest hard-fails a malformed perf manifest (no silent gate disable)", () => {
  // A malformed metric must never silently DISABLE a gate: an unknown `statistic`
  // would fall back to median, an unknown `direction` to lower-is-better, and a
  // missing/NaN `threshold` make `ratio > threshold` false (inert). validateManifest rejects each
  // before evaluation. Every case mutates exactly ONE field of a valid base.
  const validMetric = (over: Record<string, unknown> = {}): Record<string, unknown> => ({
    threshold: 1.1,
    direction: "lower-is-better",
    statistic: "median",
    source: { kind: "scalar", key: "filesPerSec" },
    ...over,
  });
  const validRaw = (workloadOver: Record<string, unknown> = {}): Record<string, unknown> => ({
    methodologyVersion: "1.0.0",
    baselineRef: "PENDING",
    corpusHash: `sha256:${"0".repeat(64)}`,
    tsgoVersion: "typescript@7.0.1-rc",
    samplesPerSide: 7,
    workloads: {
      "axis-a-codegen": { axis: "A", title: "t", gated: { m: validMetric() }, ...workloadOver },
    },
  });
  const metricRaw = (metric: unknown): Record<string, unknown> =>
    validRaw({ gated: { m: metric } });

  it("ACCEPTS the committed manifest and a well-formed synthetic manifest (positive controls)", () => {
    expect(() =>
      validateManifest(JSON.parse(JSON.stringify(readBaselineManifest()))),
    ).not.toThrow();
    expect(() => validateManifest(validRaw())).not.toThrow();
    // total_wall / rss sources legitimately carry NO key (only scalar/distribution/
    // attribution do), and a correctness-only workload (empty gated + reported) is valid.
    expect(() =>
      validateManifest(
        validRaw({
          gated: {},
          correctnessGated: true,
          reported: { w: validMetric({ source: { kind: "total_wall" } }) },
        }),
      ),
    ).not.toThrow();
  });

  it("THROWS on an unknown statistic (it must not silently fall back to median)", () => {
    expect(() => validateManifest(metricRaw(validMetric({ statistic: "p42" })))).toThrow(
      /statistic/i,
    );
  });

  it("THROWS on an unknown direction (it must not silently fall back to lower-is-better)", () => {
    expect(() => validateManifest(metricRaw(validMetric({ direction: "sideways" })))).toThrow(
      /direction/i,
    );
  });

  it("THROWS on a missing threshold (`ratio > undefined` is false ⇒ an inert gate)", () => {
    const m = validMetric();
    delete (m as { threshold?: unknown }).threshold;
    expect(() => validateManifest(metricRaw(m))).toThrow(/threshold/i);
  });

  it("THROWS on a NaN / non-finite / non-positive threshold (each reads as an inert gate)", () => {
    expect(() => validateManifest(metricRaw(validMetric({ threshold: Number.NaN })))).toThrow(
      /threshold/i,
    );
    expect(() =>
      validateManifest(metricRaw(validMetric({ threshold: Number.POSITIVE_INFINITY }))),
    ).toThrow(/threshold/i);
    expect(() => validateManifest(metricRaw(validMetric({ threshold: 0 })))).toThrow(/threshold/i);
    expect(() => validateManifest(metricRaw(validMetric({ threshold: -1 })))).toThrow(/threshold/i);
  });

  it("THROWS on an unknown source.kind or a key-less scalar/distribution/attribution source", () => {
    expect(() => validateManifest(metricRaw(validMetric({ source: { kind: "bogus" } })))).toThrow(
      /source/i,
    );
    expect(() => validateManifest(metricRaw(validMetric({ source: { kind: "scalar" } })))).toThrow(
      /key|source/i,
    );
    expect(() =>
      validateManifest(metricRaw(validMetric({ source: { kind: "attribution" } }))),
    ).toThrow(/key|source/i);
  });

  it("THROWS on a malformed gated-workload entry (a non-object metric or a non-object gated map)", () => {
    expect(() => validateManifest(metricRaw("not-an-object"))).toThrow(/metric|gated/i);
    expect(() => validateManifest(validRaw({ gated: "nope" }))).toThrow(/gated/i);
  });

  it("THROWS on an unknown workload id, an empty workloads map, or a fully-inert workload", () => {
    const unknown = validRaw();
    (unknown as { workloads: Record<string, unknown> }).workloads = {
      "not-a-real-workload": { axis: "A", title: "t", gated: { m: validMetric() } },
    };
    expect(() => validateManifest(unknown)).toThrow(/workload/i);
    const empty = validRaw();
    (empty as { workloads: Record<string, unknown> }).workloads = {};
    expect(() => validateManifest(empty)).toThrow(/workload/i);
    // inert: empty gated, no reported / correctness / content signal.
    expect(() => validateManifest(validRaw({ gated: {} }))).toThrow(/gat|measure|signal|inert/i);
  });

  it("THROWS on a malformed top-level field (missing tsgoVersion / non-positive samplesPerSide / non-object)", () => {
    const noEngine = validRaw();
    delete (noEngine as { tsgoVersion?: unknown }).tsgoVersion;
    expect(() => validateManifest(noEngine)).toThrow(/tsgoVersion|engine/i);
    expect(() => validateManifest({ ...validRaw(), samplesPerSide: 0 })).toThrow(/samples/i);
    expect(() => validateManifest(null)).toThrow();
    expect(() => validateManifest(42)).toThrow();
  });
});

describe("baseline manifest", () => {
  it("is well-formed and gates both axes (A and B)", () => {
    const m = readBaselineManifest();
    expect(m.methodologyVersion).toBeTruthy();
    expect(m.corpusHash).toMatch(/^sha256:[0-9a-f]{64}$/);
    expect(m.samplesPerSide).toBeGreaterThanOrEqual(7);
    const axes = new Set(Object.values(m.workloads).map((w) => w.axis));
    expect(axes.has("A")).toBe(true); // native-compiler codegen regression
    expect(axes.has("B")).toBe(true); // tsgo carrier-typecheck/LSP regression
  });

  it("pins the engine to an exact, comparable version string", () => {
    const m = readBaselineManifest();
    // The gate verifies the resolved engine against this exact string — it must
    // be a clean comparable id (no prose), engine "tsgo" via the typescript@7 rc.
    expect(m.tsgoVersion).toMatch(/^typescript@7\./);
    expect(m.tsgoVersion).not.toMatch(/\s/); // no free-text suffix
  });

  it("declares the honest §2.7 gated metric set per workload", () => {
    const m = readBaselineManifest();

    // AXIS A — gated ONLY on empirically-present audit signals (the per-side native
    // child measures these on every real run). Per-PID peak RSS and the full
    // non-checker aggregate are DEFERRED (their audit sources are null on a real
    // in-process compile) — see the `deferred` assertions below.
    const axisA = m.workloads["axis-a-codegen"];
    expect(axisA).toBeDefined();
    expect(axisA.gated.compile_throughput_ratio.direction).toBe("higher-is-better");
    expect(Object.keys(axisA.gated)).toEqual(
      expect.arrayContaining([
        "compile_throughput_ratio",
        "codegen_time_ratio",
        "source_map_bytes",
        "generated_carrier_count",
        "output_bytes_ratio",
      ]),
    );
    // The two UNMEASURABLE metrics are NOT gated on axis A (deferred, not faked).
    expect(Object.keys(axisA.gated)).not.toContain("non_checker_time_ratio");
    expect(Object.keys(axisA.gated)).not.toContain("peak_rss_ratio");
    // The codegen-time metric reads the PRESENT codegen + source-map emit phases —
    // named honestly for what it measures, NOT the deferred non-checker aggregate.
    expect(axisA.gated.codegen_time_ratio.direction).toBe("lower-is-better");
    expect(axisA.gated.codegen_time_ratio.source).toEqual({
      kind: "attribution",
      key: "codegenSourcemapMs",
    });
    // The codegen-output invariant trips on any material change in generated bytes.
    expect(axisA.gated.output_bytes_ratio.direction).toBe("invariant");
    expect(axisA.gated.output_bytes_ratio.source).toEqual({
      kind: "attribution",
      key: "outputBytes",
    });
    // Every axis-A gated metric declares the sample source it reads (data-driven,
    // no name-based switch).
    for (const spec of Object.values(axisA.gated)) expect(spec.source).toBeDefined();
    // The carrier count is the audited-compiles-emitting-output signal (a scalar
    // metric), not a static file count.
    expect(axisA.gated.generated_carrier_count.source.kind).toBe("scalar");
    // It is a TWO-SIDED invariant — a DROP (skipped carrier generation) is a
    // correctness regression, NOT a "lower-is-better" perf win.
    expect(axisA.gated.generated_carrier_count.direction).toBe("invariant");

    // COLD — correctness-ONLY gated; total wall is REPORTED (in the sibling
    // `reported` section, NEVER inside `gated`); the static carrier count is DEFERRED.
    // The child peak RSS is DEFERRED too: verter-tsc spawns tsgo as a SEPARATE child,
    // so the wrapper-PID VmHWM misses the engine's memory (no gated metric reads it).
    const cold = m.workloads["cold-typecheck"];
    expect(cold).toBeDefined();
    expect(Object.keys(cold.gated)).not.toContain("peak_rss_ratio");
    expect(cold.gated.total_wall_time_ratio).toBeUndefined();
    expect(cold.reported?.total_wall_time_ratio).toBeDefined();
    expect(cold.correctnessGated).toBe(true);
    expect(Object.keys(cold.gated)).not.toContain("generated_carrier_count");

    // WARM (incremental re-typecheck, on-disk carrier cache) — correctness-ONLY
    // gated; wall is REPORTED (sibling section); changed-carrier DEFERRED. The child
    // peak RSS is DEFERRED (same wrapper-PID-misses-tsgo reason as cold).
    const warm = m.workloads["warm-incremental-retypecheck"];
    expect(warm).toBeDefined();
    expect(Object.keys(warm.gated)).not.toContain("peak_rss_ratio");
    expect(warm.gated.total_wall_time_ratio).toBeUndefined();
    expect(warm.reported?.total_wall_time_ratio).toBeDefined();
    expect(warm.correctnessGated).toBe(true);
    expect(Object.keys(warm.gated)).not.toContain("changed_carrier_count");

    // WARM via the PERSISTENT LSP — the genuinely-warm signal (Program retained).
    const warmLsp = m.workloads["warm-lsp-incremental"];
    expect(warmLsp).toBeDefined();
    expect(warmLsp.gated.p95_latency_ratio.statistic).toBe("p95");
    expect(warmLsp.gated.p95_latency_ratio.source.kind).toBe("distribution");

    // INTERACTIVE — real per-operation distributions + behavioral invariants.
    const edit = m.workloads["single-file-edit-latency"];
    expect(edit.gated.p99_latency_ratio.statistic).toBe("p99");
    expect(edit.gated.p99_latency_ratio.source.kind).toBe("distribution");
    expect(edit.behavioral).toBeDefined();
    expect(edit.behavioral!.maxAffectedUriFraction).toBeGreaterThan(0);

    const ide = m.workloads["ide-query-latency"];
    // Hover and completion are SEPARATE distributions (not one conflated wall).
    expect(ide.gated.hover_p95_latency_ratio.source).toEqual({
      kind: "distribution",
      key: "hoverLatency",
    });
    expect(ide.gated.completion_p95_latency_ratio.source).toEqual({
      kind: "distribution",
      key: "completionLatency",
    });
    // Candidate/baseline query RESULTS must match — hit + item-count parity is a
    // two-sided invariant (a broken no-op LSP returning fewer results fails).
    expect(ide.gated.hover_hit_parity.direction).toBe("invariant");
    expect(ide.gated.hover_hit_parity.source).toEqual({ kind: "scalar", key: "hoverHits" });
    expect(ide.gated.completion_item_parity.direction).toBe("invariant");
    expect(ide.gated.completion_item_parity.source).toEqual({
      kind: "scalar",
      key: "completionItems",
    });
  });

  it("labels compile_throughput_ratio as per-file SERIAL, not compileMany/batch/parallel", () => {
    // The throughput metric is a SERIAL per-file compileWithAudit loop, not a
    // batch/parallel compileMany call. The note must say so and must NOT claim
    // compileMany/batch/parallel throughput. A metric with no note would not pin this.
    const axisA = readBaselineManifest().workloads["axis-a-codegen"];
    const note = axisA.gated.compile_throughput_ratio.note ?? "";
    expect(note).toMatch(/serial/i);
    expect(note).not.toMatch(/compileMany|batch|parallel/i);
  });

  it("records the DEFERRED attribution as named follow-ups (honest, not fake-passed)", () => {
    const m = readBaselineManifest();
    expect(Array.isArray(m.deferred)).toBe(true);
    expect(m.deferred!.length).toBeGreaterThanOrEqual(4);
    for (const d of m.deferred!) {
      expect(d.metric).toBeTruthy();
      expect(d.requiresRust).toBeTruthy(); // names the follow-up work
    }
    // The deferred metrics must NOT appear in any workload's gated set.
    const gatedNames = new Set<string>();
    for (const w of Object.values(m.workloads))
      for (const k of Object.keys(w.gated)) gatedNames.add(k);
    expect(gatedNames.has("changed_carrier_count")).toBe(false);
  });

  it("labels metrics honestly: peak (not steady-state) RSS, publication locality, honest thread policy", () => {
    const m = readBaselineManifest();
    // The verter-tsc child peak RSS is DEFERRED (not gated): verter-tsc spawns tsgo
    // as a SEPARATE child, so the wrapper-PID VmHWM misses the engine's memory, and
    // process-tree RSS is not cleanly portable in TS. Neither warm nor cold gates a
    // peak/steady-state RSS metric; the deferral names the Rust signal that would.
    const warm = m.workloads["warm-incremental-retypecheck"];
    expect(Object.keys(warm.gated)).not.toContain("peak_rss_ratio");
    expect(Object.keys(warm.gated)).not.toContain("steady_state_rss_ratio");
    const deferredRss = JSON.stringify(m.deferred ?? []);
    expect(deferredRss).toMatch(/axis-b[\s\S]*peak rss|wrapper-pid|tsgo/i);

    // item 10 — the single-file-edit behavioral invariant is labeled as
    // diagnostic-publication locality (a publishDiagnostics-URI proxy), and real
    // invalidation/recheck is a named Rust follow-up (not claimed as measured).
    const edit = m.workloads["single-file-edit-latency"];
    expect(edit.note ?? "").toMatch(/publication/i);
    const deferredText = JSON.stringify(m.deferred ?? []);
    expect(deferredText).toMatch(/invalidat|recheck/i); // a named Rust follow-up exists

    // The thread policy must not claim "physical core count" while the impl uses
    // os.availableParallelism() (logical), AND must be HONEST that the axis-B
    // subprocesses run at MACHINE-DEFAULT parallelism (they accept no harness
    // thread-pin) rather than a harness-enforced fixed count — only axis-A's
    // in-process host is thread-pinned. The policy must NOT claim a "fixed N …
    // held identical across candidate + baseline" as if enforced everywhere.
    const threadPolicy = (m as unknown as { threadPolicy?: string }).threadPolicy ?? "";
    expect(threadPolicy).not.toMatch(/physical core/i);
    expect(threadPolicy).toMatch(/availableParallelism/i);
    expect(threadPolicy).toMatch(/machine-default/i);
    expect(threadPolicy).toMatch(/in-process|thread-pinned|hostCpuThreads/i);
    // The subprocess thread-pin gap is named as a Rust follow-up in `deferred`.
    expect(JSON.stringify(m.deferred ?? [])).toMatch(/thread-pin|thread-count enforcement/i);
  });

  it("the deferred section is internally consistent with the RSS-free gated set (no deferred RSS entry claims it stays gated)", () => {
    const m = readBaselineManifest();
    // STRUCTURAL (the real discriminator for the deferral): NO workload gates ANY RSS
    // metric, on either axis — peak RSS is deferred everywhere (axis-A: the in-process
    // compile arms no sampler; axis-B: the verter-tsc wrapper PID misses the tsgo
    // child). This is the invariant the deferred prose must match.
    const rssGatedKeys: string[] = [];
    for (const [id, w] of Object.entries(m.workloads)) {
      for (const k of Object.keys(w.gated)) if (/rss/i.test(k)) rssGatedKeys.push(`${id}.${k}`);
    }
    expect(rssGatedKeys).toEqual([]);

    // CONSISTENCY: because NO RSS metric is gated, no DEFERRED (= not-gated) entry that
    // discusses RSS may claim an RSS metric "stays/is/remains gated" — that prose would
    // contradict the structural gated set above (e.g. an axis-A peak-RSS deferral
    // that claimed "axis-B cold/warm peak RSS stays gated").
    const rssDeferrals = (m.deferred ?? []).filter((d) => /rss/i.test(`${d.metric} ${d.reason}`));
    expect(rssDeferrals.length).toBeGreaterThan(0); // axis-A + axis-B peak-RSS deferrals exist
    for (const d of rssDeferrals) {
      expect(
        d.reason,
        `deferred RSS entry "${d.metric}" must not claim an RSS metric stays gated`,
      ).not.toMatch(/\b(?:stays|is|remains)\s+gated\b/i);
    }
  });

  it("declares the Vue .vue-ONLY corpus scope and defers Svelte to a later block (B8)", () => {
    // The corpus + discovery + workloads are .vue-only. The manifest must
    // declare that scope explicitly (no overclaimed "Vue/Svelte" coverage) AND
    // name the Svelte perf corpus as a B8-gated deferral — a manifest missing the
    // corpusScope field or the Svelte deferral entry would silently overclaim coverage.
    const m = readBaselineManifest();
    const scope = (m as unknown as { corpusScope?: string }).corpusScope ?? "";
    expect(scope).toMatch(/\.vue/);
    expect(scope).toMatch(/only/i);
    expect(scope).toMatch(/svelte/i); // explicit: no current Svelte coverage
    // A named Svelte perf-corpus deferral exists, gated on B8.
    const svelte = (m.deferred ?? []).find((d) => /svelte/i.test(d.metric));
    expect(svelte, "a Svelte perf-corpus deferral must exist").toBeTruthy();
    expect(svelte!.requiresRust).toMatch(/b8/i);
    // …and it must NOT be smuggled into any gated workload as if measured today.
    const gatedNames = new Set<string>();
    for (const w of Object.values(m.workloads)) {
      for (const k of Object.keys(w.gated)) gatedNames.add(k);
    }
    expect([...gatedNames].some((k) => /svelte/i.test(k))).toBe(false);
  });

  it("is keyed to the COMMITTED corpus manifest hash (freshness rail)", () => {
    const gate = readBaselineManifest();
    const corpusManifest = JSON.parse(
      readFileSync(
        join(
          __dirname,
          "..",
          "..",
          "..",
          "..",
          "test-corpora",
          "perf",
          "synthetic-15k",
          "manifest.json",
        ),
        "utf-8",
      ),
    ) as { contentHash: string };
    expect(gate.corpusHash).toBe(corpusManifest.contentHash);
  });
});

describe("gate predicate wiring (self-referential property)", () => {
  function jitter(base: number, spreadPct: number, n: number, seed: number): number[] {
    let a = seed >>> 0;
    const out: number[] = [];
    for (let i = 0; i < n; i++) {
      a = (a + 0x6d_2b_79_f5) | 0;
      let t = Math.imul(a ^ (a >>> 15), 1 | a);
      t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
      const u = ((t ^ (t >>> 14)) >>> 0) / 4_294_967_296;
      out.push(base * (1 + (u - 0.5) * 2 * spreadPct));
    }
    return out;
  }

  it("same build ⇒ ~1.0 ratio, no fail (the self-check invariant)", () => {
    const a = jitter(50, 0.06, 9, 42);
    const b = jitter(50, 0.06, 9, 42);
    const d = ratioDecision(a, b, 1.08, { resamples: 3000 });
    expect(d.statisticRatio).toBeCloseTo(1.0, 6);
    expect(d.fail).toBe(false);
  });

  it("a decisive Verter-owned regression in non-checker time DOES fail", () => {
    const baseline = jitter(20, 0.03, 9, 7);
    const candidate = jitter(25, 0.03, 9, 7);
    const d = ratioDecision(candidate, baseline, 1.08, { resamples: 3000 });
    expect(d.fail).toBe(true);
  });

  it("a throughput DROP (axis A) fails; a throughput GAIN does not", () => {
    const baseThroughput = jitter(2000, 0.03, 9, 3);
    const slower = jitter(1500, 0.03, 9, 3);
    const faster = jitter(2600, 0.03, 9, 3);
    expect(throughputRatioDecision(slower, baseThroughput, 1.1, { resamples: 3000 }).fail).toBe(
      true,
    );
    expect(throughputRatioDecision(faster, baseThroughput, 1.1, { resamples: 3000 }).fail).toBe(
      false,
    );
  });
});

// ── The pure evaluation: every discriminating gate property ──────────────────
describe("evaluateGate — discriminating gate evaluation", () => {
  function sample(over: Partial<WorkloadSample> = {}): WorkloadSample {
    return { totalMs: 0, attribution: null, rssBytes: 0, metrics: {}, ...over };
  }
  function side(samples: WorkloadSample[]) {
    return { samples };
  }
  function manifestOf(
    workloads: Record<string, WorkloadSpec>,
    over: Partial<BaselineManifest> = {},
  ): BaselineManifest {
    return {
      methodologyVersion: "test",
      baselineRef: "PENDING — test",
      corpusHash: `sha256:${"0".repeat(64)}`,
      tsgoVersion: "typescript@7.0.1-rc",
      samplesPerSide: 7,
      workloads,
      ...over,
    };
  }
  function gateInput(over: Partial<GateEvaluationInput>): GateEvaluationInput {
    // The default models a same-commit self-check against the PENDING manifest
    // (the shipped, unarmed state): metric/correctness/behavioral evaluation is
    // mode-independent, so these tests assert evaluation results without arming.
    // The arming-discipline tests (PENDING-never-armed, baseline-root, distinct
    // sides) override `selfCheck`/`baselineRef`/`meta` explicitly.
    return {
      manifest: manifestOf({}),
      workloads: [],
      smoke: false,
      selfCheck: true,
      engineResolved: "typescript@7.0.1-rc",
      baselineEngineResolved: "typescript@7.0.1-rc",
      meta: {
        corpusHash: `sha256:${"0".repeat(64)}`,
        candidateBin: "candidate",
        baselineBin: "baseline",
        candidateNative: "candidate-native",
        baselineNative: "baseline-native",
        baselineRoot: "baseline-root",
        threads: 4,
        samplesPerSide: 7,
      },
      ...over,
    };
  }

  it("FAILS on a full run when a required workload is unavailable; smoke tolerates it", () => {
    const spec: WorkloadSpec = { axis: "B", title: "cold", gated: {} };
    const wl: WorkloadEvaluationInput = {
      id: "cold",
      spec,
      available: false,
      unavailableReason: "verter-tsc binary not found",
      candidate: side([]),
      baseline: side([]),
    };
    const full = evaluateGate(gateInput({ manifest: manifestOf({ cold: spec }), workloads: [wl] }));
    expect(full.pass).toBe(false);
    expect(full.failures.join(" ")).toMatch(/unavailable|not found|skipped/i);

    const smoke = evaluateGate(
      gateInput({ manifest: manifestOf({ cold: spec }), workloads: [wl], smoke: true }),
    );
    expect(smoke.pass).toBe(true);
  });

  it("THROWS on an unknown metric source kind at the dispatch point (a source that bypassed validateManifest must fail loudly, not fall to an empty sample vector)", () => {
    // A malformed `source.kind` that slipped past `validateManifest` must hit the
    // exhaustive sample-set dispatch and throw — never silently yield an empty
    // sample vector the gate then reads as a degenerate-but-present (false-green)
    // metric. This drives the REAL dispatch via evaluateGate (no validation step).
    const spec: WorkloadSpec = {
      axis: "A",
      title: "t",
      gated: {
        bogus_metric: {
          threshold: 1.1,
          direction: "lower-is-better",
          statistic: "median",
          source: { kind: "not-a-real-kind" } as unknown as MetricSource,
        },
      },
    };
    const wl: WorkloadEvaluationInput = {
      id: "t",
      spec,
      available: true,
      candidate: side([sample({ totalMs: 10 })]),
      baseline: side([sample({ totalMs: 10 })]),
    };
    expect(() =>
      evaluateGate(gateInput({ manifest: manifestOf({ t: spec }), workloads: [wl] })),
    ).toThrow(/unknown metric source kind/i);
  });

  it("FAILS on a full run when a GATED metric vector is degenerate (all-zero / missing instrumentation)", () => {
    const spec: WorkloadSpec = {
      axis: "A",
      title: "axisA",
      gated: {
        peak_rss_ratio: {
          threshold: 1.1,
          direction: "lower-is-better",
          statistic: "median",
          source: { kind: "rss" },
        },
      },
    };
    const wl: WorkloadEvaluationInput = {
      id: "axisA",
      spec,
      available: true,
      candidate: side([sample({ rssBytes: 0 }), sample({ rssBytes: 0 })]),
      baseline: side([sample({ rssBytes: 0 }), sample({ rssBytes: 0 })]),
    };
    const full = evaluateGate(
      gateInput({ manifest: manifestOf({ axisA: spec }), workloads: [wl] }),
    );
    expect(full.pass).toBe(false);
    expect(full.failures.join(" ")).toMatch(
      /degenerate|missing instrumentation|all-zero|unavailable/i,
    );

    // Smoke tolerates an unavailable metric.
    const smoke = evaluateGate(
      gateInput({ manifest: manifestOf({ axisA: spec }), workloads: [wl], smoke: true }),
    );
    expect(smoke.pass).toBe(true);
  });

  it("a `reported` metric is SURFACED but NEVER gates pass/fail (only `gated` metrics decide)", () => {
    // reportedOnly metrics moved OUT of `gated` into a sibling `reported` section:
    // the gate's PASS/FAIL considers only `gated`; `reported` metrics are surfaced
    // for information but can never fail a run. Without the `reported` section such a
    // metric would either fake-pass INSIDE `gated` or not be evaluated at all; here
    // it is evaluated, surfaced (reportedOnly: true), and non-gating.
    const spec: WorkloadSpec = {
      axis: "B",
      title: "cold",
      gated: {
        peak_rss_ratio: {
          threshold: 1.1,
          direction: "lower-is-better",
          statistic: "median",
          source: { kind: "rss" },
        },
      },
      reported: {
        total_wall_time_ratio: {
          threshold: 1.25,
          direction: "lower-is-better",
          statistic: "median",
          source: { kind: "total_wall" },
        },
      },
    };
    // The reported wall-time REGRESSES decisively (candidate 2x baseline) while the
    // gated RSS is at parity. The reported regression must NOT fail the run, and the
    // reported metric MUST be surfaced in the result.
    const wl: WorkloadEvaluationInput = {
      id: "cold",
      spec,
      available: true,
      candidate: side([
        sample({ rssBytes: 100_000_000, totalMs: 200 }),
        sample({ rssBytes: 100_000_000, totalMs: 200 }),
      ]),
      baseline: side([
        sample({ rssBytes: 100_000_000, totalMs: 100 }),
        sample({ rssBytes: 100_000_000, totalMs: 100 }),
      ]),
    };
    const r = evaluateGate(gateInput({ manifest: manifestOf({ cold: spec }), workloads: [wl] }));
    expect(r.pass).toBe(true); // the reported 2x wall regression did NOT gate
    const reported = r.results[0].metrics.find((m) => m.metric === "total_wall_time_ratio");
    expect(reported).toBeDefined(); // surfaced (a non-evaluated `reported` would be undefined)
    expect(reported!.reportedOnly).toBe(true);
    expect(reported!.decision?.fail).toBe(true); // it WOULD fail if gated — but it doesn't gate

    // CONTROL: the SAME 2x regression placed in `gated` DOES fail the run.
    const gatedSpec: WorkloadSpec = {
      axis: "B",
      title: "cold",
      gated: {
        total_wall_time_ratio: {
          threshold: 1.25,
          direction: "lower-is-better",
          statistic: "median",
          source: { kind: "total_wall" },
        },
      },
    };
    const g = evaluateGate(
      gateInput({
        manifest: manifestOf({ cold: gatedSpec }),
        workloads: [{ ...wl, spec: gatedSpec }],
      }),
    );
    expect(g.pass).toBe(false);
  });

  it("FAILS a regressed distribution metric and PASSES an identical one (and persists raw samples)", () => {
    const spec: WorkloadSpec = {
      axis: "B",
      title: "edit",
      gated: {
        p95_latency_ratio: {
          threshold: 1.12,
          direction: "lower-is-better",
          statistic: "p95",
          source: { kind: "distribution", key: "editLatency" },
        },
      },
    };
    const baseDist = Array.from({ length: 60 }, () => 100);
    const candDist = Array.from({ length: 60 }, () => 140); // 40% slower

    const regressed: WorkloadEvaluationInput = {
      id: "edit",
      spec,
      available: true,
      candidate: side([sample({ distributions: { editLatency: candDist } })]),
      baseline: side([sample({ distributions: { editLatency: baseDist } })]),
    };
    const fail = evaluateGate(
      gateInput({ manifest: manifestOf({ edit: spec }), workloads: [regressed] }),
    );
    expect(fail.pass).toBe(false);
    // Raw samples are persisted into the result for reproducibility.
    const metric = fail.results[0].metrics[0];
    expect(metric.candidateSamples.length).toBe(60);
    expect(metric.baselineSamples.length).toBe(60);

    const equal: WorkloadEvaluationInput = {
      id: "edit",
      spec,
      available: true,
      candidate: side([sample({ distributions: { editLatency: [...baseDist] } })]),
      baseline: side([sample({ distributions: { editLatency: [...baseDist] } })]),
    };
    const ok = evaluateGate(
      gateInput({ manifest: manifestOf({ edit: spec }), workloads: [equal] }),
    );
    expect(ok.pass).toBe(true);
  });

  it("FAILS on a candidate/baseline diagnostic-SET or exit-code mismatch; PASSES on equality", () => {
    const spec: WorkloadSpec = { axis: "B", title: "cold", gated: {}, correctnessGated: true };

    const mismatch: WorkloadEvaluationInput = {
      id: "cold",
      spec,
      available: true,
      candidate: side([
        sample({ correctness: { exitCode: 0, diagnostics: ["a.ts(1,1): error TS2304"] } }),
      ]),
      baseline: side([
        sample({
          correctness: {
            exitCode: 0,
            diagnostics: ["a.ts(1,1): error TS2304", "b.ts(2,2): error TS2345"],
          },
        }),
      ]),
    };
    const fail = evaluateGate(
      gateInput({ manifest: manifestOf({ cold: spec }), workloads: [mismatch] }),
    );
    expect(fail.pass).toBe(false);
    expect(fail.failures.join(" ")).toMatch(/diagnostic|correctness|exit/i);

    const equal: WorkloadEvaluationInput = {
      id: "cold",
      spec,
      available: true,
      candidate: side([
        sample({ correctness: { exitCode: 2, diagnostics: ["a.ts(1,1): error TS2304"] } }),
      ]),
      baseline: side([
        sample({ correctness: { exitCode: 2, diagnostics: ["a.ts(1,1): error TS2304"] } }),
      ]),
    };
    const ok = evaluateGate(
      gateInput({ manifest: manifestOf({ cold: spec }), workloads: [equal] }),
    );
    expect(ok.pass).toBe(true);

    const exitMismatch: WorkloadEvaluationInput = {
      id: "cold",
      spec,
      available: true,
      candidate: side([
        sample({ correctness: { exitCode: 1, diagnostics: ["a.ts(1,1): error TS2304"] } }),
      ]),
      baseline: side([
        sample({ correctness: { exitCode: 0, diagnostics: ["a.ts(1,1): error TS2304"] } }),
      ]),
    };
    const efail = evaluateGate(
      gateInput({ manifest: manifestOf({ cold: spec }), workloads: [exitMismatch] }),
    );
    expect(efail.pass).toBe(false);
  });

  it("gates candidate-vs-baseline CONTENT-SET equality (contentEqualityGated): a hover-content / completion-label divergence FAILS at identical counts; identical passes; a missing set fails (smoke tolerates)", () => {
    // A counts-only IDE-query gate (hover hits / completion items) would let a
    // candidate returning bogus hover text + the same number of (differently-labeled)
    // completions pass. contentEqualityGated compares the candidate's normalized hover
    // CONTENT + completion LABEL SET against the baseline's, so without that content
    // rail a divergence would pass — here it FAILS.
    const spec: WorkloadSpec = {
      axis: "B",
      title: "ide",
      gated: {},
      contentEqualityGated: ["hoverContents", "completionLabels"],
    };
    const mk = (hovers: string[], labels: string[]): WorkloadSample =>
      sample({ contentSets: { hoverContents: hovers, completionLabels: labels } });

    // A hover-content divergence (same counts) FAILS.
    const hoverDiff: WorkloadEvaluationInput = {
      id: "ide",
      spec,
      available: true,
      candidate: side([mk(["0:Foo: number"], ["0:alpha"])]),
      baseline: side([mk(["0:Bar: string"], ["0:alpha"])]),
    };
    const f1 = evaluateGate(
      gateInput({ manifest: manifestOf({ ide: spec }), workloads: [hoverDiff] }),
    );
    expect(f1.pass).toBe(false);
    expect(f1.failures.join(" ")).toMatch(/hoverContents content/i);

    // A completion-label divergence (same item count) FAILS too.
    const labelDiff: WorkloadEvaluationInput = {
      id: "ide",
      spec,
      available: true,
      candidate: side([mk(["0:Foo"], ["0:alpha", "0:gamma"])]),
      baseline: side([mk(["0:Foo"], ["0:alpha", "0:DELTA"])]),
    };
    const f2 = evaluateGate(
      gateInput({ manifest: manifestOf({ ide: spec }), workloads: [labelDiff] }),
    );
    expect(f2.pass).toBe(false);
    expect(f2.failures.join(" ")).toMatch(/completionLabels content/i);

    // Identical content ⇒ pass (the positive control).
    const equal: WorkloadEvaluationInput = {
      id: "ide",
      spec,
      available: true,
      candidate: side([mk(["0:Foo"], ["0:alpha"])]),
      baseline: side([mk(["0:Foo"], ["0:alpha"])]),
    };
    expect(
      evaluateGate(gateInput({ manifest: manifestOf({ ide: spec }), workloads: [equal] })).pass,
    ).toBe(true);

    // A MISSING/empty content set on a full run FAILS (broken instrumentation);
    // smoke tolerates it.
    const missing: WorkloadEvaluationInput = {
      id: "ide",
      spec,
      available: true,
      candidate: side([sample({ contentSets: { hoverContents: [], completionLabels: ["0:a"] } })]),
      baseline: side([mk(["0:Foo"], ["0:a"])]),
    };
    expect(
      evaluateGate(gateInput({ manifest: manifestOf({ ide: spec }), workloads: [missing] })).pass,
    ).toBe(false);
    expect(
      evaluateGate(
        gateInput({ manifest: manifestOf({ ide: spec }), workloads: [missing], smoke: true }),
      ).pass,
    ).toBe(true);
  });

  it("AXIS-A carrier-content equality FAILS on a byte/count-preserving codegen content change where the byte+count invariants PASS; identical passes", () => {
    // The axis-A codegen invariants gated output_bytes + carrier COUNT, so a
    // byte-preserving WRONG-content carrier/source-map passed. The real manifest now
    // gates carrierContent (a content hash). Build a full axis-A sample whose
    // byte/count signals are IDENTICAL on both sides and ONLY the carrier CONTENT
    // hash differs — only the content rail must fail.
    const axisA = readBaselineManifest().workloads["axis-a-codegen"];
    expect(axisA.contentEqualityGated).toContain("carrierContent");
    const mk = (digest: string): WorkloadSample =>
      sample({
        metrics: { filesPerSec: 2000, carrierCount: 100, sfcCount: 100 },
        attribution: {
          codegenMs: 10,
          sourcemapMs: 5,
          codegenSourcemapMs: 15,
          parseTransformTransportMs: 5,
          nonCheckerMs: 20,
          outputBytes: 100_000,
          sourceMapBytes: 50_000,
          codeTransformOps: 10,
          peakRssBytes: 1,
        },
        contentSets: { carrierContent: [digest] },
      });
    const divergent: WorkloadEvaluationInput = {
      id: "axis-a-codegen",
      spec: axisA,
      available: true,
      candidate: side([mk("sha256:aaaa"), mk("sha256:aaaa")]),
      baseline: side([mk("sha256:bbbb"), mk("sha256:bbbb")]),
    };
    const f = evaluateGate(
      gateInput({ manifest: manifestOf({ "axis-a-codegen": axisA }), workloads: [divergent] }),
    );
    expect(f.pass).toBe(false);
    expect(f.failures.join(" ")).toMatch(/carrierContent content/i);
    // The byte/count invariants did NOT fire (identical both sides) — proving they
    // cannot catch a byte-preserving content change; only the content rail did.
    expect(f.failures.join(" ")).not.toMatch(
      /output_bytes|generated_carrier_count|source_map_bytes|coverage/i,
    );

    // Identical carrier content ⇒ pass (the positive control).
    const equal: WorkloadEvaluationInput = {
      id: "axis-a-codegen",
      spec: axisA,
      available: true,
      candidate: side([mk("sha256:aaaa"), mk("sha256:aaaa")]),
      baseline: side([mk("sha256:aaaa"), mk("sha256:aaaa")]),
    };
    expect(
      evaluateGate(
        gateInput({ manifest: manifestOf({ "axis-a-codegen": axisA }), workloads: [equal] }),
      ).pass,
    ).toBe(true);
  });

  it("does NOT present a green armed-gate when baselineRef is PENDING; FAILS an armed run that fell back to self-check", () => {
    // Unarmed (PENDING) self-check: a labeled self-check, a loud warning, no fail.
    const unarmed = evaluateGate(
      gateInput({
        manifest: manifestOf({}, { baselineRef: "PENDING — not armed" }),
        selfCheck: true,
      }),
    );
    expect(unarmed.mode).toBe("self-check");
    expect(unarmed.pass).toBe(true);
    expect(unarmed.warnings.join(" ")).toMatch(/not armed|self-check/i);

    // Armed (real ref) but the run fell back to self-check ⇒ baseline build is
    // missing ⇒ this must FAIL, never a silent green.
    const armedButSelfCheck = evaluateGate(
      gateInput({
        manifest: manifestOf({}, { baselineRef: PINNED_BASELINE_SHA }),
        selfCheck: true,
      }),
    );
    expect(armedButSelfCheck.pass).toBe(false);
    expect(armedButSelfCheck.failures.join(" ")).toMatch(/armed|self-check|baseline/i);

    // Armed + a real comparison ⇒ mode armed.
    const armed = evaluateGate(
      gateInput({
        manifest: manifestOf({}, { baselineRef: PINNED_BASELINE_SHA }),
        selfCheck: false,
      }),
    );
    expect(armed.mode).toBe("armed");
  });

  it("a NON-SHA non-PENDING baselineRef can NEVER arm: distinct sides + selfCheck=false FAILS (forced self-check)", () => {
    // A value that is neither PENDING nor a full 40-hex commit hash ("", "TODO", a
    // branch name, a moving tag, a 39-hex near-miss) is an UNPINNED ref. A `!PENDING`
    // check would read it as armed and a distinct-side run would produce a green armed
    // gate; instead it is forced to self-check and a non-self-check (distinct-side)
    // run HARD-FAILS — physically incapable of a green armed verdict.
    const armedMeta = {
      corpusHash: `sha256:${"0".repeat(64)}`,
      candidateBin: "cand-bin",
      baselineBin: "base-bin",
      candidateNative: "cand-native",
      baselineNative: "base-native",
      baselineRoot: "base-root",
      threads: 4,
      samplesPerSide: 7,
    };
    for (const ref of [
      "",
      "TODO",
      "main",
      "v1.2.3",
      "0123456789abcdef0123456789abcdef0123456", // 39 hex — one short of a SHA
    ]) {
      const attempt = evaluateGate(
        gateInput({
          manifest: manifestOf({}, { baselineRef: ref }),
          selfCheck: false, // distinct sides handed to an unpinned baseline
          meta: armedMeta,
        }),
      );
      expect(attempt.mode).toBe("self-check"); // never "armed"
      expect(attempt.mode).not.toBe("armed");
      expect(attempt.pass).toBe(false);
      expect(attempt.warnings.join(" ")).toMatch(/not armed|40-hex|pinned/i);
    }
    // The control: a full 40-hex SHA with the same distinct sides DOES arm green.
    const armedSha = evaluateGate(
      gateInput({
        manifest: manifestOf({}, { baselineRef: PINNED_BASELINE_SHA }),
        selfCheck: false,
        meta: armedMeta,
      }),
    );
    expect(armedSha.mode).toBe("armed");
    expect(armedSha.pass).toBe(true);
  });

  it("FAILS on an engine-version mismatch on a full run; PASSES on a match", () => {
    const mismatch = evaluateGate(
      gateInput({
        engineResolved: "typescript@6.0.3",
        manifest: manifestOf({}, { tsgoVersion: "typescript@7.0.1-rc" }),
      }),
    );
    expect(mismatch.pass).toBe(false);
    expect(mismatch.failures.join(" ")).toMatch(/engine|tsgo|version/i);

    const ok = evaluateGate(
      gateInput({
        engineResolved: "typescript@7.0.1-rc",
        manifest: manifestOf({}, { tsgoVersion: "typescript@7.0.1-rc" }),
      }),
    );
    expect(ok.pass).toBe(true);
  });

  it("FAILS a behavioral single-file-edit invariant when an edit re-publishes diagnostics for ~the whole project; PASSES a localized edit", () => {
    const spec: WorkloadSpec = {
      axis: "B",
      title: "edit",
      gated: {},
      behavioral: { maxAffectedUriFraction: 0.25 },
    };
    const wide: WorkloadEvaluationInput = {
      id: "edit",
      spec,
      available: true,
      candidate: side([sample({ behavioral: { affectedUris: 90, totalUris: 100 } })]),
      baseline: side([sample({ behavioral: { affectedUris: 1, totalUris: 100 } })]),
    };
    const fail = evaluateGate(
      gateInput({ manifest: manifestOf({ edit: spec }), workloads: [wide] }),
    );
    expect(fail.pass).toBe(false);
    expect(fail.failures.join(" ")).toMatch(/affect|publication|behavioral|locality/i);

    const localized: WorkloadEvaluationInput = {
      id: "edit",
      spec,
      available: true,
      candidate: side([sample({ behavioral: { affectedUris: 2, totalUris: 100 } })]),
      baseline: side([sample({ behavioral: { affectedUris: 2, totalUris: 100 } })]),
    };
    const ok = evaluateGate(
      gateInput({ manifest: manifestOf({ edit: spec }), workloads: [localized] }),
    );
    expect(ok.pass).toBe(true);
  });

  describe("locality fails on a missing/degenerate denominator (never fraction 0)", () => {
    const localitySpec: WorkloadSpec = {
      axis: "B",
      title: "edit",
      gated: {},
      behavioral: { maxAffectedUriFraction: 0.25 },
    };

    it("FAILS a full run when the locality denominator (totalUris) is zero on the candidate", () => {
      // totalUris 0 ⇒ a coerced-to-null `candFrac` would SKIP the threshold check,
      // so a degenerate/missing denominator would pass AS perfect locality. A full
      // run must HARD-FAIL: the locality invariant cannot be certified without a denominator.
      const wl: WorkloadEvaluationInput = {
        id: "edit",
        spec: localitySpec,
        available: true,
        candidate: side([sample({ behavioral: { affectedUris: 5, totalUris: 0 } })]),
        baseline: side([sample({ behavioral: { affectedUris: 1, totalUris: 100 } })]),
      };
      const r = evaluateGate(
        gateInput({ manifest: manifestOf({ edit: localitySpec }), workloads: [wl], smoke: false }),
      );
      expect(r.pass).toBe(false);
      expect(r.failures.join(" ")).toMatch(/denominator|totalUris|locality/i);
    });

    it("FAILS a full run when the locality denominator is zero on the baseline", () => {
      const wl: WorkloadEvaluationInput = {
        id: "edit",
        spec: localitySpec,
        available: true,
        candidate: side([sample({ behavioral: { affectedUris: 1, totalUris: 100 } })]),
        baseline: side([sample({ behavioral: { affectedUris: 5, totalUris: 0 } })]),
      };
      const r = evaluateGate(
        gateInput({ manifest: manifestOf({ edit: localitySpec }), workloads: [wl], smoke: false }),
      );
      expect(r.pass).toBe(false);
      expect(r.failures.join(" ")).toMatch(/denominator|totalUris|locality/i);
    });

    it("only WARNS (does not hard-fail) on a zero denominator under smoke", () => {
      const wl: WorkloadEvaluationInput = {
        id: "edit",
        spec: localitySpec,
        available: true,
        candidate: side([sample({ behavioral: { affectedUris: 5, totalUris: 0 } })]),
        baseline: side([sample({ behavioral: { affectedUris: 1, totalUris: 100 } })]),
      };
      const r = evaluateGate(
        gateInput({ manifest: manifestOf({ edit: localitySpec }), workloads: [wl], smoke: true }),
      );
      expect(r.warnings.join(" ")).toMatch(/denominator|totalUris|locality/i);
      expect(r.failures.join(" ")).not.toMatch(/denominator|totalUris/i);
    });

    it("PASSES a full run when both sides report a real denominator within threshold (control)", () => {
      const wl: WorkloadEvaluationInput = {
        id: "edit",
        spec: localitySpec,
        available: true,
        candidate: side([sample({ behavioral: { affectedUris: 2, totalUris: 100 } })]),
        baseline: side([sample({ behavioral: { affectedUris: 2, totalUris: 100 } })]),
      };
      const r = evaluateGate(
        gateInput({ manifest: manifestOf({ edit: localitySpec }), workloads: [wl], smoke: false }),
      );
      expect(r.pass).toBe(true);
    });
  });

  it("a same-build self-check across a multi-metric workload yields ~1.0 and never false-fails", () => {
    const spec: WorkloadSpec = {
      axis: "A",
      title: "axisA",
      gated: {
        compile_throughput_ratio: {
          threshold: 1.1,
          direction: "higher-is-better",
          statistic: "median",
          source: { kind: "scalar", key: "filesPerSec" },
        },
        peak_rss_ratio: {
          threshold: 1.1,
          direction: "lower-is-better",
          statistic: "median",
          source: { kind: "rss" },
        },
        p95_latency_ratio: {
          threshold: 1.12,
          direction: "lower-is-better",
          statistic: "p95",
          source: { kind: "distribution", key: "editLatency" },
        },
      },
    };
    const mk = () =>
      side([
        sample({
          rssBytes: 100_000_000,
          metrics: { filesPerSec: 1800 },
          distributions: { editLatency: [10, 11, 12, 13, 50] },
        }),
        sample({
          rssBytes: 101_000_000,
          metrics: { filesPerSec: 1820 },
          distributions: { editLatency: [10, 11, 12, 14, 52] },
        }),
      ]);
    const wl: WorkloadEvaluationInput = {
      id: "axisA",
      spec,
      available: true,
      candidate: mk(),
      baseline: mk(),
    };
    const report = evaluateGate(
      gateInput({ manifest: manifestOf({ axisA: spec }), workloads: [wl], selfCheck: true }),
    );
    expect(report.pass).toBe(true);
    expect(report.failures).toHaveLength(0);
  });

  // ── Honesty invariants: a broken/cheating/crashed/skipped/self-measured
  //    candidate must NEVER read green. ──────────────────────────────────────

  it("FAILS a full run when a manifest workload was never evaluated (dropped producer / typo)", () => {
    const a: WorkloadSpec = { axis: "A", title: "a", gated: {} };
    const b: WorkloadSpec = { axis: "B", title: "b", gated: {} };
    const wlA: WorkloadEvaluationInput = {
      id: "a",
      spec: a,
      available: true,
      candidate: side([sample()]),
      baseline: side([sample()]),
    };
    // Manifest declares {a,b}; only `a` was evaluated ⇒ `b` silently vanished.
    const full = evaluateGate(gateInput({ manifest: manifestOf({ a, b }), workloads: [wlA] }));
    expect(full.pass).toBe(false);
    expect(full.failures.join(" ")).toMatch(/not evaluated/i);
    expect(full.failures.join(" ")).toMatch(/\bb\b/);
    // Smoke tolerates a partially-evaluated manifest.
    const smoke = evaluateGate(
      gateInput({ manifest: manifestOf({ a, b }), workloads: [wlA], smoke: true }),
    );
    expect(smoke.pass).toBe(true);
    // All declared workloads evaluated ⇒ no completeness failure.
    const wlB: WorkloadEvaluationInput = {
      id: "b",
      spec: b,
      available: true,
      candidate: side([sample()]),
      baseline: side([sample()]),
    };
    expect(
      evaluateGate(gateInput({ manifest: manifestOf({ a, b }), workloads: [wlA, wlB] })).pass,
    ).toBe(true);
  });

  it("validates EVERY correctness sample — a NON-first mismatch FAILS", () => {
    const spec: WorkloadSpec = { axis: "B", title: "cold", gated: {}, correctnessGated: true };
    const ok = (): WorkloadSample =>
      sample({ correctness: { exitCode: 0, diagnostics: ["a.ts(1,1): error TS2304"] } });
    const diverged = (): WorkloadSample =>
      sample({
        correctness: {
          exitCode: 0,
          diagnostics: ["a.ts(1,1): error TS2304", "b.ts(2,2): error TS2345"],
        },
      });
    const wl: WorkloadEvaluationInput = {
      id: "cold",
      spec,
      available: true,
      candidate: side([ok(), ok(), diverged()]), // the 3rd sample regresses
      baseline: side([ok(), ok(), ok()]),
    };
    const fail = evaluateGate(gateInput({ manifest: manifestOf({ cold: spec }), workloads: [wl] }));
    expect(fail.pass).toBe(false);
    expect(fail.failures.join(" ")).toMatch(/correctness|diagnostic/i);

    const allOk: WorkloadEvaluationInput = {
      id: "cold",
      spec,
      available: true,
      candidate: side([ok(), ok(), ok()]),
      baseline: side([ok(), ok(), ok()]),
    };
    expect(
      evaluateGate(gateInput({ manifest: manifestOf({ cold: spec }), workloads: [allOk] })).pass,
    ).toBe(true);
  });

  it("FAILS when a correctness-gated sample is MISSING correctness data on a full run", () => {
    const spec: WorkloadSpec = { axis: "B", title: "cold", gated: {}, correctnessGated: true };
    const wl: WorkloadEvaluationInput = {
      id: "cold",
      spec,
      available: true,
      // The 2nd candidate sample carries no correctness payload.
      candidate: side([sample({ correctness: { exitCode: 0, diagnostics: [] } }), sample()]),
      baseline: side([
        sample({ correctness: { exitCode: 0, diagnostics: [] } }),
        sample({ correctness: { exitCode: 0, diagnostics: [] } }),
      ]),
    };
    expect(
      evaluateGate(gateInput({ manifest: manifestOf({ cold: spec }), workloads: [wl] })).pass,
    ).toBe(false);
  });

  it("gates behavioral locality on the WORST sample, not the first", () => {
    const spec: WorkloadSpec = {
      axis: "B",
      title: "edit",
      gated: {},
      behavioral: { maxAffectedUriFraction: 0.25 },
    };
    const wl: WorkloadEvaluationInput = {
      id: "edit",
      spec,
      available: true,
      candidate: side([
        sample({ behavioral: { affectedUris: 1, totalUris: 100 } }), // local
        sample({ behavioral: { affectedUris: 90, totalUris: 100 } }), // whole-project burst
      ]),
      baseline: side([
        sample({ behavioral: { affectedUris: 1, totalUris: 100 } }),
        sample({ behavioral: { affectedUris: 1, totalUris: 100 } }),
      ]),
    };
    const fail = evaluateGate(gateInput({ manifest: manifestOf({ edit: spec }), workloads: [wl] }));
    expect(fail.pass).toBe(false);
    expect(fail.failures.join(" ")).toMatch(/locality|publication|affect/i);
  });

  it("FAILS an armed run whose baseline sides are missing or equal (silent self-comparison)", () => {
    const armedMeta = (over: Record<string, string | undefined>) => ({
      corpusHash: `sha256:${"0".repeat(64)}`,
      candidateBin: "cand-bin",
      baselineBin: "base-bin",
      candidateNative: "cand-native",
      baselineNative: "base-native",
      baselineRoot: "base-root",
      threads: 4,
      samplesPerSide: 7,
      ...over,
    });
    // Equal natives ⇒ axis A would self-compare while reporting armed.
    const equalNative = evaluateGate(
      gateInput({
        manifest: manifestOf({}, { baselineRef: PINNED_BASELINE_SHA }),
        selfCheck: false,
        meta: armedMeta({ baselineNative: "cand-native" }),
      }),
    );
    expect(equalNative.pass).toBe(false);
    expect(equalNative.failures.join(" ")).toMatch(/distinct|native|self-compar/i);

    // Missing baseline bin ⇒ axis B would self-compare.
    const missingBin = evaluateGate(
      gateInput({
        manifest: manifestOf({}, { baselineRef: PINNED_BASELINE_SHA }),
        selfCheck: false,
        meta: armedMeta({ baselineBin: undefined }),
      }),
    );
    expect(missingBin.pass).toBe(false);

    // Distinct, present sides ⇒ no self-comparison failure.
    const distinct = evaluateGate(
      gateInput({
        manifest: manifestOf({}, { baselineRef: PINNED_BASELINE_SHA }),
        selfCheck: false,
        meta: armedMeta({}),
      }),
    );
    expect(distinct.pass).toBe(true);
  });

  it("rejects armed sides that resolve to the SAME path via a trailing slash / case-only difference, and a baselineRoot equal to the candidate root", () => {
    // The distinctness guard compared RAW path strings, so a trailing slash, a
    // case-only difference (case-insensitive FS), or a symlink could evade it and
    // present a green ARMED gate that was actually a candidate-vs-candidate
    // self-comparison. The guard now compares RESOLVED paths.
    const armedMeta = (over: Record<string, string | undefined>) => ({
      corpusHash: `sha256:${"0".repeat(64)}`,
      candidateBin: "/work/cand/bin",
      baselineBin: "/work/base/bin",
      candidateNative: "/work/cand/native",
      baselineNative: "/work/base/native",
      candidateRoot: "/work/cand",
      baselineRoot: "/work/base",
      threads: 4,
      samplesPerSide: 7,
      ...over,
    });
    const armed = (over: Record<string, string | undefined>) =>
      evaluateGate(
        gateInput({
          manifest: manifestOf({}, { baselineRef: PINNED_BASELINE_SHA }),
          selfCheck: false,
          meta: armedMeta(over),
        }),
      );

    // Trailing-slash binary dir — same resolved path; a raw === would see them as
    // distinct and let the armed run pass.
    const trailingBin = armed({ baselineBin: "/work/cand/bin/" });
    expect(trailingBin.pass).toBe(false);
    expect(trailingBin.failures.join(" ")).toMatch(/distinct.*binary|binary.*distinct/i);

    // Case-only native difference — same resolved path on a case-insensitive FS.
    const caseNative = armed({
      baselineNative: "/WORK/CAND/NATIVE",
      candidateNative: "/work/cand/native",
    });
    expect(caseNative.pass).toBe(false);
    expect(caseNative.failures.join(" ")).toMatch(/distinct.*native|native.*distinct/i);

    // baselineRoot equal to the candidate root (trailing slash) — a separate baseline
    // worktree is required; the candidate root is not it.
    const sameRoot = armed({ baselineRoot: "/work/cand/" });
    expect(sameRoot.pass).toBe(false);
    expect(sameRoot.failures.join(" ")).toMatch(/root/i);

    // Control: genuinely distinct resolved sides + a separate baseline root ⇒ pass.
    expect(armed({}).pass).toBe(true);
  });

  it("FAILS a full run with fewer samples than the manifest floor; smoke tolerates it", () => {
    const meta = (n: number) => ({
      corpusHash: `sha256:${"0".repeat(64)}`,
      candidateBin: "c",
      baselineBin: "b",
      threads: 4,
      samplesPerSide: n,
    });
    const under = evaluateGate(
      gateInput({ manifest: manifestOf({}, { samplesPerSide: 7 }), meta: meta(3) }),
    );
    expect(under.pass).toBe(false);
    expect(under.failures.join(" ")).toMatch(/sample/i);
    expect(
      evaluateGate(gateInput({ manifest: manifestOf({}, { samplesPerSide: 7 }), meta: meta(7) }))
        .pass,
    ).toBe(true);
    expect(
      evaluateGate(
        gateInput({ manifest: manifestOf({}, { samplesPerSide: 7 }), smoke: true, meta: meta(3) }),
      ).pass,
    ).toBe(true);
  });

  it("FAILS when the baseline worktree resolves a DIFFERENT engine than the candidate", () => {
    const mismatch = evaluateGate(
      gateInput({
        engineResolved: "typescript@7.0.1-rc",
        baselineEngineResolved: "typescript@7.0.0", // baseline worktree drifted
        manifest: manifestOf({}, { tsgoVersion: "typescript@7.0.1-rc" }),
      }),
    );
    expect(mismatch.pass).toBe(false);
    expect(mismatch.failures.join(" ")).toMatch(/engine|baseline|version/i);

    const ok = evaluateGate(
      gateInput({
        engineResolved: "typescript@7.0.1-rc",
        baselineEngineResolved: "typescript@7.0.1-rc",
        manifest: manifestOf({}, { tsgoVersion: "typescript@7.0.1-rc" }),
      }),
    );
    expect(ok.pass).toBe(true);
  });

  it("FAILS a full run when a gated metric vector contains NaN/Infinity (not a green pass)", () => {
    const spec: WorkloadSpec = {
      axis: "A",
      title: "axisA",
      gated: {
        compile_throughput_ratio: {
          threshold: 1.1,
          direction: "higher-is-better",
          statistic: "median",
          source: { kind: "scalar", key: "filesPerSec" },
        },
      },
    };
    const withCand = (v: number): WorkloadEvaluationInput => ({
      id: "axisA",
      spec,
      available: true,
      candidate: side([sample({ metrics: { filesPerSec: v } })]),
      baseline: side([sample({ metrics: { filesPerSec: 2000 } })]),
    });
    const nan = evaluateGate(
      gateInput({ manifest: manifestOf({ axisA: spec }), workloads: [withCand(Number.NaN)] }),
    );
    expect(nan.pass).toBe(false);
    expect(nan.failures.join(" ")).toMatch(/non-finite|nan|infinity|degenerate|instrument/i);
    expect(
      evaluateGate(
        gateInput({
          manifest: manifestOf({ axisA: spec }),
          workloads: [withCand(Number.POSITIVE_INFINITY)],
        }),
      ).pass,
    ).toBe(false);
  });

  it("gates a carrier count as a two-sided INVARIANT: a DROP fails, equal passes, a bloat fails", () => {
    const spec: WorkloadSpec = {
      axis: "A",
      title: "axisA",
      gated: {
        generated_carrier_count: {
          threshold: 1.01,
          direction: "invariant",
          statistic: "median",
          source: { kind: "scalar", key: "carrierCount" },
        },
      },
    };
    const mk = (n: number): WorkloadEvaluationInput => ({
      id: "axisA",
      spec,
      available: true,
      candidate: side([
        sample({ metrics: { carrierCount: n } }),
        sample({ metrics: { carrierCount: n } }),
      ]),
      baseline: side([
        sample({ metrics: { carrierCount: 100 } }),
        sample({ metrics: { carrierCount: 100 } }),
      ]),
    });
    // A DROP (candidate skipped carriers) FAILS — "lower" is NOT a perf win here.
    expect(
      evaluateGate(gateInput({ manifest: manifestOf({ axisA: spec }), workloads: [mk(80)] })).pass,
    ).toBe(false);
    // Equal counts pass.
    expect(
      evaluateGate(gateInput({ manifest: manifestOf({ axisA: spec }), workloads: [mk(100)] })).pass,
    ).toBe(true);
    // A bloat also fails.
    expect(
      evaluateGate(gateInput({ manifest: manifestOf({ axisA: spec }), workloads: [mk(120)] })).pass,
    ).toBe(false);
  });

  it("gates source_map_bytes as a two-sided INVARIANT: a candidate DROP FAILS (a truncated/omitted map breaks IDE position mapping — a correctness regression, not a perf win)", () => {
    // source_map_bytes must NOT be direction lower-is-better — under that a candidate
    // that truncated/omitted source maps would emit FEWER bytes and read as a perf
    // WIN. It is a two-sided invariant (mirroring output_bytes): a DROP and a BLOAT both
    // fail. This reads the REAL manifest spec so it discriminates the block6.json
    // change (not just synthetic ratio math).
    const axisA = readBaselineManifest().workloads["axis-a-codegen"];
    expect(axisA.gated.source_map_bytes.direction).toBe("invariant");
    const spec: WorkloadSpec = {
      axis: "A",
      title: "axisA",
      gated: { source_map_bytes: axisA.gated.source_map_bytes },
    };
    const attr = (sourceMapBytes: number): WorkloadSample =>
      sample({
        attribution: {
          codegenMs: 10,
          sourcemapMs: 5,
          codegenSourcemapMs: 15,
          parseTransformTransportMs: 5,
          nonCheckerMs: 20,
          outputBytes: 100_000,
          sourceMapBytes,
          codeTransformOps: 10,
          peakRssBytes: 1,
        },
      });
    const evalAt = (cand: number, base: number): boolean =>
      evaluateGate(
        gateInput({
          manifest: manifestOf({ axisA: spec }),
          workloads: [
            {
              id: "axisA",
              spec,
              available: true,
              candidate: side([attr(cand), attr(cand)]),
              baseline: side([attr(base), attr(base)]),
            },
          ],
        }),
      ).pass;
    // A 30% DROP fails (truncated/omitted source maps), with a named failure.
    const dropped = evaluateGate(
      gateInput({
        manifest: manifestOf({ axisA: spec }),
        workloads: [
          {
            id: "axisA",
            spec,
            available: true,
            candidate: side([attr(700), attr(700)]),
            baseline: side([attr(1000), attr(1000)]),
          },
        ],
      }),
    );
    expect(dropped.pass).toBe(false);
    expect(dropped.failures.join(" ")).toMatch(/source_map_bytes/);
    // Equal bytes pass; a 30% bloat also fails (two-sided).
    expect(evalAt(1000, 1000)).toBe(true);
    expect(evalAt(1300, 1000)).toBe(false);
  });

  it("AXIS A drives each side's OWN --native root through the real spawn seam, and a per-side-native regression FAILS through axisACodegen.runOnce → evaluateGate", async () => {
    // This is the spec that catches the "candidate native loaded on BOTH sides"
    // bug: it exercises the REAL axisACodegen.runOnce → evaluateGate path via an
    // injectable child-spawn seam, asserting (a) the candidate side is invoked
    // with the candidate --native root and the baseline side with the DISTINCT
    // baseline root, and (b) a synthetic axis-A regression fails the gate — while
    // a native-BLIND seam (identical metrics regardless of root, i.e. the bug)
    // does NOT trip the gate, proving the failure is caused by the genuine
    // per-side native wiring, not by a hand-built metric vector.
    const candidateNative = "/candidate/packages/native";
    const baselineNative = "/baseline/packages/native";
    const corpus = { dir: "/corpus" } as EnsuredCorpus;

    const fast: AxisAChildSample = {
      totalMs: 100,
      filesPerSec: 2000,
      outputBytes: 100_000,
      sourceMapBytes: 50_000,
      codeTransformOps: 10,
      nonCheckerMs: 20,
      codegenMs: 10,
      sourcemapMs: 5,
      parseTransformTransportMs: 5,
      carrierCount: 100,
      peakRssBytes: 100_000_000,
      sfcCount: 100,
      // A present carrier-content hash so this fixture (which exercises the per-side
      // native throughput/bytes/count wiring) keeps the content-equality rail at
      // PARITY; an absent hash correctly surfaces as an empty UNAVAILABLE set.
      carrierContentHash: `sha256:${"a".repeat(64)}`,
    };
    // A decisive per-side-native regression: slower throughput, more non-checker
    // time, a larger source-map, fewer carriers, more memory.
    const regressed: AxisAChildSample = {
      ...fast,
      filesPerSec: 1200,
      nonCheckerMs: 60,
      sourcemapMs: 30,
      sourceMapBytes: 90_000,
      carrierCount: 80,
      peakRssBytes: 160_000_000,
    };

    const nativeOf = (argv: readonly string[]): string => {
      const i = argv.indexOf("--native");
      expect(i).toBeGreaterThanOrEqual(0); // the real argv carries --native <root>
      expect(argv).toContain("--corpus"); // …and --corpus <dir>
      expect(argv).toContain("--threads");
      return argv[i + 1];
    };

    const N = 7;
    const runN = async (
      runner: AxisAChildRunner,
      nativeRoot: string,
    ): Promise<WorkloadSample[]> => {
      const ctx: WorkloadContext = { corpus, nativeRoot, threads: 4, axisAChildRunner: runner };
      const out: WorkloadSample[] = [];
      for (let i = 0; i < N; i++) out.push(await axisACodegen.runOnce(ctx));
      return out;
    };

    const axisASpec = readBaselineManifest().workloads["axis-a-codegen"];
    const evalAxisA = (cand: WorkloadSample[], base: WorkloadSample[]) =>
      evaluateGate(
        gateInput({
          manifest: manifestOf({ "axis-a-codegen": axisASpec }),
          workloads: [
            {
              id: "axis-a-codegen",
              spec: axisASpec,
              available: true,
              candidate: { samples: cand },
              baseline: { samples: base },
            },
          ],
        }),
      );

    // Native-AWARE seam: the candidate root regresses, the baseline root is fast.
    const seenRoots: string[] = [];
    const aware: AxisAChildRunner = (inv) => {
      const root = nativeOf(inv.argv);
      seenRoots.push(root);
      return root === candidateNative ? regressed : fast;
    };
    const candAware = await runN(aware, candidateNative);
    const baseAware = await runN(aware, baselineNative);

    // (a) DISTINCT --native roots actually flowed to the child spawn seam.
    expect(seenRoots).toContain(candidateNative);
    expect(seenRoots).toContain(baselineNative);
    expect(new Set(seenRoots).size).toBe(2);

    // (b) the regression flows through the REAL gated axis-A metric set and FAILS.
    const failed = evalAxisA(candAware, baseAware);
    expect(failed.pass).toBe(false);
    expect(failed.failures.join(" ")).toMatch(/axis-a-codegen/);

    // Discriminator: a native-BLIND seam (the "candidate native on both sides"
    // bug → identical metrics regardless of root) does NOT trip the gate, proving
    // the failure above is caused by the genuine per-side native wiring.
    const blind: AxisAChildRunner = () => fast;
    const candBlind = await runN(blind, candidateNative);
    const baseBlind = await runN(blind, baselineNative);
    expect(evalAxisA(candBlind, baseBlind).pass).toBe(true);
  });

  // ── A PENDING baseline can NEVER produce a green armed gate ────────────────
  it("a PENDING baseline run handed DISTINCT side paths FAILS and is labeled self-check — never a green armed gate", () => {
    const pendingMeta = {
      corpusHash: `sha256:${"0".repeat(64)}`,
      candidateBin: "cand",
      baselineBin: "base",
      candidateNative: "cand-native",
      baselineNative: "base-native",
      baselineRoot: "base-root", // present, so the failure is the PENDING rule, isolated
      threads: 4,
      samplesPerSide: 7,
    };
    const armedAttempt = evaluateGate(
      gateInput({
        manifest: manifestOf({}, { baselineRef: "PENDING — set on first arm" }),
        selfCheck: false, // distinct sides handed to an UNPINNED baseline
        meta: pendingMeta,
      }),
    );
    // Physically incapable of an "armed" verdict: forced to self-check + HARD-FAIL.
    expect(armedAttempt.mode).toBe("self-check");
    expect(armedAttempt.mode).not.toBe("armed");
    expect(armedAttempt.pass).toBe(false);
    expect(armedAttempt.failures.join(" ")).toMatch(/pending|non-self-check|self-check/i);

    // The legitimate PENDING self-check still passes with a loud NOT-ARMED warning.
    const selfCheck = evaluateGate(
      gateInput({
        manifest: manifestOf({}, { baselineRef: "PENDING — set on first arm" }),
        selfCheck: true,
      }),
    );
    expect(selfCheck.mode).toBe("self-check");
    expect(selfCheck.pass).toBe(true);
    expect(selfCheck.warnings.join(" ")).toMatch(/not armed/i);
  });

  // ── An armed run REQUIRES --baseline-root (never the candidate root) ───────
  it("an armed run REQUIRES --baseline-root; without it the gate FAILS rather than resolve the baseline engine from the candidate root", () => {
    const armedMetaSansRoot = {
      corpusHash: `sha256:${"0".repeat(64)}`,
      candidateBin: "cand",
      baselineBin: "base",
      candidateNative: "cand-native",
      baselineNative: "base-native",
      // baselineRoot intentionally omitted
      threads: 4,
      samplesPerSide: 7,
    };
    const noRoot = evaluateGate(
      gateInput({
        manifest: manifestOf({}, { baselineRef: PINNED_BASELINE_SHA }),
        selfCheck: false,
        meta: armedMetaSansRoot,
      }),
    );
    expect(noRoot.pass).toBe(false);
    expect(noRoot.failures.join(" ")).toMatch(/baseline-root/i);

    // With --baseline-root present, the baseline-root rule does not fire.
    const withRoot = evaluateGate(
      gateInput({
        manifest: manifestOf({}, { baselineRef: PINNED_BASELINE_SHA }),
        selfCheck: false,
        meta: { ...armedMetaSansRoot, baselineRoot: "base-root" },
      }),
    );
    expect(withRoot.failures.join(" ")).not.toMatch(/baseline-root/i);
    expect(withRoot.pass).toBe(true);

    // Self-check + smoke need no baseline root (the baseline IS the candidate root).
    const selfCheckSansRoot = evaluateGate(
      gateInput({
        manifest: manifestOf({}, { baselineRef: "PENDING — self-check" }),
        selfCheck: true,
        meta: { ...armedMetaSansRoot, candidateBin: "x", baselineBin: "x" },
      }),
    );
    expect(selfCheckSansRoot.failures.join(" ")).not.toMatch(/baseline-root/i);
    expect(selfCheckSansRoot.pass).toBe(true);
  });

  // ── Per-sample presence: PARTIALLY-missing instrumentation FAILS ───────────
  it("FAILS a full run on PARTIALLY-missing gated instrumentation (one sample missing the payload) — uniformly across scalar / distribution / attribution; smoke tolerates; all-present passes", () => {
    // SCALAR — one candidate sample is missing filesPerSec (others present). Coercing
    // it to 0 would let the median still pass; here it is a hard fail.
    const scalarSpec: WorkloadSpec = {
      axis: "A",
      title: "axisA",
      gated: {
        compile_throughput_ratio: {
          threshold: 1.1,
          direction: "higher-is-better",
          statistic: "median",
          source: { kind: "scalar", key: "filesPerSec" },
        },
      },
    };
    const partialScalar: WorkloadEvaluationInput = {
      id: "axisA",
      spec: scalarSpec,
      available: true,
      candidate: side([
        sample({ metrics: { filesPerSec: 2000 } }),
        sample({ metrics: {} }), // MISSING the gated payload
        sample({ metrics: { filesPerSec: 2000 } }),
      ]),
      baseline: side([
        sample({ metrics: { filesPerSec: 2000 } }),
        sample({ metrics: { filesPerSec: 2000 } }),
        sample({ metrics: { filesPerSec: 2000 } }),
      ]),
    };
    const full = evaluateGate(
      gateInput({ manifest: manifestOf({ axisA: scalarSpec }), workloads: [partialScalar] }),
    );
    expect(full.pass).toBe(false);
    expect(full.failures.join(" ")).toMatch(/missing|instrumentation/i);
    // Smoke tolerates the partial.
    expect(
      evaluateGate(
        gateInput({
          manifest: manifestOf({ axisA: scalarSpec }),
          workloads: [partialScalar],
          smoke: true,
        }),
      ).pass,
    ).toBe(true);

    // DISTRIBUTION — one baseline sample is missing its per-operation distribution.
    const distSpec: WorkloadSpec = {
      axis: "B",
      title: "edit",
      gated: {
        p95_latency_ratio: {
          threshold: 1.12,
          direction: "lower-is-better",
          statistic: "p95",
          source: { kind: "distribution", key: "editLatency" },
        },
      },
    };
    const d = Array.from({ length: 30 }, () => 100);
    const partialDist: WorkloadEvaluationInput = {
      id: "edit",
      spec: distSpec,
      available: true,
      candidate: side([
        sample({ distributions: { editLatency: [...d] } }),
        sample({ distributions: { editLatency: [...d] } }),
      ]),
      baseline: side([
        sample({ distributions: { editLatency: [...d] } }),
        sample(), // MISSING the per-operation distribution
      ]),
    };
    expect(
      evaluateGate(
        gateInput({ manifest: manifestOf({ edit: distSpec }), workloads: [partialDist] }),
      ).pass,
    ).toBe(false);

    // ATTRIBUTION — one candidate sample is missing the attribution payload.
    const attrSpec: WorkloadSpec = {
      axis: "A",
      title: "axisA",
      gated: {
        non_checker_time_ratio: {
          threshold: 1.1,
          direction: "lower-is-better",
          statistic: "median",
          source: { kind: "attribution", key: "nonCheckerMs" },
        },
      },
    };
    const attr = (ms: number | null): WorkloadSample =>
      sample({
        attribution:
          ms === null
            ? null
            : {
                codegenMs: 0,
                sourcemapMs: 0,
                parseTransformTransportMs: 0,
                nonCheckerMs: ms,
                outputBytes: 1,
                sourceMapBytes: 1,
                codeTransformOps: 1,
                peakRssBytes: 1,
              },
      });
    const partialAttr: WorkloadEvaluationInput = {
      id: "axisA",
      spec: attrSpec,
      available: true,
      candidate: side([attr(20), attr(null)]), // 2nd missing the attribution
      baseline: side([attr(20), attr(20)]),
    };
    expect(
      evaluateGate(
        gateInput({ manifest: manifestOf({ axisA: attrSpec }), workloads: [partialAttr] }),
      ).pass,
    ).toBe(false);

    // CONTROL — every sample present ⇒ evaluated, no false missing-fail.
    const allPresent: WorkloadEvaluationInput = {
      id: "axisA",
      spec: scalarSpec,
      available: true,
      candidate: side([
        sample({ metrics: { filesPerSec: 2000 } }),
        sample({ metrics: { filesPerSec: 2000 } }),
      ]),
      baseline: side([
        sample({ metrics: { filesPerSec: 2000 } }),
        sample({ metrics: { filesPerSec: 2000 } }),
      ]),
    };
    expect(
      evaluateGate(
        gateInput({ manifest: manifestOf({ axisA: scalarSpec }), workloads: [allPresent] }),
      ).pass,
    ).toBe(true);
  });

  // ── RSS presence: a single null / ≤0 sample FAILS, on EITHER side ──────────
  it("FAILS a full run on ONE null-or-≤0 RSS sample (others present) — uniformly on candidate AND baseline; ≤0 is missing", () => {
    const spec: WorkloadSpec = {
      axis: "A",
      title: "axisA",
      gated: {
        peak_rss_ratio: {
          threshold: 1.1,
          direction: "lower-is-better",
          statistic: "median",
          source: { kind: "rss" },
        },
      },
    };
    const rss = (v: number | null): WorkloadSample => sample({ rssBytes: v });
    const evalRss = (cand: WorkloadSample[], base: WorkloadSample[], smoke = false): boolean =>
      evaluateGate(
        gateInput({
          manifest: manifestOf({ axisA: spec }),
          workloads: [
            { id: "axisA", spec, available: true, candidate: side(cand), baseline: side(base) },
          ],
          smoke,
        }),
      ).pass;

    // CANDIDATE has one NULL RSS sample among present ones ⇒ a full run FAILS (the
    // producer surfaces null, the gate reads it as missing — not averaged-in 0).
    expect(
      evalRss(
        [rss(100_000_000), rss(null), rss(100_000_000)],
        [rss(100_000_000), rss(100_000_000), rss(100_000_000)],
      ),
    ).toBe(false);
    // BASELINE side, same rule (EITHER side missing fails) — and a ≤0 reading is
    // ALSO missing (RSS is never legitimately 0).
    expect(evalRss([rss(100_000_000), rss(100_000_000)], [rss(100_000_000), rss(0)])).toBe(false);
    // Smoke tolerates the partial; all-present passes.
    expect(evalRss([rss(100_000_000), rss(null)], [rss(100_000_000), rss(100_000_000)], true)).toBe(
      true,
    );
    expect(
      evalRss([rss(100_000_000), rss(101_000_000)], [rss(100_000_000), rss(101_000_000)]),
    ).toBe(true);
  });

  // ── Distribution completeness: a partial per-operation distribution FAILS ──
  it("FAILS a full run when a per-operation distribution is INCOMPLETE (49 of 50 expected ops); complete passes; smoke tolerates", () => {
    const spec: WorkloadSpec = {
      axis: "B",
      title: "edit",
      gated: {
        p95_latency_ratio: {
          threshold: 1.12,
          direction: "lower-is-better",
          statistic: "p95",
          source: { kind: "distribution", key: "editLatency" },
        },
      },
    };
    const lat = (len: number): number[] => Array.from({ length: len }, () => 100);
    const dist = (len: number): WorkloadSample =>
      sample({ expectedOps: 50, distributions: { editLatency: lat(len) } });
    // Candidate's 2nd sample returned only 49 latencies for 50 requested ops —
    // pooling them would let the percentile still compute and pass; here the
    // per-sample completeness check fails the full run.
    const incomplete: WorkloadEvaluationInput = {
      id: "edit",
      spec,
      available: true,
      candidate: side([dist(50), dist(49)]),
      baseline: side([dist(50), dist(50)]),
    };
    const f = evaluateGate(
      gateInput({ manifest: manifestOf({ edit: spec }), workloads: [incomplete] }),
    );
    expect(f.pass).toBe(false);
    expect(f.failures.join(" ")).toMatch(/missing|instrumentation/i);
    // Every distribution complete ⇒ passes.
    const complete: WorkloadEvaluationInput = {
      id: "edit",
      spec,
      available: true,
      candidate: side([dist(50), dist(50)]),
      baseline: side([dist(50), dist(50)]),
    };
    expect(
      evaluateGate(gateInput({ manifest: manifestOf({ edit: spec }), workloads: [complete] })).pass,
    ).toBe(true);
    // Smoke tolerates the partial.
    expect(
      evaluateGate(
        gateInput({ manifest: manifestOf({ edit: spec }), workloads: [incomplete], smoke: true }),
      ).pass,
    ).toBe(true);
  });

  // ── Carrier coverage: carrierCount === sfcCount per sample (axis A) ─────────
  it("FAILS a full run when carrierCount < sfcCount (subset compile) even at a ~1.0 candidate/baseline ratio; equal coverage passes; smoke tolerates", () => {
    const spec: WorkloadSpec = {
      axis: "A",
      title: "axisA",
      coverage: { actual: "carrierCount", expected: "sfcCount" },
      gated: {
        generated_carrier_count: {
          threshold: 1.01,
          direction: "invariant",
          statistic: "median",
          source: { kind: "scalar", key: "carrierCount" },
        },
      },
    };
    const cov = (carriers: number, sfcs: number): WorkloadSample =>
      sample({ metrics: { carrierCount: carriers, sfcCount: sfcs } });
    // BOTH sides compile a SUBSET (80 of 100): the candidate/baseline carrier-count
    // ratio is exactly 1.0 (both skip the same work, so the invariant metric is
    // green), but the within-sample coverage (80 != 100) FAILS a full run.
    const subset: WorkloadEvaluationInput = {
      id: "axisA",
      spec,
      available: true,
      candidate: side([cov(80, 100), cov(80, 100)]),
      baseline: side([cov(80, 100), cov(80, 100)]),
    };
    const f = evaluateGate(
      gateInput({ manifest: manifestOf({ axisA: spec }), workloads: [subset] }),
    );
    expect(f.pass).toBe(false);
    expect(f.failures.join(" ")).toMatch(/coverage|carrier|carrierCount/i);
    // Full coverage (carrierCount == sfcCount) passes.
    const fullCov: WorkloadEvaluationInput = {
      id: "axisA",
      spec,
      available: true,
      candidate: side([cov(100, 100), cov(100, 100)]),
      baseline: side([cov(100, 100), cov(100, 100)]),
    };
    expect(
      evaluateGate(gateInput({ manifest: manifestOf({ axisA: spec }), workloads: [fullCov] })).pass,
    ).toBe(true);
    // Smoke tolerates the subset.
    expect(
      evaluateGate(
        gateInput({ manifest: manifestOf({ axisA: spec }), workloads: [subset], smoke: true }),
      ).pass,
    ).toBe(true);
  });

  // ── Behavioral locality enforced on BOTH sides ─────────────────────────────
  it("enforces the behavioral locality bound on BOTH sides — a BASELINE whole-project burst FAILS even with a localized candidate", () => {
    const spec: WorkloadSpec = {
      axis: "B",
      title: "edit",
      gated: {},
      behavioral: { maxAffectedUriFraction: 0.25 },
    };
    // The candidate is LOCAL (0.02) but the BASELINE republishes ~the whole
    // project (0.9). Applying the threshold to the candidate worst sample ONLY would
    // let this pass; the locality invariant is absolute (not a candidate-vs-baseline
    // ratio), so a baseline violation must fail too.
    const wl: WorkloadEvaluationInput = {
      id: "edit",
      spec,
      available: true,
      candidate: side([sample({ behavioral: { affectedUris: 2, totalUris: 100 } })]),
      baseline: side([sample({ behavioral: { affectedUris: 90, totalUris: 100 } })]),
    };
    const f = evaluateGate(gateInput({ manifest: manifestOf({ edit: spec }), workloads: [wl] }));
    expect(f.pass).toBe(false);
    expect(f.failures.join(" ")).toMatch(/baseline/i);
    expect(f.failures.join(" ")).toMatch(/locality|publication|affect/i);
    // Both sides local ⇒ passes.
    const ok: WorkloadEvaluationInput = {
      id: "edit",
      spec,
      available: true,
      candidate: side([sample({ behavioral: { affectedUris: 2, totalUris: 100 } })]),
      baseline: side([sample({ behavioral: { affectedUris: 2, totalUris: 100 } })]),
    };
    expect(
      evaluateGate(gateInput({ manifest: manifestOf({ edit: spec }), workloads: [ok] })).pass,
    ).toBe(true);
  });

  // ── Behavioral payloads required on BOTH sides ─────────────────────────────
  it("requires behavioral payloads on BOTH candidate AND baseline samples — a missing BASELINE behavioral sample FAILS a full run", () => {
    const spec: WorkloadSpec = {
      axis: "B",
      title: "edit",
      gated: {},
      behavioral: { maxAffectedUriFraction: 0.5 },
    };
    const baseMissing: WorkloadEvaluationInput = {
      id: "edit",
      spec,
      available: true,
      candidate: side([
        sample({ behavioral: { affectedUris: 1, totalUris: 100 } }),
        sample({ behavioral: { affectedUris: 1, totalUris: 100 } }),
      ]),
      baseline: side([
        sample({ behavioral: { affectedUris: 1, totalUris: 100 } }),
        sample(), // MISSING baseline behavioral payload
      ]),
    };
    const full = evaluateGate(
      gateInput({ manifest: manifestOf({ edit: spec }), workloads: [baseMissing] }),
    );
    expect(full.pass).toBe(false);
    expect(full.failures.join(" ")).toMatch(/behavioral/i);
    expect(full.failures.join(" ")).toMatch(/baseline/i);
    // Smoke tolerates it.
    expect(
      evaluateGate(
        gateInput({ manifest: manifestOf({ edit: spec }), workloads: [baseMissing], smoke: true }),
      ).pass,
    ).toBe(true);
    // Both sides fully present ⇒ passes.
    const bothPresent: WorkloadEvaluationInput = {
      id: "edit",
      spec,
      available: true,
      candidate: side([sample({ behavioral: { affectedUris: 1, totalUris: 100 } })]),
      baseline: side([sample({ behavioral: { affectedUris: 1, totalUris: 100 } })]),
    };
    expect(
      evaluateGate(gateInput({ manifest: manifestOf({ edit: spec }), workloads: [bothPresent] }))
        .pass,
    ).toBe(true);
  });

  // ── Orchestration-level axis-A native wiring (through runInterleaved) ───────
  it("the gate ORCHESTRATION wires --baseline-native into the BASELINE side, and a synthetic candidate regression FAILS through buildSideContexts → runInterleaved → evaluateGate", async () => {
    // The existing axis-A spec drives axisACodegen.runOnce directly; this one drives
    // the REAL runGate orchestration primitives (buildSideContexts + runInterleaved
    // + evaluateGate) to prove the gate maps --baseline-native onto the BASELINE
    // context (never a candidate fallback) and that a regression caught only when
    // the two sides load DISTINCT natives flows through the actual gate path.
    const candidateNative = "/candidate/packages/native";
    const baselineNative = "/baseline/packages/native";
    const fakeCorpus = {
      dir: "/corpus",
      manifest: {} as EnsuredCorpus["manifest"],
      contentHash: "test",
      isGateCorpus: false,
      appTsconfig: "",
      kernelTsconfig: "",
      rootTsconfig: "",
    } as EnsuredCorpus;

    const fast: AxisAChildSample = {
      totalMs: 100,
      filesPerSec: 2000,
      outputBytes: 100_000,
      sourceMapBytes: 50_000,
      codeTransformOps: 10,
      nonCheckerMs: 20,
      codegenMs: 10,
      sourcemapMs: 5,
      parseTransformTransportMs: 5,
      carrierCount: 100,
      peakRssBytes: 100_000_000,
      sfcCount: 100,
      // A present carrier-content hash so this fixture (which exercises the per-side
      // native throughput/bytes/count wiring) keeps the content-equality rail at
      // PARITY; an absent hash correctly surfaces as an empty UNAVAILABLE set.
      carrierContentHash: `sha256:${"a".repeat(64)}`,
    };
    const regressed: AxisAChildSample = {
      ...fast,
      filesPerSec: 1200,
      nonCheckerMs: 60,
      sourcemapMs: 30,
      sourceMapBytes: 90_000,
      carrierCount: 80,
      peakRssBytes: 160_000_000,
    };
    const nativeOf = (argv: readonly string[]): string => {
      const i = argv.indexOf("--native");
      expect(i).toBeGreaterThanOrEqual(0);
      expect(argv).toContain("--corpus");
      return argv[i + 1];
    };

    const axisASpec = readBaselineManifest().workloads["axis-a-codegen"];
    const armed = (cand: WorkloadSample[], base: WorkloadSample[]) =>
      evaluateGate({
        manifest: manifestOf({ "axis-a-codegen": axisASpec }, { baselineRef: PINNED_BASELINE_SHA }),
        workloads: [
          {
            id: "axis-a-codegen",
            spec: axisASpec,
            available: true,
            candidate: { samples: cand },
            baseline: { samples: base },
          },
        ],
        smoke: false,
        selfCheck: false,
        engineResolved: "typescript@7.0.1-rc",
        baselineEngineResolved: "typescript@7.0.1-rc",
        meta: {
          corpusHash: `sha256:${"0".repeat(64)}`,
          candidateBin: "cand-bin",
          baselineBin: "base-bin",
          candidateNative,
          baselineNative,
          baselineRoot: "base-root",
          threads: 1,
          samplesPerSide: 7,
        },
      });

    // The native-AWARE seam returns metrics keyed on the --native root it sees,
    // recording every root so we can prove the baseline side ran the DISTINCT root.
    const seenRoots: string[] = [];
    const aware: AxisAChildRunner = (inv) => {
      const root = nativeOf(inv.argv);
      seenRoots.push(root);
      return root === candidateNative ? regressed : fast;
    };

    // (a) the REAL ctx construction maps --baseline-native onto the BASELINE side
    //     (no candidate fallback) — the exact wiring the orchestration gap missed.
    const { candidateCtx, baselineCtx } = buildSideContexts({
      corpus: fakeCorpus,
      candidateBin: "cand-bin",
      baselineBin: "base-bin",
      candidateNative,
      baselineNative,
      threads: 1,
      ops: 8,
      workRoot: "/tmp/verter-perf-spec",
      axisAChildRunner: aware,
    });
    expect(candidateCtx.nativeRoot).toBe(candidateNative);
    expect(baselineCtx.nativeRoot).toBe(baselineNative);

    // (b) drive the REAL interleaved orchestration; the baseline samples flow
    //     through the baseline ctx ⇒ the child saw BOTH distinct --native roots.
    const aw = await runInterleaved(axisACodegen, candidateCtx, baselineCtx, 7);
    expect(seenRoots).toContain(candidateNative);
    expect(seenRoots).toContain(baselineNative);
    expect(new Set(seenRoots).size).toBe(2);

    // (c) the synthetic candidate regression FAILS through the actual gate path.
    const failed = armed([...aw.candidate.samples], [...aw.baseline.samples]);
    expect(failed.pass).toBe(false);
    expect(failed.failures.join(" ")).toMatch(/axis-a-codegen\.compile_throughput_ratio/);

    // Discriminator: a native-BLIND seam (the "candidate native on both sides" bug
    // → identical metrics regardless of root) trips NO axis-A metric, proving the
    // failure above is caused by the genuine per-side native wiring.
    const blind: AxisAChildRunner = () => fast;
    const blindCtx = buildSideContexts({
      corpus: fakeCorpus,
      candidateBin: "cand-bin",
      baselineBin: "base-bin",
      candidateNative,
      baselineNative,
      threads: 1,
      ops: 8,
      workRoot: "/tmp/verter-perf-spec",
      axisAChildRunner: blind,
    });
    const bl = await runInterleaved(axisACodegen, blindCtx.candidateCtx, blindCtx.baselineCtx, 7);
    const blindReport = armed([...bl.candidate.samples], [...bl.baseline.samples]);
    expect(blindReport.failures.join(" ")).not.toMatch(/axis-a-codegen/);
    expect(blindReport.pass).toBe(true);
  });
});

describe("gate error artifact (always-emit on a pre-report failure)", () => {
  it("builds a serializable pass:false error report carrying the failure reason", () => {
    const r = buildGateErrorReport(
      new Error("Corpus hash sha256:abc does not match baseline manifest sha256:def"),
    );
    expect(r.pass).toBe(false);
    expect(r.error).toMatch(/corpus hash/i);
    expect(r.timestamp).toMatch(/^\d{4}-\d{2}-\d{2}T/);
    // The artifact is written verbatim — it must round-trip through JSON.
    const roundTripped = JSON.parse(JSON.stringify(r)) as { pass: boolean; error: string };
    expect(roundTripped.pass).toBe(false);
    expect(roundTripped.error).toMatch(/corpus hash/i);
  });

  it("coerces a non-Error throw into a reason string", () => {
    const r = buildGateErrorReport("boom");
    expect(r.pass).toBe(false);
    expect(r.error).toMatch(/boom/);
  });
});

describe("perf-gate workflow validates the samples floor before a full run", () => {
  const workflow = readFileSync(
    join(__dirname, "..", "..", "..", "..", ".github", "workflows", "perf-gate.yml"),
    "utf-8",
  );

  it("a guard step rejects a samples value below the blocking floor of 7 (fail-fast)", () => {
    // A manual workflow_dispatch can pass an arbitrary `samples`, including below the
    // blocking floor of 7. The nightly/full gate must reject that FAST with a clear
    // message — ahead of the expensive candidate+baseline builds — not deep inside
    // evaluateGate. A guard step enforces SAMPLES >= 7 before the gate runs.
    expect(workflow).toMatch(/Validate samples floor/);
    expect(workflow).toMatch(/-lt 7\b/);
    expect(workflow).toMatch(/blocking floor/i);
  });
});

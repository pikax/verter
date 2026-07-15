import { describe, it, expect, beforeAll, afterAll } from "vitest";
import { mkdtempSync, mkdirSync, copyFileSync, existsSync, rmSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import {
  runAxisA,
  nativeEntry,
  carrierContentDigest,
  type CarrierContentEntry,
  type AxisAChildSample,
  type VerterHostApi,
  type VerterHostFactory,
} from "./axis-a-child.js";
import {
  readBaselineManifest,
  evaluateGate,
  type MetricSource,
  type BaselineManifest,
  type WorkloadSpec,
  type GateEvaluationInput,
  type WorkloadEvaluationInput,
} from "./gate.js";
import { axisACodegen, type AxisAChildRunner, type WorkloadSample } from "./workloads.js";
import type { EnsuredCorpus } from "./corpus.js";

const __dirname = dirname(fileURLToPath(import.meta.url));
// The candidate's own @verter/native build (the workspace package root).
const NATIVE_ROOT = join(__dirname, "..", "..", "..", "..", "packages", "native");
const FIXTURES = join(__dirname, "..", "fixtures");
// Skip when the native build is absent (a fresh checkout / the per-PR unit-spec
// job, which does not build native) — the standard hermetic skip. When native IS
// present (local + the nightly/worktree runs) the carrier-output assertion is
// strict: it must produce real carriers, never a vacuous "carrierCount may be 0".
const nativeBuilt = existsSync(nativeEntry(NATIVE_ROOT));

// A small slice of REAL .vue fixtures (committed in the benchmark package).
const SLICE = ["tiny-template.vue", "simple-interactive.vue", "form-component.vue"];

let corpusDir: string;
beforeAll(() => {
  corpusDir = mkdtempSync(join(tmpdir(), "verter-axis-a-child-spec-"));
  mkdirSync(join(corpusDir, "app"), { recursive: true });
  for (const f of SLICE) copyFileSync(join(FIXTURES, f), join(corpusDir, "app", f));
});
afterAll(() => {
  if (corpusDir) rmSync(corpusDir, { recursive: true, force: true });
});

describe("axis-a-child native compile produces real carrier output", () => {
  it.skipIf(!nativeBuilt)(
    "the chosen IDE carrier target yields carrierCount > 0 with audited output_bytes > 0 over a real .vue slice",
    () => {
      const sample = runAxisA({ nativeRoot: NATIVE_ROOT, corpusDir, threads: 1 });
      // Every SFC in the slice was discovered…
      expect(sample.sfcCount).toBe(SLICE.length);
      // …and the chosen carrier target actually PRODUCED carrier output: each
      // audited compile emitted output_bytes > 0, so carrierCount tracks sfcCount
      // and the aggregate output is non-empty. This closes the silent zero-output
      // false-green (a target that "ran" but emitted nothing).
      expect(sample.carrierCount).toBeGreaterThan(0);
      expect(sample.carrierCount).toBe(sample.sfcCount);
      expect(sample.outputBytes ?? 0).toBeGreaterThan(0);
    },
  );
});

describe("axis-a child peak RSS is audit-ONLY — no Node-maxRSS parallel fallback", () => {
  it("the source uses NO process.resourceUsage()/maxRSS/osPeak under the audit RSS metric", () => {
    // The audit RSS is the SOLE source for peakRssBytes. A `null` audit RSS must
    // stay null (UNAVAILABLE, failed by the full gate) — never substituted by the
    // Node child's own OS maxRSS, which is a parallel measurement path under an
    // audit metric. This asserts the source does NOT read
    // `process.resourceUsage().maxRSS` as an `osPeak` fallback — that parallel path
    // must stay gone for good.
    const src = readFileSync(join(__dirname, "axis-a-child.ts"), "utf-8");
    expect(src).not.toMatch(/resourceUsage/);
    expect(src).not.toMatch(/maxRSS/);
    expect(src).not.toMatch(/osPeak/);
  });
});

describe("carrierContentDigest catches byte-preserving carrier/source-map content changes", () => {
  const entry = (canonicalId: string, code: string, sourceMap: string): CarrierContentEntry => ({
    canonicalId,
    code,
    sourceMap,
  });

  it("IDENTICAL byte counts but DIFFERENT carrier CODE content ⇒ DIFFERENT digest", () => {
    // The byte/count invariants (output_bytes, carrierCount) would PASS here — both
    // sides emit the same number of bytes — so only a content hash catches it.
    const a = [entry("a.vue", "export const x = 1;", "MAP-A")];
    const b = [entry("a.vue", "export const y = 2;", "MAP-B")];
    expect(a[0].code.length).toBe(b[0].code.length); // identical byte count
    expect(a[0].sourceMap.length).toBe(b[0].sourceMap.length);
    expect(carrierContentDigest(a)).not.toBe(carrierContentDigest(b));
  });

  it("a byte-preserving SOURCE-MAP-only content change still changes the digest", () => {
    const a = [entry("a.vue", "CODE", "}}}sm-content-one")];
    const b = [entry("a.vue", "CODE", "}}}sm-content-two")];
    expect(a[0].sourceMap.length).toBe(b[0].sourceMap.length);
    expect(carrierContentDigest(a)).not.toBe(carrierContentDigest(b));
  });

  it("identical content ⇒ identical digest, order-independent over canonicalId", () => {
    const set1 = [entry("a.vue", "A", "ma"), entry("b.vue", "B", "mb")];
    const set2 = [entry("b.vue", "B", "mb"), entry("a.vue", "A", "ma")]; // shuffled order
    expect(carrierContentDigest(set1)).toBe(carrierContentDigest(set2));
  });

  it("moving content BETWEEN code and source-map is not collision-masked (framing is unambiguous)", () => {
    const a = [entry("a.vue", "AB", "C")];
    const b = [entry("a.vue", "A", "BC")];
    expect(carrierContentDigest(a)).not.toBe(carrierContentDigest(b));
  });

  it("a digest is a stable sha256:<hex> string", () => {
    expect(carrierContentDigest([entry("a.vue", "A", "m")])).toMatch(/^sha256:[0-9a-f]{64}$/);
  });
});

// ── Axis-A gates ONLY empirically-present audit signals ──────────────────────
// The real axis-A child is the IN-PROCESS native compile: there is no spawned
// child to RSS-sample, and the compile audit record does not emit the
// parse/transform/transport phase timing. So `peakRssBytes`,
// `parseTransformTransportMs`, and therefore `nonCheckerMs` are ALWAYS null on a
// real axis-A run, while throughput, codegen + source-map phase timings, output
// bytes, source-map bytes, and carrier counts ARE present. These specs forbid
// gating an axis-A metric whose real producer returns null, and prove the gated
// set reads only present signals over the committed fixture slice.
describe("axis-A perf-gate metrics are gated ONLY on empirically-present audit signals", () => {
  it("defers the two UNMEASURABLE axis-A metrics (peak RSS, full non-checker aggregate) and gates present signals instead", () => {
    const m = readBaselineManifest();
    const axisA = m.workloads["axis-a-codegen"];
    expect(axisA).toBeDefined();
    const gatedKeys = Object.keys(axisA.gated);

    // The two metrics the real axis-A producer ALWAYS returns null for are NOT
    // gated on axis A (peak per-PID RSS is unsampled for the in-process compile;
    // the full non-checker aggregate needs the missing parse/transform/transport
    // phase timing).
    expect(gatedKeys).not.toContain("peak_rss_ratio");
    expect(gatedKeys).not.toContain("non_checker_time_ratio");

    // …replaced by present, discriminating signals (throughput, codegen(+source-
    // map) emit time, source-map bytes, carrier count, output bytes).
    expect(gatedKeys).toEqual(
      expect.arrayContaining([
        "compile_throughput_ratio",
        "codegen_time_ratio",
        "source_map_bytes",
        "generated_carrier_count",
        "output_bytes_ratio",
      ]),
    );
    // The honest codegen-time metric reads the PRESENT codegen + source-map emit
    // phases — not the deferred full non-checker aggregate.
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

    // Both deferrals are recorded with a NAMED Rust follow-up — an honest
    // deferral, not a silent deletion.
    const deferred = m.deferred ?? [];
    const peak = deferred.find((d) => /axis-a/i.test(d.metric) && /peak rss/i.test(d.metric));
    const nonChecker = deferred.find(
      (d) => /axis-a/i.test(d.metric) && /non-checker/i.test(d.metric),
    );
    expect(peak, "axis-A peak RSS deferral with a named Rust follow-up").toBeDefined();
    expect(peak!.requiresRust).toMatch(/rss sampler|footprint|process_rss_peak/i);
    expect(
      nonChecker,
      "axis-A non-checker aggregate deferral with a named Rust follow-up",
    ).toBeDefined();
    expect(nonChecker!.requiresRust).toMatch(/parse.*transform.*transport|phase timing/i);

    // Axis-B cold/warm peak RSS is ALSO deferred: verter-tsc spawns tsgo as a
    // SEPARATE child process, so the harness's wrapper-PID VmHWM sample misses the
    // tsgo engine's resident set, and full process-tree peak RSS is not cleanly
    // portable in TS. Neither cold nor warm gates peak_rss_ratio any more.
    expect(Object.keys(m.workloads["cold-typecheck"].gated)).not.toContain("peak_rss_ratio");
    expect(Object.keys(m.workloads["warm-incremental-retypecheck"].gated)).not.toContain(
      "peak_rss_ratio",
    );
    const axisBPeak = deferred.find((d) => /axis-b/i.test(d.metric) && /peak rss/i.test(d.metric));
    expect(axisBPeak, "axis-B peak RSS deferral with a named Rust follow-up").toBeDefined();
    expect(axisBPeak!.requiresRust).toMatch(/per-pid|per-engine|process-tree|tsgo/i);
  });

  it.skipIf(!nativeBuilt)(
    "every GATED axis-A metric source is non-null (>0) on a REAL runAxisA sample over the committed fixture slice",
    async () => {
      const axisA = readBaselineManifest().workloads["axis-a-codegen"];
      // Build the REAL gate-facing WorkloadSample: run the REAL native compile
      // (runAxisA) IN-PROCESS through the production axisACodegen.runOnce mapping
      // (the injected runner is the per-side child seam), so the sample carries
      // exactly the attribution the gate reads.
      const realRunner: AxisAChildRunner = () =>
        runAxisA({ nativeRoot: NATIVE_ROOT, corpusDir, threads: 1 });
      const ws: WorkloadSample = await axisACodegen.runOnce({
        corpus: { dir: corpusDir } as EnsuredCorpus,
        nativeRoot: NATIVE_ROOT,
        threads: 1,
        axisAChildRunner: realRunner,
      });

      // Resolve a gated metric's source against the real sample exactly as the
      // gate's presence rail does (scalar/attribution/rss/total_wall/distribution).
      const sourceValue = (src: MetricSource): number | null | undefined => {
        switch (src.kind) {
          case "scalar":
            return ws.metrics[src.key];
          case "attribution":
            return ws.attribution
              ? (ws.attribution as Record<string, number | null>)[src.key]
              : null;
          case "rss":
            return ws.rssBytes;
          case "total_wall":
            return ws.totalMs;
          case "distribution": {
            const d = ws.distributions?.[src.key];
            return d?.length ?? null;
          }
        }
      };

      // EVERY gated axis-A metric source must be a finite number > 0 — exactly the
      // gate's presence condition for scalar/attribution magnitudes. A metric
      // whose real source is null (peak_rss_ratio / non_checker_time_ratio) would
      // FAIL here, structurally forbidding gating it on axis A.
      for (const [name, spec] of Object.entries(axisA.gated)) {
        const v = sourceValue(spec.source);
        expect(typeof v === "number" && Number.isFinite(v) && v > 0, `${name} source present`).toBe(
          true,
        );
      }

      // Spot-check the named present sources the brief enumerates.
      expect(ws.metrics.filesPerSec).toBeGreaterThan(0);
      expect(ws.totalMs).toBeGreaterThan(0);
      expect(ws.metrics.carrierCount).toBeGreaterThan(0);
      expect(ws.metrics.carrierCount).toBe(ws.metrics.sfcCount);
      expect(ws.attribution?.outputBytes ?? 0).toBeGreaterThan(0);
      expect(ws.attribution?.sourceMapBytes ?? 0).toBeGreaterThan(0);
    },
  );
});

// ── A MISSING IDE carrier is UNAVAILABLE, never a coerced empty-string hash ──────
// The content-correctness pass reads each compiled carrier's IDE code + source-map.
// A missing carrier (the host returns null, or an absent/empty code or source-map)
// MUST surface carrierContentHash as null — NEVER hashed from "", which would let
// two both-sides-missing runs compare as an EQUAL empty hash and slip past the
// content-equality rail. These specs inject a scripted host via the VerterHostFactory
// seam, so they need no native build (and run on the unit-spec job).
describe("axis-A carrier-content rail fails closed on a MISSING IDE carrier", () => {
  class FakeHost implements VerterHostApi {
    constructor(
      private readonly ideFor: (id: string) => { code: string; sourceMap?: string } | null,
    ) {}
    upsert(): void {
      /* no-op: the carrier-content rail reads getIde, not upsert */
    }
    compileWithAudit(): Buffer | null {
      return null; // no audited attribution; the carrier-content rail reads getIde
    }
    getIde(id: string): { code: string; sourceMap?: string } | null {
      return this.ideFor(id.replace(/\\/g, "/"));
    }
  }
  const factory =
    (ideFor: (id: string) => { code: string; sourceMap?: string } | null): VerterHostFactory =>
    () =>
      new FakeHost(ideFor);
  const args = (): { nativeRoot: string; corpusDir: string; threads: number } => ({
    nativeRoot: NATIVE_ROOT,
    corpusDir,
    threads: 1,
  });
  const isFile =
    (suffix: string) =>
    (id: string): boolean =>
      id.endsWith(suffix);
  const present = (id: string): { code: string; sourceMap: string } => ({
    code: `code:${id}`,
    sourceMap: `map:${id}`,
  });

  function side(samples: WorkloadSample[]): { samples: WorkloadSample[] } {
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

  it("a NULL getIde for one expected carrier ⇒ carrierContentHash is null (never a coerced empty hash)", () => {
    const missingOne = isFile("tiny-template.vue");
    const s = runAxisA(
      args(),
      factory((id) => (missingOne(id) ? null : present(id))),
    );
    expect(s.carrierContentHash).toBeNull();
  });

  it("an empty/absent code OR source-map for one carrier ⇒ carrierContentHash is null", () => {
    const target = isFile("simple-interactive.vue");
    expect(
      runAxisA(
        args(),
        factory((id) => (target(id) ? { code: "", sourceMap: "m" } : present(id))),
      ).carrierContentHash,
    ).toBeNull();
    expect(
      runAxisA(
        args(),
        factory((id) => (target(id) ? { code: "c", sourceMap: "" } : present(id))),
      ).carrierContentHash,
    ).toBeNull();
    expect(
      runAxisA(
        args(),
        factory((id) => (target(id) ? { code: "c" } : present(id))),
      ).carrierContentHash,
    ).toBeNull();
  });

  it("ALL carriers present (non-empty code + source-map) ⇒ a real sha256 content hash (control)", () => {
    const s = runAxisA(args(), factory(present));
    expect(s.carrierContentHash).toMatch(/^sha256:[0-9a-f]{64}$/);
  });

  it("axisACodegen.runOnce maps a missing (null) hash to an EMPTY content set; a present hash to [hash]", async () => {
    const missing = runAxisA(
      args(),
      factory((id) => (isFile("form-component.vue")(id) ? null : present(id))),
    );
    const wsMissing = await axisACodegen.runOnce({
      corpus: { dir: corpusDir } as EnsuredCorpus,
      nativeRoot: NATIVE_ROOT,
      threads: 1,
      axisAChildRunner: () => missing,
    });
    expect(wsMissing.contentSets?.carrierContent).toEqual([]);

    const ok = runAxisA(args(), factory(present));
    const wsPresent = await axisACodegen.runOnce({
      corpus: { dir: corpusDir } as EnsuredCorpus,
      nativeRoot: NATIVE_ROOT,
      threads: 1,
      axisAChildRunner: () => ok,
    });
    expect(wsPresent.contentSets?.carrierContent).toEqual([ok.carrierContentHash]);
  });

  it("a both-sides-MISSING carrier HARD-FAILS a full run (never an equal empty hash); both-present passes", async () => {
    const axisA = readBaselineManifest().workloads["axis-a-codegen"];
    // A full child sample with PRESENT byte/count signals so ONLY the carrier-content
    // rail can act — its carrierContentHash comes from the REAL producer (runAxisA
    // over a scripted host), so a coerced empty-string hash (equal on both sides)
    // would falsely PASS; the null (⇒ empty set) must hard-fail instead.
    const fullChild = (carrierContentHash: string | null): AxisAChildSample => ({
      totalMs: 10,
      filesPerSec: 2000,
      outputBytes: 100_000,
      sourceMapBytes: 50_000,
      codeTransformOps: 10,
      nonCheckerMs: 20,
      codegenMs: 10,
      sourcemapMs: 5,
      parseTransformTransportMs: 5,
      carrierCount: 100,
      peakRssBytes: 1,
      sfcCount: 100,
      carrierContentHash,
    });
    const wsFor = (child: AxisAChildSample): Promise<WorkloadSample> =>
      axisACodegen.runOnce({
        corpus: { dir: corpusDir } as EnsuredCorpus,
        nativeRoot: NATIVE_ROOT,
        threads: 1,
        axisAChildRunner: () => child,
      });
    const missingHash = runAxisA(
      args(),
      factory((id) => (isFile("tiny-template.vue")(id) ? null : present(id))),
    ).carrierContentHash;
    const presentHash = runAxisA(args(), factory(present)).carrierContentHash;

    const wsMissing = await wsFor(fullChild(missingHash));
    const missingWl: WorkloadEvaluationInput = {
      id: "axis-a-codegen",
      spec: axisA,
      available: true,
      candidate: side([wsMissing, wsMissing]),
      baseline: side([wsMissing, wsMissing]),
    };
    const failed = evaluateGate(
      gateInput({ manifest: manifestOf({ "axis-a-codegen": axisA }), workloads: [missingWl] }),
    );
    expect(failed.pass).toBe(false);
    expect(failed.failures.join(" ")).toMatch(/carrierContent/i);

    const wsPresent = await wsFor(fullChild(presentHash));
    const presentWl: WorkloadEvaluationInput = {
      id: "axis-a-codegen",
      spec: axisA,
      available: true,
      candidate: side([wsPresent, wsPresent]),
      baseline: side([wsPresent, wsPresent]),
    };
    expect(
      evaluateGate(
        gateInput({ manifest: manifestOf({ "axis-a-codegen": axisA }), workloads: [presentWl] }),
      ).pass,
    ).toBe(true);
  });
});

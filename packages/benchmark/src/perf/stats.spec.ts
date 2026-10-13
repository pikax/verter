import { describe, it, expect } from "vitest";
import {
  percentile,
  summarize,
  median,
  ratioDecision,
  throughputRatioDecision,
  invariantRatioDecision,
} from "./stats.js";

describe("percentile", () => {
  it("interpolates linearly (R-7)", () => {
    const s = [1, 2, 3, 4, 5];
    expect(percentile(s, 0)).toBe(1);
    expect(percentile(s, 1)).toBe(5);
    expect(percentile(s, 0.5)).toBe(3);
    expect(percentile(s, 0.25)).toBe(2);
  });

  it("handles a singleton and rejects empty", () => {
    expect(percentile([42], 0.95)).toBe(42);
    expect(() => percentile([], 0.5)).toThrow();
  });
});

describe("summarize", () => {
  it("reports the percentile summary without mutating the input", () => {
    const input = [5, 1, 3, 2, 4];
    const snapshot = [...input];
    const s = summarize(input);
    expect(s.n).toBe(5);
    expect(s.min).toBe(1);
    expect(s.max).toBe(5);
    expect(s.mean).toBe(3);
    expect(s.p50).toBe(3);
    // p95/p99 of 1..5 interpolate near the top.
    expect(s.p95).toBeCloseTo(4.8, 5);
    expect(s.p99).toBeCloseTo(4.96, 5);
    // Input untouched (discriminating: a sort-in-place bug would fail here).
    expect(input).toEqual(snapshot);
  });

  it("median alias agrees with percentile 0.5", () => {
    const s = [10, 30, 20];
    expect(median(s)).toBe(
      percentile(
        [...s].sort((a, b) => a - b),
        0.5,
      ),
    );
  });
});

describe("ratioDecision — the gate predicate", () => {
  // A small deterministic jitter generator so the tests are reproducible
  // without depending on Math.random.
  function jitter(base: number, spreadPct: number, n: number, seed: number): number[] {
    let a = seed >>> 0;
    const out: number[] = [];
    for (let i = 0; i < n; i++) {
      a = (a + 0x6d_2b_79_f5) | 0;
      let t = Math.imul(a ^ (a >>> 15), 1 | a);
      t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
      const u = ((t ^ (t >>> 14)) >>> 0) / 4_294_967_296; // [0,1)
      out.push(base * (1 + (u - 0.5) * 2 * spreadPct));
    }
    return out;
  }

  it("yields ~1.0 and does NOT fail when candidate === baseline (no false fail)", () => {
    // The critical self-referential property: the SAME distribution against
    // itself must not trip the gate.
    const a = jitter(100, 0.05, 9, 1);
    const b = jitter(100, 0.05, 9, 1); // identical seed ⇒ identical samples
    const d = ratioDecision(a, b, 1.1, { resamples: 2000 });
    expect(d.statisticRatio).toBeCloseTo(1.0, 6);
    expect(d.fail).toBe(false);
  });

  it("does NOT fail on two independent draws of the same underlying distribution", () => {
    // Noise immunity: different samples, same mean, modest spread — the point
    // ratio is near 1 and the CI straddles 1, so no regression is reported.
    const a = jitter(100, 0.08, 9, 11);
    const b = jitter(100, 0.08, 9, 999);
    const d = ratioDecision(a, b, 1.1, { resamples: 4000 });
    expect(d.statisticRatio).toBeLessThan(1.1);
    expect(d.fail).toBe(false);
  });

  it("FAILS on a genuine, decisive regression (discriminating)", () => {
    // Candidate is 30% slower with tight spread — both the point ratio AND
    // the lower CI bound exceed the 1.10 threshold ⇒ regression.
    const baseline = jitter(100, 0.02, 9, 7);
    const candidate = jitter(130, 0.02, 9, 7);
    const d = ratioDecision(candidate, baseline, 1.1, { resamples: 4000 });
    expect(d.statisticRatio).toBeGreaterThan(1.2);
    expect(d.lowerBound95).toBeGreaterThan(1.1);
    expect(d.fail).toBe(true);
  });

  it("does NOT fail a borderline regression whose CI lower bound stays under threshold", () => {
    // A small mean shift (+5%) with wide spread: the point ratio may peek over
    // 1.10 occasionally but the lower CI bound stays below it, so the
    // conservative predicate withholds a fail. This is the conservative-CI
    // contract — a borderline, high-variance result is NOT a regression.
    const baseline = jitter(100, 0.25, 9, 3);
    const candidate = jitter(105, 0.25, 9, 91);
    const d = ratioDecision(candidate, baseline, 1.1, { resamples: 4000 });
    expect(d.lowerBound95).toBeLessThan(1.1);
    expect(d.fail).toBe(false);
  });

  it("THROWS on an unknown statistic instead of silently falling back to the median", () => {
    // A manifest typo in `statistic` must NOT coerce a tail-latency gate to the
    // median (which would disable it). `stat` is exhaustive, so an unknown statistic
    // reaching the decision is a hard throw, never a silent return of the median.
    expect(() =>
      ratioDecision([1, 2, 3], [1, 2, 3], 1.1, { statistic: "p42" as unknown as "p95" }),
    ).toThrow(/statistic/i);
  });

  it("throughputRatioDecision flips direction: a throughput DROP is the regression", () => {
    // Higher is better. Candidate throughput dropped 30% ⇒ ratio > 1 ⇒ fail.
    const baselineThroughput = jitter(1000, 0.02, 9, 5);
    const candidateThroughput = jitter(700, 0.02, 9, 5);
    const drop = throughputRatioDecision(candidateThroughput, baselineThroughput, 1.1, {
      resamples: 4000,
    });
    expect(drop.statisticRatio).toBeGreaterThan(1.2);
    expect(drop.fail).toBe(true);

    // A throughput GAIN (candidate faster) must NOT fail.
    const candidateFaster = jitter(1300, 0.02, 9, 5);
    const gain = throughputRatioDecision(candidateFaster, baselineThroughput, 1.1, {
      resamples: 4000,
    });
    expect(gain.statisticRatio).toBeLessThan(1.0);
    expect(gain.fail).toBe(false);
  });

  it("rejects empty inputs and zero baselines", () => {
    expect(() => ratioDecision([], [1], 1.1)).toThrow();
    expect(() => ratioDecision([1], [], 1.1)).toThrow();
    expect(() => ratioDecision([1], [0], 1.1)).toThrow();
  });
});

describe("ratioDecision honors percentile statistics (real tail latency)", () => {
  it("a p99 statistic reflects a tail spike the median ignores", () => {
    // Baseline: a tight distribution. Candidate: identical body, a heavy tail
    // (~4-5× the body). The MEDIAN ratio is ~1.0 (the body is unchanged); the
    // P99 ratio is large (the tail regressed). A gate that collapses p99 to the
    // median would miss this entirely.
    const baseline = Array.from({ length: 100 }, () => 100);
    const candidate = Array.from({ length: 88 }, () => 100).concat([
      300, 320, 340, 360, 380, 400, 420, 440, 460, 480, 500, 520,
    ]);

    const med = ratioDecision(candidate, baseline, 1.05, { statistic: "median", resamples: 4000 });
    const p99 = ratioDecision(candidate, baseline, 1.05, { statistic: "p99", resamples: 4000 });

    expect(med.statisticRatio).toBeCloseTo(1.0, 1);
    expect(p99.statisticRatio).toBeGreaterThan(2.0);
    // Discriminating: a gate that collapsed p99 to the median would read both as
    // ~1.0. The tail ratio must be decisively larger than the body ratio.
    expect(p99.statisticRatio).toBeGreaterThan(med.statisticRatio + 1.0);
    // The tail regression FAILS the gate; the median does not.
    expect(p99.fail).toBe(true);
    expect(med.fail).toBe(false);
  });

  it("p95 exceeds p50 on a right-skewed distribution", () => {
    const baseline = Array.from({ length: 100 }, () => 100);
    const candidate = Array.from({ length: 100 }, (_, i) => (i < 90 ? 100 : 220));
    const p50 = ratioDecision(candidate, baseline, 1.0, { statistic: "p50", resamples: 1500 });
    const p95 = ratioDecision(candidate, baseline, 1.0, { statistic: "p95", resamples: 1500 });
    expect(p50.statisticRatio).toBeCloseTo(1.0, 1);
    expect(p95.statisticRatio).toBeGreaterThan(p50.statisticRatio + 0.5);
  });

  it("an identical tail distribution stays at ratio ~1.0 with no false fail", () => {
    const a = Array.from({ length: 90 }, () => 100).concat([
      300, 310, 320, 330, 340, 350, 360, 370, 380, 390,
    ]);
    const b = [...a];
    const p99 = ratioDecision(a, b, 1.1, { statistic: "p99", resamples: 1500 });
    expect(p99.statisticRatio).toBeCloseTo(1.0, 6);
    expect(p99.fail).toBe(false);
  });
});

describe("ratioDecision predicate honesty + robustness", () => {
  it("exposes a `statisticRatio` (ratio-of-statistics), NOT a paired `medianRatio`", () => {
    // The implementation + manifest compare statistic(cand)/statistic(base); the
    // field name must say so (no "median(cand/base)" / "paired" claim).
    const d = ratioDecision([1, 2, 3], [1, 2, 3], 1.1, { resamples: 500 });
    expect(Object.prototype.hasOwnProperty.call(d, "statisticRatio")).toBe(true);
    expect(Object.prototype.hasOwnProperty.call(d, "medianRatio")).toBe(false);
  });

  it("REJECTS non-finite samples (NaN/Infinity is a broken measurement, not a pass)", () => {
    expect(() => ratioDecision([Number.NaN], [1], 1.1)).toThrow(/finite/i);
    expect(() => ratioDecision([1], [Number.POSITIVE_INFINITY], 1.1)).toThrow(/finite/i);
    expect(() => ratioDecision([1, 2, Number.NaN], [1, 2, 3], 1.1)).toThrow(/finite/i);
  });
});

describe("invariantRatioDecision — two-sided equality (a DROP is a regression)", () => {
  it("FAILS a DROP, PASSES equality, FAILS a bloat (carrier-count semantics)", () => {
    const base = Array.from({ length: 8 }, () => 100);
    // A drop: candidate generated fewer (skipped work) — must FAIL.
    const drop = invariantRatioDecision(
      Array.from({ length: 8 }, () => 80),
      base,
      1.01,
      { resamples: 1000 },
    );
    expect(drop.statisticRatio).toBeLessThan(1);
    expect(drop.fail).toBe(true);
    // Equal counts — must PASS.
    const equal = invariantRatioDecision([...base], base, 1.01, { resamples: 1000 });
    expect(equal.statisticRatio).toBeCloseTo(1, 6);
    expect(equal.fail).toBe(false);
    // A bloat — must FAIL.
    const bloat = invariantRatioDecision(
      Array.from({ length: 8 }, () => 130),
      base,
      1.01,
      { resamples: 1000 },
    );
    expect(bloat.statisticRatio).toBeGreaterThan(1);
    expect(bloat.fail).toBe(true);
  });

  it("does NOT fail a one-sided 'lower-is-better' decision on a DROP (the bug it fixes)", () => {
    // The contrast: a plain lower-is-better ratio treats a drop as a win.
    const base = Array.from({ length: 8 }, () => 100);
    const cand = Array.from({ length: 8 }, () => 80);
    expect(ratioDecision(cand, base, 1.01, { resamples: 1000 }).fail).toBe(false); // a "win"
    expect(invariantRatioDecision(cand, base, 1.01, { resamples: 1000 }).fail).toBe(true); // a regression
  });
});

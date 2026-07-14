/**
 * Statistics for the external-TS-engine perf gate.
 *
 * Two consumers:
 *  - the workload runner (median / p95 / p99 / mean over per-run samples);
 *  - the self-referential regression gate (a ratio between a candidate sample
 *    set and a baseline sample set, with a one-sided bootstrap 95% confidence
 *    bound so a noisy CI host cannot trip the gate on variance alone).
 *
 * Everything here is pure and deterministic given its inputs (the bootstrap
 * draws from a seeded PRNG), so a gate decision is reproducible from the
 * recorded samples.
 */

/** Percentile summary of a sample set. */
export interface SampleSummary {
  readonly n: number;
  readonly min: number;
  readonly max: number;
  readonly mean: number;
  readonly p50: number;
  readonly p95: number;
  readonly p99: number;
  readonly stdDev: number;
}

/** Linear-interpolation percentile (the "R-7" / Excel `PERCENTILE.INC` method). */
export function percentile(sortedAsc: readonly number[], q: number): number {
  if (sortedAsc.length === 0) throw new Error("percentile of an empty sample set");
  if (sortedAsc.length === 1) return sortedAsc[0];
  const clamped = Math.max(0, Math.min(1, q));
  const idx = clamped * (sortedAsc.length - 1);
  const lo = Math.floor(idx);
  const hi = Math.ceil(idx);
  if (lo === hi) return sortedAsc[lo];
  const frac = idx - lo;
  return sortedAsc[lo] + (sortedAsc[hi] - sortedAsc[lo]) * frac;
}

/** Summarize a sample set. Does not mutate the input. */
export function summarize(samples: readonly number[]): SampleSummary {
  if (samples.length === 0) throw new Error("cannot summarize an empty sample set");
  const sorted = [...samples].sort((a, b) => a - b);
  const n = sorted.length;
  const mean = sorted.reduce((s, v) => s + v, 0) / n;
  const variance = sorted.reduce((s, v) => s + (v - mean) * (v - mean), 0) / n;
  return {
    n,
    min: sorted[0],
    max: sorted[n - 1],
    mean,
    p50: percentile(sorted, 0.5),
    p95: percentile(sorted, 0.95),
    p99: percentile(sorted, 0.99),
    stdDev: Math.sqrt(variance),
  };
}

/** Median of a sample set (a thin alias over `percentile(.., 0.5)`). */
export function median(samples: readonly number[]): number {
  if (samples.length === 0) throw new Error("median of an empty sample set");
  const sorted = [...samples].sort((a, b) => a - b);
  return percentile(sorted, 0.5);
}

// ── Deterministic PRNG for the bootstrap (mulberry32) ──────────────────────
function mulberry32(seed: number): () => number {
  let a = seed >>> 0;
  return function next(): number {
    a |= 0;
    a = (a + 0x6d_2b_79_f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4_294_967_296;
  };
}

/** The statistic a ratio is taken of (the per-side aggregate). */
export type RatioStatistic = "median" | "mean" | "p50" | "p95" | "p99";

/** The ratio decision for one gated metric. */
export interface RatioDecision {
  /**
   * `statistic(candidate) / statistic(baseline)` — the ratio of the chosen
   * per-side statistic (median for time/throughput/RSS, the named percentile
   * for tail latency). NOT a paired per-sample median-of-ratios.
   */
  readonly statisticRatio: number;
  /**
   * The lower bound of the bootstrap 95% CI on the ratio (the 2.5th percentile
   * of the resampled ratio distribution). The gate fails only when BOTH the
   * point ratio AND this lower bound exceed the threshold — so pure CI-host
   * variance (which widens the CI downward) cannot trip the gate.
   */
  readonly lowerBound95: number;
  /** The upper bound (97.5th percentile of resampled ratios; reported, not gated). */
  readonly upperBound95: number;
  /** The threshold the ratio is compared against. */
  readonly threshold: number;
  /** `true` ⇒ regression (both gates exceeded). */
  readonly fail: boolean;
}

export interface RatioOptions {
  /** Bootstrap resample count. Default 10,000. */
  readonly resamples?: number;
  /** PRNG seed (keeps the decision reproducible). Default 0x6007_57A7. */
  readonly seed?: number;
  /**
   * Which statistic to take the ratio of. `"median"`/`"p50"` (default) is
   * robust; `"mean"` suits throughput-style aggregates; `"p95"`/`"p99"` gate
   * the TAIL of a per-operation latency distribution — the bootstrap resamples
   * the full distribution and recomputes the percentile each draw, so the tail
   * is genuinely gated (never collapsed to the median).
   */
  readonly statistic?: RatioStatistic;
}

function stat(sortedAsc: readonly number[], which: RatioStatistic): number {
  // EXHAUSTIVE over RatioStatistic — an unknown value is a hard throw, never a
  // silent median fallback (a manifest typo must not coerce a tail-latency gate to
  // the median and disable it).
  switch (which) {
    case "mean":
      return sortedAsc.reduce((s, v) => s + v, 0) / sortedAsc.length;
    case "p95":
      return percentile(sortedAsc, 0.95);
    case "p99":
      return percentile(sortedAsc, 0.99);
    case "median":
    case "p50":
      return percentile(sortedAsc, 0.5);
    default: {
      const exhaustive: never = which;
      throw new Error(`unknown ratio statistic ${JSON.stringify(exhaustive)}`);
    }
  }
}

/**
 * Decide whether `candidate` regressed against `baseline` for one metric.
 *
 * The predicate (conservative — thresholded + bootstrap-CI-gated, so pure variance
 * does not trip it; NOT absolute noise immunity — the pooled bootstrap ignores
 * per-run clustering, biasing toward a false RED, never a false green): FAIL only if
 *   `statisticRatio > threshold` AND `lower_bound_95pct(statisticRatio) > threshold`,
 * where `statisticRatio = statistic(candidate) / statistic(baseline)` — a
 * ratio-of-statistics, NOT a paired per-sample median-of-ratios.
 *
 * The CI is a one-sided percentile bootstrap on the ratio-of-statistics: for
 * each of `resamples` iterations, draw a with-replacement resample of each
 * side, take the ratio of their statistics, and read the 2.5th percentile of
 * the resampled ratios as the lower bound (and the 97.5th as the upper).
 *
 * Lower values are "better" for latency/time/RSS metrics — the caller passes
 * those directly. For a "higher is better" metric (e.g. files/sec throughput)
 * the caller inverts the inputs (baseline/candidate) so a DROP shows as a
 * ratio > 1; see `throughputRatioDecision`.
 */
export function ratioDecision(
  candidate: readonly number[],
  baseline: readonly number[],
  threshold: number,
  options: RatioOptions = {},
): RatioDecision {
  if (candidate.length === 0 || baseline.length === 0) {
    throw new Error("ratioDecision requires non-empty candidate and baseline samples");
  }
  // A NaN/Infinity sample is a broken/missing measurement, never a real datum:
  // `NaN > threshold` is false, so a silent NaN would otherwise read as a pass.
  // Reject it here so the gate cannot decide a regression off non-finite data.
  if (!candidate.every((v) => Number.isFinite(v)) || !baseline.every((v) => Number.isFinite(v))) {
    throw new Error("ratioDecision requires finite samples (NaN/Infinity is a broken measurement)");
  }
  const which = options.statistic ?? "median";
  const candSorted = [...candidate].sort((a, b) => a - b);
  const baseSorted = [...baseline].sort((a, b) => a - b);
  const baseStat = stat(baseSorted, which);
  if (baseStat === 0) throw new Error("baseline statistic is zero — cannot form a ratio");
  const statisticRatio = stat(candSorted, which) / baseStat;

  const resamples = options.resamples ?? 10_000;
  const rng = mulberry32(options.seed ?? 0x60_07_57_a7);
  const resampleStat = (src: readonly number[]): number => {
    const drawn: number[] = new Array(src.length);
    for (let i = 0; i < src.length; i++) drawn[i] = src[Math.floor(rng() * src.length)];
    drawn.sort((a, b) => a - b);
    return stat(drawn, which);
  };
  const ratios: number[] = new Array(resamples);
  for (let r = 0; r < resamples; r++) {
    const b = resampleStat(baseSorted);
    ratios[r] = b === 0 ? Number.POSITIVE_INFINITY : resampleStat(candSorted) / b;
  }
  ratios.sort((a, b) => a - b);
  const lowerBound95 = percentile(ratios, 0.025);
  const upperBound95 = percentile(ratios, 0.975);

  return {
    statisticRatio,
    lowerBound95,
    upperBound95,
    threshold,
    fail: statisticRatio > threshold && lowerBound95 > threshold,
  };
}

/**
 * A "higher is better" metric (throughput, cache-reuse rate): a regression is
 * a DROP. Inverts the inputs so the same `ratio > threshold ⇒ fail` predicate
 * applies — the returned ratio is `baselineStat / candidateStat`, so a
 * candidate that is SLOWER (lower throughput) yields a ratio > 1.
 */
export function throughputRatioDecision(
  candidate: readonly number[],
  baseline: readonly number[],
  threshold: number,
  options: RatioOptions = {},
): RatioDecision {
  // Swap candidate/baseline: the decision math is identical, only the
  // direction of "worse" flips.
  return ratioDecision(baseline, candidate, threshold, options);
}

/**
 * A TWO-SIDED INVARIANT metric (e.g. the generated-carrier count): the candidate
 * must stay within `threshold` of the baseline in EITHER direction. A DROP is a
 * regression (skipped work / missing carriers), NOT a perf win; a BLOAT is a
 * regression too. Fails when the point ratio AND the matching CI bound clear the
 * tolerance: a bloat (`ratio > threshold` ∧ `lb95 > threshold`) OR a drop
 * (`ratio < 1/threshold` ∧ `ub95 < 1/threshold`). The CI bounds keep pure noise
 * from tripping it, exactly like the one-sided predicate.
 */
export function invariantRatioDecision(
  candidate: readonly number[],
  baseline: readonly number[],
  threshold: number,
  options: RatioOptions = {},
): RatioDecision {
  const d = ratioDecision(candidate, baseline, threshold, options);
  const lo = 1 / threshold;
  const bloat = d.statisticRatio > threshold && d.lowerBound95 > threshold;
  const drop = d.statisticRatio < lo && d.upperBound95 < lo;
  return { ...d, fail: bloat || drop };
}

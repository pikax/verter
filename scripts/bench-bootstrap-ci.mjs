#!/usr/bin/env node
/**
 * Paired bench bootstrap CI helper for the R22-final S5 gate.
 *
 * Reads N paired R21.5 / HEAD bench JSON results
 * (`meta-ui-verter-repo_first_pass.json` written by
 * `packages/benchmark/src/meta-ui-bench.ts`) and computes:
 *   - per-pair aggregate ratio (HEAD / R215)
 *   - per-component ratio per pair
 *   - bootstrap 95% CI on the median ratio (10 000 resamples)
 *
 * Gate (per Phase 4 amendment 5):
 *   - aggregate CI upper bound ≤ 1.05 (R21.5 + 5%) → PASS
 *   - per-component CI upper bound ≤ 1.10 (R21.5 + 10%) → PASS
 *
 * Usage:
 *   node scripts/bench-bootstrap-ci.mjs <runs-dir>
 *     <runs-dir>/r215-run{1..N}/meta-ui-verter-repo_first_pass.json
 *     <runs-dir>/head-run{1..N}/meta-ui-verter-repo_first_pass.json
 *
 * Output JSON: <runs-dir>/bootstrap-ci.json
 */

import { existsSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const runsDir = process.argv[2] || "D:/tmp/r22-final-s5/runs";
const BENCH_FILE = "meta-ui-verter-repo_first_pass.json";
const RESAMPLES = 10000;
const AGGREGATE_GATE = 1.05; // +5 % over R21.5
const PER_COMPONENT_GATE = 1.1; // +10 % over R21.5

function discoverPairs(dir) {
  const dirs = readdirSync(dir, { withFileTypes: true })
    .filter((d) => d.isDirectory())
    .map((d) => d.name);
  const r215Runs = dirs
    .filter((d) => /^r215-run\d+$/.test(d))
    .map((d) => parseInt(d.replace(/^r215-run/, ""), 10))
    .sort((a, b) => a - b);
  const headRuns = dirs
    .filter((d) => /^head-run\d+$/.test(d))
    .map((d) => parseInt(d.replace(/^head-run/, ""), 10))
    .sort((a, b) => a - b);
  const pairIdxs = r215Runs.filter((i) => headRuns.includes(i));
  return pairIdxs;
}

function loadRun(dir, kind, idx) {
  const path = join(dir, `${kind}-run${idx}`, BENCH_FILE);
  if (!existsSync(path)) {
    throw new Error(`Missing run JSON: ${path}`);
  }
  return JSON.parse(readFileSync(path, "utf-8"));
}

function aggregateMsFromRun(run) {
  // Sum of latencyMs across all components in repeat 1.
  const repeat = run.repeats?.[0];
  if (!repeat) return 0;
  let sum = 0;
  for (const c of repeat.componentResults || []) {
    if (c.outcome === "success" || c.outcome === "succeeded") {
      sum += Number(c.latencyMs) || 0;
    }
  }
  return sum;
}

function componentMapFromRun(run) {
  const m = new Map();
  const repeat = run.repeats?.[0];
  if (!repeat) return m;
  for (const c of repeat.componentResults || []) {
    if (c.outcome === "success" || c.outcome === "succeeded") {
      m.set(c.relativePath, Number(c.latencyMs) || 0);
    }
  }
  return m;
}

// Bootstrap median CI on a paired array of ratios.
function bootstrapMedianCi(ratios, resamples = RESAMPLES) {
  if (ratios.length < 2) {
    return { lower: NaN, median: NaN, upper: NaN, n: ratios.length };
  }
  const n = ratios.length;
  const medians = new Array(resamples);
  for (let r = 0; r < resamples; r++) {
    const sample = new Array(n);
    for (let i = 0; i < n; i++) {
      sample[i] = ratios[(Math.random() * n) | 0];
    }
    sample.sort((a, b) => a - b);
    const mid = Math.floor(n / 2);
    medians[r] = n % 2 ? sample[mid] : (sample[mid - 1] + sample[mid]) / 2;
  }
  medians.sort((a, b) => a - b);
  const lowerIdx = Math.floor(0.025 * resamples);
  const upperIdx = Math.floor(0.975 * resamples);
  const midIdx = Math.floor(0.5 * resamples);
  return {
    lower: medians[lowerIdx],
    median: medians[midIdx],
    upper: medians[upperIdx],
    n,
  };
}

function median(arr) {
  const s = [...arr].sort((a, b) => a - b);
  if (s.length === 0) return NaN;
  const mid = Math.floor(s.length / 2);
  return s.length % 2 ? s[mid] : (s[mid - 1] + s[mid]) / 2;
}

// === Run ===
console.log(`Reading paired runs from: ${runsDir}`);
const pairs = discoverPairs(runsDir);
console.log(`Discovered ${pairs.length} pairs: ${pairs.join(", ")}`);
if (pairs.length < 7) {
  console.warn(`WARN: Fewer than 7 pairs (${pairs.length}) — Phase 4 amendment 5 requires ≥7.`);
}

const pairData = [];
for (const i of pairs) {
  const r215 = loadRun(runsDir, "r215", i);
  const head = loadRun(runsDir, "head", i);
  const r215Aggr = aggregateMsFromRun(r215);
  const headAggr = aggregateMsFromRun(head);
  const r215Comp = componentMapFromRun(r215);
  const headComp = componentMapFromRun(head);
  pairData.push({
    pairIdx: i,
    r215Aggr,
    headAggr,
    aggrRatio: headAggr / r215Aggr,
    r215Comp,
    headComp,
  });
  console.log(
    `  pair ${i}: r215_aggr=${r215Aggr.toFixed(1)}ms head_aggr=${headAggr.toFixed(1)}ms ratio=${(headAggr / r215Aggr).toFixed(4)}`,
  );
}

// === Aggregate CI ===
console.log("\n=== Aggregate ratio bootstrap CI ===");
const aggRatios = pairData.map((p) => p.aggrRatio);
const aggCi = bootstrapMedianCi(aggRatios);
console.log(`  n=${aggCi.n}`);
console.log(`  median ratio = ${aggCi.median.toFixed(4)}`);
console.log(`  95% CI = [${aggCi.lower.toFixed(4)}, ${aggCi.upper.toFixed(4)}]`);
const aggGatePass = aggCi.upper <= AGGREGATE_GATE;
console.log(
  `  GATE (CI upper ≤ ${AGGREGATE_GATE}): ${aggGatePass ? "PASS" : "FAIL"} (upper=${aggCi.upper.toFixed(4)})`,
);

// === Per-component CI ===
console.log("\n=== Per-component ratio bootstrap CI ===");
// Collect set of components observed in ALL pairs
const componentSet = new Set();
for (const p of pairData) {
  for (const path of p.r215Comp.keys()) {
    if (p.headComp.has(path)) componentSet.add(path);
  }
}
const components = [...componentSet].sort();
console.log(`  ${components.length} components present in all pairs`);

const componentCis = [];
for (const comp of components) {
  const ratios = [];
  for (const p of pairData) {
    const r = p.r215Comp.get(comp);
    const h = p.headComp.get(comp);
    if (r && h && r > 0 && h > 0) {
      ratios.push(h / r);
    }
  }
  if (ratios.length === pairData.length) {
    const ci = bootstrapMedianCi(ratios);
    componentCis.push({
      component: comp,
      ratios,
      median: ci.median,
      lower: ci.lower,
      upper: ci.upper,
      r215Median: median(pairData.map((p) => p.r215Comp.get(comp))),
      headMedian: median(pairData.map((p) => p.headComp.get(comp))),
    });
  }
}

// Sort by upper-bound descending
componentCis.sort((a, b) => b.upper - a.upper);

console.log("\nTop 20 components by CI upper bound:");
console.log(
  "  component                                         r215_med    head_med     ratio_med  ratio_lower  ratio_upper  gate",
);
for (const c of componentCis.slice(0, 20)) {
  const gate = c.upper <= PER_COMPONENT_GATE ? "OK" : "FAIL";
  console.log(
    `  ${c.component.padEnd(50)}  ${c.r215Median.toFixed(1).padStart(8)}  ${c.headMedian.toFixed(1).padStart(8)}  ${c.median.toFixed(4).padStart(8)}  ${c.lower.toFixed(4).padStart(11)}  ${c.upper.toFixed(4).padStart(11)}  ${gate}`,
  );
}

const componentGateFails = componentCis.filter((c) => c.upper > PER_COMPONENT_GATE);
console.log(`\n${componentGateFails.length} components exceed CI upper > ${PER_COMPONENT_GATE}`);
for (const f of componentGateFails) {
  console.log(`  FAIL ${f.component}  ratio CI=[${f.lower.toFixed(4)}, ${f.upper.toFixed(4)}]`);
}
const componentGatePass = componentGateFails.length === 0;
console.log(
  `\nPER-COMPONENT GATE (all CIs upper ≤ ${PER_COMPONENT_GATE}): ${componentGatePass ? "PASS" : "FAIL"}`,
);

// === Report ===
const report = {
  generatedAt: new Date().toISOString(),
  runsDir,
  pairs: pairs.length,
  resamples: RESAMPLES,
  aggregateGate: AGGREGATE_GATE,
  perComponentGate: PER_COMPONENT_GATE,
  aggregate: {
    ratios: aggRatios,
    ci: aggCi,
    gatePass: aggGatePass,
  },
  perComponent: {
    count: componentCis.length,
    gatePass: componentGatePass,
    failures: componentGateFails.map((c) => ({
      component: c.component,
      ratios: c.ratios,
      median: c.median,
      lower: c.lower,
      upper: c.upper,
    })),
    topByUpper: componentCis.slice(0, 30).map((c) => ({
      component: c.component,
      r215Median: c.r215Median,
      headMedian: c.headMedian,
      ratioMedian: c.median,
      ratioLower: c.lower,
      ratioUpper: c.upper,
    })),
    bottomByUpper: componentCis.slice(-10).map((c) => ({
      component: c.component,
      r215Median: c.r215Median,
      headMedian: c.headMedian,
      ratioMedian: c.median,
      ratioLower: c.lower,
      ratioUpper: c.upper,
    })),
  },
  perPair: pairData.map((p) => ({
    pairIdx: p.pairIdx,
    r215Aggr: p.r215Aggr,
    headAggr: p.headAggr,
    aggrRatio: p.aggrRatio,
  })),
};
const reportPath = join(runsDir, "bootstrap-ci.json");
writeFileSync(reportPath, JSON.stringify(report, null, 2));
console.log(`\nWrote ${reportPath}`);

const overallPass = aggGatePass && componentGatePass;
console.log(`\n=== OVERALL GATE: ${overallPass ? "PASS" : "FAIL"} ===`);
process.exit(overallPass ? 0 : 1);

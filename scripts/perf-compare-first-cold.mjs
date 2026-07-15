#!/usr/bin/env node
/**
 * Block 6.c perf gate: compare two `summary.csv` files emitted by
 * `cargo run --release --example audit_real_component_meta -p verter_bench`
 * and assert each of 10 components + the aggregate `total_ms` is within
 * +10% of the baseline.
 *
 * Usage:
 *   node scripts/perf-compare-first-cold.mjs <baseline.csv> <current.csv> [--tolerance=0.10]
 *
 * The baseline path may be supplied via the
 * `VERTER_AUDIT_BASELINE_CSV` env var; the current path is required
 * as the second positional arg. Defaults to a +10% tolerance per
 * component AND per aggregate (codex's "all 10 components + total
 * within +10%" gate from the brief).
 *
 * Exit codes:
 *   0 — every component + aggregate within tolerance.
 *   1 — at least one component OR the aggregate exceeded tolerance.
 *   2 — invalid input (missing file, bad CSV, etc.).
 */

import { readFileSync, existsSync } from "node:fs";

const COMPONENTS = [
  "Avatar",
  "AvatarGroup",
  "Button",
  "Editor",
  "Form",
  "Icon",
  "InputMenu",
  "Modal",
  "SelectMenu",
  "Table",
];

const TOTAL_MS_FIELD = "total_ms";
const TARGET_FIELD = "target";

function parseCsv(path) {
  if (!existsSync(path)) {
    process.stderr.write(`perf-compare: missing file: ${path}\n`);
    process.exit(2);
  }
  const text = readFileSync(path, "utf8");
  const lines = text.split(/\r?\n/).filter((l) => l.length > 0);
  if (lines.length < 2) {
    process.stderr.write(`perf-compare: empty CSV: ${path}\n`);
    process.exit(2);
  }
  const header = lines[0].split(",");
  const targetIdx = header.indexOf(TARGET_FIELD);
  const totalMsIdx = header.indexOf(TOTAL_MS_FIELD);
  if (targetIdx < 0 || totalMsIdx < 0) {
    process.stderr.write(
      `perf-compare: CSV missing required columns (${TARGET_FIELD} / ${TOTAL_MS_FIELD}): ${path}\n`,
    );
    process.exit(2);
  }
  const rows = {};
  for (let i = 1; i < lines.length; i++) {
    const cols = lines[i].split(",");
    const name = cols[targetIdx];
    const totalMs = Number(cols[totalMsIdx]);
    if (!Number.isFinite(totalMs)) {
      continue;
    }
    rows[name] = (rows[name] ?? 0) + totalMs;
  }
  return rows;
}

function fmtPct(pct) {
  const sign = pct >= 0 ? "+" : "";
  return `${sign}${pct.toFixed(1)}%`;
}

function fmtMs(ms) {
  return ms.toFixed(2);
}

function main() {
  const args = process.argv.slice(2);
  const positional = args.filter((a) => !a.startsWith("--"));
  const flags = args.filter((a) => a.startsWith("--"));

  let baselinePath = positional[0] ?? process.env.VERTER_AUDIT_BASELINE_CSV;
  const currentPath = positional[1];
  if (!baselinePath || !currentPath) {
    process.stderr.write(
      "usage: perf-compare-first-cold.mjs <baseline.csv> <current.csv> [--tolerance=0.10]\n",
    );
    process.exit(2);
  }

  let tolerance = 0.1;
  for (const flag of flags) {
    if (flag.startsWith("--tolerance=")) {
      tolerance = Number(flag.slice("--tolerance=".length));
      if (!Number.isFinite(tolerance) || tolerance <= 0) {
        process.stderr.write(`perf-compare: invalid --tolerance: ${flag}\n`);
        process.exit(2);
      }
    }
  }

  const baseline = parseCsv(baselinePath);
  const current = parseCsv(currentPath);

  process.stdout.write(`Block 6.c perf gate: tolerance = +${(tolerance * 100).toFixed(1)}%\n`);
  process.stdout.write(`  baseline = ${baselinePath}\n`);
  process.stdout.write(`  current  = ${currentPath}\n\n`);

  process.stdout.write(
    "| Component   | Baseline (ms) | Current (ms) |    Δ ms | Δ %  | Within tolerance? |\n",
  );
  process.stdout.write(
    "|-------------|---------------|--------------|---------|------|-------------------|\n",
  );

  let baselineTotal = 0;
  let currentTotal = 0;
  let failed = [];

  for (const name of COMPONENTS) {
    const base = baseline[name];
    const cur = current[name];
    if (base === undefined || cur === undefined) {
      process.stdout.write(
        `| ${name.padEnd(11)} | ${base !== undefined ? fmtMs(base).padStart(13) : "       MISSING"} | ${cur !== undefined ? fmtMs(cur).padStart(12) : "      MISSING"} |      -- |    -- | N/A               |\n`,
      );
      if (cur === undefined) {
        failed.push(`${name}: missing in current`);
      }
      continue;
    }
    baselineTotal += base;
    currentTotal += cur;
    const delta = cur - base;
    const deltaPct = (delta / base) * 100;
    const within = cur <= base * (1 + tolerance);
    const mark = within ? "Y" : "N";
    process.stdout.write(
      `| ${name.padEnd(11)} | ${fmtMs(base).padStart(13)} | ${fmtMs(cur).padStart(12)} | ${fmtMs(delta).padStart(7)} | ${fmtPct(deltaPct).padStart(5)} | ${mark.padEnd(17)} |\n`,
    );
    if (!within) {
      failed.push(
        `${name}: ${fmtMs(cur)}ms exceeds baseline ${fmtMs(base)}ms by ${fmtPct(deltaPct)}`,
      );
    }
  }

  const totalDelta = currentTotal - baselineTotal;
  const totalDeltaPct = baselineTotal > 0 ? (totalDelta / baselineTotal) * 100 : 0;
  const totalWithin = currentTotal <= baselineTotal * (1 + tolerance);
  const totalMark = totalWithin ? "Y" : "N";
  process.stdout.write(
    `|-------------|---------------|--------------|---------|------|-------------------|\n`,
  );
  process.stdout.write(
    `| ${"total".padEnd(11)} | ${fmtMs(baselineTotal).padStart(13)} | ${fmtMs(currentTotal).padStart(12)} | ${fmtMs(totalDelta).padStart(7)} | ${fmtPct(totalDeltaPct).padStart(5)} | ${totalMark.padEnd(17)} |\n`,
  );
  if (!totalWithin) {
    failed.push(
      `aggregate: ${fmtMs(currentTotal)}ms exceeds baseline ${fmtMs(baselineTotal)}ms by ${fmtPct(totalDeltaPct)}`,
    );
  }

  if (failed.length > 0) {
    process.stdout.write(`\nFAIL — ${failed.length} threshold(s) exceeded:\n`);
    for (const reason of failed) {
      process.stdout.write(`  - ${reason}\n`);
    }
    process.exit(1);
  }

  process.stdout.write(
    `\nPASS — every component + aggregate within +${(tolerance * 100).toFixed(1)}%.\n`,
  );
  process.exit(0);
}

main();

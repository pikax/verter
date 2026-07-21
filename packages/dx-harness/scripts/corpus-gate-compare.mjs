#!/usr/bin/env node
/**
 * Diff two corpus-gate receipts:
 *   node scripts/corpus-gate-compare.mjs <baseline.json> <current.json>
 *
 * Prints the per-route, per-kind deltas (latency percentiles, timeout/empty
 * counts, wedge transitions, max RSS) plus comparability caveats. Exits 0 on
 * a successful comparison (deltas are information, not a gate), 2 on usage or
 * load errors. The gate's pass/fail itself lives in the lane's assertions.
 */
import { pathToFileURL } from "node:url";
import path from "node:path";
import { existsSync } from "node:fs";

const [baselinePath, currentPath] = process.argv.slice(2);
if (!baselinePath || !currentPath) {
  console.error("usage: corpus-gate-compare.mjs <baseline.json> <current.json>");
  process.exit(2);
}

// The compare logic lives in the TS sources; consume the built package output
// so this script needs no TS loader. `pnpm --filter @verter/dx-harness build`
// produces it.
const receiptModuleUrl = pathToFileURL(
  path.join(import.meta.dirname, "..", "dist", "corpus-gate", "receipt.js"),
).href;
if (!existsSync(new URL(receiptModuleUrl))) {
  console.error(
    "dist/corpus-gate/receipt.js is missing — run `pnpm --filter @verter/dx-harness build` first",
  );
  process.exit(2);
}
const { loadCorpusReceipt, compareCorpusReceipts, formatCompare } = await import(receiptModuleUrl);

try {
  const baseline = loadCorpusReceipt(path.resolve(baselinePath));
  const current = loadCorpusReceipt(path.resolve(currentPath));
  const result = compareCorpusReceipts(baseline, current);
  for (const line of formatCompare(result)) console.log(line);
  console.log(
    `\ncomparable=${result.comparable} (${result.lines.length} metrics, ${result.caveats.length} caveat(s))`,
  );
} catch (error) {
  console.error(String(error?.message ?? error));
  process.exit(2);
}

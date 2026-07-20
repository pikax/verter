#!/usr/bin/env node
/**
 * Recombine fanned-out corpus-gate shard receipts into one verdict:
 *   node scripts/corpus-gate-summary.mjs <receipts-dir> [route,route,...]
 *
 * Each shard runs ONE route on its own machine, so the gate's wall clock is
 * the slowest shard rather than the sum. This script asserts every expected
 * route produced exactly one receipt (a missing shard is a FAIL, never a
 * skip), replays each shard's failures and advisories, and reports the
 * fan-out wall clock against the gate budget.
 *
 * Exits 0 when every expected route passed, 1 when the merged verdict fails,
 * 2 on usage or load errors. A budget breach is reported, not fatal.
 */
import { existsSync, readFileSync, readdirSync } from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";

const [receiptsDir, routesArg, budgetArg] = process.argv.slice(2);
if (!receiptsDir) {
  console.error("usage: corpus-gate-summary.mjs <receipts-dir> [routes-csv] [budget-ms]");
  process.exit(2);
}

const distUrl = pathToFileURL(
  path.join(import.meta.dirname, "..", "dist", "corpus-gate", "shards.js"),
).href;
if (!existsSync(new URL(distUrl))) {
  console.error(
    "dist/corpus-gate/shards.js is missing — run `pnpm --filter @verter/dx-harness build` first",
  );
  process.exit(2);
}
const { summarizeShards, formatShardSummary } = await import(distUrl);

const expectedRoutes = (routesArg ?? "tsserver,tsgo,shared-tsgo")
  .split(",")
  .map((entry) => entry.trim())
  .filter((entry) => entry.length > 0);
const budgetMs = Number(budgetArg ?? 20 * 60_000);

const root = path.resolve(receiptsDir);
if (!existsSync(root)) {
  console.error(`receipts directory does not exist: ${root}`);
  process.exit(2);
}

/** Collect every `*.json` receipt under the directory (one level of nesting). */
function collectReceiptPaths(dir) {
  const found = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) found.push(...collectReceiptPaths(full));
    else if (entry.name.endsWith(".json")) found.push(full);
  }
  return found;
}

const receipts = [];
for (const receiptPath of collectReceiptPaths(root)) {
  const parsed = JSON.parse(readFileSync(receiptPath, "utf8"));
  if (parsed?.harness !== "corpus-gate") {
    console.error(`skipping non-receipt JSON: ${receiptPath}`);
    continue;
  }
  receipts.push(parsed);
}

const summary = summarizeShards(receipts, expectedRoutes, budgetMs);
for (const line of formatShardSummary(summary)) console.log(line);
process.exit(summary.pass ? 0 : 1);

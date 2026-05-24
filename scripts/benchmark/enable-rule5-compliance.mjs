/**
 * Bulk-enable `rule5Compliance.enabled: true` on every
 * `packages/benchmark/audit-specs/component-meta/*.json` spec file.
 *
 * Round-16 Commit 3. The Rule-5 audit-validator extension (Round-16
 * Commit 2) added a per-spec opt-in field. This script lifts the
 * opt-in to "on by default" across the corpus so the regression
 * guard takes effect everywhere.
 *
 * Idempotent: re-running the script on an already-enabled spec is a
 * no-op. The script logs the per-file outcome (`enabled`, `already-on`,
 * or `error`).
 *
 * Usage:
 *   node scripts/benchmark/enable-rule5-compliance.mjs
 *
 * Inverse (disable everywhere) is not provided — the gate is the
 * intended steady-state. Edit individual spec files if a temporary
 * opt-out is needed for one component.
 */

import { readFileSync, readdirSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const repoRoot = resolve(__dirname, "../..");
const specDir = resolve(repoRoot, "packages/benchmark/audit-specs/component-meta");

let touched = 0;
let alreadyOn = 0;
let failed = 0;

for (const name of readdirSync(specDir).sort()) {
  if (!name.endsWith(".json")) continue;
  const specPath = resolve(specDir, name);
  let raw;
  let spec;
  try {
    raw = readFileSync(specPath, "utf-8");
    spec = JSON.parse(raw);
  } catch (err) {
    console.error(`  [error]      ${name} — ${err.message}`);
    failed += 1;
    continue;
  }
  if (
    spec.rule5Compliance &&
    typeof spec.rule5Compliance === "object" &&
    spec.rule5Compliance.enabled === true
  ) {
    console.log(`  [already-on] ${name}`);
    alreadyOn += 1;
    continue;
  }
  spec.rule5Compliance = { enabled: true };
  const renderedSpec = `${JSON.stringify(spec, null, 2)}\n`;
  writeFileSync(specPath, renderedSpec);
  console.log(`  [enabled]    ${name}`);
  touched += 1;
}

console.log("");
console.log(`Updated:    ${touched}`);
console.log(`Already on: ${alreadyOn}`);
console.log(`Failed:     ${failed}`);
process.exit(failed === 0 ? 0 : 1);

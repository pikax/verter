/**
 * CLI harness: validates trace artifacts against desired trace specs.
 *
 * Usage:
 *   npx tsx packages/benchmark/src/trace-check.ts <trace-dir> [--strict]
 *
 * Example:
 *   npx tsx packages/benchmark/src/trace-check.ts tmp/batch1-trace-002 --strict
 *
 * --strict: requires each spec to have forbidden and maxCount assertions
 *
 * The script:
 * 1. Discovers all *.json spec files under packages/benchmark/trace-specs/component-meta/
 * 2. For each spec, finds the matching trace log in <trace-dir>
 * 3. Validates the trace against the spec
 * 4. Reports pass/fail for each component
 * 5. Exits non-zero if any component fails
 */

import * as fs from "node:fs";
import * as path from "node:path";

import {
  formatValidationResult,
  loadTraceSpec,
  parseTraceLog,
  validateTrace,
} from "./trace-validator.js";

const traceDir = process.argv[2];
const strict = process.argv.includes("--strict");

if (!traceDir) {
  console.error("Usage: tsx src/trace-check.ts <trace-dir> [--strict]");
  process.exit(1);
}

const specsDir = path.resolve(import.meta.dirname, "../trace-specs/component-meta");
if (!fs.existsSync(specsDir)) {
  console.error(`Specs directory not found: ${specsDir}`);
  process.exit(1);
}

const specFiles = fs.readdirSync(specsDir).filter((f) => f.endsWith(".json"));
if (specFiles.length === 0) {
  console.error(`No spec files found in ${specsDir}`);
  process.exit(1);
}

const absTraceDir = path.resolve(traceDir);
if (!fs.existsSync(absTraceDir)) {
  console.error(`Trace directory not found: ${absTraceDir}`);
  process.exit(1);
}

let passed = 0;
let failed = 0;
let skipped = 0;

for (const specFile of specFiles.sort()) {
  const specContent = fs.readFileSync(path.join(specsDir, specFile), "utf-8");
  let spec;
  try {
    spec = loadTraceSpec(specContent, {
      requireForbidden: strict,
      requireMaxCounts: strict,
    });
  } catch (e) {
    console.error(`[ERROR] ${specFile}: ${e instanceof Error ? e.message : e}`);
    failed++;
    continue;
  }

  // Find matching trace file: try ComponentName.trace.log and
  // src__runtime__components__ComponentName.vue.trace.log patterns
  const componentName = spec.component;
  const candidates = [
    `${componentName}.trace.log`,
    `src__runtime__components__${componentName}.vue.trace.log`,
  ];

  let traceContent: string | null = null;
  let tracePath: string | null = null;
  for (const candidate of candidates) {
    const candidatePath = path.join(absTraceDir, candidate);
    if (fs.existsSync(candidatePath)) {
      traceContent = fs.readFileSync(candidatePath, "utf-8");
      tracePath = candidatePath;
      break;
    }
  }

  if (!traceContent || !tracePath) {
    console.log(`[SKIP] ${componentName} — no trace file found in ${absTraceDir}`);
    skipped++;
    continue;
  }

  const { events, coreEvents } = parseTraceLog(traceContent);
  if (events.length === 0) {
    console.log(`[SKIP] ${componentName} — trace file is empty or unparseable`);
    skipped++;
    continue;
  }

  const result = validateTrace(spec, events, coreEvents);
  console.log(formatValidationResult(result));
  console.log("");

  if (result.passed) {
    passed++;
  } else {
    failed++;
  }
}

console.log("---");
console.log(
  `Results: ${passed} passed, ${failed} failed, ${skipped} skipped out of ${specFiles.length} specs`,
);

if (failed > 0) {
  process.exit(1);
}

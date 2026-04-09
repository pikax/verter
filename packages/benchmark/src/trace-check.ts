/**
 * CLI harness: validates trace artifacts against desired trace specs.
 *
 * Usage:
 *   npx tsx packages/benchmark/src/trace-check.ts <trace-dir> [options]
 *
 * Options:
 *   --strict          Require each spec to have forbidden and maxCount assertions
 *   --batch <names>   Comma-separated component names to check. Only these specs
 *                     are loaded, and missing traces within the batch are FAILURES
 *                     (not skips). This is the campaign's "is this batch done?" gate.
 *
 * Examples:
 *   # Check all specs (skips missing traces)
 *   npx tsx packages/benchmark/src/trace-check.ts tmp/batch1-trace-002
 *
 *   # Batch gate: only check Accordion,Alert,App — fail if any are missing
 *   npx tsx packages/benchmark/src/trace-check.ts tmp/batch1-trace-002 --batch Accordion,Alert,App --strict
 */

import * as fs from "node:fs";
import * as path from "node:path";

import {
  formatValidationResult,
  loadTraceSpec,
  parseTraceLog,
  validateTrace,
} from "./trace-validator.js";
import { compareResultArtifactToExpected, findTraceLogPath } from "./trace-check-core.js";

const args = process.argv.slice(2);
const traceDir = args.find((a) => !a.startsWith("--"));
const strict = args.includes("--strict");
const checkExpected = args.includes("--check-expected");

let expectedDir = path.resolve(
  import.meta.dirname,
  "../benchmark-results/meta-ui/.expected-vue-component-meta",
);
const expectedDirIdx = args.indexOf("--expected-dir");
if (expectedDirIdx !== -1 && args[expectedDirIdx + 1]) {
  expectedDir = path.resolve(args[expectedDirIdx + 1]);
}

let batchComponents: Set<string> | null = null;
const batchIdx = args.indexOf("--batch");
if (batchIdx !== -1 && args[batchIdx + 1]) {
  batchComponents = new Set(args[batchIdx + 1].split(",").map((s) => s.trim()));
}

if (!traceDir) {
  console.error(
    "Usage: tsx src/trace-check.ts <trace-dir> [--strict] [--batch Name1,Name2,...] [--check-expected] [--expected-dir <dir>]",
  );
  process.exit(1);
}

const specsDir = path.resolve(import.meta.dirname, "../trace-specs/component-meta");
if (!fs.existsSync(specsDir)) {
  console.error(`Specs directory not found: ${specsDir}`);
  process.exit(1);
}

let specFiles = fs.readdirSync(specsDir).filter((f) => f.endsWith(".json"));
if (specFiles.length === 0) {
  console.error(`No spec files found in ${specsDir}`);
  process.exit(1);
}

// If --batch is set, filter specs to only the requested components.
if (batchComponents) {
  specFiles = specFiles.filter((f) => {
    const name = f.replace(".json", "");
    return batchComponents!.has(name);
  });
  // Check that all requested batch components have specs.
  let missingSpecs = false;
  for (const name of batchComponents) {
    if (!specFiles.includes(`${name}.json`)) {
      console.error(`[FAIL] ${name} — no spec file found in ${specsDir}`);
      missingSpecs = true;
    }
  }
  if (missingSpecs) {
    process.exit(1);
  }
}

const absTraceDir = path.resolve(traceDir);
if (!fs.existsSync(absTraceDir)) {
  console.error(`Trace directory not found: ${absTraceDir}`);
  process.exit(1);
}

if (checkExpected && !fs.existsSync(expectedDir)) {
  console.error(`Expected artifact directory not found: ${expectedDir}`);
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

  // Find the trace log in either the older flat layout or the structured traces/ layout.
  const componentName = spec.component;
  const tracePath = findTraceLogPath({
    componentName,
    componentPath: spec.componentPath,
    traceDir: absTraceDir,
  });
  const traceContent = tracePath ? fs.readFileSync(tracePath, "utf-8") : null;

  if (!traceContent || !tracePath) {
    if (batchComponents) {
      console.log(`[FAIL] ${componentName} — no trace file found in ${absTraceDir}`);
      failed++;
    } else {
      console.log(`[SKIP] ${componentName} — no trace file found in ${absTraceDir}`);
      skipped++;
    }
    continue;
  }

  const { events, coreEvents } = parseTraceLog(traceContent);
  if (events.length === 0) {
    if (batchComponents) {
      console.log(`[FAIL] ${componentName} — trace file is empty or unparseable`);
      failed++;
    } else {
      console.log(`[SKIP] ${componentName} — trace file is empty or unparseable`);
      skipped++;
    }
    continue;
  }

  const result = validateTrace(spec, events, coreEvents);
  console.log(formatValidationResult(result));
  let expectedComparisonPassed = true;
  if (checkExpected) {
    const expectedComparison = compareResultArtifactToExpected({
      componentPath: spec.componentPath,
      traceDir: absTraceDir,
      expectedDir,
    });
    if (expectedComparison.passed) {
      console.log(`[PASS] ${componentName} expected artifact matches pinned output`);
    } else {
      console.log(`[FAIL] ${componentName} expected artifact mismatch`);
      console.log(`  ${expectedComparison.message}`);
      expectedComparisonPassed = false;
    }
  }
  console.log("");

  if (result.passed && expectedComparisonPassed) {
    passed++;
  } else {
    failed++;
  }
}

console.log("---");
const total = batchComponents ? batchComponents.size : specFiles.length;
console.log(
  `Results: ${passed} passed, ${failed} failed, ${skipped} skipped out of ${total} specs`,
);

if (failed > 0) {
  process.exit(1);
}

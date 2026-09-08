#!/usr/bin/env node
// Standalone entry point for the aggregate memory budget contract.
//
//   node roadmap/0.1.0-tama/tools/validate-semantic-memory-budget.mjs
//   node roadmap/0.1.0-tama/tools/validate-semantic-memory-budget.mjs --emit-manifest <path>
//
// The same validation also runs inside `validate-program-dag.mjs --strict`,
// which is what binds it to the docs-domain gate profile. This entry point
// exists so the contract's own commands can be executed and read on their
// own, and so the frozen manifest can be materialised for the long-churn
// runner without re-implementing the expansion.

import fs from "node:fs";
import process from "node:process";

import { emitManifest, validateSemanticMemoryBudget } from "./semantic-memory-budget.mjs";
import { validateSchemaObject } from "./lib.mjs";

const emitIndex = process.argv.indexOf("--emit-manifest");
if (emitIndex !== -1) {
  const target = process.argv[emitIndex + 1];
  if (!target) {
    console.error("ERROR: --emit-manifest requires a path");
    process.exit(2);
  }
  fs.writeFileSync(target, emitManifest(), "utf8");
  console.log(`validate-semantic-memory-budget: wrote manifest to ${target}`);
}

const errors = validateSemanticMemoryBudget(validateSchemaObject);
if (errors.length) {
  console.error(errors.map((error) => `ERROR: ${error}`).join("\n"));
  process.exit(1);
}
console.log("validate-semantic-memory-budget: PASS");

#!/usr/bin/env node
// Standalone entry point for the aggregate memory budget contract.
//
//   node roadmap/0.1.0-tama/tools/validate-semantic-memory-budget.mjs
//   node roadmap/0.1.0-tama/tools/validate-semantic-memory-budget.mjs --emit-manifest <path>
//
// This is the ONLY entry point that runs the validation. It is bound in
// two places, both as a direct command: the `docs-domain` gate profile's
// final list in `catalogs/gate-profiles.toml`, and the required `Tama
// Roadmap` CI job. The negative controls
// (`tools/semantic-memory-budget.test.mjs`) are bound alongside it in both.
// The catalog declares both bindings, and validation resolves them against
// those two files, so removing either one fails here.
//
// It is deliberately NOT folded into `validate-program-dag.mjs --strict`.
// The closure instrument re-executes that validator against a partial
// mirror of the tree, and this contract reads repo-level files no such
// mirror contains — performance-gates.toml and the crates it resolves
// owner anchors in. Folding it in would turn that mirror red before its
// own planted mutation, destroying the instrument's proof that its plant
// applied.
//
// `--emit-manifest` materialises the frozen manifest for the long-churn
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

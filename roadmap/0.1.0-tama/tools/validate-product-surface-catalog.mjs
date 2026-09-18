#!/usr/bin/env node
// Standalone entry point for the public product-surface catalog.
//
//   node roadmap/0.1.0-tama/tools/validate-product-surface-catalog.mjs
//
// Bound in the docs-domain gate profile and the Tama Roadmap CI job, the
// same way as the MEM0 budget validator. Not folded into
// validate-program-dag.mjs --strict: this contract reads repo-level files
// (performance-gates.toml, published docs and package entrypoints) that
// the closure instrument's partial mirror does not contain.

import { validateProductSurfaceCatalog } from "./product-surface-catalog.mjs";
import { validateSchemaObject } from "./lib.mjs";

const errors = validateProductSurfaceCatalog(validateSchemaObject);
if (errors.length) {
  console.error(errors.map((error) => `ERROR: ${error}`).join("\n"));
  process.exit(1);
}
console.log("validate-product-surface-catalog: PASS");

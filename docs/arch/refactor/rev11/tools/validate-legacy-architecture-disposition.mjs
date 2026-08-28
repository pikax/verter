#!/usr/bin/env node
import { loadAuthority, validateLegacyArchitectureDisposition } from "./lib.mjs";

const errors = validateLegacyArchitectureDisposition(loadAuthority());
if (errors.length) {
  for (const error of errors) console.error(`ERROR: ${error}`);
  process.exit(1);
}
console.log(
  "validate-legacy-architecture-disposition: PASS (418 exact source paths deleted and reference-clean)",
);

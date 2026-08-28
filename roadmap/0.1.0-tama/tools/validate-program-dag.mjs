#!/usr/bin/env node
import { loadAuthority, metrics, validateAuthority } from "./lib.mjs";

const authority = loadAuthority();
const errors = validateAuthority(authority, { strict: process.argv.includes("--strict") });
if (errors.length) {
  console.error(errors.map((error) => `ERROR: ${error}`).join("\n"));
  process.exit(1);
}
const value = metrics(authority);
console.log(
  `validate-program-dag: PASS nodes=${value.nodes} edges=${value.edges} modules=${value.modules} charters=${value.charters} critical_path=${value.critical_path.length} max_width=${value.topological_width.max}`,
);

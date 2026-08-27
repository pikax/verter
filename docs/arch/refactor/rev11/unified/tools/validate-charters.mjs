#!/usr/bin/env node
import { loadAuthority, validateCharters } from "./lib.mjs";

const authority = loadAuthority();
const errors = validateCharters(authority.nodes, authority.packageRoot);
if (errors.length) {
  console.error(errors.map((error) => `ERROR: ${error}`).join("\n"));
  process.exit(1);
}
console.log(`validate-charters: PASS charters=${authority.nodes.length}`);

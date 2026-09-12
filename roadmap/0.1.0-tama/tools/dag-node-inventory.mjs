#!/usr/bin/env node
/**
 * The inventory of `validate-program-dag.mjs`: how many DAG nodes the
 * authority declares, counted without the validator.
 *
 * The validator prints the node count it loaded, and a clean run of it is
 * otherwise judged only on that line — so a loader that silently stops
 * reading some modules, or some rows of one, reports a smaller, perfectly
 * green count. This is the count the tree itself declares: every `[[node]]`
 * table header in every module the authority root lists, read as raw lines
 * rather than through the loader or its model. The two agree unless the
 * validator stopped reaching part of the DAG.
 *
 * Prints one line, which `parseInventory` reads back.
 */

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { readToml } from "./toml.mjs";

const PACKAGE_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const AUTHORITY = path.join(PACKAGE_ROOT, "authority");

const root = readToml(path.join(AUTHORITY, "root.toml"));
if (!Array.isArray(root.modules) || root.modules.length === 0) {
  console.error("dag-node-inventory: the authority root lists no DAG modules");
  process.exit(1);
}
let nodes = 0;
for (const module of root.modules) {
  const text = fs.readFileSync(path.join(AUTHORITY, module), "utf8");
  nodes += text
    .split(/\r?\n/u)
    .filter((line) => /^\s*\[\[\s*node\s*\]\]\s*(#.*)?$/u.test(line)).length;
}
console.log(`tool-line inventory: nodes=${nodes}`);

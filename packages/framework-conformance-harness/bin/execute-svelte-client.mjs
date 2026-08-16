#!/usr/bin/env node
// Thin CLI over the Svelte CLIENT executor (`src/execute-svelte-client.mjs`).
// It adds no semantics: it mounts each supplied module against the pinned
// official client runtime and prints what rendered.
//
// Usage: node bin/execute-svelte-client.mjs --input <file.json>
// where the input is `{ "modules": { "<label>": "<client module source>" },
//                      "props": { … } }`.
//
// Exit code 0 means every module was ATTEMPTED; whether each mounted is in its
// own `ok` field. Exit code 2 means the invocation was malformed.

import { readFileSync } from "node:fs";

import { cleanupClientScratch, executeSvelteClient } from "../src/execute-svelte-client.mjs";

function fail(message) {
  process.stderr.write(`${message}\n`);
  process.exit(2);
}

const args = process.argv.slice(2);
let inputPath = null;
for (let i = 0; i < args.length; i += 1) {
  if (args[i] === "--input") {
    inputPath = args[i + 1] ?? null;
    i += 1;
  } else {
    fail(`unknown argument: ${args[i]}`);
  }
}
if (inputPath === null) fail("missing --input <file.json>");

let payload;
try {
  payload = JSON.parse(readFileSync(inputPath, "utf8"));
} catch (error) {
  fail(`cannot read ${inputPath}: ${String(error?.message ?? error)}`);
}

const modules = payload?.modules;
if (typeof modules !== "object" || modules === null || Object.keys(modules).length === 0) {
  fail("the input carries no `modules` object");
}
const props = payload?.props ?? {};

const results = {};
let runtime = null;
for (const [label, code] of Object.entries(modules)) {
  if (typeof code !== "string") fail(`module ${JSON.stringify(label)} is not a string`);
  const result = await executeSvelteClient(code, props);
  runtime = result.runtime ?? runtime;
  results[label] = result;
}
cleanupClientScratch();

// The runtime every module bound is reported at the top level so a caller can
// pin it: a mount measured against a runtime other than the pinned one decides
// nothing, whether it passes or fails.
process.stdout.write(JSON.stringify({ ...results, runtime }));

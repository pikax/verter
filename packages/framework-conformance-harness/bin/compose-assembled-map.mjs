#!/usr/bin/env node
// The independent JavaScript reference implementation, driven as a subprocess.
//
// This is the ORACLE side of the cross-implementation equality harness: the
// production Rust assembler and this reference are run over the SAME serialized
// pre-assembly input DTO (`spec/assembled-map-composition-layer1.md` §3.3), and
// the two decoded map artifacts are compared for exact equality — field for
// field and position for position, including the exact ordered segment
// sequence (§7.1).
//
// It adds NO semantics of its own. Its whole job is transport: read DTOs, call
// `composeAssembledVueMainModule`, print the results. Every rule it appears to
// apply is one the reference module already applies.
//
// USAGE
//   node bin/compose-assembled-map.mjs            # a batch on stdin
//   node bin/compose-assembled-map.mjs <file>     # a batch in a file
//
// INPUT  (JSON)  { "cases": [ <AssembleInput>, ... ] }
// OUTPUT (JSON)  { "results": [ <Result>, ... ] }   — one per case, in order
//
// Each result is one of:
//   { outcome: "composed", code, map, segments, provenance }
//   { outcome: "MissingRequiredInputMap", fragment }
//   { outcome: "UncomposableInputMap", family, code, fragment }
//   { outcome: "MalformedAssembleInput", message }   // §11.4 — out of scope
//
// A case that throws anything other than `MalformedAssembleInputError` is NOT
// swallowed into a result: it exits non-zero, because an unexpected throw is a
// defect in the reference, not an outcome of the algebra.

import { readFileSync } from "node:fs";

import {
  composeAssembledVueMainModule,
  MalformedAssembleInputError,
} from "../src/assembled-map-composition-reference.mjs";

function readAllStdin() {
  // Blocking read of fd 0, so the driver needs no async plumbing.
  return readFileSync(0, "utf8");
}

function runCase(input) {
  try {
    return composeAssembledVueMainModule(input);
  } catch (error) {
    if (error instanceof MalformedAssembleInputError) {
      return { outcome: "MalformedAssembleInput", message: error.message };
    }
    throw error;
  }
}

function main() {
  const file = process.argv[2];
  const raw = file === undefined ? readAllStdin() : readFileSync(file, "utf8");

  let batch;
  try {
    batch = JSON.parse(raw);
  } catch (error) {
    process.stderr.write(`compose-assembled-map: input is not JSON: ${error.message}\n`);
    process.exit(2);
  }
  if (batch === null || typeof batch !== "object" || !Array.isArray(batch.cases)) {
    process.stderr.write('compose-assembled-map: input must be { "cases": [ ... ] }\n');
    process.exit(2);
  }

  const results = batch.cases.map((input, index) => {
    try {
      return runCase(input);
    } catch (error) {
      process.stderr.write(
        `compose-assembled-map: case ${index} threw an unexpected error:\n${error?.stack ?? error}\n`,
      );
      process.exit(3);
      return undefined; // unreachable; keeps the map total for a reader
    }
  });

  process.stdout.write(JSON.stringify({ results }));
}

main();

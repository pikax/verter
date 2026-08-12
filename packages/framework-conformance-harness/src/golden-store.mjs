// Immutable golden storage (conformance-goldens.md "Golden provenance").
//
// WRITE PATH: exactly one function in this module can write a golden file
// (`writeGoldenFile`), and it is called ONLY from `bin/generate-goldens.mjs`
// — never from the comparator, never from a candidate-producing module. The
// comparator (`src/compare.mjs`) never imports `node:fs` write functions at
// all, so "candidate output cannot update its own expectation" holds
// structurally: there is no code path from a comparison result back into
// this module's write function. `readGoldenFile` returns a deep-frozen
// object so an accidental in-memory mutation cannot silently succeed either.

import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, renameSync, writeFileSync } from "node:fs";
import { dirname } from "node:path";

export function sha256(text) {
  return createHash("sha256").update(text, "utf8").digest("hex");
}

function deepFreeze(value) {
  if (value === null || typeof value !== "object" || Object.isFrozen(value)) return value;
  Object.freeze(value);
  for (const key of Object.keys(value)) deepFreeze(value[key]);
  return value;
}

/**
 * @param {string} path
 * @param {object} record must already carry full provenance (see
 *   golden-generate.mjs's `buildProvenance`)
 */
export function writeGoldenFile(path, record) {
  mkdirSync(dirname(path), { recursive: true });
  const text = `${JSON.stringify(record, null, 2)}\n`;
  // Atomic: write to a sibling temp file, then rename — a crash mid-write
  // never leaves a torn/partial golden on disk.
  const tmpPath = `${path}.tmp-${process.pid}`;
  writeFileSync(tmpPath, text, "utf8");
  renameSync(tmpPath, path);
  return text;
}

export function readGoldenFile(path) {
  const record = JSON.parse(readFileSync(path, "utf8"));
  return deepFreeze(record);
}

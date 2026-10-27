// The STP engine pins, read from the ONE place they are declared:
// tests/sfc-projection/STP1/products/engine-matrix.json. Node protocols that
// require their evidence index to name the engine pins check against these
// values instead of spelling the versions again, so moving an engine is one
// edit to the matrix.

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");

export const ENGINE_MATRIX_REL = "tests/sfc-projection/STP1/products/engine-matrix.json";

export function loadEngineMatrixPins(repoRoot = REPO_ROOT) {
  const matrix = JSON.parse(fs.readFileSync(path.join(repoRoot, ENGINE_MATRIX_REL), "utf8"));
  const pins = {};
  for (const engine of matrix.engines || []) {
    pins[engine.id] = engine.version;
  }
  if (!pins["ts-js"] || !pins["ts-native"]) {
    throw new Error(`${ENGINE_MATRIX_REL} must declare the ts-js and ts-native engines`);
  }
  return pins;
}

/** `ts-js <v> and ts-native <v>` — the phrase protocol diagnostics use. */
export function describeEnginePins(pins = loadEngineMatrixPins()) {
  return `ts-js ${pins["ts-js"]} and ts-native ${pins["ts-native"]}`;
}

/** Whether `text` names both engine pins. */
export function textRecordsEnginePins(text, pins = loadEngineMatrixPins()) {
  return text.includes(pins["ts-js"]) && text.includes(pins["ts-native"]);
}

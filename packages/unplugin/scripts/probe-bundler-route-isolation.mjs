// Per-invocation fixture allocation and fail-loud outcome policy for
// probe-bundler-route.mjs. A shared on-disk path is a collision: concurrent
// probes delete each other's files and can still exit 0 with fresh:true.

import { mkdir, mkdtemp } from "node:fs/promises";
import path from "node:path";

export const RECOMPILE_LEAF_PREFIX = "recompile-";

export async function allocateRecompileFixture(parentDir) {
  await mkdir(parentDir, { recursive: true });
  return mkdtemp(path.join(parentDir, RECOMPILE_LEAF_PREFIX));
}

export function collectErroredCaseLabels(cases = {}, exportCases = {}) {
  return [
    ...Object.entries(cases),
    ...Object.entries(exportCases).map(([label, value]) => [`exportCase.${label}`, value]),
  ]
    .filter(([, value]) => value?.outcome === "error")
    .map(([label]) => label)
    .sort();
}

export function probeExitCode(erroredCases) {
  return erroredCases.length > 0 ? 1 : 0;
}

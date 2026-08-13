#!/usr/bin/env node
/**
 * Candidate acceptance CLI over src/check-candidate.mjs.
 *
 * Usage:
 *   node bin/check-candidate.mjs --golden <logical-name> --candidate <file.json> [--authoritative]
 *
 * The candidate file is JSON: { "code": string, "map"?: object|null,
 * "diagnostics"?: array }. The golden name is a committed manifest entry
 * (e.g. "vue/basic-interpolation__vdom__map1__prod0").
 *
 * Default: an axis whose environment prerequisite is absent reports
 * `skipped` with its reason and does not fail. With --authoritative (or
 * BF2_AUTHORITATIVE=1): every applicable axis must genuinely run — a
 * skipped axis exits 2 (fail-closed), so a consumer can prove its
 * acceptance evidence executed. Comparison failure exits 1; pass exits 0.
 */

import { readFileSync } from "node:fs";

import { checkCandidate } from "../src/check-candidate.mjs";
import { cleanupScratch as cleanupVueScratch } from "../src/execute-vue-runtime.mjs";
import { cleanupScratch as cleanupVaporScratch } from "../src/execute-vue-vapor.mjs";
import { cleanupScratch as cleanupSvelteScratch } from "../src/execute-svelte-runtime.mjs";

function parseArgs(argv) {
  const args = { authoritative: ["1", "true"].includes(process.env.BF2_AUTHORITATIVE ?? "") };
  for (let i = 0; i < argv.length; i += 1) {
    if (argv[i] === "--golden") args.goldenName = argv[++i];
    else if (argv[i] === "--candidate") args.candidatePath = argv[++i];
    else if (argv[i] === "--authoritative") args.authoritative = true;
    else throw new Error(`unknown argument: ${argv[i]}`);
  }
  if (!args.goldenName || !args.candidatePath) {
    throw new Error("required: --golden <logical-name> --candidate <file.json>");
  }
  return args;
}

async function main() {
  const { goldenName, candidatePath, authoritative } = parseArgs(process.argv.slice(2));
  const candidate = JSON.parse(readFileSync(candidatePath, "utf8"));
  // stdout is EXACTLY one JSON report. Oracle runtime modules print
  // informational banners (e.g. the dev-build notice) on first evaluation —
  // during link-axis export-surface loading as well as runtime mounts — so
  // console chatter is routed to stderr for the duration of the check.
  const originalLog = console.log;
  const originalInfo = console.info;
  console.log = console.error;
  console.info = console.error;
  let result;
  try {
    result = await checkCandidate({ goldenName, candidate, authoritative });
  } finally {
    console.log = originalLog;
    console.info = originalInfo;
  }
  console.log(JSON.stringify({ goldenName, authoritative, ...result }, null, 2));
  if (result.verdict === "pass") return 0;
  const onlyAuthoritativeSkips =
    result.reasons.length > 0 &&
    result.reasons.every((reason) => reason.startsWith("authoritative mode:"));
  return onlyAuthoritativeSkips ? 2 : 1;
}

main()
  .then((code) => {
    cleanupVueScratch();
    cleanupVaporScratch();
    cleanupSvelteScratch();
    // exitCode (not process.exit) lets stdout flush before the process
    // ends — a large JSON report survives a piped consumer intact.
    process.exitCode = code;
  })
  .catch((error) => {
    console.error(String(error?.stack ?? error));
    process.exitCode = 1;
  });

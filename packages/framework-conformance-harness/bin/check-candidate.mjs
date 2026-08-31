#!/usr/bin/env node
/**
 * Candidate acceptance CLI over src/check-candidate.mjs.
 *
 * Usage:
 *   node bin/check-candidate.mjs --golden <logical-name> --candidate <file.json> [--authoritative]
 *   node bin/check-candidate.mjs --batch <file.json> [--authoritative]
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
 * Batch input is `{ "cases": [{ "caseId": string, "goldenName": string,
 * "candidate": object }] }`. Cases run sequentially in one process and the
 * ordered result envelope is emitted as exactly one JSON value on stdout.
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
    else if (argv[i] === "--batch") args.batchPath = argv[++i];
    else if (argv[i] === "--authoritative") args.authoritative = true;
    else throw new Error(`unknown argument: ${argv[i]}`);
  }
  const single = Boolean(args.goldenName || args.candidatePath);
  if (args.batchPath && single) {
    throw new Error("--batch is mutually exclusive with --golden/--candidate");
  }
  if (!args.batchPath && (!args.goldenName || !args.candidatePath)) {
    throw new Error(
      "required: --golden <logical-name> --candidate <file.json>, or --batch <file.json>",
    );
  }
  return args;
}

function resultExitCode(result) {
  if (result.verdict === "pass") return 0;
  const onlyAuthoritativeSkips =
    result.reasons.length > 0 &&
    result.reasons.every((reason) => reason.startsWith("authoritative mode:"));
  return onlyAuthoritativeSkips ? 2 : 1;
}

function typedFailure(error) {
  return {
    kind: "exception",
    name: typeof error?.name === "string" ? error.name : "Error",
    message: String(error?.message ?? error),
  };
}

function validateBatch(value) {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    throw new TypeError("batch input must be an object");
  }
  if (!Array.isArray(value.cases)) throw new TypeError("batch input.cases must be an array");
  return value.cases;
}

async function checkBatch(batchPath, authoritative) {
  const cases = validateBatch(JSON.parse(readFileSync(batchPath, "utf8")));
  const reports = [];
  for (const [index, entry] of cases.entries()) {
    const caseId = typeof entry?.caseId === "string" ? entry.caseId : null;
    const goldenName = typeof entry?.goldenName === "string" ? entry.goldenName : null;
    try {
      if (caseId === null) throw new TypeError(`case ${index}.caseId must be a string`);
      if (goldenName === null) throw new TypeError(`case ${index}.goldenName must be a string`);
      if (
        entry.candidate === null ||
        typeof entry.candidate !== "object" ||
        Array.isArray(entry.candidate)
      ) {
        throw new TypeError(`case ${index}.candidate must be an object`);
      }
      const result = await checkCandidate({
        goldenName,
        candidate: entry.candidate,
        authoritative,
      });
      reports.push({
        index,
        caseId,
        goldenName,
        status: "reported",
        exitCode: resultExitCode(result),
        result: { goldenName, authoritative, ...result },
      });
    } catch (error) {
      reports.push({
        index,
        caseId,
        goldenName,
        status: "error",
        failure: typedFailure(error),
      });
    }
  }

  const hasError = reports.some((report) => report.status === "error");
  const envelope = {
    schema: "verter-check-candidate-batch/v1",
    verdict: hasError ? "error" : "reported",
    reports,
  };
  let exitCode = 0;
  if (hasError) exitCode = 3;
  else if (reports.some((report) => report.exitCode === 1)) exitCode = 1;
  else if (reports.some((report) => report.exitCode === 2)) exitCode = 2;
  return { envelope, exitCode };
}

async function main() {
  const { goldenName, candidatePath, batchPath, authoritative } = parseArgs(process.argv.slice(2));
  // stdout is EXACTLY one JSON value. Oracle runtime modules print
  // informational banners (e.g. the dev-build notice) on first evaluation —
  // during link-axis export-surface loading as well as runtime mounts — so
  // console chatter is routed to stderr for the duration of the check.
  const originalLog = console.log;
  const originalInfo = console.info;
  console.log = console.error;
  console.info = console.error;
  let payload;
  let exitCode;
  try {
    if (batchPath) {
      const batch = await checkBatch(batchPath, authoritative);
      payload = batch.envelope;
      exitCode = batch.exitCode;
    } else {
      const candidate = JSON.parse(readFileSync(candidatePath, "utf8"));
      const result = await checkCandidate({ goldenName, candidate, authoritative });
      payload = { goldenName, authoritative, ...result };
      exitCode = resultExitCode(result);
    }
  } finally {
    console.log = originalLog;
    console.info = originalInfo;
  }
  console.log(JSON.stringify(payload, null, batchPath ? 0 : 2));
  return exitCode;
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

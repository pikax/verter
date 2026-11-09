#!/usr/bin/env node
// Entry point of the architecture-closure validator; see
// tools/architecture-closure/README.md for the inputs and the reason codes.
//
//   node tools/validate-architecture-closure.mjs --mode owner-lock|closure \
//     --plan <export.json> --report <report.json>
//
// Prints one JSON decision on stdout. Exit status: 0 accepted, 1 rejected,
// 2 a usage error or an unreadable input (no decision was made).

import { readFileSync } from "node:fs";
import process from "node:process";
import { parseArgs } from "node:util";

import { MODES, validateClosure } from "./architecture-closure/validate.mjs";

const USAGE = `usage: node tools/validate-architecture-closure.mjs --mode ${MODES.join("|")} --plan <export.json> --report <report.json>`;

function usageFailure(message) {
  process.stderr.write(`${message}\n${USAGE}\n`);
  process.exit(2);
}

function readInput(label, file) {
  try {
    return JSON.parse(readFileSync(file, "utf8"));
  } catch (error) {
    return usageFailure(`cannot read ${label} ${file}: ${error.message}`);
  }
}

function main(argv) {
  let values;
  try {
    ({ values } = parseArgs({
      args: argv,
      options: {
        mode: { type: "string" },
        plan: { type: "string" },
        report: { type: "string" },
      },
    }));
  } catch (error) {
    return usageFailure(error.message);
  }
  if (!MODES.includes(values.mode))
    return usageFailure(`--mode must be one of ${MODES.join(", ")}`);
  if (!values.plan) return usageFailure("--plan is required");
  if (!values.report) return usageFailure("--report is required");

  const plan = readInput("plan export", values.plan);
  const report = readInput("report", values.report);
  let decision;
  try {
    decision = validateClosure({ mode: values.mode, plan, report });
  } catch (error) {
    // An input the validator could not evaluate is no decision, never a rejection.
    return usageFailure(`cannot evaluate the inputs: ${error.stack ?? error}`);
  }
  process.stdout.write(`${JSON.stringify(decision, null, 2)}\n`);
  process.exitCode = decision.ok ? 0 : 1;
}

main(process.argv.slice(2));

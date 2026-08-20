#!/usr/bin/env node
// Stack-window validator (AMD-001 §1 — the Node reimplementation of
// tools/validate_stack_window.py under maintainer ruling R-4; the repo
// forbids committed Python, see CLAUDE.md Dependencies Policy). Validates a
// resolved stack-window record (templates/stack-window.template.toml,
// instantiated) against contracts/stacked-prs.md.
//
//   node scripts/validate-stack-window.mjs \
//     --window <stack-window.toml> --mode template|live \
//     [--dag <program-dag.toml>] \
//     [--current-program-state <program-state.toml>]
//
// --dag enables the ATOMIC_REVIEW private-layer class check (contracts/
// stacked-prs.md 3.2 — a private layer must repeat the acceptance block's own
// id or name a "foundational-private-checkpoint"-class DAG block).
//
// --current-program-state is the composite cross-validation named in
// contracts/stacked-prs.md §2 ("tools/validate_stack_window.py
// --current-program-state ..."): the mutable ledger (program-state.toml) is
// checked against this immutable snapshot (stack_id, stack_snapshot_digest —
// the SHA-256 of this fully resolved file — and stack_layer per block, plus
// the checkpoint-status binding for a NON_MERGEABLE_PRIVATE_LAYER). This is
// the SAME model scripts/validate-program-state.mjs uses (via
// scripts/lib/stack-window-lib.mjs) to supersede its own PRIVATE_CHECKPOINT-
// predecessor fail-closed refusal (AMD-001 §3) — one model, two entry
// points, never a forked second implementation.
//
// Exit: 0 pass, 1 validation failure (one violation per line), 2 usage /
// unreadable input.

import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { basename } from "node:path";
import process from "node:process";
import { TomlError, parseToml } from "./lib/rev11-toml.mjs";
import {
  buildDagClassMap,
  buildStateById,
  crossValidateAgainstProgramState,
  validateStackWindowStructure,
} from "./lib/stack-window-lib.mjs";

function usageFail(msg) {
  process.stderr.write(
    `${msg}\nusage: node scripts/validate-stack-window.mjs --window <stack-window.toml> --mode template|live [--dag <program-dag.toml>] [--current-program-state <program-state.toml>]\n`,
  );
  process.exit(2);
}

function parseArgs(argv) {
  const opts = Object.create(null);
  const known = ["--window", "--mode", "--dag", "--current-program-state"];
  for (let i = 0; i < argv.length; i += 2) {
    const flag = argv[i];
    const value = argv[i + 1];
    if (!known.includes(flag)) usageFail(`unknown argument: ${flag}`);
    if (value === undefined) usageFail(`missing value for ${flag}`);
    opts[flag.slice(2)] = value;
  }
  if (!opts.window || !opts.mode) usageFail("--window and --mode are both required");
  if (opts.mode !== "template" && opts.mode !== "live") {
    usageFail(`--mode must be "template" or "live", got ${JSON.stringify(opts.mode)}`);
  }
  return opts;
}

function loadFile(path, what) {
  try {
    return readFileSync(path, "utf8");
  } catch (err) {
    usageFail(`cannot read ${what} file ${path}: ${err.message}`);
  }
}

function loadToml(path, what) {
  const text = loadFile(path, what);
  try {
    return { text, parsed: parseToml(text, path) };
  } catch (err) {
    if (err instanceof TomlError) {
      process.stderr.write(`VIOLATION: ${err.message}\n`);
      process.stderr.write("FAIL: 0 checks completed — input could not be parsed\n");
      process.exit(1);
    }
    throw err;
  }
}

function main() {
  const opts = parseArgs(process.argv.slice(2));
  const violations = [];
  const v = (msg) => violations.push(msg);

  const { text: windowText, parsed: window } = loadToml(opts.window, "stack-window");

  let dagClassMap = null;
  if (opts.dag) {
    const { parsed: dag } = loadToml(opts.dag, "DAG");
    dagClassMap = buildDagClassMap(dag);
  }

  const structural = validateStackWindowStructure(window, {
    cliMode: opts.mode,
    dagClassMap,
    label: opts.window,
  });
  for (const msg of structural) v(msg);

  const snapshotDigest = createHash("sha256").update(windowText).digest("hex");

  if (opts["current-program-state"]) {
    if (opts.mode !== "live") {
      v(
        `--current-program-state was given but --mode is ${JSON.stringify(opts.mode)} — cross-validation against the mutable ledger requires a live (fully resolved) window`,
      );
    } else if (structural.length > 0) {
      v(
        `--current-program-state cross-validation skipped — ${opts.window} failed its own structural validation first (see violations above); a malformed window cannot be meaningfully cross-checked`,
      );
    } else {
      const { parsed: state } = loadToml(opts["current-program-state"], "program-state");
      const stateById = buildStateById(state);
      const cross = crossValidateAgainstProgramState({
        window,
        label: opts.window,
        snapshotDigest,
        stateById,
      });
      for (const msg of cross) v(msg);
    }
  }

  if (violations.length > 0) {
    for (const violation of violations) process.stderr.write(`VIOLATION: ${violation}\n`);
    process.stderr.write(
      `FAIL: ${violations.length} violation(s) in ${opts.window} (mode ${opts.mode})\n`,
    );
    process.exit(1);
  }
  process.stdout.write(
    `OK: ${basename(opts.window)} (${opts.window}) — validated ${(Array.isArray(window.layer) ? window.layer : []).length} layer(s) in mode ${opts.mode}; StackSnapshotId (SHA-256) = ${snapshotDigest}\n`,
  );
  process.exit(0);
}

main();

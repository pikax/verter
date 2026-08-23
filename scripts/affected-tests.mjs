#!/usr/bin/env node

/**
 * affected-tests.mjs — fast INNER-LOOP selector for which Rust tests to run
 * for a given change. NOT the canonical gate: `node scripts/gate.mjs` is the
 * only thing that runs before landing. This tool is for an agent iterating
 * on a branch who wants a fast approximate signal, not a merge decision.
 *
 * Algorithm:
 *   1. Get the set of changed files (default: working-tree diff against
 *      HEAD, tracked + untracked; or an explicit `--range <rev>..<rev>`).
 *   2. Classify each changed file: escape hatch (fall back to the FULL
 *      workspace and say so loudly), maps to a workspace crate, a known
 *      non-Rust path (ignored), or unrecognized (also falls back to FULL —
 *      over-select, never under-select).
 *   3. If nothing forced a full-workspace fallback, BFS the crate
 *      reverse-dependency graph (built once from `cargo metadata`) from
 *      every directly-changed crate to the full transitive closure of
 *      crates that depend on it, directly or indirectly.
 *   4. Print the equivalent `cargo nextest run -p <pkg> -p <pkg> ...`
 *      invocation. `--run` actually executes it.
 *
 * Usage:
 *   node scripts/affected-tests.mjs [--range <rev>..<rev>] [--run] [--json]
 *
 * See `node scripts/affected-tests.mjs --help` for the full escape-hatch
 * list this tool enforces.
 */

import { execFileSync } from "node:child_process";
import process from "node:process";
import {
  buildWorkspaceIndex,
  buildReverseDependencyGraph,
  transitiveDependents,
  classifyChangedFile,
  ESCAPE_HATCH_RULES,
} from "./lib/crate-graph.mjs";

export const BANNER =
  "This is NOT the canonical gate. It never replaces `node scripts/gate.mjs` at " +
  "landing — full gate required before merge.";

// Distinct from the generic usage-error exit code (2): every changed file
// classified as a known non-Rust path with no owning crate, so the selector
// has NOTHING to run. This is deliberately NOT exit 0 — a bare
// `cargo nextest run` with zero `-p` flags is not "run nothing", it is "run
// the WHOLE workspace" (there are no `default-members` declared), so this
// state must never be reported as a silent, successful no-op that could then
// quietly fall through to a full-workspace run.
export const EMPTY_SELECTION_EXIT_CODE = 3;

const HELP = `${BANNER}

affected-tests.mjs — select the Rust crates whose tests plausibly cover a
change: the changed crate(s)' own tests, plus every crate that transitively
depends on a changed crate.

Usage:
  node scripts/affected-tests.mjs [options]

Options:
  --range <rev>..<rev>   Diff this commit range instead of the working tree
                          (passed straight to \`git diff --name-only\`).
  --run                  Actually execute the selected \`cargo nextest run\`.
  --json                 Emit a machine-readable JSON report instead of text.
  --help                 Show this message.

Default input (no --range): the working-tree diff against HEAD (staged +
unstaged, tracked files) UNION untracked new files. This is a fast INNER-LOOP
tool for an agent iterating on a branch — always confirm with the full gate
before landing.

Escape hatches — any changed file matching one of these forces a FULL
workspace recommendation instead of a partial selection, because the
selector cannot soundly reason about the blast radius:

${ESCAPE_HATCH_RULES.map((r) => `  - ${r.id}: ${r.reason}`).join("\n")}

Over-select safety net: a changed file that maps to no workspace crate, is
not a known non-Rust path (docs/, packages/ non-generated sources, editor
config, etc.), and is not caught by an escape hatch above is treated as
UNRECOGNIZED and also forces the FULL workspace.
`;

function git(args, cwd) {
  return execFileSync("git", args, { cwd, maxBuffer: 1024 * 1024 * 64 }).toString("utf8");
}

function repoRoot(cwd) {
  return git(["rev-parse", "--show-toplevel"], cwd).trim();
}

/**
 * @param {string} root
 * @param {string | null} range
 * @returns {string[]} workspace-root-relative, forward-slash changed paths
 */
export function getChangedFiles(root, range) {
  let files;
  if (range) {
    files = git(["diff", "--no-renames", "--name-only", range], root).split("\n").filter(Boolean);
  } else {
    const tracked = git(["diff", "--no-renames", "--name-only", "HEAD"], root)
      .split("\n")
      .filter(Boolean);
    const untracked = git(["ls-files", "--others", "--exclude-standard"], root)
      .split("\n")
      .filter(Boolean);
    files = [...new Set([...tracked, ...untracked])];
  }
  return files.map((f) => f.replace(/\\/g, "/"));
}

function loadCargoMetadata(root) {
  const raw = execFileSync("cargo", ["metadata", "--format-version=1"], {
    cwd: root,
    maxBuffer: 1024 * 1024 * 128,
  });
  return JSON.parse(raw.toString("utf8"));
}

/**
 * Pure decision core: given the classification of every changed file and the
 * reverse-dependency graph, decide FULL vs a specific package set.
 *
 * @returns {{
 *   full: boolean,
 *   fullReasons: Array<{file: string, id: string, reason: string}>,
 *   directCrates: string[],
 *   selectedCrates: string[],
 *   ignoredFiles: string[],
 * }}
 */
export function decideSelection(changedFiles, index, reverseGraph) {
  const fullReasons = [];
  const directCrates = new Set();
  const ignoredFiles = [];

  for (const file of changedFiles) {
    const classification = classifyChangedFile(index, file);
    switch (classification.kind) {
      case "escape-hatch":
        fullReasons.push({ file, id: classification.id, reason: classification.reason });
        break;
      case "crate":
        directCrates.add(classification.name);
        break;
      case "ignored":
        ignoredFiles.push(file);
        break;
      case "unrecognized":
        fullReasons.push({
          file,
          id: "unrecognized-path",
          reason:
            "does not map to a workspace crate or a known non-Rust path — over-select rather than guess",
        });
        break;
      default:
        throw new Error(`unreachable classification kind: ${classification.kind}`);
    }
  }

  if (fullReasons.length > 0) {
    return {
      full: true,
      fullReasons,
      directCrates: [...directCrates].sort(),
      selectedCrates: [],
      ignoredFiles,
    };
  }

  const selected = transitiveDependents(reverseGraph, directCrates);
  return {
    full: false,
    fullReasons: [],
    directCrates: [...directCrates].sort(),
    selectedCrates: [...selected].sort(),
    ignoredFiles,
  };
}

/** Build the `cargo nextest run` argv for a selected package set. */
export function buildNextestArgv(packageNames) {
  const argv = ["nextest", "run"];
  for (const name of packageNames) argv.push("-p", name);
  return argv;
}

function printTextReport(result, changedFiles) {
  process.stdout.write(`${BANNER}\n\n`);
  process.stdout.write(`Changed files (${changedFiles.length}):\n`);
  for (const f of changedFiles) process.stdout.write(`  ${f}\n`);
  process.stdout.write("\n");

  if (result.full) {
    process.stdout.write("DECISION: FULL WORKSPACE — cannot soundly narrow this change.\n\n");
    for (const r of result.fullReasons) {
      process.stdout.write(`  [${r.id}] ${r.file}\n    ${r.reason}\n`);
    }
    process.stdout.write("\nRun: cargo nextest run --workspace\n");
    process.stdout.write(
      "(or, for the exhaustive landing verdict: node scripts/gate.mjs --exhaustive — see the banner above)\n",
    );
    return;
  }

  process.stdout.write(
    `Directly changed crate(s): ${result.directCrates.join(", ") || "(none)"}\n`,
  );
  process.stdout.write(
    `Selected (changed + transitive dependents): ${result.selectedCrates.length} crate(s)\n`,
  );
  for (const c of result.selectedCrates) process.stdout.write(`  ${c}\n`);
  if (result.ignoredFiles.length > 0) {
    process.stdout.write(
      `\nIgnored (known non-Rust, no crate involved): ${result.ignoredFiles.length}\n`,
    );
  }

  if (result.selectedCrates.length === 0) {
    process.stdout.write(
      "\nDECISION: NOTHING TO TEST — every changed file is a known non-Rust path with no " +
        "owning crate. NOT recommending `cargo nextest run`: with zero `-p` filters and no " +
        "`default-members`, that command silently runs the WHOLE workspace, which would " +
        'contradict this "0 crate(s)" result. Exiting as a failed verification ' +
        `(code ${EMPTY_SELECTION_EXIT_CODE}) rather than a silent success.\n`,
    );
    return;
  }

  const argv = buildNextestArgv(result.selectedCrates);
  process.stdout.write(`\nRun: cargo ${argv.join(" ")}\n`);
}

export function main(argv, { cwd = process.cwd(), run = execFileSync } = {}) {
  if (argv.includes("--help") || argv.includes("-h")) {
    process.stdout.write(HELP);
    return 0;
  }

  const rangeIdx = argv.indexOf("--range");
  const range = rangeIdx >= 0 ? argv[rangeIdx + 1] : null;
  if (rangeIdx >= 0 && !range) {
    process.stderr.write("--range requires a value, e.g. --range HEAD~1..HEAD\n");
    return 2;
  }
  const doRun = argv.includes("--run");
  const asJson = argv.includes("--json");

  const root = repoRoot(cwd);
  const changedFiles = getChangedFiles(root, range);

  if (changedFiles.length === 0) {
    if (asJson) {
      process.stdout.write(
        JSON.stringify({ full: false, selectedCrates: [], changedFiles: [] }) + "\n",
      );
    } else {
      process.stdout.write(`${BANNER}\n\nNo changed files detected — nothing to select.\n`);
    }
    return 0;
  }

  const metadata = loadCargoMetadata(root);
  const index = buildWorkspaceIndex(metadata);
  const reverseGraph = buildReverseDependencyGraph(metadata, index);
  const result = decideSelection(changedFiles, index, reverseGraph);
  const nothingToTest = !result.full && result.selectedCrates.length === 0;

  if (asJson) {
    process.stdout.write(
      JSON.stringify({ ...result, nothingToTest, changedFiles, banner: BANNER }, null, 2) + "\n",
    );
  } else {
    printTextReport(result, changedFiles);
  }

  if (nothingToTest) {
    // Deliberately does not fall through to `--run`: there is no sound
    // `cargo nextest` invocation that means "run nothing" (zero `-p` flags
    // means "run everything"), so the only coherent behavior is to run
    // nothing and report this as a failed verification, not a silent pass.
    return EMPTY_SELECTION_EXIT_CODE;
  }

  if (doRun) {
    const packages = result.full ? [] : result.selectedCrates;
    const argv2 = result.full ? ["nextest", "run", "--workspace"] : buildNextestArgv(packages);
    process.stdout.write(`\n--run: executing cargo ${argv2.join(" ")}\n`);
    try {
      run("cargo", argv2, { cwd: root, stdio: "inherit" });
    } catch (error) {
      return typeof error.status === "number" ? error.status : 1;
    }
  }

  return 0;
}

if (import.meta.url === `file://${process.argv[1]}`) {
  process.exit(main(process.argv.slice(2)));
}

#!/usr/bin/env node
// Script-test discovery runner (SCR0).
//
// Replaces the two root `package.json` `&&` chains (`test:scripts` /
// `test:scripts:ci`) with one runner over per-owner registration fragments in
// `scripts/test-scripts.d/*.json`. A node that adds a script test adds its own
// fragment and never edits `package.json`.
//
// Semantics preserved from the replaced chains:
//   * Entries execute sequentially — fragment filename order (byte-wise),
//     then entry order — and the run STOPS at the first non-zero exit and
//     returns that exit code (`&&` semantics).
//   * `--ci` selects the entries marked `"ci": true`. Ordering: an entry with
//     an explicit `ciIndex` sorts by it; entries without one keep fragment
//     order and follow every indexed entry. The replaced CI chain ran
//     `scripts/observe-lane.test.mjs` second although it sits late in the full
//     chain, and `ciIndex` reproduces exactly that without reordering the
//     full chain. Fragments added after the conversion (no `ciIndex`) append
//     to the CI lane in fragment order, after every indexed entry.
//   * `--list` prints the resolved command lines and executes nothing.
//   * Commands run with the repository root as cwd, so resolution does not
//     depend on the invoking directory (the replaced chains ran from the
//     root via pnpm; the runner keeps that anchor explicit).
//
// Fragment format (see scripts/test-scripts.d/README.md):
//   {
//     "owner": "<task slug or area>",
//     "tests": [
//       {
//         "runner": "node-test" | "vitest" | "node" | "pnpm",
//         "file": "<repo-relative path>",              // node-test|vitest|node
//         "script": "<root package.json script name>", // pnpm
//         "ci": true | false,
//         "nodeFlags": ["--experimental-strip-types"], // optional, node-test
//         "ciIndex": 2                                 // optional, --ci order
//       }
//     ]
//   }
//   * `node-test` runs `node --test <file>`; `nodeFlags` prepend node flags
//     (the sfc-projection specs run under `--experimental-strip-types`).
//   * `vitest` runs `vitest run <file>`.
//   * `node` runs `node <file>` (a sub-runner such as REG0's
//     `scripts/run-framework-locks.mjs`).
//   * `pnpm` runs `pnpm run <script>`, validated against the root
//     package.json (the replaced chains invoked
//     `pnpm run test:generic-interface-identity-oracle` directly).
//
// Loader rejections — each is rejected for its own reason, carried on the
// thrown error's `reason` property:
//   fragments-dir-missing, malformed-json, fragment-not-object,
//   unknown-fragment-key, missing-owner, invalid-tests, entry-not-object,
//   unknown-entry-key, invalid-entry-shape, unknown-runner, missing-file,
//   invalid-file, missing-script, unknown-script, missing-ci,
//   invalid-node-flags, invalid-ci-index, duplicate-file, duplicate-script.
//
// Usage:
//   node scripts/run-script-tests.mjs [--ci] [--list]

import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(HERE, "..");
const FRAGMENTS_DIR = path.join(REPO_ROOT, "scripts", "test-scripts.d");

// Commands are dispatched through a shell (see executeScriptTests), so every
// interpolated string is validated against a shell-safe pattern first. The
// repository's test paths, node flags and script names all fit these.
const SAFE_PATH = /^[A-Za-z0-9][A-Za-z0-9._\-/]*$/;
const SAFE_FLAG = /^--?[A-Za-z0-9][A-Za-z0-9._=-]*$/;
const SAFE_SCRIPT = /^[A-Za-z0-9][A-Za-z0-9._:-]*$/;

const FILE_RUNNERS = new Set(["node-test", "vitest", "node"]);
const RUNNERS = new Set([...FILE_RUNNERS, "pnpm"]);
const FRAGMENT_KEYS = new Set(["owner", "tests"]);
const ENTRY_KEYS = new Set(["runner", "file", "script", "ci", "nodeFlags", "ciIndex"]);

export class ScriptTestFragmentError extends Error {
  constructor(reason, message) {
    super(message);
    this.name = "ScriptTestFragmentError";
    this.reason = reason;
  }
}

function fail(reason, message) {
  throw new ScriptTestFragmentError(reason, message);
}

function readRootPackageScripts() {
  const pkgPath = path.join(REPO_ROOT, "package.json");
  const pkg = JSON.parse(fs.readFileSync(pkgPath, "utf8"));
  return pkg.scripts && typeof pkg.scripts === "object" ? pkg.scripts : {};
}

function validateFile(file, fragment, i) {
  if (typeof file !== "string" || file.length === 0) {
    fail("missing-file", `${fragment} test ${i}: "file" must be a non-empty string`);
  }
  if (
    !SAFE_PATH.test(file) ||
    file.includes("/../") ||
    file.includes("/./") ||
    file.endsWith("/..")
  ) {
    fail(
      "invalid-file",
      `${fragment} test ${i}: "file" must be a plain repo-relative path: ${JSON.stringify(file)}`,
    );
  }
  if (!fs.existsSync(path.join(REPO_ROOT, file))) {
    fail("missing-file", `${fragment} test ${i}: file does not exist: ${file}`);
  }
  if (!fs.statSync(path.join(REPO_ROOT, file)).isFile()) {
    fail("invalid-file", `${fragment} test ${i}: not a file: ${file}`);
  }
}

function buildCommand(entry, fragment, i) {
  if (!RUNNERS.has(entry.runner)) {
    fail(
      "unknown-runner",
      `${fragment} test ${i}: unknown runner ${JSON.stringify(entry.runner)} (expected one of ${[...RUNNERS].join(", ")})`,
    );
  }
  if (entry.runner === "pnpm") {
    if (typeof entry.script !== "string" || !SAFE_SCRIPT.test(entry.script)) {
      fail(
        "missing-script",
        `${fragment} test ${i}: pnpm runner needs a "script" naming a root package.json script`,
      );
    }
    if (!(entry.script in readRootPackageScripts())) {
      fail("unknown-script", `${fragment} test ${i}: no root package.json script ${entry.script}`);
    }
    return ["pnpm", "run", entry.script];
  }
  if (typeof entry.file !== "string") {
    fail("missing-file", `${fragment} test ${i}: runner ${entry.runner} needs a "file"`);
  }
  validateFile(entry.file, fragment, i);
  if (entry.runner === "vitest") {
    return ["vitest", "run", entry.file];
  }
  if (entry.runner === "node") {
    return ["node", entry.file];
  }
  const flags = entry.nodeFlags ?? [];
  return ["node", ...flags, "--test", entry.file];
}

function validateEntry(entry, fragment, i) {
  if (entry === null || typeof entry !== "object" || Array.isArray(entry)) {
    fail("entry-not-object", `${fragment} test ${i}: each test must be an object`);
  }
  for (const key of Object.keys(entry)) {
    if (!ENTRY_KEYS.has(key)) {
      fail("unknown-entry-key", `${fragment} test ${i}: unknown test key "${key}"`);
    }
  }
  if (typeof entry.runner !== "string") {
    fail("unknown-runner", `${fragment} test ${i}: "runner" must be a string`);
  }
  if (typeof entry.ci !== "boolean") {
    fail("missing-ci", `${fragment} test ${i}: "ci" must be true or false`);
  }
  if (entry.file !== undefined && entry.script !== undefined) {
    fail(
      "invalid-entry-shape",
      `${fragment} test ${i}: an entry registers either "file" or "script", not both`,
    );
  }
  if (entry.nodeFlags !== undefined) {
    if (entry.runner !== "node-test" || !Array.isArray(entry.nodeFlags)) {
      fail(
        "invalid-node-flags",
        `${fragment} test ${i}: "nodeFlags" is a node-test-only array of node flags`,
      );
    }
    for (const flag of entry.nodeFlags) {
      if (typeof flag !== "string" || !SAFE_FLAG.test(flag)) {
        fail(
          "invalid-node-flags",
          `${fragment} test ${i}: unsafe node flag ${JSON.stringify(flag)}`,
        );
      }
    }
  }
  if (entry.ciIndex !== undefined && (!Number.isInteger(entry.ciIndex) || entry.ciIndex < 1)) {
    fail("invalid-ci-index", `${fragment} test ${i}: "ciIndex" must be a positive integer`);
  }
  if (entry.ciIndex !== undefined && entry.ci !== true) {
    fail(
      "invalid-entry-shape",
      `${fragment} test ${i}: "ciIndex" orders a ci entry; drop it or set "ci": true`,
    );
  }
  return {
    owner: null,
    fragment,
    index: i,
    runner: entry.runner,
    file: entry.file ?? null,
    script: entry.script ?? null,
    ci: entry.ci,
    ciIndex: entry.ciIndex ?? null,
    command: null,
  };
}

function loadFragment(fragmentPath, fragment) {
  let parsed;
  try {
    parsed = JSON.parse(fs.readFileSync(fragmentPath, "utf8"));
  } catch (error) {
    fail("malformed-json", `${fragment}: ${error.message}`);
  }
  if (parsed === null || typeof parsed !== "object" || Array.isArray(parsed)) {
    fail("fragment-not-object", `${fragment}: fragment must be a JSON object`);
  }
  for (const key of Object.keys(parsed)) {
    if (!FRAGMENT_KEYS.has(key)) {
      fail("unknown-fragment-key", `${fragment}: unknown fragment key "${key}"`);
    }
  }
  if (typeof parsed.owner !== "string" || parsed.owner.length === 0) {
    fail("missing-owner", `${fragment}: fragment needs a non-empty "owner"`);
  }
  if (!Array.isArray(parsed.tests)) {
    fail("invalid-tests", `${fragment}: "tests" must be an array`);
  }
  return parsed.tests.map((entry, i) => {
    const resolved = validateEntry(entry, fragment, i);
    resolved.owner = parsed.owner;
    resolved.command = buildCommand(entry, fragment, i);
    resolved.display = resolved.command.join(" ");
    return resolved;
  });
}

// Resolve the registered script tests. `ci` selects the CI subset; a custom
// `fragmentsDir` (used by the self-test's fixture corpora) overrides the
// registry directory. Returns the entries in execution order, each carrying
// `command` (argv array), `display` (the command line), `owner`, `fragment`
// and `index`.
export function resolveScriptTests({ ci = false, fragmentsDir = FRAGMENTS_DIR } = {}) {
  let names;
  try {
    names = fs.readdirSync(fragmentsDir);
  } catch (error) {
    fail(
      "fragments-dir-missing",
      `cannot read fragments directory ${fragmentsDir}: ${error.message}`,
    );
  }
  names = names.filter((name) => name.endsWith(".json"));
  names.sort((a, b) => Buffer.compare(Buffer.from(a, "utf8"), Buffer.from(b, "utf8")));

  const entries = [];
  const registered = new Map();
  for (const name of names) {
    for (const entry of loadFragment(path.join(fragmentsDir, name), name)) {
      const isPnpm = entry.runner === "pnpm";
      const key = isPnpm ? `script:${entry.script}` : `file:${entry.file}`;
      const seen = registered.get(key);
      if (seen) {
        fail(
          isPnpm ? "duplicate-script" : "duplicate-file",
          `${entry.fragment} test ${entry.index}: ${isPnpm ? `script ${entry.script}` : entry.file} is already registered by ${seen}`,
        );
      }
      registered.set(key, `${entry.fragment} test ${entry.index}`);
      entries.push(entry);
    }
  }

  if (!ci) {
    return entries;
  }
  // Indexed entries sort by ciIndex; unindexed entries keep fragment order
  // and follow every indexed entry (see the ordering note in the header).
  // The comparator's final 0 relies on sort stability for equal keys.
  return entries
    .filter((entry) => entry.ci)
    .sort(
      (a, b) =>
        Number(b.ciIndex !== null) - Number(a.ciIndex !== null) ||
        (a.ciIndex ?? 0) - (b.ciIndex ?? 0) ||
        0,
    );
}

// Execute resolved entries sequentially, stopping at the first non-zero exit.
// Returns the propagating exit code (0 when every entry passes).
export function executeScriptTests(entries, { cwd = REPO_ROOT } = {}) {
  for (const entry of entries) {
    // `shell: true` with a single command string — on Windows `vitest` and
    // `pnpm` resolve to `.cmd` shims that spawn cannot launch without a shell
    // (same rationale as scripts/build-host.mjs); POSIX dispatches through
    // /bin/sh -c exactly like the replaced `&&` chains did. Every interpolated
    // component of `display` is validated shell-safe at load time
    // (SAFE_PATH / SAFE_FLAG / SAFE_SCRIPT; the binaries are literals).
    const result = spawnSync(entry.display, { stdio: "inherit", cwd, shell: true });
    if (result.error) {
      throw result.error;
    }
    if (result.status !== 0) {
      return result.status ?? 1;
    }
  }
  return 0;
}

export function main(argv) {
  let ci = false;
  let list = false;
  for (const arg of argv) {
    if (arg === "--ci") ci = true;
    else if (arg === "--list") list = true;
    else {
      process.stderr.write(`run-script-tests: unknown argument '${arg}'\n`);
      process.stderr.write("usage: node scripts/run-script-tests.mjs [--ci] [--list]\n");
      return 2;
    }
  }
  let entries;
  try {
    entries = resolveScriptTests({ ci });
  } catch (error) {
    if (error instanceof ScriptTestFragmentError) {
      process.stderr.write(`run-script-tests: ${error.reason}: ${error.message}\n`);
      return 2;
    }
    throw error;
  }
  if (list) {
    for (const entry of entries) {
      process.stdout.write(`${entry.display}\n`);
    }
    return 0;
  }
  return executeScriptTests(entries);
}

if (
  process.argv[1] !== undefined &&
  path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  process.exitCode = main(process.argv.slice(2));
}

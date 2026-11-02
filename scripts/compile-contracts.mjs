#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import { dirname, resolve } from "node:path";
import { performance } from "node:perf_hooks";
import { fileURLToPath } from "node:url";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const OBSERVE_MANIFEST = "crates/verter_compile_contracts_session_observe/Cargo.toml";

// A group runs serially against one shared outer/trybuild target directory.
// CI gives each group its own runner and cache: concurrent Cargo/trybuild
// processes must never rewrite the same generated fixture workspace.
export const CONTRACT_GROUPS = Object.freeze({
  core: Object.freeze([
    "audit",
    "css-syntax",
    "formatter",
    "identity",
    "language",
    "semantic",
    "session-query",
    "type-runtime",
    "workspace",
    "session-feature-variants",
  ]),
  compiler: Object.freeze(["compiler-default", "compiler-bench", "session-soak"]),
  session: Object.freeze(["session"]),
  "session-observe": Object.freeze(["session-observe"]),
});

const SPECIAL_PACKAGES = Object.freeze({
  "compiler-bench": "verter_compile_contracts_bench",
  "session-feature-variants": "verter_compile_contracts_session_variants",
  "session-soak": "verter_compile_contracts_session_soak",
});
const OWNERS = Object.values(CONTRACT_GROUPS)
  .flat()
  .filter((name) => !(name in SPECIAL_PACKAGES) && name !== "session-observe")
  .sort();

export function compileContractPlan(group) {
  if (group !== undefined && !Object.hasOwn(CONTRACT_GROUPS, group)) {
    throw new Error(`unknown compile-contract group: ${group}`);
  }
  const names =
    group === undefined ? Object.values(CONTRACT_GROUPS).flat() : CONTRACT_GROUPS[group];
  return names.map((name) => ({
    name,
    args:
      name === "session-observe"
        ? ["run", "--locked", "--manifest-path", OBSERVE_MANIFEST]
        : name in SPECIAL_PACKAGES
          ? ["run", "--locked", "-p", SPECIAL_PACKAGES[name]]
          : [
              "run",
              "--locked",
              "-p",
              "verter_compile_contracts",
              "--no-default-features",
              "--features",
              name,
            ],
  }));
}

export function runCompileContracts({
  group,
  env = process.env,
  run = spawnSync,
  log = (message) => process.stderr.write(message),
} = {}) {
  const tasks = compileContractPlan(group);
  const groupStart = performance.now();
  const options = {
    cwd: REPO_ROOT,
    stdio: "inherit",
    windowsHide: true,
    // The excluded observe crate otherwise gets its own crate-local target.
    // Resolve relative overrides before its runner changes cwd as well.
    env: { ...env, CARGO_TARGET_DIR: resolve(REPO_ROOT, env.CARGO_TARGET_DIR || "target") },
  };
  const fetches = [{ name: "fetch workspace", args: ["fetch", "--locked"] }];
  if (tasks.some(({ name }) => name === "session-observe")) {
    // Its semantic-observe edge must stay outside workspace feature unification;
    // trybuild goes offline using this runner's separate committed lockfile.
    fetches.push({
      name: "fetch session-observe",
      args: ["fetch", "--locked", "--manifest-path", OBSERVE_MANIFEST],
    });
  }
  function execute({ name, args }) {
    log(`\nCOMPILE CONTRACTS: ${name}\n`);
    const start = performance.now();
    const result = run("cargo", args, options);
    const passed = !result.error && !result.signal && result.status === 0;
    if (result.error) log(`COMPILE CONTRACTS: could not start cargo: ${result.error.message}\n`);
    if (result.signal) log(`COMPILE CONTRACTS: cargo was killed by ${result.signal}\n`);
    log(
      `COMPILE CONTRACT TIME: ${name}: ${((performance.now() - start) / 1000).toFixed(2)}s (${passed ? "PASS" : "FAIL"})\n`,
    );
    return passed;
  }
  function finish(failed) {
    log(
      `COMPILE CONTRACT GROUP TIME: ${group ?? "all"}: ${((performance.now() - groupStart) / 1000).toFixed(2)}s (${failed ? "FAIL" : "PASS"})\n`,
    );
    return failed ? 1 : 0;
  }
  for (const fetch of fetches) if (!execute(fetch)) return finish(true);
  let failed = false;
  // Complete diagnostics: a failed configuration never suppresses another.
  for (const task of tasks) if (!execute(task)) failed = true;
  return finish(failed);
}

function main(argv) {
  if (argv.length === 1 && argv[0] === "--list-owners") {
    process.stdout.write(`${OWNERS.join("\n")}\n`);
    return 0;
  }
  if (argv.length === 1 && argv[0] === "--list-groups") {
    process.stdout.write(`${JSON.stringify(Object.keys(CONTRACT_GROUPS))}\n`);
    return 0;
  }
  if (argv.length !== 0 && !(argv.length === 2 && argv[0] === "--group")) {
    throw new Error(
      "usage: compile-contracts.mjs [--group <name> | --list-groups | --list-owners]",
    );
  }
  return runCompileContracts({ group: argv[1] });
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    process.exitCode = main(process.argv.slice(2));
  } catch (error) {
    process.stderr.write(`COMPILE CONTRACTS: ${error.message}\n`);
    process.exitCode = 1;
  }
}

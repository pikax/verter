#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");

const OWNERS = Object.freeze([
  "audit",
  "compiler-default",
  "css-syntax",
  "identity",
  "language",
  "semantic",
  "session",
  "session-query",
  "type-runtime",
  "workspace",
]);

// The owners this lane covers are declared here and nowhere else, so the list
// is printable. A checker asking whether this lane still re-runs a given
// owner's contracts asks the script, rather than restating its contents
// somewhere the two can drift apart without either side noticing.
if (process.argv.includes("--list-owners")) {
  process.stdout.write(`${OWNERS.join("\n")}\n`);
  process.exit(0);
}

process.stderr.write("\nCOMPILE CONTRACTS: fetch locked workspace\n");
const fetches = [
  ["fetch", "--locked"],
  // The session-observe runner's own lock: the crate is outside the
  // workspace (its `semantic-observe` edge must not unify into `--workspace`
  // builds), so the workspace fetch above does not cover its lockfile, and
  // trybuild builds the fixture project offline against it.
  [
    "fetch",
    "--locked",
    "--manifest-path",
    "crates/verter_compile_contracts_session_observe/Cargo.toml",
  ],
];
for (const args of fetches) {
  const fetch = spawnSync("cargo", args, {
    stdio: "inherit",
    windowsHide: true,
  });
  if (fetch.error) {
    process.stderr.write(`COMPILE CONTRACTS: could not start cargo: ${fetch.error.message}\n`);
    process.exit(1);
  }
  if (fetch.signal) {
    process.stderr.write(`COMPILE CONTRACTS: cargo was killed by ${fetch.signal}\n`);
    process.exit(1);
  }
  if (fetch.status !== 0) {
    process.exit(fetch.status ?? 1);
  }
}

let failed = false;
for (const owner of OWNERS) {
  process.stderr.write(`\nCOMPILE CONTRACTS: ${owner}\n`);
  const result = spawnSync(
    "cargo",
    [
      "run",
      "--locked",
      "-p",
      "verter_compile_contracts",
      "--no-default-features",
      "--features",
      owner,
    ],
    { stdio: "inherit", windowsHide: true },
  );
  if (result.error) {
    process.stderr.write(`COMPILE CONTRACTS: could not start cargo: ${result.error.message}\n`);
    failed = true;
  } else if (result.signal) {
    process.stderr.write(`COMPILE CONTRACTS: cargo was killed by ${result.signal}\n`);
    failed = true;
  } else if (result.status !== 0) {
    failed = true;
  }
}

process.stderr.write("\nCOMPILE CONTRACTS: compiler-bench\n");
const bench = spawnSync("cargo", ["run", "--locked", "-p", "verter_compile_contracts_bench"], {
  stdio: "inherit",
  windowsHide: true,
});
if (bench.error) {
  process.stderr.write(`COMPILE CONTRACTS: could not start cargo: ${bench.error.message}\n`);
  failed = true;
} else if (bench.signal) {
  process.stderr.write(`COMPILE CONTRACTS: cargo was killed by ${bench.signal}\n`);
  failed = true;
} else if (bench.status !== 0) {
  failed = true;
}

process.stderr.write("\nCOMPILE CONTRACTS: session-feature-variants\n");
const sessionVariants = spawnSync(
  "cargo",
  ["run", "--locked", "-p", "verter_compile_contracts_session_variants"],
  { stdio: "inherit", windowsHide: true },
);
if (sessionVariants.error) {
  process.stderr.write(
    `COMPILE CONTRACTS: could not start cargo: ${sessionVariants.error.message}\n`,
  );
  failed = true;
} else if (sessionVariants.signal) {
  process.stderr.write(`COMPILE CONTRACTS: cargo was killed by ${sessionVariants.signal}\n`);
  failed = true;
} else if (sessionVariants.status !== 0) {
  failed = true;
}

process.stderr.write("\nCOMPILE CONTRACTS: session-soak\n");
const sessionSoak = spawnSync(
  "cargo",
  ["run", "--locked", "-p", "verter_compile_contracts_session_soak"],
  { stdio: "inherit", windowsHide: true },
);
if (sessionSoak.error) {
  process.stderr.write(`COMPILE CONTRACTS: could not start cargo: ${sessionSoak.error.message}\n`);
  failed = true;
} else if (sessionSoak.signal) {
  process.stderr.write(`COMPILE CONTRACTS: cargo was killed by ${sessionSoak.signal}\n`);
  failed = true;
} else if (sessionSoak.status !== 0) {
  failed = true;
}

process.stderr.write("\nCOMPILE CONTRACTS: session-observe\n");
// The observe runner is NOT a workspace member (see its manifest): running
// it by manifest path keeps its `semantic-observe` dependency edge out of
// `--workspace` feature unification, so the core gate keeps testing the
// capture-off configuration. An explicit target dir (defaulting to the
// repository's) shares the already-built dependency closure instead of
// compiling it again in a crate-local target directory.
const sessionObserve = spawnSync(
  "cargo",
  [
    "run",
    "--locked",
    "--manifest-path",
    "crates/verter_compile_contracts_session_observe/Cargo.toml",
  ],
  {
    stdio: "inherit",
    windowsHide: true,
    env: {
      ...process.env,
      CARGO_TARGET_DIR: process.env.CARGO_TARGET_DIR ?? resolve(REPO_ROOT, "target"),
    },
  },
);
if (sessionObserve.error) {
  process.stderr.write(
    `COMPILE CONTRACTS: could not start cargo: ${sessionObserve.error.message}\n`,
  );
  failed = true;
} else if (sessionObserve.signal) {
  process.stderr.write(`COMPILE CONTRACTS: cargo was killed by ${sessionObserve.signal}\n`);
  failed = true;
} else if (sessionObserve.status !== 0) {
  failed = true;
}

process.exitCode = failed ? 1 : 0;

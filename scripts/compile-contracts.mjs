#!/usr/bin/env node

import { spawnSync } from "node:child_process";

const OWNERS = Object.freeze([
  "audit",
  "compiler-default",
  "css-syntax",
  "identity",
  "language",
  "semantic",
  "session",
  "type-runtime",
  "workspace",
]);

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

process.exitCode = failed ? 1 : 0;

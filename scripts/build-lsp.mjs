#!/usr/bin/env node
// Builds the verter_lsp binary under an explicit host --target, matching
// NAPI's target layout instead of Cargo's implicit host target. Backs the
// standalone `build:lsp` /
// `build:lsp:release` scripts (used directly by `dev-extension` and
// contributors doing a quick LSP-only rebuild) — `pnpm build`'s combined
// NAPI+LSP invocation goes through `scripts/build-host.mjs` instead.
//
// Usage: node scripts/build-lsp.mjs [--release] [--profile <name>]

import { spawnSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { resolveHostTarget } from "./host-target.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(HERE, "..");

function parseArgs(argv) {
  let profile;
  let release = false;
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === "--release") {
      release = true;
    } else if (argv[i] === "--profile") {
      profile = argv[i + 1];
      i++;
    }
  }
  return { profile, release };
}

const { profile, release } = parseArgs(process.argv.slice(2));
const target = resolveHostTarget();

const args = ["build", "-p", "verter_lsp", "--target", target];
if (profile) {
  args.push("--profile", profile);
} else if (release) {
  args.push("--release");
}

const result = spawnSync("cargo", args, { stdio: "inherit", cwd: REPO_ROOT });
if (result.error) {
  throw result.error;
}
process.exit(result.status ?? 1);

#!/usr/bin/env node
// ONE-TIME, OFFLINE-AFTERWARDS provisioning of the two pinned official GIT
// SOURCE checkouts this harness's manifest/source-drift self-tests need.
//
// This script is the ONLY part of the harness that touches the network, it is
// NEVER invoked from a test, and after it has run once the affected suites run
// fully offline against the local checkouts. It is the reproducible replacement
// for "a contributor happens to have a clone lying around": it materializes the
// exact commits recorded in src/domain-pin.mjs (which mirrors
// packages/framework-conformance-harness/evidence/version-domain.md) and
// then verifies them with the harness's own drift-refusal module, so a wrong or
// tampered fetch fails closed here rather than silently weakening a self-test.
//
//   node scripts/provision-oracle-checkouts.mjs
//   eval "$(node scripts/provision-oracle-checkouts.mjs --print-env)"
//
// The checkouts land under .oracle-checkouts/ inside this package (gitignored)
// unless BF2_ORACLE_CACHE points elsewhere. Single-commit fetches
// (`git fetch --depth 1 <sha>`) are used so the working tree is complete for the
// pinned commit without downloading upstream history.

import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync } from "node:fs";
import path from "node:path";

import { VUE_DOMAIN, SVELTE_DOMAIN } from "../src/domain-pin.mjs";
import { assertCheckoutPinned } from "../src/checkout-pin.mjs";
import { HARNESS_ROOT } from "../src/paths.mjs";

export const ORACLE_CACHE_ROOT = process.env.BF2_ORACLE_CACHE
  ? path.resolve(process.env.BF2_ORACLE_CACHE)
  : path.join(HARNESS_ROOT, ".oracle-checkouts");

export function checkoutPathFor(domain) {
  return path.join(ORACLE_CACHE_ROOT, domain.framework);
}

function run(cwd, ...args) {
  return execFileSync("git", ["-C", cwd, ...args], {
    encoding: "utf8",
    stdio: ["ignore", "pipe", "inherit"],
  }).trim();
}

function provision(domain, { quiet = false } = {}) {
  const dest = checkoutPathFor(domain);
  const log = (message) => {
    if (!quiet) process.stderr.write(`${message}\n`);
  };
  if (existsSync(path.join(dest, ".git"))) {
    try {
      assertCheckoutPinned(dest, domain);
      log(`${domain.framework}: already pinned at ${domain.commit}`);
      return dest;
    } catch (error) {
      log(`${domain.framework}: existing checkout not pinned (${error.message}); re-fetching`);
    }
  }
  mkdirSync(dest, { recursive: true });
  if (!existsSync(path.join(dest, ".git"))) {
    execFileSync("git", ["init", "-q", dest], { stdio: ["ignore", "pipe", "inherit"] });
  }
  const remotes = run(dest, "remote");
  if (!remotes.split("\n").includes("origin")) {
    run(dest, "remote", "add", "origin", domain.upstream);
  } else {
    run(dest, "remote", "set-url", "origin", domain.upstream);
  }
  log(`${domain.framework}: fetching ${domain.commit} from ${domain.upstream} (depth 1)`);
  run(dest, "fetch", "--depth", "1", "--no-tags", "origin", domain.commit);
  run(dest, "checkout", "-q", "--detach", domain.commit);
  // Fail closed: the harness's own pin assertion is the acceptance oracle.
  const identity = assertCheckoutPinned(dest, domain);
  log(`${domain.framework}: pinned commit ${identity.commit} tree ${identity.tree}`);
  return dest;
}

/** Provisions both domains and returns the env the harness's suites read. */
export function provisionAll(options = {}) {
  return {
    BF2_VUE_SOURCE: provision(VUE_DOMAIN, options),
    BF2_SVELTE_SOURCE: provision(SVELTE_DOMAIN, options),
  };
}

const invokedDirectly =
  process.argv[1] &&
  path.resolve(process.argv[1]) === path.resolve(new URL(import.meta.url).pathname);
if (invokedDirectly) {
  const printEnv = process.argv.includes("--print-env");
  const env = provisionAll({ quiet: printEnv });
  for (const [key, value] of Object.entries(env)) {
    if (printEnv) process.stdout.write(`export ${key}=${JSON.stringify(value)}\n`);
    else process.stdout.write(`${key}=${value}\n`);
  }
}

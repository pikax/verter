#!/usr/bin/env node
// ONE-TIME, OFFLINE-AFTERWARDS provisioning of a local npm cache holding
// exactly the tarballs the committed oracle lockfiles
// (packages/framework-conformance-harness/evidence/oracles/{vue,svelte}/package-lock.json)
// resolve. Like scripts/provision-oracle-checkouts.mjs, this is the ONLY
// network-touching step for its consumers and is NEVER invoked from a test:
// once it has run, the disposable-install closure self-test
// (test/closure-drift.spec.mjs) performs its `npm ci --offline
// --ignore-scripts` installs entirely from this cache, network-denied.
//
// The cache lands at <package>/.oracle-npm-cache (gitignored) unless
// BF2_ORACLE_NPM_CACHE points elsewhere.

import { execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

import { HARNESS_ROOT, VUE_EVIDENCE_LOCK, SVELTE_EVIDENCE_LOCK } from "../src/paths.mjs";

export const ORACLE_NPM_CACHE_ROOT = process.env.BF2_ORACLE_NPM_CACHE
  ? path.resolve(process.env.BF2_ORACLE_NPM_CACHE)
  : path.join(HARNESS_ROOT, ".oracle-npm-cache");

const NPM = process.platform === "win32" ? "npm.cmd" : "npm";

function warm(lockPath) {
  const oracleDir = path.dirname(lockPath);
  const scratch = mkdtempSync(path.join(tmpdir(), "bf2-npm-cache-warm-"));
  try {
    copyFileSync(path.join(oracleDir, "package.json"), path.join(scratch, "package.json"));
    copyFileSync(lockPath, path.join(scratch, "package-lock.json"));
    execFileSync(
      NPM,
      ["ci", "--ignore-scripts", "--no-audit", "--no-fund", "--cache", ORACLE_NPM_CACHE_ROOT],
      { cwd: scratch, stdio: "inherit" },
    );
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
}

mkdirSync(ORACLE_NPM_CACHE_ROOT, { recursive: true });
warm(VUE_EVIDENCE_LOCK);
warm(SVELTE_EVIDENCE_LOCK);
process.stderr.write(`oracle npm cache warmed at ${ORACLE_NPM_CACHE_ROOT}\n`);

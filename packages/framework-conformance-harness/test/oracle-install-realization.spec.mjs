// Self-test: oracle-install REALIZATION discipline — cross-process
// exclusion on the realize-then-swap sequence, and offline-only fail-closed
// behavior when the provisioned npm cache is absent.
//
// Three properties, each proven against the real production entry point
// (`ensureOracleDomain`) in child processes (the install/cache roots are
// bound at module load from BF2_ORACLE_INSTALLS / BF2_ORACLE_NPM_CACHE, so
// each scenario gets its own process with its own roots):
//
//  1. DETERMINISTIC EXCLUSION: with the realization lock directory HELD by
//     the test itself, a spawned production realizer makes NO progress (no
//     exit, no stage directory, no final tree, no digest) for a window far
//     beyond a full realization's duration, and completes a validated
//     realization only after the lock is released. This is the regression
//     discriminator for the exclusion mechanism — it fails on every run if
//     the mkdir test-and-set is bypassed, with no dependence on race
//     timing.
//
//  2. CONCURRENCY CONVERGENCE: two processes realizing the SAME domain into
//     the SAME (initially empty) installs root race the exclusive mkdir
//     lock; exactly one installs, the other waits and adopts the winner's
//     validated tree. Both succeed, both report the identical realized
//     closure digest, the final tree independently validates against the
//     committed lock, and no lock/stage residue is left behind. (A genuine
//     happy-path convergence check; the deterministic test above, not this
//     race, is what kills an exclusion-removal regression.)
//
//  3. OFFLINE FAIL-CLOSED: with NO provisioned cache, realization REFUSES
//     with the actionable OracleCacheUnprovisionedError BEFORE any lock,
//     stage, or npm work — never a silent networked `npm ci` fallback.

import { describe, expect, it, afterAll } from "vitest";
import { spawn, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readdirSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

import {
  compareRealizedToLock,
  enumerateInstalledClosure,
  enumerateLockClosure,
} from "../src/closure-verify.mjs";
import { realizedClosureDigest } from "../src/oracle-install.mjs";
import { HARNESS_ROOT, SVELTE_EVIDENCE_LOCK } from "../src/paths.mjs";

const ORACLE_NPM_CACHE = process.env.BF2_ORACLE_NPM_CACHE
  ? path.resolve(process.env.BF2_ORACLE_NPM_CACHE)
  : path.join(HARNESS_ROOT, ".oracle-npm-cache");

const scratchDirs = [];
function scratchDir(prefix) {
  const dir = mkdtempSync(path.join(tmpdir(), prefix));
  scratchDirs.push(dir);
  return dir;
}
afterAll(() => {
  for (const dir of scratchDirs) rmSync(dir, { recursive: true, force: true });
});

/** Child script: realize one domain and print its realized closure digest. */
const REALIZE_SCRIPT = `
  const { ensureOracleDomain } = await import(${JSON.stringify(
    path.join(HARNESS_ROOT, "src/oracle-install.mjs"),
  )});
  const result = ensureOracleDomain(process.argv[1]);
  console.log("REALIZED_DIGEST", result.realizedClosureSha256);
`;

function spawnRealizer(framework, env) {
  return new Promise((resolvePromise) => {
    const child = spawn(
      process.execPath,
      ["--input-type=module", "-e", REALIZE_SCRIPT, framework],
      { cwd: HARNESS_ROOT, env: { ...process.env, ...env }, stdio: ["ignore", "pipe", "pipe"] },
    );
    let stdout = "";
    let stderr = "";
    child.stdout.on("data", (chunk) => {
      stdout += chunk;
    });
    child.stderr.on("data", (chunk) => {
      stderr += chunk;
    });
    child.on("close", (code) => resolvePromise({ code, stdout, stderr }));
  });
}

describe("offline fail-closed realization (no provisioned cache)", () => {
  it("refuses realization with an actionable error instead of a networked npm ci", () => {
    const installsRoot = scratchDir("bf2-installs-nocache-");
    const missingCache = path.join(scratchDir("bf2-cache-missing-"), "does-not-exist");
    expect(existsSync(missingCache)).toBe(false); // the no-cache precondition is real
    const result = spawnSync(
      process.execPath,
      ["--input-type=module", "-e", REALIZE_SCRIPT, "vue"],
      {
        encoding: "utf8",
        cwd: HARNESS_ROOT,
        env: {
          ...process.env,
          BF2_ORACLE_INSTALLS: installsRoot,
          BF2_ORACLE_NPM_CACHE: missingCache,
        },
      },
    );
    expect(result.status).not.toBe(0);
    expect(result.stderr).toContain("OracleCacheUnprovisionedError");
    expect(result.stderr).toContain("provision-oracle-npm-cache.mjs");
    expect(result.stderr).toContain("never falls back to a networked install");
    expect(result.stdout).not.toContain("REALIZED_DIGEST"); // no realization happened
    // The refusal fired BEFORE any lock or stage work: nothing was created
    // under the installs root, proving npm was never even attempted.
    expect(readdirSync(installsRoot)).toEqual([]);
  });
});

describe("cross-process realization exclusion", () => {
  const cacheReady = existsSync(ORACLE_NPM_CACHE);
  const runIf = cacheReady ? it : it.skip; // provision via scripts/provision-oracle-npm-cache.mjs — never silently passed

  runIf(
    "a HELD realization lock blocks a realizer deterministically, and releasing it lets the realizer complete",
    async () => {
      // The DETERMINISTIC exclusion discriminator — no timing luck. The
      // two-racer convergence test below is a genuine happy-path check, but
      // as a regression discriminator it is probabilistic: with the mkdir
      // test-and-set removed the race is not always fast enough to expose
      // the missing exclusion, and it has been measured passing green in
      // 1-in-5 runs with the mechanism deleted outright. This test replaces
      // luck with a schedule: the test itself HOLDS the lock (creating the
      // exact directory acquireRealizeLock's mkdir test-and-set creates),
      // spawns one real production realizer, and asserts it makes NO
      // progress while the lock is held — a realizer that ignores the held
      // lock finishes and/or creates its stage directory within that
      // window regardless of machine speed, so removal of the exclusion
      // fails this test on every run, not most runs.
      const installsRoot = scratchDir("bf2-installs-lockheld-");
      const lockPath = path.join(installsRoot, "svelte.lock");
      mkdirSync(lockPath); // hold the lock, exactly as acquireRealizeLock creates it
      expect(existsSync(lockPath)).toBe(true);

      let finished = false;
      let stdout = "";
      let stderr = "";
      let exitCode = null;
      const child = spawn(
        process.execPath,
        ["--input-type=module", "-e", REALIZE_SCRIPT, "svelte"],
        {
          cwd: HARNESS_ROOT,
          env: { ...process.env, BF2_ORACLE_INSTALLS: installsRoot },
          stdio: ["ignore", "pipe", "pipe"],
        },
      );
      child.stdout.on("data", (chunk) => {
        stdout += chunk;
      });
      child.stderr.on("data", (chunk) => {
        stderr += chunk;
      });
      const done = new Promise((resolvePromise) =>
        child.on("close", (code) => {
          finished = true;
          exitCode = code;
          resolvePromise();
        }),
      );

      // An unmutated realization of this domain completes in well under a
      // second once it holds the lock; a realizer that bypassed the held
      // lock creates its stage directory within milliseconds of starting.
      // Ten seconds is therefore pure margin, not a tuned race window.
      await new Promise((resolvePromise) => setTimeout(resolvePromise, 10_000));

      // While the lock is HELD: no completion, no stage, no final tree, no
      // digest — the realizer is genuinely blocked, not merely slow.
      const stageDirsWhileHeld = readdirSync(installsRoot).filter((entry) =>
        entry.startsWith(".stage-"),
      );
      expect(finished).toBe(false);
      expect(stageDirsWhileHeld).toEqual([]);
      expect(existsSync(path.join(installsRoot, "svelte"))).toBe(false);
      expect(stdout).not.toContain("REALIZED_DIGEST");

      // Release the lock: the SAME blocked realizer must now complete a
      // full validated realization and leave no lock/stage residue.
      rmSync(lockPath, { recursive: true, force: true });
      await done;
      expect(stderr).toBe("");
      expect(exitCode).toBe(0);
      const digest = stdout.match(/REALIZED_DIGEST ([0-9a-f]{64})/)?.[1];
      expect(digest).toMatch(/^[0-9a-f]{64}$/);
      const realized = enumerateInstalledClosure(path.join(installsRoot, "svelte"));
      const comparison = compareRealizedToLock(
        realized,
        enumerateLockClosure(SVELTE_EVIDENCE_LOCK),
      );
      expect(comparison.problems).toEqual([]);
      expect(comparison.ok).toBe(true);
      expect(realizedClosureDigest(realized)).toBe(digest);
      const residue = readdirSync(installsRoot).filter(
        (entry) => entry.endsWith(".lock") || entry.startsWith(".stage-"),
      );
      expect(residue).toEqual([]);
    },
    240_000,
  );

  runIf(
    "two concurrent realizations of the same domain converge on ONE validated tree",
    async () => {
      const installsRoot = scratchDir("bf2-installs-race-");
      const env = { BF2_ORACLE_INSTALLS: installsRoot };
      const [a, b] = await Promise.all([
        spawnRealizer("svelte", env),
        spawnRealizer("svelte", env),
      ]);
      // Both racers succeed — neither corrupted, neither observed a
      // half-written tree (a torn observation fails the realized-closure
      // validation inside ensureOracleDomain and exits non-zero).
      expect(a.stderr).toBe("");
      expect(b.stderr).toBe("");
      expect(a.code).toBe(0);
      expect(b.code).toBe(0);
      const digestOf = (out) => out.stdout.match(/REALIZED_DIGEST ([0-9a-f]{64})/)?.[1];
      const digestA = digestOf(a);
      const digestB = digestOf(b);
      expect(digestA).toMatch(/^[0-9a-f]{64}$/);
      expect(digestA).toBe(digestB); // both converged on the identical closure
      // The final installed tree independently validates against the
      // committed lock, and the parent's own enumeration reproduces the
      // digest both children reported.
      const installDir = path.join(installsRoot, "svelte");
      const realized = enumerateInstalledClosure(installDir);
      const comparison = compareRealizedToLock(
        realized,
        enumerateLockClosure(SVELTE_EVIDENCE_LOCK),
      );
      expect(comparison.problems).toEqual([]);
      expect(comparison.ok).toBe(true);
      expect(realizedClosureDigest(realized)).toBe(digestA);
      // No lock or stage residue left behind.
      const leftovers = readdirSync(installsRoot).filter(
        (entry) => entry.endsWith(".lock") || entry.startsWith(".stage-"),
      );
      expect(leftovers).toEqual([]);
    },
    240_000,
  );
});

// Self-test: TRANSITIVE package-closure drift refusal (BF2 required exits
// FC-HARNESS-001 + "source/package drift refusal").
//
// Three layers of proof, each with a planted mutation shown to be caught:
//
//  1. STATIC cross-derivation — the full closure (every nested package's
//     path/name/version/integrity/resolution/edges) independently
//     re-derived from the committed lockfile must equal the committed
//     closure.tsv, whose own bytes must match the recorded digest. A
//     hand-edited NESTED lock entry (a transitive dependency's version or
//     integrity — no direct package touched) is refused, and because this
//     runs inside assertPackagesPinned — the first statement of every
//     oracle invocation — refusal happens BEFORE any compiler invocation.
//  2. END-TO-END ordering — a child process pointed (BF2_EVIDENCE_ROOT) at
//     an evidence tree with a transitively-mutated lockfile fails inside
//     compileVueFixture's pin gate and produces NO compilation artifact.
//  3. REALIZED closure — the committed lockfile is installed into a
//     disposable, scripts-disabled, network-denied store; the installed
//     tree is independently enumerated (real manifests at real nested
//     paths, edges re-resolved through the physical tree) and compared
//     against the committed closure; a tampered installed package at any
//     depth is caught by the enumeration.

import { describe, expect, it, afterAll } from "vitest";
import { execFileSync, spawnSync } from "node:child_process";
import {
  copyFileSync,
  cpSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { createHash } from "node:crypto";
import { tmpdir } from "node:os";
import path from "node:path";
import { pathToFileURL } from "node:url";

import {
  assertClosurePinned,
  assertPackagesPinned,
  PackageDriftError,
} from "../src/package-pin.mjs";
import {
  closureDigest,
  closureRowsToTsv,
  compareRealizedToLock,
  enumerateInstalledClosure,
  enumerateLockClosure,
  CLOSURE_COLUMNS,
} from "../src/closure-verify.mjs";
import { VUE_DOMAIN, SVELTE_DOMAIN, EVIDENCE_LOCK_DIGESTS } from "../src/domain-pin.mjs";
import { oracleLinkBaseDir, realizedClosureDigest } from "../src/oracle-install.mjs";
import {
  HARNESS_ROOT,
  SVELTE_EVIDENCE_CLOSURE,
  SVELTE_EVIDENCE_LOCK,
  VUE_EVIDENCE_CLOSURE,
  VUE_EVIDENCE_LOCK,
} from "../src/paths.mjs";

const ORACLE_NPM_CACHE = process.env.BF2_ORACLE_NPM_CACHE
  ? path.resolve(process.env.BF2_ORACLE_NPM_CACHE)
  : path.join(HARNESS_ROOT, ".oracle-npm-cache");
const NPM = process.platform === "win32" ? "npm.cmd" : "npm";

const scratchDirs = [];
function scratchDir(prefix) {
  const dir = mkdtempSync(path.join(tmpdir(), prefix));
  scratchDirs.push(dir);
  return dir;
}
afterAll(() => {
  for (const dir of scratchDirs) rmSync(dir, { recursive: true, force: true });
});

function sha256(text) {
  return createHash("sha256").update(text, "utf8").digest("hex");
}

/** A NESTED (transitive, non-direct) lock entry per domain, for planting. */
const NESTED_VUE_ENTRY = "node_modules/@babel/parser";
const NESTED_SVELTE_ENTRY = "node_modules/@jridgewell/sourcemap-codec";

function mutatedLockCopy(lockPath, entryKey, mutate) {
  const lock = JSON.parse(readFileSync(lockPath, "utf8"));
  expect(lock.packages[entryKey]).toBeDefined(); // the planted target exists
  const before = JSON.stringify(lock.packages[entryKey]);
  mutate(lock.packages[entryKey]);
  const after = JSON.stringify(lock.packages[entryKey]);
  expect(after).not.toBe(before); // plant proven applied
  const dir = scratchDir("bf2-closure-lock-");
  const mutatedPath = path.join(dir, "package-lock.json");
  writeFileSync(mutatedPath, `${JSON.stringify(lock, null, 2)}\n`);
  return mutatedPath;
}

describe("static transitive-closure cross-derivation", () => {
  it("accepts the genuine committed evidence for BOTH domains", () => {
    assertClosurePinned(
      VUE_EVIDENCE_LOCK,
      VUE_EVIDENCE_CLOSURE,
      EVIDENCE_LOCK_DIGESTS.vueClosureSha256,
    );
    assertClosurePinned(
      SVELTE_EVIDENCE_LOCK,
      SVELTE_EVIDENCE_CLOSURE,
      EVIDENCE_LOCK_DIGESTS.svelteClosureSha256,
    );
  });

  it("the closure covers the FULL transitive graph, not only direct packages", () => {
    const rows = enumerateLockClosure(VUE_EVIDENCE_LOCK);
    const direct = rows.filter((r) => r.direct === "yes");
    const transitive = rows.filter((r) => r.direct === "no");
    expect(direct.length).toBe(Object.keys(VUE_DOMAIN.directPackages).length);
    expect(transitive.length).toBeGreaterThan(0); // @babel/parser, postcss, …
    for (const row of rows) {
      expect(row.version).toBeTruthy();
      expect(row.integrity).toMatch(/^sha512-/);
    }
  });

  it("refuses a NESTED lock entry's mutated resolved VERSION (transitive, no direct package touched)", () => {
    const mutated = mutatedLockCopy(VUE_EVIDENCE_LOCK, NESTED_VUE_ENTRY, (entry) => {
      entry.version = "7.0.0-bf2-planted";
    });
    expect(() =>
      assertClosurePinned(mutated, VUE_EVIDENCE_CLOSURE, EVIDENCE_LOCK_DIGESTS.vueClosureSha256),
    ).toThrow(PackageDriftError);
    try {
      assertClosurePinned(mutated, VUE_EVIDENCE_CLOSURE, EVIDENCE_LOCK_DIGESTS.vueClosureSha256);
    } catch (error) {
      expect(error.details.layer).toBe("closure-derivation");
    }
  });

  it("refuses a NESTED lock entry's mutated INTEGRITY hash", () => {
    const mutated = mutatedLockCopy(SVELTE_EVIDENCE_LOCK, NESTED_SVELTE_ENTRY, (entry) => {
      entry.integrity = "sha512-BF2PLANTEDINTEGRITYVALUE==";
    });
    try {
      assertClosurePinned(
        mutated,
        SVELTE_EVIDENCE_CLOSURE,
        EVIDENCE_LOCK_DIGESTS.svelteClosureSha256,
      );
      expect.unreachable("expected PackageDriftError");
    } catch (error) {
      expect(error).toBeInstanceOf(PackageDriftError);
      expect(error.details.layer).toBe("closure-derivation");
    }
  });

  it("refuses a byte-mutated closure.tsv evidence file (digest layer)", () => {
    const original = readFileSync(VUE_EVIDENCE_CLOSURE, "utf8");
    const mutatedText = original.replace("@babel/parser\t7.29.8", "@babel/parser\t7.29.9");
    expect(mutatedText).not.toBe(original); // plant proven applied
    const dir = scratchDir("bf2-closure-tsv-");
    const mutatedPath = path.join(dir, "closure.tsv");
    writeFileSync(mutatedPath, mutatedText);
    try {
      assertClosurePinned(VUE_EVIDENCE_LOCK, mutatedPath, EVIDENCE_LOCK_DIGESTS.vueClosureSha256);
      expect.unreachable("expected PackageDriftError");
    } catch (error) {
      expect(error).toBeInstanceOf(PackageDriftError);
      expect(error.details.layer).toBe("closure-digest");
    }
  });

  it("refuses a COHERENT double mutation (lock + regenerated matching closure.tsv) via the digest pin", () => {
    // Attacker mutates a nested entry AND regenerates closure.tsv from the
    // mutated lock so the derivation cross-check would pass — the committed
    // digest transcription still refuses the regenerated evidence bytes.
    const mutatedLock = mutatedLockCopy(VUE_EVIDENCE_LOCK, NESTED_VUE_ENTRY, (entry) => {
      entry.version = "7.0.0-bf2-planted";
    });
    const regeneratedTsv = closureRowsToTsv(enumerateLockClosure(mutatedLock));
    expect(regeneratedTsv).toContain("7.0.0-bf2-planted");
    const dir = scratchDir("bf2-closure-coherent-");
    const regeneratedPath = path.join(dir, "closure.tsv");
    writeFileSync(regeneratedPath, regeneratedTsv);
    try {
      assertClosurePinned(mutatedLock, regeneratedPath, EVIDENCE_LOCK_DIGESTS.vueClosureSha256);
      expect.unreachable("expected PackageDriftError");
    } catch (error) {
      expect(error).toBeInstanceOf(PackageDriftError);
      expect(error.details.layer).toBe("closure-digest");
    }
  });

  it("assertPackagesPinned runs the closure layers (production wiring, not only the direct layers)", () => {
    const mutated = mutatedLockCopy(VUE_EVIDENCE_LOCK, NESTED_VUE_ENTRY, (entry) => {
      entry.version = "7.0.0-bf2-planted";
    });
    // Same-bytes digest so layers 1-3 (direct) pass and the failure is
    // attributable to the closure layers alone.
    const mutatedDigest = sha256(readFileSync(mutated, "utf8"));
    try {
      assertPackagesPinned(
        VUE_DOMAIN,
        oracleLinkBaseDir("vue"),
        mutated,
        mutatedDigest,
        VUE_EVIDENCE_CLOSURE,
        EVIDENCE_LOCK_DIGESTS.vueClosureSha256,
      );
      expect.unreachable("expected PackageDriftError");
    } catch (error) {
      expect(error).toBeInstanceOf(PackageDriftError);
      expect(error.details.layer).toBe("closure-derivation");
    }
  });
});

describe("executing oracle closure equals the committed lock (isolated installs)", () => {
  it("the ISOLATED install trees the compilers load from carry the LOCKED transitive versions, not workspace ones", () => {
    // The drifting members the pass-4 arbitration demonstrated: the
    // harness previously let the Svelte compiler's own parser/plugin combo
    // (acorn, @sveltejs/acorn-typescript) and Vue compiler-sfc's transitive
    // deps resolve from the workspace store. Each must now realize at the
    // exact locked version inside the isolated install the oracle loads
    // from.
    const vueBase = oracleLinkBaseDir("vue");
    const svelteBase = oracleLinkBaseDir("svelte");
    const version = (base, entryPath) =>
      JSON.parse(readFileSync(path.join(base, entryPath, "package.json"), "utf8")).version;
    const vueLock = JSON.parse(readFileSync(VUE_EVIDENCE_LOCK, "utf8"));
    const svelteLock = JSON.parse(readFileSync(SVELTE_EVIDENCE_LOCK, "utf8"));
    for (const dep of [
      "node_modules/postcss",
      "node_modules/nanoid",
      "node_modules/@babel/parser",
    ]) {
      expect(version(vueBase, dep)).toBe(vueLock.packages[dep].version);
    }
    for (const dep of [
      "node_modules/acorn",
      "node_modules/@sveltejs/acorn-typescript",
      "node_modules/devalue",
    ]) {
      expect(version(svelteBase, dep)).toBe(svelteLock.packages[dep].version);
    }
  });

  it("the LOADED compiler modules attest the pinned versions (loaded-module identity)", async () => {
    // Discriminates an oracle loaded from anywhere but the isolated install:
    // the workspace root hoists svelte/vue at DIFFERENT versions, so a
    // planted workspace import fails this identity check (and the
    // production loaded-module identity gate) rather than silently
    // compiling with the wrong closure.
    const { vueOracleCompilerVersion } = await import("../src/invoke-vue-oracle.mjs");
    const { svelteOracleCompilerVersion } = await import("../src/invoke-svelte-oracle.mjs");
    expect(vueOracleCompilerVersion()).toBe(VUE_DOMAIN.packageVersion);
    expect(svelteOracleCompilerVersion()).toBe(SVELTE_DOMAIN.packageVersion);
  });
});

describe("transitive drift is refused BEFORE any compiler invocation (end-to-end)", () => {
  it("a child process with a transitively-mutated evidence tree fails inside the pin gate, producing no artifact", () => {
    // Build a mutated evidence copy: nested @babel/parser version bumped.
    const evidenceRoot = scratchDir("bf2-evidence-root-");
    const vueDir = path.join(evidenceRoot, "oracles", "vue");
    const svelteDir = path.join(evidenceRoot, "oracles", "svelte");
    mkdirSync(vueDir, { recursive: true });
    mkdirSync(svelteDir, { recursive: true });
    const lock = JSON.parse(readFileSync(VUE_EVIDENCE_LOCK, "utf8"));
    lock.packages[NESTED_VUE_ENTRY].version = "7.0.0-bf2-planted";
    writeFileSync(path.join(vueDir, "package-lock.json"), `${JSON.stringify(lock, null, 2)}\n`);
    copyFileSync(VUE_EVIDENCE_CLOSURE, path.join(vueDir, "closure.tsv"));
    copyFileSync(SVELTE_EVIDENCE_LOCK, path.join(svelteDir, "package-lock.json"));
    copyFileSync(SVELTE_EVIDENCE_CLOSURE, path.join(svelteDir, "closure.tsv"));

    const script = `
      import { readFileSync } from "node:fs";
      import path from "node:path";
      const { compileVueFixture } = await import(${JSON.stringify(
        path.join(HARNESS_ROOT, "src/invoke-vue-oracle.mjs"),
      )});
      const source = readFileSync(path.join(${JSON.stringify(HARNESS_ROOT)}, "fixtures/vue/basic-interpolation.vue"), "utf8");
      const artifact = compileVueFixture(source, "fixtures/vue/basic-interpolation.vue", { backend: "vdom", sourceMap: false, isProd: false });
      console.log("ARTIFACT_PRODUCED", artifact.code === null ? "null" : "code");
    `;
    const result = spawnSync(process.execPath, ["--input-type=module", "-e", script], {
      encoding: "utf8",
      cwd: HARNESS_ROOT,
      env: { ...process.env, BF2_EVIDENCE_ROOT: evidenceRoot },
    });
    expect(result.status).not.toBe(0);
    expect(result.stderr).toContain("PackageDriftError");
    expect(result.stdout).not.toContain("ARTIFACT_PRODUCED"); // refused before any compilation
  });
});

describe("realized closure from a disposable, scripts-disabled, network-denied install", () => {
  const cacheReady = existsSync(ORACLE_NPM_CACHE);
  const runIf = cacheReady ? it : it.skip; // provision via scripts/provision-oracle-npm-cache.mjs — never silently passed

  /** Installs a committed lockfile offline into a fresh disposable dir. */
  function disposableInstall(lockPath) {
    const oracleDir = path.dirname(lockPath);
    const installDir = scratchDir("bf2-realized-");
    copyFileSync(path.join(oracleDir, "package.json"), path.join(installDir, "package.json"));
    copyFileSync(lockPath, path.join(installDir, "package-lock.json"));
    const npmArgs = [
      "ci",
      "--offline",
      "--ignore-scripts",
      "--no-audit",
      "--no-fund",
      "--cache",
      ORACLE_NPM_CACHE,
    ];
    if (process.platform === "darwin") {
      // Operational network denial for the whole install process tree, on
      // top of npm's own --offline: the established sandbox-exec pattern.
      const profile = path.join(installDir, "deny-network.sb");
      writeFileSync(profile, "(version 1)\n(allow default)\n(deny network*)\n");
      execFileSync("sandbox-exec", ["-f", profile, NPM, ...npmArgs], { cwd: installDir });
    } else {
      execFileSync(NPM, npmArgs, { cwd: installDir });
    }
    return installDir;
  }

  runIf(
    "the committed Vue lockfile realizes EXACTLY the committed closure (paths, versions, edges, digest)",
    () => {
      const installDir = disposableInstall(VUE_EVIDENCE_LOCK);
      const realized = enumerateInstalledClosure(installDir);
      const lockRows = enumerateLockClosure(VUE_EVIDENCE_LOCK);
      expect(realized.length).toBe(lockRows.length); // every closure member realized, nothing extra
      const comparison = compareRealizedToLock(realized, lockRows);
      expect(comparison.problems).toEqual([]);
      expect(comparison.ok).toBe(true);
      // Closure digest: the lock-derived rows and the committed closure.tsv
      // rows produce the identical canonical digest.
      const committedRows = parseCommittedClosure(VUE_EVIDENCE_CLOSURE);
      expect(closureDigest(lockRows)).toBe(closureDigest(committedRows));
    },
    120_000,
  );

  runIf(
    "the committed Svelte lockfile realizes EXACTLY the committed closure",
    () => {
      const installDir = disposableInstall(SVELTE_EVIDENCE_LOCK);
      const realized = enumerateInstalledClosure(installDir);
      const lockRows = enumerateLockClosure(SVELTE_EVIDENCE_LOCK);
      const comparison = compareRealizedToLock(realized, lockRows);
      expect(comparison.problems).toEqual([]);
      const committedRows = parseCommittedClosure(SVELTE_EVIDENCE_CLOSURE);
      expect(closureDigest(lockRows)).toBe(closureDigest(committedRows));
    },
    120_000,
  );

  runIf(
    "a tampered TRANSITIVE package inside the realized tree is caught by the independent enumeration",
    () => {
      const installDir = disposableInstall(VUE_EVIDENCE_LOCK);
      const tamperedManifest = path.join(installDir, NESTED_VUE_ENTRY, "package.json");
      const manifest = JSON.parse(readFileSync(tamperedManifest, "utf8"));
      const originalVersion = manifest.version;
      manifest.version = "7.0.0-bf2-tampered";
      writeFileSync(tamperedManifest, JSON.stringify(manifest));
      expect(readFileSync(tamperedManifest, "utf8")).toContain("7.0.0-bf2-tampered"); // plant applied

      const realized = enumerateInstalledClosure(installDir);
      const comparison = compareRealizedToLock(realized, enumerateLockClosure(VUE_EVIDENCE_LOCK));
      expect(comparison.ok).toBe(false);
      expect(
        comparison.problems.some(
          (p) => p.includes(NESTED_VUE_ENTRY) && p.includes("7.0.0-bf2-tampered"),
        ),
      ).toBe(true);
      expect(originalVersion).not.toBe("7.0.0-bf2-tampered");
    },
    120_000,
  );

  runIf(
    "a tampered payload FILE inside an installed package (package.json name/version untouched) is caught by the content-bearing enumeration",
    () => {
      // The round-4 gap made concrete: overwriting a package's executable
      // payload while leaving package.json alone passed every closure check,
      // because the enumeration read no file bytes beyond package.json.
      const installDir = disposableInstall(VUE_EVIDENCE_LOCK);
      const lockRows = enumerateLockClosure(VUE_EVIDENCE_LOCK);
      const baseline = enumerateInstalledClosure(installDir);
      // Every realized row now carries a per-package content digest.
      for (const row of baseline) expect(row.contentSha256).toMatch(/^[0-9a-f]{64}$/);
      const baselineClosureDigest = closureDigest(baseline);
      const baselineRealizedDigest = realizedClosureDigest(baseline);

      // PLANT: append executable code to a real payload .js file the
      // package actually ships — NOT package.json.
      const payloadFile = path.join(installDir, NESTED_VUE_ENTRY, "lib", "index.js");
      const originalBytes = readFileSync(payloadFile, "utf8");
      expect(originalBytes).not.toContain("bf2-planted-payload"); // plant is genuinely NEW
      writeFileSync(
        payloadFile,
        `${originalBytes}\nglobalThis.__bf2PlantedPayload = true; // bf2-planted-payload\n`,
      );
      expect(readFileSync(payloadFile, "utf8")).toContain("bf2-planted-payload"); // plant proven applied
      const manifestAfterPlant = JSON.parse(
        readFileSync(path.join(installDir, NESTED_VUE_ENTRY, "package.json"), "utf8"),
      );

      const after = enumerateInstalledClosure(installDir);
      // The lock comparison alone STILL passes — the lock records no file
      // contents, which is exactly why the enumeration must.
      expect(compareRealizedToLock(after, lockRows).ok).toBe(true);
      const tamperedRow = after.find((row) => row.path === NESTED_VUE_ENTRY);
      const baselineRow = baseline.find((row) => row.path === NESTED_VUE_ENTRY);
      // name/version are unchanged (the silent-pass scenario)…
      expect(tamperedRow.name).toBe(baselineRow.name);
      expect(tamperedRow.version).toBe(baselineRow.version);
      expect(manifestAfterPlant.version).toBe(baselineRow.version);
      // …and the tampered tree is REJECTED anyway: the per-package content
      // digest and BOTH folded closure digests (the enumeration digest and
      // the provenance-recorded realized digest) no longer match the
      // pre-tamper tree, so the strict digest comparisons refuse it.
      expect(tamperedRow.contentSha256).not.toBe(baselineRow.contentSha256);
      expect(closureDigest(after)).not.toBe(baselineClosureDigest);
      expect(realizedClosureDigest(after)).not.toBe(baselineRealizedDigest);
    },
    120_000,
  );
});

describe("production load path REFUSES a drifted or torn realized tree before any compiler execution", () => {
  // Both tests drive the REAL production load path (compileVueFixture, whose
  // pin gate is ensureOracleDomain) in a child process against a private
  // installs root primed from the validated default install: copy the tree,
  // let the production gate validate it once and record its content
  // manifest, then plant the mutation and prove the second load REFUSES with
  // PackageDriftError before the oracle compiler ever runs. "Never ran" is
  // proven the same way the end-to-end test above proves it: a marker only
  // the (poisoned) loaded compiler would print is absent, and no artifact
  // line is printed.

  const PRIME_SCRIPT = `
    const { ensureOracleDomain } = await import(${JSON.stringify(
      path.join(HARNESS_ROOT, "src/oracle-install.mjs"),
    )});
    console.log("PRIMED", ensureOracleDomain("vue").realizedClosureSha256);
  `;

  const SVELTE_PRIME_SCRIPT = `
    const { ensureOracleDomain } = await import(${JSON.stringify(
      path.join(HARNESS_ROOT, "src/oracle-install.mjs"),
    )});
    console.log("PRIMED", ensureOracleDomain("svelte").realizedClosureSha256);
  `;

  const SVELTE_LOAD_SCRIPT = `
    import { readFileSync } from "node:fs";
    import path from "node:path";
    const { compileSvelteFixture } = await import(${JSON.stringify(
      path.join(HARNESS_ROOT, "src/invoke-svelte-oracle.mjs"),
    )});
    const source = readFileSync(path.join(${JSON.stringify(
      HARNESS_ROOT,
    )}, "fixtures/svelte/basic-runes.svelte"), "utf8");
    const artifact = compileSvelteFixture(source, "fixtures/svelte/basic-runes.svelte", { generate: "client", runes: true, dev: false, sourceMap: false });
    console.log("SVELTE_ARTIFACT_PRODUCED", artifact.code === null ? "null" : "code");
  `;

  const LOAD_SCRIPT = `
    import { readFileSync } from "node:fs";
    import path from "node:path";
    const { compileVueFixture } = await import(${JSON.stringify(
      path.join(HARNESS_ROOT, "src/invoke-vue-oracle.mjs"),
    )});
    const source = readFileSync(path.join(${JSON.stringify(
      HARNESS_ROOT,
    )}, "fixtures/vue/basic-interpolation.vue"), "utf8");
    const artifact = compileVueFixture(source, "fixtures/vue/basic-interpolation.vue", { backend: "vdom", sourceMap: false, isProd: false });
    console.log("ARTIFACT_PRODUCED", artifact.code === null ? "null" : "code");
  `;

  /** Copies the validated default vue install into a fresh private installs
   * root and lets the production gate validate it + record its manifest. */
  function primedInstallsRoot() {
    const vueInstall = oracleLinkBaseDir("vue");
    const installsRoot = scratchDir("bf2-installs-gate-");
    cpSync(vueInstall, path.join(installsRoot, "vue"), { recursive: true });
    const prime = spawnSync(process.execPath, ["--input-type=module", "-e", PRIME_SCRIPT], {
      encoding: "utf8",
      cwd: HARNESS_ROOT,
      env: { ...process.env, BF2_ORACLE_INSTALLS: installsRoot },
    });
    expect(prime.status).toBe(0);
    expect(prime.stdout).toContain("PRIMED");
    expect(existsSync(path.join(installsRoot, "vue.content-manifest.json"))).toBe(true);
    return installsRoot;
  }

  function attemptLoad(installsRoot) {
    return spawnSync(process.execPath, ["--input-type=module", "-e", LOAD_SCRIPT], {
      encoding: "utf8",
      cwd: HARNESS_ROOT,
      env: { ...process.env, BF2_ORACLE_INSTALLS: installsRoot },
    });
  }

  it("a poisoned installed payload (package.json metadata untouched) is REFUSED before the compiler runs", () => {
    const installsRoot = primedInstallsRoot();
    // PLANT: append executable code to the compiler entry file itself, so
    // any load of the poisoned oracle would print the marker.
    const payloadFile = path.join(
      installsRoot,
      "vue/node_modules/@vue/compiler-sfc/dist/compiler-sfc.cjs.js",
    );
    const originalBytes = readFileSync(payloadFile, "utf8");
    expect(originalBytes).not.toContain("BF2_POISONED_COMPILER_EXECUTED"); // plant is genuinely NEW
    writeFileSync(
      payloadFile,
      `${originalBytes}\nconsole.log("BF2_POISONED_COMPILER_EXECUTED");\n`,
    );
    expect(readFileSync(payloadFile, "utf8")).toContain("BF2_POISONED_COMPILER_EXECUTED"); // plant proven applied
    const manifestPath = path.join(installsRoot, "vue/node_modules/@vue/compiler-sfc/package.json");
    expect(JSON.parse(readFileSync(manifestPath, "utf8")).name).toBe("@vue/compiler-sfc"); // metadata untouched

    const result = attemptLoad(installsRoot);
    expect(result.status).not.toBe(0);
    expect(result.stderr).toContain("PackageDriftError");
    expect(result.stderr).toContain("realized-content-drift");
    // The poisoned compiler was never evaluated and no artifact was made.
    expect(result.stdout).not.toContain("BF2_POISONED_COMPILER_EXECUTED");
    expect(result.stdout).not.toContain("ARTIFACT_PRODUCED");
  }, 240_000);

  it("a torn install tree (payload subtree deleted, manifests intact) is REFUSED even with NO recorded content manifest", () => {
    const installsRoot = primedInstallsRoot();
    // PLANT: delete the compiler's dist/ payload directory — every
    // package.json survives, so the lock comparison alone still passes —
    // and ALSO delete the recorded content manifest, proving the refusal
    // comes from the INDEPENDENT structural entry-resolvability check,
    // not from the content-digest record.
    const distDir = path.join(installsRoot, "vue/node_modules/@vue/compiler-sfc/dist");
    expect(existsSync(distDir)).toBe(true);
    rmSync(distDir, { recursive: true });
    expect(existsSync(distDir)).toBe(false); // plant proven applied
    expect(
      existsSync(path.join(installsRoot, "vue/node_modules/@vue/compiler-sfc/package.json")),
    ).toBe(true); // metadata intact — the half-written-tree shape
    rmSync(path.join(installsRoot, "vue.content-manifest.json"));
    expect(existsSync(path.join(installsRoot, "vue.content-manifest.json"))).toBe(false);

    const result = attemptLoad(installsRoot);
    expect(result.status).not.toBe(0);
    expect(result.stderr).toContain("PackageDriftError");
    expect(result.stderr).toContain("oracle-entry-unresolvable");
    expect(result.stdout).not.toContain("ARTIFACT_PRODUCED"); // refused before any compilation
  }, 240_000);

  it("a torn Svelte tree (ESM compiler subpath deleted, package root and every manifest intact) is REFUSED with NO recorded content manifest", () => {
    // The Svelte analogue of the Vue torn-tree case, targeting the loader
    // divergence that root resolvability cannot see: `svelte`'s package root
    // entry and its CJS `svelte/compiler` bundle both still resolve after
    // `src/compiler` is deleted, but the production loader is an ESM import
    // of `svelte/compiler`, whose `default` exports target is
    // src/compiler/index.js. The gate must refuse under the loader's own
    // resolution — before any oracle module is evaluated and before the tree
    // can be recorded as a fresh content baseline.
    const svelteInstall = oracleLinkBaseDir("svelte");
    const installsRoot = scratchDir("bf2-installs-svelte-torn-");
    cpSync(svelteInstall, path.join(installsRoot, "svelte"), { recursive: true });
    const prime = spawnSync(process.execPath, ["--input-type=module", "-e", SVELTE_PRIME_SCRIPT], {
      encoding: "utf8",
      cwd: HARNESS_ROOT,
      env: { ...process.env, BF2_ORACLE_INSTALLS: installsRoot },
    });
    expect(prime.status).toBe(0);
    expect(prime.stdout).toContain("PRIMED");
    expect(existsSync(path.join(installsRoot, "svelte.content-manifest.json"))).toBe(true);

    // PLANT: delete ONLY what the production ESM import resolves to. The
    // package root entry, the CJS compiler bundle, and every package.json
    // stay intact, and the content manifest is removed, so acceptance would
    // come from the structural gate alone.
    const esmCompilerDir = path.join(installsRoot, "svelte/node_modules/svelte/src/compiler");
    expect(existsSync(esmCompilerDir)).toBe(true);
    rmSync(esmCompilerDir, { recursive: true });
    expect(existsSync(esmCompilerDir)).toBe(false); // plant proven applied
    expect(existsSync(path.join(installsRoot, "svelte/node_modules/svelte/package.json"))).toBe(
      true,
    );
    expect(
      existsSync(path.join(installsRoot, "svelte/node_modules/svelte/src/index-server.js")),
    ).toBe(true); // the package ROOT entry would still resolve — the pre-fix acceptance shape
    rmSync(path.join(installsRoot, "svelte.content-manifest.json"));
    expect(existsSync(path.join(installsRoot, "svelte.content-manifest.json"))).toBe(false);

    const result = spawnSync(process.execPath, ["--input-type=module", "-e", SVELTE_LOAD_SCRIPT], {
      encoding: "utf8",
      cwd: HARNESS_ROOT,
      env: { ...process.env, BF2_ORACLE_INSTALLS: installsRoot },
    });
    expect(result.status).not.toBe(0);
    expect(result.stderr).toContain("PackageDriftError");
    expect(result.stderr).toContain("oracle-entry-unresolvable");
    // Refused by the gate, not by the loader tripping over the missing file.
    expect(result.stderr).not.toContain("ERR_MODULE_NOT_FOUND");
    expect(result.stdout).not.toContain("SVELTE_ARTIFACT_PRODUCED"); // no compiler ran
    // The torn tree was NOT recorded as a new content baseline.
    expect(existsSync(path.join(installsRoot, "svelte.content-manifest.json"))).toBe(false);
  }, 240_000);

  it("a payload mutated AFTER a successful load in the SAME process is REFUSED on the next load (memoization does not bypass the gates)", async () => {
    // The two child-process tests above start their load half in a FRESH
    // process whose per-process memo map is empty, so they can never observe
    // the memoized path. This test is the same-process complement: prime a
    // validated install through the real production loaders IN THIS PROCESS,
    // mutate a payload file, then call the same production loaders again in
    // this same process and require the refusal — the content gate must run
    // on every load, not only the first one per process.
    const installsRoot = scratchDir("bf2-installs-memo-");
    cpSync(oracleLinkBaseDir("vue"), path.join(installsRoot, "vue"), { recursive: true });
    const previousInstallsEnv = process.env.BF2_ORACLE_INSTALLS;
    process.env.BF2_ORACLE_INSTALLS = installsRoot;
    try {
      // The exact production module bound to the private installs root: the
      // root is read from the environment at module load, so a cache-busting
      // query yields this test's own instance of the production code with an
      // empty memo map — same file, same code, private state.
      const mod = await import(
        `${pathToFileURL(path.join(HARNESS_ROOT, "src/oracle-install.mjs")).href}?bf2-same-process-reload`
      );

      // Prime: full production CJS and ESM loads succeed and memoize.
      const compilerSfc = mod.oracleRequire("vue", "@vue/compiler-sfc");
      expect(typeof compilerSfc.parse).toBe("function"); // the real compiler genuinely loaded
      await mod.importOracleModule("vue", "vue");
      expect(existsSync(path.join(installsRoot, "vue.content-manifest.json"))).toBe(true); // gate armed

      // PLANT in the SAME process: mutate a payload file's content while the
      // recorded manifest (and every package.json) stays untouched.
      const payloadFile = path.join(
        installsRoot,
        "vue/node_modules/@vue/compiler-sfc/dist/compiler-sfc.cjs.js",
      );
      const originalBytes = readFileSync(payloadFile, "utf8");
      expect(originalBytes).not.toContain("BF2_SAME_PROCESS_POISON"); // plant is genuinely NEW
      writeFileSync(payloadFile, `${originalBytes}\nconsole.log("BF2_SAME_PROCESS_POISON");\n`);
      expect(readFileSync(payloadFile, "utf8")).toContain("BF2_SAME_PROCESS_POISON"); // plant proven applied

      // The next load through EACH production path, in this same process,
      // refuses with the content-drift layer before touching the compiler.
      let requireError = null;
      try {
        mod.oracleRequire("vue", "@vue/compiler-sfc");
      } catch (error) {
        requireError = error;
      }
      expect(requireError).toBeInstanceOf(PackageDriftError);
      expect(requireError.details.layer).toBe("realized-content-drift");

      let importError = null;
      try {
        await mod.importOracleModule("vue", "vue");
      } catch (error) {
        importError = error;
      }
      expect(importError).toBeInstanceOf(PackageDriftError);
      expect(importError.details.layer).toBe("realized-content-drift");
    } finally {
      if (previousInstallsEnv === undefined) delete process.env.BF2_ORACLE_INSTALLS;
      else process.env.BF2_ORACLE_INSTALLS = previousInstallsEnv;
    }
  }, 240_000);
});

function parseCommittedClosure(tsvPath) {
  const lines = readFileSync(tsvPath, "utf8")
    .replace(/\r?\n$/, "")
    .split("\n");
  const columns = lines[0].split("\t");
  expect(columns).toEqual(CLOSURE_COLUMNS);
  return lines.slice(1).map((line) => {
    const values = line.split("\t");
    return Object.fromEntries(columns.map((c, i) => [c, values[i]]));
  });
}

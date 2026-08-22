// Self-test: source/package drift refusal (BF2 required exit).
//
// Proves the harness REJECTS a mutated pin at the direct layers — git
// checkout drift, evidence-lock byte drift, and evidence-lock content
// drift — using REAL mutated copies (never a mocked assertion), and that it
// ACCEPTS the genuine, unmutated pin. The TRANSITIVE half of drift refusal
// — nested lock-entry mutations, closure-evidence digest/derivation, and
// the realized disposable-install enumeration — lives in
// test/closure-drift.spec.mjs, with its own planted mutations.

import { describe, expect, it, afterAll } from "vitest";
import { execFileSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync, copyFileSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

import { assertCheckoutPinned, CheckoutDriftError } from "../src/checkout-pin.mjs";
import { assertPackagesPinned, PackageDriftError } from "../src/package-pin.mjs";
import { VUE_DOMAIN, EVIDENCE_LOCK_DIGESTS } from "../src/domain-pin.mjs";
import { VUE_EVIDENCE_LOCK } from "../src/paths.mjs";
import { oracleLinkBaseDir } from "../src/oracle-install.mjs";
import { oracleSourcePaths } from "../src/env-paths.mjs";

// Layer 1 resolves the direct packages from the ISOLATED oracle install —
// the only place the oracle domain resolves from since workspace
// resolution stopped defining it.
const VUE_INSTALL = oracleLinkBaseDir("vue");

const scratchDirs = [];
function scratchDir() {
  const dir = mkdtempSync(path.join(tmpdir(), "bf2-drift-"));
  scratchDirs.push(dir);
  return dir;
}
afterAll(() => {
  for (const dir of scratchDirs) rmSync(dir, { recursive: true, force: true });
});

describe("git checkout drift refusal", () => {
  const { vueSource } = oracleSourcePaths();
  const runIf = vueSource ? it : it.skip;

  runIf("accepts the genuine pinned checkout", () => {
    const identity = assertCheckoutPinned(vueSource, VUE_DOMAIN);
    expect(identity.commit).toBe(VUE_DOMAIN.commit);
    expect(identity.tree).toBe(VUE_DOMAIN.tree);
  });

  runIf("rejects a checkout at the wrong commit", () => {
    const dir = scratchDir();
    execFileSync("git", ["init", "-q", dir]);
    writeFileSync(path.join(dir, "marker.txt"), "not vue core\n");
    execFileSync("git", ["-C", dir, "add", "-A"]);
    execFileSync("git", [
      "-C",
      dir,
      "-c",
      "user.email=t@t",
      "-c",
      "user.name=t",
      "commit",
      "-q",
      "-m",
      "x",
    ]);
    expect(() => assertCheckoutPinned(dir, VUE_DOMAIN)).toThrow(CheckoutDriftError);
    try {
      assertCheckoutPinned(dir, VUE_DOMAIN);
    } catch (error) {
      expect(error.details.kind).toBe("commit");
    }
  });

  runIf("rejects a dirty pinned checkout", () => {
    // Mutate a real, uncommitted change into the pinned checkout, verify
    // rejection, then revert — never leaves the shared oracle checkout dirty.
    const marker = path.join(vueSource, "BF2_SELFTEST_MARKER_DO_NOT_COMMIT.txt");
    writeFileSync(marker, "drift probe\n");
    try {
      expect(() => assertCheckoutPinned(vueSource, VUE_DOMAIN)).toThrow(CheckoutDriftError);
      try {
        assertCheckoutPinned(vueSource, VUE_DOMAIN);
      } catch (error) {
        expect(error.details.kind).toBe("dirty");
      }
    } finally {
      rmSync(marker, { force: true });
    }
    // Confirm the checkout is clean again and now passes.
    expect(() => assertCheckoutPinned(vueSource, VUE_DOMAIN)).not.toThrow();
  });
});

describe("package/evidence-lock drift refusal", () => {
  it("accepts the genuine committed evidence lock", () => {
    const resolved = assertPackagesPinned(
      VUE_DOMAIN,
      VUE_INSTALL,
      VUE_EVIDENCE_LOCK,
      EVIDENCE_LOCK_DIGESTS.vuePackageLockSha256,
    );
    expect(resolved.vue).toBe(VUE_DOMAIN.packageVersion);
  });

  it("rejects a byte-mutated evidence lock (layer 2: lock-digest)", () => {
    const dir = scratchDir();
    const mutated = path.join(dir, "package-lock.json");
    const original = readFileSync(VUE_EVIDENCE_LOCK, "utf8");
    // A real, minimal, PROVEN-applied mutation: flip one integrity char.
    const mutatedText = original.replace("sha512-yM", "sha512-XM");
    expect(mutatedText).not.toBe(original); // proves the plant actually applied
    writeFileSync(mutated, mutatedText);
    expect(() =>
      assertPackagesPinned(
        VUE_DOMAIN,
        VUE_INSTALL,
        mutated,
        EVIDENCE_LOCK_DIGESTS.vuePackageLockSha256,
      ),
    ).toThrow(PackageDriftError);
    try {
      assertPackagesPinned(
        VUE_DOMAIN,
        VUE_INSTALL,
        mutated,
        EVIDENCE_LOCK_DIGESTS.vuePackageLockSha256,
      );
    } catch (error) {
      expect(error.details.layer).toBe("lock-digest");
    }
  });

  it("rejects an evidence lock whose recorded integrity drifted from domain-pin.mjs (layer 3)", () => {
    const dir = scratchDir();
    const mutated = path.join(dir, "package-lock.json");
    const original = JSON.parse(readFileSync(VUE_EVIDENCE_LOCK, "utf8"));
    original.packages["node_modules/vue"].integrity = "sha512-MUTATED_INTEGRITY_VALUE==";
    const mutatedText = `${JSON.stringify(original)}\n`;
    writeFileSync(mutated, mutatedText);
    // This mutation necessarily also changes the file's own digest, so
    // pass the freshly recomputed expected digest to isolate layer 3 from
    // layer 2 — proving layer 3 independently catches content drift even
    // when the caller (incorrectly) supplied a matching digest.
    const freshDigest = execFileSync("shasum", ["-a", "256", mutated], { encoding: "utf8" }).split(
      " ",
    )[0];
    expect(freshDigest).not.toBe(EVIDENCE_LOCK_DIGESTS.vuePackageLockSha256);
    try {
      assertPackagesPinned(VUE_DOMAIN, VUE_INSTALL, mutated, freshDigest);
      throw new Error("expected PackageDriftError");
    } catch (error) {
      expect(error).toBeInstanceOf(PackageDriftError);
      expect(error.details.layer).toBe("lock-integrity");
    }
  });

  it("rejects when the installed package version drifts from the pin (layer 1)", () => {
    // A package resolved to a wrong version is exactly what layer 1 exists
    // to catch. We cannot un-pin the real installed devDependency inside
    // this test process, so this proves the mechanism directly: a domain
    // object asserting a version the real installed package does NOT have.
    const impossibleDomain = { ...VUE_DOMAIN, packageVersion: "3.6.0-rc.999-does-not-exist" };
    expect(() =>
      assertPackagesPinned(
        impossibleDomain,
        VUE_INSTALL,
        VUE_EVIDENCE_LOCK,
        EVIDENCE_LOCK_DIGESTS.vuePackageLockSha256,
      ),
    ).toThrow(PackageDriftError);
  });
});

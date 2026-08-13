// Self-test: golden provenance binds the generator's git identity, the
// COMPLETE generation implementation, and the exact realized oracle
// closure — and a missing or mutated bound field fails golden
// regeneration/checking.
//
// Three layers, mirroring the store's own trust chain:
//  1. validation-mechanism discrimination: validateRecordProvenance
//     rejects a record with EACH bound field missing or malformed;
//  2. committed-set binding: every committed golden carries the full
//     binding, the recorded implementation digest equals the CURRENT
//     generation implementation's digest, and the recorded realized
//     closures equal the CURRENT isolated installs' digests;
//  3. end-to-end check refusal: a child-process `--check` against a
//     doctored golden-set copy (one record's provenance field mutated,
//     manifest coherently updated so the content-address alone cannot be
//     the catch) exits non-zero naming the provenance failure.

import { describe, expect, it, afterAll } from "vitest";
import { spawnSync } from "node:child_process";
import { cpSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createHash } from "node:crypto";
import { tmpdir } from "node:os";
import path from "node:path";

import {
  checkComparableRecord,
  generationImplementationFiles,
  generationImplementationSha256,
  generatorGitIdentity,
  validateRecordProvenance,
  ProvenanceValidationError,
} from "../src/provenance.mjs";
import { ensureOracleDomain } from "../src/oracle-install.mjs";
import { readGoldenManifest, readGoldenSet, serializeGoldenRecord } from "../src/golden-store.mjs";
import { GOLDENS_ROOT, HARNESS_ROOT } from "../src/paths.mjs";

const GENERATOR_ENTRY = path.join(HARNESS_ROOT, "bin/generate-goldens.mjs");

const scratchDirs = [];
afterAll(() => {
  for (const dir of scratchDirs) rmSync(dir, { recursive: true, force: true });
});

describe("provenance validation mechanism", () => {
  function validRecord() {
    // A committed record IS the canonical valid shape.
    const set = readGoldenSet(GOLDENS_ROOT);
    const [name, record] = [...set.entries()][0];
    return { name, record: JSON.parse(JSON.stringify(record)) };
  }

  it("accepts every COMMITTED golden record", () => {
    for (const [name, record] of readGoldenSet(GOLDENS_ROOT)) {
      validateRecordProvenance(name, record);
    }
  });

  const FIELD_MUTATIONS = [
    ["generator.commit missing", (r) => delete r.generator.commit],
    ["generator.commit malformed", (r) => (r.generator.commit = "not-a-git-oid")],
    ["generator.tree missing", (r) => delete r.generator.tree],
    ["generator.tree malformed", (r) => (r.generator.tree = "1234")],
    ["generator.worktreeDirty missing", (r) => delete r.generator.worktreeDirty],
    ["generator.implementationSha256 missing", (r) => delete r.generator.implementationSha256],
    [
      "generator.implementationSha256 malformed",
      (r) => (r.generator.implementationSha256 = "zz".repeat(32)),
    ],
    ["realizedClosureSha256 missing", (r) => delete r.realizedClosureSha256],
    ["realizedClosureSha256 malformed", (r) => (r.realizedClosureSha256 = "short")],
    ["packageLockSha256 missing", (r) => delete r.packageLockSha256],
    ["fixture.sha256 missing", (r) => delete r.fixture.sha256],
    ["normalizer.implementationSha256 missing", (r) => delete r.normalizer.implementationSha256],
    ["generator object missing entirely", (r) => delete r.generator],
    ["schemaVersion downgraded", (r) => (r.schemaVersion = 2)],
  ];

  for (const [label, mutate] of FIELD_MUTATIONS) {
    it(`rejects a record with ${label}`, () => {
      const { name, record } = validRecord();
      const before = JSON.stringify(record);
      mutate(record);
      expect(JSON.stringify(record)).not.toBe(before); // plant proven applied
      expect(() => validateRecordProvenance(name, record)).toThrow(ProvenanceValidationError);
    });
  }
});

describe("committed-set binding", () => {
  it("every committed record's implementation digest equals the CURRENT generation implementation", () => {
    const current = generationImplementationSha256(GENERATOR_ENTRY);
    for (const [, record] of readGoldenSet(GOLDENS_ROOT)) {
      expect(record.generator.implementationSha256).toBe(current);
    }
  });

  it("the implementation digest covers the TRANSITIVE module closure, not just the entry script", () => {
    const files = generationImplementationFiles(GENERATOR_ENTRY).map((f) =>
      path.relative(HARNESS_ROOT, f).split(path.sep).join("/"),
    );
    expect(files).toContain("bin/generate-goldens.mjs");
    // The load-bearing generation modules all participate in the digest.
    for (const module of [
      "src/normalize.mjs",
      "src/invoke-vue-oracle.mjs",
      "src/invoke-svelte-oracle.mjs",
      "src/oracle-install.mjs",
      "src/golden-store.mjs",
      "src/package-pin.mjs",
      "src/closure-verify.mjs",
      "src/fragments.mjs",
      "src/provenance.mjs",
    ]) {
      expect(files).toContain(module);
    }
  });

  it("every committed record's realized-closure digest equals the CURRENT isolated install's digest", () => {
    const realized = {
      vue: ensureOracleDomain("vue").realizedClosureSha256,
      svelte: ensureOracleDomain("svelte").realizedClosureSha256,
    };
    for (const [, record] of readGoldenSet(GOLDENS_ROOT)) {
      expect(record.realizedClosureSha256).toBe(realized[record.framework]);
    }
  });

  it("committed records carry a well-formed generator git identity, and the CURRENT identity is derivable", () => {
    const identity = generatorGitIdentity();
    expect(identity.commit).toMatch(/^[0-9a-f]{40}$/);
    expect(identity.tree).toMatch(/^[0-9a-f]{40}$/);
    for (const [, record] of readGoldenSet(GOLDENS_ROOT)) {
      expect(record.generator.commit).toMatch(/^[0-9a-f]{40}$/);
      expect(record.generator.tree).toMatch(/^[0-9a-f]{40}$/);
      expect(typeof record.generator.worktreeDirty).toBe("boolean");
    }
  });

  it("the check projection normalizes ONLY generation-time git identity; content-bound digests stay strict", () => {
    const set = readGoldenSet(GOLDENS_ROOT);
    const [, record] = [...set.entries()][0];
    const projected = checkComparableRecord(record);
    expect(projected.generator.commit).toBeNull();
    expect(projected.generator.tree).toBeNull();
    expect(projected.generator.worktreeDirty).toBeNull();
    // Strictly-compared fields survive the projection untouched…
    expect(projected.generator.implementationSha256).toBe(record.generator.implementationSha256);
    expect(projected.realizedClosureSha256).toBe(record.realizedClosureSha256);
    // …so a mutated implementation digest still differs under projection.
    const mutated = JSON.parse(JSON.stringify(record));
    mutated.generator.implementationSha256 = "0".repeat(64);
    expect(serializeGoldenRecord(checkComparableRecord(mutated))).not.toBe(
      serializeGoldenRecord(checkComparableRecord(record)),
    );
  });
});

describe("end-to-end check refusal", () => {
  it("--check against a set whose record lost a provenance field fails, even with a COHERENT manifest", () => {
    // Doctor a full COPY of the committed set: strip one record's
    // realizedClosureSha256, rewrite the record under its new
    // content-address, and update the manifest coherently — so the
    // content-address check alone would PASS and only the provenance
    // validation can refuse.
    const doctored = mkdtempSync(path.join(tmpdir(), "bf2-goldens-doctored-"));
    scratchDirs.push(doctored);
    cpSync(GOLDENS_ROOT, doctored, { recursive: true });
    const manifest = JSON.parse(readFileSync(path.join(doctored, "manifest.json"), "utf8"));
    const [name, digest] = Object.entries(manifest.entries)[0];
    const recordFile = path.join(doctored, "records", `${digest}.json`);
    const record = JSON.parse(readFileSync(recordFile, "utf8"));
    expect(record.realizedClosureSha256).toMatch(/^[0-9a-f]{64}$/); // present before the plant
    delete record.realizedClosureSha256;
    const newText = serializeGoldenRecord(record);
    expect(newText).not.toContain("realizedClosureSha256"); // plant proven applied
    const newDigest = createHash("sha256").update(newText, "utf8").digest("hex");
    writeFileSync(path.join(doctored, "records", `${newDigest}.json`), newText, "utf8");
    manifest.entries[name] = newDigest;
    writeFileSync(
      path.join(doctored, "manifest.json"),
      `${JSON.stringify(manifest, null, 2)}\n`,
      "utf8",
    );

    const result = spawnSync(
      process.execPath,
      [path.join(HARNESS_ROOT, "bin/generate-goldens.mjs"), "--check"],
      {
        encoding: "utf8",
        cwd: HARNESS_ROOT,
        env: { ...process.env, BF2_GOLDENS_ROOT: doctored },
      },
    );
    expect(result.status).not.toBe(0);
    expect(result.stderr).toContain("PROVENANCE");
    expect(result.stderr).toContain("realizedClosureSha256");
  }, 60_000); // spawns a full --check child; the 5s default flakes under parallel worker contention

  it("--check against the GENUINE committed set passes (control arm)", () => {
    const result = spawnSync(
      process.execPath,
      [path.join(HARNESS_ROOT, "bin/generate-goldens.mjs"), "--check"],
      { encoding: "utf8", cwd: HARNESS_ROOT },
    );
    expect(result.stderr).toBe("");
    expect(result.status).toBe(0);
    expect(result.stdout).toContain("OK");
  }, 60_000); // spawns a full --check child; the 5s default flakes under parallel worker contention

  it("the committed manifest records the generation metadata (generator identity + realized closures)", () => {
    const manifest = readGoldenManifest(GOLDENS_ROOT);
    expect(manifest.generation).toBeGreaterThanOrEqual(1);
    expect(manifest.generator.implementationSha256).toMatch(/^[0-9a-f]{64}$/);
    expect(manifest.realizedClosures.vue).toMatch(/^[0-9a-f]{64}$/);
    expect(manifest.realizedClosures.svelte).toMatch(/^[0-9a-f]{64}$/);
  });
});

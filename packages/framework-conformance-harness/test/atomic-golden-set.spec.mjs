// Self-test: atomic GOLDEN-SET publication (BF2 required exit "atomic
// result accounting" — the golden-set half; the per-artifact half is
// test/atomic-result-accounting.spec.mjs).
//
// A golden set is published through ONE reader-visible commit point (the
// manifest rename). A generation run that fails partway — including after
// several record files already reached disk — leaves the ENTIRE previous
// set fully observable and never exposes a mixed or partial set to any
// reader, not even transiently.

import { describe, expect, it, afterEach } from "vitest";
import { createHash } from "node:crypto";
import { existsSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

import {
  goldenManifestPath,
  publishGoldenSet,
  readGoldenByName,
  readGoldenManifest,
  readGoldenSet,
} from "../src/golden-store.mjs";
import { GOLDENS_ROOT, HARNESS_ROOT } from "../src/paths.mjs";

const dirs = [];
afterEach(() => {
  for (const d of dirs.splice(0)) rmSync(d, { recursive: true, force: true });
});
function freshRoot() {
  const d = mkdtempSync(path.join(tmpdir(), "bf2-goldenset-"));
  dirs.push(d);
  return d;
}

/** A record whose serialization THROWS — simulates the Nth write failing. */
function boobyTrappedRecord() {
  return {
    get poison() {
      throw new Error("simulated mid-generation failure");
    },
  };
}

describe("atomic golden-set publication", () => {
  it("publishes a complete set exactly once, resolvable only through the manifest", () => {
    const root = freshRoot();
    publishGoldenSet(root, [
      { name: "vue/a", record: { framework: "vue", code: "export default 1;" } },
      { name: "svelte/b", record: { framework: "svelte", code: "export default 2;" } },
    ]);
    const set = readGoldenSet(root);
    expect([...set.keys()].sort()).toEqual(["svelte/b", "vue/a"]);
    expect(set.get("vue/a").code).toBe("export default 1;");
    expect(Object.isFrozen(set.get("vue/a"))).toBe(true);
  });

  it("a failure at the Nth record leaves NO manifest — no partial set is ever reader-visible", () => {
    const root = freshRoot();
    expect(() =>
      publishGoldenSet(root, [
        { name: "vue/a", record: { code: "export default 1;" } },
        { name: "vue/b", record: { code: "export default 2;" } },
        { name: "vue/c", record: boobyTrappedRecord() }, // 3rd of 4 throws
        { name: "vue/d", record: { code: "export default 4;" } },
      ]),
    ).toThrow(/simulated mid-generation failure/);
    // Records for a/b DID reach disk (work happened)…
    expect(readdirSync(path.join(root, "records")).length).toBeGreaterThan(0);
    // …but the commit point never happened: no manifest, so a reader sees
    // no set at all rather than a partial one.
    expect(existsSync(goldenManifestPath(root))).toBe(false);
    expect(() => readGoldenSet(root)).toThrow();
  });

  it("a failed re-publication leaves the ENTIRE previous set fully observable (no mixing)", () => {
    const root = freshRoot();
    publishGoldenSet(root, [
      { name: "vue/a", record: { generation: 1, code: "export default 1;" } },
      { name: "vue/b", record: { generation: 1, code: "export default 2;" } },
    ]);
    const before = readGoldenSet(root);

    expect(() =>
      publishGoldenSet(root, [
        { name: "vue/a", record: { generation: 2, code: "export default 100;" } }, // lands on disk
        { name: "vue/b", record: boobyTrappedRecord() }, // then the run dies
      ]),
    ).toThrow(/simulated mid-generation failure/);

    // Reader still sees generation 1 for EVERY entry — never generation 2
    // for one entry and generation 1 for another.
    const after = readGoldenSet(root);
    expect([...after.keys()].sort()).toEqual([...before.keys()].sort());
    for (const [name, record] of after) {
      expect(record.generation).toBe(1);
      expect(record.code).toBe(before.get(name).code);
    }
  });

  it("a successful re-publication atomically replaces the set, retains TWO generations of grace records, then GCs them", () => {
    const root = freshRoot();
    publishGoldenSet(root, [
      { name: "vue/a", record: { setGeneration: 1, case: "a" } },
      { name: "vue/gone", record: { setGeneration: 1, case: "gone" } },
    ]);
    publishGoldenSet(root, [
      { name: "vue/a", record: { setGeneration: 2, case: "a" } },
      { name: "vue/new", record: { setGeneration: 2, case: "new" } },
    ]);
    const set = readGoldenSet(root);
    expect([...set.keys()].sort()).toEqual(["vue/a", "vue/new"]);
    expect(set.get("vue/a").setGeneration).toBe(2);
    expect(readGoldenManifest(root).generation).toBe(2);
    // Reader-schedule grace: the two live records PLUS the replaced
    // manifest's two records remain.
    expect(readdirSync(path.join(root, "records")).length).toBe(4);
    // A THIRD publish still retains generation 1: with two full
    // generations of grace, records collect only once unreferenced for
    // three consecutive generations.
    publishGoldenSet(root, [{ name: "vue/a", record: { setGeneration: 3, case: "a" } }]);
    // gen-3 live (1) + gen-2 grace (2) + gen-1 grace (2).
    expect(readdirSync(path.join(root, "records")).length).toBe(5);
    // A FOURTH publish finally collects the generation-1 records while
    // retaining generations 2-3 as the new grace window.
    publishGoldenSet(root, [{ name: "vue/a", record: { setGeneration: 4, case: "a" } }]);
    const files = readdirSync(path.join(root, "records"));
    expect(files.length).toBe(4); // gen-4 live (1) + gen-3 grace (1) + gen-2 grace (2)
    // No temp files left behind at the goldens root either.
    expect(readdirSync(root).filter((f) => f.includes(".tmp-"))).toEqual([]);
  });

  it("stale-reader schedule across TWO publishes: manifest N's records survive publishes N+1 and N+2, and a FOURTH publish evicts them", () => {
    const root = freshRoot();
    publishGoldenSet(root, [
      { name: "vue/a", record: { setGeneration: 1, case: "a" } },
      { name: "vue/exclusive", record: { setGeneration: 1, case: "exclusive" } },
    ]);
    // READER SCHEDULE: the reader loads manifest generation N…
    const staleManifest = readGoldenManifest(root);
    // …then STALLS long enough for TWO full publishes to complete (the
    // round-4 counter-schedule that one generation of grace provably lost)…
    publishGoldenSet(root, [{ name: "vue/a", record: { setGeneration: 2, case: "a" } }]);
    publishGoldenSet(root, [{ name: "vue/a", record: { setGeneration: 3, case: "a" } }]);
    // …then dereferences the records its manifest lists: every one is
    // still present and digest-consistent.
    for (const [name, digest] of Object.entries(staleManifest.entries)) {
      const recordFile = path.join(root, "records", `${digest}.json`);
      expect(existsSync(recordFile), `stale reader lost record ${name} (${digest})`).toBe(true);
      const text = readFileSync(recordFile, "utf8");
      expect(createHash("sha256").update(text, "utf8").digest("hex")).toBe(digest);
      expect(JSON.parse(text).setGeneration).toBe(1);
    }
    // Grace is BOUNDED, not unbounded retention: a FOURTH publish moves
    // generation N out of the two-generation window and its exclusive
    // records are collected.
    publishGoldenSet(root, [{ name: "vue/a", record: { setGeneration: 4, case: "a" } }]);
    for (const [, digest] of Object.entries(staleManifest.entries)) {
      expect(existsSync(path.join(root, "records", `${digest}.json`))).toBe(false);
    }
    // The live set still reads back intact after the eviction.
    expect(readGoldenSet(root).get("vue/a").setGeneration).toBe(4);
  });

  it("stale-reader schedule: a reader of the OLD manifest can still read every record it lists after a publish", () => {
    const root = freshRoot();
    publishGoldenSet(root, [
      { name: "vue/a", record: { setGeneration: 1, case: "a" } },
      { name: "vue/gone", record: { setGeneration: 1, case: "gone" } },
    ]);
    // READER SCHEDULE: the reader loads the manifest FIRST…
    const staleManifest = readGoldenManifest(root);
    // …THEN a full publish replaces the set (drops "vue/gone" entirely)…
    publishGoldenSet(root, [{ name: "vue/a", record: { setGeneration: 2, case: "a" } }]);
    // …THEN the reader dereferences the records its manifest lists — the
    // exact schedule the pre-fix immediate sweep broke.
    for (const [name, digest] of Object.entries(staleManifest.entries)) {
      const recordFile = path.join(root, "records", `${digest}.json`);
      expect(existsSync(recordFile), `stale reader lost record ${name} (${digest})`).toBe(true);
      const text = readFileSync(recordFile, "utf8");
      // Digest-consistent, not just present: the reader's own verification
      // path succeeds.
      expect(createHash("sha256").update(text, "utf8").digest("hex")).toBe(digest);
      expect(JSON.parse(text).setGeneration).toBe(1);
    }
  });

  it("record files are content-addressed: bytes must match the manifest digest at read time", () => {
    const root = freshRoot();
    publishGoldenSet(root, [{ name: "vue/a", record: { code: "export default 1;" } }]);
    const manifest = readGoldenManifest(root);
    const digest = manifest.entries["vue/a"];
    expect(digest).toMatch(/^[0-9a-f]{64}$/);
    // Tamper with the record file's bytes; the digest check must refuse it.
    const recordFile = path.join(root, "records", `${digest}.json`);
    const original = readFileSync(recordFile, "utf8");
    const tampered = original.replace("export default 1;", "export default 666;");
    expect(tampered).not.toBe(original); // plant proven applied
    writeFileSync(recordFile, tampered, "utf8");
    expect(() => readGoldenByName(root, "vue/a")).toThrow(/do not match manifest digest/);
  });

  it("write-once collision: publishing a record whose content-address holds DIFFERENT bytes is a hard error", () => {
    // The adversarial K7 scenario: plant bytes B' at records/<D>.json, then
    // publish a record whose serialization digests to D — the write-once
    // guard must refuse with the digest-collision error, never silently
    // overwrite and never silently accept the planted bytes.
    const root = freshRoot();
    const record = { code: "export default 1;" };
    publishGoldenSet(root, [{ name: "vue/a", record }]);
    const digest = readGoldenManifest(root).entries["vue/a"];
    const recordFile = path.join(root, "records", `${digest}.json`);
    const original = readFileSync(recordFile, "utf8");
    const planted = original.replace("export default 1;", "export default 666;");
    expect(planted).not.toBe(original); // plant proven applied
    writeFileSync(recordFile, planted, "utf8");
    expect(readFileSync(recordFile, "utf8")).toBe(planted); // plant proven present
    // Re-publishing the ORIGINAL record must hit the collision guard.
    expect(() => publishGoldenSet(root, [{ name: "vue/a", record }])).toThrow(
      /digest collision with different bytes/,
    );
    // Neither writer's data was corrupted or silently replaced: the file
    // still holds exactly the planted bytes, and no manifest replaced the
    // committed one mid-error.
    expect(readFileSync(recordFile, "utf8")).toBe(planted);
    expect(readGoldenManifest(root).entries["vue/a"]).toBe(digest);
  });

  it("write-once race: two concurrent publishers of the SAME content-addressed records corrupt nothing", async () => {
    const root = freshRoot();
    const writerScript = `
      const { publishGoldenSet } = await import(${JSON.stringify(
        path.join(HARNESS_ROOT, "src/golden-store.mjs"),
      )});
      const root = process.argv[1];
      for (let i = 0; i < 20; i += 1) {
        publishGoldenSet(root, [
          { name: "vue/a", record: { code: "export default 1;" } },
          { name: "vue/b", record: { code: "export default 2;" } },
        ]);
      }
      console.log("WRITER_DONE");
    `;
    const { spawn } = await import("node:child_process");
    const run = () =>
      new Promise((resolvePromise) => {
        const child = spawn(process.execPath, ["--input-type=module", "-e", writerScript, root], {
          stdio: ["ignore", "pipe", "pipe"],
        });
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
    // Two REAL concurrent processes racing the same content-addressed
    // records and the same manifest path (distinct pids -> distinct temp
    // names; identical bytes -> rename-over-identical is benign).
    const [a, b] = await Promise.all([run(), run()]);
    expect(a.stderr).toBe("");
    expect(b.stderr).toBe("");
    expect(a.code).toBe(0);
    expect(b.code).toBe(0);
    // Neither writer's data lost or corrupted: the final set reads back
    // digest-consistent with exactly the published contents.
    const set = readGoldenSet(root);
    expect([...set.keys()].sort()).toEqual(["vue/a", "vue/b"]);
    expect(set.get("vue/a").code).toBe("export default 1;");
    expect(set.get("vue/b").code).toBe("export default 2;");
  });

  it("the COMMITTED golden set is complete, manifest-resolvable, and digest-consistent", () => {
    const set = readGoldenSet(GOLDENS_ROOT); // throws on any digest mismatch
    expect(set.size).toBe(48);
    for (const record of set.values()) {
      expect(record.schemaVersion).toBe(3);
      expect(record.normalizer.version).toBe(6);
      expect(record.normalizer.implementationSha256).toMatch(/^[0-9a-f]{64}$/);
    }
  });
});

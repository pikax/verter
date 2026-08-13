// Self-test: the ATOMIC-COMMIT PRIMITIVE at both publication points is
// kill-sensitive.
//
// Vocabulary discipline (arbitration): this suite is about the
// atomic-commit PRIMITIVE — is the final path only ever produced by a
// rename from a completed temp file — as distinct from the reader-schedule
// property (atomic-golden-set.spec.mjs's stale-reader test) and from the
// mechanisms' result-level behavior (the other atomic specs). Before this
// suite, replacing either `renameSync` commit with a direct non-atomic
// `writeFileSync` of the final path left the whole test suite green: every
// existing test observed only the AFTER state, which a direct write also
// produces. This suite interposes the exact `node:fs` functions the
// production modules call and asserts the commit DISCIPLINE itself:
//
//  - `writeFileSync` never targets the final path (temp files only), and
//  - the final path's content arrives via `renameSync(temp, final)` where
//    the temp was fully written first.
//
// Kill sensitivity, proven by self-mutation in both directions: swap
// either commit's rename for a direct final-path write and the
// corresponding test here fails; restore it and the suite passes.

import { describe, expect, it, vi, afterEach } from "vitest";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

// Interpose the fs functions BOTH production modules import, recording
// every call while delegating to the real implementation — never a stub:
// real files land on a real filesystem and the results are re-read below.
const fsCalls = { writeFileSync: [], renameSync: [] };
vi.mock("node:fs", async (importOriginal) => {
  const original = await importOriginal();
  return {
    ...original,
    writeFileSync: (file, ...rest) => {
      fsCalls.writeFileSync.push(String(file));
      return original.writeFileSync(file, ...rest);
    },
    renameSync: (from, to) => {
      fsCalls.renameSync.push({ from: String(from), to: String(to) });
      return original.renameSync(from, to);
    },
  };
});

const { runAtomic } = await import("../src/result-writer.mjs");
const { publishGoldenSet, readGoldenSet, goldenManifestPath, readGoldenManifest } =
  await import("../src/golden-store.mjs");
const { readFileSync } = await import("node:fs");

const dirs = [];
afterEach(() => {
  fsCalls.writeFileSync.length = 0;
  fsCalls.renameSync.length = 0;
  for (const d of dirs.splice(0)) rmSync(d, { recursive: true, force: true });
});
function freshDir(prefix) {
  const d = mkdtempSync(path.join(tmpdir(), prefix));
  dirs.push(d);
  return d;
}

describe("atomic-commit primitive — result-writer.mjs runAtomic", () => {
  it("the final artifact path is produced ONLY by rename-from-temp, never a direct write", () => {
    const dir = freshDir("bf2-rename-kill-result-");
    const outPath = path.join(dir, "result.json");
    const result = runAtomic(outPath, () => ({ items: [1, 2, 3] }));
    expect(result.items).toEqual([1, 2, 3]);
    expect(JSON.parse(readFileSync(outPath, "utf8")).items).toEqual([1, 2, 3]);
    // Commit discipline: no writeFileSync ever targeted the final path…
    expect(fsCalls.writeFileSync).not.toContain(outPath);
    // …the payload was written to a DISTINCT temp path…
    const tempWrites = fsCalls.writeFileSync.filter((f) => f.startsWith(`${outPath}.tmp-`));
    expect(tempWrites.length).toBe(1);
    // …and the final path arrived via exactly one rename from that temp.
    const commits = fsCalls.renameSync.filter((c) => c.to === outPath);
    expect(commits.length).toBe(1);
    expect(commits[0].from).toBe(tempWrites[0]);
  });
});

describe("atomic-commit primitive — golden-store.mjs manifest swap", () => {
  it("the manifest path is produced ONLY by rename-from-temp, never a direct write", () => {
    const root = freshDir("bf2-rename-kill-goldens-");
    publishGoldenSet(root, [
      { name: "vue/a", record: { code: "export default 1;" } },
      { name: "vue/b", record: { code: "export default 2;" } },
    ]);
    const manifestTarget = goldenManifestPath(root);
    expect(readGoldenManifest(root).entries["vue/a"]).toMatch(/^[0-9a-f]{64}$/);
    expect(readGoldenSet(root).size).toBe(2);
    // Commit discipline for the manifest — the single reader-visible
    // commit point of the whole set:
    expect(fsCalls.writeFileSync).not.toContain(manifestTarget);
    const tempWrites = fsCalls.writeFileSync.filter((f) => f.startsWith(`${manifestTarget}.tmp-`));
    expect(tempWrites.length).toBe(1);
    const commits = fsCalls.renameSync.filter((c) => c.to === manifestTarget);
    expect(commits.length).toBe(1);
    expect(commits[0].from).toBe(tempWrites[0]);
  });

  it("every content-addressed RECORD file is likewise rename-committed, never directly written", () => {
    const root = freshDir("bf2-rename-kill-records-");
    publishGoldenSet(root, [{ name: "vue/a", record: { code: "export default 1;" } }]);
    const digest = readGoldenManifest(root).entries["vue/a"];
    const recordFile = path.join(root, "records", `${digest}.json`);
    expect(fsCalls.writeFileSync).not.toContain(recordFile);
    expect(fsCalls.renameSync.some((c) => c.to === recordFile)).toBe(true);
  });
});

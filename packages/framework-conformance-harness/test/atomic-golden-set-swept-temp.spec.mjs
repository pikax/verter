// The Linux write-once race failed with ENOENT: a peer publisher committed
// the same bytes, then the post-commit sweep unlinked this process's
// `<digest>.json.tmp-<pid>` before rename. Reproduce that order against
// the real rename.

import { describe, expect, it, vi, afterEach } from "vitest";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

vi.mock("node:fs", async (importOriginal) => {
  const original = await importOriginal();
  return {
    ...original,
    renameSync(from, to) {
      const source = String(from);
      if (source.includes(`${path.sep}records${path.sep}`) && source.includes(".tmp-")) {
        if (!original.existsSync(to)) original.copyFileSync(source, to);
        original.rmSync(source, { force: true });
      }
      return original.renameSync(from, to);
    },
  };
});

const { publishGoldenSet, readGoldenSet } = await import("../src/golden-store.mjs");

const dirs = [];
afterEach(() => {
  for (const d of dirs.splice(0)) rmSync(d, { recursive: true, force: true });
});

describe("write-once record temp swept before rename", () => {
  it("succeeds when the content-addressed target already holds the same bytes", () => {
    const root = mkdtempSync(path.join(tmpdir(), "bf2-goldenset-swept-"));
    dirs.push(root);
    publishGoldenSet(root, [{ name: "vue/a", record: { code: "export default 1;" } }]);
    expect(readGoldenSet(root).get("vue/a").code).toBe("export default 1;");
  });
});

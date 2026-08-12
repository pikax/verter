// Self-test: atomic result accounting (BF2 required exit).
//
// A run that fails partway through must leave NO artifact on disk — never a
// truncated or partial one. A run that succeeds publishes exactly once,
// atomically.

import { describe, expect, it, afterEach } from "vitest";
import { existsSync, mkdtempSync, readdirSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

import { runAtomic, PartialResultError } from "../src/result-writer.mjs";

const dirs = [];
afterEach(() => {
  for (const d of dirs.splice(0)) rmSync(d, { recursive: true, force: true });
});

function freshDir() {
  const d = mkdtempSync(path.join(tmpdir(), "bf2-atomic-"));
  dirs.push(d);
  return d;
}

describe("atomic result accounting", () => {
  it("publishes nothing when work() throws after partial accumulation", () => {
    const dir = freshDir();
    const outPath = path.join(dir, "result.json");
    const accumulated = [];
    expect(() =>
      runAtomic(outPath, () => {
        for (let i = 0; i < 5; i += 1) {
          accumulated.push(i);
          if (i === 3) throw new Error("simulated mid-flight failure");
        }
        return { accumulated };
      }),
    ).toThrow(PartialResultError);
    expect(accumulated).toEqual([0, 1, 2, 3]); // work happened...
    expect(existsSync(outPath)).toBe(false); // ...but nothing was published
    expect(readdirSync(dir)).toEqual([]); // no temp file left behind either
  });

  it("publishes the complete result exactly once on success", () => {
    const dir = freshDir();
    const outPath = path.join(dir, "result.json");
    const result = runAtomic(outPath, () => ({
      total: 1000,
      items: Array.from({ length: 1000 }, (_, i) => i),
    }));
    expect(result.total).toBe(1000);
    expect(existsSync(outPath)).toBe(true);
    const onDisk = JSON.parse(readFileSync(outPath, "utf8"));
    expect(onDisk.items.length).toBe(1000);
    // No leftover temp file after a successful publish.
    expect(readdirSync(dir)).toEqual(["result.json"]);
  });

  it("a second successful run atomically replaces the first (no torn intermediate state)", () => {
    const dir = freshDir();
    const outPath = path.join(dir, "result.json");
    runAtomic(outPath, () => ({ version: 1 }));
    runAtomic(outPath, () => ({ version: 2 }));
    const onDisk = JSON.parse(readFileSync(outPath, "utf8"));
    expect(onDisk.version).toBe(2);
    expect(readdirSync(dir)).toEqual(["result.json"]);
  });

  it("a failing second run leaves the FIRST successful result intact", () => {
    const dir = freshDir();
    const outPath = path.join(dir, "result.json");
    runAtomic(outPath, () => ({ version: 1 }));
    expect(() =>
      runAtomic(outPath, () => {
        throw new Error("second run fails");
      }),
    ).toThrow(PartialResultError);
    const onDisk = JSON.parse(readFileSync(outPath, "utf8"));
    expect(onDisk.version).toBe(1);
  });
});

/**
 * @ai-generated - Proves the gate smoke executable reaches the real Vapor
 * bootstrap and canonical in-memory TypeScript observation paths.
 */

import { spawnSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";

const HARNESS_ROOT = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const SMOKE = path.join(HARNESS_ROOT, "bin", "gate-smoke.mjs");

function run(mode) {
  return spawnSync(process.execPath, [SMOKE, mode], {
    cwd: HARNESS_ROOT,
    encoding: "utf8",
    timeout: 60_000,
  });
}

describe("canonical gate smoke executable", () => {
  it.each(["vapor", "typescript"])(
    "runs the real %s harness path and emits its receipt",
    (mode) => {
      const result = run(mode);
      expect(result.error).toBeUndefined();
      expect(result.signal).toBeNull();
      expect(result.status, result.stderr).toBe(0);
      expect(JSON.parse(result.stdout)).toEqual({
        schema: "verter-harness-smoke/v1",
        mode,
        ok: true,
      });
      expect(result.stderr).toBe("");
    },
  );

  it("refuses an unknown mode without emitting a success receipt", () => {
    const result = run("unknown");
    expect(result.status).not.toBe(0);
    expect(result.stdout).toBe("");
    expect(result.stderr).toContain("unknown harness smoke mode");
  });
});

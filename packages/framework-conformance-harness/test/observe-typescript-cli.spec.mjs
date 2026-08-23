/**
 * @ai-generated - Exercises deterministic public TypeScript-observation CLI refusals.
 */

import { spawnSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

import { describe, expect, it } from "vitest";

const CLI = path.resolve(import.meta.dirname, "../bin/observe-typescript.mjs");

describe("observe-typescript CLI refusals", () => {
  // @ai-generated - Proves canonical aliases refuse without an uncaught stack or machine path.
  it("classifies canonical virtual-file aliases as a structured refusal", () => {
    const directory = mkdtempSync(path.join(tmpdir(), "verter-observe-cli-"));
    const inputPath = path.join(directory, "aliases.json");
    const fileNames = ["/alias/Api.ts", "/alias\\Api.ts"];
    try {
      writeFileSync(
        inputPath,
        JSON.stringify({
          artifacts: [
            { fileName: fileNames[0], code: 'export const selected = "slash" as const;\n' },
            { fileName: fileNames[1], code: 'export const selected = "backslash" as const;\n' },
          ],
        }),
      );

      const result = spawnSync(process.execPath, [CLI, "--input", inputPath], {
        cwd: import.meta.dirname,
        encoding: "utf8",
      });

      expect.soft(result.status).toBe(3);
      expect.soft(result.signal).toBeNull();
      expect.soft(result.stdout).toBe(
        JSON.stringify({
          refused: "virtual-file-identity",
          collisions: [{ fileNames: [...fileNames].sort() }],
        }),
      );
      expect.soft(result.stderr).toBe("");
      const publicOutput = `${result.stdout}\n${result.stderr}`;
      expect.soft(publicOutput).not.toContain("VirtualFileIdentityError");
      expect.soft(publicOutput).not.toContain("node:internal");
      expect.soft(publicOutput).not.toContain("at file:");
      expect.soft(publicOutput).not.toContain(directory);
      expect.soft(publicOutput).not.toContain(inputPath);
    } finally {
      rmSync(directory, { recursive: true, force: true });
    }
  });
});

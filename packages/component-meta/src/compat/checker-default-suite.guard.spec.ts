/**
 * @ai-generated - Pins the compat checker suite into the package's default
 * test command and proves the suite contains executable tests.
 */

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const packageJsonUrl = new URL("../../package.json", import.meta.url);
const checkerSpecUrl = new URL("./checker.spec.ts", import.meta.url);
const checkerExclusion = "--exclude '**/checker.spec.ts'";

describe("A2-01: checker.spec.ts belongs to the default suite", () => {
  it("does not exclude checker.spec.ts and discovers non-zero tests", () => {
    const packageJson = JSON.parse(readFileSync(fileURLToPath(packageJsonUrl), "utf8")) as {
      scripts?: { test?: string };
    };
    const testScript = packageJson.scripts?.test ?? "";
    const checkerSource = readFileSync(fileURLToPath(checkerSpecUrl), "utf8");
    const executableTests = checkerSource.match(/\b(?:it|test)\s*\(/g) ?? [];

    expect(testScript).not.toContain(checkerExclusion);
    expect(executableTests.length).toBeGreaterThan(0);
  });
});

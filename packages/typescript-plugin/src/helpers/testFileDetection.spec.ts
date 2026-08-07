import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { clearTestFileDetectionCache, isTestFileWithContext } from "./testFileDetection";

interface TestFileSystem {
  [fileName: string]: string;
}

function normalizePath(fileName: string): string {
  return fileName.replace(/\\/g, "/");
}

function createHost(files: TestFileSystem) {
  const normalizedFiles = new Map(
    Object.entries(files).map(([fileName, content]) => [normalizePath(fileName), content]),
  );

  return {
    fileExists(fileName: string) {
      return normalizedFiles.has(normalizePath(fileName));
    },
    readFile(fileName: string) {
      return normalizedFiles.get(normalizePath(fileName));
    },
  };
}

beforeEach(() => {
  clearTestFileDetectionCache();
  // Freeze the clock: config discovery carries a small wall-clock budget (a
  // production resilience fuse for slow disk walks). These tests pin the
  // GLOB-MATCHING semantics over an in-memory host, so a loaded parallel
  // test run must not be able to expire the budget mid-discovery and turn a
  // semantic assertion into a scheduler lottery.
  const frozenNow = Date.now();
  vi.spyOn(Date, "now").mockReturnValue(frozenNow);
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("isTestFileWithContext", () => {
  it("detects non-standard vitest test paths from nearest config include globs", () => {
    const host = createHost({
      "/repo/vitest.config.ts": `
import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    include: ["tests/unit/**/*.ts"],
  },
});
`,
    });

    expect(isTestFileWithContext("/repo/tests/unit/helper.ts", host)).toBe(true);
    expect(isTestFileWithContext("/repo/src/helper.ts", host)).toBe(false);
  });

  it("detects jest test paths from testMatch in the nearest config", () => {
    const host = createHost({
      "/repo/packages/app/jest.config.js": `
module.exports = {
  testMatch: ["<rootDir>/specs/**/*.ts"],
};
`,
    });

    expect(isTestFileWithContext("/repo/packages/app/specs/formatter.ts", host)).toBe(true);
    expect(isTestFileWithContext("/repo/packages/app/src/formatter.ts", host)).toBe(false);
  });

  it("falls back to filename heuristics when config parsing is unsupported", () => {
    const host = createHost({
      "/repo/vite.config.ts": `
export default new Proxy({}, {
  get() {
    throw new Error("not statically readable");
  },
});
`,
    });

    expect(isTestFileWithContext("/repo/src/App.spec.ts", host)).toBe(true);
    expect(isTestFileWithContext("/repo/src/App.ts", host)).toBe(false);
  });
});

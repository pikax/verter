/**
 * @ai-generated - Tests cross-platform path resolution for the LSP benchmark CLI.
 */

import { describe, expect, it } from "vitest";

import {
  parseLspBenchConfig,
  resolveBenchmarkTarget,
  resolveTypeScriptSdk,
  resolveVerterBinary,
  resolveVolarScript,
} from "./lsp-bench.config";

describe("resolveVerterBinary", () => {
  it("falls back to the debug binary on macOS when release is missing", () => {
    const binary = resolveVerterBinary({
      repoRoot: "/repo",
      platform: "darwin",
      pathExists: (path) => path === "/repo/target/debug/verter-lsp",
    });

    expect(binary).toBe("/repo/target/debug/verter-lsp");
  });

  it("uses the .exe suffix on Windows", () => {
    const binary = resolveVerterBinary({
      repoRoot: "C:/repo",
      platform: "win32",
      pathExists: (path) => path === "C:/repo/target/release/verter-lsp.exe",
    });

    expect(binary).toBe("C:/repo/target/release/verter-lsp.exe");
  });
});

describe("resolveTypeScriptSdk", () => {
  it("prefers the workspace-local TypeScript SDK", () => {
    const tsdk = resolveTypeScriptSdk({
      workspaceRoot: "/workspace",
      repoRoot: "/repo",
      pathExists: (path) => path === "/workspace/node_modules/typescript/lib",
      resolvePackage: () => {
        throw new Error("package fallback should not be used");
      },
    });

    expect(tsdk).toBe("/workspace/node_modules/typescript/lib");
  });
});

describe("resolveVolarScript", () => {
  it("derives the script path from the installed package root", () => {
    const script = resolveVolarScript({
      pathExists: (path) =>
        path === "/repo/node_modules/@vue/language-server/bin/vue-language-server.js",
      resolvePackage: () => "/repo/node_modules/@vue/language-server/package.json",
    });

    expect(script).toBe("/repo/node_modules/@vue/language-server/bin/vue-language-server.js");
  });
});

describe("resolveBenchmarkTarget", () => {
  it("defaults to the checked-in example fixture and converts hover flags to zero-based", () => {
    const target = resolveBenchmarkTarget({
      repoRoot: "/repo",
      cwd: "/repo",
      pathExists: (path) =>
        path === "/repo/packages/example" || path === "/repo/packages/example/Test.vue",
      hoverLine: "2",
      hoverChar: "9",
    });

    expect(target.workspaceRoot).toBe("/repo/packages/example");
    expect(target.testFile).toBe("/repo/packages/example/Test.vue");
    expect(target.testFileRel).toBe("Test.vue");
    expect(target.hoverLine).toBe(1);
    expect(target.hoverChar).toBe(8);
  });
});

describe("parseLspBenchConfig", () => {
  it("does not require Volar dependencies when --skip-volar is set", () => {
    const config = parseLspBenchConfig({
      argv: ["node", "lsp-bench.ts", "--skip-volar"],
      cwd: "/repo",
      env: {},
      platform: "darwin",
      repoRoot: "/repo",
      pathExists: (path) =>
        path === "/repo/packages/example" ||
        path === "/repo/packages/example/Test.vue" ||
        path === "/repo/target/debug/verter-lsp",
      resolvePackage: () => {
        throw new Error("volar and typescript resolution should be skipped");
      },
    });

    expect(config.skipVolar).toBe(true);
    expect(config.volarScript).toBeUndefined();
    expect(config.tsdkPath).toBeUndefined();
    expect(config.verterBin).toBe("/repo/target/debug/verter-lsp");
  });
});

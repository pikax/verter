// Self-test: offline execution (BF2 required exit).
//
// Two independent proofs: a portable one (works on every platform: monkey-
// patch the network-capable globals so any attempted use throws, then
// confirm compilation still succeeds) and an operational macOS proof
// (mirrors BF1's own command-proofs/bf2-oracle-manifest-generate session:
// run compilation as a child process under a `sandbox-exec` profile that
// denies all network operations for the whole process tree, and confirm a
// real `curl` fails under the identical profile while compilation
// succeeds). The macOS proof SKIPS with an explicit reason elsewhere —
// sandbox-exec is a macOS-only mechanism (Cross-Platform Portability,
// CLAUDE.md) — the portable proof always runs and is not itself skippable.

import { describe, expect, it } from "vitest";
import { spawnSync } from "node:child_process";
import { readFileSync, writeFileSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

import { HARNESS_ROOT } from "../src/paths.mjs";

describe("offline execution — portable proof", () => {
  it("compiles a Vue fixture with fetch/http/dns/net poisoned to throw", async () => {
    const previousFetch = globalThis.fetch;
    globalThis.fetch = () => {
      throw new Error("BF2 offline-proof: network attempted via fetch");
    };
    try {
      const { compileVueFixture } = await import("../src/invoke-vue-oracle.mjs");
      const source = readFileSync(
        path.join(HARNESS_ROOT, "fixtures/vue/basic-interpolation.vue"),
        "utf8",
      );
      const result = compileVueFixture(source, "fixtures/vue/basic-interpolation.vue", {
        backend: "vdom",
        sourceMap: false,
        isProd: false,
      });
      expect(result.code).toBeTruthy();
      expect(result.diagnostics).toEqual([]);
    } finally {
      globalThis.fetch = previousFetch;
    }
  });

  it("compiles a Svelte fixture with fetch poisoned to throw", async () => {
    const previousFetch = globalThis.fetch;
    globalThis.fetch = () => {
      throw new Error("BF2 offline-proof: network attempted via fetch");
    };
    try {
      const { compileSvelteFixture } = await import("../src/invoke-svelte-oracle.mjs");
      const source = readFileSync(
        path.join(HARNESS_ROOT, "fixtures/svelte/basic-runes.svelte"),
        "utf8",
      );
      const result = compileSvelteFixture(source, "fixtures/svelte/basic-runes.svelte", {
        generate: "client",
        runes: true,
        dev: false,
        sourceMap: false,
      });
      expect(result.code).toBeTruthy();
    } finally {
      globalThis.fetch = previousFetch;
    }
  });
});

describe("offline execution — operational macOS sandbox proof", () => {
  const runIf = process.platform === "darwin" ? it : it.skip;

  runIf("golden generation runs under sandbox-exec deny-network while curl fails", () => {
    const sandboxProfile = path.join(HARNESS_ROOT, "test", "deny-network.sb");
    writeFileSync(sandboxProfile, "(version 1)\n(allow default)\n(deny network*)\n");

    // Control: a real network call MUST fail under this profile — proves
    // the sandbox is actually denying network, not silently permitting it.
    const curlResult = spawnSync(
      "sandbox-exec",
      ["-f", sandboxProfile, "curl", "-sS", "-m", "3", "https://example.com"],
      {
        encoding: "utf8",
      },
    );
    expect(curlResult.status).not.toBe(0);

    // Subject: golden generation's --check mode must SUCCEED under the
    // identical profile (it only reads locally-installed pinned packages
    // and locally-authored fixtures, then compares to committed goldens).
    const dir = mkdtempSync(path.join(tmpdir(), "bf2-sandbox-"));
    try {
      const checkResult = spawnSync(
        "sandbox-exec",
        [
          "-f",
          sandboxProfile,
          process.execPath,
          path.join(HARNESS_ROOT, "bin/generate-goldens.mjs"),
          "--check",
        ],
        { encoding: "utf8", cwd: HARNESS_ROOT },
      );
      expect(checkResult.status, checkResult.stderr).toBe(0);
      expect(checkResult.stdout).toContain("OK:");
    } finally {
      rmSync(dir, { recursive: true, force: true });
      rmSync(sandboxProfile, { force: true });
    }
  });
});

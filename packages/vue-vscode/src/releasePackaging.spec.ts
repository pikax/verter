import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const here = path.dirname(fileURLToPath(import.meta.url));
const extensionRoot = path.resolve(here, "..");
const repoRoot = path.resolve(extensionRoot, "..", "..");

const read = (rel: string) => readFileSync(path.join(repoRoot, rel), "utf8");

/**
 * The packaged VSIX must actually carry the engine it advertises.
 *
 * `release.yml` cross-compiles a per-platform `verter-lsp`, copies it into
 * `packages/vue-vscode/bin/`, and `findLspBinary` resolves
 * `<extensionPath>/bin/verter-lsp` as the bundled path. `stageShimBinary`
 * prunes every `bin/` entry outside its whitelist, so the engine only survives
 * packaging if the whitelist admits it.
 */
describe("VSIX engine payload", () => {
  const stageBin = read("packages/vue-vscode/stage-bin.mjs");

  it("whitelists the LSP engine so staging does not delete it", () => {
    const match = stageBin.match(/EXTRA_ALLOWED_BIN_ENTRIES\s*=\s*\[([^\]]*)\]/);
    expect(match, "EXTRA_ALLOWED_BIN_ENTRIES must exist in stage-bin.mjs").toBeTruthy();

    const entries = match![1];
    expect(
      entries.includes("verter-lsp"),
      "stage-bin.mjs prunes every bin/ entry outside its whitelist. Without `verter-lsp` " +
        "the release workflow's pre-staged engine is deleted before `vsce package`, and the " +
        "published VSIX ships with no engine (extension.ts falls through to a PATH lookup).",
    ).toBe(true);
  });

  it("whitelists the Windows engine filename too", () => {
    const match = stageBin.match(/EXTRA_ALLOWED_BIN_ENTRIES\s*=\s*\[([^\]]*)\]/);
    expect(
      match![1].includes("verter-lsp.exe"),
      "Windows ships `verter-lsp.exe`; a POSIX-only whitelist drops the engine on win32.",
    ).toBe(true);
  });

  it("still prunes unknown bin/ entries", () => {
    // The whitelist must stay a whitelist — a wildcard would let arbitrary
    // artifacts into the published VSIX.
    expect(stageBin).toMatch(/if\s*\(!allowedBinEntries\.includes\(entry\)\)/);
  });
});

/**
 * A tag push publishes to crates.io, npm and the VS Code Marketplace. Every
 * publish job must therefore transitively depend on a job that actually runs
 * the test suite — otherwise a red tree publishes silently.
 */
describe("release gating", () => {
  const release = read(".github/workflows/release.yml");

  const needsOf = (job: string): string[] => {
    const jobRe = new RegExp(`^  ${job}:\\n([\\s\\S]*?)(?=^  \\S|\\Z)`, "m");
    const body = release.match(jobRe)?.[1] ?? "";
    const needs = body.match(/^\s{4}needs:\s*(.+)$/m)?.[1] ?? "";
    return needs
      .replace(/[[\]]/g, "")
      .split(",")
      .map((s) => s.trim())
      .filter(Boolean);
  };

  const publishJobs = ["publish-crates", "publish-npm", "publish-vscode"];

  it.each(publishJobs)("%s depends on a job that runs tests", (job) => {
    const needs = needsOf(job);
    expect(needs.length, `${job} must declare needs:`).toBeGreaterThan(0);
    expect(
      needs.includes("test"),
      `${job} publishes to a public registry. Its dependency chain is [${needs.join(", ")}], ` +
        "none of which runs the test suite. A tag push therefore publishes without tests, " +
        "clippy or fmt ever running.",
    ).toBe(true);
  });

  it("defines a test job that runs the canonical Rust gate and the JS suite", () => {
    expect(release, "release.yml must define a `test:` job").toMatch(/^ {2}test:$/m);
    const body = release.match(/^ {2}test:\n([\s\S]*?)(?=^ {2}\S|\Z)/m)?.[1] ?? "";
    expect(body, "the test job must run the canonical Rust gate").toContain("scripts/gate.mjs");
    expect(body, "the test job must run clippy with -D warnings").toMatch(
      /clippy[\s\S]*-D warnings/,
    );
    expect(body, "the test job must check formatting").toContain("cargo fmt");
    expect(body, "the test job must run the JS suite").toMatch(/pnpm (run )?test/);
  });
});

/**
 * @ai-generated - Verifies setup-script argument parsing, project resolution, and prepared-repo validation.
 *
 * Tier 6 §8.2 / T9.3 — `--ref=<sha>` is now MANDATORY. The
 * floating-branch fallback was retired; tests pin both the new
 * required-ref behavior and the porcelain-status detector that
 * refuses to clobber dirty target worktrees.
 */

import { describe, expect, it } from "vitest";

import {
  getMetaUiCheckoutCommands,
  getMetaUiInstallStrategies,
  isCommitSha,
  parseGitStatusPorcelain,
  parseMetaUiSetupArgs,
  resolveMetaUiProject,
  validatePreparedMetaUiRepo,
} from "./meta-ui-setup.mjs";

describe("resolveMetaUiProject", () => {
  it("reads the canonical nuxt-ui project definition from the integration project list", async () => {
    const project = await resolveMetaUiProject();

    expect(project.name).toBe("nuxt-ui");
    expect(project.repo).toBe("nuxt/ui");
    expect(project.branch).toBe("v4");
  });
});

describe("parseMetaUiSetupArgs", () => {
  it("returns the explicit ref, target, and unfrozen-install flag when --ref is provided", () => {
    const args = parseMetaUiSetupArgs(
      ["--ref=90a94fb162d532ada26012bfe1ab82adc9217988"],
      "D:/dev/personal/verter",
    );

    expect(args.repoRoot).toBe("D:/dev/personal/verter");
    expect(args.targetRoot).toBe("D:/dev/personal/verter/.integration-tests/repos/nuxt-ui");
    expect(args.ref).toBe("90a94fb162d532ada26012bfe1ab82adc9217988");
    expect(args.allowUnfrozenInstall).toBe(false);
    expect(args.allowDirtyTarget).toBe(false);
  });

  it("Tier 6 §8.2 / T9.3 — throws when --ref is absent (strict-ref enforcement)", () => {
    // Discriminator: pre-T9.3 the parser silently fell back to the
    // floating branch HEAD. Post-T9.3 it MUST throw.
    expect(() => parseMetaUiSetupArgs([], "D:/dev/personal/verter")).toThrow(
      /`--ref=.*` is required/,
    );
  });

  it("Tier 6 §8.2 / T9.3 — throws when --ref is empty (`--ref=`)", () => {
    expect(() => parseMetaUiSetupArgs(["--ref="], "D:/dev/personal/verter")).toThrow(
      /`--ref=.*` is required/,
    );
  });

  it("Tier 6 §8.2 / T9.3 — surfaces the --allow-dirty-target opt-in", () => {
    const args = parseMetaUiSetupArgs(
      ["--ref=v0.5.0", "--allow-dirty-target"],
      "D:/dev/personal/verter",
    );
    expect(args.allowDirtyTarget).toBe(true);
  });

  it("requires explicit opt-in before allowing an unfrozen install fallback", () => {
    expect(getMetaUiInstallStrategies({ allowUnfrozenInstall: false })).toEqual([
      ["install", "--frozen-lockfile"],
    ]);
    expect(getMetaUiInstallStrategies({ allowUnfrozenInstall: true })).toEqual([
      ["install", "--frozen-lockfile"],
      ["install", "--no-frozen-lockfile"],
    ]);
  });
});

describe("getMetaUiCheckoutCommands", () => {
  const project = {
    name: "nuxt-ui",
    repo: "nuxt/ui",
    branch: "v4",
  };

  it("checks out a pinned commit sha after fetching the canonical branch", () => {
    expect(
      getMetaUiCheckoutCommands(project, {
        ref: "90a94fb162d532ada26012bfe1ab82adc9217988",
      }),
    ).toEqual([
      ["fetch", "origin", "v4", "--prune", "--tags"],
      ["checkout", "--detach", "90a94fb162d532ada26012bfe1ab82adc9217988"],
    ]);
  });

  it("fetches symbolic refs explicitly without relying on object-id fetches", () => {
    expect(getMetaUiCheckoutCommands(project, { ref: "refs/pull/123/head" })).toEqual([
      ["fetch", "origin", "v4", "--prune", "--tags"],
      ["fetch", "origin", "refs/pull/123/head", "--prune", "--tags"],
      ["checkout", "--detach", "FETCH_HEAD"],
    ]);
  });

  it("Tier 6 §8.2 / T9.3 — refuses to build commands without an explicit ref", () => {
    // Discriminator: defensive guard in case a future caller bypasses
    // `parseMetaUiSetupArgs`. Pre-T9.3 the function returned a
    // floating-branch checkout list; post-T9.3 it throws.
    expect(() => getMetaUiCheckoutCommands(project, {})).toThrow(/`ref` is required/);
    expect(() => getMetaUiCheckoutCommands(project, { ref: null })).toThrow(/`ref` is required/);
  });
});

describe("isCommitSha", () => {
  it("distinguishes raw commit ids from branch and ref names", () => {
    expect(isCommitSha("90a94fb162d532ada26012bfe1ab82adc9217988")).toBe(true);
    expect(isCommitSha("90a94fb")).toBe(true);
    expect(isCommitSha("v4")).toBe(false);
    expect(isCommitSha("refs/pull/123/head")).toBe(false);
  });
});

describe("validatePreparedMetaUiRepo", () => {
  it("requires the component directory and generated nuxt tsconfigs", () => {
    expect(() =>
      validatePreparedMetaUiRepo("D:/repo", {
        exists: (path) =>
          path === "D:/repo/src/runtime/components" || path === "D:/repo/.nuxt/tsconfig.app.json",
      }),
    ).toThrow(/tsconfig\.shared\.json/i);
  });
});

describe("Tier 6 §8.2 / T9.3 — parseGitStatusPorcelain", () => {
  // Discriminator: characterize the pure parser the
  // setup-meta-ui.mjs cleanliness check uses. The parser must
  // surface every dirty entry so the caller can refuse to clobber.
  it("returns an empty array for a clean worktree", () => {
    expect(parseGitStatusPorcelain("")).toEqual([]);
    expect(parseGitStatusPorcelain("\n")).toEqual([]);
  });

  it("parses staged-modified, untracked, and deleted entries", () => {
    const out = [
      " M src/runtime/components/Button.vue",
      "?? new-file.txt",
      " D removed-file.json",
      "M  staged-modification.ts",
    ].join("\n");
    expect(parseGitStatusPorcelain(out)).toEqual([
      { xy: " M", path: "src/runtime/components/Button.vue" },
      { xy: "??", path: "new-file.txt" },
      { xy: " D", path: "removed-file.json" },
      { xy: "M ", path: "staged-modification.ts" },
    ]);
  });

  it("strips trailing CR characters from porcelain output (Windows compat)", () => {
    const crlf = " M src/file.ts\r\n?? added.ts\r\n";
    expect(parseGitStatusPorcelain(crlf)).toEqual([
      { xy: " M", path: "src/file.ts" },
      { xy: "??", path: "added.ts" },
    ]);
  });

  it("ignores malformed short lines (defensive)", () => {
    expect(parseGitStatusPorcelain("ab")).toEqual([]);
  });
});

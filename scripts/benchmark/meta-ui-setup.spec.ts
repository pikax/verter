/**
 * @ai-generated - Verifies setup-script argument parsing, project resolution, and prepared-repo validation.
 */

import { describe, expect, it } from "vitest";

import {
  getMetaUiCheckoutCommands,
  getMetaUiInstallStrategies,
  isCommitSha,
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
  it("defaults to the integration-test checkout path and no explicit ref", () => {
    const args = parseMetaUiSetupArgs([], "D:/dev/personal/verter");

    expect(args.repoRoot).toBe("D:/dev/personal/verter");
    expect(args.targetRoot).toBe("D:/dev/personal/verter/.integration-tests/repos/nuxt-ui");
    expect(args.ref).toBeNull();
    expect(args.allowUnfrozenInstall).toBe(false);
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

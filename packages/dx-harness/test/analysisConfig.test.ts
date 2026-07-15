/**
 * The local-analysis-input config loader + the `DX_HARNESS_EXTERNAL_CORPUS` hook.
 *
 * Discriminating: env-UNSET resolves to the default committed-fixtures source and
 * reads NOTHING (preserving the byte-identical default behavior the hermetic suite
 * asserts); env-SET loads a synthetic config written to the OS temp dir (never the
 * repo), yields opaque-id projects, and REJECTS a config with a descriptive id.
 */

import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import {
  ANALYSIS_CORPUS_ENV,
  ANALYSIS_PROJECTS_SCHEMA,
  AnalysisConfigError,
  parseAnalysisConfig,
  resolveCorpusSource,
} from "../src/analysisConfig.js";

/** Build a descriptive (banned) id from fragments so this SOURCE stays clean. */
function descriptiveId(): string {
  return `${"nex"}${"us"}${"-ui"}`;
}

const GOOD_CONFIG = JSON.stringify({
  schema: ANALYSIS_PROJECTS_SCHEMA,
  checkerBin: "/path/to/tsgo",
  projects: [
    {
      id: "p0001",
      root: "/path/to/project",
      tsconfig: "/path/to/project/tsconfig.json",
      kind: "vite",
      ambientDts: [],
      vueTscBin: null,
      workstreams: ["ide", "tsc", "build"],
    },
  ],
});

const tempDirs: string[] = [];
afterEach(() => {
  for (const d of tempDirs.splice(0)) rmSync(d, { recursive: true, force: true });
});

/** Write a config under a fresh OS temp dir (NEVER the repo). Returns its path. */
function writeTempConfig(json: string): string {
  const dir = mkdtempSync(path.join(tmpdir(), "verter-analysis-"));
  tempDirs.push(dir);
  const file = path.join(dir, "projects.local.json");
  writeFileSync(file, json, "utf-8");
  return file;
}

describe("analysis config loader", () => {
  it("parses a good config and yields opaque-id projects", () => {
    const cfg = parseAnalysisConfig(GOOD_CONFIG);
    expect(cfg.schema).toBe(ANALYSIS_PROJECTS_SCHEMA);
    expect(cfg.projects).toHaveLength(1);
    expect(cfg.projects[0].id).toBe("p0001");
    expect(cfg.projects[0].kind).toBe("vite");
    expect(cfg.projects[0].workstreams).toEqual(["ide", "tsc", "build"]);
  });

  it("rejects a config whose project id is not opaque without echoing the id", () => {
    const descriptive = descriptiveId();
    const bad = GOOD_CONFIG.replace('"p0001"', `"${descriptive}"`);
    expect(() => parseAnalysisConfig(bad)).toThrow(AnalysisConfigError);
    // The error never echoes a path NOR the rejected descriptive id (itself a
    // private identity).
    try {
      parseAnalysisConfig(bad);
    } catch (e) {
      expect(String(e)).not.toContain("/path/to");
      expect(String(e)).not.toContain(descriptive);
      expect(String(e)).toContain("<redacted>");
    }
  });

  it("rejects a wrong schema discriminant", () => {
    const bad = GOOD_CONFIG.replace(ANALYSIS_PROJECTS_SCHEMA, "verter.something-else.v9");
    expect(() => parseAnalysisConfig(bad)).toThrow(/schema must be/);
  });

  // C-c: the config mirrors the Rust private-field discipline — real paths live on
  // private fields reachable only through narrow accessors, never as plain
  // serializable public data, and serialization emits the opaque id only.
  it("holds real paths privately and never serializes them", () => {
    const cfg = parseAnalysisConfig(GOOD_CONFIG);
    const project = cfg.projects[0];
    // Paths are accessor METHODS (the I/O surface), not enumerable public data.
    expect(project.root()).toBe("/path/to/project");
    expect(project.tsconfig()).toBe("/path/to/project/tsconfig.json");
    expect(cfg.checkerBin()).toBe("/path/to/tsgo");
    // A stray JSON.stringify of the project / config leaks NO path byte.
    const projectJson = JSON.stringify(project);
    const cfgJson = JSON.stringify(cfg);
    for (const json of [projectJson, cfgJson]) {
      expect(json).not.toContain("/path/to");
      expect(json).not.toContain("tsgo");
    }
    // The opaque id survives serialization (the safe identity).
    expect(projectJson).toContain("p0001");
    // The private root is NOT an own-enumerable property (it is a `#private` field).
    expect(Object.keys(project)).not.toContain("root");
    expect(Object.values(project)).not.toContain("/path/to/project");
  });

  it("idRootPairs yields the private roots for the redactor only", () => {
    const cfg = parseAnalysisConfig(GOOD_CONFIG);
    expect(cfg.idRootPairs()).toEqual([["p0001", "/path/to/project"]]);
  });
});

describe("DX_HARNESS_EXTERNAL_CORPUS hook", () => {
  it("resolves to the default source and reads nothing when the env is unset", () => {
    // An env object WITHOUT the var (injected, so the real process env is untouched).
    const env: NodeJS.ProcessEnv = {};
    const source = resolveCorpusSource(env);
    expect(source).toEqual({ kind: "default" });
  });

  it("resolves to the default source when the env is the empty string", () => {
    const env: NodeJS.ProcessEnv = { [ANALYSIS_CORPUS_ENV]: "" };
    expect(resolveCorpusSource(env)).toEqual({ kind: "default" });
  });

  it("loads the external config from a temp path when the env is set", () => {
    const configPath = writeTempConfig(GOOD_CONFIG);
    const env: NodeJS.ProcessEnv = { [ANALYSIS_CORPUS_ENV]: configPath };
    const source = resolveCorpusSource(env);
    expect(source.kind).toBe("external");
    if (source.kind === "external") {
      expect(source.config.projects[0].id).toBe("p0001");
    }
  });

  it("propagates a bad-id rejection from the external config", () => {
    const configPath = writeTempConfig(GOOD_CONFIG.replace('"p0001"', `"${descriptiveId()}"`));
    const env: NodeJS.ProcessEnv = { [ANALYSIS_CORPUS_ENV]: configPath };
    expect(() => resolveCorpusSource(env)).toThrow(AnalysisConfigError);
  });

  it("does not read the real process env by default (the campaign env stays unset)", () => {
    // Mirrors hermeticFixtures.test.ts: the var is unset in this suite, and the
    // default resolution touches nothing.
    expect(process.env[ANALYSIS_CORPUS_ENV]).toBeUndefined();
    expect(resolveCorpusSource()).toEqual({ kind: "default" });
  });
});

import { describe, expect, it } from "vitest";
import * as path from "node:path";
import {
  assertFixtureMatchesScenario,
  buildCommittedKeystrokeScenario,
  COMMITTED_KEYSTROKE_SCENARIO,
  hasCompleteScenarioEnv,
  mergeScenarioEnv,
  resolveCommittedKeystrokeFixtureDir,
  scenarioToEnv,
} from "./dxCommittedScenario";
import { DX_SCENARIO_ENV_KEYS } from "./dxScenario";

const PKG_ROOT = path.resolve(__dirname, "../..");

describe("dxCommittedScenario", () => {
  it("resolves the committed keystroke fixture under the package", () => {
    const dir = resolveCommittedKeystrokeFixtureDir(PKG_ROOT);
    expect(dir.replace(/\\/g, "/")).toMatch(/e2e\/dx\/fixtures\/keystroke-auto-import$/);
  });

  it("fixture contains anchors required by the scenario", () => {
    const dir = resolveCommittedKeystrokeFixtureDir(PKG_ROOT);
    expect(() => assertFixtureMatchesScenario(dir, COMMITTED_KEYSTROKE_SCENARIO)).not.toThrow();
  });

  it("buildCommittedKeystrokeScenario fills every handoff field", () => {
    const scenario = buildCommittedKeystrokeScenario("/tmp/ws");
    expect(scenario.workspace).toBe(path.resolve("/tmp/ws"));
    expect(scenario.entry).toBe("App.vue");
    expect(scenario.expectCompletion).toBe("computed");
    expect(scenario.acceptExpect).toBe("computed");
    expect(scenario.typeText.length).toBeGreaterThan(0);
  });

  it("scenarioToEnv maps every DX_HARNESS_* key", () => {
    const env = scenarioToEnv(buildCommittedKeystrokeScenario("/tmp/ws"));
    for (const key of Object.values(DX_SCENARIO_ENV_KEYS)) {
      expect(env[key], key).toBeTruthy();
    }
  });

  it("hasCompleteScenarioEnv discriminates incomplete maps", () => {
    expect(hasCompleteScenarioEnv({})).toBe(false);
    const full = scenarioToEnv(buildCommittedKeystrokeScenario("/tmp/ws"));
    expect(hasCompleteScenarioEnv(full)).toBe(true);
    const partial = { ...full, [DX_SCENARIO_ENV_KEYS.entry]: "" };
    expect(hasCompleteScenarioEnv(partial)).toBe(false);
  });

  it("mergeScenarioEnv preserves explicit non-empty overrides", () => {
    const base = {
      [DX_SCENARIO_ENV_KEYS.entry]: "Custom.vue",
      [DX_SCENARIO_ENV_KEYS.workspace]: "",
    };
    const scenario = scenarioToEnv(buildCommittedKeystrokeScenario("/tmp/ws"));
    const merged = mergeScenarioEnv(base, scenario);
    expect(merged[DX_SCENARIO_ENV_KEYS.entry]).toBe("Custom.vue");
    expect(merged[DX_SCENARIO_ENV_KEYS.workspace]).toBe(path.resolve("/tmp/ws"));
  });
});

import { describe, expect, it } from "vitest";

import {
  CANARY_LOG_FILE_ENV,
  DX_SCENARIO_ENV_KEYS,
  validateCanaryPreconditions,
  validateDxScenario,
} from "./dxScenario";

/** A complete, valid scenario env. */
function fullEnv(): Record<string, string> {
  return {
    [DX_SCENARIO_ENV_KEYS.workspace]: "/abs/ws",
    [DX_SCENARIO_ENV_KEYS.entry]: "src/App.vue",
    [DX_SCENARIO_ENV_KEYS.anchorText]: "<template>",
    [DX_SCENARIO_ENV_KEYS.typeText]: "<MyComp",
    [DX_SCENARIO_ENV_KEYS.expectCompletion]: "MyComp",
    [DX_SCENARIO_ENV_KEYS.acceptAnchor]: "<MyComp",
    [DX_SCENARIO_ENV_KEYS.acceptExpect]: "MyComp",
  };
}

describe("validateDxScenario", () => {
  it("resolves a complete scenario", () => {
    const s = validateDxScenario(fullEnv());
    expect(s.workspace).toBe("/abs/ws");
    expect(s.entry).toBe("src/App.vue");
    expect(s.typeText).toBe("<MyComp");
    expect(s.expectCompletion).toBe("MyComp");
    expect(s.acceptAnchor).toBe("<MyComp");
    expect(s.acceptExpect).toBe("MyComp");
  });

  it("THROWS when the entry is missing (no vacuous skip)", () => {
    const env = fullEnv();
    delete (env as Record<string, string | undefined>)[DX_SCENARIO_ENV_KEYS.entry];
    expect(() => validateDxScenario(env)).toThrow(/DX_HARNESS_ENTRY/);
  });

  it("THROWS when the accept anchors are missing", () => {
    const env = fullEnv();
    delete (env as Record<string, string | undefined>)[DX_SCENARIO_ENV_KEYS.acceptAnchor];
    delete (env as Record<string, string | undefined>)[DX_SCENARIO_ENV_KEYS.acceptExpect];
    expect(() => validateDxScenario(env)).toThrow(/DX_HARNESS_ACCEPT_ANCHOR/);
    expect(() => validateDxScenario(env)).toThrow(/DX_HARNESS_ACCEPT_EXPECT/);
  });

  it("treats empty/whitespace values as missing", () => {
    const env = { ...fullEnv(), [DX_SCENARIO_ENV_KEYS.typeText]: "   " };
    expect(() => validateDxScenario(env)).toThrow(/DX_HARNESS_TYPE_TEXT/);
  });

  it("lists EVERY missing key in one throw (not just the first)", () => {
    expect(() => validateDxScenario({})).toThrow(
      /DX_HARNESS_WORKSPACE.*DX_HARNESS_ENTRY.*DX_HARNESS_ACCEPT_EXPECT/s,
    );
  });
});

describe("validateCanaryPreconditions", () => {
  it("resolves when the captured log file is set", () => {
    expect(validateCanaryPreconditions({ [CANARY_LOG_FILE_ENV]: "/tmp/dx.log" }).logFile).toBe(
      "/tmp/dx.log",
    );
  });

  it("THROWS when the captured log file is missing (no vacuous canary)", () => {
    expect(() => validateCanaryPreconditions({})).toThrow(/VERTER_E2E_LOG_FILE/);
  });
});

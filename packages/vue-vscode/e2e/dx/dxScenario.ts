/**
 * The extension-host DX scenario handoff contract.
 *
 * The in-host gate is corpus-agnostic: the concrete scenario (entry file, the typing
 * anchor, the characters to type, the completion that must appear, the accept anchor,
 * and the label the real accept must land) is handed in via `DX_HARNESS_*` env by the
 * launching harness, which owns the materialized workspace and its anchor map.
 *
 * Because the gate is env-driven, a misconfigured run is the danger: if scenario env
 * is missing, per-test `this.skip()` would let a requested `VERTER_E2E_DX=1` run report
 * success while exercising NOTHING. {@link validateDxScenario} closes that hole — it is
 * the single precondition oracle the suite calls in `suiteSetup`, and it FAILS HARD
 * (listing every missing key) so a requested DX run cannot pass with its gates skipped.
 *
 * Pure (reads an injected env map), so it is unit-tested without VS Code.
 */

/** The fully-resolved scenario every main DX gate needs. */
export interface DxScenario {
  /** Absolute materialized workspace root. */
  readonly workspace: string;
  /** Workspace-relative `.vue` entry file the gates open. */
  readonly entry: string;
  /** Text to locate in `entry`; typing starts at its end. */
  readonly anchorText: string;
  /** Characters typed one-by-one to drive incremental completions. */
  readonly typeText: string;
  /** Completion label that MUST appear after typing `typeText`. */
  readonly expectCompletion: string;
  /** Text to locate in `entry`; the accept cursor sits at its end. */
  readonly acceptAnchor: string;
  /** The label the real accept must rank first and land (the auto-import). */
  readonly acceptExpect: string;
}

/** The env var carrying each {@link DxScenario} field. */
export const DX_SCENARIO_ENV_KEYS = {
  workspace: "DX_HARNESS_WORKSPACE",
  entry: "DX_HARNESS_ENTRY",
  anchorText: "DX_HARNESS_ANCHOR_TEXT",
  typeText: "DX_HARNESS_TYPE_TEXT",
  expectCompletion: "DX_HARNESS_EXPECT_COMPLETION",
  acceptAnchor: "DX_HARNESS_ACCEPT_ANCHOR",
  acceptExpect: "DX_HARNESS_ACCEPT_EXPECT",
} as const satisfies Record<keyof DxScenario, string>;

/**
 * Resolve the complete DX scenario from an env map, or THROW listing every missing
 * key. A requested `VERTER_E2E_DX=1` run calls this in `suiteSetup`, so an incomplete
 * handoff fails the run loudly instead of silently skipping the gates.
 */
export function validateDxScenario(env: Record<string, string | undefined>): DxScenario {
  const missing: string[] = [];
  const read = (key: string): string => {
    const value = env[key];
    if (value === undefined || value.trim() === "") {
      missing.push(key);
      return "";
    }
    return value;
  };

  const scenario: DxScenario = {
    workspace: read(DX_SCENARIO_ENV_KEYS.workspace),
    entry: read(DX_SCENARIO_ENV_KEYS.entry),
    anchorText: read(DX_SCENARIO_ENV_KEYS.anchorText),
    typeText: read(DX_SCENARIO_ENV_KEYS.typeText),
    expectCompletion: read(DX_SCENARIO_ENV_KEYS.expectCompletion),
    acceptAnchor: read(DX_SCENARIO_ENV_KEYS.acceptAnchor),
    acceptExpect: read(DX_SCENARIO_ENV_KEYS.acceptExpect),
  };

  if (missing.length > 0) {
    throw new Error(
      `DX scenario handoff incomplete — a VERTER_E2E_DX=1 run requires the full scenario, but ` +
        `these env vars are missing/empty: ${missing.join(", ")}. The launching harness writes ` +
        `them from the materialized workspace's anchor map; set them so startup-readiness, ` +
        `per-character typing, and the real accept path all run. A requested DX run must not ` +
        `pass with gates skipped.`,
    );
  }
  return scenario;
}

/** The env var carrying the canary's captured-log file path. */
export const CANARY_LOG_FILE_ENV = "VERTER_E2E_LOG_FILE";

/** Preconditions the dedicated canary launch needs (it forces its own provider state). */
export interface CanaryPreconditions {
  /** The captured extension log file the canary reads. */
  readonly logFile: string;
}

/**
 * Resolve the canary preconditions, or THROW. The canary forces an unavailable
 * provider itself, so it needs no scenario anchors — but it must have a captured log
 * file to read, else its verdict would be vacuous.
 */
export function validateCanaryPreconditions(
  env: Record<string, string | undefined>,
): CanaryPreconditions {
  const logFile = env[CANARY_LOG_FILE_ENV];
  if (logFile === undefined || logFile.trim() === "") {
    throw new Error(
      `DX canary launch requires ${CANARY_LOG_FILE_ENV} (the captured extension log the canary ` +
        `reads to verify the forced server WARN reached the file).`,
    );
  }
  return { logFile };
}

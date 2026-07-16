/**
 * Committed keystroke DX scenario producer.
 *
 * The real VS Code DX launch needs a complete `DX_HARNESS_*` handoff. CI previously
 * could not run `test:e2e:dx` because no producer existed. This module owns the
 * hermetic default: copy the committed keystroke fixture into a temp workspace,
 * install vue types if needed, and emit the full scenario env map.
 *
 * Pure helpers are unit-tested; I/O helpers are used by the launcher.
 */
import {
  cpSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import * as path from "node:path";
import { execSync } from "node:child_process";

import { DX_SCENARIO_ENV_KEYS, type DxScenario } from "./dxScenario";

/** Relative path of the committed keystroke fixture under `e2e/dx/fixtures/`. */
export const KEYSTROKE_FIXTURE_DIR = "keystroke-auto-import";

/** Default scenario field values for the committed auto-import keystroke gate. */
export const COMMITTED_KEYSTROKE_SCENARIO = {
  entry: "App.vue",
  /** Text ending where typing begins (empty line after comment). */
  anchorText: "// ANCHOR_TYPE\n",
  /** Characters typed one-by-one (must surface `computed`). */
  typeText: "const doubled = comput",
  expectCompletion: "computed",
  /** Accept cursor at end of this prefix (incomplete `comput`). */
  acceptAnchor: "const doubled = comput",
  acceptExpect: "computed",
} as const;

/**
 * Build a {@link DxScenario} from an absolute workspace root + committed defaults.
 */
export function buildCommittedKeystrokeScenario(workspace: string): DxScenario {
  return {
    workspace: path.resolve(workspace),
    entry: COMMITTED_KEYSTROKE_SCENARIO.entry,
    anchorText: COMMITTED_KEYSTROKE_SCENARIO.anchorText,
    typeText: COMMITTED_KEYSTROKE_SCENARIO.typeText,
    expectCompletion: COMMITTED_KEYSTROKE_SCENARIO.expectCompletion,
    acceptAnchor: COMMITTED_KEYSTROKE_SCENARIO.acceptAnchor,
    acceptExpect: COMMITTED_KEYSTROKE_SCENARIO.acceptExpect,
  };
}

/** Convert a scenario into the env map the extension host reads. */
export function scenarioToEnv(scenario: DxScenario): Record<string, string> {
  return {
    [DX_SCENARIO_ENV_KEYS.workspace]: scenario.workspace,
    [DX_SCENARIO_ENV_KEYS.entry]: scenario.entry,
    [DX_SCENARIO_ENV_KEYS.anchorText]: scenario.anchorText,
    [DX_SCENARIO_ENV_KEYS.typeText]: scenario.typeText,
    [DX_SCENARIO_ENV_KEYS.expectCompletion]: scenario.expectCompletion,
    [DX_SCENARIO_ENV_KEYS.acceptAnchor]: scenario.acceptAnchor,
    [DX_SCENARIO_ENV_KEYS.acceptExpect]: scenario.acceptExpect,
  };
}

/**
 * Resolve the on-disk committed fixture directory given the `vue-vscode` package root.
 */
export function resolveCommittedKeystrokeFixtureDir(extensionDevelopmentPath: string): string {
  const dir = path.join(extensionDevelopmentPath, "e2e", "dx", "fixtures", KEYSTROKE_FIXTURE_DIR);
  if (!existsSync(path.join(dir, "App.vue"))) {
    throw new Error(`committed keystroke fixture missing App.vue at ${dir}`);
  }
  return dir;
}

/**
 * Assert the fixture contains the anchors the scenario relies on (discrimination).
 */
export function assertFixtureMatchesScenario(
  fixtureDir: string,
  scenario: Pick<DxScenario, "entry" | "anchorText" | "typeText" | "acceptAnchor">,
): void {
  const source = readFileSync(path.join(fixtureDir, scenario.entry), "utf8");
  // Type site is a committed marker; accept text is produced by typing typeText.
  if (!source.includes("// ANCHOR_TYPE")) {
    throw new Error(`fixture ${scenario.entry} missing // ANCHOR_TYPE type site`);
  }
  if (!scenario.typeText.includes("comput")) {
    throw new Error(`scenario typeText must drive the incomplete "comput" prefix`);
  }
  if (
    scenario.acceptAnchor !== scenario.typeText &&
    !scenario.typeText.endsWith(scenario.acceptAnchor)
  ) {
    // Accept cursor sits at end of acceptAnchor after typing; usually equal to typeText.
    throw new Error(
      `acceptAnchor should match typed prefix (got accept=${scenario.acceptAnchor}, type=${scenario.typeText})`,
    );
  }
}

export interface PreparedKeystrokeWorkspace {
  readonly workspace: string;
  readonly scenario: DxScenario;
  readonly env: Record<string, string>;
  /** Remove the temp workspace (best-effort). */
  readonly dispose: () => void;
}

/**
 * Copy the committed fixture to a temp dir, optionally npm-install vue, return scenario+env.
 */
export function prepareCommittedKeystrokeWorkspace(
  extensionDevelopmentPath: string,
  options?: { installDeps?: boolean; keep?: boolean },
): PreparedKeystrokeWorkspace {
  const source = resolveCommittedKeystrokeFixtureDir(extensionDevelopmentPath);
  assertFixtureMatchesScenario(source, COMMITTED_KEYSTROKE_SCENARIO);

  const workspace = mkdtempSync(path.join(tmpdir(), "verter-dx-keystroke-"));
  cpSync(source, workspace, { recursive: true });

  // Ensure .vscode exists for settings writer.
  mkdirSync(path.join(workspace, ".vscode"), { recursive: true });

  if (options?.installDeps !== false) {
    const nodeModules = path.join(workspace, "node_modules");
    if (!existsSync(nodeModules)) {
      execSync("npm install --no-package-lock --ignore-scripts", {
        cwd: workspace,
        stdio: "pipe",
        timeout: 120_000,
      });
    }
  }

  const scenario = buildCommittedKeystrokeScenario(workspace);
  const env = scenarioToEnv(scenario);

  return {
    workspace,
    scenario,
    env,
    dispose: () => {
      if (options?.keep) return;
      try {
        rmSync(workspace, { recursive: true, force: true });
      } catch {
        /* best-effort */
      }
    },
  };
}

/**
 * Merge prepared scenario env into `process.env` (or a provided map).
 * Existing non-empty DX_HARNESS_* keys win (explicit override).
 */
export function mergeScenarioEnv(
  base: Record<string, string | undefined>,
  scenarioEnv: Record<string, string>,
): Record<string, string | undefined> {
  const out: Record<string, string | undefined> = { ...base };
  for (const [key, value] of Object.entries(scenarioEnv)) {
    const existing = out[key];
    if (existing === undefined || existing.trim() === "") {
      out[key] = value;
    }
  }
  return out;
}

/** True when every DX_HARNESS_* scenario key is already set non-empty. */
export function hasCompleteScenarioEnv(env: Record<string, string | undefined>): boolean {
  return Object.values(DX_SCENARIO_ENV_KEYS).every((key) => {
    const v = env[key];
    return v !== undefined && v.trim() !== "";
  });
}

/** Write a tiny marker file so CI logs can prove the producer ran. */
export function writeProducerReceipt(workspace: string, scenario: DxScenario): string {
  const receiptPath = path.join(workspace, ".verter-dx-scenario.json");
  writeFileSync(
    receiptPath,
    JSON.stringify(
      {
        producer: "dxCommittedScenario",
        entry: scenario.entry,
        expectCompletion: scenario.expectCompletion,
        acceptExpect: scenario.acceptExpect,
      },
      null,
      2,
    ),
    "utf8",
  );
  return receiptPath;
}

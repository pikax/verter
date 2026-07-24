/**
 * Node-side launcher for the extension-host DX driver.
 *
 * This is the plain-Node entry point invoked by `pnpm --filter verter-vscode
 * test:e2e:dx`. It is the ONLY DX module allowed to touch `@verter/dx-harness`
 * (the harness is ESM; the in-host suite compiles CommonJS and must stay a thin VS
 * Code actuator — see `dxScenarioRunner.ts`). The launcher:
 *
 *   1. validates an ABSOLUTE `DX_HARNESS_WORKSPACE` (the materialized workspace),
 *   2. writes + verifies `.vscode/settings.json` with `verter.server.logLevel:
 *      "debug"` BEFORE launch (the extension copies that setting into the server's
 *      `VERTER_LOG`, default "info", so the readiness/log gates need it),
 *   3. launches real VS Code with the materialized workspace as the workspace
 *      folder, and NEVER runs a fixture `npm install` — the workspace is already
 *      materialized with vendored shims.
 *
 * The pure helpers (`validateWorkspaceArg`, `ensureDebugLogLevel`,
 * `assertDebugLogLevel`, `buildDxLaunch`) carry all the gate logic and are unit
 * tested with no real VS Code; `main()` is the thin wiring exercised by the
 * env-gated real launch.
 */
import { existsSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from "fs";
import * as os from "os";
import * as path from "path";

import { copyLspBinaryToTemp, readE2eEnv, resolveVscodeExecutablePath } from "../sharedLaunch";
import { CANARY_TYPE_PROVIDER } from "./dxLogCanary";
import { importEsm } from "./esmImport";

/** The VS Code setting the extension copies into the server's `VERTER_LOG`. */
export const LOG_LEVEL_SETTING = "verter.server.logLevel";

/** The setting selecting the TypeScript type provider; the canary pins it to `off`. */
export const TYPE_PROVIDER_SETTING = "verter.typeProvider";

/** The setting enabling the MCP endpoint; the canary pins it true so `--mcp-port=0` is passed. */
export const MCP_ENABLED_SETTING = "verter.mcp.enabled";

/**
 * Env var carrying the materialized workspace root to the extension host. Mirrors
 * `@verter/dx-harness`'s `DX_HARNESS_WORKSPACE_ENV`; `main()` asserts the two agree
 * so the value cannot silently drift from the harness contract.
 */
export const DX_HARNESS_WORKSPACE_ENV = "DX_HARNESS_WORKSPACE";

/**
 * Env var marking the isolated canary launch. The in-host suite runs ONLY the log
 * canary under this flag (it forces the MCP config so the server emits its deterministic
 * MCP-deprecation WARN, a different launch from the main gates), and `suiteSetup`
 * requires the canary preconditions instead of the full scenario.
 */
export const CANARY_MODE_ENV = "VERTER_E2E_DX_CANARY";

/**
 * Validate the workspace handoff argument: it must be a non-empty ABSOLUTE path to
 * an existing directory. Returns the normalized absolute path.
 */
export function validateWorkspaceArg(value: string | undefined): string {
  if (!value) {
    throw new Error(
      `${DX_HARNESS_WORKSPACE_ENV} is required: pass the absolute materialized workspace root`,
    );
  }
  if (!path.isAbsolute(value)) {
    throw new Error(
      `${DX_HARNESS_WORKSPACE_ENV} must be an absolute path, got: ${JSON.stringify(value)}`,
    );
  }
  if (!existsSync(value)) {
    throw new Error(`${DX_HARNESS_WORKSPACE_ENV} does not exist: ${value}`);
  }
  if (!statSync(value).isDirectory()) {
    throw new Error(`${DX_HARNESS_WORKSPACE_ENV} is not a directory: ${value}`);
  }
  return path.resolve(value);
}

/**
 * Return a settings object guaranteed to pin `verter.server.logLevel: "debug"`,
 * preserving every other setting (e.g. the materializer's tsdk/provider pins).
 * `changed` reports whether the input already had debug logging.
 */
export function ensureDebugLogLevel(settings: Record<string, unknown>): {
  settings: Record<string, unknown>;
  changed: boolean;
} {
  const changed = settings[LOG_LEVEL_SETTING] !== "debug";
  return { settings: { ...settings, [LOG_LEVEL_SETTING]: "debug" }, changed };
}

/** Throw unless `settings` pins `verter.server.logLevel: "debug"`. */
export function assertDebugLogLevel(settings: Record<string, unknown>): void {
  const level = settings[LOG_LEVEL_SETTING];
  if (level !== "debug") {
    throw new Error(
      `${LOG_LEVEL_SETTING} must be "debug" before launch (the extension copies it into ` +
        `VERTER_LOG); got ${JSON.stringify(level)}`,
    );
  }
}

/** Read `<root>/.vscode/settings.json` as an object, or `{}` if it is absent. */
export function readWorkspaceSettings(root: string): Record<string, unknown> {
  const settingsPath = path.join(root, ".vscode", "settings.json");
  if (!existsSync(settingsPath)) return {};
  const parsed: unknown = JSON.parse(readFileSync(settingsPath, "utf-8"));
  return parsed && typeof parsed === "object" ? (parsed as Record<string, unknown>) : {};
}

/** Inputs to {@link buildDxLaunch}. */
export interface BuildDxLaunchOptions {
  /** Absolute materialized workspace root (already validated). */
  workspace: string;
  /** The extension development path (the `verter-vscode` package root). */
  extensionDevelopmentPath: string;
  /** Compiled DX mocha entry (e.g. `out-test/e2e/dx/index`). */
  extensionTestsPath: string;
  /** Resolved VS Code executable path. */
  vscodeExecutablePath: string;
  /** Extension-host log file the readiness/canary gates read. */
  logFile: string;
  /** Base environment to inherit (usually `process.env`). */
  baseEnv: Record<string, string | undefined>;
  /** Optional copied LSP binary path. */
  lspBinaryPath?: string;
  /** Optional type provider override. */
  typeProvider?: string;
  /** Optional startup-timing output file. */
  timingFile?: string;
  /**
   * Isolated canary launch: runs ONLY the log canary and pins the type provider to
   * {@link CANARY_TYPE_PROVIDER} so the captured `--type-provider=off` proves provider
   * discovery is not the trigger; the forced MCP config makes the server emit its
   * deterministic MCP-deprecation WARN.
   */
  canary?: boolean;
}

/** The `runTests` inputs the launcher computes (kept pure for testing). */
export interface DxLaunch {
  vscodeExecutablePath: string;
  extensionDevelopmentPath: string;
  extensionTestsPath: string;
  launchArgs: string[];
  extensionTestsEnv: Record<string, string | undefined>;
}

/**
 * Assemble the VS Code launch args + env for the DX run. The materialized
 * workspace is the workspace folder (last positional arg); the env carries the
 * `DX_HARNESS_WORKSPACE` handoff, debug logging, and a dx-only test filter. It
 * declares NO fixture (so in-host helpers never treat the workspace as a named
 * fixture) — `VERTER_E2E_FIXTURE` is explicitly scrubbed so an inherited value
 * cannot leak through — and triggers NO install.
 *
 * When `canary`, it marks the isolated canary launch and pins the provider to
 * {@link CANARY_TYPE_PROVIDER} (overriding any `typeProvider`), and also exports it as
 * `VERTER_E2E_TYPE_PROVIDER` so inherited env cannot override the workspace setting —
 * the captured `--type-provider=off` is the proof that provider discovery is not the trigger.
 */
export function buildDxLaunch(opts: BuildDxLaunchOptions): DxLaunch {
  const launchArgs = ["--disable-extensions", "--disable-updates", opts.workspace];
  // In the canary launch the provider is pinned to `off` (proving provider discovery is
  // not the trigger); otherwise honour the caller's optional override.
  const typeProvider = opts.canary ? CANARY_TYPE_PROVIDER : opts.typeProvider;

  const extensionTestsEnv: Record<string, string | undefined> = {
    ...opts.baseEnv,
    VERTER_E2E_TEST: "1",
    VERTER_E2E_DX: "1",
    VERTER_E2E_LOG_FILE: opts.logFile,
    VERTER_E2E_ONLY: "dx-harness",
    VERTER_LOG: "debug",
    // The DX path consumes a materialized workspace, never a named fixture; scrub any
    // inherited fixture var so in-host helpers cannot treat the workspace as one.
    VERTER_E2E_FIXTURE: undefined,
    [DX_HARNESS_WORKSPACE_ENV]: opts.workspace,
    [CANARY_MODE_ENV]: opts.canary ? "1" : undefined,
    ...(opts.timingFile ? { VERTER_E2E_TIMING_FILE: opts.timingFile } : {}),
    ...(opts.lspBinaryPath ? { VERTER_E2E_LSP_PATH: opts.lspBinaryPath } : {}),
    ...(typeProvider ? { VERTER_E2E_TYPE_PROVIDER: typeProvider } : {}),
  };

  return {
    vscodeExecutablePath: opts.vscodeExecutablePath,
    extensionDevelopmentPath: opts.extensionDevelopmentPath,
    extensionTestsPath: opts.extensionTestsPath,
    launchArgs,
    extensionTestsEnv,
  };
}

/** Writes `<root>/.vscode/settings.json` from a settings object (the harness writer). */
export type WorkspaceSettingsWriter = (root: string, settings: Record<string, unknown>) => void;

/** The slice of `@verter/dx-harness` the launcher consumes at runtime. */
interface DxHarnessModule {
  writeWorkspaceSettings(
    root: string,
    opts?: { settings?: Record<string, unknown> },
  ): { settingsPath: string; settings: Record<string, unknown> };
  DX_HARNESS_WORKSPACE_ENV: string;
}

// The harness is ESM-only and its `dist` is gitignored; a non-literal specifier
// keeps the CommonJS e2e typecheck independent of a built harness (the real launch
// resolves it from the workspace link). The boundary rule forbids the IN-HOST
// suite from importing the harness — the Node-side launcher is explicitly the
// place that may. It is loaded through `importEsm` so the genuine `import()` survives
// the CommonJS emit (a bare `await import` would downlevel to `require` and throw
// `ERR_REQUIRE_ESM` against this ESM-only package).
const HARNESS_SPECIFIER = "@verter/dx-harness";

/** Load the ESM harness and assert its workspace-env key matches the launcher's. */
async function loadHarness(): Promise<DxHarnessModule> {
  const harness = await importEsm<DxHarnessModule>(HARNESS_SPECIFIER);
  if (harness.DX_HARNESS_WORKSPACE_ENV !== DX_HARNESS_WORKSPACE_ENV) {
    throw new Error(
      `DX workspace env key drift: launcher=${DX_HARNESS_WORKSPACE_ENV} ` +
        `harness=${harness.DX_HARNESS_WORKSPACE_ENV}`,
    );
  }
  return harness;
}

/** Adapt the harness `writeWorkspaceSettings` to the {@link WorkspaceSettingsWriter} shape. */
function harnessSettingsWriter(harness: DxHarnessModule): WorkspaceSettingsWriter {
  return (root, settings) => {
    harness.writeWorkspaceSettings(root, { settings });
  };
}

/**
 * Ensure `<root>/.vscode/settings.json` pins debug logging, writing through the
 * injected `writeSettings` authority (preserving other pins), then RE-READ the
 * written file and assert the level stuck. Throws if the writer dropped the level —
 * the read→write→reread→assert round trip is the guarantee, so a writer that fails
 * to persist `verter.server.logLevel` is caught before launch. Returns the verified
 * settings. `writeSettings` is injected so the round trip is unit-testable.
 */
export function ensureAndVerifyDebugSettings(
  workspace: string,
  writeSettings: WorkspaceSettingsWriter,
): Record<string, unknown> {
  const existing = readWorkspaceSettings(workspace);
  const { settings: merged } = ensureDebugLogLevel(existing);
  writeSettings(workspace, merged);
  const written = readWorkspaceSettings(workspace);
  assertDebugLogLevel(written);
  return written;
}

/**
 * The deterministic, provider-independent forcing trigger for the canary: pin
 * `verter.mcp.enabled=true` (so `buildServerOptions` passes `--mcp-port=0`) and
 * `verter.typeProvider=off` (so it passes `--type-provider=off`, proving provider
 * discovery is not involved). The server then emits its unconditional MCP-deprecation
 * WARN on every run (crates/verter_lsp/src/main.rs:63). Debug logging stays on so the
 * `[buildServerOptions]` proof line is captured.
 */
export function buildCanaryForcingSettings(): Record<string, unknown> {
  return {
    [LOG_LEVEL_SETTING]: "debug",
    [MCP_ENABLED_SETTING]: true,
    [TYPE_PROVIDER_SETTING]: CANARY_TYPE_PROVIDER,
  };
}

/** Run the assembled launch in real VS Code. Tooling import is lazy + CJS-resolvable. */
async function runVsCodeTests(launch: DxLaunch): Promise<void> {
  const { runTests } = await import("@vscode/test-electron");
  await runTests({
    vscodeExecutablePath: launch.vscodeExecutablePath,
    extensionDevelopmentPath: launch.extensionDevelopmentPath,
    extensionTestsPath: launch.extensionTestsPath,
    launchArgs: launch.launchArgs,
    extensionTestsEnv: launch.extensionTestsEnv,
  });
}

/** Launch real VS Code against the materialized workspace and run the main DX gates. */
export async function main(): Promise<void> {
  const harness = await loadHarness();

  const extensionDevelopmentPath = path.resolve(__dirname, "../../../");
  const extensionTestsPath = path.resolve(__dirname, "./index");

  // Prefer an explicit DX_HARNESS_* handoff; otherwise produce the committed
  // keystroke auto-import scenario so CI can run always-on without a manual producer.
  const {
    hasCompleteScenarioEnv,
    mergeScenarioEnv,
    prepareCommittedKeystrokeWorkspace,
    writeProducerReceipt,
  } = await import("./dxCommittedScenario.js");

  let disposeWorkspace: (() => void) | undefined;
  let baseEnv: Record<string, string | undefined> = { ...process.env };

  let workspace: string;
  if (hasCompleteScenarioEnv(process.env) && process.env[DX_HARNESS_WORKSPACE_ENV]) {
    workspace = validateWorkspaceArg(process.env[DX_HARNESS_WORKSPACE_ENV]);
    console.log(`DX scenario: using explicit DX_HARNESS_* handoff`);
  } else {
    const prepared = prepareCommittedKeystrokeWorkspace(extensionDevelopmentPath, {
      installDeps: true,
    });
    disposeWorkspace = prepared.dispose;
    workspace = prepared.workspace;
    baseEnv = mergeScenarioEnv(baseEnv, prepared.env);
    writeProducerReceipt(workspace, prepared.scenario);
    console.log(`DX scenario: committed keystroke producer → ${workspace}`);
  }

  try {
    ensureAndVerifyDebugSettings(workspace, harnessSettingsWriter(harness));

    const vscodeVersion = readE2eEnv("VSCODE_VERSION") ?? "stable";
    const vscodeExecutablePath = await resolveVscodeExecutablePath(vscodeVersion, {
      explicitExecutablePath: readE2eEnv("VSCODE_EXECUTABLE"),
    });
    const lspBinaryPath = copyLspBinaryToTemp(extensionDevelopmentPath);

    const logFile =
      readE2eEnv("LOG_FILE") ?? path.join(os.tmpdir(), `verter-e2e-dx-${process.pid}.log`);
    const timingFile =
      readE2eEnv("TIMING_FILE") ??
      path.join(os.tmpdir(), `verter-e2e-dx-timing-${process.pid}.json`);

    const launch = buildDxLaunch({
      workspace,
      extensionDevelopmentPath,
      extensionTestsPath,
      vscodeExecutablePath,
      logFile,
      baseEnv,
      lspBinaryPath,
      typeProvider: readE2eEnv("TYPE_PROVIDER") ?? "tsserver",
      timingFile,
    });

    // Ensure scenario env is present on the extension host even when produced.
    Object.assign(launch.extensionTestsEnv, {
      ...Object.fromEntries(Object.entries(baseEnv).filter(([k]) => k.startsWith("DX_HARNESS_"))),
      [DX_HARNESS_WORKSPACE_ENV]: workspace,
    });

    console.log(`DX extension-host run`);
    console.log(`  workspace: ${workspace}`);
    console.log(`  logFile:   ${logFile}`);

    await runVsCodeTests(launch);
  } finally {
    disposeWorkspace?.();
  }
}

/**
 * The isolated canary launch (`--canary`). It materializes a throwaway workspace, forces
 * the MCP config (mcp.enabled=true + typeProvider=off) so the server emits its
 * deterministic MCP-deprecation WARN, and runs ONLY the in-host canary test. Kept
 * separate from {@link main} because the canary needs a different launch config from the
 * main gates — one launch cannot be both. The throwaway workspace is removed afterward.
 */
export async function mainCanary(): Promise<void> {
  const harness = await loadHarness();

  const extensionDevelopmentPath = path.resolve(__dirname, "../../../");
  const extensionTestsPath = path.resolve(__dirname, "./index");

  // A throwaway workspace gives the extension a `.vue` document to activate on; the
  // forcing is the MCP config (mcp.enabled=true + typeProvider=off), not the workspace
  // contents, so it needs no special TypeScript state. Removed in `finally`.
  const workspace = mkdtempSync(path.join(os.tmpdir(), "verter-dx-canary-ws-"));
  try {
    writeFileSync(
      path.join(workspace, "App.vue"),
      '<template><div>dx canary</div></template>\n<script setup lang="ts"></script>\n',
    );
    ensureAndVerifyDebugSettings(workspace, (root, settings) => {
      harness.writeWorkspaceSettings(root, {
        settings: { ...settings, ...buildCanaryForcingSettings() },
      });
    });

    const vscodeVersion = readE2eEnv("VSCODE_VERSION") ?? "stable";
    const vscodeExecutablePath = await resolveVscodeExecutablePath(vscodeVersion, {
      explicitExecutablePath: readE2eEnv("VSCODE_EXECUTABLE"),
    });
    const lspBinaryPath = copyLspBinaryToTemp(extensionDevelopmentPath);
    const logFile = path.join(os.tmpdir(), `verter-e2e-dx-canary-${process.pid}.log`);

    const launch = buildDxLaunch({
      workspace,
      extensionDevelopmentPath,
      extensionTestsPath,
      vscodeExecutablePath,
      logFile,
      baseEnv: process.env,
      lspBinaryPath,
      canary: true,
    });

    console.log(
      `DX log-canary run (forced MCP config: mcp.enabled=true, typeProvider=${CANARY_TYPE_PROVIDER})`,
    );
    console.log(`  workspace: ${workspace}`);
    console.log(`  logFile:   ${logFile}`);

    await runVsCodeTests(launch);
  } finally {
    rmSync(workspace, { recursive: true, force: true });
  }
}

// Run when invoked directly (`node out-test/e2e/dx/dxLauncher.js[ --canary]`), not
// when imported by the unit tests.
if (require.main === module) {
  const isCanary = process.argv.includes("--canary") || process.env[CANARY_MODE_ENV] === "1";
  (isCanary ? mainCanary() : main()).catch((err) => {
    console.error(`DX extension-host ${isCanary ? "canary " : ""}runner failed:`, err);
    process.exit(1);
  });
}

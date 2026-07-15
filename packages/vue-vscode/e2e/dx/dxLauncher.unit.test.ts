import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import { CANARY_TYPE_PROVIDER } from "./dxLogCanary";
import {
  assertDebugLogLevel,
  buildCanaryForcingSettings,
  buildDxLaunch,
  CANARY_MODE_ENV,
  DX_HARNESS_WORKSPACE_ENV,
  ensureAndVerifyDebugSettings,
  ensureDebugLogLevel,
  LOG_LEVEL_SETTING,
  MCP_ENABLED_SETTING,
  TYPE_PROVIDER_SETTING,
  validateWorkspaceArg,
  type WorkspaceSettingsWriter,
} from "./dxLauncher";

const tmps: string[] = [];
afterEach(() => {
  for (const d of tmps.splice(0)) rmSync(d, { recursive: true, force: true });
});

function tmpDir(): string {
  const d = mkdtempSync(join(tmpdir(), "dx-launch-"));
  tmps.push(d);
  return d;
}

describe("validateWorkspaceArg", () => {
  it("accepts an existing absolute directory and returns a canonical path", () => {
    const dir = tmpDir();
    const resolved = validateWorkspaceArg(dir);
    expect(resolved.length).toBeGreaterThan(0);
    // Idempotent: re-validating the canonical form yields the same value.
    expect(validateWorkspaceArg(resolved)).toBe(resolved);
  });

  it("rejects a missing/unset workspace argument", () => {
    expect(() => validateWorkspaceArg(undefined)).toThrow(/DX_HARNESS_WORKSPACE/);
    expect(() => validateWorkspaceArg("")).toThrow(/DX_HARNESS_WORKSPACE/);
  });

  it("rejects a relative path", () => {
    expect(() => validateWorkspaceArg("./relative/workspace")).toThrow(/absolute/i);
    expect(() => validateWorkspaceArg("relative")).toThrow(/absolute/i);
  });

  it("rejects an absolute path that does not exist", () => {
    const missing = join(tmpDir(), "does-not-exist");
    expect(() => validateWorkspaceArg(missing)).toThrow(/exist|director/i);
  });

  it("rejects an absolute path that is a file, not a directory", () => {
    const dir = tmpDir();
    const file = join(dir, "a-file.txt");
    writeFileSync(file, "x");
    expect(() => validateWorkspaceArg(file)).toThrow(/director/i);
  });
});

describe("ensureDebugLogLevel", () => {
  it("adds the debug log level to an empty settings object", () => {
    const { settings, changed } = ensureDebugLogLevel({});
    expect(settings[LOG_LEVEL_SETTING]).toBe("debug");
    expect(changed).toBe(true);
  });

  it("upgrades a non-debug log level to debug", () => {
    const { settings, changed } = ensureDebugLogLevel({ [LOG_LEVEL_SETTING]: "info" });
    expect(settings[LOG_LEVEL_SETTING]).toBe("debug");
    expect(changed).toBe(true);
  });

  it("is a no-op (changed=false) when already debug and preserves other pins", () => {
    const input = {
      [LOG_LEVEL_SETTING]: "debug",
      "verter.typescript.tsdk": "/opt/ts/lib",
      "verter.typeProvider": "tsgo",
    };
    const { settings, changed } = ensureDebugLogLevel(input);
    expect(changed).toBe(false);
    expect(settings["verter.typescript.tsdk"]).toBe("/opt/ts/lib");
    expect(settings["verter.typeProvider"]).toBe("tsgo");
    expect(settings[LOG_LEVEL_SETTING]).toBe("debug");
  });
});

describe("assertDebugLogLevel", () => {
  it("passes when the log level is debug", () => {
    expect(() => assertDebugLogLevel({ [LOG_LEVEL_SETTING]: "debug" })).not.toThrow();
  });

  it("throws when the log level is missing", () => {
    expect(() => assertDebugLogLevel({})).toThrow(/debug/);
  });

  it("throws when the log level is not debug", () => {
    expect(() => assertDebugLogLevel({ [LOG_LEVEL_SETTING]: "info" })).toThrow(/debug/);
  });
});

describe("buildDxLaunch", () => {
  const base = {
    workspace: "/abs/materialized/ws",
    extensionDevelopmentPath: "/abs/ext",
    extensionTestsPath: "/abs/ext/out-test/e2e/dx/index",
    vscodeExecutablePath: "/abs/vscode/code",
    logFile: "/abs/tmp/dx.log",
    baseEnv: { PATH: "/usr/bin", HOME: "/home/u" } as Record<string, string>,
  };

  it("passes the materialized workspace as the VS Code workspace folder", () => {
    const { launchArgs } = buildDxLaunch(base);
    expect(launchArgs).toContain("/abs/materialized/ws");
    expect(launchArgs).toContain("--disable-extensions");
    // The workspace folder must be the LAST positional arg (VS Code convention).
    expect(launchArgs[launchArgs.length - 1]).toBe("/abs/materialized/ws");
  });

  it("wires the DX env handoff, debug logging, and dx-only test filter", () => {
    const { extensionTestsEnv } = buildDxLaunch(base);
    expect(extensionTestsEnv[DX_HARNESS_WORKSPACE_ENV]).toBe("/abs/materialized/ws");
    expect(extensionTestsEnv.VERTER_E2E_TEST).toBe("1");
    expect(extensionTestsEnv.VERTER_E2E_DX).toBe("1");
    expect(extensionTestsEnv.VERTER_E2E_LOG_FILE).toBe("/abs/tmp/dx.log");
    expect(extensionTestsEnv.VERTER_LOG).toBe("debug");
    // The dx suite is the only one that may run under this launch.
    expect(extensionTestsEnv.VERTER_E2E_ONLY).toBe("dx-harness");
    // Inherited base env is preserved.
    expect(extensionTestsEnv.HOME).toBe("/home/u");
  });

  it("never declares a fixture, so in-host helpers cannot treat the workspace as a fixture", () => {
    const { extensionTestsEnv } = buildDxLaunch(base);
    // Negative: the DX path consumes a materialized workspace, not a named fixture.
    expect(extensionTestsEnv.VERTER_E2E_FIXTURE).toBeUndefined();
  });

  it("scrubs an INHERITED VERTER_E2E_FIXTURE so a leaked value cannot pass through", () => {
    const { extensionTestsEnv } = buildDxLaunch({
      ...base,
      baseEnv: { ...base.baseEnv, VERTER_E2E_FIXTURE: "leaked-fixture" },
    });
    expect(extensionTestsEnv.VERTER_E2E_FIXTURE).toBeUndefined();
  });

  it("does not mark the canary flag for a normal DX launch", () => {
    expect(buildDxLaunch(base).extensionTestsEnv[CANARY_MODE_ENV]).toBeUndefined();
  });

  it("forwards an optional LSP binary path and type provider, omitting them when unset", () => {
    const withExtras = buildDxLaunch({
      ...base,
      lspBinaryPath: "/abs/tmp/verter-lsp",
      typeProvider: "tsgo",
      timingFile: "/abs/tmp/timing.json",
    });
    expect(withExtras.extensionTestsEnv.VERTER_E2E_LSP_PATH).toBe("/abs/tmp/verter-lsp");
    expect(withExtras.extensionTestsEnv.VERTER_E2E_TYPE_PROVIDER).toBe("tsgo");
    expect(withExtras.extensionTestsEnv.VERTER_E2E_TIMING_FILE).toBe("/abs/tmp/timing.json");

    const without = buildDxLaunch(base);
    expect(without.extensionTestsEnv.VERTER_E2E_LSP_PATH).toBeUndefined();
    expect(without.extensionTestsEnv.VERTER_E2E_TYPE_PROVIDER).toBeUndefined();
    expect(without.extensionTestsEnv.VERTER_E2E_TIMING_FILE).toBeUndefined();
  });
});

describe("ensureAndVerifyDebugSettings (read → write → reread → assert round trip)", () => {
  /** A writer that persists settings to `<root>/.vscode/settings.json` verbatim. */
  function fileWriter(transform: (s: Record<string, unknown>) => Record<string, unknown>) {
    const writer: WorkspaceSettingsWriter = (root, settings) => {
      const dir = join(root, ".vscode");
      mkdirSync(dir, { recursive: true });
      writeFileSync(join(dir, "settings.json"), JSON.stringify(transform(settings)));
    };
    return writer;
  }

  it("returns the verified settings when the writer persists debug logging", () => {
    const dir = tmpDir();
    const verified = ensureAndVerifyDebugSettings(
      dir,
      fileWriter((s) => s),
    );
    expect(verified[LOG_LEVEL_SETTING]).toBe("debug");
  });

  it("THROWS when the writer DROPS verter.server.logLevel (verify-after-write catches it)", () => {
    const dir = tmpDir();
    // A faulty writer that silently omits the log level — the reread + assert must fail.
    const dropping = fileWriter(({ [LOG_LEVEL_SETTING]: _dropped, ...rest }) => rest);
    expect(() => ensureAndVerifyDebugSettings(dir, dropping)).toThrow(/debug/);
  });
});

describe("buildCanaryForcingSettings (the deterministic, provider-independent WARN trigger)", () => {
  it("forces the MCP config (mcp.enabled=true + typeProvider=off) with debug logging", () => {
    const s = buildCanaryForcingSettings();
    // mcp.enabled=true makes buildServerOptions pass --mcp-port=0 (the WARN trigger).
    expect(s[MCP_ENABLED_SETTING]).toBe(true);
    // typeProvider=off proves provider discovery is not the trigger.
    expect(s[TYPE_PROVIDER_SETTING]).toBe(CANARY_TYPE_PROVIDER);
    expect(s[LOG_LEVEL_SETTING]).toBe("debug");
    // Negative: the trigger is no longer provider-unavailability — no absent-tsdk pin.
    expect(s["verter.typescript.tsdk"]).toBeUndefined();
  });
});

describe("buildDxLaunch (canary mode)", () => {
  const base = {
    workspace: "/abs/canary/ws",
    extensionDevelopmentPath: "/abs/ext",
    extensionTestsPath: "/abs/ext/out-test/e2e/dx/index",
    vscodeExecutablePath: "/abs/vscode/code",
    logFile: "/abs/tmp/canary.log",
    baseEnv: { PATH: "/usr/bin" } as Record<string, string>,
  };

  it("marks the canary launch and pins the provider to off over any override", () => {
    const { extensionTestsEnv } = buildDxLaunch({ ...base, canary: true, typeProvider: "tsgo" });
    expect(extensionTestsEnv[CANARY_MODE_ENV]).toBe("1");
    // The pinned `off` provider wins over the caller's typeProvider AND inherited env, so
    // the captured --type-provider=off proves provider discovery is not the trigger.
    expect(extensionTestsEnv.VERTER_E2E_TYPE_PROVIDER).toBe(CANARY_TYPE_PROVIDER);
    // It still runs only the dx suite and captures to the canary log file.
    expect(extensionTestsEnv.VERTER_E2E_ONLY).toBe("dx-harness");
    expect(extensionTestsEnv.VERTER_E2E_LOG_FILE).toBe("/abs/tmp/canary.log");
  });
});

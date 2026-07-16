import { mkdtempSync, readFileSync, rmSync, writeFileSync, mkdirSync } from "fs";
import { tmpdir } from "os";
import { join } from "path";
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  findWorkspaceRcTsgoBinary,
  provisionVsCodeExtension,
  writeVsCodeUserSettings,
  type SynchronousCommandRunner,
} from "../e2e/sharedLaunch";

const temporaryRoots: string[] = [];

afterEach(() => {
  for (const root of temporaryRoots.splice(0)) {
    rmSync(root, { recursive: true, force: true });
  }
});

describe("VS Code E2E extension provisioning", () => {
  it("discovers the pinned RC tsgo package from a nested pnpm workspace", () => {
    const root = mkdtempSync(join(tmpdir(), "verter-e2e-tsgo-"));
    temporaryRoots.push(root);
    const extensionPath = join(root, "packages", "vue-vscode");
    const binary = join(
      root,
      "node_modules",
      ".pnpm",
      "@typescript+typescript-win32-x64@7.0.2",
      "node_modules",
      "@typescript",
      "typescript-win32-x64",
      "lib",
      "tsc.exe",
    );
    mkdirSync(join(binary, ".."), { recursive: true });
    writeFileSync(binary, "rc-tsgo");

    expect(
      findWorkspaceRcTsgoBinary(extensionPath, {
        env: {},
        platform: "win32",
        arch: "x64",
      }),
    ).toBe(binary);
  });

  it("never treats the legacy native-preview package as the RC tsgo engine", () => {
    const root = mkdtempSync(join(tmpdir(), "verter-e2e-legacy-tsgo-"));
    temporaryRoots.push(root);
    const extensionPath = join(root, "packages", "vue-vscode");
    const legacy = join(
      root,
      "node_modules",
      ".pnpm",
      "@typescript+native-preview-win32-x64@7.0.0-dev.20260101.1",
      "node_modules",
      "@typescript",
      "native-preview-win32-x64",
      "lib",
      "tsgo.exe",
    );
    mkdirSync(join(legacy, ".."), { recursive: true });
    writeFileSync(legacy, "legacy-tsgo");

    expect(
      findWorkspaceRcTsgoBinary(extensionPath, {
        env: {},
        platform: "win32",
        arch: "x64",
      }),
    ).toBeUndefined();
  });

  it("fails closed when an explicit E2E tsgo binary is configured but absent", () => {
    const root = mkdtempSync(join(tmpdir(), "verter-e2e-explicit-tsgo-"));
    temporaryRoots.push(root);
    const missing = join(root, "missing", "tsc.exe");

    expect(() =>
      findWorkspaceRcTsgoBinary(root, {
        env: { VERTER_TSGO_BIN: missing },
        platform: "win32",
        arch: "x64",
      }),
    ).toThrow(/configured VERTER_TSGO_BIN does not exist/i);
  });

  it("installs into the exact isolated profile while preserving platform CLI bootstrap args", () => {
    const run = vi.fn<SynchronousCommandRunner>(() => ({
      status: 0,
      stdout: "installed",
      stderr: "",
    }));

    provisionVsCodeExtension({
      cliArgs: [
        "/Applications/Code",
        "--ms-enable-electron-run-as-node",
        "/Applications/Code/resources/app/out/cli.js",
        "--extensions-dir=/stale/extensions",
        "--user-data-dir=/stale/user-data",
      ],
      extension: "TypeScriptTeam.native-preview@0.20260708.2",
      extensionsDir: "/isolated/extensions",
      userDataDir: "/isolated/user-data",
      run,
      platform: "darwin",
    });

    expect(run).toHaveBeenCalledOnce();
    const [command, args, options] = run.mock.calls[0];
    expect(command).toBe("/Applications/Code");
    expect(args).toEqual([
      "--ms-enable-electron-run-as-node",
      "/Applications/Code/resources/app/out/cli.js",
      "--extensions-dir=/isolated/extensions",
      "--user-data-dir=/isolated/user-data",
      "--install-extension",
      "TypeScriptTeam.native-preview@0.20260708.2",
      "--force",
    ]);
    expect(options).toMatchObject({ timeout: 180_000, shell: false, windowsHide: true });
  });

  it("turns a missing or failed installation into a hard gate failure", () => {
    const run: SynchronousCommandRunner = () => ({
      status: 1,
      stdout: "",
      stderr: "extension not found",
    });

    expect(() =>
      provisionVsCodeExtension({
        cliArgs: ["code"],
        extension: "TypeScriptTeam.native-preview@0.20260708.2",
        extensionsDir: "/isolated/extensions",
        userDataDir: "/isolated/user-data",
        run,
      }),
    ).toThrow(/extension not found/);
  });

  it("seeds Native Preview enablement in the isolated user profile without dropping existing settings", () => {
    const userDataDir = mkdtempSync(join(tmpdir(), "verter-e2e-user-settings-"));
    temporaryRoots.push(userDataDir);
    const userDir = join(userDataDir, "User");
    mkdirSync(userDir, { recursive: true });
    writeFileSync(
      join(userDir, "settings.json"),
      JSON.stringify({ "editor.fontSize": 15 }),
      "utf8",
    );

    writeVsCodeUserSettings(userDataDir, {
      "js/ts.experimental.useTsgo": true,
    });

    expect(JSON.parse(readFileSync(join(userDir, "settings.json"), "utf8"))).toEqual({
      "editor.fontSize": 15,
      "js/ts.experimental.useTsgo": true,
    });
  });
});

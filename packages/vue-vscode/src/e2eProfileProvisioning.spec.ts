import { mkdtempSync, readFileSync, rmSync, writeFileSync, mkdirSync } from "fs";
import { tmpdir } from "os";
import { join } from "path";
import { afterEach, describe, expect, it, vi } from "vitest";

import {
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

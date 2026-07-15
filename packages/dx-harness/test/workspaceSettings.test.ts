import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import { DX_HARNESS_WORKSPACE_ENV, writeWorkspaceSettings } from "../src/workspaceSettings.js";

const tmps: string[] = [];
afterEach(() => {
  for (const d of tmps.splice(0)) rmSync(d, { recursive: true, force: true });
});

function tmp(): string {
  const d = mkdtempSync(join(tmpdir(), "dx-settings-"));
  tmps.push(d);
  return d;
}

describe("writeWorkspaceSettings", () => {
  it("writes valid JSON settings under <root>/.vscode and pins tsdk + provider", () => {
    const root = tmp();
    const result = writeWorkspaceSettings(root, {
      tsdk: "/opt/ts/lib",
      typeProvider: "tsgo",
    });
    const onDisk: unknown = JSON.parse(readFileSync(result.settingsPath, "utf-8"));
    expect(onDisk).toMatchObject({
      "verter.typescript.tsdk": "/opt/ts/lib",
      "verter.typeProvider": "tsgo",
    });
    // The returned settings mirror what was written.
    expect(result.settings).toEqual(onDisk);
  });

  it("pins verter.server.logLevel to debug by default", () => {
    const root = tmp();
    const result = writeWorkspaceSettings(root);
    const onDisk: Record<string, unknown> = JSON.parse(readFileSync(result.settingsPath, "utf-8"));
    // The extension-host transport copies `verter.server.logLevel` (package
    // default "info") into VERTER_LOG; pinning "debug" keeps the readiness/log
    // signal the differential run gates on.
    expect(onDisk["verter.server.logLevel"]).toBe("debug");
    expect(result.settings["verter.server.logLevel"]).toBe("debug");
    // Negative: the default is NOT absent and NOT the package default "info".
    expect(onDisk).toHaveProperty("verter.server.logLevel");
    expect(onDisk["verter.server.logLevel"]).not.toBe("info");
  });

  it("lets a caller override the pinned debug logLevel", () => {
    const root = tmp();
    const result = writeWorkspaceSettings(root, {
      settings: { "verter.server.logLevel": "info" },
    });
    // Caller overrides still win over the pinned default.
    expect(result.settings["verter.server.logLevel"]).toBe("info");
  });

  it("hands off the workspace root via DX_HARNESS_WORKSPACE (canonicalised)", () => {
    const root = tmp();
    const result = writeWorkspaceSettings(root);
    expect(result.env[DX_HARNESS_WORKSPACE_ENV]).toBe(result.root);
    // Negative: the handoff carries exactly the one key.
    expect(Object.keys(result.env)).toEqual([DX_HARNESS_WORKSPACE_ENV]);
  });

  it("places settings.json strictly under <root>/.vscode with portable separators", () => {
    const root = tmp();
    const result = writeWorkspaceSettings(root);
    expect(result.settingsPath).toBe(`${result.root}/.vscode/settings.json`);
    expect(result.settingsPath).not.toContain("\\");
  });

  it("merges caller overrides, which win over the pinned defaults", () => {
    const root = tmp();
    const result = writeWorkspaceSettings(root, {
      typeProvider: "tsgo",
      settings: { "verter.typeProvider": "tsserver", "custom.flag": true },
    });
    expect(result.settings["verter.typeProvider"]).toBe("tsserver");
    expect(result.settings["custom.flag"]).toBe(true);
  });

  it("omits unset pins and still writes valid JSON", () => {
    const root = tmp();
    const result = writeWorkspaceSettings(root);
    expect(result.settings).not.toHaveProperty("verter.typescript.tsdk");
    expect(result.settings).not.toHaveProperty("verter.typeProvider");
    // Re-reading the file must still parse.
    expect(() => JSON.parse(readFileSync(result.settingsPath, "utf-8"))).not.toThrow();
  });

  it("is idempotent — a second write reproduces the same file", () => {
    const root = tmp();
    const a = writeWorkspaceSettings(root, { tsdk: "/opt/ts/lib" });
    const first = readFileSync(a.settingsPath, "utf-8");
    const b = writeWorkspaceSettings(root, { tsdk: "/opt/ts/lib" });
    expect(readFileSync(b.settingsPath, "utf-8")).toBe(first);
  });
});

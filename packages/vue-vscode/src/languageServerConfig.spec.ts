import { join } from "node:path";

import { describe, expect, it } from "vitest";
import {
  buildLspLaunchArgs,
  lspTsdkLaunchArg,
  shouldRestartLanguageServerForConfigurationChange,
} from "./languageServerConfig";

function makeEvent(changed: string[]) {
  return {
    affectsConfiguration(section: string) {
      return changed.includes(section);
    },
  };
}

describe("shouldRestartLanguageServerForConfigurationChange", () => {
  it("restarts for other init-only experimental settings", () => {
    expect(
      shouldRestartLanguageServerForConfigurationChange(
        makeEvent(["verter.experimental.conditionalRootNarrowing"]),
      ),
    ).toBe(true);
    expect(
      shouldRestartLanguageServerForConfigurationChange(
        makeEvent(["verter.experimental.strictSlots"]),
      ),
    ).toBe(true);
  });

  // @ai-generated - Pins analysis.enabled as init-only until the server supports live updates.
  it("restarts for the init-only analysis setting", () => {
    expect(
      shouldRestartLanguageServerForConfigurationChange(makeEvent(["verter.analysis.enabled"])),
    ).toBe(true);
  });

  it("restarts for init-only hover policy settings", () => {
    expect(
      shouldRestartLanguageServerForConfigurationChange(
        makeEvent(["verter.hover.nativeSemantics"]),
      ),
    ).toBe(true);
    expect(
      shouldRestartLanguageServerForConfigurationChange(makeEvent(["verter.hover.provenance"])),
    ).toBe(true);
  });
});

describe("lspTsdkLaunchArg", () => {
  // The LSP's own discovery cascade (project-local installs first, then the
  // configured tsdk, then a global install) fails closed with an actionable
  // error when nothing resolves. The extension must not inject a bundled
  // TypeScript into that cascade: no user setting, no `--tsdk` flag.
  it("emits no --tsdk flag when the user has not configured one (no bundled default)", () => {
    expect(lspTsdkLaunchArg("")).toBeUndefined();
  });

  it("forwards the user-configured typescript.tsdk verbatim", () => {
    expect(lspTsdkLaunchArg("/opt/ts/lib")).toBe("--tsdk=/opt/ts/lib");
  });
});

// The LAUNCH BOUNDARY, not a string helper: this is the argv the language
// server process is actually spawned with (`ServerOptions.run.args`). The
// extension used to default `--tsdk` to its OWN staged TypeScript
// (`<extensionPath>/node_modules/typescript/lib`) whenever the user had not
// configured one, injecting an extension-owned compiler into the LSP's
// discovery cascade. Restoring that default anywhere in argv assembly fails
// these — a helper-only test would not notice.
describe("buildLspLaunchArgs", () => {
  const EXTENSION_PATH = join("/ext", "verter");

  function baseInput(userTsdk: string) {
    return {
      clientProcessLifetimeArg: "--client-pid=4242",
      typeProvider: "auto",
      userTsdk,
      pluginPath: join(EXTENSION_PATH, "node_modules"),
      mcp: { port: 0, lintPreset: "recommended" },
      sharedLspArgs: ["--editor-attested=1"],
      rootPath: "/work/app",
    };
  }

  it("spawns the server with no --tsdk at all when the user configured none", () => {
    const args = buildLspLaunchArgs(baseInput(""));

    expect(args.filter((a) => a.startsWith("--tsdk"))).toEqual([]);
    // Nothing may smuggle the extension's own TypeScript in under another flag.
    expect(args.some((a) => a.includes(join("node_modules", "typescript")))).toBe(false);
    // The rest of the launch contract still holds.
    expect(args[0]).toBe("--client-pid=4242");
    expect(args).toContain("--type-provider=auto");
    expect(args).toContain(`--plugin-path=${join(EXTENSION_PATH, "node_modules")}`);
    expect(args).toContain("--mcp-port=0");
    expect(args).toContain("--editor-attested=1");
    // The positional root is last, after every `--` flag.
    expect(args[args.length - 1]).toBe("/work/app");
  });

  it("spawns the server with exactly the user's --tsdk when configured", () => {
    const args = buildLspLaunchArgs(baseInput("/opt/ts/lib"));

    expect(args.filter((a) => a.startsWith("--tsdk"))).toEqual(["--tsdk=/opt/ts/lib"]);
    // `--tsdk` precedes the positional root.
    expect(args.indexOf("--tsdk=/opt/ts/lib")).toBeLessThan(args.length - 1);
  });

  it("omits the MCP flags entirely when MCP is disabled", () => {
    const args = buildLspLaunchArgs({ ...baseInput(""), mcp: undefined });
    expect(args.some((a) => a.startsWith("--mcp-"))).toBe(false);
  });
});

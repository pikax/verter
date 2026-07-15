import { join } from "path";

import { describe, expect, it } from "vitest";

import {
  RELAY_SHIM_STEM,
  buildSharedLspArgs,
  buildRelayEditorEnv,
  buildShimSpawnArgs,
  discoverNativePreviewTsgo,
  discoverRelayShim,
  establishControlDir,
  isShimAdvertisement,
  lspArgsPropagated,
  mintSessionKey,
  parseArmedControlDir,
  planSharedTsgo,
  prepareEditorTsdk,
  relayShimBasename,
  relayShimCandidates,
  sessionKeyFromControlDir,
  typeProviderRoutesTsgo,
  verifySharedArmedHandshake,
} from "./sharedTsgoLaunch";

describe("relayShimBasename", () => {
  it("suffixes .exe on win32 and nothing elsewhere", () => {
    expect(relayShimBasename("win32")).toBe(`${RELAY_SHIM_STEM}.exe`);
    expect(relayShimBasename("linux")).toBe(RELAY_SHIM_STEM);
    expect(relayShimBasename("darwin")).toBe(RELAY_SHIM_STEM);
  });
  it("never names tsgo", () => {
    for (const p of ["win32", "linux", "darwin"] as const) {
      expect(/tsgo/i.test(relayShimBasename(p))).toBe(false);
    }
  });
});

describe("relayShimCandidates", () => {
  it("honors VERTER_RELAY_SHIM_BINARY as the sole candidate", () => {
    const explicit = "/ci/verter-relay-shim";
    expect(
      relayShimCandidates({
        extensionPath: "/ext",
        env: { VERTER_RELAY_SHIM_BINARY: explicit },
        platform: "linux",
      }),
    ).toEqual([explicit]);
  });
  it("prefers dev target/{debug,release} over the packaged bin/ (freshest local build wins)", () => {
    const c = relayShimCandidates({ extensionPath: "/ext", env: {}, platform: "win32" });
    const devDebug = join("/ext", "target", "debug", `${RELAY_SHIM_STEM}.exe`);
    const packaged = join("/ext", "bin", `${RELAY_SHIM_STEM}.exe`);
    expect(c[0]).toBe(devDebug);
    expect(c).toContain(packaged);
    expect(c.indexOf(devDebug)).toBeLessThan(c.indexOf(packaged));
  });
});

describe("discoverRelayShim", () => {
  it("returns the first existing candidate", () => {
    const packaged = join("/ext", "bin", RELAY_SHIM_STEM);
    const got = discoverRelayShim({
      extensionPath: "/ext",
      env: {},
      platform: "linux",
      exists: (p) => p === packaged,
    });
    expect(got).toBe(packaged);
  });
  it("returns undefined (fail-closed) when nothing exists", () => {
    expect(
      discoverRelayShim({ extensionPath: "/ext", env: {}, platform: "linux", exists: () => false }),
    ).toBeUndefined();
  });
});

describe("discoverNativePreviewTsgo", () => {
  it("prefers VERTER_TSGO_BIN when it exists", () => {
    const bin = "/prov/tsc.exe";
    expect(
      discoverNativePreviewTsgo({
        env: { VERTER_TSGO_BIN: bin },
        platform: "win32",
        exists: (p) => p === bin,
      }),
    ).toBe(bin);
  });
  it("falls back to the native-preview.tsdk dir", () => {
    const tsdk = "/tsdk";
    const bin = join(tsdk, "tsc");
    expect(
      discoverNativePreviewTsgo({
        env: {},
        nativePreviewTsdk: tsdk,
        platform: "linux",
        exists: (p) => p === bin,
      }),
    ).toBe(bin);
  });
  it("falls back to the workspace @typescript native-preview package", () => {
    const bin = join(
      "/ws",
      "node_modules",
      "@typescript",
      "typescript-win32-x64",
      "lib",
      "tsc.exe",
    );
    expect(
      discoverNativePreviewTsgo({
        env: {},
        workspaceRoot: "/ws",
        platform: "win32",
        arch: "x64",
        exists: (p) => p === bin,
      }),
    ).toBe(bin);
  });
  it("returns undefined (fail-closed) when nothing resolves", () => {
    expect(discoverNativePreviewTsgo({ env: {}, exists: () => false })).toBeUndefined();
  });
});

describe("rendezvous construction", () => {
  it("mints a 128-bit hex session key", () => {
    const key = mintSessionKey((n) => Buffer.alloc(n, 0xab));
    expect(key).toBe("ab".repeat(16));
    expect(key).toMatch(/^[0-9a-f]{32}$/);
  });
  it("establishes an isolated, session-scoped control dir and creates it", () => {
    const created: string[] = [];
    const dir = establishControlDir({
      root: "/tmp",
      sessionKey: "deadbeef",
      mkdir: (p) => created.push(p),
    });
    expect(dir).toBe(join("/tmp", "verter-shared-deadbeef"));
    expect(created).toEqual([dir]);
  });
});

describe("buildSharedLspArgs (both --shared-* or throw)", () => {
  it("emits BOTH args (the LSP engages SHARED only when both are present)", () => {
    expect(buildSharedLspArgs({ controlDir: "/c", sessionKey: "k" })).toEqual([
      "--shared-control-dir=/c",
      "--shared-session-key=k",
    ]);
  });
  it("throws on a partial rendezvous (a single --shared-* would be a silent no-op)", () => {
    expect(() => buildSharedLspArgs({ controlDir: "", sessionKey: "k" })).toThrow(/BOTH/);
    expect(() => buildSharedLspArgs({ controlDir: "/c", sessionKey: "" })).toThrow(/BOTH/);
  });
});

describe("buildShimSpawnArgs", () => {
  it("forwards --lsp --stdio after -- and passes the rendezvous", () => {
    expect(buildShimSpawnArgs({ realTsgo: "/tsc.exe", controlDir: "/c", sessionKey: "k" })).toEqual(
      [
        "--real-tsgo",
        "/tsc.exe",
        "--control-dir",
        "/c",
        "--session-key",
        "k",
        "--",
        "--lsp",
        "--stdio",
      ],
    );
  });
});

describe("prepareEditorTsdk", () => {
  it("stages the relay bytes under Native Preview's tsgo executable name", () => {
    const made: string[] = [];
    const copied: Array<[string, string]> = [];
    const staged = prepareEditorTsdk({
      shimPath: "/ext/bin/verter-relay-shim",
      controlDir: "/tmp/session",
      platform: "linux",
      mkdir: (path) => made.push(path),
      copy: (source, destination) => copied.push([source, destination]),
      chmod: () => {},
    });
    expect(staged).toEqual({
      dir: join("/tmp/session", "editor-tsdk"),
      executable: join("/tmp/session", "editor-tsdk", "tsgo"),
    });
    expect(made).toEqual([staged.dir]);
    expect(copied).toEqual([["/ext/bin/verter-relay-shim", staged.executable]]);
  });
});

describe("buildRelayEditorEnv", () => {
  it("binds the editor-spawned shim to the real engine and rendezvous", () => {
    expect(
      buildRelayEditorEnv({ realTsgo: "/real/tsgo", controlDir: "/ctl", sessionKey: "key" }),
    ).toEqual({
      VERTER_RELAY_REAL_TSGO: "/real/tsgo",
      VERTER_RELAY_CONTROL_DIR: "/ctl",
      VERTER_RELAY_SESSION_KEY: "key",
    });
  });
});

describe("planSharedTsgo (fail-closed)", () => {
  const shim = join("/ext", "target", "debug", "verter-relay-shim");
  const tsgo = "/prov/tsc";

  it("engages with a full rendezvous when shim AND tsgo resolve", () => {
    const created: string[] = [];
    const plan = planSharedTsgo({
      extensionPath: "/ext",
      controlDirRoot: "/tmp",
      env: { VERTER_TSGO_BIN: tsgo },
      platform: "linux",
      exists: (p) => p === shim || p === tsgo,
      mkdir: (p) => created.push(p),
      rng: (n) => Buffer.alloc(n, 0x01),
    });
    expect(plan.engaged).toBe(true);
    if (!plan.engaged) throw new Error("unreachable");
    expect(plan.shimPath).toBe(shim);
    expect(plan.realTsgo).toBe(tsgo);
    expect(plan.lspArgs).toEqual([
      `--shared-control-dir=${plan.controlDir}`,
      `--shared-session-key=${plan.sessionKey}`,
    ]);
    expect(created).toEqual([plan.controlDir]);
  });

  it("does NOT engage when VERTER_DISABLE_SHARED_TSGO is set, even with a full rendezvous available", () => {
    const plan = planSharedTsgo({
      extensionPath: "/ext",
      controlDirRoot: "/tmp",
      env: { VERTER_TSGO_BIN: tsgo, VERTER_DISABLE_SHARED_TSGO: "1" },
      platform: "linux",
      exists: (p) => p === shim || p === tsgo, // both present — the hatch must still win
      rng: (n) => Buffer.alloc(n, 0x01),
    });
    expect(plan.engaged).toBe(false);
    if (plan.engaged) throw new Error("unreachable");
    expect(plan.reason).toMatch(/VERTER_DISABLE_SHARED_TSGO|OWNED/);
  });

  it("does NOT engage (stays OWNED) when the shim is absent — never throws", () => {
    const plan = planSharedTsgo({
      extensionPath: "/ext",
      controlDirRoot: "/tmp",
      env: { VERTER_TSGO_BIN: tsgo },
      platform: "linux",
      exists: (p) => p === tsgo, // tsgo yes, shim no
    });
    expect(plan.engaged).toBe(false);
    if (plan.engaged) throw new Error("unreachable");
    expect(plan.reason).toMatch(/shim/i);
  });

  it("does NOT engage (stays OWNED) when the tsgo engine is absent — never throws", () => {
    const plan = planSharedTsgo({
      extensionPath: "/ext",
      controlDirRoot: "/tmp",
      env: {},
      platform: "linux",
      exists: (p) => p === shim, // shim yes, tsgo no
    });
    expect(plan.engaged).toBe(false);
    if (plan.engaged) throw new Error("unreachable");
    expect(plan.reason).toMatch(/tsgo|tsdk/i);
  });
});

describe("typeProviderRoutesTsgo", () => {
  it("attempts editor-owned tsgo for auto / shared-tsgo, while explicit tsgo is managed", () => {
    expect(typeProviderRoutesTsgo("tsgo")).toBe(false);
    expect(typeProviderRoutesTsgo("shared-tsgo")).toBe(true);
    expect(typeProviderRoutesTsgo("tsserver")).toBe(false);
    expect(typeProviderRoutesTsgo("auto")).toBe(true);
    expect(typeProviderRoutesTsgo("off")).toBe(false);
    expect(typeProviderRoutesTsgo(undefined)).toBe(false);
  });
});

// ── SHARED armed-handshake verification (Q3) ────────────────────────────────────
//
// The verifier ties "[shared-tsgo] armed" to two OBSERVABLES (shim advertised in the
// control dir + `--shared-*` propagated into the verter-lsp argv). Each RED case below
// flips exactly one observable and MUST fail — proving the verifier is not a bare
// log-string check that would false-green owned-only.

const KEY = "abcdef0123456789abcdef0123456789";
const WIN_CONTROL_DIR = `C:\\Users\\x\\AppData\\Local\\Temp\\verter-shared-${KEY}`;
const POSIX_CONTROL_DIR = `/tmp/verter-shared-${KEY}`;
const AD_FILE = `${RELAY_SHIM_STEM}-${KEY}-12345.json`;

/** The extension's dual-written `[shared-tsgo] armed` line for a control dir. */
function armedLine(controlDir: string): string {
  return `[INFO] [shared-tsgo] armed: shim=C:\\ext\\bin\\${RELAY_SHIM_STEM}.exe realTsgo=C:\\tsc.exe controlDir=${controlDir} (SHARED editor-attach overlay will bind lazily per query)`;
}

/** The extension's dual-written `[buildServerOptions]` line with the argv JSON-encoded. */
function buildServerOptionsLine(argv: string[]): string {
  return `[INFO] [buildServerOptions] typeProvider=tsgo, tsdk=C:\\ext\\ts (bundled), args=${JSON.stringify(
    argv,
  )}`;
}

/** A verter-lsp argv that ARMED SHARED (JSON.stringify escapes the Windows backslashes). */
function armedArgv(controlDir: string, key: string): string[] {
  return [
    "--type-provider=tsgo",
    "--tsdk=C:\\ext\\ts",
    `--shared-control-dir=${controlDir}`,
    `--shared-session-key=${key}`,
    "C:\\Users\\x\\ws",
  ];
}

describe("parseArmedControlDir", () => {
  it("captures the controlDir from the armed line (Windows path)", () => {
    expect(parseArmedControlDir(`prefix\n${armedLine(WIN_CONTROL_DIR)}\nsuffix`)).toBe(
      WIN_CONTROL_DIR,
    );
  });
  it("captures the controlDir from the armed line (POSIX path)", () => {
    expect(parseArmedControlDir(armedLine(POSIX_CONTROL_DIR))).toBe(POSIX_CONTROL_DIR);
  });
  it("is undefined when SHARED was never armed / no controlDir logged", () => {
    expect(
      parseArmedControlDir("[INFO] [shared-tsgo] not engaged — OWNED baseline"),
    ).toBeUndefined();
    // A bare "armed" mention with no controlDir= must NOT parse (owned-only masquerade).
    expect(parseArmedControlDir("[INFO] [shared-tsgo] armed")).toBeUndefined();
  });
});

describe("sessionKeyFromControlDir", () => {
  it("extracts the hex session key from a Windows and a POSIX control dir", () => {
    expect(sessionKeyFromControlDir(WIN_CONTROL_DIR)).toBe(KEY);
    expect(sessionKeyFromControlDir(POSIX_CONTROL_DIR)).toBe(KEY);
    expect(sessionKeyFromControlDir(`${POSIX_CONTROL_DIR}/`)).toBe(KEY);
  });
  it("is undefined for a dir that is not a rendezvous control dir", () => {
    expect(sessionKeyFromControlDir("/tmp/unrelated")).toBeUndefined();
  });
});

describe("isShimAdvertisement", () => {
  it("recognizes a verter-relay-shim-*.json advertisement, nothing else", () => {
    expect(isShimAdvertisement(AD_FILE)).toBe(true);
    expect(isShimAdvertisement(`${RELAY_SHIM_STEM}-x-1.json`)).toBe(true);
    expect(isShimAdvertisement(`${RELAY_SHIM_STEM}.exe`)).toBe(false);
    expect(isShimAdvertisement("some-other-file.json")).toBe(false);
    expect(isShimAdvertisement("tsgo.log")).toBe(false);
  });
});

describe("lspArgsPropagated (the rendezvous reached the verter-lsp argv)", () => {
  it("is true when BOTH --shared-* flags carry this session (JSON-escaped Windows argv)", () => {
    const log = buildServerOptionsLine(armedArgv(WIN_CONTROL_DIR, KEY));
    expect(lspArgsPropagated(log, WIN_CONTROL_DIR)).toBe(true);
  });
  it("is true cross-platform for a POSIX argv", () => {
    const log = buildServerOptionsLine(armedArgv(POSIX_CONTROL_DIR, KEY));
    expect(lspArgsPropagated(log, POSIX_CONTROL_DIR)).toBe(true);
  });
  it("is false when the --shared-session-key arg is absent (rendezvous did not propagate)", () => {
    const log = buildServerOptionsLine([
      "--type-provider=tsgo",
      `--shared-control-dir=${WIN_CONTROL_DIR}`,
      "C:\\Users\\x\\ws",
    ]);
    expect(lspArgsPropagated(log, WIN_CONTROL_DIR)).toBe(false);
  });
  it("is false when NO --shared-* args were passed (OWNED baseline argv)", () => {
    const log = buildServerOptionsLine(["--type-provider=tsgo", "C:\\Users\\x\\ws"]);
    expect(lspArgsPropagated(log, WIN_CONTROL_DIR)).toBe(false);
  });
});

describe("verifySharedArmedHandshake (Q3 — armed only with an observable handshake)", () => {
  function greenLog(controlDir: string): string {
    return [armedLine(controlDir), buildServerOptionsLine(armedArgv(controlDir, KEY))].join("\n");
  }

  it("GREEN: armed line + propagated args + a live shim advertisement ⇒ ok", () => {
    const v = verifySharedArmedHandshake({
      logText: greenLog(WIN_CONTROL_DIR),
      controlDirEntries: [AD_FILE],
    });
    expect(v.ok).toBe(true);
    expect(v.controlDir).toBe(WIN_CONTROL_DIR);
    expect(v.advertisements).toEqual([AD_FILE]);
    expect(v.argsPropagated).toBe(true);
  });

  // §1a RED: remove the shim advertisement — the handshake is no longer observable, so
  // the verdict MUST flip RED (a bare log-string check would still pass here).
  it("RED: the SAME armed log but NO advertisement in the control dir ⇒ NOT ok", () => {
    const v = verifySharedArmedHandshake({
      logText: greenLog(WIN_CONTROL_DIR),
      controlDirEntries: ["tsgo.log", "unrelated.txt"],
    });
    expect(v.ok).toBe(false);
    expect(v.advertisements).toEqual([]);
    expect(v.argsPropagated).toBe(true); // args still propagated — ONLY the advertisement is gone
    expect(v.reason).toMatch(/advertisement/i);
  });

  // §1a RED: drop the propagated args — the rendezvous never reached the LSP.
  it("RED: armed line + advertisement but the --shared-* args did NOT propagate ⇒ NOT ok", () => {
    const log = [
      armedLine(WIN_CONTROL_DIR),
      buildServerOptionsLine(["--type-provider=tsgo", "C:\\Users\\x\\ws"]),
    ].join("\n");
    const v = verifySharedArmedHandshake({ logText: log, controlDirEntries: [AD_FILE] });
    expect(v.ok).toBe(false);
    expect(v.argsPropagated).toBe(false);
    expect(v.reason).toMatch(/propagate|argv/i);
  });

  // §1a RED: a bare "armed" mention with no controlDir is the owned-only masquerade.
  it("RED: a bare '[shared-tsgo] armed' with no controlDir ⇒ NOT ok (owned-only)", () => {
    const v = verifySharedArmedHandshake({
      logText: "[INFO] [shared-tsgo] armed",
      controlDirEntries: [AD_FILE],
    });
    expect(v.ok).toBe(false);
    expect(v.controlDir).toBeUndefined();
    expect(v.reason).toMatch(/never armed|owned-only/i);
  });
});

import {
  cpSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, describe, expect, it } from "vitest";

// The C6 packaging helper: staging the verter-relay-shim binary into the VSIX bin/.
// It is a pure module (no import-time side effects) so it can be unit-tested without
// running the whole `package.mjs` VSIX pipeline.
import {
  SHIM_STEM,
  resolveShimBinarySource,
  shimBinaryBasename,
  shimBinaryCandidates,
  stageShimBinary,
  vsceTargetToRustTarget,
} from "../stage-bin.mjs";

const tmpDirs: string[] = [];

function makeTmp(tag: string): string {
  const dir = mkdtempSync(path.join(tmpdir(), `verter-stage-${tag}-`));
  tmpDirs.push(dir);
  return dir;
}

/** Write a fake compiled binary at target/<rust-target|.>/<profile>/<basename>. */
function writeFakeBinary(repoRoot: string, rel: string, bytes: string): string {
  const full = path.join(repoRoot, "target", rel);
  mkdirSync(path.dirname(full), { recursive: true });
  writeFileSync(full, bytes);
  return full;
}

afterEach(() => {
  while (tmpDirs.length) {
    const dir = tmpDirs.pop()!;
    try {
      rmSync(dir, { recursive: true, force: true });
    } catch {
      /* best-effort temp cleanup */
    }
  }
});

describe("VSCE target -> Rust target mapping", () => {
  it("maps every declared platform VSCE target to its Rust triple", () => {
    expect(vsceTargetToRustTarget("win32-x64")).toBe("x86_64-pc-windows-msvc");
    expect(vsceTargetToRustTarget("win32-arm64")).toBe("aarch64-pc-windows-msvc");
    expect(vsceTargetToRustTarget("linux-x64")).toBe("x86_64-unknown-linux-gnu");
    expect(vsceTargetToRustTarget("linux-arm64")).toBe("aarch64-unknown-linux-gnu");
    expect(vsceTargetToRustTarget("darwin-x64")).toBe("x86_64-apple-darwin");
    expect(vsceTargetToRustTarget("darwin-arm64")).toBe("aarch64-apple-darwin");
  });

  it("returns null for a universal (targetless) build", () => {
    expect(vsceTargetToRustTarget(undefined)).toBeNull();
    expect(vsceTargetToRustTarget("")).toBeNull();
  });

  // Fail closed: an unrecognized target must NOT silently fall through to a host
  // binary (that would poison a cross-target VSIX). It throws.
  it("throws on an unrecognized --target (never a silent host fallback)", () => {
    expect(() => vsceTargetToRustTarget("solaris-sparc")).toThrow(/unrecognized/i);
  });
});

describe("shim binary basename (.exe suffix follows the TARGET, not the host)", () => {
  it("uses .exe for win32 targets and no suffix for unix targets", () => {
    expect(shimBinaryBasename("win32-x64", "linux")).toBe(`${SHIM_STEM}.exe`);
    expect(shimBinaryBasename("win32-arm64", "darwin")).toBe(`${SHIM_STEM}.exe`);
    expect(shimBinaryBasename("linux-x64", "win32")).toBe(SHIM_STEM);
    expect(shimBinaryBasename("darwin-arm64", "win32")).toBe(SHIM_STEM);
  });

  it("falls back to the host platform for a universal build", () => {
    expect(shimBinaryBasename(undefined, "win32")).toBe(`${SHIM_STEM}.exe`);
    expect(shimBinaryBasename(undefined, "linux")).toBe(SHIM_STEM);
  });

  // NEGATIVE: the stem is fixed and never tsgo-shaped.
  it("only ever names the Verter shim, never tsgo", () => {
    for (const t of ["win32-x64", "linux-x64", "darwin-arm64", undefined]) {
      const name = shimBinaryBasename(t, "linux");
      expect(name.startsWith(SHIM_STEM)).toBe(true);
      expect(/tsgo/i.test(name)).toBe(false);
    }
  });
});

describe("shim binary candidate resolution order", () => {
  it("prefers VERTER_RELAY_SHIM_BINARY when set (the CI seam)", () => {
    const repoRoot = makeTmp("repo");
    const explicit = "/ci/artifacts/verter-relay-shim";
    const candidates = shimBinaryCandidates({
      vsceTarget: "linux-x64",
      env: { VERTER_RELAY_SHIM_BINARY: explicit },
      repoRoot,
      hostPlatform: "linux",
    });
    expect(candidates).toEqual([explicit]);
  });

  // G7 fail closed: an unknown --target must be rejected BEFORE the env override is honored, so a
  // typo can never bypass the unknown-target mapping even when VERTER_RELAY_SHIM_BINARY is set.
  it("validates the target before honoring VERTER_RELAY_SHIM_BINARY (unknown target fails closed)", () => {
    const repoRoot = makeTmp("repo");
    expect(() =>
      shimBinaryCandidates({
        vsceTarget: "solaris-sparc",
        env: { VERTER_RELAY_SHIM_BINARY: "/ci/artifacts/verter-relay-shim" },
        repoRoot,
        hostPlatform: "linux",
      }),
    ).toThrow(/unrecognized/i);
  });

  it("uses target/<rust-target>/{release,debug} for a platform build", () => {
    const repoRoot = makeTmp("repo");
    const candidates = shimBinaryCandidates({
      vsceTarget: "linux-arm64",
      env: {},
      repoRoot,
      hostPlatform: "win32",
    });
    expect(candidates).toEqual([
      path.join(repoRoot, "target", "aarch64-unknown-linux-gnu", "release", SHIM_STEM),
      path.join(repoRoot, "target", "aarch64-unknown-linux-gnu", "debug", SHIM_STEM),
    ]);
  });

  it("uses host target/{release,debug} for a universal build", () => {
    const repoRoot = makeTmp("repo");
    const candidates = shimBinaryCandidates({
      vsceTarget: undefined,
      env: {},
      repoRoot,
      hostPlatform: "win32",
    });
    expect(candidates).toEqual([
      path.join(repoRoot, "target", "release", `${SHIM_STEM}.exe`),
      path.join(repoRoot, "target", "debug", `${SHIM_STEM}.exe`),
    ]);
  });
});

describe("stageShimBinary copies ONLY the Verter shim into bin/", () => {
  it("stages the target-specific release binary and never any tsgo file", () => {
    const repoRoot = makeTmp("repo");
    const extensionDir = makeTmp("ext");
    const rustTarget = "x86_64-pc-windows-msvc";
    // A real shim binary AND a decoy tsgo binary sit side by side in the profile dir.
    writeFakeBinary(repoRoot, path.join(rustTarget, "release", `${SHIM_STEM}.exe`), "SHIM-BYTES");
    writeFakeBinary(repoRoot, path.join(rustTarget, "release", "tsgo.exe"), "TSGO-BYTES");

    const staged = stageShimBinary({
      vsceTarget: "win32-x64",
      env: {},
      repoRoot,
      extensionDir,
      hostPlatform: "linux",
    });

    // The shim was copied verbatim under bin/.
    expect(staged.basename).toBe(`${SHIM_STEM}.exe`);
    const dest = path.join(extensionDir, "bin", `${SHIM_STEM}.exe`);
    expect(existsSync(dest)).toBe(true);
    expect(readFileSync(dest, "utf8")).toBe("SHIM-BYTES");

    // NEGATIVE: bin/ contains ONLY the shim — no tsgo file was ever staged.
    const binEntries = readdirSync(path.join(extensionDir, "bin"));
    expect(binEntries).toEqual([`${SHIM_STEM}.exe`]);
    expect(binEntries.some((f) => /tsgo/i.test(f))).toBe(false);
  });

  it("falls back to the debug profile when release is absent", () => {
    const repoRoot = makeTmp("repo");
    const extensionDir = makeTmp("ext");
    writeFakeBinary(repoRoot, path.join("debug", SHIM_STEM), "DEBUG-SHIM");

    const staged = stageShimBinary({
      vsceTarget: undefined,
      env: {},
      repoRoot,
      extensionDir,
      hostPlatform: "linux",
    });
    expect(staged.basename).toBe(SHIM_STEM);
    expect(readFileSync(staged.dest, "utf8")).toBe("DEBUG-SHIM");
  });

  it("honors VERTER_RELAY_SHIM_BINARY as the exact source (the CI seam)", () => {
    const repoRoot = makeTmp("repo");
    const extensionDir = makeTmp("ext");
    const explicitDir = makeTmp("ci");
    const explicit = path.join(explicitDir, `${SHIM_STEM}.exe`);
    writeFileSync(explicit, "CI-SHIM");

    const staged = stageShimBinary({
      vsceTarget: "win32-x64",
      env: { VERTER_RELAY_SHIM_BINARY: explicit },
      repoRoot,
      extensionDir,
      hostPlatform: "linux",
    });
    expect(staged.source).toBe(explicit);
    expect(readFileSync(staged.dest, "utf8")).toBe("CI-SHIM");
  });
});

describe("fail-closed behavior (never auto-build, never poison a cross-target VSIX)", () => {
  it("throws an actionable error when the shim binary is missing", () => {
    const repoRoot = makeTmp("repo");
    const extensionDir = makeTmp("ext");
    expect(() =>
      resolveShimBinarySource({
        vsceTarget: "linux-x64",
        env: {},
        repoRoot,
        hostPlatform: "win32",
      }),
    ).toThrow(/not found/i);
    // No bin/ was created on the failed path.
    expect(existsSync(path.join(extensionDir, "bin"))).toBe(false);
  });

  // G7 end-to-end: even with a VALID, existing shim binary named by
  // VERTER_RELAY_SHIM_BINARY, an unknown --target fails the whole stage closed — the
  // unknown-target mapping is validated before the env override is honored.
  it("fails closed on an unknown --target even when VERTER_RELAY_SHIM_BINARY points at a real shim", () => {
    const repoRoot = makeTmp("repo");
    const extensionDir = makeTmp("ext");
    const ciDir = makeTmp("ci");
    // A perfectly good, existing shim binary sits behind the env seam.
    const good = path.join(ciDir, SHIM_STEM);
    writeFileSync(good, "GOOD-SHIM");

    expect(() =>
      stageShimBinary({
        vsceTarget: "solaris-sparc",
        env: { VERTER_RELAY_SHIM_BINARY: good },
        repoRoot,
        extensionDir,
        hostPlatform: "linux",
      }),
    ).toThrow(/unrecognized/i);
    // Nothing was staged — the unknown target short-circuits before any copy.
    expect(existsSync(path.join(extensionDir, "bin"))).toBe(false);
  });

  // The poison invariant: a PLATFORM build must NOT fall back to the host
  // target/{release,debug} dir — even if a host binary exists there. Only the
  // target-specific dir counts, else fail closed.
  it("does not stage the host binary into a cross-target VSIX", () => {
    const repoRoot = makeTmp("repo");
    const extensionDir = makeTmp("ext");
    // A host binary exists at target/release, but we are packaging for linux-x64.
    writeFakeBinary(repoRoot, path.join("release", `${SHIM_STEM}.exe`), "HOST-SHIM");

    expect(() =>
      stageShimBinary({
        vsceTarget: "linux-x64",
        env: {},
        repoRoot,
        extensionDir,
        hostPlatform: "win32",
      }),
    ).toThrow(/not found/i);
    // Nothing was staged (no host-binary poison).
    expect(existsSync(path.join(extensionDir, "bin"))).toBe(false);
  });
});

// F5: the no-tsgo defense must validate the resolved SOURCE basename, not only the
// destination name. `VERTER_RELAY_SHIM_BINARY=/path/to/tsgo(.exe)` would otherwise copy tsgo
// bytes renamed as the shim.
describe("source-basename validation (F5): never copy tsgo bytes as the shim", () => {
  it("rejects a tsgo-shaped VERTER_RELAY_SHIM_BINARY source (the CI seam)", () => {
    const repoRoot = makeTmp("repo");
    const extensionDir = makeTmp("ext");
    const ciDir = makeTmp("ci");
    // The env seam points at a real tsgo binary (a config slip / hostile override).
    const tsgoSrc = path.join(ciDir, "tsgo.exe");
    writeFileSync(tsgoSrc, "TSGO-BYTES");

    expect(() =>
      stageShimBinary({
        vsceTarget: "win32-x64",
        env: { VERTER_RELAY_SHIM_BINARY: tsgoSrc },
        repoRoot,
        extensionDir,
        hostPlatform: "linux",
      }),
    ).toThrow(/tsgo/i);

    // NEGATIVE: no tsgo bytes were staged under bin/.
    const binDir = path.join(extensionDir, "bin");
    if (existsSync(binDir)) {
      expect(readdirSync(binDir).some((f) => /tsgo/i.test(f))).toBe(false);
      expect(existsSync(path.join(binDir, `${SHIM_STEM}.exe`))).toBe(false);
    }
  });

  it("rejects a source whose basename is neither the shim nor tsgo (a mismatch)", () => {
    const repoRoot = makeTmp("repo");
    const extensionDir = makeTmp("ext");
    const ciDir = makeTmp("ci");
    // A wrong-named artifact (not tsgo, not the shim) — a config slip.
    const wrongSrc = path.join(ciDir, "some-other-binary");
    writeFileSync(wrongSrc, "WRONG");

    expect(() =>
      stageShimBinary({
        vsceTarget: "linux-x64",
        env: { VERTER_RELAY_SHIM_BINARY: wrongSrc },
        repoRoot,
        extensionDir,
        hostPlatform: "linux",
      }),
    ).toThrow(/basename|expected|verter-relay-shim/i);
  });

  it("accepts an exactly-named shim source through the env seam", () => {
    const repoRoot = makeTmp("repo");
    const extensionDir = makeTmp("ext");
    const ciDir = makeTmp("ci");
    const good = path.join(ciDir, `${SHIM_STEM}.exe`);
    writeFileSync(good, "GOOD-SHIM");

    const staged = stageShimBinary({
      vsceTarget: "win32-x64",
      env: { VERTER_RELAY_SHIM_BINARY: good },
      repoRoot,
      extensionDir,
      hostPlatform: "linux",
    });
    expect(staged.source).toBe(good);
    expect(readFileSync(staged.dest, "utf8")).toBe("GOOD-SHIM");
  });
});

// F6(a): the FINAL bin/ contents — not just the copied file — must satisfy the no-tsgo +
// single-shim invariant, so stale artifacts from a prior build never ship.
describe("final bin/ invariant (F6): no tsgo, no stale opposite-platform shim", () => {
  it("removes a stale tsgo artifact from bin/ before packaging", () => {
    const repoRoot = makeTmp("repo");
    const extensionDir = makeTmp("ext");
    writeFakeBinary(
      repoRoot,
      path.join("x86_64-pc-windows-msvc", "release", `${SHIM_STEM}.exe`),
      "SHIM",
    );
    // A stale tsgo artifact from a prior (wrong) build already sits in bin/.
    const binDir = path.join(extensionDir, "bin");
    mkdirSync(binDir, { recursive: true });
    writeFileSync(path.join(binDir, "tsgo.exe"), "STALE-TSGO");

    stageShimBinary({
      vsceTarget: "win32-x64",
      env: {},
      repoRoot,
      extensionDir,
      hostPlatform: "linux",
    });

    const entries = readdirSync(binDir);
    expect(entries.some((f) => /tsgo/i.test(f))).toBe(false);
    expect(entries).toContain(`${SHIM_STEM}.exe`);
  });

  it("prunes a stale opposite-platform shim so bin/ holds only the target shim", () => {
    const repoRoot = makeTmp("repo");
    const extensionDir = makeTmp("ext");
    writeFakeBinary(
      repoRoot,
      path.join("x86_64-pc-windows-msvc", "release", `${SHIM_STEM}.exe`),
      "WIN-SHIM",
    );
    const binDir = path.join(extensionDir, "bin");
    mkdirSync(binDir, { recursive: true });
    // A stale Unix shim from a prior universal build.
    writeFileSync(path.join(binDir, SHIM_STEM), "STALE-UNIX-SHIM");

    stageShimBinary({
      vsceTarget: "win32-x64",
      env: {},
      repoRoot,
      extensionDir,
      hostPlatform: "linux",
    });

    // bin/ holds ONLY the target shim — the stale opposite-platform shim was pruned.
    expect(readdirSync(binDir)).toEqual([`${SHIM_STEM}.exe`]);
  });
});

// F6(b): the fail-closed shim staging must run BEFORE package.mjs mutates node_modules, so a
// missing binary throws without leaving node_modules / package.json in a mutated state.
describe("packaging pipeline ordering (F6b): stage before node_modules mutation", () => {
  it("calls stageShimBinary before the first node_modules mutation in package.mjs", () => {
    const src = readFileSync(fileURLToPath(new URL("../package.mjs", import.meta.url)), "utf8");
    const stageAt = src.indexOf("stageShimBinary(");
    const mutateAt = src.indexOf("removeSafe(tsPluginDst)");
    expect(stageAt).toBeGreaterThanOrEqual(0);
    expect(mutateAt).toBeGreaterThanOrEqual(0);
    expect(stageAt).toBeLessThan(mutateAt);
  });
});

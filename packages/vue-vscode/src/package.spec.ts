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
  SHIM_IDENTITY_MARKER as MODULE_SHIM_IDENTITY_MARKER,
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
function writeFakeBinary(repoRoot: string, rel: string, bytes: Buffer | string): string {
  const full = path.join(repoRoot, "target", rel);
  mkdirSync(path.dirname(full), { recursive: true });
  writeFileSync(full, bytes);
  return full;
}

// --- Fake-binary provenance helpers -------------------------------------------------
// The staging validation guards against a wrong-artifact mixup: it checks a candidate has a
// valid executable header whose CPU arch matches the packaged target, plus the embedded
// identity marker (a PUBLIC, forgeable string — so this validates against an accidental mixup,
// it does not prove authentic provenance). These helpers synthesize MINIMAL valid headers so the
// hermetic tests exercise real byte production instead of trivial placeholder strings.

/** The identity-marker prefix embedded in the real shim binary and grepped by staging. */
const SHIM_IDENTITY_MARKER = "VERTER_RELAY_SHIM_IDENTITY:v1:";

type ExeFormat = "ELF" | "PE" | "MachO";
type ExeArch = "x86_64" | "aarch64";

/** Normalize a `process.arch`-style token to the canonical arch the validator compares. */
function canonicalArch(arch: string): ExeArch {
  if (arch === "x64" || arch === "x86_64") return "x86_64";
  if (arch === "arm64" || arch === "aarch64") return "aarch64";
  throw new Error(`test helper: unsupported host arch ${JSON.stringify(arch)}`);
}

/** The executable format the validator expects for a host platform (universal build). */
function hostFormat(hostPlatform: string): ExeFormat {
  if (hostPlatform === "win32") return "PE";
  if (hostPlatform === "darwin") return "MachO";
  return "ELF";
}

/** The (format, arch) the validator expects for a `vsceTarget`, or the host for a universal build. */
function expectedFormatArch(
  vsceTarget: string | undefined,
  hostPlatform: string,
  hostArch: string,
): { format: ExeFormat; arch: ExeArch } {
  const table: Record<string, { format: ExeFormat; arch: ExeArch }> = {
    "win32-x64": { format: "PE", arch: "x86_64" },
    "win32-arm64": { format: "PE", arch: "aarch64" },
    "linux-x64": { format: "ELF", arch: "x86_64" },
    "linux-arm64": { format: "ELF", arch: "aarch64" },
    "darwin-x64": { format: "MachO", arch: "x86_64" },
    "darwin-arm64": { format: "MachO", arch: "aarch64" },
  };
  if (vsceTarget) return table[vsceTarget];
  return { format: hostFormat(hostPlatform), arch: canonicalArch(hostArch) };
}

/** Build a MINIMAL valid executable header (magic + machine only) for a (format, arch). */
function fakeArchHeader(format: ExeFormat, arch: ExeArch): Buffer {
  if (format === "ELF") {
    const buf = Buffer.alloc(64, 0);
    buf[0] = 0x7f;
    buf[1] = 0x45; // 'E'
    buf[2] = 0x4c; // 'L'
    buf[3] = 0x46; // 'F'
    buf[4] = 2; // ELFCLASS64
    buf[5] = 1; // little-endian
    buf[6] = 1; // EV_CURRENT
    buf.writeUInt16LE(arch === "x86_64" ? 0x3e : 0xb7, 18); // e_machine
    return buf;
  }
  if (format === "PE") {
    const peOff = 0x80;
    const buf = Buffer.alloc(peOff + 24, 0);
    buf[0] = 0x4d; // 'M'
    buf[1] = 0x5a; // 'Z'
    buf.writeUInt32LE(peOff, 0x3c); // e_lfanew
    buf[peOff] = 0x50; // 'P'
    buf[peOff + 1] = 0x45; // 'E'
    buf[peOff + 2] = 0;
    buf[peOff + 3] = 0;
    buf.writeUInt16LE(arch === "x86_64" ? 0x8664 : 0xaa64, peOff + 4); // Machine
    return buf;
  }
  // Mach-O thin, little-endian, 64-bit (MH_MAGIC_64 = 0xFEEDFACF → CF FA ED FE on disk).
  const buf = Buffer.alloc(32, 0);
  buf[0] = 0xcf;
  buf[1] = 0xfa;
  buf[2] = 0xed;
  buf[3] = 0xfe;
  buf.writeUInt32LE(arch === "x86_64" ? 0x01000007 : 0x0100000c, 4); // cputype
  return buf;
}

/**
 * A MINIMAL fake shim binary the provenance validator accepts: a valid arch header for
 * the packaged target (or the host, for a universal build) followed by the embedded
 * identity marker + padding. The hermetic tests write these bytes so they satisfy the
 * arch + identity validation while preserving each test's original intent.
 */
function fakeShimBytes(
  vsceTarget: string | undefined,
  hostPlatform: string = process.platform,
  hostArch: string = process.arch,
): Buffer {
  const { format, arch } = expectedFormatArch(vsceTarget, hostPlatform, hostArch);
  const header = fakeArchHeader(format, arch);
  const marker = Buffer.from(`${SHIM_IDENTITY_MARKER}0.0.0-test\n`, "utf8");
  const padding = Buffer.alloc(16, 0);
  return Buffer.concat([header, marker, padding]);
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

  it("uses target/<rust-target>/{release,debug} for a platform build (debug is opt-in)", () => {
    const repoRoot = makeTmp("repo");
    const candidates = shimBinaryCandidates({
      vsceTarget: "linux-arm64",
      env: {},
      repoRoot,
      hostPlatform: "win32",
      allowDebug: true,
    });
    expect(candidates).toEqual([
      path.join(repoRoot, "target", "aarch64-unknown-linux-gnu", "release", SHIM_STEM),
      path.join(repoRoot, "target", "aarch64-unknown-linux-gnu", "debug", SHIM_STEM),
    ]);
  });

  it("uses host target/{release,debug} for a universal build (dev-only, allowUniversal)", () => {
    const repoRoot = makeTmp("repo");
    const candidates = shimBinaryCandidates({
      vsceTarget: undefined,
      env: {},
      repoRoot,
      hostPlatform: "win32",
      allowUniversal: true,
      allowDebug: true,
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
    const shimBytes = fakeShimBytes("win32-x64");
    // A real shim binary AND a decoy tsgo binary sit side by side in the profile dir.
    writeFakeBinary(repoRoot, path.join(rustTarget, "release", `${SHIM_STEM}.exe`), shimBytes);
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
    expect(readFileSync(dest)).toEqual(shimBytes);

    // NEGATIVE: bin/ contains ONLY the shim — no tsgo file was ever staged.
    const binEntries = readdirSync(path.join(extensionDir, "bin"));
    expect(binEntries).toEqual([`${SHIM_STEM}.exe`]);
    expect(binEntries.some((f) => /tsgo/i.test(f))).toBe(false);
  });

  it("falls back to the debug profile when release is absent", () => {
    const repoRoot = makeTmp("repo");
    const extensionDir = makeTmp("ext");
    const shimBytes = fakeShimBytes(undefined, "linux");
    writeFakeBinary(repoRoot, path.join("debug", SHIM_STEM), shimBytes);

    const staged = stageShimBinary({
      vsceTarget: undefined,
      env: {},
      repoRoot,
      extensionDir,
      hostPlatform: "linux",
      allowUniversal: true,
      allowDebug: true,
    });
    expect(staged.basename).toBe(SHIM_STEM);
    expect(readFileSync(staged.dest)).toEqual(shimBytes);
  });

  it("honors VERTER_RELAY_SHIM_BINARY as the exact source (the CI seam)", () => {
    const repoRoot = makeTmp("repo");
    const extensionDir = makeTmp("ext");
    const explicitDir = makeTmp("ci");
    const explicit = path.join(explicitDir, `${SHIM_STEM}.exe`);
    const shimBytes = fakeShimBytes("win32-x64");
    writeFileSync(explicit, shimBytes);

    const staged = stageShimBinary({
      vsceTarget: "win32-x64",
      env: { VERTER_RELAY_SHIM_BINARY: explicit },
      repoRoot,
      extensionDir,
      hostPlatform: "linux",
    });
    expect(staged.source).toBe(explicit);
    expect(readFileSync(staged.dest)).toEqual(shimBytes);
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
    const shimBytes = fakeShimBytes("win32-x64");
    writeFileSync(good, shimBytes);

    const staged = stageShimBinary({
      vsceTarget: "win32-x64",
      env: { VERTER_RELAY_SHIM_BINARY: good },
      repoRoot,
      extensionDir,
      hostPlatform: "linux",
    });
    expect(staged.source).toBe(good);
    expect(readFileSync(staged.dest)).toEqual(shimBytes);
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
      fakeShimBytes("win32-x64"),
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
      fakeShimBytes("win32-x64"),
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

// 10-JS: basename validation is not enough — the staged BYTES must prove they are the
// Verter shim: the executable format + CPU arch must match the packaged target, the
// source must be an absolute, regular, non-symlink file, and the bytes must carry the
// embedded shim identity marker. A renamed tsgo / wrong-arch / unrelated binary is
// refused BEFORE anything is staged.
describe("staged-byte provenance (10-JS): arch + identity + real source", () => {
  it("stage_rejects_wrong_arch_binary", () => {
    const repoRoot = makeTmp("repo");
    const extensionDir = makeTmp("ext");
    const ciDir = makeTmp("ci");
    // A correctly-named, identity-bearing shim — but built for the WRONG arch: an
    // ELF/x86_64 binary staged for linux-arm64 (which must be ELF/aarch64).
    const src = path.join(ciDir, SHIM_STEM);
    writeFileSync(src, fakeShimBytes("linux-x64"));

    expect(() =>
      stageShimBinary({
        vsceTarget: "linux-arm64",
        env: { VERTER_RELAY_SHIM_BINARY: src },
        repoRoot,
        extensionDir,
        hostPlatform: "linux",
      }),
    ).toThrow(/arch|architecture|machine/i);

    // Fail closed BEFORE any bin/ mutation.
    expect(existsSync(path.join(extensionDir, "bin"))).toBe(false);
  });

  it("stage_requires_embedded_shim_identity", () => {
    const repoRoot = makeTmp("repo");
    const extensionDir = makeTmp("ext");
    const ciDir = makeTmp("ci");
    // Correct name + correct arch header, but NO embedded identity marker — e.g. an
    // unrelated binary that merely shares the target arch. It must be rejected.
    const src = path.join(ciDir, `${SHIM_STEM}.exe`);
    writeFileSync(src, fakeArchHeader("PE", "x86_64"));

    expect(() =>
      stageShimBinary({
        vsceTarget: "win32-x64",
        env: { VERTER_RELAY_SHIM_BINARY: src },
        repoRoot,
        extensionDir,
        hostPlatform: "linux",
      }),
    ).toThrow(/identity|marker/i);

    expect(existsSync(path.join(extensionDir, "bin"))).toBe(false);
  });

  it("stage_rejects_symlink_or_relative_source", () => {
    const repoRoot = makeTmp("repo");
    const extensionDir = makeTmp("ext");
    const ciDir = makeTmp("ci");
    const real = path.join(ciDir, `${SHIM_STEM}.exe`);
    writeFileSync(real, fakeShimBytes("win32-x64"));

    // (a) A SYMLINK source is rejected — modeled via an injected lstat that reports a
    // symlink, so the test is hermetic on Windows (no real symlink privilege needed).
    expect(() =>
      stageShimBinary({
        vsceTarget: "win32-x64",
        env: { VERTER_RELAY_SHIM_BINARY: real },
        repoRoot,
        extensionDir,
        hostPlatform: "linux",
        lstat: () => ({ isSymbolicLink: () => true, isFile: () => false }),
      }),
    ).toThrow(/symlink|symbolic|regular/i);

    // (b) A RELATIVE VERTER_RELAY_SHIM_BINARY is rejected — fully injected FS so the
    // guard under test is the absolute-path check, not an incidental missing-file error.
    expect(() =>
      stageShimBinary({
        vsceTarget: "win32-x64",
        env: { VERTER_RELAY_SHIM_BINARY: "relative/verter-relay-shim.exe" },
        repoRoot,
        extensionDir,
        hostPlatform: "linux",
        exists: () => true,
        copy: () => {},
        mkdir: () => {},
        readdir: () => [`${SHIM_STEM}.exe`],
        remove: () => {},
      }),
    ).toThrow(/absolute/i);

    // Neither rejection created a bin/ directory.
    expect(existsSync(path.join(extensionDir, "bin"))).toBe(false);
  });
});

// 11: a targetless "universal" build silently fell back to the HOST binary — a VSIX
// with no platform target installs anywhere. Production packaging now REQUIRES --target;
// the host-dir fallback is reachable ONLY behind an explicit allowUniversal (dev) flag.
describe("production requires --target; host fallback is dev-only (11)", () => {
  it("production_package_requires_vsce_target", () => {
    const repoRoot = makeTmp("repo");
    const extensionDir = makeTmp("ext");

    // No target and no allowUniversal → fail closed at candidate resolution.
    expect(() =>
      shimBinaryCandidates({ vsceTarget: undefined, env: {}, repoRoot, hostPlatform: "linux" }),
    ).toThrow(/target|universal/i);

    // ...and through the full stage (nothing is staged).
    expect(() =>
      stageShimBinary({
        vsceTarget: undefined,
        env: {},
        repoRoot,
        extensionDir,
        hostPlatform: "linux",
      }),
    ).toThrow(/target|universal/i);
    expect(existsSync(path.join(extensionDir, "bin"))).toBe(false);

    // With the explicit dev flag, the host target/ dir IS used (the universal dev path).
    const candidates = shimBinaryCandidates({
      vsceTarget: undefined,
      env: {},
      repoRoot,
      hostPlatform: "linux",
      allowUniversal: true,
    });
    expect(candidates).toContain(path.join(repoRoot, "target", "release", SHIM_STEM));
  });
});

// 12: packaging could silently fall back to a DEBUG-profile binary for production.
// Candidates are release-only by default; the debug profile is opt-in (allowDebug), a
// dev-only fallback that package.mjs never passes.
describe("release-only by default; debug is opt-in (12)", () => {
  it("production_candidates_never_include_debug_profile", () => {
    const repoRoot = makeTmp("repo");

    // Platform build: default candidates are release-only — NO debug-profile path.
    const platformDefault = shimBinaryCandidates({
      vsceTarget: "linux-x64",
      env: {},
      repoRoot,
      hostPlatform: "linux",
    });
    expect(platformDefault).toEqual([
      path.join(repoRoot, "target", "x86_64-unknown-linux-gnu", "release", SHIM_STEM),
    ]);
    expect(platformDefault.every((c) => !/[\\/]debug[\\/]/.test(c))).toBe(true);

    // allowDebug: true adds the debug candidate AFTER release (dev-only fallback).
    const platformDebug = shimBinaryCandidates({
      vsceTarget: "linux-x64",
      env: {},
      repoRoot,
      hostPlatform: "linux",
      allowDebug: true,
    });
    expect(platformDebug).toEqual([
      path.join(repoRoot, "target", "x86_64-unknown-linux-gnu", "release", SHIM_STEM),
      path.join(repoRoot, "target", "x86_64-unknown-linux-gnu", "debug", SHIM_STEM),
    ]);

    // The universal (dev) build is release-only by default too.
    const universalDefault = shimBinaryCandidates({
      vsceTarget: undefined,
      env: {},
      repoRoot,
      hostPlatform: "linux",
      allowUniversal: true,
    });
    expect(universalDefault).toEqual([path.join(repoRoot, "target", "release", SHIM_STEM)]);

    // package.mjs must NOT enable the debug fallback for production packaging.
    const pkgSrc = readFileSync(fileURLToPath(new URL("../package.mjs", import.meta.url)), "utf8");
    expect(pkgSrc.includes("allowDebug")).toBe(false);
  });
});

// 13: the final bin/ invariant allowed UNRELATED files to survive (it only pruned
// tsgo-shaped + stale shim* names). bin/ is now an explicit whitelist — exactly the
// staged shim — so any other entry is pruned and the final dir is asserted equal to it.
describe("strict bin/ whitelist (13): bin/ is exactly the staged shim", () => {
  it("stage_rejects_unrelated_bin_entry", () => {
    const repoRoot = makeTmp("repo");
    const extensionDir = makeTmp("ext");
    writeFakeBinary(
      repoRoot,
      path.join("x86_64-pc-windows-msvc", "release", `${SHIM_STEM}.exe`),
      fakeShimBytes("win32-x64"),
    );
    // Unrelated files (neither tsgo nor a shim) already sit in bin/ from some prior step.
    const binDir = path.join(extensionDir, "bin");
    mkdirSync(binDir, { recursive: true });
    writeFileSync(path.join(binDir, "NOTICE.txt"), "unrelated");
    writeFileSync(path.join(binDir, "leftover.bin"), "junk");

    stageShimBinary({
      vsceTarget: "win32-x64",
      env: {},
      repoRoot,
      extensionDir,
      hostPlatform: "linux",
    });

    // bin/ is EXACTLY the staged shim — every unrelated entry was pruned.
    expect(readdirSync(binDir).sort()).toEqual([`${SHIM_STEM}.exe`]);
  });
});

// 14: the staged Unix binary's executable bit was never asserted, so a copy that dropped
// the +x bit (or a source without it) would ship a non-executable shim. Staging now
// chmods the dest to 0o755 for a Unix TARGET (no-op for a Windows target).
describe("executable bit on Unix targets (14)", () => {
  it("unix_target_chmods_staged_shim_executable", () => {
    // A Unix TARGET (even on a Windows host) chmods the staged shim to 0o755.
    {
      const repoRoot = makeTmp("repo");
      const extensionDir = makeTmp("ext");
      writeFakeBinary(
        repoRoot,
        path.join("x86_64-unknown-linux-gnu", "release", SHIM_STEM),
        fakeShimBytes("linux-x64"),
      );
      const chmodCalls: Array<[string, number]> = [];
      const staged = stageShimBinary({
        vsceTarget: "linux-x64",
        env: {},
        repoRoot,
        extensionDir,
        hostPlatform: "win32",
        chmod: (p: string, mode: number) => chmodCalls.push([p, mode]),
      });
      expect(chmodCalls).toEqual([[staged.dest, 0o755]]);
    }

    // A Windows TARGET never chmods (the +x bit is meaningless on NTFS).
    {
      const repoRoot = makeTmp("repo");
      const extensionDir = makeTmp("ext");
      writeFakeBinary(
        repoRoot,
        path.join("x86_64-pc-windows-msvc", "release", `${SHIM_STEM}.exe`),
        fakeShimBytes("win32-x64"),
      );
      const chmodCalls: Array<[string, number]> = [];
      stageShimBinary({
        vsceTarget: "win32-x64",
        env: {},
        repoRoot,
        extensionDir,
        hostPlatform: "linux",
        chmod: (p: string, mode: number) => chmodCalls.push([p, mode]),
      });
      expect(chmodCalls).toEqual([]);
    }
  });
});

// The staged DEST bytes must be re-validated AFTER the copy, not just the pre-copy source: a source
// swapped between the source check and the copy would otherwise ship unvalidated bytes (a
// validate-then-copy TOCTOU). Re-running the wrong-artifact guard on the dest closes it.
describe("staged-DEST re-validation (TOCTOU): the FINAL bytes are validated, not just the source", () => {
  it("fails closed when the copied dest bytes differ from the validated source", () => {
    const repoRoot = makeTmp("repo");
    const extensionDir = makeTmp("ext");
    const ciDir = makeTmp("ci");
    const src = path.join(ciDir, `${SHIM_STEM}.exe`);
    // The SOURCE on disk is a valid, identity-bearing shim → the source check passes.
    writeFileSync(src, fakeShimBytes("win32-x64"));
    // ...but the COPY lands DIFFERENT bytes at dest (model a swap between validate and copy): a
    // valid PE/x86_64 header WITHOUT the identity marker (a renamed / wrong artifact).
    const tampered = fakeArchHeader("PE", "x86_64");

    expect(() =>
      stageShimBinary({
        vsceTarget: "win32-x64",
        env: { VERTER_RELAY_SHIM_BINARY: src },
        repoRoot,
        extensionDir,
        hostPlatform: "linux",
        copy: (_s: string, dst: string) => writeFileSync(dst, tampered),
      }),
    ).toThrow(/identity|marker/i);
  });

  it("accepts a dest whose bytes match the validated source (the normal copy)", () => {
    const repoRoot = makeTmp("repo");
    const extensionDir = makeTmp("ext");
    const ciDir = makeTmp("ci");
    const src = path.join(ciDir, `${SHIM_STEM}.exe`);
    const shimBytes = fakeShimBytes("win32-x64");
    writeFileSync(src, shimBytes);

    const staged = stageShimBinary({
      vsceTarget: "win32-x64",
      env: { VERTER_RELAY_SHIM_BINARY: src },
      repoRoot,
      extensionDir,
      hostPlatform: "linux",
    });
    // The FINAL staged bytes equal the validated source — the dest re-validation passed.
    expect(readFileSync(staged.dest)).toEqual(shimBytes);
  });
});

// bin/ prune must remove a DIRECTORY entry too — a bare `rmSync(path, { force: true })` throws
// EISDIR on a directory, so an unlisted directory would otherwise survive the strict whitelist.
describe("bin/ prune removes stale directory entries, not just files", () => {
  it("prunes an unlisted directory in bin/ so the final dir is exactly the shim", () => {
    const repoRoot = makeTmp("repo");
    const extensionDir = makeTmp("ext");
    writeFakeBinary(
      repoRoot,
      path.join("x86_64-pc-windows-msvc", "release", `${SHIM_STEM}.exe`),
      fakeShimBytes("win32-x64"),
    );
    const binDir = path.join(extensionDir, "bin");
    mkdirSync(binDir, { recursive: true });
    // A stale DIRECTORY (with contents) from a prior step sits in bin/ alongside the shim slot.
    const staleDir = path.join(binDir, "stale-subdir");
    mkdirSync(staleDir, { recursive: true });
    writeFileSync(path.join(staleDir, "leftover.txt"), "junk");

    stageShimBinary({
      vsceTarget: "win32-x64",
      env: {},
      repoRoot,
      extensionDir,
      hostPlatform: "linux",
    });

    // The stale directory was pruned (recursive removal); bin/ is EXACTLY the staged shim.
    expect(readdirSync(binDir)).toEqual([`${SHIM_STEM}.exe`]);
  });
});

// The DEFAULT `package` script must fail closed on universal: `node package.mjs` with no --target
// errors during shim staging (production requires --target). The dev host-binary fallback
// (--allow-universal) lives in a separate, clearly-named `package:dev:universal` script, never the
// default entrypoint — otherwise a bare `pnpm package` silently ships a host-arch, install-anywhere
// VSIX.
describe("default package script fails closed on universal", () => {
  it("the `package` script requires --target and never passes --allow-universal", () => {
    const pkgPath = fileURLToPath(new URL("../package.json", import.meta.url));
    const pkg = JSON.parse(readFileSync(pkgPath, "utf8"));
    const scripts = (pkg.scripts ?? {}) as Record<string, string>;

    // The default entrypoint is fail-closed: no --allow-universal.
    expect(scripts.package).toBe("node package.mjs");
    expect(scripts.package).not.toContain("--allow-universal");

    // The dev host-binary fallback is a SEPARATE, explicitly-named script.
    expect(scripts["package:dev:universal"]).toBeDefined();
    expect(scripts["package:dev:universal"]).toContain("--allow-universal");

    // The per-target production scripts are unaffected — each still passes --target and never
    // --allow-universal.
    for (const [name, cmd] of Object.entries(scripts)) {
      if (name.startsWith("package:") && name !== "package:dev:universal") {
        expect(cmd).toContain("--target");
        expect(cmd).not.toContain("--allow-universal");
      }
    }
  });
});

// Cross-language drift guard: the shim identity marker is a CLOSED contract embedded by
// the Rust binary (crates/verter_relay_shim/src/main.rs) and grepped by this JS staging
// module. Both sides MUST pin the identical literal — if either drifts, packaging can no
// longer prove a candidate's bytes are the shim. This fails if either side's prefix changes.
describe("shim identity marker is a pinned cross-language contract", () => {
  it("shim_identity_marker_prefix_matches_rust_and_js", () => {
    const PINNED = "VERTER_RELAY_SHIM_IDENTITY:v1:";

    // The JS module's exported constant is the pinned literal.
    expect(MODULE_SHIM_IDENTITY_MARKER).toBe(PINNED);

    // The JS staging module source embeds it (the grep literal staging scans candidates for).
    const stageBinSrc = readFileSync(
      fileURLToPath(new URL("../stage-bin.mjs", import.meta.url)),
      "utf8",
    );
    expect(stageBinSrc).toContain(PINNED);

    // The Rust shim binary embeds the SAME literal in its identity marker (SHIM_IDENTITY).
    const mainRsSrc = readFileSync(
      fileURLToPath(new URL("../../../crates/verter_relay_shim/src/main.rs", import.meta.url)),
      "utf8",
    );
    expect(mainRsSrc).toContain(PINNED);

    // ...and the Rust literal is exactly the JS module constant — no cross-language drift.
    expect(mainRsSrc).toContain(MODULE_SHIM_IDENTITY_MARKER);
  });
});

/**
 * Canonical supported-platform matrix for `@verter/native`, derived from
 * the AUTHORITATIVE source: `package.json#napi.targets` (the rust-target
 * list the napi build is driven by). Every other location that enumerates
 * platforms — `optionalDependencies`, the `npm/<triple>/` template dirs,
 * the generated `dist/index.js` loader, and the `release.yml` build matrix
 * — is reconciled against THIS matrix (see `platform-matrix.spec.ts`), so
 * a triple added or dropped in one place but not the others fails loudly
 * instead of shipping a half-wired platform.
 *
 * The napi triple and the os/cpu/libc fields are computed from the
 * rust-target's own components (arch + os + abi) via an explicit, total
 * decomposition — NOT copied from `optionalDependencies` or the npm
 * templates (those are the things under test, and deriving the expected
 * value from a thing-under-test would make the reconciliation vacuous).
 *
 * This is a committed test-support module (consumed by the hermetic
 * specs and available to CI tooling). It is intentionally NOT added to
 * `package.json#files`: the published package surface is the wrapper +
 * generated loader + types, and the loader already bakes the platform
 * decisions at build time — the matrix here exists to GUARD that the
 * committed enumerations agree, not to be a runtime export.
 */

import { execSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const packageDir = dirname(fileURLToPath(import.meta.url));

/** A node-style libc tag as carried by `package.json#libc`. */
export type LibcTag = "glibc" | "musl";

/** One fully-reconciled platform row. */
export interface PlatformEntry {
  /** The rust target triple, e.g. `x86_64-unknown-linux-gnu`. */
  readonly rustTarget: string;
  /** The napi platform triple, e.g. `linux-x64-gnu`. */
  readonly napiTriple: string;
  /** The per-platform optional-dependency package name. */
  readonly packageName: string;
  /** The `.node` filename shipped by the optional-dependency package. */
  readonly nodeFileName: string;
  /** Node's `process.platform` value for this target. */
  readonly os: NodeJS.Platform;
  /** Node's `process.arch` value for this target. */
  readonly cpu: string;
  /**
   * The libc tag, present ONLY for targets whose package template carries
   * a `libc` field (the linux gnu/musl split). `undefined` for darwin and
   * windows, which have no `libc` constraint.
   */
  readonly libc?: LibcTag;
}

const BINARY_NAME = "verter-native";
const PACKAGE_SCOPE = "@verter/native-";

/** rust-arch → node `process.arch`. */
const ARCH_MAP: Record<string, string> = {
  x86_64: "x64",
  aarch64: "arm64",
};

/**
 * Decompose a rust target triple into the napi platform components.
 *
 * Rust targets are `<arch>-<vendor>-<os>[-<abi>]`. We only support the
 * 7 targets in `napi.targets`; anything else throws so a newly-added
 * target without a mapping cannot silently produce a wrong triple.
 */
function decomposeRustTarget(rustTarget: string): {
  napiTriple: string;
  os: NodeJS.Platform;
  cpu: string;
  libc?: LibcTag;
} {
  const parts = rustTarget.split("-");
  const rustArch = parts[0];
  const cpu = ARCH_MAP[rustArch];
  if (!cpu) {
    throw new Error(
      `platforms.ts: unmapped rust arch "${rustArch}" in target "${rustTarget}". ` +
        `Add it to ARCH_MAP.`,
    );
  }

  // Apple: `<arch>-apple-darwin` → `darwin-<cpu>`, no libc.
  if (rustTarget.includes("-apple-darwin")) {
    return { napiTriple: `darwin-${cpu}`, os: "darwin", cpu };
  }

  // Windows MSVC: `<arch>-pc-windows-msvc` → `win32-<cpu>-msvc`, no libc.
  if (rustTarget.endsWith("-pc-windows-msvc")) {
    return { napiTriple: `win32-${cpu}-msvc`, os: "win32", cpu };
  }

  // Linux gnu/musl: `<arch>-unknown-linux-(gnu|musl)` → `linux-<cpu>-(gnu|musl)`.
  // The package template's `libc` tag is `glibc` for gnu and `musl` for musl.
  if (rustTarget.endsWith("-unknown-linux-gnu")) {
    return { napiTriple: `linux-${cpu}-gnu`, os: "linux", cpu, libc: "glibc" };
  }
  if (rustTarget.endsWith("-unknown-linux-musl")) {
    return { napiTriple: `linux-${cpu}-musl`, os: "linux", cpu, libc: "musl" };
  }

  throw new Error(
    `platforms.ts: unmapped rust target "${rustTarget}". ` +
      `Extend decomposeRustTarget() with its napi triple + os/cpu/libc.`,
  );
}

function buildEntry(rustTarget: string): PlatformEntry {
  const { napiTriple, os, cpu, libc } = decomposeRustTarget(rustTarget);
  return {
    rustTarget,
    napiTriple,
    packageName: `${PACKAGE_SCOPE}${napiTriple}`,
    nodeFileName: `${BINARY_NAME}.${napiTriple}.node`,
    os,
    cpu,
    ...(libc ? { libc } : {}),
  };
}

/**
 * Read the authoritative rust-target list from `package.json#napi.targets`.
 * `packageJsonPath` is overridable so a reconciliation spec can point the
 * derivation at a temp-edited package.json to PROVE the matrix tracks the
 * source (discrimination), but defaults to the real package.json.
 */
export function readNapiTargets(packageJsonPath = join(packageDir, "package.json")): string[] {
  const pkg = JSON.parse(readFileSync(packageJsonPath, "utf8")) as {
    napi?: { targets?: string[] };
  };
  const targets = pkg.napi?.targets;
  if (!Array.isArray(targets) || targets.length === 0) {
    throw new Error(
      `platforms.ts: package.json#napi.targets is missing or empty at ${packageJsonPath}.`,
    );
  }
  return targets;
}

/**
 * The canonical platform matrix, derived from `package.json#napi.targets`.
 * Pass an explicit target list (e.g. a temp-edited copy) to derive a
 * variant matrix for discrimination testing.
 */
export function buildPlatformMatrix(rustTargets: string[] = readNapiTargets()): PlatformEntry[] {
  return rustTargets.map(buildEntry);
}

/** The canonical matrix for the real, committed `package.json`. */
export const PLATFORM_MATRIX: readonly PlatformEntry[] = buildPlatformMatrix();

/** The set of napi triples the package supports, canonical order. */
export function napiTriples(matrix: readonly PlatformEntry[] = PLATFORM_MATRIX): string[] {
  return matrix.map((e) => e.napiTriple);
}

/** The set of optional-dependency package names, canonical order. */
export function optionalDependencyPackageNames(
  matrix: readonly PlatformEntry[] = PLATFORM_MATRIX,
): string[] {
  return matrix.map((e) => e.packageName);
}

/**
 * The shape of the `process.report.getReport()` payload the loader reads when
 * deciding musl-vs-gnu from the process report. Only the two fields the loader
 * actually inspects are modelled (`header.glibcVersionRuntime` and
 * `sharedObjects`); a real report carries far more, all irrelevant here.
 */
interface MuslReport {
  readonly header?: { readonly glibcVersionRuntime?: string };
  readonly sharedObjects?: readonly string[];
}

/**
 * The three host probes the loader's `isMusl()` consults, injected so the
 * detection algorithm can be driven with faked sources in a discrimination
 * test WITHOUT a second copy of the algorithm. {@link detectHostIsMusl} wires
 * the REAL host sources in; a test wires fakes in. There is exactly ONE musl
 * algorithm ({@link resolveHostMusl}); these are only its inputs.
 */
export interface HostMuslProbes {
  /**
   * Read the ldd binary the loader reads (`/usr/bin/ldd`). Return its contents
   * on success, or throw on any fs error (the real loader catches the throw).
   */
  readonly readLddBinary: () => string;
  /**
   * `process.report.getReport()` if a report facility exists, else `null`
   * (mirrors the loader's `typeof process.report?.getReport === 'function'`
   * guard).
   */
  readonly getReport: () => MuslReport | null;
  /**
   * Run `ldd --version` and return its output (the loader's last-resort
   * `isMuslFromChildProcess`). Throws when no `ldd` is reachable.
   */
  readonly lddVersion: () => string;
}

/**
 * A shared-object/ldd path is musl iff it carries the musl libc marker. This
 * is a BYTE-FOR-BYTE port of the generated loader's `isFileMusl`
 * (`dist/index.js` line 24): `f.includes('libc.musl-') || f.includes('ld-musl-')`.
 */
function isFileMusl(f: string): boolean {
  return f.includes("libc.musl-") || f.includes("ld-musl-");
}

/**
 * Filesystem ldd probe — port of the loader's `isMuslFromFilesystem`
 * (`dist/index.js` lines 26-32): read `/usr/bin/ldd`; musl iff its contents
 * include `"musl"`; `null` (inconclusive — defer to the next probe) on ANY
 * read error. A definite boolean when the read succeeds.
 */
function muslFromFilesystem(probes: HostMuslProbes): boolean | null {
  try {
    return probes.readLddBinary().includes("musl");
  } catch {
    return null;
  }
}

/**
 * `process.report` probe — port of the loader's `isMuslFromReport`
 * (`dist/index.js` lines 34-52). EXACT branch order:
 *   1. no report facility ⇒ `null` (inconclusive — defer to child-process);
 *   2. `header.glibcVersionRuntime` present ⇒ `false` (glibc/gnu) — short
 *      circuits BEFORE `sharedObjects` is consulted;
 *   3. `sharedObjects` is an array AND some entry matches {@link isFileMusl}
 *      ⇒ `true` (musl);
 *   4. otherwise ⇒ `false` (a report with neither signal is glibc).
 * NOTE: once a report EXISTS this returns a definite boolean (steps 2-4); it
 * is `null` ONLY when there is no report facility — matching the loader, whose
 * child-process fallback runs solely when both fs AND report are unavailable.
 */
function muslFromReport(probes: HostMuslProbes): boolean | null {
  const report = probes.getReport();
  if (!report) {
    return null;
  }
  if (report.header && report.header.glibcVersionRuntime) {
    return false;
  }
  if (Array.isArray(report.sharedObjects)) {
    if (report.sharedObjects.some(isFileMusl)) {
      return true;
    }
  }
  return false;
}

/**
 * Child-process `ldd --version` probe — port of the loader's
 * `isMuslFromChildProcess` (`dist/index.js` lines 54-61): musl iff
 * `ldd --version` output includes `"musl"`; `false` when no `ldd` is reachable
 * (the loader's "we don't know, fall back to false" terminal case).
 */
function muslFromChildProcess(probes: HostMuslProbes): boolean {
  try {
    return probes.lddVersion().includes("musl");
  } catch {
    // If we reach this case, we don't know if the system is musl or not, so it
    // is better to just fall back to false (mirrors the loader's comment).
    return false;
  }
}

/**
 * The SINGLE host musl-detection algorithm, a line-for-line port of the
 * generated loader's `isMusl()` (`dist/index.js` lines 10-22). It short
 * circuits on the first probe that returns non-null, in the loader's EXACT
 * order: filesystem ldd → `process.report` → child-process `ldd --version`,
 * defaulting to `false` (gnu) when every probe is inconclusive. Both the host
 * detector ({@link detectHostIsMusl}) and the discrimination test route
 * through THIS function — there is no second musl algorithm.
 *
 * `probes` is injected so the test can feed faked fs/report/child-process
 * sources and assert the gnu-vs-musl decision matches the loader on each host
 * shape; {@link detectHostIsMusl} feeds the real host sources.
 */
export function resolveHostMusl(probes: HostMuslProbes): boolean {
  let musl: boolean | null = muslFromFilesystem(probes);
  if (musl === null) {
    musl = muslFromReport(probes);
  }
  if (musl === null) {
    musl = muslFromChildProcess(probes);
  }
  return musl;
}

/**
 * Detect whether the REAL host runs musl libc by feeding the real host
 * sources into the shared {@link resolveHostMusl} algorithm — which is a
 * line-for-line port of the generated `dist/index.js` loader's `isMusl()`
 * (lines 10-22, with `isMuslFromFilesystem`/`isMuslFromReport`/
 * `isMuslFromChildProcess`/`isFileMusl` at lines 24-61). The detector and the
 * loader therefore return the SAME musl/gnu decision on any Linux host: the
 * filesystem ldd probe wins when readable, else the process report decides
 * (glibcVersionRuntime ⇒ gnu, an `ld-musl-`/`libc.musl-` shared object ⇒
 * musl), else `ldd --version` decides, else gnu.
 *
 * This is the SINGLE host-side musl detector for the test-support layer; specs
 * that need the host triple (the offline tarball smoke, the fallback spec)
 * call {@link currentHostEntry} rather than each re-deriving musl. Returns
 * `false` on non-linux (no musl concept), matching the loader's
 * `process.platform === 'linux'` guard.
 */
export function detectHostIsMusl(): boolean {
  if (process.platform !== "linux") return false;
  return resolveHostMusl({
    readLddBinary: () => readFileSync("/usr/bin/ldd", "utf-8"),
    getReport: () => {
      if (typeof process.report?.getReport !== "function") return null;
      // The loader sets `excludeNetwork = true` before `getReport()` (so the
      // synchronous report does not block on network probes); mirror that.
      const reportFacility = process.report as { excludeNetwork?: boolean };
      reportFacility.excludeNetwork = true;
      return process.report.getReport() as MuslReport;
    },
    // The loader does `require('child_process').execSync('ldd --version', …)`;
    // a static import of `execSync` here is equivalent — importing the module
    // never spawns a process, only this call (reached solely when both the fs
    // and report probes are inconclusive) does.
    lddVersion: () => execSync("ldd --version", { encoding: "utf8" }),
  });
}

/**
 * The canonical {@link PlatformEntry} for the CURRENT host, or `null` if this
 * platform/arch (with musl detection for linux) is not in the supported
 * matrix. Derives the napi triple from `process.platform` + `process.arch` +
 * {@link detectHostIsMusl} and looks it up in {@link PLATFORM_MATRIX} — so a
 * SUPPORTED host yields its real `.node` filename + package name, and a
 * genuinely unsupported host yields `null` (the caller may then loud-skip).
 * The triple is computed independently of `optionalDependencies` (the thing
 * under test), exactly like the matrix derivation.
 */
export function currentHostEntry(
  matrix: readonly PlatformEntry[] = PLATFORM_MATRIX,
): PlatformEntry | null {
  const { platform, arch } = process;
  const triple = hostNapiTriple(platform, arch);
  if (triple === null) return null;
  return matrix.find((e) => e.napiTriple === triple) ?? null;
}

/**
 * Map a `process.platform`/`process.arch` (+ host musl detection for linux)
 * to the napi triple string, or `null` for a platform/arch we never ship.
 * Kept total and explicit: an arch we do not publish returns `null` (loud
 * skip) rather than fabricating a wrong triple.
 */
function hostNapiTriple(platform: NodeJS.Platform, arch: string): string | null {
  if (platform === "win32") {
    return arch === "x64" ? "win32-x64-msvc" : null;
  }
  if (platform === "darwin") {
    if (arch === "x64") return "darwin-x64";
    if (arch === "arm64") return "darwin-arm64";
    return null;
  }
  if (platform === "linux") {
    const musl = detectHostIsMusl();
    if (arch === "x64") return musl ? "linux-x64-musl" : "linux-x64-gnu";
    if (arch === "arm64") return musl ? "linux-arm64-musl" : "linux-arm64-gnu";
    return null;
  }
  return null;
}

/** The directory holding this module (and the package root). */
export const PACKAGE_DIR = packageDir;

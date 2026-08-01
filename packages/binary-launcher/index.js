"use strict";

/**
 * Shared launcher substrate for Verter's per-platform binary packages.
 *
 * A Verter binary (`verter-lsp`, `verter-mcp`) ships as a launcher package plus
 * one optional dependency per platform. Every launcher needs the same three
 * things: a platform matrix derived from the rust targets its release builds,
 * a host probe that splits Linux by libc, and an ordered search for the binary
 * (installed platform package, then a local cargo build, then `PATH`).
 *
 * That logic lives here once. A launcher supplies only what differs — its
 * rust-target list, its package prefix, its binary stem, and its own module
 * resolution context — so the two launchers cannot drift apart in how they
 * choose a binary.
 */

const { execSync } = require("node:child_process");
const { existsSync, readFileSync } = require("node:fs");
const { dirname, join } = require("node:path");

// ---------------------------------------------------------------------------
// Platform matrix
// ---------------------------------------------------------------------------

/** Rust arch -> Node `process.arch`. */
const ARCH_BY_RUST = Object.freeze({
  x86_64: "x64",
  aarch64: "arm64",
});

/**
 * Rust os/abi tail -> { os, abiSuffix, libc }.
 *
 * `os` is a Node `process.platform` value. `abiSuffix` is the trailing
 * component of the npm suffix (absent for darwin, which has one ABI). `libc`
 * is the npm `libc` field tag, present only for the Linux split.
 */
const OS_BY_RUST_TAIL = Object.freeze({
  "unknown-linux-gnu": { os: "linux", abiSuffix: "gnu", libc: "glibc" },
  "unknown-linux-musl": { os: "linux", abiSuffix: "musl", libc: "musl" },
  "apple-darwin": { os: "darwin", abiSuffix: null, libc: null },
  "pc-windows-msvc": { os: "win32", abiSuffix: "msvc", libc: null },
});

/**
 * Decompose one rust target into a fully-reconciled platform row.
 *
 * Throws on any target this decomposition does not cover, so adding a target
 * without teaching the decomposition fails loudly instead of silently
 * producing a half-formed row.
 */
function decomposeRustTarget(rustTarget, { packagePrefix, binaryStem }) {
  const firstDash = rustTarget.indexOf("-");
  if (firstDash === -1) {
    throw new Error(`binary-launcher: malformed rust target "${rustTarget}"`);
  }
  const rustArch = rustTarget.slice(0, firstDash);
  const tail = rustTarget.slice(firstDash + 1);

  const cpu = ARCH_BY_RUST[rustArch];
  if (!cpu) {
    throw new Error(`binary-launcher: unknown rust arch "${rustArch}" (${rustTarget})`);
  }
  const osEntry = OS_BY_RUST_TAIL[tail];
  if (!osEntry) {
    throw new Error(`binary-launcher: unknown rust os/abi "${tail}" (${rustTarget})`);
  }

  const npmSuffix = osEntry.abiSuffix
    ? `${osEntry.os}-${cpu}-${osEntry.abiSuffix}`
    : `${osEntry.os}-${cpu}`;

  return Object.freeze({
    rustTarget,
    npmSuffix,
    packageName: `${packagePrefix}${npmSuffix}`,
    os: osEntry.os,
    cpu,
    libc: osEntry.libc,
    binaryName: osEntry.os === "win32" ? `${binaryStem}.exe` : binaryStem,
  });
}

/**
 * Build a platform matrix from a rust-target list.
 *
 * Every field is COMPUTED from the target's own components, never copied from
 * the platform packages or `optionalDependencies` — those are the things a
 * launcher's guards reconcile against this matrix, and deriving the expected
 * value from a thing under test would make the reconciliation vacuous.
 */
function buildPlatformMatrix(rustTargets, { packagePrefix, binaryStem }) {
  if (!packagePrefix || !binaryStem) {
    throw new Error("binary-launcher: buildPlatformMatrix needs a packagePrefix and binaryStem");
  }

  const rows = rustTargets.map((target) =>
    decomposeRustTarget(target, { packagePrefix, binaryStem }),
  );

  const seen = new Set();
  for (const row of rows) {
    if (seen.has(row.npmSuffix)) {
      throw new Error(`binary-launcher: duplicate npm suffix "${row.npmSuffix}"`);
    }
    seen.add(row.npmSuffix);
  }

  return Object.freeze(rows);
}

// ---------------------------------------------------------------------------
// musl detection — the same signals as the `@verter/native` napi loader. Node
// reports `process.platform === "linux"` for both glibc and musl, so the libc
// must be probed explicitly.
// ---------------------------------------------------------------------------

const isFileMusl = (f) => f.includes("libc.musl-") || f.includes("ld-musl-");

function isMuslFromFilesystem() {
  try {
    return readFileSync("/usr/bin/ldd", "utf-8").includes("musl");
  } catch {
    return null;
  }
}

function isMuslFromReport() {
  let report;
  try {
    process.report.excludeNetwork = true;
    report = process.report.getReport();
  } catch {
    report = null;
  }
  if (!report) return null;
  if (report.header && report.header.glibcVersionRuntime) return false;
  if (Array.isArray(report.sharedObjects) && report.sharedObjects.some(isFileMusl)) return true;
  return false;
}

function isMuslFromChildProcess() {
  try {
    return execSync("ldd --version", { encoding: "utf8" }).includes("musl");
  } catch {
    // Unknown libc — glibc is the safer assumption than refusing to resolve.
    return false;
  }
}

/** Whether the host's libc is musl. Always `false` off linux. */
function isMusl() {
  if (process.platform !== "linux") return false;
  let musl = isMuslFromFilesystem();
  if (musl === null) musl = isMuslFromReport();
  if (musl === null) musl = isMuslFromChildProcess();
  return musl;
}

// ---------------------------------------------------------------------------
// Package resolution
// ---------------------------------------------------------------------------

/**
 * A platform-package directory lookup bound to a CALLER's module resolution.
 *
 * The launcher package declares the platform packages as optional
 * dependencies; this shared package does not. Resolving from here would look
 * in the wrong place under a strict (non-hoisted) node_modules layout, so each
 * launcher passes its own `require`.
 */
function packageDirResolver(requireFn) {
  return (packageName) => {
    try {
      return dirname(requireFn.resolve(`${packageName}/package.json`));
    } catch {
      return null;
    }
  };
}

// ---------------------------------------------------------------------------
// Launcher
// ---------------------------------------------------------------------------

/**
 * Build the resolution surface for one binary family.
 *
 * @param toolName          user-facing name, used in error messages
 * @param matrix            the platform matrix from `buildPlatformMatrix`
 * @param workspaceRoot     repository root, for development-build discovery
 * @param resolvePackageDir platform-package lookup (see `packageDirResolver`)
 */
function createLauncher({ toolName, matrix, workspaceRoot, resolvePackageDir }) {
  if (!toolName || !matrix || !workspaceRoot || !resolvePackageDir) {
    throw new Error(
      "binary-launcher: createLauncher needs toolName, matrix, workspaceRoot and resolvePackageDir",
    );
  }

  const SUPPORTED_TARGETS = matrix.map((row) => row.npmSuffix).join(", ");

  /**
   * The npm platform suffix serving a (platform, arch, libc) host, or `null`
   * when no platform package covers it.
   */
  function resolveSuffix(platform, arch, musl) {
    const wantLibc = platform === "linux" ? (musl ? "musl" : "glibc") : null;
    const row = matrix.find(
      (entry) => entry.os === platform && entry.cpu === arch && entry.libc === wantLibc,
    );
    return row ? row.npmSuffix : null;
  }

  /** The platform package name for an npm platform suffix. */
  function platformPackageName(npmSuffix) {
    const row = matrix.find((entry) => entry.npmSuffix === npmSuffix);
    return row ? row.packageName : null;
  }

  function hostFrom(options) {
    const platform = options.platform ?? process.platform;
    const arch = options.arch ?? process.arch;
    const musl = options.musl ?? (platform === "linux" ? isMusl() : false);
    return { platform, arch, musl };
  }

  /**
   * The ordered candidate locations of the binary for a host, each tagged with
   * its provenance:
   *
   *   1. `platform-package` — the installed per-platform package's binary.
   *   2. `dev-build` — a `cargo build` result in this workspace.
   *   3. `path` — the bare binary name, resolved by the OS via `PATH`.
   *
   * Empty for a host no platform package covers.
   */
  function binaryCandidates(options = {}) {
    const { platform, arch, musl } = hostFrom(options);
    const suffix = resolveSuffix(platform, arch, musl);
    if (!suffix) return Object.freeze([]);

    const row = matrix.find((entry) => entry.npmSuffix === suffix);
    const lookup = options.platformPackageDir ?? resolvePackageDir;
    const candidates = [];

    const packageDir = lookup(row.packageName);
    if (packageDir) {
      candidates.push({ path: join(packageDir, row.binaryName), source: "platform-package" });
    }

    // A local cargo build wins over an installed package for contributors; in
    // a published install neither `target/` directory exists.
    candidates.push({
      path: join(workspaceRoot, "target", "debug", row.binaryName),
      source: "dev-build",
    });
    candidates.push({
      path: join(workspaceRoot, "target", "release", row.binaryName),
      source: "dev-build",
    });

    candidates.push({ path: row.binaryName, source: "path" });

    return Object.freeze(candidates);
  }

  /**
   * Resolve the binary for a host: the first candidate that exists on disk,
   * falling back to the bare name for `PATH` lookup.
   *
   * Throws when no platform package covers the host — a wrong-platform install
   * must fail loudly rather than spawn something that is not the tool.
   */
  function resolveBinary(options = {}) {
    const { platform, arch } = hostFrom(options);
    const candidates = binaryCandidates(options);

    if (candidates.length === 0) {
      throw new Error(
        `${toolName}: unsupported platform '${platform}/${arch}'.\n` +
          `Supported targets: ${SUPPORTED_TARGETS}`,
      );
    }

    for (const candidate of candidates) {
      if (candidate.source === "path") break;
      if (existsSync(candidate.path)) return candidate;
    }
    return candidates[candidates.length - 1];
  }

  /** The path (or bare `PATH` name) of the binary for a host. */
  function binaryPath(options = {}) {
    return resolveBinary(options).path;
  }

  return Object.freeze({
    toolName,
    PLATFORM_MATRIX: matrix,
    SUPPORTED_TARGETS,
    isMusl,
    resolveSuffix,
    platformPackageName,
    binaryCandidates,
    resolveBinary,
    binaryPath,
  });
}

module.exports = {
  buildPlatformMatrix,
  createLauncher,
  isMusl,
  packageDirResolver,
};

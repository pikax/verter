"use strict";

/**
 * `verter-lsp` server resolution.
 *
 * The published package is a launcher: the native server binary ships in one
 * per-platform optional dependency (`@verter/lsp-<suffix>`) and this module
 * resolves the one that matches the host. Editor clients should resolve the
 * PATH through here and spawn the native binary DIRECTLY — the CLI shim in
 * `bin/run.js` exists for `npx` and for editors that launch a bare
 * `verter-lsp` command, and deliberately keeps no proxy on the per-message
 * path.
 *
 * Platform decisions are read from the canonical matrix in `platforms.js`;
 * this module contains no second platform list.
 */

const { execSync } = require("node:child_process");
const { existsSync, readFileSync } = require("node:fs");
const { dirname, join } = require("node:path");

const { PLATFORM_MATRIX, SUPPORTED_TARGETS, PLATFORM_PACKAGE_PREFIX } = require("./platforms.js");

/** Repository/installation root used for development-build discovery. */
const WORKSPACE_ROOT = join(__dirname, "..", "..");

// ---------------------------------------------------------------------------
// musl detection — the same signals as the `verter-tsc` launcher and the
// `@verter/native` napi loader. Node reports `process.platform === "linux"`
// for both glibc and musl, so the libc must be probed explicitly.
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
// Platform resolution
// ---------------------------------------------------------------------------

/**
 * The npm platform suffix serving a (platform, arch, libc) host, or `null`
 * when no platform package covers it.
 */
function resolveSuffix(platform, arch, musl) {
  const wantLibc = platform === "linux" ? (musl ? "musl" : "glibc") : null;
  const row = PLATFORM_MATRIX.find(
    (entry) => entry.os === platform && entry.cpu === arch && entry.libc === wantLibc,
  );
  return row ? row.npmSuffix : null;
}

/** The platform package name for an npm platform suffix. */
function platformPackageName(npmSuffix) {
  return `${PLATFORM_PACKAGE_PREFIX}${npmSuffix}`;
}

/** Default lookup: the installed platform package's directory, or `null`. */
function installedPlatformPackageDir(packageName) {
  try {
    return dirname(require.resolve(`${packageName}/package.json`));
  } catch {
    return null;
  }
}

function matrixRowForSuffix(npmSuffix) {
  return PLATFORM_MATRIX.find((entry) => entry.npmSuffix === npmSuffix) ?? null;
}

function hostFrom(options) {
  const platform = options.platform ?? process.platform;
  const arch = options.arch ?? process.arch;
  const musl = options.musl ?? (platform === "linux" ? isMusl() : false);
  return { platform, arch, musl };
}

/**
 * The ordered candidate locations of the server binary for a host, each tagged
 * with its provenance:
 *
 *   1. `platform-package` — the installed `@verter/lsp-<suffix>` binary.
 *   2. `dev-build` — a `cargo build -p verter_lsp` result in this workspace.
 *   3. `path` — the bare binary name, resolved by the OS via `PATH`.
 *
 * Empty for a host no platform package covers.
 */
function serverBinaryCandidates(options = {}) {
  const { platform, arch, musl } = hostFrom(options);
  const suffix = resolveSuffix(platform, arch, musl);
  if (!suffix) return Object.freeze([]);

  const row = matrixRowForSuffix(suffix);
  const lookup = options.platformPackageDir ?? installedPlatformPackageDir;
  const candidates = [];

  const packageDir = lookup(platformPackageName(suffix));
  if (packageDir) {
    candidates.push({ path: join(packageDir, row.binaryName), source: "platform-package" });
  }

  // A local cargo build wins over an installed package for contributors; in a
  // published install neither `target/` directory exists.
  candidates.push({
    path: join(WORKSPACE_ROOT, "target", "debug", row.binaryName),
    source: "dev-build",
  });
  candidates.push({
    path: join(WORKSPACE_ROOT, "target", "release", row.binaryName),
    source: "dev-build",
  });

  candidates.push({ path: row.binaryName, source: "path" });

  return Object.freeze(candidates);
}

/**
 * Resolve the server binary for a host: the first candidate that exists on
 * disk, falling back to the bare name for `PATH` lookup.
 *
 * Throws when no platform package covers the host — a wrong-platform install
 * must fail loudly rather than spawn something that is not the server.
 */
function resolveServerBinary(options = {}) {
  const { platform, arch } = hostFrom(options);
  const candidates = serverBinaryCandidates(options);

  if (candidates.length === 0) {
    throw new Error(
      `verter-lsp: unsupported platform '${platform}/${arch}'.\n` +
        `Supported targets: ${SUPPORTED_TARGETS}`,
    );
  }

  for (const candidate of candidates) {
    if (candidate.source === "path") break;
    if (existsSync(candidate.path)) return candidate;
  }
  return candidates[candidates.length - 1];
}

/** The absolute path (or bare `PATH` name) of the server binary for a host. */
function serverBinaryPath(options = {}) {
  return resolveServerBinary(options).path;
}

module.exports = {
  PLATFORM_MATRIX,
  SUPPORTED_TARGETS,
  isMusl,
  platformPackageName,
  resolveServerBinary,
  resolveSuffix,
  serverBinaryCandidates,
  serverBinaryPath,
};

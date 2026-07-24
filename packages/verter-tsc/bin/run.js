#!/usr/bin/env node
// verter-tsc launcher — finds and spawns the platform-specific Rust binary.

"use strict";

const { spawnSync, execSync } = require("child_process");
const { readFileSync } = require("fs");
const path = require("path");
const fs = require("fs");

// ---------------------------------------------------------------------------
// musl detection — same signals as the @verter/native napi loader
// (packages/native/dist/index.js). Node reports process.platform === "linux"
// for both glibc and musl, so the libc must be probed explicitly.
// ---------------------------------------------------------------------------
const isFileMusl = (f) => f.includes("libc.musl-") || f.includes("ld-musl-");

const isMuslFromFilesystem = () => {
  try {
    return readFileSync("/usr/bin/ldd", "utf-8").includes("musl");
  } catch {
    return null;
  }
};

const isMuslFromReport = () => {
  let report;
  try {
    process.report.excludeNetwork = true;
    report = process.report.getReport();
  } catch {
    report = null;
  }
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
};

const isMuslFromChildProcess = () => {
  try {
    return execSync("ldd --version", { encoding: "utf8" }).includes("musl");
  } catch {
    // If we reach this case, we don't know if the system is musl or not, so is better to just fallback to false
    return false;
  }
};

const isMusl = () => {
  if (process.platform !== "linux") {
    return false;
  }
  let musl = isMuslFromFilesystem();
  if (musl === null) {
    musl = isMuslFromReport();
  }
  if (musl === null) {
    musl = isMuslFromChildProcess();
  }
  return musl;
};

// ---------------------------------------------------------------------------
// Platform resolution
// ---------------------------------------------------------------------------

// Map Node.js platform/arch (+ libc on Linux) to the npm package name suffix.
const platformMap = {
  darwin: { arm64: "darwin-arm64", x64: "darwin-x64" },
  linux: {
    x64: { glibc: "linux-x64-gnu", musl: "linux-x64-musl" },
    arm64: { glibc: "linux-arm64-gnu", musl: "linux-arm64-musl" },
  },
  win32: { x64: "win32-x64-msvc" },
};

const SUPPORTED_TARGETS =
  "darwin-arm64, darwin-x64, linux-x64-gnu, linux-x64-musl, linux-arm64-gnu, linux-arm64-musl, win32-x64-msvc";

/**
 * Resolve the npm package suffix for a platform/arch/libc combination.
 * Returns null when the combination is not supported.
 */
function resolveSuffix(platform, arch, musl) {
  const archMap = platformMap[platform];
  if (!archMap) return null;
  const entry = archMap[arch];
  if (!entry) return null;
  if (typeof entry === "string") return entry;
  return entry[musl ? "musl" : "glibc"] ?? null;
}

function fail(message) {
  process.stderr.write(`${message}\n`);
  process.exit(1);
}

/**
 * Find the verter-tsc binary:
 * 1. Platform-specific npm package (e.g. @verter/tsc-win32-x64-msvc)
 * 2. Local debug/release build (for development, from a source checkout)
 * 3. PATH fallback
 */
function findBinary() {
  const platform = process.platform;
  const arch = process.arch;

  const suffix = resolveSuffix(platform, arch, isMusl());
  if (!suffix) {
    fail(
      `verter-tsc: unsupported platform '${platform}/${arch}'.\n` +
        `Supported targets: ${SUPPORTED_TARGETS}`,
    );
  }

  const binName = platform === "win32" ? "verter-tsc.exe" : "verter-tsc";

  try {
    const pkg = `@verter/tsc-${suffix}`;
    // The binary lives at the root of the platform package.
    const pkgDir = path.dirname(require.resolve(`${pkg}/package.json`));
    const candidate = path.join(pkgDir, binName);
    if (fs.existsSync(candidate)) return candidate;
  } catch {
    // Platform package not installed — fall through to dev build.
  }

  // Development: use local debug build from Cargo workspace.
  const devBuild = path.join(__dirname, "..", "..", "..", "target", "debug", binName);
  if (fs.existsSync(devBuild)) return devBuild;

  // Release build fallback.
  const releaseBuild = path.join(__dirname, "..", "..", "..", "target", "release", binName);
  if (fs.existsSync(releaseBuild)) return releaseBuild;

  // Last resort: look on PATH.
  return binName;
}

/**
 * Ensure a resolved binary path is executable before spawning it.
 *
 * npm normalises shipped files to 0644 at pack/install time for any file not
 * declared in a package's `bin` field, so the platform package's binary
 * (`@verter/tsc-<target>/verter-tsc`, shipped via `files: ["verter-tsc"]`)
 * loses its exec bit after a real `npm install` and spawning it fails with
 * EACCES. Restore the bit here instead of fighting npm's mode normalisation.
 * Best-effort: a read-only install or an already-correct mode must not crash
 * the launcher — spawn will surface any real failure. No-op on Windows.
 */
function ensureExecutable(binary) {
  if (process.platform === "win32" || !path.isAbsolute(binary)) return;
  try {
    fs.chmodSync(binary, 0o755);
  } catch {
    // Read-only filesystem / permissions — let spawn report the real error.
  }
}

function main() {
  const binary = findBinary();
  ensureExecutable(binary);
  const result = spawnSync(binary, process.argv.slice(2), { stdio: "inherit" });

  if (result.error) {
    process.stderr.write(
      `verter-tsc: failed to start binary '${binary}': ${result.error.message}\n`,
    );
    process.exit(2);
  }

  process.exit(result.status ?? 1);
}

if (require.main === module) {
  main();
}

module.exports = { resolveSuffix };

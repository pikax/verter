"use strict";

/**
 * Canonical supported-platform matrix for `verter-lsp`.
 *
 * The AUTHORITATIVE source is `SUPPORTED_RUST_TARGETS` — the rust targets the
 * release actually builds `verter_lsp` for. Every other location that
 * enumerates platforms is reconciled against the matrix derived from it:
 *
 *   - `package.json#optionalDependencies`
 *   - the `npm/<suffix>/package.json` platform packages
 *   - the runtime resolver in `index.js`
 *   - the `build-lsp` job matrix in `.github/workflows/release.yml`
 *
 * The npm suffix, package name, binary name and the os/cpu/libc fields are
 * COMPUTED from each rust target's own components (arch + os + abi) by an
 * explicit, total decomposition — never copied from the platform packages or
 * from `optionalDependencies`. Those are the things under test, and deriving
 * the expected value from a thing under test would make the reconciliation
 * vacuous.
 *
 * This module ships in the published package: `index.js` resolves the host's
 * platform package through it at runtime.
 */

/** Rust targets the release builds `verter_lsp` for. */
const SUPPORTED_RUST_TARGETS = Object.freeze([
  "x86_64-unknown-linux-gnu",
  "x86_64-unknown-linux-musl",
  "aarch64-unknown-linux-gnu",
  "aarch64-unknown-linux-musl",
  "x86_64-apple-darwin",
  "aarch64-apple-darwin",
  "x86_64-pc-windows-msvc",
]);

/** Rust arch -> Node `process.arch`. */
const ARCH_BY_RUST = Object.freeze({
  x86_64: "x64",
  aarch64: "arm64",
});

/**
 * Rust os/abi tail -> { os, abiSuffix, libc }.
 *
 * `os` is a Node `process.platform` value. `abiSuffix` is the trailing
 * component of the npm suffix (absent for darwin, which has one ABI).
 * `libc` is the npm `libc` field tag, present only for the Linux split.
 */
const OS_BY_RUST_TAIL = Object.freeze({
  "unknown-linux-gnu": { os: "linux", abiSuffix: "gnu", libc: "glibc" },
  "unknown-linux-musl": { os: "linux", abiSuffix: "musl", libc: "musl" },
  "apple-darwin": { os: "darwin", abiSuffix: null, libc: null },
  "pc-windows-msvc": { os: "win32", abiSuffix: "msvc", libc: null },
});

/** The on-disk stem of the server binary (no extension). */
const BINARY_STEM = "verter-lsp";

/** The npm scope the per-platform binary packages live in. */
const PLATFORM_PACKAGE_PREFIX = "@verter/lsp-";

/**
 * Decompose one rust target into a fully-reconciled platform row.
 *
 * Throws on any target this decomposition does not cover, so adding a target
 * to `SUPPORTED_RUST_TARGETS` without teaching the decomposition fails loudly
 * instead of silently producing a half-formed row.
 */
function decomposeRustTarget(rustTarget) {
  const firstDash = rustTarget.indexOf("-");
  if (firstDash === -1) {
    throw new Error(`verter-lsp platforms: malformed rust target "${rustTarget}"`);
  }
  const rustArch = rustTarget.slice(0, firstDash);
  const tail = rustTarget.slice(firstDash + 1);

  const cpu = ARCH_BY_RUST[rustArch];
  if (!cpu) {
    throw new Error(`verter-lsp platforms: unknown rust arch "${rustArch}" (${rustTarget})`);
  }
  const osEntry = OS_BY_RUST_TAIL[tail];
  if (!osEntry) {
    throw new Error(`verter-lsp platforms: unknown rust os/abi "${tail}" (${rustTarget})`);
  }

  const npmSuffix = osEntry.abiSuffix
    ? `${osEntry.os}-${cpu}-${osEntry.abiSuffix}`
    : `${osEntry.os}-${cpu}`;

  return Object.freeze({
    rustTarget,
    npmSuffix,
    packageName: `${PLATFORM_PACKAGE_PREFIX}${npmSuffix}`,
    os: osEntry.os,
    cpu,
    libc: osEntry.libc,
    binaryName: osEntry.os === "win32" ? `${BINARY_STEM}.exe` : BINARY_STEM,
  });
}

/** Build a platform matrix from an arbitrary rust-target list. */
function buildPlatformMatrix(rustTargets) {
  const rows = rustTargets.map(decomposeRustTarget);

  const seen = new Set();
  for (const row of rows) {
    if (seen.has(row.npmSuffix)) {
      throw new Error(`verter-lsp platforms: duplicate npm suffix "${row.npmSuffix}"`);
    }
    seen.add(row.npmSuffix);
  }

  return Object.freeze(rows);
}

/** The canonical matrix, derived from the authoritative rust-target list. */
const PLATFORM_MATRIX = buildPlatformMatrix(SUPPORTED_RUST_TARGETS);

/** Human-readable supported-target list, for error messages. */
const SUPPORTED_TARGETS = PLATFORM_MATRIX.map((row) => row.npmSuffix).join(", ");

module.exports = {
  BINARY_STEM,
  PLATFORM_MATRIX,
  PLATFORM_PACKAGE_PREFIX,
  SUPPORTED_RUST_TARGETS,
  SUPPORTED_TARGETS,
  buildPlatformMatrix,
};

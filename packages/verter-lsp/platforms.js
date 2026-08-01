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
 * The decomposition of a rust target into an npm suffix, package name, binary
 * name and os/cpu/libc is shared with every other Verter binary family
 * (`@verter/binary-launcher`), so the families cannot drift apart in how they
 * name or select a platform.
 *
 * This module ships in the published package: `index.js` resolves the host's
 * platform package through it at runtime.
 */

const { buildPlatformMatrix: buildMatrix } = require("@verter/binary-launcher");

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

/** The on-disk stem of the server binary (no extension). */
const BINARY_STEM = "verter-lsp";

/** The npm scope the per-platform binary packages live in. */
const PLATFORM_PACKAGE_PREFIX = "@verter/lsp-";

/** Build a matrix for this family from an arbitrary rust-target list. */
function buildPlatformMatrix(rustTargets) {
  return buildMatrix(rustTargets, {
    packagePrefix: PLATFORM_PACKAGE_PREFIX,
    binaryStem: BINARY_STEM,
  });
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

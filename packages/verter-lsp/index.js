"use strict";

/**
 * `verter-lsp` server resolution.
 *
 * The published package is a launcher: the native server binary ships in one
 * per-platform optional dependency (`@verter/lsp-<suffix>`) and this module
 * resolves the one that matches the host. Editor clients should resolve the
 * path through here and spawn the native binary DIRECTLY — the CLI shim in
 * `bin/run.js` exists for `npx` and for editors that launch a bare
 * `verter-lsp` command, and deliberately keeps no proxy on the per-message
 * path.
 *
 * Resolution itself lives in `@verter/binary-launcher`, shared with every
 * other Verter binary family; this module supplies only what is specific to
 * the LSP server.
 */

const { join } = require("node:path");

const { createLauncher, isMusl, packageDirResolver } = require("@verter/binary-launcher");

const { PLATFORM_MATRIX, SUPPORTED_TARGETS } = require("./platforms.js");

const launcher = createLauncher({
  toolName: "verter-lsp",
  matrix: PLATFORM_MATRIX,
  // Repository root, for development-build discovery.
  workspaceRoot: join(__dirname, "..", ".."),
  // Bound to THIS package's resolution: the platform packages are its optional
  // dependencies, not the shared launcher's.
  resolvePackageDir: packageDirResolver(require),
});

module.exports = {
  PLATFORM_MATRIX,
  SUPPORTED_TARGETS,
  isMusl,
  launcher,
  platformPackageName: launcher.platformPackageName,
  resolveServerBinary: launcher.resolveBinary,
  resolveSuffix: launcher.resolveSuffix,
  serverBinaryCandidates: launcher.binaryCandidates,
  serverBinaryPath: launcher.binaryPath,
};

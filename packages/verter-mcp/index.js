"use strict";

/**
 * `verter-mcp` server resolution.
 *
 * The published package is a launcher: the native MCP server binary ships in
 * one per-platform optional dependency (`@verter/mcp-<suffix>`) and this module
 * resolves the one that matches the host. An MCP client normally launches the
 * CLI shim over stdio (`npx -y verter-mcp`); the shim hands the client's stdio
 * straight to the native binary and keeps no proxy on the message path. A host
 * that spawns the binary itself should resolve the path through here.
 *
 * Resolution itself lives in `@verter/binary-launcher`, shared with every
 * other Verter binary family; this module supplies only what is specific to
 * the MCP server.
 */

const { join } = require("node:path");

const { createLauncher, isMusl, packageDirResolver } = require("@verter/binary-launcher");

const { PLATFORM_MATRIX, SUPPORTED_TARGETS } = require("./platforms.js");

const launcher = createLauncher({
  toolName: "verter-mcp",
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

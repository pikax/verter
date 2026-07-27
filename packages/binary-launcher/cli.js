"use strict";

/**
 * The shared CLI shim behind every Verter launcher's `bin` entry.
 *
 * The child INHERITS the real stdin/stdout descriptors, so a stdio protocol
 * (LSP, MCP) runs directly between the client and the native binary: this
 * wrapper is not on the per-message path. Programmatic consumers should skip
 * it entirely and spawn the resolved path themselves.
 */

const { spawnSync } = require("node:child_process");
const { chmodSync } = require("node:fs");
const { isAbsolute } = require("node:path");

/** The flag every launcher answers with its resolved native binary path. */
const PRINT_PATH_FLAG = "--print-server-path";

/**
 * Restore the exec bit on a resolved binary.
 *
 * npm normalises shipped files to 0644 at pack/install time for any file not
 * declared in a package's `bin` field, so a platform package's binary (shipped
 * via `files`) loses its exec bit after a real install and spawning it fails
 * with EACCES. Best-effort: a read-only install or an already-correct mode must
 * not crash the launcher — spawn surfaces any real failure. No-op on Windows.
 */
function ensureExecutable(binary) {
  if (process.platform === "win32" || !isAbsolute(binary)) return;
  try {
    chmodSync(binary, 0o755);
  } catch {
    // Read-only filesystem / permissions — let spawn report the real error.
  }
}

/**
 * Run a launcher's CLI: resolve the native binary and hand it the process
 * stdio. Returns the exit code; it never returns on the spawn path because the
 * caller exits with it.
 */
function runLauncherCli({ launcher, argv, stderr = process.stderr, stdout = process.stdout }) {
  let resolved;
  try {
    resolved = launcher.resolveBinary();
  } catch (error) {
    stderr.write(`${error.message}\n`);
    return 1;
  }

  // Editors and agent hosts that cannot resolve a Node module ask for the path
  // and launch the native binary themselves.
  if (argv[0] === PRINT_PATH_FLAG) {
    stdout.write(`${resolved.path}\n`);
    return 0;
  }

  ensureExecutable(resolved.path);
  const result = spawnSync(resolved.path, argv, { stdio: "inherit" });

  if (result.error) {
    stderr.write(
      `${launcher.toolName}: failed to start '${resolved.path}' (${resolved.source}): ${result.error.message}\n`,
    );
    return 2;
  }

  return result.status ?? 1;
}

module.exports = { PRINT_PATH_FLAG, ensureExecutable, runLauncherCli };

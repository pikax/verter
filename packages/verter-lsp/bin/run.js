#!/usr/bin/env node
// verter-lsp launcher — resolves the platform-specific Rust server binary and
// hands it the process stdio.
//
// The child INHERITS the real stdin/stdout descriptors, so the LSP stream runs
// directly between the editor and the native server: this wrapper is not on the
// per-message path. Programmatic consumers should skip it entirely and spawn
// `require("verter-lsp").serverBinaryPath()` themselves.

"use strict";

const { spawnSync } = require("node:child_process");
const { chmodSync } = require("node:fs");
const path = require("node:path");

const { resolveServerBinary } = require("../index.js");

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
  if (process.platform === "win32" || !path.isAbsolute(binary)) return;
  try {
    chmodSync(binary, 0o755);
  } catch {
    // Read-only filesystem / permissions — let spawn report the real error.
  }
}

function main(argv) {
  let resolved;
  try {
    resolved = resolveServerBinary();
  } catch (error) {
    process.stderr.write(`${error.message}\n`);
    process.exit(1);
    return;
  }

  // Editors that cannot resolve a Node module (Helix, Neovim, any bare-command
  // client) can ask for the path and launch the native binary themselves.
  if (argv[0] === "--print-server-path") {
    process.stdout.write(`${resolved.path}\n`);
    process.exit(0);
    return;
  }

  ensureExecutable(resolved.path);
  const result = spawnSync(resolved.path, argv, { stdio: "inherit" });

  if (result.error) {
    process.stderr.write(
      `verter-lsp: failed to start server '${resolved.path}' (${resolved.source}): ${result.error.message}\n`,
    );
    process.exit(2);
    return;
  }

  process.exit(result.status ?? 1);
}

if (require.main === module) {
  main(process.argv.slice(2));
}

module.exports = { main };

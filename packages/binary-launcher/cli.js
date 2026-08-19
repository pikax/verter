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
const { chmodSync, closeSync, existsSync, openSync, readSync, realpathSync } = require("node:fs");
const { delimiter, dirname, isAbsolute, join, sep } = require("node:path");

/** The flag every launcher answers with its resolved native binary path. */
const PRINT_PATH_FLAG = "--print-server-path";

/**
 * Env var recording which launcher tool names are already running in this
 * process tree, so a launcher that ends up spawning itself again — however
 * that happens — fails closed instead of recursing without bound.
 */
const ACTIVE_ENV_VAR = "VERTER_LAUNCHER_ACTIVE";

function activeToolNames() {
  return (process.env[ACTIVE_ENV_VAR] ?? "").split(",").filter(Boolean);
}

/** `process.env` for the child, with this tool name added to the active list. */
function envWithToolMarked(toolName) {
  const names = activeToolNames();
  if (!names.includes(toolName)) names.push(toolName);
  return { ...process.env, [ACTIVE_ENV_VAR]: names.join(",") };
}

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

/** First bytes of a file as text; `""` on any read failure. */
function readHead(path, length = 32) {
  let fd;
  try {
    fd = openSync(path, "r");
    const buf = Buffer.alloc(length);
    const bytesRead = readSync(fd, buf, 0, length, 0);
    return buf.toString("utf8", 0, bytesRead);
  } catch {
    return "";
  } finally {
    if (fd !== undefined) {
      try {
        closeSync(fd);
      } catch {
        // already closed
      }
    }
  }
}

const NODE_SHEBANGS = ["#!/usr/bin/env node", "#!/usr/bin/node"];

/** Whether a file is a node script rather than a native executable. */
function isNodeScript(path) {
  const head = readHead(path);
  return NODE_SHEBANGS.some((shebang) => head.startsWith(shebang));
}

/**
 * Manually walk `PATH` for a bare binary name — the same lookup the OS does
 * for `spawnSync(name, ...)` — so a self-spawn can be caught BEFORE handing
 * the name to the OS. Returns the first hit's real (symlink-resolved) path,
 * or `null`.
 */
function resolveOnPath(name) {
  for (const dir of (process.env.PATH ?? "").split(delimiter)) {
    if (!dir) continue;
    const candidate = join(dir, name);
    if (!existsSync(candidate)) continue;
    try {
      return realpathSync(candidate);
    } catch {
      // Broken symlink or a race with something deleting it — keep looking.
    }
  }
  return null;
}

/** Candidate list, formatted for a diagnostic. */
function formatCandidates(launcher) {
  return launcher
    .binaryCandidates()
    .map((candidate) => `  - [${candidate.source}] ${candidate.path}`)
    .join("\n");
}

/** `verter-lsp` -> `pnpm run build:lsp`, `verter-mcp` -> `pnpm run build:mcp`. */
function buildCommandFor(toolName) {
  const suffix = toolName.startsWith("verter-") ? toolName.slice("verter-".length) : toolName;
  return `pnpm run build:${suffix}`;
}

function nativeBinaryMissingDiagnostic(launcher) {
  return (
    `The native '${launcher.toolName}' binary was not found. Tried:\n` +
    `${formatCandidates(launcher)}\n` +
    `Build it with: ${buildCommandFor(launcher.toolName)}`
  );
}

/**
 * Refuse to spawn the launcher's own CLI shim, anything inside its own
 * package, or a `PATH` hit that is a node script rather than a native binary.
 *
 * `resolveBinary`'s `PATH` fallback is a legitimate resolution mode (an
 * installed binary can genuinely live on `PATH`) — but under pnpm/npm script
 * execution `PATH` also includes `node_modules/.bin`, where the launcher's
 * own bare tool name resolves back to ITS OWN shim script. Spawning that
 * recurses without bound; this is the check that makes the fallback safe.
 *
 * `selfPath` (the calling `bin/run.js`, when known) tightens the check to
 * catch a resolution that landed on the launcher's own script even outside
 * `PATH`; it is optional because the node-script check alone already covers
 * the fork-bomb mechanism.
 */
function assertNotSelfSpawn({ resolved, launcher, selfPath }) {
  const actualPath = resolved.source === "path" ? resolveOnPath(resolved.path) : resolved.path;
  if (!actualPath) return; // Nothing on disk to protect against; spawn will report ENOENT.

  let real;
  try {
    real = realpathSync(actualPath);
  } catch {
    return;
  }

  let selfReal = null;
  try {
    selfReal = selfPath ? realpathSync(selfPath) : null;
  } catch {
    selfReal = null;
  }
  const ownPackageDir = selfReal ? dirname(dirname(selfReal)) : null;

  const isSelf = selfReal !== null && real === selfReal;
  const isInsideOwnPackage = ownPackageDir !== null && real.startsWith(ownPackageDir + sep);
  const isPathNodeShim = resolved.source === "path" && isNodeScript(real);

  if (isSelf || isInsideOwnPackage || isPathNodeShim) {
    throw new Error(
      `${launcher.toolName}: refusing to spawn '${real}' — it is this launcher's own script, ` +
        `not the native binary.\n${nativeBinaryMissingDiagnostic(launcher)}`,
    );
  }
}

function reentrancyDiagnostic(launcher) {
  return (
    `${launcher.toolName}: refusing to start — this launcher is already active in this ` +
    `process tree (${ACTIVE_ENV_VAR} already lists it). Starting again would recurse.\n` +
    `${nativeBinaryMissingDiagnostic(launcher)}`
  );
}

/**
 * Run a launcher's CLI: resolve the native binary and hand it the process
 * stdio. Returns the exit code; it never returns on the spawn path because the
 * caller exits with it.
 *
 * `selfPath` should be the calling `bin/run.js`'s own path (`__filename`) —
 * see `assertNotSelfSpawn`.
 */
function runLauncherCli({
  launcher,
  argv,
  selfPath,
  stderr = process.stderr,
  stdout = process.stdout,
}) {
  if (activeToolNames().includes(launcher.toolName)) {
    stderr.write(`${reentrancyDiagnostic(launcher)}\n`);
    return 3;
  }

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

  try {
    assertNotSelfSpawn({ resolved, launcher, selfPath });
  } catch (error) {
    stderr.write(`${error.message}\n`);
    return 1;
  }

  ensureExecutable(resolved.path);
  const result = spawnSync(resolved.path, argv, {
    stdio: "inherit",
    env: envWithToolMarked(launcher.toolName),
  });

  if (result.error) {
    stderr.write(
      `${launcher.toolName}: failed to start '${resolved.path}' (${resolved.source}): ${result.error.message}\n`,
    );
    return 2;
  }

  return result.status ?? 1;
}

module.exports = { PRINT_PATH_FLAG, ensureExecutable, runLauncherCli };

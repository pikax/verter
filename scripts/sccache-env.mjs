#!/usr/bin/env node
// Portable, opt-in sccache environment helper.
//
// Computes the sccache compiler-cache environment (RUSTC_WRAPPER, CARGO_INCREMENTAL,
// SCCACHE_DIR, SCCACHE_CACHE_SIZE, SCCACHE_BASEDIRS) and either prints it
// (`--print-env`) or runs a child command with it merged in (`--exec -- <cmd> ...`).
//
// Hard rules:
// - This script is NEVER itself a rustc wrapper: it never execs rustc, never sets
//   RUSTC_WRAPPER to its own path, and never impersonates sccache. Its only job is
//   compute-env + print/exec.
// - Opt-in only: nothing here changes the default build. A machine without sccache
//   is unaffected — optional mode is a LOUD no-op (warn + run the child with the
//   caller's unmodified environment), `--required` mode is a hard failure.
// - Portable: node stdlib only, no hardcoded per-OS paths, `.exe` handled on
//   win32, paths joined with `path.delimiter` / built with `path.join`.
//
// Environment:
//   VERTER_SCCACHE_BIN   Absolute-path override for the sccache executable
//                        (authoritative: when set, PATH is not scanned).
//   SCCACHE_DIR          Respected if set; default: ~/.cache/verter-sccache
//   SCCACHE_CACHE_SIZE   Respected if set; default: 10G
//   SCCACHE_BASEDIRS     Respected if set; default: every `git worktree list`
//                        root, joined with the platform path delimiter.

import os from "node:os";
import path from "node:path";
import fs from "node:fs";
import process from "node:process";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const USAGE = `Usage: node scripts/sccache-env.mjs <mode> [--required]

Modes (exactly one):
  --exec -- <cmd> [args..]  RECOMMENDED: run <cmd> with the computed sccache
                            env merged in. Space-safe: argv goes straight to
                            the child (spawnSync, no shell).
  --print-env               Print the computed sccache env as KEY=VALUE lines.
                            Line-parseable output for tooling/inspection only —
                            values are unquoted, so it is NOT safe for raw
                            shell eval; use --exec to run commands.
  --help                    Show this help.

Flags:
  --required                Missing sccache is a hard failure (exit 1) instead
                            of a loud no-op.

Environment:
  VERTER_SCCACHE_BIN        Absolute-path override for the sccache executable.
  SCCACHE_DIR               Respected if set; default: ~/.cache/verter-sccache
  SCCACHE_CACHE_SIZE        Respected if set; default: 10G
  SCCACHE_BASEDIRS          Respected if set; default: all git worktree roots.
`;

function isFileAt(p) {
  try {
    return fs.statSync(p).isFile();
  } catch {
    return false;
  }
}

/**
 * Pure win32 executable-name decision: is `filename`'s extension executable?
 * The DEFAULT executable set (`.exe`/`.com`/`.bat`/`.cmd`) is ALWAYS accepted
 * — an absolute `sccache.exe` is executable regardless of PATHEXT, which may
 * be empty or omit `.EXE` — UNIONED with the entries parsed from `pathextEnv`
 * (split on `;`, trimmed, lowercased, `.`-prefixed), so a custom PATHEXT
 * extends the set but can never shrink it. Extension matching is
 * case-insensitive. Exported so the win32 branch is deterministically
 * unit-testable on any OS.
 */
export function isWindowsExecutableName(filename, pathextEnv) {
  const ext = path.extname(filename).toLowerCase();
  if (ext === "") return false;
  const allowed = new Set([".exe", ".com", ".bat", ".cmd"]);
  for (const raw of (pathextEnv ?? "").split(";")) {
    const entry = raw.trim().toLowerCase();
    if (entry === "") continue;
    allowed.add(entry.startsWith(".") ? entry : `.${entry}`);
  }
  return allowed.has(ext);
}

/**
 * True when `p` is a regular file AND executable. A non-executable candidate
 * (e.g. a plain data file named `sccache`) must never be promoted to
 * RUSTC_WRAPPER — it is treated as sccache-absent, so optional mode no-ops
 * and `--required` fails cleanly instead of hard-breaking the build.
 */
function isExecutableFileAt(p, env) {
  if (!isFileAt(p)) return false;
  if (process.platform === "win32") {
    // X_OK is not meaningful on Windows; executability is extension-based:
    // the standard executable extensions are always accepted, and PATHEXT
    // only ever EXTENDS the set (see isWindowsExecutableName).
    return isWindowsExecutableName(p, env.PATHEXT);
  }
  try {
    fs.accessSync(p, fs.constants.X_OK);
    return true;
  } catch {
    return false;
  }
}

/** Absolute path to the repo root that contains this script (scripts/..). */
function scriptRepoRoot() {
  return path.resolve(path.dirname(path.resolve(process.argv[1])), "..");
}

/**
 * Locate the sccache executable.
 * Returns an absolute path, or null when sccache is not available.
 */
function findSccache(env) {
  const override = env.VERTER_SCCACHE_BIN;
  if (override !== undefined && override !== "") {
    // Authoritative override: used when valid, otherwise sccache counts as
    // absent (no PATH fallback) so presence/absence is fully deterministic.
    const candidate = path.resolve(override);
    return isExecutableFileAt(candidate, env) ? candidate : null;
  }
  const names = process.platform === "win32" ? ["sccache.exe", "sccache"] : ["sccache"];
  for (const dir of (env.PATH ?? "").split(path.delimiter)) {
    if (!dir) continue;
    for (const name of names) {
      const candidate = path.resolve(dir, name);
      if (isExecutableFileAt(candidate, env)) return candidate;
    }
  }
  return null;
}

/**
 * Derive SCCACHE_BASEDIRS: every worktree root of the repo we are invoked in,
 * joined with the platform path delimiter, so builds under different worktree
 * roots relativize to identical cache keys (cross-worktree cache hits).
 * Falls back to this script's repo root when the git call fails.
 */
function deriveBasedirs(env) {
  if (env.SCCACHE_BASEDIRS !== undefined && env.SCCACHE_BASEDIRS !== "") {
    return env.SCCACHE_BASEDIRS;
  }
  const roots = [];
  const res = spawnSync("git", ["worktree", "list", "--porcelain"], {
    cwd: process.cwd(),
    encoding: "utf8",
  });
  if (!res.error && res.status === 0 && typeof res.stdout === "string") {
    for (const line of res.stdout.split(/\r?\n/)) {
      if (line.startsWith("worktree ")) {
        roots.push(path.resolve(line.slice("worktree ".length)));
      }
    }
  }
  if (roots.length === 0) roots.push(scriptRepoRoot());
  return roots.join(path.delimiter);
}

/**
 * Fail-closed SCCACHE_BASEDIRS validation: sccache rejects relative basedirs
 * (the server refuses to start), so every entry — derived OR caller-overridden
 * via the environment — must be a non-empty absolute path. An invalid entry is
 * a hard error, never silently emitted.
 */
function validateBasedirs(basedirs) {
  for (const entry of basedirs.split(path.delimiter)) {
    if (entry === "" || !path.isAbsolute(entry)) {
      process.stderr.write(
        `sccache-env: error: invalid SCCACHE_BASEDIRS entry ${JSON.stringify(entry)}: every entry must be a non-empty absolute path (sccache rejects relative basedirs).\n`,
      );
      process.exit(1);
    }
  }
  return basedirs;
}

/** Compute the sccache env vars. Only called when sccache was found. */
function computeEnv(sccachePath, env) {
  const computed = {
    RUSTC_WRAPPER: sccachePath,
    // sccache cannot cache incremental compilation artifacts.
    CARGO_INCREMENTAL: "0",
  };
  if (env.SCCACHE_DIR !== undefined && env.SCCACHE_DIR !== "") {
    computed.SCCACHE_DIR = path.resolve(env.SCCACHE_DIR);
  } else {
    // Portable shared default OUTSIDE any worktree.
    const dir = path.join(os.homedir(), ".cache", "verter-sccache");
    fs.mkdirSync(dir, { recursive: true });
    computed.SCCACHE_DIR = dir;
  }
  computed.SCCACHE_CACHE_SIZE =
    env.SCCACHE_CACHE_SIZE !== undefined && env.SCCACHE_CACHE_SIZE !== ""
      ? env.SCCACHE_CACHE_SIZE
      : "10G";
  computed.SCCACHE_BASEDIRS = validateBasedirs(deriveBasedirs(env));
  return computed;
}

function usageError(message) {
  process.stderr.write(`sccache-env: ${message}\n\n${USAGE}`);
  process.exit(2);
}

function parseArgs(argv) {
  const flags = { printEnv: false, exec: false, required: false, help: false };
  let childArgv = null;
  for (let i = 0; i < argv.length; i += 1) {
    const token = argv[i];
    if (token === "--") {
      childArgv = argv.slice(i + 1);
      break;
    }
    switch (token) {
      case "--print-env":
        if (flags.printEnv) usageError("duplicate --print-env");
        flags.printEnv = true;
        break;
      case "--exec":
        if (flags.exec) usageError("duplicate --exec");
        flags.exec = true;
        break;
      case "--required":
        if (flags.required) usageError("duplicate --required");
        flags.required = true;
        break;
      case "--help":
        flags.help = true;
        break;
      default:
        usageError(`unknown argument: ${token}`);
    }
  }
  if (flags.help) {
    if (flags.printEnv || flags.exec || flags.required || childArgv !== null) {
      usageError("--help cannot be combined with other arguments");
    }
    return { mode: "help", required: false, childArgv: [] };
  }
  if (flags.printEnv && flags.exec) {
    usageError("--print-env and --exec are mutually exclusive");
  }
  if (!flags.printEnv && !flags.exec) {
    usageError("exactly one mode is required: --print-env, --exec, or --help");
  }
  if (flags.printEnv) {
    if (childArgv !== null) usageError("--print-env does not take a -- command");
    return { mode: "print-env", required: flags.required, childArgv: [] };
  }
  if (childArgv === null || childArgv.length === 0) {
    usageError("--exec requires `-- <cmd> [args...]`");
  }
  return { mode: "exec", required: flags.required, childArgv };
}

function main() {
  const { mode, required, childArgv } = parseArgs(process.argv.slice(2));

  if (mode === "help") {
    process.stdout.write(USAGE);
    process.exit(0);
  }

  const sccache = findSccache(process.env);

  if (mode === "print-env") {
    if (sccache === null) {
      if (required) {
        process.stderr.write(
          "sccache-env: error: sccache not found (checked VERTER_SCCACHE_BIN and PATH); --required set, refusing to continue.\n",
        );
        process.exit(1);
      }
      // LOUD absence on stderr: an `eval "$(...)"` caller never sees stdout
      // comments, so the warning must not live only there. STDOUT stays free
      // of env assignments.
      process.stderr.write(
        "sccache-env: WARNING: sccache not found; no sccache environment computed (a build would use plain rustc, no compiler cache).\n",
      );
      process.stdout.write(
        "# sccache not found; no sccache environment computed (build would use plain rustc)\n",
      );
      process.exit(0);
    }
    const computed = computeEnv(sccache, process.env);
    for (const key of [
      "RUSTC_WRAPPER",
      "CARGO_INCREMENTAL",
      "SCCACHE_DIR",
      "SCCACHE_CACHE_SIZE",
      "SCCACHE_BASEDIRS",
    ]) {
      process.stdout.write(`${key}=${computed[key]}\n`);
    }
    process.exit(0);
  }

  // mode === "exec"
  const [cmd, ...args] = childArgv;
  if (sccache === null) {
    if (required) {
      process.stderr.write(
        "sccache-env: error: sccache not found (checked VERTER_SCCACHE_BIN and PATH); --required set, NOT running the command.\n",
      );
      process.exit(1);
    }
    // LOUD no-op: run the child with the caller's UNMODIFIED environment
    // (plain rustc) — never a silent fake-sccache shim.
    process.stderr.write(
      "sccache-env: WARNING: sccache not found; running WITHOUT compiler cache (plain rustc, unmodified environment).\n",
    );
    const child = spawnSync(cmd, args, { stdio: "inherit", env: process.env });
    if (child.error) {
      process.stderr.write(`sccache-env: failed to run ${cmd}: ${child.error.message}\n`);
      process.exit(1);
    }
    process.exit(child.status ?? 1);
  }

  const computed = computeEnv(sccache, process.env);
  const child = spawnSync(cmd, args, {
    stdio: "inherit",
    env: { ...process.env, ...computed },
  });
  if (child.error) {
    process.stderr.write(`sccache-env: failed to run ${cmd}: ${child.error.message}\n`);
    process.exit(1);
  }
  process.exit(child.status ?? 1);
}

// Run only when invoked directly (`node scripts/sccache-env.mjs ...`) — an
// import (e.g. the vitest self-tests importing isWindowsExecutableName) must
// never trigger arg parsing or process.exit.
if (
  process.argv[1] !== undefined &&
  path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  main();
}

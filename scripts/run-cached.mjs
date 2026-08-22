#!/usr/bin/env node
// Explicit sccache-wrapped build lane, backing build:cached / dist:cached /
// gate:cached. NOT the unconditional default: sccache disables
// CARGO_INCREMENTAL (see scripts/sccache-env.mjs's computeEnv), which helps a
// clean/cold build (nothing local to reuse anyway) but HURTS a warm
// edit-compile-test cycle, where incremental compilation would otherwise
// reuse local object files sccache cannot. Opt in explicitly per invocation.
//
// The wrapped command is joined into ONE shell command line and run with
// `shell: true` (Node dispatches to the right shell per platform — `/bin/sh`
// on POSIX, `cmd.exe` on Windows), so a `pnpm run a && pnpm run b` chain
// works uniformly; on Windows a bare `spawnSync('pnpm', ..., {shell:false})`
// cannot launch pnpm's `.cmd` shim at all, and a raw `&&` needs a real shell
// to interpret regardless of platform. The command line is a static,
// developer-authored string from package.json (not attacker-controlled
// input), so shell interpretation here is intentional, not a hazard.
//
// Zeroes sccache's stats before the wrapped command and prints them after,
// so a run's cache effectiveness is visible inline instead of requiring a
// separate `sccache --show-stats` call. Warns (does not fail — the wrapped
// command's own exit status is authoritative) when the run recorded zero
// compile requests, since that means sccache never actually intercepted a
// rustc invocation (a config problem, or a fully no-op build), not that
// caching "worked".
//
// Usage: node scripts/run-cached.mjs -- <shell command line>

import { spawnSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(HERE, "..");
const SCCACHE_ENV = path.join(HERE, "sccache-env.mjs");

const dashIndex = process.argv.indexOf("--");
if (dashIndex === -1 || dashIndex === process.argv.length - 1) {
  process.stderr.write("Usage: node scripts/run-cached.mjs -- <shell command line>\n");
  process.exit(2);
}
const commandLine = process.argv.slice(dashIndex + 1).join(" ");

/**
 * Compute the sccache environment via the EXISTING scripts/sccache-env.mjs
 * (one source of truth for sccache resolution/config — not a second
 * lookup that could disagree with what a plain `--exec` invocation would
 * apply). `--required` so a missing sccache fails loud here rather than
 * silently running the wrapped command uncached and reporting misleading
 * "0 compile requests".
 */
function resolveSccacheEnv() {
  const result = spawnSync(process.execPath, [SCCACHE_ENV, "--print-env", "--required"], {
    encoding: "utf8",
    cwd: REPO_ROOT,
  });
  if (result.status !== 0) {
    process.stderr.write(result.stderr ?? "");
    process.exit(result.status ?? 1);
  }
  const env = {};
  for (const line of (result.stdout ?? "").split("\n")) {
    const eq = line.indexOf("=");
    if (eq === -1) continue;
    env[line.slice(0, eq)] = line.slice(eq + 1);
  }
  if (!env.RUSTC_WRAPPER) {
    throw new Error("sccache-env.mjs --print-env did not report RUSTC_WRAPPER");
  }
  return env;
}

const sccacheEnv = resolveSccacheEnv();
const sccacheBin = sccacheEnv.RUSTC_WRAPPER;

function run(cmd, args, options = {}) {
  const result = spawnSync(cmd, args, { stdio: "inherit", cwd: REPO_ROOT, ...options });
  if (result.error) throw result.error;
  return result.status ?? 1;
}

run(sccacheBin, ["--zero-stats"]);

const buildStatus = run(commandLine, [], {
  shell: true,
  env: { ...process.env, ...sccacheEnv },
});

const stats = spawnSync(sccacheBin, ["--show-stats"], { encoding: "utf8", cwd: REPO_ROOT });
process.stdout.write(stats.stdout ?? "");
process.stderr.write(stats.stderr ?? "");

const requestsLine = (stats.stdout ?? "")
  .split("\n")
  .find((line) => line.trim().startsWith("Compile requests"));
const requests = requestsLine
  ? Number.parseInt(
      requestsLine
        .trim()
        .split(/\s{2,}/)
        .pop() ?? "",
      10,
    )
  : Number.NaN;
if (Number.isFinite(requests) && requests === 0) {
  process.stderr.write(
    "[run-cached] WARNING: sccache reported 0 compile requests — the wrapped " +
      "command never invoked rustc through sccache (fully cached already, " +
      "nothing to compile, or RUSTC_WRAPPER did not take effect).\n",
  );
}

process.exit(buildStatus);

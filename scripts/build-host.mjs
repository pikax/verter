#!/usr/bin/env node
// One shared Cargo host invocation building verter_napi + verter_lsp
// together under the same explicit --target and --profile, with NAPI
// packaging (binding generation, platform-suffix rename, .d.ts, Windows
// artefact copy) run as a SEPARATE step afterward. This is the "one Cargo
// host invocation" + "NAPI packaging separated from its compilation" rule
// in
// docs/arch/refactor/rev11/rulings/MAINTAINER-DIRECTIVE-BUILD-LANE-SEPARATION.md:
// every shared dependency (oxc_allocator, tokio, serde, verter_compiler,
// verter_session, ...) compiles ONCE under target/<target>/<profile>/deps/
// instead of two separate invocations landing in different target
// directories (NAPI explicit-triple, LSP implicit-host) that shared
// nothing.
//
// Usage: node scripts/build-host.mjs [--profile <name>]
// Default profile: artifact-dev — the host developer artifact profile both
// NAPI and the LSP share (see Cargo.toml's `[profile.artifact-dev]`).

import { spawnSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { resolveHostTarget } from "./host-target.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(HERE, "..");
const NATIVE_DIR = path.join(REPO_ROOT, "packages", "native");

function run(command, args, options = {}) {
  const result = spawnSync(command, args, { stdio: "inherit", cwd: REPO_ROOT, ...options });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    process.exit(result.status ?? 1);
  }
}

function parseArgs(argv) {
  let profile = "artifact-dev";
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === "--profile") {
      profile = argv[i + 1];
      i++;
    }
  }
  if (!profile) {
    throw new Error("--profile requires a value");
  }
  return { profile };
}

const { profile } = parseArgs(process.argv.slice(2));
const target = resolveHostTarget();

process.stdout.write(`[build-host] target=${target} profile=${profile}\n`);

// Step 1: ONE combined cargo invocation compiles both crates.
run("cargo", [
  "build",
  "-p",
  "verter_napi",
  "-p",
  "verter_lsp",
  "--target",
  target,
  "--profile",
  profile,
]);

// Step 2: NAPI packaging. Cargo already built the cdylib in step 1 under
// the identical --target/--profile, and the SHARED dependency tree (tokio,
// oxc_allocator, verter_session, verter_compiler, ...) is NOT rebuilt here —
// confirmed empirically via `CARGO_LOG=cargo::core::compiler::fingerprint=info`:
// no shared crate appears dirty or recompiles across repeated runs. But this
// step's own internal `cargo build -p verter_napi` (napi CLI, narrower unit
// selection than step 1's combined `-p verter_napi -p verter_lsp`) is NOT a
// fingerprint-cache no-op either: verter_napi's OWN build/link unit shows
// `UnitDependencyInfoChanged` (a different unit graph than step 1's) plus an
// `EnvVarChanged` on `NAPI_TYPE_DEF_TMP_FOLDER` (napi's own build.rs toggling
// its type-def scratch dir), so verter_napi recompiles/relinks a second time
// here — a genuine, bounded (~3-5s locally) partial rebuild, not zero cost.
// The rest of this step's work is binding generation, the platform-suffixed
// `.node` rename, and `.d.ts` emission.
run("node", [path.join(NATIVE_DIR, "scripts", "clean-dist.mjs")]);
// `shell: true` — on Windows `pnpm` resolves to the `pnpm.cmd` shim, and
// Node's `spawn(..., { shell: false })` (the default) cannot launch a
// `.cmd`/`.bat` shim directly; see scripts/run-cached.mjs's identical
// rationale. POSIX dispatches through `/bin/sh -c` unaffected.
run(
  "pnpm",
  [
    "exec",
    "napi",
    "build",
    "-o",
    "dist",
    "--platform",
    "--target",
    target,
    "--profile",
    profile,
    "--manifest-path",
    path.join("..", "..", "crates", "verter_napi", "Cargo.toml"),
  ],
  { cwd: NATIVE_DIR, shell: true },
);
run("pnpm", ["--filter", "@verter/native", "run", "build:types"], { shell: true });
run("node", [path.join(NATIVE_DIR, "scripts", "copy-windows-artefact.mjs")]);

process.stdout.write(
  `[build-host] LSP binary: target/${target}/${profile}/verter-lsp${
    process.platform === "win32" ? ".exe" : ""
  }\n`,
);

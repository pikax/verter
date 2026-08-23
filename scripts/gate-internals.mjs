// Reusable internals of the canonical Rust gate.
//
// Primitives, classifiers, parsers, single-flight mutex, contained-step
// and multi-step seam runners. No CLI, argv, `process.exit`, or top-level
// side effects — importing this runs nothing. Production `gate.mjs`
// composes them into the real gate. The self-test imports the functions
// in-process so the production CLI never needs a test-seam mode that
// could exit 0 without running the suite.
//
// SECURITY: `gate.mjs` must never expose a CLI mode that returns the
// success contract without running the real gate. Only the self-test
// harness drives the cargo-free seam.

import { spawn, spawnSync } from "node:child_process";
import {
  mkdirSync,
  rmSync,
  writeFileSync,
  readFileSync,
  existsSync,
  renameSync,
  copyFileSync,
  statSync,
  readdirSync,
  realpathSync,
} from "node:fs";
import {
  availableParallelism,
  arch as osArch,
  cpus,
  platform as osPlatform,
  release as osRelease,
  tmpdir,
  totalmem,
  type as osType,
  version as osVersion,
} from "node:os";
import {
  join,
  dirname,
  basename,
  sep,
  isAbsolute,
  resolve,
  relative,
  win32,
  posix,
} from "node:path";
import { createHash } from "node:crypto";

// ----------------------------------------------------------------------------------------------------
// Exit-code constants (distinct, documented). Shared with gate.mjs and gate-selftest.mjs.
//   0   PASS / PASS-WITH-TOLERATED
//   1   FAIL          (a build/test command failed / a non-tolerated test failed)
//   123 ABORTED       (memory ceiling reached, or the RSS monitor became unavailable)
//   124 TIMEOUT       (whole-gate wallclock deadline tripped)
//   125 STALL         (no progress within the stall window)
//   126 LOCK-REFUSED  (another gate holds the single-flight mutex and is alive / lock uninspectable)
//   127 USAGE/SETUP   (bad arguments, repo root not found, archive/list setup failure)
// ----------------------------------------------------------------------------------------------------
export const EXIT_PASS = 0;
export const EXIT_FAIL = 1;
export const EXIT_MEMORY = 123;
export const EXIT_TIMEOUT = 124;
export const EXIT_STALL = 125;
export const EXIT_LOCK_REFUSED = 126;
export const EXIT_USAGE = 127;

// Canonical conformance-harness preflight contract. Command construction is
// pure so the self-test can prove the production gate targets the harness-
// owned executable in both modes without introducing a production test seam.
export const HARNESS_SMOKE_MARKER = "HARNESS-SMOKE FAILED";
export const HARNESS_SMOKE_MODES = Object.freeze(["vapor", "typescript"]);
export const HARNESS_SMOKE_RECEIPT_SCHEMA = "verter-harness-smoke/v1";

export function harnessSmokeCommand(repoRoot, mode, nodePath = process.execPath) {
  if (!HARNESS_SMOKE_MODES.includes(mode)) {
    throw new RangeError(`unknown harness smoke mode: ${JSON.stringify(mode)}`);
  }
  return {
    cmd: nodePath,
    args: [
      join(repoRoot, "packages", "framework-conformance-harness", "bin", "gate-smoke.mjs"),
      mode,
    ],
    cwd: repoRoot,
  };
}

// Convert every runContainedStep result shape into an explicit smoke verdict.
// A status-0 process is insufficient: success requires one exact, mode-bound
// receipt and no timeout/stall/memory/signal/spawn ambiguity.
export function decideHarnessSmokeResult(mode, result) {
  if (result.reason) {
    return { ok: false, detail: `${result.reason}: smoke did not complete` };
  }
  if (result.spawnError) {
    return { ok: false, detail: "spawn error: smoke executable could not be launched" };
  }
  if (result.signalName) {
    return { ok: false, detail: `${result.signalName}: smoke child was signalled` };
  }
  if (result.code !== 0) {
    return { ok: false, detail: `smoke exited non-zero (exit ${result.code})` };
  }
  const output = typeof result.stdout === "string" ? result.stdout.trim() : "";
  if (output === "") return { ok: false, detail: "missing smoke receipt" };
  let receipt;
  try {
    receipt = JSON.parse(output);
  } catch (error) {
    return { ok: false, detail: `invalid smoke receipt JSON (${error.message})` };
  }
  if (receipt === null || typeof receipt !== "object" || Array.isArray(receipt)) {
    return { ok: false, detail: "invalid smoke receipt: expected an object" };
  }
  const keys = Object.keys(receipt).sort();
  if (JSON.stringify(keys) !== JSON.stringify(["mode", "ok", "schema"])) {
    return { ok: false, detail: `receipt keys invalid: ${JSON.stringify(keys)}` };
  }
  if (receipt.schema !== HARNESS_SMOKE_RECEIPT_SCHEMA) {
    return { ok: false, detail: `receipt schema invalid: ${JSON.stringify(receipt.schema)}` };
  }
  if (receipt.mode !== mode) {
    return {
      ok: false,
      detail: `receipt mode mismatch: expected ${mode}, got ${JSON.stringify(receipt.mode)}`,
    };
  }
  if (receipt.ok !== true) {
    return { ok: false, detail: "receipt did not attest ok:true" };
  }
  return { ok: true, receipt };
}

// Single production formatter for every smoke refusal. Keeping the mode-bound prefix here lets the
// self-test pin exact attribution for watchdog, process, and receipt failures without mirroring the
// gate's diagnostic assembly.
export function formatHarnessSmokeFailure(mode, decision) {
  if (!HARNESS_SMOKE_MODES.includes(mode)) {
    throw new RangeError(`unknown harness smoke mode: ${JSON.stringify(mode)}`);
  }
  return `${HARNESS_SMOKE_MARKER} [${mode}]: ${decision.detail}`;
}

// MEMORY / MEMORY_MONITOR reap escalation is far tighter than the TIMEOUT/STALL default (5000ms):
// a ceiling breach means the tree is allocating right now, so runContainedStep's reapNow gives it this
// short a SIGTERM grace (instead of killGraceMs) before SIGKILL — materially bounding how much further it
// can grow after the breach is observed. See runContainedStep's memoryKillGraceMs option.
export const MEMORY_KILL_GRACE_MS = 200;

export const IS_WINDOWS = process.platform === "win32";
export const IS_MAC = process.platform === "darwin";

const MiB = 1024 ** 2;
const GiB = 1024 ** 3;

export function parseMemorySize(value) {
  const match = /^([0-9]+(?:\.[0-9]+)?)(MiB|GiB)$/i.exec(String(value || "").trim());
  if (!match)
    throw new Error(
      `memory limit must be a positive MiB/GiB value (for example 12288MiB or 12GiB)`,
    );
  const bytes = Number(match[1]) * (match[2].toLowerCase() === "gib" ? GiB : MiB);
  if (!Number.isFinite(bytes) || bytes <= 0) throw new Error("memory limit must be positive");
  return Math.floor(bytes);
}

export function formatMemorySize(bytes) {
  if (!Number.isFinite(bytes) || bytes < 0) return "unknown";
  if (bytes < GiB) return `${(bytes / MiB).toFixed(0)} MiB`;
  return `${(bytes / GiB).toFixed(2)} GiB`;
}

function positiveInteger(value, label) {
  const parsed = Number(value);
  if (!Number.isSafeInteger(parsed) || parsed <= 0)
    throw new Error(`${label} must be a positive integer`);
  return parsed;
}

// Independently measured caps for the canonical gate. On the benchmark host, twelve-way Cargo compilation
// and twelve-way nextest execution were the fastest tested points on their separate 4/8/12 axes. Keep the
// policies independently owned: omitted build jobs also honor the memory tier below, while omitted test
// threads only clamp to available CPU capacity. Explicit caller overrides remain exact positive values.
// The child-tree RSS ceiling continues to reserve half of physical memory for the OS, the parent agent,
// editors, and unrelated work.
export const DEFAULT_GATE_BUILD_JOBS = 12;
export const DEFAULT_GATE_TEST_THREADS = 12;
export const GATE_BUILD_JOBS_12_MIN_MEMORY_LIMIT_BYTES = 16 * GiB;
export const GATE_BUILD_JOBS_8_MIN_MEMORY_LIMIT_BYTES = 12 * GiB;

export function deriveGateResourceLimits({
  cpuCount = typeof availableParallelism === "function" ? availableParallelism() : cpus().length,
  totalMemBytes = totalmem(),
  buildJobs,
  testThreads,
  memoryLimitBytes,
} = {}) {
  const saneCpuCount = Number.isSafeInteger(cpuCount) && cpuCount > 0 ? cpuCount : 1;
  const saneTotalMem =
    Number.isFinite(totalMemBytes) && totalMemBytes > 0 ? totalMemBytes : 2 * GiB;
  const effectiveMemoryLimitBytes = positiveInteger(
    memoryLimitBytes ?? Math.max(512 * MiB, Math.floor(saneTotalMem * 0.5)),
    "memory limit bytes",
  );
  // Twelve build jobs peaked at 11.60 GiB on the measured host, leaving unsafe headroom under the
  // documented 24-GiB host's 12-GiB default ceiling. Use twelve only from a 16-GiB effective ceiling;
  // a 12-GiB ceiling selects the measured 8-job point (9.90-GiB peak), and smaller ceilings retain the
  // prior four-job cap. Test execution is independently much lighter (3.84-GiB peak at twelve threads).
  const memoryBoundBuildJobs =
    effectiveMemoryLimitBytes >= GATE_BUILD_JOBS_12_MIN_MEMORY_LIMIT_BYTES
      ? 12
      : effectiveMemoryLimitBytes >= GATE_BUILD_JOBS_8_MIN_MEMORY_LIMIT_BYTES
        ? 8
        : 4;
  const defaultBuildJobs = Math.max(
    1,
    Math.min(DEFAULT_GATE_BUILD_JOBS, memoryBoundBuildJobs, saneCpuCount),
  );
  const defaultTestThreads = Math.max(1, Math.min(DEFAULT_GATE_TEST_THREADS, saneCpuCount));
  return {
    buildJobs: positiveInteger(buildJobs ?? defaultBuildJobs, "build jobs"),
    testThreads: positiveInteger(testThreads ?? defaultTestThreads, "test threads"),
    memoryLimitBytes: effectiveMemoryLimitBytes,
  };
}

// The two post-list lanes (Surface 1 and the shipped-cfg guard) normally run CONCURRENTLY — Surface 1's
// archive-backed nextest run overlaps the shipped-cfg lane's own `cargo check` compile (its own cold,
// isolated target dir) and then its `nextest run` build+execute. `deriveGateResourceLimits` above sizes ONE
// ceiling; historically BOTH lanes were independently sized to that SAME ceiling, so a host that measured
// itself at N cores requested 2N cores for the whole overlap window ("cargo build jobs=8, test
// threads=8" applied twice, concurrently, on an 8-core host). This function partitions ONE ceiling across
// the two lanes so their COMBINED demand — build jobs and test threads independently — never exceeds it.
// Surface 1 (the full workspace test universe) gets the majority share; the shipped-cfg lane (a small
// package-scoped contract — ten-ish tests — whose own wall-clock matters far less than Surface 1's) gets
// a minority share, floored at 1.
//
// Temporary skip of that shipped-cfg lane. Flip to `true` to restore both steps:
//   cargo check --workspace --all-targets --profile no-debug-assertions
//   cargo nextest run -p verter_shipped_cfg_contract --cargo-profile no-debug-assertions
// TODO: re-enable. Until then the gate does not execute tests with debug_assertions /
// overflow-checks off. That is the only path that catches a state mutation written inside a
// debug_assert! argument — a silent no-op in every shipped build, while compiling and passing
// in debug. `cargo check --workspace --release` compiles the shipped cfg but runs nothing, so
// it does not cover this class.
export const SHIPPED_CFG_LANE_ENABLED = false;
export const SHIPPED_CFG_SKIP_SUMMARY =
  "SHIPPED-CFG GUARD: SKIPPED (temporary). This verdict is Surface 1 only; " +
  "cargo check --workspace --all-targets --profile no-debug-assertions and " +
  "cargo nextest run -p verter_shipped_cfg_contract --cargo-profile no-debug-assertions did not run. " +
  "Until re-enabled, a state mutation written inside a debug_assert! argument is uncovered " +
  "(silent no-op in every shipped build; cargo check --workspace --release compiles that cfg but runs nothing).";
export const SHIPPED_CFG_SKIP_VERDICT_NOTE = "shipped-cfg guard SKIPPED";
//
// Below a ceiling of 2 on either axis there is no way to give both lanes >= 1 unit of that axis while also
// bounding their SUM to the ceiling — a lane cannot run `cargo`/`nextest` with 0 build jobs or 0 test
// threads. Rather than let the caller run both lanes concurrently at a combined demand that exceeds the
// ceiling (the exact defect this function exists to fix), the returned `concurrent: false` flag tells the
// caller the two lanes must be run ONE AT A TIME instead — see `orchestrateGateLanes`'s `concurrent` option.
// Serialized, only one lane's `cargo`/`nextest` invocation is ever live, so the ceiling is honored even
// though each lane's own numeric share still reads 1 (the minimum a lane can run with). `concurrent` is
// false whenever EITHER axis is unsplittable at this ceiling, even if the other axis has room, because the
// two lanes overlap in wall-clock as a unit — a build-axis oversubscription is not cured by a fine
// test-axis split.
export const SHIPPED_CFG_LANE_SHARE = 0.25;

export function deriveGateLaneResourceSplit({ buildJobs, testThreads }) {
  const totalBuildJobs = positiveInteger(buildJobs, "build jobs");
  const totalTestThreads = positiveInteger(testThreads, "test threads");
  const split = (total) => {
    if (total <= 1) return { surface: total, shipped: total };
    const shipped = Math.max(1, Math.min(total - 1, Math.round(total * SHIPPED_CFG_LANE_SHARE)));
    return { surface: total - shipped, shipped };
  };
  const buildSplit = split(totalBuildJobs);
  const testSplit = split(totalTestThreads);
  return {
    surface: { buildJobs: buildSplit.surface, testThreads: testSplit.surface },
    shippedCfg: { buildJobs: buildSplit.shipped, testThreads: testSplit.shipped },
    concurrent: totalBuildJobs >= 2 && totalTestThreads >= 2,
  };
}

// cargo-nextest's Windows archive contains test executables but omits their hashed PDB sidecars. Most
// tests do not need a PDB at runtime, but verter_napi's allocation-site audit intentionally proves that
// sampled frames resolve to the semantic caller name. Running that test from the extracted archive without
// its matching PDB turns every frame into a raw address and makes the canonical gate differ from a direct
// Cargo run. Keep the required set closed and explicit: copying every workspace PDB would duplicate several
// gigabytes, while silently weakening the attribution assertion would stop testing the production contract.
const WINDOWS_RUNTIME_SYMBOL_SUITES = Object.freeze(["verter_napi"]);

/**
 * Restore the closed set of runtime-required Windows PDBs beside their extracted test binaries.
 *
 * The filesystem operations are injectable so the gate self-test exercises the exact production path on
 * every host without writing fake Windows paths. Returns an error instead of throwing for every malformed
 * archive/suite/source shape; the production gate maps that to a loud setup failure before Surface 1.
 */
export function ensureRequiredWindowsDebugSidecars({
  allSuites,
  runnerTarget,
  extractDir,
  destinationExtractDir = extractDir,
  windows = IS_WINDOWS,
  existsFn = existsSync,
  mkdirFn = (path) => mkdirSync(path, { recursive: true }),
  copyFileFn = copyFileSync,
}) {
  if (!windows) return { copied: 0 };
  if (!Array.isArray(allSuites))
    return { error: "archive suite listing is not an array", copied: 0 };

  const pathApi = windows ? win32 : posix;
  const extractedTarget = pathApi.resolve(extractDir, "target");
  let copied = 0;

  for (const binaryId of WINDOWS_RUNTIME_SYMBOL_SUITES) {
    const matches = allSuites.filter((suite) => suite && suite["binary-id"] === binaryId);
    if (matches.length !== 1) {
      return {
        error: `required Windows symbol suite '${binaryId}' occurs ${matches.length} times in the archive (expected exactly 1)`,
        copied,
      };
    }

    const binaryPath = matches[0]["binary-path"];
    if (typeof binaryPath !== "string" || !binaryPath.toLowerCase().endsWith(".exe")) {
      return {
        error: `required Windows symbol suite '${binaryId}' has no .exe binary path`,
        copied,
      };
    }

    const resolvedBinary = pathApi.resolve(binaryPath);
    const relativeBinary = pathApi.relative(extractedTarget, resolvedBinary);
    if (
      relativeBinary === "" ||
      relativeBinary === ".." ||
      relativeBinary.startsWith(`..${pathApi.sep}`) ||
      pathApi.isAbsolute(relativeBinary)
    ) {
      return {
        error: `required Windows symbol suite '${binaryId}' escapes the extracted target tree: ${binaryPath}`,
        copied,
      };
    }

    const toPdb = (path) => `${path.slice(0, -4)}.pdb`;
    const sourcePdb = toPdb(pathApi.join(pathApi.resolve(runnerTarget), relativeBinary));
    const destinationPdb = toPdb(
      pathApi.join(pathApi.resolve(destinationExtractDir, "target"), relativeBinary),
    );
    if (!existsFn(sourcePdb)) {
      return {
        error: `required Windows debug sidecar is missing for '${binaryId}': ${sourcePdb}`,
        copied,
      };
    }

    try {
      // `--extract-overwrite` replaces members present in the new archive but does not remove
      // sidecars the archive omits. Therefore destination existence proves nothing about freshness:
      // always overwrite it from the PDB produced alongside this run's exact test executable.
      if (pathApi.resolve(destinationExtractDir) !== pathApi.resolve(extractDir)) {
        mkdirFn(pathApi.dirname(destinationPdb));
      }
      copyFileFn(sourcePdb, destinationPdb);
    } catch (error) {
      return {
        error: `could not copy required Windows debug sidecar for '${binaryId}': ${error && error.message ? error.message : error}`,
        copied,
      };
    }
    if (!existsFn(destinationPdb)) {
      return {
        error: `required Windows debug sidecar copy did not materialize for '${binaryId}': ${destinationPdb}`,
        copied,
      };
    }
    copied += 1;
  }

  return { copied };
}

// ----------------------------------------------------------------------------------------------------
// Platform-aware `pnpm install --frozen-lockfile` command resolution (returns `{ cmd, args }`, plus
// `windowsVerbatimArguments: true` on the Windows arm, for `runContainedStep`). The launch uses the
// RESOLVED `pnpmPath` (the path `resolvePnpm` already proved on
// PATH in the preflight) as the SINGLE SOURCE OF TRUTH — never a bare `pnpm` token. A bare token is a
// CWD-tool-source hazard on Windows: `cmd.exe` searches the CURRENT DIRECTORY first for a bare command,
// so a bare `pnpm` could resolve a repo-local `pnpm.cmd`/`.bat`/`.exe` (or a different PATHEXT candidate)
// than the resolver approved — letting the CWD control the installer and creating a preflight-vs-installer
// asymmetry. Launching the resolved absolute path removes that bare-token CWD search.
//
// On POSIX the resolved `pnpmPath` is a directly-executable shim, so `spawn(pnpmPath, …, { shell:false })`
// launches it with NO PATH re-search. On Windows `pnpmPath` is `pnpm.cmd` (a batch shim), and Node's
// `child_process.spawn` with `shell:false` CANNOT launch a `.cmd`/`.bat` directly (documented Node
// behavior — it errors with ENOENT / a spawn error). So on Windows we invoke the resolved path THROUGH
// the command processor: `cmd.exe` is a real `.exe` that `spawn` launches directly with `shell:false`,
// and crucially it stays the REAPABLE TREE ROOT — `runContainedStep`'s teardown keys on `child.pid` and
// reaps the whole tree via `taskkill /PID <pid> /T /F`, so cmd.exe (not the .cmd shim) must be the
// spawned process. We pass the classic `cmd /d /s /c "<quoted-command>"` form as ONE verbatim args
// element (`""<pnpmPath>" install --frozen-lockfile"`) so a `.cmd` path containing spaces survives, and
// the caller forwards `windowsVerbatimArguments: true` to `spawn` so Node does NOT re-quote the args.
// The explicit cmd.exe command keeps the spawn `shell:false` and the containment model UNCHANGED (we do
// NOT switch `runContainedStep` to `shell:true`). `windows` and `env` are parameterized so this is
// unit-testable on a POSIX host without actually spawning. `--frozen-lockfile` is preserved on both
// paths (the install never mutates the lockfile).
//
// The Windows command processor is PINNED to a VERIFIED ABSOLUTE executable — never a bare/relative
// `cmd.exe` token (which `cmd.exe`'s own CWD-first search, or `spawn`'s lookup, could resolve to a repo-local
// imposter). Resolution order: (1) read `ComSpec` CASE-INSENSITIVELY (Windows folds env-var names, so the
// key may be `COMSPEC`/`ComSpec`/`comspec`) — use that value ONLY if it is ABSOLUTE and an existing file;
// (2) else read `SystemRoot` CASE-INSENSITIVELY and form `<SystemRoot>\System32\cmd.exe` — use it if it is
// ABSOLUTE and an existing file; (3) else SETUP-FAIL — return `{ setupFail: true, detail }` so the caller
// FAILS LOUD (the production `runInstall` closure turns this into a `runContainedStep`-shaped `spawnError`
// result, which `preflightFreshnessTooling` already maps to action "setup-fail" ⇒ EXIT_USAGE) rather than
// launching a bare `cmd.exe`. The is-file presence predicate is INJECTED (`isFileFn`, default
// `defaultIsFile`, mirroring the Rust `Path::is_file()`) so the self-test drives a fake fs with NO real
// filesystem access — the same injection pattern `resolveExecutableShim` / `resolveLocalBinShim` use. The
// `<SystemRoot>\System32\cmd.exe` path is built with explicit `\` separators ON PURPOSE: it is a Windows
// path STRING (data), not a host path join, so it stays correct when the Windows branch runs on a POSIX
// self-test host. The POSIX branch is UNCHANGED — the resolved `pnpmPath` is directly executable.
// ----------------------------------------------------------------------------------------------------
export function pnpmInstallCommand(
  pnpmPath,
  windows = IS_WINDOWS,
  env = process.env,
  isFileFn = defaultIsFile,
) {
  if (windows) {
    const cmdProcessor = resolveWindowsCommandProcessor(env, isFileFn);
    if (cmdProcessor === null) {
      return {
        setupFail: true,
        detail:
          `no absolute Windows command processor could be resolved: neither a case-insensitive \`ComSpec\` ` +
          `that is absolute and an existing file, nor an absolute existing \`<SystemRoot>\\System32\\cmd.exe\` ` +
          `(\`SystemRoot\` also read case-insensitively). Refusing to launch a bare/relative \`cmd.exe\` ` +
          `(a CWD-search hazard). Set a valid \`ComSpec\` or \`SystemRoot\` and re-run the gate.`,
      };
    }
    return {
      cmd: cmdProcessor,
      args: ["/d", "/s", "/c", `""${pnpmPath}" install --frozen-lockfile"`],
      windowsVerbatimArguments: true,
    };
  }
  return { cmd: pnpmPath, args: ["install", "--frozen-lockfile"] };
}

// Canonical, content-addressed Vue macro oracle verification. The gate owns
// these launch descriptions so tests can prove the live gate and the package
// scripts execute byte-identical Node entry points without a shell or PATH
// lookup. `nodePath` is `process.execPath` in production.
export function vueMacroOracleGateCommands(nodePath) {
  return [
    {
      name: "gen:vue-macro-oracle:check",
      cmd: nodePath,
      args: ["scripts/gen-vue-macro-runtime-oracle.mjs", "--check"],
    },
    {
      name: "test:vue-macro-oracle",
      cmd: nodePath,
      args: ["--test", "scripts/vue-macro-runtime-oracle/oracle.test.mjs"],
    },
  ];
}

// Resolve the Windows command processor to a VERIFIED ABSOLUTE executable, or null when none is available.
// Reads `ComSpec` then `SystemRoot` CASE-INSENSITIVELY (Windows folds env-var names case-insensitively, so
// the gate must find the value under ANY casing the way the OS would). A candidate is accepted ONLY when it
// is a fully-qualified absolute Windows path AND `isFileFn` reports it as an existing file — a relative
// `cmd.exe`, a `%SystemRoot%`-derived path that does not exist, or a missing/empty value all fall through.
// `isFileFn` is injected (default `defaultIsFile`, mirroring the Rust `Path::is_file()`) so the self-test
// can drive a fake fs.
function resolveWindowsCommandProcessor(env, isFileFn) {
  const comSpec = readEnvCaseInsensitive(env, "COMSPEC");
  if (comSpec && isAbsoluteWindowsPath(comSpec) && isFileFn(comSpec)) return comSpec;
  const systemRoot = readEnvCaseInsensitive(env, "SYSTEMROOT");
  if (systemRoot) {
    // Build the canonical `<SystemRoot>\System32\cmd.exe` with explicit `\` separators — a Windows path
    // STRING (data), not a host path join, so it stays correct when this runs on a POSIX self-test host.
    const candidate = `${systemRoot}\\System32\\cmd.exe`;
    if (isAbsoluteWindowsPath(candidate) && isFileFn(candidate)) return candidate;
  }
  return null;
}

// Read an env value by a CASE-INSENSITIVE key match (`wantUpper` is the already-upper-cased name). Returns
// the first string value whose key folds to `wantUpper`, else undefined. Windows folds env-var names
// case-insensitively, so `ComSpec` may be stored as `COMSPEC`/`comspec`/`ComSpec`.
function readEnvCaseInsensitive(env, wantUpper) {
  for (const k of Object.keys(env)) {
    if (k.toUpperCase() === wantUpper && typeof env[k] === "string") return env[k];
  }
  return undefined;
}

// Pure SYNTACTIC Windows-absolute classification (no filesystem IO, platform-independent so it is correct on
// a POSIX self-test host): a drive-ROOTED `C:\x` / `C:/x`, a UNC `\\server\share`, or a device `\\?\…` /
// `\\.\…` path. A drive-RELATIVE `C:foo` (drive + colon WITHOUT a separator) and a root-relative `\x` / `/x`
// are NOT absolute.
function isAbsoluteWindowsPath(p) {
  if (/^[\\/]{2}/.test(p)) return true; // UNC / device (`\\?\` / `\\.\` subsumed)
  return /^[A-Za-z]:[\\/]/.test(p); // drive-rooted (drive-relative `C:foo` excluded)
}

// ----------------------------------------------------------------------------------------------------
// Freshness-tooling shim resolution (platform-aware) — mirrors the Rust freshness test's
// `resolve_executable_shim` / `locate_buf_binary` / `locate_oxfmt_binary`
// (crates/verter_protocol/tests/cases/typeinfo_proto_ts_freshness.rs). The two byte-equality freshness
// tests regenerate the committed TS proto bindings through the workspace `buf` + `oxfmt` binaries; those
// binaries are resolved FIRST under `node_modules/.bin`, then PATH. In a fresh `git worktree` nobody runs
// `pnpm install`, so `node_modules` is absent and `oxfmt` (a Node devDependency that typically is NOT on
// PATH, so a fresh worktree without `pnpm install` lacks it — though the resolver, like the Rust test's
// `locate_oxfmt_binary` PATH fallback, DOES fall back to PATH if it happens to be present there) is
// missing. With the tooling absent the buf-absent pair SKIPS-and-PASSES (no FAIL line),
// so the gate self-ensures the tooling here precisely so the byte-pin RUNS GENUINELY — it must NOT blanket-
// swallow the pair as "env-only tolerated", because that would also mask a fixable missing `pnpm install` OR
// a GENUINE drift (tools present, bindings stale, the test RUNS and FAILS). It tolerates the pair ONLY when
// the tools are GENUINELY absent (install could not run at all). The is-file presence predicate is injected
// so the self-test can drive a fake filesystem; production passes the live `defaultIsFile` (mirroring the
// Rust `Path::is_file()`, so a directory at a shim path does NOT count as the tool being present).
//
// POSIX: the extensionless `node_modules/.bin/<tool>` shim is directly executable — return it if it
// exists. Windows: `CreateProcess` cannot launch the extensionless POSIX shell script, so try the runnable
// `.CMD`/`.cmd`/`.exe`/`.bat` forms (this exact order, matching the Rust resolver) and return the first
// that exists. Returns the resolved path string, or `null` when none exists.
//
// PRESENCE PREDICATE: the Rust freshness test decides a shim is present with `Path::is_file()`, so these
// resolvers mirror that with an IS-FILE check (`defaultIsFile`), NOT a bare `existsSync` — a corrupt
// DIRECTORY at a shim path (e.g. `node_modules/.bin/buf/`) must NOT count as the tool being present
// (which would wrongly DISABLE install/tolerance and diverge from the Rust predicate). `isFileFn` is
// injected so the self-test can drive a fake fs; production passes the live `defaultIsFile`. For a real
// file shim the result is identical to the old `existsSync` check — the only behavior change is that a
// directory at the shim path now resolves to `null`.
// ----------------------------------------------------------------------------------------------------
export const WINDOWS_SHIM_EXTS = ["CMD", "cmd", "exe", "bat"];

// Default IS-FILE predicate mirroring the Rust `Path::is_file()`: a path counts as present only when it
// is a regular file (a directory, a missing path, or an stat error all return false).
export function defaultIsFile(p) {
  try {
    return statSync(p).isFile();
  } catch {
    return false;
  }
}

// The LITERAL PATH-list delimiter selected SOLELY by the injected `windows` flag — `;` for Windows, `:`
// for POSIX — and NOT the ambient `node:path.delimiter` (which is the HOST's delimiter, i.e. `;` on a
// Windows host and `:` on POSIX). Reading the host `delimiter` would make a `windows:false` self-test on a
// Windows host split a POSIX PATH on `;` (a false test-harness failure for a Windows contributor) and
// vice-versa. This helper makes the PATH-splitting helpers (`resolvePathShim`, `sanitizePathValue`) truly
// platform-PARAMETERIZED — the `windows` flag fully determines the delimiter, host-independent — which is
// the stated intent of that flag. Single source of truth for both loci (no duplicated `windows ? ";" : …`).
export function pathDelimiterFor(windows) {
  return windows ? ";" : ":";
}

// Concatenate a PATH component `dir` with a bare `toolName` WITHOUT any normalization — the JS mirror of the
// Rust freshness test's `std::path::Path::join(dir, tool)` / `PathBuf::push`, which are PURELY LEXICAL: they
// do NOT collapse `..`/`.` and do NOT resolve symlinks. Deliberately NOT `node:path.join`/`normalize`/
// `resolve` — those NORMALIZE (`/tmp/link/../bin` => `/tmp/bin`), so against a symlinked-or-`..`-bearing
// absolute component the JS candidate would address a DIFFERENT file than the Rust child (a residual
// preflight-vs-child disagreement that could silently tolerate a real stale-binding regression). Returns
// `dir + sep + toolName` where `sep` is the platform separator selected by the injected `windows` flag
// (`\` on Windows, else `/`), UNLESS `dir` already ends with a platform separator — in which case `toolName`
// is appended directly, with no doubled separator. The trailing-separator test is platform-ACCURATE to match
// Rust's `Path::join`: on Windows a trailing `/` OR `\` suppresses the inserted separator (both are path
// separators), but on POSIX ONLY a trailing `/` does — a trailing `\` on POSIX is an ORDINARY filename byte,
// so the `/` IS inserted (`/tmp/abs\` + `buf` => `/tmp/abs\/buf`), mirroring POSIX `Path::join`. Pure string
// concatenation, byte-preserving on `dir`. An EMPTY `dir` (`""`) returns `toolName` UNCHANGED (no leading
// separator), mirroring Rust's `Path::new("").join(tool)` / `PathBuf::from("").push(tool)`, both of which
// yield exactly `tool` (the relative file itself) — see the `"".join("buf") == "buf"` note on
// `sanitizePathValue`. In production an empty PATH entry never reaches this helper: the resolver
// (`resolvePathShim`) drops it via `if (!dir) continue` and `sanitizePathValue` rejects empties UPSTREAM, so
// this short-circuit is a contract-completeness/footgun-closure (a true byte-for-byte mirror at the empty
// edge), not a live production code path.
export function appendPathComponentRaw(dir, toolName, windows) {
  if (dir === "") return toolName;
  const sep = windows ? "\\" : "/";
  const endsWithSep = windows ? dir.endsWith("/") || dir.endsWith("\\") : dir.endsWith("/");
  return endsWithSep ? `${dir}${toolName}` : `${dir}${sep}${toolName}`;
}

// Resolve a single base path (an extensionless shim path, e.g. `<dir>/buf`) to a runnable form, applying
// the platform-aware suffix handling. `windows` is parameterized (defaulting to the live platform) so the
// Windows branch is unit-testable on a POSIX host. `isFileFn` is the is-file presence predicate (default
// `defaultIsFile`). Returns the resolved path, or null.
export function resolveExecutableShim(basePath, isFileFn = defaultIsFile, windows = IS_WINDOWS) {
  if (windows) {
    for (const ext of WINDOWS_SHIM_EXTS) {
      const candidate = `${basePath}.${ext}`;
      if (isFileFn(candidate)) return candidate;
    }
    return null;
  }
  return isFileFn(basePath) ? basePath : null;
}

// Resolve a tool's LOCAL `node_modules/.bin` shim (the version-locked devDependency form the freshness
// test prefers). `repoRoot` is the workspace root; `toolName` is the bare tool (`buf` / `oxfmt`). Returns
// the runnable shim path, or null when no local shim exists. Mirrors the `node_modules/.bin/<tool>`
// preference in the Rust `locate_*_binary` helpers.
export function resolveLocalBinShim(
  repoRoot,
  toolName,
  isFileFn = defaultIsFile,
  windows = IS_WINDOWS,
) {
  const base = join(repoRoot, "node_modules", ".bin", toolName);
  return resolveExecutableShim(base, isFileFn, windows);
}

// Find the actual env key that NAMES the PATH variable, matching how the Rust freshness test reads it.
// Rust's `std::env::var_os("PATH")` is CASE-SENSITIVE on POSIX (it reads `PATH` and never `Path`) but the
// Windows OS folds env-var names case-INSENSITIVELY, so `var_os("PATH")` there resolves a PATH var stored
// under ANY casing (`PATH`/`Path`/`PaTh`/`path`). The gate's JS therefore must read AND sanitize the SAME
// key the Rust side reads, or the preflight verdict and the executed test disagree on a non-canonical
// casing. Returns the env key string, or null when no PATH var is present.
//   - POSIX: `"PATH"` iff `typeof env.PATH === "string"` (case-EXACT — a `Path` key is a DIFFERENT,
//     non-PATH var Rust never reads, so it is not the PATH key on POSIX).
//   - Windows: the case-INSENSITIVE PATH key. DETERMINISTIC tie-break when multiple casings coexist
//     (pathological): prefer an exact `PATH`, then `Path`, then the FIRST remaining `k` with
//     `k.toUpperCase() === "PATH"` and a string value (stable insertion-order scan).
export function findPathEnvKey(env, windows) {
  if (!windows) {
    return typeof env.PATH === "string" ? "PATH" : null;
  }
  if (typeof env.PATH === "string") return "PATH";
  if (typeof env.Path === "string") return "Path";
  for (const k of Object.keys(env)) {
    if (k.toUpperCase() === "PATH" && typeof env[k] === "string") return k;
  }
  return null;
}

// Resolve a tool from PATH (the `which <tool>` fallback the Rust helpers use after the local shim miss).
// Splits the PATH var on the LITERAL platform delimiter selected by the injected `windows` flag
// (`pathDelimiterFor(windows)` — `;` for Windows, `:` for POSIX), NOT the ambient host `node:path.delimiter`,
// so the delimiter is host-INDEPENDENT: a `windows:true` self-test on a POSIX host genuinely exercises
// `;`-separated multi-entry Windows PATH resolution, and a `windows:false` resolve on a Windows host still
// splits on `:`. This matches `sanitizePathValue`, which splits on the same `pathDelimiterFor(windows)`. The
// per-directory candidate is then built by `appendPathComponentRaw` — a NON-NORMALIZING concatenation that
// mirrors the Rust child's purely-lexical `dir.join(tool)` (no `..`/`.` collapse, no symlink resolution), so
// an absolute component containing `..`/`.` or a symlink resolves IDENTICALLY on both sides — and the same
// suffix handling is applied per directory. Returns the first runnable path, or null. `env` and the
// platform are injected for the self-test's fake env. This
// resolver itself skips EMPTY PATH components only (`if (!dir) continue`). The cwd-relative stripping is
// NOT done here — it is applied UPSTREAM by `buildCargoEnv` / `sanitizePathValue` to the env handed to this
// resolver, so in the production gate (which always passes the sanitized `buildCargoEnv` env) the resolver
// only ever sees CWD-INDEPENDENT ABSOLUTE PATH components: no empty, no dot-only, no non-dot relative, no
// `..`-relative, and no Windows drive-relative / root-relative entry survives the sanitizer. Calling
// `resolvePathShim` directly on a raw `{ PATH: "." }` returns the tool (the resolver does not itself filter
// relative entries); only the upstream sanitizer drops them. Together they make the preflight verdict and
// the executed Rust freshness test resolve from the SAME absolute-only PATH — the CLOSED invariant: gate
// PATH is cwd-independent/absolute-only for both the preflight resolver and the test children, so a real
// stale-binding regression still FAILS (both sides resolve `buf` identically) with no cwd-relative
// disagreement.
export function resolvePathShim(
  toolName,
  env = process.env,
  isFileFn = defaultIsFile,
  windows = IS_WINDOWS,
) {
  // Read PATH from the SAME key the Rust freshness test reads via `std::env::var_os("PATH")`. On POSIX that
  // is CASE-SENSITIVE — it reads `PATH` and never `Path` — so `findPathEnvKey(env, false)` returns `"PATH"`
  // only: a `Path`-only env (no `PATH`) yields NO PATH, matching Rust's `var_os("PATH") => None`, so the JS
  // resolver and the Rust test AGREE (a POSIX `Path` fallback would resolve a tool JS-side that Rust skips —
  // a fail-open asymmetry). On Windows env-var names fold case-INSENSITIVELY, so `var_os("PATH")` resolves a
  // PATH var stored under ANY casing (`PATH`/`Path`/`PaTh`/`path`); `findPathEnvKey(env, true)` returns that
  // actual case-insensitively-matched key so the resolver reads exactly what Rust reads. `??` (not `||`) so
  // a present `""` at that key is NOT treated as "fall back" — a present empty PATH stays empty.
  const pathKey = findPathEnvKey(env, windows);
  const pathVar = (pathKey === null ? undefined : env[pathKey]) ?? "";
  if (!pathVar) return null;
  // Split on the LITERAL platform delimiter selected by the `windows` flag (`pathDelimiterFor(windows)` —
  // `;` on Windows, `:` on POSIX), host-INDEPENDENT (NOT the ambient `node:path.delimiter`), so a
  // `windows:true` resolve splits a `;`-separated PATH correctly even on a POSIX host and a `windows:false`
  // resolve splits on `:` even on a Windows host (matching `sanitizePathValue`, which splits on the same
  // `pathDelimiterFor(windows)`).
  const pathDelim = pathDelimiterFor(windows);
  for (const dir of pathVar.split(pathDelim)) {
    // Skips EMPTY components only. The cwd-relative stripping lives UPSTREAM in `sanitizePathValue` (applied
    // by `buildCargoEnv` to the env this resolver consumes), so the production gate only ever hands this loop
    // CWD-INDEPENDENT ABSOLUTE components (no dot-only, non-dot relative, `..`-relative, or Windows
    // drive-relative / root-relative entry survives); calling the resolver directly on a raw `{ PATH: "." }`
    // does NOT itself filter the `.`. Together they make the preflight and the executed Rust test resolve
    // `buf` from the SAME absolute-only PATH — the closed cwd-independent invariant, so a real regression
    // still FAILS with no preflight-vs-test disagreement.
    if (!dir) continue;
    // Build the candidate by RAW non-normalizing concatenation (`appendPathComponentRaw`), mirroring the Rust
    // child's lexical `dir.join(tool)` — NOT `node:path.join`, which would collapse `..`/`.` and make the JS
    // candidate address a different file than the Rust child for a symlinked/`..`-bearing absolute component.
    const resolved = resolveExecutableShim(
      appendPathComponentRaw(dir, toolName, windows),
      isFileFn,
      windows,
    );
    if (resolved) return resolved;
  }
  return null;
}

// The freshness tools the byte-equality tests need. Both are ENSURED (present / installed) before the
// archive build; `FRESHNESS_TOOLS` is the "ensure both" set used for the both-present short-circuit and the
// post-install re-resolve. It is NOT the tolerance gate — see `BUF_TOOL` below.
export const FRESHNESS_TOOLS = ["buf", "oxfmt"];

// The SKIP-DETERMINING tool. The Rust freshness test (`regenerate_gen_tree_via_buf` in
// crates/verter_protocol/tests/cases/typeinfo_proto_ts_freshness.rs) skips GRACEFULLY iff `buf` is
// unavailable — `let buf_bin = locate_buf_binary(root)?;` early-returns `None`. If `buf` IS present it RUNS
// `buf generate` + the byte-compare; `oxfmt` is CONDITIONAL there (`if let Some(oxfmt_bin) =
// locate_oxfmt_binary(root)`), so a MISSING `oxfmt` only skips the format step — the test STILL byte-compares
// and can FAIL. Therefore the byte-pin TOLERANCE keys on `buf` SPECIFICALLY (mirrors the Rust
// `locate_buf_binary(root)?` early-return), NOT on "both tools": tolerance is allowed ONLY when `buf` is not
// resolvable. `oxfmt` is a REQUIRED-WHEN-buf-PRESENT canonical tool — a missing `oxfmt` with `buf` available
// is a LOUD setup-fail (it would otherwise run a degraded, un-oxfmt'd byte-compare that can false-positive),
// never tolerate and never a degraded run.
export const BUF_TOOL = "buf";

// The installer the freshness tools come from. "pnpm genuinely absent" is a POSITIVE, platform-aware
// resolver fact (resolved via PATH — with the WINDOWS_SHIM_EXTS suffix set on Windows — via `resolvePnpm`),
// determined BEFORE the install. The RESOLVED path that probe returns is then the exact binary
// `pnpmInstallCommand` launches (POSIX: directly; Windows: that resolved `.cmd` path quoted under
// `cmd.exe /d /s /c`), so the probe and the launch run the SAME binary — NOT a bare `pnpm` token re-searched
// at launch time. Absence is NOT inferred from the install's `spawnError`: after the Windows install was
// wrapped as `cmd.exe /d /s /c "<resolved-pnpm>" install …` (Node's `spawn(shell:false)` cannot launch a
// `.cmd` shim directly), the install's `spawnError` means "cmd.exe failed to spawn", NOT "pnpm absent", so it
// can no longer stand in for pnpm-absence.
export const PNPM_TOOL = "pnpm";

// Resolve `pnpm` as a POSITIVE fact — via PATH ONLY (with the WINDOWS_SHIM_EXTS suffix handling on Windows).
// This is PATH-only ON PURPOSE: pnpm is the bootstrap INSTALL DRIVER, not a version-locked freshness shim, so
// a repo-local `node_modules/.bin/pnpm` must NOT become an approved installer. The RESOLVED path this returns
// is the single source of truth the launcher uses: `pnpmInstallCommand(pnpmPath, …)` runs THAT exact binary —
// directly on POSIX (`{ cmd: pnpmPath, … }`) and as that quoted resolved `.cmd` path under `cmd.exe /d /s /c`
// on Windows — so the probe and the launch are the SAME binary, never a bare `pnpm` re-searched at launch (on
// Windows a bare token would let cmd.exe search the CWD first — a CWD-tool-source hazard). Probing a local-only
// shim the launcher never invokes would treat pnpm as "positively resolved" while the PATH launch fails →
// setup-fail, the probe contradicting the launch. pnpm is conventionally a global/PATH tool; resolving it via
// PATH keeps the probe and the launch consistent. (buf/oxfmt differ: the Rust freshness test prefers their
// version-locked `node_modules/.bin` shim and runs THAT directly, so their resolution legitimately checks
// node_modules first — pnpm is the install DRIVER, not a freshness shim.) Uses the SAME injected
// `env`/`isFileFn`/`windows` so the self-test can drive a fake PATH with `windows:true`. Returns the resolved
// path, or null when pnpm is not on PATH (i.e. not resolvable the way the launcher runs it). (No `repoRoot`
// param: pnpm is resolved via PATH ONLY — it never consults `node_modules/.bin`, so a workspace root would be
// unused.)
export function resolvePnpm(env, isFileFn = defaultIsFile, windows = IS_WINDOWS) {
  return resolvePathShim(PNPM_TOOL, env, isFileFn, windows);
}

// ----------------------------------------------------------------------------------------------------
// Freshness-tooling preflight — the verdict-gating authority. Run AFTER the gate mutex is held and BEFORE
// the archive build. It decides whether the typeinfo freshness-test tolerance is ALLOWED for this run.
//
// BUF-ABSENCE-ONLY tolerance (the codex ruling). The byte-pin tolerance is allowed ONLY when `buf` is NOT
// resolvable — exactly the condition under which the Rust freshness test SKIPS. `regenerate_gen_tree_via_buf`
// (crates/verter_protocol/tests/cases/typeinfo_proto_ts_freshness.rs) early-returns `None` when
// `locate_buf_binary(root)?` cannot find `buf`; with `buf` PRESENT it runs `buf generate` + the byte-compare.
// `oxfmt` is CONDITIONAL in that test (`if let Some(oxfmt_bin) = locate_oxfmt_binary(root)`) — a MISSING
// `oxfmt` only skips the format step, so the test STILL byte-compares and CAN FAIL. Therefore `oxfmt` is a
// REQUIRED-WHEN-buf-PRESENT canonical tool: a missing `oxfmt` with `buf` available is a LOUD `setup-fail`,
// never `tolerate` and never a degraded run (it would otherwise run an un-oxfmt'd byte-compare that can
// false-positive). The skip-determining tool is `BUF_TOOL` — the tolerance gate keys on `buf` SPECIFICALLY,
// resolved local + PATH, NOT on "both tools".
//
// POSITIVE-PNPM-PROBE model (preserved). Whether to ATTEMPT the install is decided by a POSITIVE,
// platform-aware `pnpm` resolver fact determined BEFORE the install — `pnpm` is resolved via PATH (with the
// WINDOWS_SHIM_EXTS suffix set on Windows; via `resolvePnpm`). The RESOLVED path is then the exact binary
// `pnpmInstallCommand` launches (POSIX: directly; Windows: that resolved `.cmd` path quoted under
// `cmd.exe /d /s /c`) — a local `node_modules/.bin/pnpm` shim is NOT the launch target, and the resolved
// path (not a bare `pnpm` token) removes any launch-time re-search. Absence is NOT inferred from the
// install's `spawnError`. This is load-bearing on Windows: the production install is wrapped as
// `cmd.exe /d /s /c "<resolved-pnpm>" install …` (Node's `spawn(shell:false)` cannot launch a `.cmd` shim
// directly), so the install's `spawnError` means "cmd.exe failed to spawn", NOT "pnpm absent".
// After a POSITIVE pnpm probe, `spawnError` from the install is NO LONGER a tolerance signal — it means a
// launcher / race / permission / cmd failure, i.e. `setup-fail`.
//
// Branch table (the contract):
//   * BOTH local `node_modules/.bin` shims present up front → tolerance DISABLED (action "already-present";
//     `runInstall` is NOT called, pnpm is NOT probed) — the tools exist, so a freshness FAIL is a real
//     regression.
//   * a local shim is missing → resolve `pnpm` via PATH (platform-aware) as a POSITIVE fact (PATH-only —
//     the resolved path is what the launcher runs; a local `node_modules/.bin/pnpm` shim does NOT count):
//       - pnpm NOT resolvable: re-resolve `buf` and `oxfmt` SEPARATELY (local + PATH — they may be on PATH
//         without a local install). The install is NOT run (`runInstall` is NOT called):
//           · `buf` NOT resolvable                  → tolerance ENABLED (action "tolerate-genuinely-absent");
//             the Rust test would SKIP, so the exact freshness pair is tolerated — REGARDLESS of `oxfmt`.
//           · `buf` resolvable + `oxfmt` resolvable → tolerance DISABLED (action "path-fallback").
//           · `buf` resolvable + `oxfmt` MISSING    → action "setup-fail" (tolerance OFF). The Rust test would
//             RUN but not canonically format — FAIL LOUD (ensure `oxfmt`), do NOT tolerate, do NOT run a
//             degraded un-oxfmt'd byte-compare.
//       - pnpm IS resolvable: run the injected `runInstall()` (production: the platform-aware
//         `pnpmInstallCommand` inside the contained-step / timeout / stall machinery). Strict PRECEDENCE
//         (a later branch never overrides an earlier one):
//           · watchdog reason                       → action "watchdog" (PROPAGATED via `mapStepReason`,
//             never tolerated).
//           · install spawnError                    → action "setup-fail" (tolerance OFF). After a positive
//             pnpm probe, spawnError is a launcher/race/permission/cmd failure, NOT proven pnpm absence.
//           · install LAUNCHED, exited NON-ZERO     → action "setup-fail" (tolerance OFF) — a deterministic
//             install failure (canonical case: a frozen-lockfile mismatch), checked REGARDLESS of whether
//             buf/oxfmt are independently resolvable on PATH (silently proceeding on a PATH fallback past a
//             frozen-lockfile mismatch is the bypass the codex ruling forbids).
//           · install exited ZERO → RE-RESOLVE `buf`/`oxfmt` local + PATH:
//               - both now resolvable               → tolerance DISABLED (action "installed" | "path-fallback").
//               - `buf` MISSING                     → action "setup-fail" (an exit-0 install that did not
//                 produce `buf` is a deterministic setup failure).
//               - `buf` present + `oxfmt` MISSING   → action "setup-fail" (an exit-0 install that produced
//                 `buf` but not `oxfmt` is a deterministic setup failure — `oxfmt` is required when buf runs).
//
// `runInstall` is injected (the self-test substitutes a fake; production passes a closure that calls
// `runContainedStep` with the platform-aware `pnpmInstallCommand`). It returns the `runContainedStep` shape
// ({ code, reason, spawnError, ... }). The is-file presence predicate (the `existsSyncFn` opts key — an
// IS-FILE check, default `defaultIsFile`), `env`, and `windows` are injected so the self-test can drive a
// fake PATH/fs with `windows:true`; the pnpm/buf/oxfmt probes use the SAME injected `env`/`isFileFn`/`windows`.
// Returns `{ freshnessToleranceAllowed, action, detail, installRes }`.
// ----------------------------------------------------------------------------------------------------
export async function preflightFreshnessTooling(opts) {
  const {
    repoRoot,
    env = process.env,
    runInstall,
    // The shim PRESENCE predicate (default `defaultIsFile`, mirroring the Rust `Path::is_file()`). The
    // self-test injects a fake via `existsSyncFn`; that injected predicate is treated as an IS-FILE check
    // (a fake that returns true only for the intended FILE paths resolves identically — only a directory
    // at a shim path now fails to count as present, matching Rust).
    existsSyncFn: isFileFn = defaultIsFile,
    windows = IS_WINDOWS,
  } = opts;

  const resolveAll = () =>
    FRESHNESS_TOOLS.map((t) => ({
      tool: t,
      local: resolveLocalBinShim(repoRoot, t, isFileFn, windows),
      path: resolvePathShim(t, env, isFileFn, windows),
    }));
  // Resolve a single tool local-then-PATH (the same precedence the Rust `locate_*_binary` helpers use). The
  // tolerance gate consults `buf` through this; `oxfmt` is a required-when-buf-present tool consulted the
  // same way.
  const resolveTool = (t) =>
    resolveLocalBinShim(repoRoot, t, isFileFn, windows) ||
    resolvePathShim(t, env, isFileFn, windows);

  // Step 1–2: both LOCAL shims present up front ⇒ tolerance disabled, no install.
  const beforeLocal = FRESHNESS_TOOLS.map((t) =>
    resolveLocalBinShim(repoRoot, t, isFileFn, windows),
  );
  if (beforeLocal.every((p) => p)) {
    return {
      freshnessToleranceAllowed: false,
      action: "already-present",
      detail: `both freshness tools resolved in node_modules/.bin (${beforeLocal.join(", ")})`,
      installRes: null,
    };
  }

  const missingBefore = FRESHNESS_TOOLS.filter(
    (t) => !resolveLocalBinShim(repoRoot, t, isFileFn, windows),
  );

  // Step 3 (POSITIVE pnpm probe — gates whether to ATTEMPT the install). Before attempting any install,
  // resolve `pnpm` as a POSITIVE, platform-aware RESOLVER fact via PATH (with the WINDOWS_SHIM_EXTS suffix set
  // on Windows) — PATH-only. The resolved path is the exact binary `pnpmInstallCommand` then launches (POSIX:
  // directly; Windows: that resolved `.cmd` path quoted under `cmd.exe /d /s /c`); a local
  // `node_modules/.bin/pnpm` shim the launcher never invokes does NOT count. "pnpm not resolvable" is THIS
  // probe failing — NOT the install's spawnError (which on Windows is a cmd.exe-wrapper failure, not
  // pnpm-absence).
  const pnpmPath = resolvePnpm(env, isFileFn, windows);

  if (!pnpmPath) {
    // pnpm is genuinely unresolvable ⇒ the install CANNOT run. Decide on `buf` SPECIFICALLY (the
    // skip-determining tool): the Rust byte-pin test SKIPS iff `buf` is unavailable, so tolerance is allowed
    // ONLY when `buf` is not resolvable — REGARDLESS of `oxfmt`. With `buf` present, `oxfmt` is required:
    // missing `oxfmt` is a LOUD setup-fail (a degraded un-oxfmt'd byte-compare can false-positive), never
    // tolerate. Do NOT call `runInstall`. Resolve both tools local + PATH.
    const bufResolved = resolveTool(BUF_TOOL);
    const oxfmtResolved = resolveTool("oxfmt");
    if (!bufResolved) {
      return {
        freshnessToleranceAllowed: true,
        action: "tolerate-genuinely-absent",
        detail:
          `pnpm is not resolvable (not on PATH — the way \`pnpmInstallCommand\` launches it) and \`buf\` is not resolvable — ` +
          `exactly the condition under which the Rust byte-pin test SKIPS. Tolerating the exact freshness ` +
          `pair (the install was NOT attempted; \`oxfmt\` resolvability is irrelevant when \`buf\` is absent).`,
        installRes: null,
      };
    }
    if (oxfmtResolved) {
      return {
        freshnessToleranceAllowed: false,
        action: "path-fallback",
        detail:
          `pnpm is not resolvable (not on PATH — the way \`pnpmInstallCommand\` launches it) but both freshness tools resolve on ` +
          `PATH: buf=${bufResolved}, oxfmt=${oxfmtResolved} — tolerance DISABLED (the tools exist, so a ` +
          `freshness FAIL is a real regression).`,
        installRes: null,
      };
    }
    return {
      freshnessToleranceAllowed: false,
      action: "setup-fail",
      detail:
        `pnpm is not resolvable but \`buf\` IS resolvable (${bufResolved}) while \`oxfmt\` is MISSING — the ` +
        `Rust byte-pin test would RUN but the regenerated bindings would not be canonically formatted, a ` +
        `degraded un-oxfmt'd byte-compare that can false-positive. FAILING LOUD as setup; \`oxfmt\` is a ` +
        `required canonical-run tool. Ensure \`oxfmt\` (run \`pnpm install --frozen-lockfile\` from the repo ` +
        `root) and re-run the gate. Tolerance is NEVER granted on \`oxfmt\` absence.`,
      installRes: null,
    };
  }

  // Step 4: pnpm IS resolvable ⇒ attempt the injected install (production runs the platform-aware
  // `pnpmInstallCommand` inside the contained-step machinery). Pass the RESOLVED `pnpmPath` so the
  // launcher runs THAT exact binary (no bare-`pnpm` re-search, no Windows CWD search) — it is the single
  // source of truth for both the probe and the launch.
  const installRes = await runInstall({ pnpmPath });

  // A watchdog kill (memory/timeout/stall) from the install step is PROPAGATED, never tolerated.
  if (installRes && installRes.reason) {
    return {
      freshnessToleranceAllowed: false,
      action: "watchdog",
      detail: `pnpm install ${installRes.reason} while ensuring freshness tooling`,
      installRes,
    };
  }

  // A spawnError now means a LAUNCHER / race / permission / cmd failure — NOT proven pnpm absence (pnpm was
  // POSITIVELY resolved in step 3, so spawnError is no longer a tolerance signal). FAIL LOUD as setup.
  if (installRes && installRes.spawnError) {
    return {
      freshnessToleranceAllowed: false,
      action: "setup-fail",
      detail:
        `pnpm install FAILED to launch (spawnError) even though pnpm was resolved at ${pnpmPath} — a ` +
        `launcher / race / permission / cmd failure, NOT proven pnpm absence (after a positive pnpm probe ` +
        `spawnError is no longer a tolerance signal). FAILING LOUD as setup. Re-run the gate.`,
      installRes,
    };
  }

  // A pnpm that LAUNCHED and exited NON-ZERO is a deterministic install FAILURE — the canonical case is a
  // frozen-lockfile mismatch. This takes PRECEDENCE over the resolve-based branch below: it FAILS LOUD as
  // setup (the caller maps action "setup-fail" → EXIT_USAGE 127) REGARDLESS of whether `buf`/`oxfmt` happen
  // to be resolvable on PATH afterward. A launched non-zero exit cannot be reliably told apart from a
  // deterministic breakage, and silently proceeding because the tools are INDEPENDENTLY on PATH would let a
  // frozen-lockfile mismatch slip past the gate — exactly the bypass the codex ruling forbids.
  if (installRes && installRes.code !== 0) {
    return {
      freshnessToleranceAllowed: false,
      action: "setup-fail",
      detail:
        `pnpm install LAUNCHED but exited non-zero (exit ${installRes.code}) while ensuring freshness ` +
        `tooling — a deterministic install failure (e.g. a frozen-lockfile mismatch). FAILING LOUD as ` +
        `setup; the freshness tolerance is NEVER granted on a launched non-zero install (even if buf/oxfmt ` +
        `are resolvable on PATH). Run \`pnpm install --frozen-lockfile\` from the repo root and re-run the gate.`,
      installRes,
    };
  }

  // Step 5: the install LAUNCHED and exited ZERO — re-resolve LOCAL + PATH after the install attempt. An
  // exit-0 install that did not produce `buf` (or produced `buf` but not the required `oxfmt`) is a
  // deterministic setup failure — NEVER tolerated (an exit-0 install is not a genuinely-tooling-less runner).
  const after = resolveAll();
  const allResolved = after.every((r) => r.local || r.path);
  const usedPathFallback = allResolved && after.some((r) => !r.local && r.path);

  if (allResolved) {
    return {
      freshnessToleranceAllowed: false,
      action: usedPathFallback ? "path-fallback" : "installed",
      detail:
        `freshness tooling available after install` +
        (usedPathFallback ? " (one or both via PATH fallback)" : "") +
        `: ${after.map((r) => `${r.tool}=${r.local || r.path}`).join(", ")}`,
      installRes,
    };
  }

  // pnpm launched, exit-0, but a tool is still missing ⇒ a deterministic setup failure. FAIL LOUD; never
  // PASS-WITH-TOLERATED. Distinguish `buf` missing (the test would skip if pnpm-less, but an exit-0 install
  // is not that case) from `oxfmt` missing (the required canonical formatter) for a precise message.
  const stillMissing = after.filter((r) => !r.local && !r.path).map((r) => r.tool);
  return {
    freshnessToleranceAllowed: false,
    action: "setup-fail",
    detail:
      `pnpm install ran (exit ${installRes ? installRes.code : "?"}) but the freshness tools are still ` +
      `missing (${stillMissing.join(", ")}) — a deterministic setup failure (e.g. frozen-lockfile ` +
      `mismatch). An exit-0 install that did not produce ${stillMissing.includes(BUF_TOOL) ? "`buf`" : "the required `oxfmt`"} is a setup ` +
      `failure, never tolerated. Run \`pnpm install --frozen-lockfile\` from the repo root and re-run the gate.`,
    installRes,
  };
}

// ----------------------------------------------------------------------------------------------------
// BUILD-PREREQUISITE PREFLIGHT — the FIRST thing the gate does, before the freshness preflight, before
// the archive build, before a single test runs.
//
// WHY IT EXISTS. Parts of the Rust suite load artifacts that CARGO DOES NOT BUILD: the real-provider
// suites spawn the pinned `tsserver` with `--globalPlugins @verter/typescript-plugin
// --pluginProbeLocations <repo>/packages/vue-vscode/node_modules`, and that probe dir is a pnpm symlink
// to `packages/typescript-plugin`, whose `main` is `dist/index.js` — a `tsc -b` OUTPUT. `pnpm install`
// creates the symlink but NOT the `dist`. In that state tsserver silently loads no plugin, cannot resolve
// `.vue`/`.svelte` carriers, and ~64 `*_tsserver` tests fail with `TS2307: Cannot find module
// './Comp.vue' or its corresponding type declarations.` — sixty-four opaque failures that read exactly
// like a compiler regression and cost a full investigation to trace back to one missing build step.
//
// That is the failure class CLAUDE.md's "Verification Must Prove Execution (MANDATORY)" names directly:
// a gate must prove "required source, build, and fixture prerequisites matched the tested tree" and that
// "unexpected prerequisite skips were zero". A gate that cannot tell "the code is broken" from "an
// artifact was never built" fails that rule. So the gate FAILS CLOSED here and names what is wrong.
//
// THE ORACLE IS A REAL LOAD, NOT A FILE LIST. The check LOADS the plugin entry the way tsserver does —
// `require()` of the probe directory in a child process — and treats any load failure as the refusal.
// This is deliberate. A list of `index.js` paths to `stat` is a MIRROR OF THE EMIT GRAPH, and it drifts:
// the plugin entry eagerly requires its emitted helpers (`dist/helpers/carrierStore.js` and friends) and
// `@verter/language-shared`'s entry eagerly re-exports a dozen emitted siblings, so a tree with both
// `index.js` files present and ONE helper missing satisfies every stat and still throws inside tsserver —
// exactly the condition this preflight exists to prevent. Loading proves the transitive closure actually
// RESOLVES, costs one process spawn, and cannot fall out of step with what `tsc` emits.
//
// WHAT IT DOES NOT PROVE: freshness. A dist that loads but was emitted from an older commit passes here.
// That is a DIFFERENT problem (a stale-but-loadable artifact) and is deliberately out of scope for this
// check — it is not an oversight. The check answers exactly one question: can the plugin tsserver is
// about to load actually be loaded?
//
// IT DOES NOT BUILD FOR YOU, and it does not skip the affected tests. Building implicitly would make the
// gate's verdict depend on a mutation it performed itself; skipping would reintroduce the silent pass
// (with NO install at all the affected tests SKIP and the gate goes green while proving nothing — the
// "unexpected prerequisite skips" half of the rule). The only correct outcome is a loud refusal.
//
// WHY IT RUNS BEFORE THE FRESHNESS PREFLIGHT. The freshness preflight may run `pnpm install
// --frozen-lockfile`, which is precisely what turns the SILENT-SKIP state (no node_modules ⇒ tsserver not
// found ⇒ tests skip ⇒ false green) into the LOUD-FAILURE state (tsserver found, plugin dist absent ⇒ 64
// failures). Checking first catches both states with one message, before any install and before any cargo.
// It is deliberately NOT applied to `--prepare`, which builds the archive and runs no test.
//
// SCOPE OF THE PRODUCER COMMAND. Two workspace packages produce the closure the load walks: the plugin
// itself, and `@verter/language-shared`, which its entry requires at load time. `@verter/native` is
// deliberately NOT among them — the plugin's tsconfig is `"files": ["src/index.ts"]`, so `src/tsc/`, its
// only consumer, is not in the built plugin, and no Rust test loads a `.node`. Requiring it would drag a
// full `napi build --release` into the gate's prerequisites.
// ----------------------------------------------------------------------------------------------------

// The stable marker every build-prerequisite refusal carries. Operators and the self-test key on it to
// tell this refusal apart from every other exit-127 setup failure the gate can emit.
export const BUILD_PREREQUISITE_MARKER = "BUILD-PREREQUISITE MISSING";

// The ONE command that produces the closure, in dependency order. `pnpm` runs a multi-filter recursive
// script topologically, so `@verter/language-shared` builds before `@verter/typescript-plugin` (which
// type-checks against its emitted `.d.ts`). NOT `pnpm build` (native + LSP + wasm + every TS package) and
// NOT `--filter @verter/typescript-plugin...`: the trailing ellipsis selects the package AND ITS
// DEPENDENCIES, which pulls in `@verter/native` and its `napi build --release`.
export const BUILD_PREREQUISITE_COMMAND =
  "pnpm --filter @verter/language-shared --filter @verter/typescript-plugin build";

// The workspace packages whose `build` the command above runs. DOCUMENTATION for the refusal message —
// NOT the oracle, and deliberately not a file list: the oracle is the load probe, which walks whatever
// `tsc` actually emitted. Adding an entry here changes the message, never the verdict.
export const BUILD_PREREQUISITE_PACKAGES = [
  {
    id: "@verter/language-shared",
    why: "its entry is `require`d by the plugin entry at load time (and re-exports a dozen emitted siblings)",
  },
  {
    id: "@verter/typescript-plugin",
    why: "its `dist/index.js` is the plugin entry tsserver loads (and it eagerly requires its emitted helpers)",
  },
];

// The path the probe loads: the EXACT `--pluginProbeLocations` directory the real-provider harness passes
// to tsserver (`crates/verter_lsp/src/test_harness.rs`), joined with the plugin's package name. Node
// resolves a directory path through its `package.json` `main`, which is the same `dist/index.js` tsserver
// ends up executing — so the probe walks the real chain: probe dir → package manifest → emitted entry →
// emitted helpers → `@verter/language-shared` → its emitted siblings.
export const BUILD_PREREQUISITE_PROBE_SEGMENTS = [
  "packages",
  "vue-vscode",
  "node_modules",
  "@verter",
  "typescript-plugin",
];

// ----------------------------------------------------------------------------------------------------
// PROBE ENVIRONMENT EQUIVALENCE. The probe's claim is "the plugin tsserver is about to load can be
// loaded", so it MUST run under the same Node environment tsserver does. It does not by default:
// `TsserverTypeProvider::spawn` REMOVES a denylist of Node/Electron env vars before launching node
// (`crates/verter_type_runtime/src/tsserver/ipc.rs`, `CHILD_PROCESS_ENV_DENYLIST`), and an inheriting
// probe therefore runs with strictly MORE influence than the process it speaks for.
//
// That gap is exploitable, not theoretical. Measured: with the entry requiring a helper that does not
// exist, `NODE_OPTIONS=--require=<preload>` where the preload patches `Module._load` to return a dummy for
// `process.argv[1]` makes the probe exit 0 and report `loaded: true`, while tsserver still fails on the
// missing helper — the exact false positive this probe replaced the stat oracle to prevent, reached
// through the environment instead of the filesystem.
//
// The denylist is READ FROM THE RUST CALL SITE rather than restated here, so the two cannot drift. A
// committed generated mirror was the alternative and is rejected for a bootstrap reason: its freshness
// test lives in the Rust suite, which this probe runs BEFORE, so a stale mirror would be exactly the
// silent drift window the mirror was meant to close. If the const cannot be found or parsed the probe
// FAILS CLOSED — without knowing tsserver's sanitization we cannot claim equivalence, and guessing is how
// the gap reappears.
//
// It strips EXACTLY that denylist and nothing more. Equivalence is the goal, not maximal hardening: a var
// tsserver also inherits (`NODE_PATH`, say) influences the real load identically, so stripping it here
// would make the probe stricter than the thing it models and could refuse a tree tsserver handles fine.
// ----------------------------------------------------------------------------------------------------
export const TSSERVER_ENV_DENYLIST_SOURCE_SEGMENTS = [
  "crates",
  "verter_type_runtime",
  "src",
  "tsserver",
  "ipc.rs",
];
export const TSSERVER_ENV_DENYLIST_CONST_NAME = "CHILD_PROCESS_ENV_DENYLIST";

// Extract the denylisted env-var names from the Rust source. Returns the names, or `null` when the
// declaration cannot be located or yields nothing — the caller treats `null` as fail-closed, never as
// "nothing to strip".
// DECLARATION-BOUNDED on purpose. The previous version scanned for the bare NAME and then took the next
// `[`…`]`, so a COMMENTED-OUT `// const CHILD_PROCESS_ENV_DENYLIST: &[&str] = &["UNRELATED"];` earlier in
// the file parsed as `["UNRELATED"]` — a plausible list, silently wrong, and precisely the drift this
// reads the live Rust const to avoid. Reading the source is only safe if a stale or dead mention CANNOT
// win: comments are stripped first, and the match then requires the real declaration SHAPE
// (`const NAME: &[&str] = &[ … ]`), so anything else fails closed to `null`.
export function parseTsserverEnvDenylist(rustSource) {
  if (typeof rustSource !== "string") return null;
  const withoutComments = rustSource
    .replace(/\/\*[\s\S]*?\*\//g, "")
    .split("\n")
    .map((line) => {
      const lineComment = line.indexOf("//");
      return lineComment === -1 ? line : line.slice(0, lineComment);
    })
    .join("\n");
  const declaration = new RegExp(
    `\\bconst\\s+${TSSERVER_ENV_DENYLIST_CONST_NAME}\\s*:\\s*&\\s*\\[\\s*&\\s*str\\s*\\]\\s*=\\s*&\\s*\\[([^\\]]*)\\]`,
  ).exec(withoutComments);
  if (!declaration) return null;
  const names = [...declaration[1].matchAll(/"([A-Za-z_][A-Za-z0-9_]*)"/g)].map((m) => m[1]);
  return names.length > 0 ? names : null;
}

// Build the child environment the probe must run under: the caller's env MINUS the denylist the tsserver
// launcher strips. Returns `{ env, denylist, source }` or `{ error }` (fail-closed). On Windows the delete
// folds case, mirroring both `Command::env_remove` and this gate's own `buildCargoEnv`; on POSIX it stays
// case-exact, because a differently-cased name there is a different variable node never reads — and
// deleting it would make the probe diverge from the process it models.
export function resolveProbeChildEnv(opts) {
  const {
    repoRoot,
    env = process.env,
    readFileFn = readFileSync,
    joinFn = join,
    windows = IS_WINDOWS,
  } = opts;
  const source = joinFn(repoRoot, ...TSSERVER_ENV_DENYLIST_SOURCE_SEGMENTS);
  let rustSource;
  try {
    rustSource = readFileFn(source, "utf8");
  } catch (error) {
    return {
      error:
        `cannot read the tsserver launcher at ${source} (${error && error.message}), so the environment ` +
        "tsserver runs under is unknown and the probe cannot claim equivalence",
    };
  }
  const denylist = parseTsserverEnvDenylist(rustSource);
  if (!denylist) {
    return {
      error:
        `could not parse \`${TSSERVER_ENV_DENYLIST_CONST_NAME}\` out of ${source}, so the environment ` +
        "tsserver runs under is unknown and the probe cannot claim equivalence (did the const move or " +
        "change shape?)",
    };
  }
  const childEnv = { ...env };
  for (const name of denylist) {
    if (windows) {
      const wanted = name.toUpperCase();
      for (const key of Object.keys(childEnv)) {
        if (key.toUpperCase() === wanted) delete childEnv[key];
      }
    } else {
      delete childEnv[name];
    }
  }
  return { env: childEnv, denylist, source };
}

// The child-process probe body. Kept as a single-expression `-e` script (no temp file, no shell) that
// reports a load failure as STRUCTURED JSON on stdout plus a distinctive exit code, so the parent never
// has to scrape a Node stack trace. `process.argv[1]` under `-e` is the first extra argument — the
// absolute target path — so nothing is interpolated into the source and no quoting can go wrong.
export const BUILD_PREREQUISITE_PROBE_SOURCE =
  "try { require(process.argv[1]); } catch (e) { " +
  "process.stdout.write(JSON.stringify({ message: String(e && e.message || e), code: e && e.code, " +
  "requireStack: (e && e.requireStack) || [] })); process.exit(3); }";

// The signal the probe's timeout kills with. MUST be unignorable. `spawnSync`'s default `killSignal` is
// SIGTERM, which a child can trap — and then `timeout` is not a bound at all: the parent stays BLOCKED
// until the child chooses to exit, and if it exits 0 `spawnSync` reports status 0 and the probe would
// answer `loaded: true`. Measured with a child doing `process.on("SIGTERM", () => {})` plus an open
// handle: under the default the parent blocked for the child's FULL 25s lifetime and then read status 0
// (a hang AND a false positive); with SIGKILL it returned in ~700ms with `ETIMEDOUT`.
//
// That matters here more than the milliseconds suggest: this probe is the gate's FIRST step and it runs
// with the single-flight mutex HELD, so an unbounded block does not stall one run — it holds the lock,
// the stale-heavy-gate-lock hazard already tracked as GI-12 in docs/arch/gate-integrity-ledger.md.
//
// SIGKILL with NO graceful phase is deliberate, not a shortcut. The child's entire job is one
// `require()`: it owns no transaction, buffers nothing a reader depends on, and has no cleanup that a
// SIGTERM grace window would let it finish — so an escalation would add a tunable delay and a second
// failure mode while buying nothing. (Contrast `runContainedStep`, which DOES escalate: it reaps whole
// cargo/rustc/test process TREES that legitimately need a chance to flush.) Honest limit: this kills the
// direct child only. A module that spawns a detached grandchild on require would leak it — the same
// documented limitation the contained-step runner carries, and out of proportion to fix here.
export const BUILD_PREREQUISITE_PROBE_KILL_SIGNAL = "SIGKILL";

// Upper bound on the probe. The EFFECTIVE budget is the SMALLER of this cap and the gate's own remaining
// wallclock (see `probeBudgetMs`): an independent constant here could outlive the `--timeout` deadline the
// probe sits inside, which is not a bound at all — it is a second, longer deadline nobody asked for.
export const BUILD_PREREQUISITE_PROBE_MAX_MS = 60_000;

// The probe budget for a gate whose deadline is `deadlineMs` at wallclock `nowMsValue`: the remaining
// time, capped at MAX. It MAY be zero or negative, and there is deliberately NO FLOOR.
//
// A floor was here and was wrong. It made `probeBudgetMs(0, 10_000)` return 2000ms, so an expired deadline
// (or `--timeout 0s`) bought the probe two seconds of holding the SINGLE-FLIGHT MUTEX past the gate's own
// wallclock limit. The mutex is what is at stake, so a bounded overshoot is still an overshoot: with no
// time remaining the correct answer is to refuse IMMEDIATELY, which `runBuildPrerequisiteLoadProbe` does
// without spawning at all.
//
// Refusing to spawn on a non-positive budget is load-bearing for a second, sharper reason: Node applies
// `spawnSync`'s timeout only when it is `> 0`, so passing `0` or a negative value would SILENTLY DISABLE
// the timeout — turning an expired deadline into an UNBOUNDED probe, the exact inverse of the intent.
export function probeBudgetMs(deadlineMs, nowMsValue) {
  const remaining = deadlineMs - nowMsValue;
  if (!Number.isFinite(remaining)) return BUILD_PREREQUISITE_PROBE_MAX_MS;
  return Math.min(BUILD_PREREQUISITE_PROBE_MAX_MS, remaining);
}

// Run the load probe. Returns `{ loaded, detail }`. FAIL-CLOSED on every non-success shape: a structured
// load failure (exit 3), a spawn error, a TIMEOUT, a crash, or ANY other non-zero exit all report
// `loaded: false` with whatever diagnostic is available — the gate must never read "the probe itself did
// not work" as "the prerequisite is present". `spawnFn` and `nodePath` are injected so the self-test can
// drive every one of those shapes without a real subprocess.
export function runBuildPrerequisiteLoadProbe(opts) {
  const {
    repoRoot,
    nodePath = process.execPath,
    spawnFn = spawnSync,
    joinFn = join,
    env = process.env,
    readFileFn = readFileSync,
    windows = IS_WINDOWS,
    timeoutMs = BUILD_PREREQUISITE_PROBE_MAX_MS,
  } = opts;
  const target = joinFn(repoRoot, ...BUILD_PREREQUISITE_PROBE_SEGMENTS);
  // NO TIME, NO SPAWN. A non-positive budget means the gate's own deadline is spent; launching here would
  // hold the single-flight mutex past it. It would ALSO be unbounded: Node applies `spawnSync`'s timeout
  // only when it is `> 0`, so a `0`/negative value silently disables it. Refuse immediately instead.
  if (!(timeoutMs > 0)) {
    return {
      target,
      loaded: false,
      reason: "timeout",
      detail:
        `no gate wallclock remained for the probe (budget ${timeoutMs}ms) — refusing to launch it rather ` +
        "than hold the single-flight mutex past the gate deadline",
    };
  }
  // Equivalence first: without knowing what the tsserver launcher strips, a "loaded" answer is not about
  // the same environment tsserver runs in and must not be given.
  const childEnv = resolveProbeChildEnv({ repoRoot, env, readFileFn, joinFn, windows });
  if (childEnv.error) {
    return { target, loaded: false, reason: "environment-unknown", detail: childEnv.error };
  }
  let res;
  try {
    res = spawnFn(nodePath, ["-e", BUILD_PREREQUISITE_PROBE_SOURCE, target], {
      encoding: "utf8",
      env: childEnv.env,
      timeout: timeoutMs,
      killSignal: BUILD_PREREQUISITE_PROBE_KILL_SIGNAL,
      windowsHide: true,
    });
  } catch (error) {
    return {
      target,
      loaded: false,
      reason: "spawn-error",
      detail: `probe could not be spawned: ${error && error.message}`,
    };
  }
  if (!res) {
    return { target, loaded: false, reason: "spawn-error", detail: "probe returned no result" };
  }
  // TIMEOUT FIRST. On a timeout Node sets BOTH `error` (code `ETIMEDOUT`) AND `signal` (the killSignal),
  // so an `error`-before-timeout ordering reports a real timeout as "could not be spawned: … ETIMEDOUT" —
  // fail-closed but pointing at the wrong cause, which on the gate's first step is how someone spends an
  // hour on the wrong thing.
  if (res.error && res.error.code === "ETIMEDOUT") {
    return {
      target,
      loaded: false,
      reason: "timeout",
      detail:
        `probe TIMED OUT after ${timeoutMs}ms and was killed with ` +
        `${BUILD_PREREQUISITE_PROBE_KILL_SIGNAL}${res.signal ? ` (signal ${res.signal})` : ""} — the ` +
        "plugin entry did not finish loading, or it left the probe process alive",
    };
  }
  if (res.error) {
    return {
      target,
      loaded: false,
      reason: "spawn-error",
      detail: `probe could not be spawned: ${res.error.message}`,
    };
  }
  if (res.signal) {
    return {
      target,
      loaded: false,
      reason: "signalled",
      detail: `probe was killed by signal ${res.signal}`,
    };
  }
  if (res.status === 0) return { target, loaded: true, reason: "loaded", detail: "" };
  if (res.status === 3) {
    let parsed = null;
    try {
      parsed = JSON.parse(res.stdout || "");
    } catch {
      /* fall through to the raw shape below */
    }
    if (parsed && parsed.message) {
      const stack =
        Array.isArray(parsed.requireStack) && parsed.requireStack.length > 0
          ? `\n  require stack: ${parsed.requireStack.join(" <- ")}`
          : "";
      // MODULE_NOT_FOUND is the ARTIFACT-MISSING class specifically — the only failure a caller may treat
      // as "this tree was never built". Every other load error is the plugin failing for its own reasons
      // and must NOT be read as a missing build.
      return {
        target,
        loaded: false,
        reason: parsed.code === "MODULE_NOT_FOUND" ? "module-not-found" : "load-error",
        detail: `${parsed.code ? `${parsed.code}: ` : ""}${parsed.message}${stack}`,
      };
    }
  }
  const stderr = (res.stderr || "").trim().split("\n").slice(0, 8).join("\n");
  return {
    target,
    loaded: false,
    reason: "unknown-exit",
    detail: `probe exited ${res.status}${stderr ? `\n${stderr}` : ""}`,
  };
}

// The preflight itself. Returns `{ ok, target, reason, detail, lines }` — `lines` is the operator-facing
// report, already naming what failed to load, the packages that produce it, and the exact producer
// command. `reason` is the TYPED failure class (see `runBuildPrerequisiteLoadProbe`); callers that need to
// distinguish "this tree was never built" from "the probe could not answer" read it instead of matching on
// `detail`, so an infrastructure failure can never be mistaken for a missing artifact.
//
// `timeoutMs` is threaded through so the caller can bound the probe by ITS OWN deadline rather than an
// independent constant — a probe that can outlive the whole-gate deadline it sits inside is not bounded.
// `loadProbe` is injected so the self-test can drive both directions in-process; production passes the
// real `runBuildPrerequisiteLoadProbe`.
export function checkBuildPrerequisites(opts) {
  const { repoRoot, loadProbe = runBuildPrerequisiteLoadProbe, timeoutMs } = opts;
  const probe = loadProbe({ repoRoot, ...(timeoutMs === undefined ? {} : { timeoutMs }) });
  if (probe.loaded) {
    return {
      ok: true,
      target: probe.target,
      reason: probe.reason || "loaded",
      detail: "",
      lines: [],
    };
  }
  const lines = [
    `${BUILD_PREREQUISITE_MARKER}: the tsserver plugin the real-provider suites load could not be ` +
      "loaded from this tree. Running the gate now would report test failures that are really a missing " +
      "build step.",
    `  probe target: ${probe.target}`,
    `  load failure: ${probe.detail}`,
    "  produced by:",
  ];
  for (const pkg of BUILD_PREREQUISITE_PACKAGES) {
    lines.push(`    ${pkg.id} — ${pkg.why}`);
  }
  lines.push(
    "Produce them with (from the repo root, after `pnpm install --frozen-lockfile` — the probe target is " +
      "an install-created directory, so a failure naming it means the install is missing too):",
    `    ${BUILD_PREREQUISITE_COMMAND}`,
    "The gate refuses to build them for you (its verdict must not depend on a mutation it performed) and " +
      "refuses to skip the tests that need them (with no install at all those tests SKIP and the gate " +
      "goes green while proving nothing). This check proves the plugin RESOLVES, not that it is fresh; a " +
      "stale-but-loadable dist is a separate problem and is out of scope here.",
  );
  return { ok: false, target: probe.target, reason: probe.reason, detail: probe.detail, lines };
}

// ----------------------------------------------------------------------------------------------------
// ORACLE-CACHE PREREQUISITE PREFLIGHT (gate mode only; the gate's SECOND step — right after the
// build-prerequisite preflight above, still before Cargo touches anything).
//
// THE BUG IT GUARDS. `verter_session/bf2-authoritative` gates 45 tests that were absent from every
// canonical gate run (see `ARCHIVE_FEATURES` below, which turns the feature on for the archived suite).
// Among them is the ENTIRE `compile::map_equality_tests::svelte_official_conformance_gate` /
// `_matrix` suite — the tests that actually compare Verter's Svelte output against the pinned official
// `svelte@5.56.10` oracle. Once the feature is on, those tests spawn `node bin/check-candidate.mjs`
// (`crates/verter_session/src/compile/map_equality_tests/bf2_full_axis_gate.rs`), which calls
// `ensureOracleDomain(framework)` (`packages/framework-conformance-harness/src/oracle-install.mjs`) to
// realize each oracle (`vue`, `svelte`) OFFLINE from `.oracle-npm-cache` — a GITIGNORED local cache
// `packages/framework-conformance-harness/scripts/provision-oracle-npm-cache.mjs` warms from the network — into `.oracle-installs`. In a fresh
// checkout or worktree the cache does not exist, and `check-candidate.mjs` does NOT fail when
// `ensureOracleDomain` cannot realize an oracle: it records the affected axis as `"authoritative mode:
// link axis skipped (oracle install unavailable: …)"` and keeps comparing every OTHER axis — an
// environment absence masquerading as a compiled-output divergence. Measured on a fresh worktree with the
// cache missing: 5 failures that read exactly like conformance regressions, versus 2 real ones with the
// cache present.
//
// THE ORACLE UNDER TEST IS A REAL REALIZATION, same shape as the build-prerequisite preflight above: not
// a stat of whether `.oracle-npm-cache` exists, but an actual call to the SAME `ensureOracleDomain` the
// suite's own `check-candidate.mjs` calls on every axis of every request — which validates the realized
// `.oracle-installs` tree against the committed lockfile's closure (paths, names, versions, edges) and
// per-package content digests, not merely "a directory is there". A `.oracle-npm-cache` directory that
// exists but holds no matching tarballs (or a torn/corrupted `.oracle-installs`) fails this the same way
// `npm ci --offline` would: loudly, not silently.
//
// THIS IS REALIZATION, NOT PROVISIONING. `ensureOracleDomain` is OFFLINE (the cache is the sole package
// source, `npm ci --offline`) and idempotent — it re-validates and reuses `.oracle-installs` on every
// call, which is exactly what happens automatically the first time a `bf2-authoritative` test runs
// regardless of whether this preflight exists. Running it here only makes the SAME automatic step happen
// loudly, first, and before Cargo, instead of silently inside a test's divergence report. The ONE
// networked step, `packages/framework-conformance-harness/scripts/provision-oracle-npm-cache.mjs`, is never invoked here or anywhere else in the
// gate — an absent or unusable cache fails setup and names that exact command; the gate does not run it.
// ----------------------------------------------------------------------------------------------------
export const ORACLE_CACHE_PREREQUISITE_MARKER = "ORACLE-CACHE PREREQUISITE MISSING";

export const ORACLE_CACHE_PROVISION_COMMAND =
  "node packages/framework-conformance-harness/scripts/provision-oracle-npm-cache.mjs";

// The two oracle domains `bf2-authoritative` tests realize from `.oracle-npm-cache`
// (`packages/framework-conformance-harness/src/oracle-install.mjs`'s `FRAMEWORKS` map).
export const ORACLE_CACHE_FRAMEWORKS = Object.freeze(["vue", "svelte"]);

// The module the probe loads `ensureOracleDomain` from — the SAME function `bin/check-candidate.mjs`
// calls for every axis on every request.
export const ORACLE_CACHE_PROBE_MODULE_SEGMENTS = [
  "packages",
  "framework-conformance-harness",
  "src",
  "oracle-install.mjs",
];

// `node -e` source: dynamic-import the oracle-install module, then call `ensureOracleDomain` for every
// framework named in argv. Exits 0 with `{ ok: true, realized: { <framework>: installDir } }` on success.
// Exits 3 with a structured `{ stage, framework, name, message }` JSON on the FIRST framework that fails
// to realize — `stage` is `"import"` when the module itself could not be loaded, `"realize"` when a
// specific framework's `ensureOracleDomain` call threw. `name` is the thrown error's `.name`:
// `OracleCacheUnprovisionedError` for a missing cache, `PackageDriftError` for a validated-but-drifted
// realized tree, or the generic `Error` an offline `npm ci` itself throws (e.g. `ENOTCACHED`) for a
// present-but-unusable cache — the caller (`checkOracleCachePrerequisite`) distinguishes "absent" from
// "invalid" on this field, never on message text.
export const ORACLE_CACHE_PROBE_SOURCE =
  "const { pathToFileURL } = require('node:url');\n" +
  "const target = process.argv[1];\n" +
  "const frameworks = process.argv.slice(2);\n" +
  "(async () => {\n" +
  "  let mod;\n" +
  "  try {\n" +
  "    mod = await import(pathToFileURL(target).href);\n" +
  "  } catch (e) {\n" +
  "    process.stdout.write(JSON.stringify({ stage: 'import', name: e && e.name, message: String((e && e.message) || e) }));\n" +
  "    process.exit(3);\n" +
  "  }\n" +
  "  const realized = {};\n" +
  "  for (const framework of frameworks) {\n" +
  "    try {\n" +
  "      realized[framework] = mod.ensureOracleDomain(framework).installDir;\n" +
  "    } catch (e) {\n" +
  "      process.stdout.write(JSON.stringify({ stage: 'realize', framework, name: e && e.name, message: String((e && e.message) || e) }));\n" +
  "      process.exit(3);\n" +
  "    }\n" +
  "  }\n" +
  "  process.stdout.write(JSON.stringify({ ok: true, realized }));\n" +
  "  process.exit(0);\n" +
  "})();\n";

// SIGKILL: the child may itself have an `npm ci` grandchild in flight; a trappable SIGTERM (spawnSync's
// default killSignal) can leave the parent blocked until that child chooses to exit — the same hazard
// documented on `BUILD_PREREQUISITE_PROBE_KILL_SIGNAL` above, and for the same reason: this runs as the
// gate's SECOND step, still holding the single-flight mutex.
export const ORACLE_CACHE_PROBE_KILL_SIGNAL = "SIGKILL";

// Upper bound on the probe. Larger than `BUILD_PREREQUISITE_PROBE_MAX_MS`: unlike the tsserver probe (one
// `require()`), a cold cache genuinely spawns TWO offline `npm ci` realizations (`oracle-install.mjs`
// bounds its OWN cross-process realize lock at 5 minutes for the same reason — `REALIZE_LOCK_TIMEOUT_MS`).
// A warm cache is validate-only (no npm spawn) and returns in well under a second — measured ~150ms for
// both frameworks combined on a realized checkout; measured ~1s for a genuinely COLD two-framework
// realize from a freshly-provisioned cache.
export const ORACLE_CACHE_PROBE_MAX_MS = 5 * 60_000;

// The probe budget for a gate whose deadline is `deadlineMs` at wallclock `nowMsValue`: the remaining
// time, capped at MAX. Deliberately no floor — see `probeBudgetMs` above for why a floor would let an
// expired deadline buy the probe time past the gate's own wallclock limit.
export function oracleCacheProbeBudgetMs(deadlineMs, nowMsValue) {
  const remaining = deadlineMs - nowMsValue;
  if (!Number.isFinite(remaining)) return ORACLE_CACHE_PROBE_MAX_MS;
  return Math.min(ORACLE_CACHE_PROBE_MAX_MS, remaining);
}

// Run the load probe. Same fail-closed contract as `runBuildPrerequisiteLoadProbe`: every non-success
// shape (spawn error, signal, timeout, unparseable output) reports `ok: false` — "the probe itself did
// not work" must never read as "the cache is usable". `spawnFn`/`nodePath`/`joinFn`/`env` are injected so
// the self-test can drive every outcome without a real npm/network dependency.
export function runOracleCacheLoadProbe(opts) {
  const {
    repoRoot,
    nodePath = process.execPath,
    spawnFn = spawnSync,
    joinFn = join,
    env = process.env,
    frameworks = ORACLE_CACHE_FRAMEWORKS,
    timeoutMs = ORACLE_CACHE_PROBE_MAX_MS,
  } = opts;
  const target = joinFn(repoRoot, ...ORACLE_CACHE_PROBE_MODULE_SEGMENTS);
  // NO TIME, NO SPAWN — mirrors `runBuildPrerequisiteLoadProbe`'s refusal for the same reason: a
  // non-positive budget means the gate's own deadline is spent, and `spawnSync`'s timeout is only applied
  // when it is `> 0` — a `0`/negative value would silently disable it.
  if (!(timeoutMs > 0)) {
    return {
      target,
      ok: false,
      reason: "timeout",
      detail:
        `no gate wallclock remained for the oracle-cache probe (budget ${timeoutMs}ms) — refusing to ` +
        "launch it rather than hold the single-flight mutex past the gate deadline",
    };
  }
  let res;
  try {
    res = spawnFn(nodePath, ["-e", ORACLE_CACHE_PROBE_SOURCE, target, ...frameworks], {
      encoding: "utf8",
      env,
      timeout: timeoutMs,
      killSignal: ORACLE_CACHE_PROBE_KILL_SIGNAL,
      windowsHide: true,
    });
  } catch (error) {
    return {
      target,
      ok: false,
      reason: "spawn-error",
      detail: `probe could not be spawned: ${error && error.message}`,
    };
  }
  if (!res) {
    return { target, ok: false, reason: "spawn-error", detail: "probe returned no result" };
  }
  // TIMEOUT FIRST — same ordering reason as the tsserver probe: on a timeout Node sets BOTH `error`
  // (`ETIMEDOUT`) AND `signal`, so an `error`-before-timeout check would misreport a real timeout as "could
  // not be spawned".
  if (res.error && res.error.code === "ETIMEDOUT") {
    return {
      target,
      ok: false,
      reason: "timeout",
      detail:
        `probe TIMED OUT after ${timeoutMs}ms and was killed with ` +
        `${ORACLE_CACHE_PROBE_KILL_SIGNAL}${res.signal ? ` (signal ${res.signal})` : ""} — oracle ` +
        "realization did not finish, or it left the probe process alive",
    };
  }
  if (res.error) {
    return {
      target,
      ok: false,
      reason: "spawn-error",
      detail: `probe could not be spawned: ${res.error.message}`,
    };
  }
  if (res.signal) {
    return {
      target,
      ok: false,
      reason: "signalled",
      detail: `probe was killed by signal ${res.signal}`,
    };
  }
  if (res.status === 0) {
    let parsed = null;
    try {
      parsed = JSON.parse(res.stdout || "");
    } catch {
      /* fall through — a missing/unparseable stdout on exit 0 still counts as realized */
    }
    return {
      target,
      ok: true,
      reason: "realized",
      detail: "",
      realized: (parsed && parsed.realized) || {},
    };
  }
  if (res.status === 3) {
    let parsed = null;
    try {
      parsed = JSON.parse(res.stdout || "");
    } catch {
      /* fall through to the raw shape below */
    }
    if (parsed && parsed.message) {
      return {
        target,
        ok: false,
        reason: parsed.stage === "import" ? "import-error" : "realize-error",
        errorName: parsed.name,
        framework: parsed.framework,
        detail: `${parsed.name ? `${parsed.name}: ` : ""}${parsed.message}`,
      };
    }
  }
  const stderr = (res.stderr || "").trim().split("\n").slice(0, 8).join("\n");
  return {
    target,
    ok: false,
    reason: "unknown-exit",
    detail: `probe exited ${res.status}${stderr ? `\n${stderr}` : ""}`,
  };
}

// The ONE error name the probe's structured failure may be read as "the operator never ran the
// provisioning command" — every other name (or an import failure) is a validated-but-unusable ("invalid")
// cache/install, which gets the SAME loud refusal but different guidance text.
const ORACLE_CACHE_UNPROVISIONED_ERROR_NAME = "OracleCacheUnprovisionedError";

// The preflight itself. Returns `{ ok, lines, reason, detail }` — same contract shape as
// `checkBuildPrerequisites`. `lines` is the operator-facing report, already naming the probe target, the
// underlying failure, and the exact (never auto-run) provisioning command. `loadProbe` is injected (the
// self-test substitutes a fake; production passes the real `runOracleCacheLoadProbe`).
export function checkOracleCachePrerequisite(opts) {
  const { repoRoot, loadProbe = runOracleCacheLoadProbe, timeoutMs, env } = opts;
  const probe = loadProbe({
    repoRoot,
    ...(timeoutMs === undefined ? {} : { timeoutMs }),
    ...(env === undefined ? {} : { env }),
  });
  if (probe.ok) {
    return {
      ok: true,
      reason: probe.reason || "realized",
      detail: "",
      lines: [],
      realized: probe.realized || {},
    };
  }
  const unprovisioned = probe.errorName === ORACLE_CACHE_UNPROVISIONED_ERROR_NAME;
  const lines = [
    `${ORACLE_CACHE_PREREQUISITE_MARKER}: the offline oracle npm cache this tree's Svelte/Vue compiled-` +
      "output conformance tests need (`verter_session/bf2-authoritative`, including the ENTIRE " +
      "`svelte_official_conformance_gate` suite) is " +
      `${unprovisioned ? "not provisioned" : "provisioned but could not be realized (present but unusable)"}.` +
      " Running the gate now would report divergences that are really an infrastructure absence, never a " +
      "compiler regression — or, with the feature off, silently omit these 45 tests again.",
    `  probe target: ${probe.target}`,
    `  probe failure: ${probe.detail}`,
  ];
  if (probe.framework) lines.push(`  framework: ${probe.framework}`);
  lines.push(
    "Produce it with (from the repo root — this is the ONLY sanctioned network step; the gate never runs " +
      "it for you):",
    `    ${ORACLE_CACHE_PROVISION_COMMAND}`,
    "The gate refuses to provision the cache for you (its verdict must not depend on a network mutation it " +
      "performed) and refuses to silently skip or mis-report the tests that need it. This check performs " +
      "the SAME offline realization `bin/check-candidate.mjs` performs on every request — a " +
      "present-but-unusable cache (corrupt, torn, or drifted from the committed lockfile closure) fails " +
      "identically to a missing one, never as a quiet 'axis skipped' comparison note.",
  );
  return { ok: false, reason: probe.reason, detail: probe.detail, lines };
}

// ----------------------------------------------------------------------------------------------------
// ARCHIVE-BUILD FEATURES — the features the ONE `cargo nextest archive --workspace` build (SURFACE 1's
// archive, per SINGLE-TEST-UNIVERSE) is built with.
//
// `ARCHIVE_FEATURES` turns `verter_session/bf2-authoritative` on for that archive build
// (`archiveAndList` in gate.mjs), so the 45 tests that feature gates are PRESENT in the archived test
// universe — not merely listed by a standalone `cargo test --features` invocation nobody runs. The
// separate shipped-cfg lane (`runShippedCfgLane`) does not consume this archive at all — it is a
// small package-scoped `cargo nextest run -p verter_shipped_cfg_contract`, built and run independently.
export const ARCHIVE_FEATURES = Object.freeze(["verter_session/bf2-authoritative"]);

// Pure builder for the `cargo nextest archive` argv — used by BOTH archive variants in gate.mjs, so a
// feature dropped here is dropped from every surface at once (never a per-variant divergence) and is
// directly unit-testable without invoking cargo.
export function buildNextestArchiveArgs(opts) {
  const {
    buildJobs,
    cargoProfile,
    archiveFile,
    runnerTarget,
    features = ARCHIVE_FEATURES,
    timingsEnabled = false,
  } = opts;
  return [
    "nextest",
    "archive",
    "--workspace",
    "--build-jobs",
    String(buildJobs),
    ...(timingsEnabled ? ["--timings"] : []),
    ...(features.length > 0 ? ["--features", features.join(",")] : []),
    ...(cargoProfile ? ["--cargo-profile", cargoProfile] : []),
    "--archive-file",
    archiveFile,
    "--target-dir",
    runnerTarget,
    "--zstd-level",
    "-7",
  ];
}

// Pure builder for archive-backed Surface 1. The local default deliberately lets nextest stop scheduling
// after its first failure; exhaustive CI/diagnostic runs opt back into the historical all-failures argv.
export function buildSurface1RunArgs({
  archiveFile,
  extractDir,
  repoRealpath,
  filterExpr,
  exhaustive = false,
  testThreads,
}) {
  return [
    "nextest",
    "run",
    "--archive-file",
    archiveFile,
    "--extract-to",
    extractDir,
    "--extract-overwrite",
    "--workspace-remap",
    repoRealpath,
    "-E",
    filterExpr,
    ...(exhaustive ? ["--no-fail-fast"] : []),
    "--test-threads",
    String(testThreads),
  ];
}

// Pure builders for the two shipped-cfg Cargo commands. `timingsEnabled` is a report-only capability
// result: false omits Cargo's stable HTML timing report, while true adds it at the verified supported
// position. Execution policy controls only nextest fail-fast; selection, profile and thread arguments stay
// identical.
export function buildShippedCfgCheckArgs({ timingsEnabled = false } = {}) {
  return [
    "check",
    "--workspace",
    "--all-targets",
    "--profile",
    "no-debug-assertions",
    ...(timingsEnabled ? ["--timings"] : []),
  ];
}

export function buildShippedCfgContractArgs({
  timingsEnabled = false,
  exhaustive = false,
  testThreads,
}) {
  return [
    "nextest",
    "run",
    "-p",
    "verter_shipped_cfg_contract",
    "--cargo-profile",
    "no-debug-assertions",
    ...(timingsEnabled ? ["--timings"] : []),
    ...(exhaustive ? ["--no-fail-fast"] : []),
    "--test-threads",
    String(testThreads),
  ];
}

// ----------------------------------------------------------------------------------------------------
// PARALLEL GATE LANES. The front archive/list phase remains owned by runnerTarget/gateDir. Only the two
// post-list execution lanes receive isolated mutable roots. Layout derivation is pure and fail-closed so a
// future path edit cannot silently make Cargo targets, extracted archives, work files, or buffered output
// aliases of one another.
// ----------------------------------------------------------------------------------------------------
function pathIsExactlyContained(parent, candidate) {
  const rel = relative(parent, candidate);
  return rel !== "" && rel !== ".." && !rel.startsWith(`..${sep}`) && !isAbsolute(rel);
}

export function deriveGateLaneLayout(runnerTarget, gateDir) {
  const runnerRoot = resolve(runnerTarget);
  const gateRoot = resolve(gateDir);
  if (!pathIsExactlyContained(runnerRoot, gateRoot)) {
    throw new RangeError(
      "gate work root must be exactly contained in the runner-owned target root",
    );
  }

  const surfaceGateRoot = join(gateRoot, "lanes", "surface-1");
  const shippedGateRoot = join(gateRoot, "lanes", "shipped-cfg");
  const layout = {
    front: { targetDir: runnerRoot, gateDir: gateRoot },
    surface1: {
      laneId: "surface-1",
      targetDir: join(runnerRoot, "lanes", "surface-1", "target"),
      workDir: join(surfaceGateRoot, "work"),
      extractDir: join(surfaceGateRoot, "extract"),
      outputFile: join(surfaceGateRoot, "output.log"),
    },
    shippedCfg: {
      laneId: "shipped-cfg",
      targetDir: join(runnerRoot, "lanes", "shipped-cfg", "target"),
      workDir: join(shippedGateRoot, "work"),
      outputFile: join(shippedGateRoot, "output.log"),
    },
  };
  const mutableRoots = [
    layout.surface1.targetDir,
    layout.surface1.workDir,
    layout.surface1.extractDir,
    layout.surface1.outputFile,
    layout.shippedCfg.targetDir,
    layout.shippedCfg.workDir,
    layout.shippedCfg.outputFile,
  ].map((root) => resolve(root));
  for (const candidate of mutableRoots) {
    if (!pathIsExactlyContained(runnerRoot, candidate)) {
      throw new RangeError(`lane mutable root is not exactly contained: ${candidate}`);
    }
  }
  for (let i = 0; i < mutableRoots.length; i++) {
    for (let j = i + 1; j < mutableRoots.length; j++) {
      if (
        mutableRoots[i] === mutableRoots[j] ||
        pathIsExactlyContained(mutableRoots[i], mutableRoots[j]) ||
        pathIsExactlyContained(mutableRoots[j], mutableRoots[i])
      ) {
        throw new RangeError(
          `lane mutable roots must be pairwise-disjoint: ${mutableRoots[i]} / ${mutableRoots[j]}`,
        );
      }
    }
  }
  return layout;
}

// One command plan delegates to the pre-existing command builders. It is shared by production and the
// cargo-free architecture test, so overlap cannot introduce a second filter/profile/archive policy.
// `shippedTestThreads` defaults to `testThreads` (the historical single-value behavior) when omitted, so
// existing callers that size both lanes identically are unaffected; production sizes it independently via
// `deriveGateLaneResourceSplit` so the two lanes' combined test-thread demand, when they run concurrently,
// sums to one ceiling instead of each independently claiming the whole ceiling.
export function buildGateLaneCommandPlan({
  archiveFile,
  surfaceExtractDir,
  repoRealpath,
  filterExpr,
  exhaustive,
  testThreads,
  shippedTestThreads = testThreads,
  shippedCheckTimingsEnabled = false,
  shippedContractTimingsEnabled = false,
}) {
  return {
    surface1: {
      args: buildSurface1RunArgs({
        archiveFile,
        extractDir: surfaceExtractDir,
        repoRealpath,
        filterExpr,
        exhaustive,
        testThreads,
      }),
    },
    shippedCfg: {
      checkArgs: buildShippedCfgCheckArgs({ timingsEnabled: shippedCheckTimingsEnabled }),
      contractArgs: buildShippedCfgContractArgs({
        timingsEnabled: shippedContractTimingsEnabled,
        exhaustive,
        testThreads: shippedTestThreads,
      }),
    },
  };
}

// Admit both lane promises before observing either result. A local Surface hard failure cancels only the
// shipped lane; exhaustive mode always awaits both. Watchdog/setup receipts carry an exitCode and are left
// to the supervisor's stronger aggregate abort authority.
//
// `concurrent` (default true — the historical/normal behavior) governs WHETHER both lanes are admitted
// together. When the caller's `deriveGateLaneResourceSplit` reports `concurrent: false` (the ceiling is too
// small to give both lanes their own core without the combined demand exceeding it), pass `concurrent:
// false` here: `runShippedLane` is never even INVOKED until `runSurfaceLane` has settled, so only one
// lane's `cargo`/`nextest` invocation is ever live and the ceiling is genuinely honored, not just described.
// The serial path reuses the exact same cancellation rules as the concurrent path (local fail-fast, infra
// exitCode) so the two scheduling modes differ only in overlap, never in verdict semantics.
export async function orchestrateGateLanes({
  exhaustive,
  runSurfaceLane,
  runShippedLane,
  cancelLane,
  concurrent = true,
}) {
  if (typeof exhaustive !== "boolean") throw new TypeError("exhaustive must be a boolean");
  if (typeof concurrent !== "boolean") throw new TypeError("concurrent must be a boolean");
  for (const [name, fn] of [
    ["runSurfaceLane", runSurfaceLane],
    ["runShippedLane", runShippedLane],
    ["cancelLane", cancelLane],
  ]) {
    if (typeof fn !== "function") throw new TypeError(`${name} must be a function`);
  }

  if (!concurrent) {
    // Serialized: `runShippedLane` is not called at all until `runSurfaceLane` resolves, so the two lanes'
    // `cargo`/`nextest` invocations never overlap in wall-clock — the combined demand at any instant is
    // exactly one lane's share, never both summed.
    let surface;
    try {
      surface = await runSurfaceLane();
    } catch (error) {
      await cancelLane("shipped-cfg", "SURFACE_1_INFRASTRUCTURE");
      throw error;
    }
    if (surface?.exitCode != null) {
      await cancelLane("shipped-cfg", "SURFACE_1_INFRASTRUCTURE");
      return { surface, shipped: null };
    }
    if (!exhaustive && surface?.hardFailure) {
      await cancelLane("shipped-cfg", "SURFACE_1_FAIL_FAST");
      return { surface, shipped: null };
    }
    let shipped;
    try {
      shipped = await runShippedLane();
    } catch (error) {
      await cancelLane("surface-1", "SHIPPED_CFG_INFRASTRUCTURE");
      throw error;
    }
    if (shipped?.exitCode != null) {
      await cancelLane("surface-1", "SHIPPED_CFG_INFRASTRUCTURE");
    }
    return { surface, shipped };
  }

  let surfacePromise;
  let shippedPromise;
  try {
    surfacePromise = Promise.resolve(runSurfaceLane());
  } catch (error) {
    surfacePromise = Promise.reject(error);
  }
  try {
    shippedPromise = Promise.resolve(runShippedLane());
  } catch (error) {
    shippedPromise = Promise.reject(error);
  }

  const surfaceCancellation = surfacePromise.then(
    async (surface) => {
      if (surface?.exitCode != null) {
        await cancelLane("shipped-cfg", "SURFACE_1_INFRASTRUCTURE");
      } else if (!exhaustive && surface?.hardFailure) {
        await cancelLane("shipped-cfg", "SURFACE_1_FAIL_FAST");
      }
    },
    () => cancelLane("shipped-cfg", "SURFACE_1_INFRASTRUCTURE"),
  );
  const shippedCancellation = shippedPromise.then(
    async (shipped) => {
      if (shipped?.exitCode != null) {
        await cancelLane("surface-1", "SHIPPED_CFG_INFRASTRUCTURE");
      }
    },
    () => cancelLane("surface-1", "SHIPPED_CFG_INFRASTRUCTURE"),
  );
  let surface;
  let shipped;
  try {
    [surface, shipped] = await Promise.all([surfacePromise, shippedPromise]);
  } catch (error) {
    await Promise.allSettled([surfaceCancellation, shippedCancellation]);
    throw error;
  }
  await Promise.all([surfaceCancellation, shippedCancellation]);
  return { surface, shipped };
}

function receiptExitCode(receipt) {
  return Number.isSafeInteger(receipt?.exitCode) ? receipt.exitCode : null;
}

// Pure final authority. Promise completion order cannot enter this function: it reads fixed receipt slots
// and appends failures in Surface/check/contract order. Coverage is an independent green fence.
export function reduceGateLaneReceipts({
  surface = null,
  shipped = null,
  shippedCfgLaneEnabled = true,
} = {}) {
  const exits = [receiptExitCode(surface), receiptExitCode(shipped)].filter(
    (code) => code !== null,
  );
  if (exits.length > 0) {
    const priority = [EXIT_MEMORY, EXIT_TIMEOUT, EXIT_STALL, EXIT_USAGE, EXIT_FAIL];
    const exitCode = priority.find((code) => exits.includes(code)) ?? exits[0];
    return {
      verdict: null,
      exitCode,
      failures: [],
      coverageComplete: false,
      measurementComplete: false,
      coverageDisposition: "aborted",
    };
  }

  const surfaceComplete = Boolean(surface?.coverage?.parseable && surface?.coverage?.complete);
  const shippedComplete = shippedCfgLaneEnabled
    ? Boolean(
        shipped?.check?.status === "ok" &&
          shipped?.contract?.status === "ok" &&
          shipped?.contract?.parseable &&
          shipped?.contract?.complete &&
          shipped?.parity?.complete &&
          shipped?.parity?.matches,
      )
    : true;
  const coverageComplete = surfaceComplete && shippedComplete;
  const coverageDisposition = coverageComplete
    ? "complete"
    : shippedCfgLaneEnabled &&
        (shipped?.check?.status === "cancelled" || shipped?.contract?.status === "cancelled")
      ? "cancelled-by-local-fail-fast"
      : surface?.hardFailure || (shippedCfgLaneEnabled && shipped?.hardFailure)
        ? "blocked-by-failure"
        : "incomplete";
  const failures = [];
  for (const failure of surface?.failures || []) failures.push({ ...failure });
  if (shippedCfgLaneEnabled) {
    for (const failure of shipped?.failures || []) {
      failures.push({
        ...failure,
        surface: String(failure.surface || "unknown").startsWith("shipped-cfg/")
          ? failure.surface
          : `shipped-cfg/${failure.surface || "unknown"}`,
      });
    }
  }
  if (!coverageComplete) {
    const missing = [];
    if (!surfaceComplete) missing.push("complete parseable Surface 1 receipt");
    if (shippedCfgLaneEnabled) {
      if (shipped?.check?.status !== "ok") missing.push("successful shipped-cfg check receipt");
      if (
        !shipped?.contract ||
        shipped.contract.status !== "ok" ||
        !shipped.contract.parseable ||
        !shipped.contract.complete
      ) {
        missing.push("complete parseable shipped-cfg contract receipt");
      }
      if (!shipped?.parity?.complete || !shipped?.parity?.matches) {
        missing.push("shipped-cfg expected-count parity");
      }
    }
    failures.push({
      surface: "gate/incomplete",
      name: `<required ${shippedCfgLaneEnabled ? "parallel-lane" : "Surface 1"} coverage incomplete: ${missing.join("; ")}>`,
    });
  }
  return {
    verdict:
      failures.length > 0 ? "FAIL" : surface?.toleratedOccurred ? "PASS-WITH-TOLERATED" : "PASS",
    exitCode: null,
    failures,
    coverageComplete,
    measurementComplete: coverageComplete,
    coverageDisposition,
  };
}

export const GATE_LANE_TRANSCRIPT_HEADERS = Object.freeze({
  "surface-1": "SURFACE 1: nextest run from the archive (process isolation) …",
  "shipped-check":
    "SHIPPED-CFG GUARD: cargo check --workspace --all-targets --profile no-debug-assertions …",
  "shipped-contract":
    "SHIPPED-CFG GUARD: cargo nextest run -p verter_shipped_cfg_contract --cargo-profile no-debug-assertions …",
});

export function canonicalGateLaneTranscriptSegments({ surface = null, shipped = null } = {}) {
  return [
    {
      phaseId: "surface-1",
      header: GATE_LANE_TRANSCRIPT_HEADERS["surface-1"],
      output: String(surface?.output || ""),
    },
    {
      phaseId: "shipped-check",
      header: GATE_LANE_TRANSCRIPT_HEADERS["shipped-check"],
      output: String(shipped?.check?.output || ""),
    },
    {
      phaseId: "shipped-contract",
      header: GATE_LANE_TRANSCRIPT_HEADERS["shipped-contract"],
      output: String(shipped?.contract?.output || ""),
    },
  ];
}

// ----------------------------------------------------------------------------------------------------
// TRYBUILD EXCLUSION — INTERIM, pending maintainer disposition. Do not delete this section without a
// ruling: their permanent disposition (drop them for good, keep this exclusion permanently, or restore
// them once the trybuild target dir is cached) is an open decision, not settled by this exclusion.
//
// A `trybuild::TestCases::new()` harness SPAWNS a real `cargo` build against a generated crate and, cold,
// compiles the crate's ENTIRE dependency closure before checking a single fixture — not a unit test.
// Measured here: 98s cold, 0.8s warm. Two of them tripped the gate's own 360s budget in a real run while
// passing 3/3 in isolation (already-raised once). `.config/nextest.toml` carries a `slow-timeout` override
// for the same class (see the "trybuild compile-fail tests" comment there) — that override is UNCHANGED
// and still applies to anyone running these tests directly; this exclusion removes them from canonical
// archive-backed Surface 1. The shipped contract has an independent package-only inventory.
//
// One row per file that actually calls `trybuild::TestCases::new()` — verified against a real
// `cargo nextest list --workspace --message-format json` listing, not guessed from filenames. A
// substring match on "compile_fail" also catches two UNRELATED tests that must stay in the gate and are
// deliberately NOT rows here: verter_lsp's
// `external_ts::membership_reconciler::tests::absent_compile_failed_removes` and verter_session's
// `types::tests::compile_failure_code_classification`. Each row's `modulePrefix` is the exact source-order
// module path (no trailing test name) so it covers every test in that file, present or future, including
// ones already marked `#[ignore]` (most of the verter_session rows are — they cost nothing today because
// no surface passes `--run-ignored`, but a future un-ignore must not silently reintroduce the cost without
// this exclusion already covering it).
// ----------------------------------------------------------------------------------------------------
export const TRYBUILD_EXCLUDED_SUITES = Object.freeze([
  { package: "verter_session", modulePrefix: "cases::g_compile::compile_fail::" },
  { package: "verter_language", modulePrefix: "cases::compile_fail::" },
  { package: "verter_identity", modulePrefix: "cases::compile_fail::" },
  { package: "verter_compiler", modulePrefix: "cases::assembly::assemble_sequence_compile_fail::" },
  { package: "verter_compiler", modulePrefix: "cases::pending_nav_request_compile_fail::" },
  { package: "verter_compiler", modulePrefix: "cases::registered_geometry_compile_fail::" },
  { package: "verter_compiler", modulePrefix: "cases::segmented_overwrite_compile_fail::" },
  { package: "verter_audit", modulePrefix: "cases::attribution_compile_fail::" },
  { package: "verter_type_runtime", modulePrefix: "cases::compile_fail::" },
]);

// Builds the nextest filterset expression (see https://nexte.st/docs/filtersets) that excludes every row
// above. `test(/^prefix/)` anchors at the start of the fully-qualified test name so it never matches a
// same-named module nested deeper, and pairing each `test()` arm with its own `package()` arm means a
// module path that happens to collide across two packages cannot cross-exclude the wrong crate's tests.
export function buildTrybuildExclusionFilterExpr(suites = TRYBUILD_EXCLUDED_SUITES) {
  const arms = suites.map((s) => `(package(${s.package}) and test(/^${s.modulePrefix}/))`);
  return `not (${arms.join(" or ")})`;
}

export function buildCanonicalSurface1FilterExpr(suites = TRYBUILD_EXCLUDED_SUITES) {
  return `(${buildTrybuildExclusionFilterExpr(suites)}) and not package(verter_shipped_cfg_contract)`;
}

// Legacy Surface-2 selftest fixture: per-row skip args for a directly executed libtest binary, which never
// sees a nextest filterset. Production gate.mjs no longer calls this helper. `--skip <prefix>` remains a
// plain (non-`--exact`) substring filter for the frozen regression classifier exercised in gate-selftest.mjs.
export function trybuildSkipArgsForPackage(pkg, suites = TRYBUILD_EXCLUDED_SUITES) {
  const args = [];
  for (const s of suites) {
    if (s.package !== pkg) continue;
    args.push("--skip", s.modulePrefix);
  }
  return args;
}

// Counts, from a REAL (unfiltered) nextest list JSON's suites, how many testcases each registered row
// actually matches in the archive under test. A row that matches ZERO tests means its file was renamed,
// moved, or deleted without updating this registry — the exclusion has silently gone stale (either it now
// excludes nothing for that file, letting the cargo-spawning cost back into the gate unnoticed, or worse,
// a differently-named module drifted under the same prefix). The caller must treat `missing.length > 0` as
// a hard setup failure, never a silent pass — the same "selectors matched non-zero work" contract the
// shipped-cfg guard's independent expected-test-inventory scan (`countTestAttributesInDir`) enforces for
// verter_shipped_cfg_contract, applied per-row here.
export function countTrybuildExclusionMatches(allSuites, suites = TRYBUILD_EXCLUDED_SUITES) {
  const perRow = suites.map(() => 0);
  let total = 0;
  for (const suite of allSuites || []) {
    const pkg = suite["package-name"];
    const testcases = suite.testcases || {};
    for (let i = 0; i < suites.length; i++) {
      if (suites[i].package !== pkg) continue;
      for (const name of Object.keys(testcases)) {
        if (name.startsWith(suites[i].modulePrefix)) {
          perRow[i]++;
          total++;
        }
      }
    }
  }
  const missing = suites.filter((_, i) => perRow[i] === 0);
  return { total, perRow, missing };
}

// ----------------------------------------------------------------------------------------------------
// Tolerated-failure allowlist — EXACT nextest test names (the env-only typeinfo freshness pair). A test
// whose EXACT name is in this set is tolerated ONLY when the freshness-tooling preflight ALLOWS it (the
// tools are genuinely absent); when the tools are present or were installed, a FAIL of one of these names
// is a HARD regression. The consultation of this set is GATED by the `freshnessToleranceAllowed` flag the
// preflight produces (see `preflightFreshnessTooling`) — the classifiers below take that flag and default
// it to `false` (fail-closed: tolerance off unless explicitly allowed). Matched against the EXACT name
// (the final whitespace token of a `FAIL [   …s] <binary> <test::path::name>` line), NOT a substring of
// the line — so a real regression in a differently-named test that merely CONTAINS one of these tokens
// still FAILS, and a name equal to an allowlisted one PLUS a suffix still FAILS.
// ----------------------------------------------------------------------------------------------------
// The binary that OWNS the tolerated tests. Tolerance is scoped to it, because a bare test PATH is not
// an identity: named failures already key on `<binary-id> <name>` so two binaries owning
// `cases::shared::same_name` stay two distinct failures, and the exemption has to use the same identity
// or any crate that happens to define a test at the allowlisted path inherits the exemption - several
// at once, all tolerated together.
export const TOLERATED_TEST_BINARY_ID = "verter_protocol::main";

export const TOLERATED_TEST_NAMES = new Set([
  // Post-consolidation, both env-only freshness tests live in the single `verter_protocol::main`
  // integration binary under the module path `cases::typeinfo_proto_ts_freshness::<fn>`. nextest renders
  // a run line as "<STATUS> [   …s] (n/m) verter_protocol::main cases::typeinfo_proto_ts_freshness::<fn>"
  // (the last whitespace token is the bare libtest path). The retained legacy direct-libtest classifier's
  // fixture prints "test cases::typeinfo_proto_ts_freshness::<fn> ... FAILED", so the exact name in both
  // active Surface 1 and that selftest-only fixture is the `cases::`-prefixed module path.
  // (Pre-consolidation these were a standalone `typeinfo_proto_ts_freshness`
  // binary; that bare/`typeinfo_proto_ts_freshness::`-qualified form no longer exists in the archive.)
  "cases::typeinfo_proto_ts_freshness::typeinfo_ts_bindings_are_byte_equal_to_regenerated_buf_output",
  "cases::typeinfo_proto_ts_freshness::proto_ts_bindings_byte_pinned_repo_wide",
]);

// Is `<binaryId, name>` the deliberately-exempt freshness pair? BOTH halves must match: the path alone
// is not an identity (see TOLERATED_TEST_BINARY_ID).
export function isToleratedIdentity(binaryId, name) {
  return binaryId === TOLERATED_TEST_BINARY_ID && TOLERATED_TEST_NAMES.has(name);
}

// ----------------------------------------------------------------------------------------------------
// Logging helpers (all to stderr so a piped JSON capture stays clean).
// ----------------------------------------------------------------------------------------------------
export function log(msg) {
  process.stderr.write(`[gate] ${msg}\n`);
}
export function warn(msg) {
  process.stderr.write(`[gate][warn] ${msg}\n`);
}
export function err(msg) {
  process.stderr.write(`[gate][error] ${msg}\n`);
}

// ----------------------------------------------------------------------------------------------------
// --prepare success marker + output. `--prepare` is a warm-pass, NOT a gate PASS: its exit 0 means
// "prepared", never "the suite built and passed". To keep a CI `grep PASS` from EVER mistaking a prepare
// run for a gate pass, the success marker is `PREPARED_NOT_GATE` and NONE of the prepare success-output
// lines may contain the token `PASS`. These lines are produced here (one place) so the self-test can assert
// both invariants in-process WITHOUT running cargo. `assertNoPassToken` is the byte-level guarantee the
// caller emits exactly these strings (a future edit re-introducing `PASS` trips it).
// ----------------------------------------------------------------------------------------------------
export const PREPARE_SUCCESS_MARKER = "PREPARED_NOT_GATE";

// A Windows proc-macro test harness is a real executable suite, but unlike ordinary target test binaries
// it is built for the host and dynamically loads Rust's host libraries. cargo-nextest supplies the listed
// host libdir while running it; --prepare's direct first-launch must reproduce that loader environment.
// The source of truth is the same nextest list JSON that supplied the suite. Missing/malformed metadata is
// an explicit warm setup failure, never a reason to skip the suite or fall back to ambient process.env.
export function buildPrepareWarmSpawnEnv({
  suite,
  rustBuildMeta,
  baseEnv,
  windows = IS_WINDOWS,
} = {}) {
  if (!windows || suite?.kind !== "proc-macro") return { ok: true, env: baseEnv };

  const binaryId = suite?.["binary-id"] || "?";
  const buildPlatform = suite?.["build-platform"];
  const libdir =
    typeof buildPlatform === "string" && buildPlatform !== ""
      ? rustBuildMeta?.platforms?.[buildPlatform]?.libdir
      : null;
  if (
    libdir?.status !== "available" ||
    typeof libdir.path !== "string" ||
    !isCwdIndependentAbsolute(libdir.path, true)
  ) {
    return {
      ok: false,
      detail:
        `prepare: proc-macro suite ${binaryId} has no usable Windows runtime libdir for listed ` +
        `build platform ${JSON.stringify(buildPlatform)} (expected rust-build-meta.platforms` +
        `[build-platform].libdir with status="available" and a CWD-independent absolute path)`,
    };
  }
  if (baseEnv === null || typeof baseEnv !== "object" || Array.isArray(baseEnv)) {
    return {
      ok: false,
      detail: `prepare: proc-macro suite ${binaryId} has no constructed base environment`,
    };
  }

  const env = { ...baseEnv };
  const pathKey = findPathEnvKey(baseEnv, true);
  const priorPath = pathKey === null ? "" : baseEnv[pathKey];
  for (const key of Object.keys(env)) {
    if (key.toUpperCase() === "PATH") delete env[key];
  }
  const libdirFolded = libdir.path.toUpperCase();
  const priorComponents =
    typeof priorPath === "string" && priorPath !== ""
      ? priorPath
          .split(pathDelimiterFor(true))
          .filter((component) => component.toUpperCase() !== libdirFolded)
      : [];
  env.PATH = [libdir.path, ...priorComponents].join(pathDelimiterFor(true));
  return { ok: true, env };
}

// The only successful direct warm is an exact numeric status 0 with no simultaneous signal or spawn error.
// Keep the classification pure so STATUS_DLL_NOT_FOUND, ordinary non-zero exits, contradictory result
// shapes, signals, and timeouts/spawn failures remain cargo-free testable and can never be tolerated as
// "probably warmed".
export function classifyPrepareWarmResult(result) {
  const hasSignal = result?.signal !== null && result?.signal !== undefined;
  const hasSpawnError = result?.error !== null && result?.error !== undefined;
  if (result?.status === 0 && !hasSignal && !hasSpawnError) return { ok: true };

  const details = [];
  if (result?.status !== null && result?.status !== undefined) {
    details.push(`exit ${result.status}`);
  }
  if (hasSignal) details.push(`signal ${result.signal}`);
  if (hasSpawnError) {
    const spawnError = result.error;
    const spawnErrorDetail =
      typeof spawnError?.message === "string" && spawnError.message !== ""
        ? spawnError.message
        : String(spawnError);
    details.push(`spawn error ${spawnErrorDetail}`);
  }
  return { ok: false, detail: details.join("; ") || "no exit status (spawn/timeout)" };
}

export function preparedSuccessLines(suiteCount, warmed, warmFailures, missing) {
  // NB: no line below may contain the token "PASS" — a CI `grep PASS` of prepare's output must find nothing
  // that looks like a gate verdict. `assertNoPassToken` enforces this on the assembled array.
  const lines = [
    `prepare: archived + listed ${suiteCount} suites; warmed first-launch assessment for ${warmed} ` +
      `binaries (${warmFailures} warm-list failure(s), ${missing} missing binary/-ies)`,
    "prepare is a PRE-WARM (it moves the legitimate first-launch assessment earlier); it does NOT disable " +
      "Gatekeeper or remove the cost, and it is NOT a gate verdict — run a bare or exhaustive gate.",
    `${PREPARE_SUCCESS_MARKER}: tests were NOT run — run the local gate (\`node scripts/gate.mjs\`) or ` +
      `the exhaustive gate (\`node scripts/gate.mjs --exhaustive\`) ` +
      "to actually build + verify the suite. A prepare exit 0 means PREPARED, never a verdict.",
  ];
  return assertNoPassToken(lines);
}

// Guard: assert no line contains the uppercase token PASS. Returns the array on success; throws otherwise.
// Used to byte-pin the prepare success output (and exercisable by the self-test) so a regression that
// reintroduces a `PASS`-bearing line fails loudly instead of silently making prepare grep-confusable.
export function assertNoPassToken(lines) {
  for (const l of lines) {
    if (l.includes("PASS")) {
      throw new Error(
        `prepare success output must not contain the token PASS; offending line: ${l}`,
      );
    }
  }
  return lines;
}

export function nowMs() {
  return Date.now();
}
export function delay(ms) {
  return new Promise((r) => setTimeout(r, ms));
}

// Duration parser: "50m" / "8m" / "5s" / "2h" / bare seconds -> integer seconds.
export function parseDuration(d) {
  const s = String(d);
  let n;
  let mult;
  if (s.endsWith("h")) {
    n = s.slice(0, -1);
    mult = 3600;
  } else if (s.endsWith("m")) {
    n = s.slice(0, -1);
    mult = 60;
  } else if (s.endsWith("s")) {
    n = s.slice(0, -1);
    mult = 1;
  } else {
    n = s;
    mult = 1;
  }
  if (!/^\d+$/.test(n)) {
    throw new Error(`invalid duration: '${d}'`);
  }
  return parseInt(n, 10) * mult;
}

// ----------------------------------------------------------------------------------------------------
// Process start-identity (defeats PID reuse). POSIX: `ps -o lstart=`. Windows: CIM CreationDate (or wmic).
// Returns a normalized non-empty string, or "" if the identity is uncheckable (the caller FAILs CLOSED on
// an alive-but-uncheckable holder).
// ----------------------------------------------------------------------------------------------------
const DOTNET_UNIX_EPOCH_MS = 62_135_596_800_000;

export function normalizeWindowsWmicCreationDate(value) {
  const match =
    /^\s*CreationDate=(\d{4})(\d{2})(\d{2})(\d{2})(\d{2})(\d{2})\.(\d{6})([+-])(\d{3})\s*$/i.exec(
      String(value || ""),
    );
  if (!match) return "";
  const [, yearText, monthText, dayText, hourText, minuteText, secondText, micros, sign, zone] =
    match;
  const year = Number(yearText);
  const month = Number(monthText);
  const day = Number(dayText);
  const hour = Number(hourText);
  const minute = Number(minuteText);
  const second = Number(secondText);
  const millis = Number(micros.slice(0, 3));
  const wallMs = Date.UTC(year, month - 1, day, hour, minute, second, millis);
  const wall = new Date(wallMs);
  if (
    year < 1601 ||
    wall.getUTCFullYear() !== year ||
    wall.getUTCMonth() !== month - 1 ||
    wall.getUTCDate() !== day ||
    wall.getUTCHours() !== hour ||
    wall.getUTCMinutes() !== minute ||
    wall.getUTCSeconds() !== second ||
    wall.getUTCMilliseconds() !== millis
  ) {
    return "";
  }
  const offsetMinutes = Number(zone) * (sign === "+" ? 1 : -1);
  const dotnetMs = wallMs - offsetMinutes * 60_000 + DOTNET_UNIX_EPOCH_MS;
  return Number.isSafeInteger(dotnetMs) ? `win-start-ms:${dotnetMs}` : "";
}

export function procIdentity(pid) {
  if (!/^\d+$/.test(String(pid))) return "";
  if (IS_WINDOWS) {
    // Get-Process is materially faster than a CIM query. Millisecond-normalized start time is shared by
    // every bulk snapshot below, avoiding representation drift between DateTime sources.
    let r = spawnSync(
      "powershell",
      [
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        `$p = Get-Process -Id ${pid} -ErrorAction SilentlyContinue; ` +
          `if ($p) { "win-start-ms:$([math]::Floor($p.StartTime.ToUniversalTime().Ticks / 10000))" }`,
      ],
      { encoding: "utf8", windowsHide: true },
    );
    let out = (r.stdout || "").trim();
    if (!out) {
      r = spawnSync(
        "wmic",
        ["process", "where", `ProcessId=${pid}`, "get", "CreationDate", "/value"],
        {
          encoding: "utf8",
          windowsHide: true,
        },
      );
      out = normalizeWindowsWmicCreationDate(r.stdout || "");
    }
    return out.replace(/\s+/g, " ").trim();
  }
  const r = spawnSync("ps", ["-o", "lstart=", "-p", String(pid)], { encoding: "utf8" });
  return (r.stdout || "").trim().replace(/\s+/g, " ");
}

export function classifyProcessIdentityComparison(storedIdentity, liveIdentity) {
  const stored = String(storedIdentity || "");
  const live = String(liveIdentity || "");
  const bothPresent = Boolean(stored && live);
  const hasRawWmic = /^\s*CreationDate=/i.test(stored) || /^\s*CreationDate=/i.test(live);
  const comparable =
    bothPresent &&
    !hasRawWmic &&
    stored.startsWith("win-start-ms:") === live.startsWith("win-start-ms:");
  return {
    comparable,
    matches: comparable && stored === live,
    provesReuse: comparable && stored !== live,
  };
}

// Is a pid alive? EPERM ⇒ alive (a process we cannot signal but that exists).
export function pidAlive(pid) {
  if (!/^\d+$/.test(String(pid))) return false;
  try {
    process.kill(pid, 0);
    return true;
  } catch (e) {
    return e.code === "EPERM";
  }
}

// ----------------------------------------------------------------------------------------------------
// Is a process GROUP (POSIX) / a pid (Windows) live RIGHT NOW? On POSIX, `process.kill(-pgid, 0)` throws
// ESRCH only when the whole group is gone; success or EPERM means a live target. On Windows there is no
// process-group signal, so we probe the tree-root pid directly. Used both by the contained-step watchdog's
// signaled-live discriminator and by reapTree's post-kill verification poll.
// ----------------------------------------------------------------------------------------------------
export function groupOrPidAlive(pid) {
  if (!pid) return false;
  if (IS_WINDOWS) return pidAlive(pid);
  try {
    process.kill(-pid, 0);
    return true;
  } catch (e) {
    return e.code === "EPERM";
  }
}

// ----------------------------------------------------------------------------------------------------
// Negative-PGID reap (POSIX) / taskkill tree (Windows). TERM, grace, KILL, then a bounded VERIFICATION
// poll that confirms the tree is actually gone before returning. Safe on an already-dead group. Returns a
// VERIFIED outcome `{ reaped, confirmedDead, wasLive }`:
//   - `wasLive`        — the group/pid was live at the instant the reap began (the signaled-live
//                        discriminator: a real TIMEOUT/STALL reap hits a live target; a pure race finds
//                        ESRCH). Captured SYNCHRONOUSLY before any await.
//   - `confirmedDead`  — after SIGKILL (POSIX) / taskkill /T /F (Windows) we POLLED the group/pid and it
//                        is now provably gone (ESRCH on POSIX; pidAlive false on Windows). Teardown MUST
//                        NOT treat the lock as cleanly released while the active child tree is still live,
//                        so the caller keys "is the tree dead?" on THIS flag, not on the kill returning.
//   - `reaped`         — a reap was attempted (true whenever pid was non-null).
// If death cannot be confirmed within the bound, `confirmedDead` is false and the caller logs the
// uncertainty (and still releases, to avoid a permanent hang) — it does NOT silently claim clean teardown.
//
// `verifyBudgetMs` bounds the post-KILL confirmation poll (default 4000ms). It is SEPARATE from `graceMs`
// (the SIGTERM→SIGKILL grace window): grace lets a well-behaved tree exit on TERM; the verify budget
// confirms a KILL'd tree is actually reaped (Windows taskkill returns before the tree is fully gone;
// POSIX SIGKILL is async w.r.t. the process actually being reaped by its parent).
// ----------------------------------------------------------------------------------------------------
export async function reapTree(pid, graceMs, verifyBudgetMs = 4000) {
  if (!pid) return { reaped: false, confirmedDead: true, wasLive: false };
  // Capture the signaled-live discriminator SYNCHRONOUSLY, before any await can interleave.
  const wasLive = groupOrPidAlive(pid);

  if (IS_WINDOWS) {
    spawnSync("taskkill.exe", ["/PID", String(pid), "/T", "/F"], {
      windowsHide: true,
      stdio: "ignore",
    });
    // taskkill /T /F returns BEFORE the OS has finished tearing the tree down. POLL the tree-root pid
    // until it is confirmed gone (bounded). A re-query of the pid is the only portable Windows liveness
    // probe; if it ever comes back alive we re-issue the taskkill (PID reuse aside, this is a forced kill).
    const deadline = nowMs() + verifyBudgetMs;
    while (nowMs() < deadline) {
      if (!pidAlive(pid)) return { reaped: true, confirmedDead: true, wasLive };
      spawnSync("taskkill.exe", ["/PID", String(pid), "/T", "/F"], {
        windowsHide: true,
        stdio: "ignore",
      });
      await delay(150);
    }
    return { reaped: true, confirmedDead: !pidAlive(pid), wasLive };
  }

  const pgid = pid;
  const sig = (s) => {
    try {
      process.kill(-pgid, s);
    } catch (e) {
      if (e.code !== "ESRCH") {
        /* swallow EPERM/other: best-effort reap */
      }
    }
  };
  sig("SIGTERM");
  // Grace loop: give the tree a chance to exit cleanly on SIGTERM before SIGKILL. Probe the GROUP directly
  // (negative pgid): ESRCH => the whole group is gone; EPERM => still live (cannot signal it).
  const groupAlive = () => {
    try {
      process.kill(-pgid, 0);
      return true;
    } catch (e) {
      return e.code === "EPERM";
    }
  };
  const graceDeadline = nowMs() + graceMs;
  let aliveAfterGrace = true;
  while (nowMs() < graceDeadline) {
    if (!groupAlive()) {
      aliveAfterGrace = false;
      break;
    }
    await delay(200);
  }
  if (!aliveAfterGrace) {
    return { reaped: true, confirmedDead: true, wasLive };
  }
  // Still live after grace => SIGKILL, then VERIFY death (SIGKILL is async w.r.t. the process actually
  // being reaped by its parent). POLL the group until ESRCH or the verify budget elapses.
  sig("SIGKILL");
  const verifyDeadline = nowMs() + verifyBudgetMs;
  while (nowMs() < verifyDeadline) {
    if (!groupAlive()) return { reaped: true, confirmedDead: true, wasLive };
    sig("SIGKILL");
    await delay(150);
  }
  // Could not confirm death within the budget. Report the uncertainty; the caller logs it and still
  // releases to avoid a permanent hang.
  return { reaped: true, confirmedDead: !groupAlive(), wasLive };
}

// ----------------------------------------------------------------------------------------------------
// Provenance-filtered sweep helpers: lingering cargo/rustc/cargo-nextest/nextest that reference the
// runner-owned target dir, TERM->KILL. The provenance gate is SOLELY the runner-owned target dir — NOT the
// repo root, because a developer's interactive cargo / rust-analyzer / rustc all carry the repo root in
// argv but write the DEFAULT target/debug, never the gate-runner dir. Conservative: only runner-owned
// target-dir processes.
// ----------------------------------------------------------------------------------------------------
export function listProcesses() {
  // Returns [{ pid, cmd, identity }]. Every provenance signal is conditioned on this start identity.
  if (IS_WINDOWS) {
    let r = spawnSync(
      "powershell",
      [
        "-NoProfile",
        "-Command",
        'Get-CimInstance Win32_Process | ForEach-Object { "$($_.ProcessId)`twin-start-ms:$([math]::Floor($_.CreationDate.ToUniversalTime().Ticks / 10000))`t$($_.CommandLine)" }',
      ],
      { encoding: "utf8", windowsHide: true, maxBuffer: 64 * 1024 * 1024 },
    );
    let out = r.stdout || "";
    if (!out.trim()) {
      r = spawnSync(
        "wmic",
        ["process", "get", "ProcessId,CreationDate,CommandLine", "/format:csv"],
        {
          encoding: "utf8",
          windowsHide: true,
          maxBuffer: 64 * 1024 * 1024,
        },
      );
      out = r.stdout || "";
    }
    const rows = [];
    for (const line of out.split(/\r?\n/)) {
      const firstTab = line.indexOf("\t");
      const secondTab = firstTab < 0 ? -1 : line.indexOf("\t", firstTab + 1);
      if (firstTab > 0 && secondTab > firstTab) {
        const pid = line.slice(0, firstTab).trim();
        const identity = line.slice(firstTab + 1, secondTab).trim();
        const cmd = line.slice(secondTab + 1).trim();
        if (/^\d+$/.test(pid) && identity) rows.push({ pid: parseInt(pid, 10), cmd, identity });
        continue;
      }
      // WMIC's CSV format is command-line ambiguous when argv contains commas. Refuse to synthesize an
      // identity from an ambiguous row; an empty fallback is fail-closed (it sends no provenance signal).
    }
    return rows;
  }
  const r = spawnSync("ps", ["-axww", "-o", "pid=,lstart=,command="], {
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
  });
  const out = r.stdout || "";
  const rows = [];
  for (const line of out.split("\n")) {
    const trimmed = line.replace(/^\s+/, "");
    if (!trimmed) continue;
    const match = /^(\d+)\s+(\S+\s+\S+\s+\S+\s+\S+\s+\S+)\s+(.*)$/.exec(trimmed);
    if (!match) continue;
    rows.push({ pid: Number(match[1]), identity: match[2].replace(/\s+/g, " "), cmd: match[3] });
  }
  return rows;
}

// Parse `ps -axo pid=,ppid=,rss=` and recursively sum the resident sets of the REAL process TREE rooted at
// `rootPid` — every descendant reachable by walking child->parent links, not just processes that still share
// the root's process GROUP. RSS is reported by ps in KiB. Summing per-process RSS is intentionally
// conservative because shared pages may appear in more than one process: the watchdog's job is to preserve
// machine headroom, not to maximize utilization up to the last uniquely-resident byte.
//
// This used to group by PROCESS GROUP (`pgid == rootPid`, since `runContainedStep` spawns the root detached
// so its own pid is its own pgid). That undercounted catastrophically for a `cargo nextest run` root: nextest
// puts each test process it executes into its OWN fresh process group (confirmed against the real binary —
// `ps -axo pid=,ppid=,pgid=` during a live run shows every executing test with a unique pgid equal to its own
// pid, while its ppid stays the nextest process), which is how nextest signals/kills a single hung test
// without touching its own group. A pgid-only sum sees the `cargo`/`cargo-nextest` wrapper and NONE of the
// actual test processes doing the work — exactly the "43 MiB across 1 process(es)" reading a full-workspace
// SURFACE 1/3 run produced. Parent-pid tree walk (mirroring `parseWindowsProcessTableRss`, which was never
// vulnerable to this because Windows job-tree membership is a parent-pid property, not group membership)
// finds every descendant regardless of what process group it reassigned itself into.
function parseProcessForestRss(rows, roots, unitBytes, absentDetail) {
  const perRoot = [];
  const claimedByPid = new Map();
  for (const root of Array.isArray(roots) ? roots : []) {
    const rootPid = Number(root.pid);
    const expectedRootIdentity = root.identity || root.rootIdentity || "";
    const identityRequired =
      Object.prototype.hasOwnProperty.call(root, "identity") ||
      Object.prototype.hasOwnProperty.call(root, "rootIdentity");
    const rootRow = rows.get(rootPid);
    if (!root.closed && (!Number.isInteger(rootPid) || rootPid <= 0 || !rootRow)) {
      const detail = `${absentDetail}: live root ${rootPid || String(root.pid)} missing`;
      return { ok: false, rssBytes: 0, processCount: 0, detail, error: detail };
    }
    if (!root.closed && identityRequired && !expectedRootIdentity) {
      const detail = `process identity unavailable at live root ${rootPid}`;
      return { ok: false, rssBytes: 0, processCount: 0, detail, error: detail };
    }
    if (!root.closed && root.pendingIdentity && !rootRow.identity) {
      const detail = `process identity unavailable at pending live root ${rootPid}`;
      return { ok: false, rssBytes: 0, processCount: 0, detail, error: detail };
    }
    if (
      !root.closed &&
      expectedRootIdentity &&
      (!rootRow.identity || rootRow.identity !== expectedRootIdentity)
    ) {
      const detail =
        `process identity mismatch at live root ${rootPid}: expected ${expectedRootIdentity}, got ` +
        `${rootRow.identity || "<uncheckable>"}`;
      return { ok: false, rssBytes: 0, processCount: 0, detail, error: detail };
    }
    const included = new Set();
    if (
      !root.closed &&
      rootRow &&
      (!expectedRootIdentity || rootRow.identity === expectedRootIdentity)
    ) {
      included.add(rootPid);
    }
    for (const owned of root.ownedIdentities || []) {
      const row = rows.get(Number(owned.pid));
      if (row?.identity && owned.identity && row.identity === owned.identity) {
        included.add(Number(owned.pid));
      }
    }
    let changed = true;
    while (changed) {
      changed = false;
      for (const [pid, row] of rows) {
        if (!included.has(pid) && included.has(row.parentPid)) {
          included.add(pid);
          changed = true;
        }
      }
    }
    if (root.closed && included.size === 0) continue;
    for (const pid of included) {
      const prior = claimedByPid.get(pid);
      if (prior !== undefined) {
        const detail =
          `process forest overlap at pid ${pid}: registration ${String(prior)} and ` +
          `${String(root.tokenId)}`;
        return { ok: false, rssBytes: 0, processCount: 0, detail, error: detail };
      }
      claimedByPid.set(pid, root.tokenId);
    }
    let rssBytes = 0;
    for (const pid of included) rssBytes += rows.get(pid).rss * unitBytes;
    const identities = [...included]
      .map((pid) => ({ pid, identity: rows.get(pid).identity || "" }))
      .filter((row) => row.identity);
    perRoot.push({
      tokenId: root.tokenId,
      laneId: root.laneId,
      pid: rootPid,
      rssBytes,
      processCount: included.size,
      pids: [...included],
      identities,
    });
  }
  const perLane = {};
  for (const row of perRoot) {
    const lane = perLane[row.laneId] || { rssBytes: 0, processCount: 0 };
    lane.rssBytes += row.rssBytes;
    lane.processCount += row.processCount;
    perLane[row.laneId] = lane;
  }
  return {
    ok: true,
    rssBytes: perRoot.reduce((sum, row) => sum + row.rssBytes, 0),
    processCount: perRoot.reduce((sum, row) => sum + row.processCount, 0),
    perRoot,
    perLane,
  };
}

export function parsePosixProcessForestRss(text, roots) {
  const rows = new Map();
  for (const line of String(text || "").split(/\r?\n/)) {
    const match = /^\s*(\d+)\s+(\d+)\s+(\d+)(?:\s+(.+?))?\s*$/.exec(line);
    if (!match) continue;
    rows.set(Number(match[1]), {
      parentPid: Number(match[2]),
      rss: Number(match[3]),
      identity: (match[4] || "").trim().replace(/\s+/g, " "),
    });
  }
  return parseProcessForestRss(rows, roots, 1024, "process tree root absent from ps snapshot");
}

export function parsePosixProcessTableRss(text, rootPid) {
  const result = parsePosixProcessForestRss(text, [
    { tokenId: 1, laneId: "single-step", pid: rootPid },
  ]);
  return result.ok
    ? { ok: true, rssBytes: result.rssBytes, processCount: result.processCount }
    : {
        ok: false,
        rssBytes: 0,
        processCount: 0,
        detail: "process tree root absent from ps snapshot",
      };
}

// Parse `pid<TAB>parent-pid<TAB>working-set-bytes` rows and recursively sum a Windows process tree.
export function parseWindowsProcessForestRss(text, roots) {
  const rows = new Map();
  for (const line of String(text || "").split(/\r?\n/)) {
    const match = /^\s*(\d+)\s+([0-9]+)\s+([0-9]+)(?:\s+(.+?))?\s*$/.exec(line);
    if (!match) continue;
    rows.set(Number(match[1]), {
      parentPid: Number(match[2]),
      rss: Number(match[3]),
      identity: (match[4] || "").trim(),
    });
  }
  return parseProcessForestRss(rows, roots, 1, "tree root absent from CIM snapshot");
}

export function parseWindowsProcessTableRss(text, rootPid) {
  const result = parseWindowsProcessForestRss(text, [
    { tokenId: 1, laneId: "single-step", pid: rootPid },
  ]);
  return result.ok
    ? { ok: true, rssBytes: result.rssBytes, processCount: result.processCount }
    : { ok: false, rssBytes: 0, processCount: 0, detail: "tree root absent from CIM snapshot" };
}

function parseProcessForestSnapshotWithExitRaces(parser, text, roots) {
  const first = parser(text, roots);
  if (first.ok) return first;
  // A root may exit while the single native snapshot command is in flight, before Node can dispatch the
  // child's close event. Reparse the SAME snapshot (never issue a second OS query) with only roots proven
  // dead after capture marked closed. A still-live missing/overlapping root remains a hard monitor failure.
  let changed = false;
  const after = roots.map((root) => {
    if (root.closed || pidAlive(root.pid)) return root;
    changed = true;
    return { ...root, closed: true };
  });
  return changed ? parser(text, after) : first;
}

// Platform-native process-tree RSS snapshot. The production watchdog treats repeated inability to sample
// as a memory-safety abort, so a missing/broken process inspector cannot silently disable the ceiling.
export function sampleProcessForestRssBytes(roots) {
  if (!Array.isArray(roots) || roots.length === 0) {
    return { ok: true, rssBytes: 0, processCount: 0, perRoot: [], perLane: {} };
  }
  if (IS_WINDOWS) {
    const command =
      'Get-CimInstance Win32_Process | ForEach-Object { "$($_.ProcessId)`t$($_.ParentProcessId)`t$($_.WorkingSetSize)`twin-start-ms:$([math]::Floor($_.CreationDate.ToUniversalTime().Ticks / 10000))" }';
    const result = spawnSync(
      "powershell.exe",
      ["-NoProfile", "-NonInteractive", "-Command", command],
      {
        encoding: "utf8",
        windowsHide: true,
        timeout: 5_000,
        maxBuffer: 64 * 1024 * 1024,
      },
    );
    if (result.status !== 0 || result.error) {
      return {
        ok: false,
        rssBytes: 0,
        processCount: 0,
        detail: result.error ? result.error.message : `PowerShell CIM exited ${result.status}`,
      };
    }
    return parseProcessForestSnapshotWithExitRaces(
      parseWindowsProcessForestRss,
      result.stdout,
      roots,
    );
  }

  const result = spawnSync("ps", ["-axww", "-o", "pid=,ppid=,rss=,lstart="], {
    encoding: "utf8",
    timeout: 5_000,
    maxBuffer: 64 * 1024 * 1024,
  });
  if (result.status !== 0 || result.error) {
    return {
      ok: false,
      rssBytes: 0,
      processCount: 0,
      detail: result.error ? result.error.message : `ps exited ${result.status}`,
    };
  }
  return parseProcessForestSnapshotWithExitRaces(parsePosixProcessForestRss, result.stdout, roots);
}

export function sampleProcessTreeRssBytes(rootPid) {
  if (!rootPid) return { ok: false, rssBytes: 0, processCount: 0, detail: "child pid unavailable" };
  const result = sampleProcessForestRssBytes([{ tokenId: 1, laneId: "single-step", pid: rootPid }]);
  return result.ok
    ? { ok: true, rssBytes: result.rssBytes, processCount: result.processCount }
    : { ok: false, rssBytes: 0, processCount: 0, detail: result.detail };
}

export function isBuildTool(cmd) {
  // cargo / rustc / cargo-nextest / nextest — word-ish boundaries so "cargo-nextest" and "/usr/bin/cargo"
  // both match but an unrelated path containing "cargocult" does not. An optional `.exe` suffix is matched
  // so real Windows command lines (`C:\Users\…\.cargo\bin\cargo.exe`, `rustc.exe`, `cargo-nextest.exe`,
  // `nextest.exe`) are recognized. A leading/trailing QUOTE (`"` or `'`) is ALSO an executable-name
  // boundary so a quoted full path — `"C:\Users\Name With Space\.cargo\bin\cargo.exe" nextest run` (the
  // standard Windows form when the path contains spaces) — is recognized; without quote boundaries the
  // opening `"` would block the `(^|[\s/\\])` boundary and the build tool would escape the sweep. The argv
  // is lowercased first so a mixed-case Windows path matches.
  const c = cmd.toLowerCase();
  const B = `(^|[\\s/\\\\"'])`; // start-of-string OR whitespace / path-sep / quote — an exec-name boundary.
  const E = `(\\.exe)?([\\s"']|$)`; // optional `.exe`, then whitespace / quote / end — closing boundary.
  return (
    new RegExp(`${B}cargo-nextest${E}`).test(c) ||
    new RegExp(`${B}cargo${E}`).test(c) ||
    new RegExp(`${B}rustc${E}`).test(c) ||
    new RegExp(`${B}nextest${E}`).test(c)
  );
}

// Does a process command line reference the runner-owned target dir? On Windows, command lines and the
// target path can differ in case and in slash direction (`\` vs `/`); normalize both to a lowercase,
// forward-slash form before the containment check so the sweep's "only the runner-owned target dir"
// scoping holds on Windows. On POSIX, paths are case- and separator-stable.
//
// The match is PATH-TOKEN aware, NOT a raw substring: the target dir must appear at a path-SEGMENT
// boundary — the character immediately after the matched target dir must be a separator (`/`, or `\` on
// Windows), a quote, whitespace, or end-of-string. A raw `includes` would let `…/target/gate-runner`
// spuriously match a SIBLING `…/target/gate-runner2` (the runner target dir is a substring of the
// sibling's path), which would sweep an unrelated runner's processes. Requiring a trailing segment
// boundary stops the sibling-dir false positive while still matching the runner dir itself and any
// `…/gate-runner/debug/deps/…` descendant path.
// `windows` is parameterized (defaulting to the live platform) so the matcher's Windows branch is unit-
// testable on a POSIX host.
export function targetDirMatches(cmd, targetDir, windows) {
  if (!targetDir) return false;
  const norm = windows ? (s) => s.toLowerCase().replace(/\\/g, "/") : (s) => s;
  const hay = norm(cmd);
  let needle = norm(targetDir);
  // Trailing separators on the target dir are not significant to the boundary check.
  needle = needle.replace(/[/\\]+$/, "");
  if (!needle) return false;
  let from = 0;
  for (;;) {
    const at = hay.indexOf(needle, from);
    if (at < 0) return false;
    const after = hay[at + needle.length];
    // end-of-string, a path separator, a quote, or whitespace = a segment boundary => a real match.
    if (
      after === undefined ||
      after === "/" ||
      after === "\\" ||
      after === '"' ||
      after === "'" ||
      /\s/.test(after)
    ) {
      return true;
    }
    // Otherwise this occurrence is mid-segment (e.g. the `2` in `gate-runner2`); keep scanning.
    from = at + 1;
  }
}
export function cmdReferencesTargetDir(cmd, targetDir) {
  return targetDirMatches(cmd, targetDir, IS_WINDOWS);
}

export async function provenanceSweep(targetDir, graceMs, options = {}) {
  if (!targetDir) return { matched: 0, signalled: 0, identityMismatches: 0 };
  const {
    listProcessesFn = listProcesses,
    processIdentityFn = procIdentity,
    delayFn = delay,
    signalProcessFn = null,
  } = options;
  const self = process.pid;
  const nativeSignal = (pid, signal) => {
    if (IS_WINDOWS) {
      // /T tears down the whole tree (a swept cargo.exe may have spawned rustc.exe children), /F forces it.
      spawnSync("taskkill.exe", ["/PID", String(pid), "/T", "/F"], {
        windowsHide: true,
        stdio: "ignore",
      });
    } else {
      try {
        process.kill(pid, signal);
      } catch {
        /* ignore */
      }
    }
  };
  const signal = signalProcessFn || nativeSignal;
  const matches = () =>
    listProcessesFn().filter(
      (p) =>
        p.pid !== self &&
        p.identity &&
        isBuildTool(p.cmd) &&
        cmdReferencesTargetDir(p.cmd, targetDir),
    );
  let matched = 0;
  let signalled = 0;
  let identityMismatches = 0;
  const signalExact = (candidate, signalName) => {
    matched += 1;
    const liveIdentity = processIdentityFn(candidate.pid);
    if (!liveIdentity || liveIdentity !== candidate.identity) {
      identityMismatches += 1;
      return;
    }
    signal(candidate.pid, signalName);
    signalled += 1;
  };
  // TERM pass.
  const termMatches = matches();
  for (const p of termMatches) signalExact(p, "SIGTERM");
  if (termMatches.length === 0) return { matched, signalled, identityMismatches };
  await delayFn(Math.min(graceMs, 1500));
  // KILL pass.
  for (const p of matches()) signalExact(p, "SIGKILL");
  return { matched, signalled, identityMismatches };
}

// The gate-owned sentinel marker written INSIDE the lockdir at acquire time. A directory is reclaimable
// (rename+recursive-remove) ONLY if it carries this marker — i.e. ONLY if the gate itself created it — AND
// the marker's stored repo realpath matches ours. An arbitrary pre-existing directory that a mis-set
// VERTER_GATE_LOCK / MOM_GATE_LOCK happens to point at is NEVER renamed or removed (no sentinel), and
// neither is a foreign checkout's lock (sentinel repo realpath differs).
export const GATE_LOCK_SENTINEL = ".verter-gate-lock";

// Parse the gate-owned sentinel. The file is written as "<token>\n<repoRealpath>\n" at acquire. Returns
// `{ present, token, repoRealpath }`. `present` is false (and the repo realpath empty) when the file is
// absent or unreadable — an unparseable/absent sentinel is NEVER treated as ours.
export function parseLockSentinel(sentinelFile) {
  let raw;
  try {
    raw = readFileSync(sentinelFile, "utf8");
  } catch {
    return { present: false, token: "", repoRealpath: "" };
  }
  // First line = token, second line = repo realpath. A repo path can in principle contain a newline on
  // some filesystems, but the gate writes a realpath (newlines in a realpath are exceptionally rare); we
  // take the FIRST line as the token and the REMAINDER (up to a trailing newline) as the repo realpath so
  // a path is reconstructed faithfully even if it ever contained an embedded separator that is not `\n`.
  const nl = raw.indexOf("\n");
  if (nl < 0) {
    // Only one line present — a malformed/partial sentinel with no repo realpath. Treat as present but
    // with an empty repo so the reclaim "repo matches ours" check fails closed (never reclaimed).
    return { present: true, token: raw.trim(), repoRealpath: "" };
  }
  const token = raw.slice(0, nl);
  let rest = raw.slice(nl + 1);
  // Strip exactly one trailing newline (the format's terminator); keep any interior content verbatim.
  if (rest.endsWith("\n")) rest = rest.slice(0, -1);
  return { present: true, token: token.trim(), repoRealpath: rest.trim() };
}

// ----------------------------------------------------------------------------------------------------
// Mutex: mkdir lockdir + a gate-owned sentinel (storing the OWNING repo realpath) + owner.json +
// atomic-rename reclaim. NEVER renames/removes a directory the gate did not create (no sentinel), NEVER
// reclaims a live holder's dir, and NEVER reclaims a FOREIGN repo's lock — EVERY reclaim path (including
// owner.json absent / owner == null) parses the sentinel and refuses unless its stored repo realpath
// equals ours. owner.json = { token, pid, repoRealpath, targetDir, createdAtMs, processStartIdentity }.
// ----------------------------------------------------------------------------------------------------
export class Mutex {
  constructor(lockdir, token, ctx) {
    this.lockdir = lockdir;
    this.ownerFile = join(lockdir, "owner.json");
    this.sentinelFile = join(lockdir, GATE_LOCK_SENTINEL);
    this.token = token;
    this.ctx = ctx; // { pid, repoRealpath, targetDir }
    this.held = false;
    this.refuseDetail = "";
    this.reclaimRefused = false; // set true by _reclaim when it refuses a non-gate-owned/foreign dir
    this.INIT_GRACE_MS = 5000;
    this.RECLAIM_RACE_RETRIES = 8;
    this.RECLAIM_RACE_BACKOFF_MS = 200;
    this.KILL_GRACE_MS = 5000;
  }

  // Write the gate-owned sentinel marker FIRST (immediately after winning the mkdir, before owner.json), so
  // that even a crash between mkdir and the owner write leaves a dir provably created by THIS gate AND
  // stamped with THIS repo's realpath — the mid-init reclaim path keys on the sentinel's repo, not on
  // owner.json presence. Format: "<token>\n<repoRealpath>\n".
  _writeSentinel() {
    writeFileSync(this.sentinelFile, `${this.token}\n${this.ctx.repoRealpath}\n`);
  }

  _readSentinel() {
    return parseLockSentinel(this.sentinelFile);
  }

  _writeOwner() {
    const owner = {
      token: this.token,
      pid: this.ctx.pid,
      repoRealpath: this.ctx.repoRealpath,
      targetDir: this.ctx.targetDir,
      createdAtMs: nowMs(),
      processStartIdentity: procIdentity(this.ctx.pid),
    };
    // Heartbeat-style: temp-write + atomic rename so a reader never sees a half-written owner.json.
    const tmp = join(this.lockdir, `owner.json.tmp.${process.pid}`);
    writeFileSync(tmp, JSON.stringify(owner, null, 0));
    renameSync(tmp, this.ownerFile);
  }

  _readOwner() {
    try {
      return JSON.parse(readFileSync(this.ownerFile, "utf8"));
    } catch {
      return null;
    }
  }

  _lockdirBirthMs() {
    try {
      return statSync(this.lockdir).mtimeMs;
    } catch {
      return nowMs(); // un-inspectable => treat as fresh (SAFE side: do not reclaim)
    }
  }

  // Is this lockdir SAFE to reclaim (rename + recursive-remove)? The hard rule: NEVER delete a directory the
  // gate did not create, and NEVER delete a FOREIGN repo's lock. Reclaim requires, on EVERY path
  // (owner present OR owner null/absent):
  //   - the gate-owned sentinel is PRESENT (proves the gate created the dir), AND
  //   - the sentinel's stored repo realpath EQUALS ours (proves it is OUR checkout's lock, not another
  //     checkout sharing a mis-set VERTER_GATE_LOCK).
  // owner.json's repoRealpath, when present, must ALSO match (defence in depth), but the sentinel repo is
  // the authoritative, always-present gate (it is written at acquire before owner.json, and survives a
  // crashed mid-init lock where owner.json never landed). A dir without a parseable sentinel, or whose
  // sentinel repo differs from ours, is NEVER reclaimable — regardless of owner.json presence. Returns
  // { ok, why }.
  _reclaimable(owner) {
    const sentinel = this._readSentinel();
    if (!sentinel.present) {
      return {
        ok: false,
        why:
          `lockdir lacks the gate-owned sentinel (${GATE_LOCK_SENTINEL}) — it is not a directory this gate ` +
          `created; refusing to rename/remove it (a mis-set VERTER_GATE_LOCK/MOM_GATE_LOCK must never delete ` +
          `an arbitrary directory)`,
      };
    }
    // Authoritative gate: the sentinel's stored repo realpath MUST equal ours, on EVERY reclaim path
    // (including owner == null / owner.json absent). A foreign checkout's mid-init sentinel-only lock is
    // NEVER ours to delete.
    if (!sentinel.repoRealpath || sentinel.repoRealpath !== this.ctx.repoRealpath) {
      return {
        ok: false,
        why:
          `lockdir sentinel repoRealpath=${sentinel.repoRealpath || "<missing>"} does not match this repo ` +
          `(${this.ctx.repoRealpath}) — refusing to reclaim another checkout's lock (a foreign-repo or ` +
          `unparseable-sentinel dir is never reclaimed/deleted)`,
      };
    }
    // Defence in depth: when owner.json IS present, its repoRealpath must also match ours (a torn/foreign
    // owner.json under our-repo sentinel is implausible, but we refuse rather than delete on a mismatch).
    if (owner) {
      const ownerRepo = owner.repoRealpath || "";
      if (!ownerRepo || ownerRepo !== this.ctx.repoRealpath) {
        return {
          ok: false,
          why:
            `lockdir owner.json repoRealpath=${ownerRepo || "<missing>"} does not match this repo ` +
            `(${this.ctx.repoRealpath}) — refusing to reclaim another repo's lock`,
        };
      }
    }
    return { ok: true, why: "" };
  }

  // Reclaim a dead/stale lock via atomic rename — ONLY after _reclaimable() confirms the dir is gate-owned
  // AND owned by THIS repo (sentinel repo == ours), on EVERY path. Returns true if we won the reclaim,
  // false if we lost the rename race. NEVER renames/removes a non-reclaimable dir: on a non-reclaimable
  // verdict it sets refuseDetail and returns false WITHOUT touching the filesystem (the caller maps that to
  // LOCK-REFUSED).
  _reclaim(owner) {
    const verdict = this._reclaimable(owner);
    if (!verdict.ok) {
      this.refuseDetail = verdict.why;
      this.reclaimRefused = true;
      return false;
    }
    const stale = `${this.lockdir}.stale.${this.token}`;
    try {
      renameSync(this.lockdir, stale);
    } catch {
      return false; // lost the race to a concurrent reclaimer
    }
    try {
      rmSync(stale, { recursive: true, force: true });
    } catch {
      /* ignore */
    }
    return true;
  }

  async acquire() {
    let reclaimRaces = 0;
    for (;;) {
      // Try to win the slot. mkdir with recursive:false is atomic (EEXIST on contention).
      try {
        mkdirSync(this.lockdir, { recursive: false });
        // Stamp the gate-owned sentinel BEFORE owner.json so a crash mid-acquire still leaves the dir
        // provably gate-created AND stamped with our repo (the mid-init reclaim path keys on this marker's
        // repo, never on a bare rm).
        this._writeSentinel();
        this._writeOwner();
        this.held = true;
        return true;
      } catch (e) {
        if (e.code !== "EEXIST") {
          // e.g. parent dir missing — make the parent and retry once.
          if (e.code === "ENOENT") {
            mkdirSync(dirname(this.lockdir), { recursive: true });
            continue;
          }
          throw e;
        }
      }
      // Held. Classify the holder.
      const owner = this._readOwner();
      if (!owner) {
        // Lockdir exists but no readable owner.json yet => still INITIALIZING. Refuse until past the init
        // grace; only then (if STILL unreadable) is it a crashed mid-init lock to reclaim — and even then
        // ONLY if the sentinel proves it is OUR repo's lock (a foreign mid-init sentinel-only dir is
        // refused, never deleted).
        const ageMs = nowMs() - this._lockdirBirthMs();
        if (ageMs < this.INIT_GRACE_MS) {
          this.refuseDetail = `initializing, no owner.json, age=${Math.round(ageMs / 1000)}s < ${Math.round(this.INIT_GRACE_MS / 1000)}s grace`;
          return false;
        }
        // Past grace, still no owner.json => crashed mid-init OR a non-gate/foreign dir. _reclaim only
        // renames/removes when the gate-owned sentinel is present AND its repo == ours; otherwise it refuses
        // (reclaimRefused) and we map that to LOCK-REFUSED — never deleting an arbitrary or foreign dir.
        if (this._reclaim(null)) continue;
        if (this.reclaimRefused) return false; // non-gate-owned / foreign-repo dir => refuse, do NOT delete
        reclaimRaces++;
        if (reclaimRaces >= this.RECLAIM_RACE_RETRIES) {
          this.refuseDetail = `could not reclaim a crashed mid-init lock after ${reclaimRaces} attempts`;
          return false;
        }
        await delay(this.RECLAIM_RACE_BACKOFF_MS);
        continue;
      }
      const holderPid = owner.pid;
      const holderIdent = owner.processStartIdentity || "";
      if (holderPid && pidAlive(holderPid)) {
        // FAIL CLOSED: an alive holder PID is reclaimed ONLY when PID reuse is PROVEN — i.e. BOTH the
        // stored start-identity and the live start-identity are non-empty AND they differ (the old PID
        // exited and the OS handed its number to an unrelated process). In every other alive case —
        // matching identities, a missing/empty stored identity, an uncheckable live identity, or any
        // identity we cannot positively distinguish — we REFUSE. Reclaiming a live lock would let two gates
        // run concurrently, which is worse than a manual cleanup, so an empty/uncheckable identity is
        // NEVER treated as evidence of PID reuse.
        const liveIdent = procIdentity(holderPid);
        const identityComparison = classifyProcessIdentityComparison(holderIdent, liveIdent);
        const proveReuse = identityComparison.provesReuse;
        if (!proveReuse) {
          const ageS = Math.round((nowMs() - (owner.createdAtMs || this._lockdirBirthMs())) / 1000);
          if (identityComparison.matches) {
            // Identities both present and equal => genuinely the same live holder.
            this.refuseDetail = `live holder pid=${holderPid} age=${ageS}s targetDir=${owner.targetDir || "?"}`;
          } else if (holderIdent && liveIdent && !identityComparison.comparable) {
            this.refuseDetail =
              `holder pid=${holderPid} appears alive but PID reuse cannot be ruled out because the stored ` +
              `and live start identities use different formats — refusing (fail-closed)`;
          } else {
            // One or both identities empty/uncheckable while the PID is alive => fail-closed refusal.
            this.refuseDetail =
              `holder pid=${holderPid} appears alive but PID reuse cannot be ruled out ` +
              `(stored-identity=${holderIdent ? "present" : "missing"}, ` +
              `live-identity=${liveIdent ? "present" : "uncheckable"}) — refusing (fail-closed)`;
          }
          return false;
        }
        // Both identities present and DIFFERENT => proven PID reuse; treat as stale and reclaim.
        warn(
          `lock pid=${holderPid} reused by an unrelated process (identity mismatch) => reclaiming`,
        );
      } else {
        warn(`lock holder pid=${holderPid} is dead/stale => reclaiming`);
      }
      // Reclaim only when the dir is gate-owned (sentinel present) AND owned by THIS repo (sentinel repo ==
      // ours, plus owner.json repo == ours). A foreign-repo or sentinel-less dir => refuse (LOCK-REFUSED),
      // never a bare rm.
      if (this._reclaim(owner)) continue;
      if (this.reclaimRefused) return false; // non-gate-owned / foreign-repo dir => refuse, do NOT delete
      reclaimRaces++;
      if (reclaimRaces >= this.RECLAIM_RACE_RETRIES) {
        this.refuseDetail = `could not acquire lock after ${reclaimRaces} reclaim-race attempts`;
        return false;
      }
      await delay(this.RECLAIM_RACE_BACKOFF_MS);
    }
  }

  release() {
    if (!this.held) return;
    const owner = this._readOwner();
    if (owner && owner.token === this.token) {
      const rel = `${this.lockdir}.release.${this.token}`;
      try {
        renameSync(this.lockdir, rel);
        rmSync(rel, { recursive: true, force: true });
      } catch {
        /* ignore */
      }
    }
    this.held = false;
  }
}

// ----------------------------------------------------------------------------------------------------
// Artifact-progress signature: a cheap fingerprint of the runner-owned target tree that CHANGES while a
// cold build lands .o/.rlib/.d artifacts even when the log emits zero bytes. "<file-count>:<newest-mtime>"
// over files modified in the last ~2 minutes, bounded so the probe is O(seconds). BUILD-phase signal ONLY.
// ----------------------------------------------------------------------------------------------------
export function artifactSignature(dir) {
  if (!dir || !existsSync(dir)) return "0:0";
  const cutoff = nowMs() - 2 * 60 * 1000;
  let count = 0;
  let newest = 0;
  const MAX = 5000;
  const stack = [dir];
  while (stack.length && count < MAX) {
    const cur = stack.pop();
    let entries;
    try {
      entries = readdirSync(cur, { withFileTypes: true });
    } catch {
      continue;
    }
    for (const ent of entries) {
      if (count >= MAX) break;
      const full = join(cur, ent.name);
      if (ent.isDirectory()) {
        stack.push(full);
      } else if (ent.isFile()) {
        let st;
        try {
          st = statSync(full);
        } catch {
          continue;
        }
        if (st.mtimeMs >= cutoff) {
          count++;
          if (st.mtimeMs > newest) newest = st.mtimeMs;
        }
      }
    }
  }
  return `${count}:${Math.floor(newest)}`;
}

// ----------------------------------------------------------------------------------------------------
// Exact multi-registration tree teardown. A registration is identified by a monotonic token plus the
// ChildProcess object that was admitted synchronously. Native snapshots capture descendant creation
// identities; signals are sent only while those exact identities remain live. This handles descendants
// that establish their own POSIX process groups without broad name/path killing or PID-reuse hazards.
// ----------------------------------------------------------------------------------------------------
function exactChildTerminal(registration) {
  const child = registration?.child;
  return Boolean(
    registration?.childClosed ||
    (child &&
      ((child.exitCode !== null && child.exitCode !== undefined) ||
        (child.signalCode !== null && child.signalCode !== undefined))),
  );
}

function processIdentitySnapshot() {
  const rows = new Map();
  if (IS_WINDOWS) {
    const command =
      'Get-CimInstance Win32_Process | ForEach-Object { "$($_.ProcessId),$($_.ParentProcessId),win-start-ms:$([math]::Floor($_.CreationDate.ToUniversalTime().Ticks / 10000))" }';
    const result = spawnSync(
      "powershell.exe",
      ["-NoProfile", "-NonInteractive", "-Command", command],
      {
        encoding: "utf8",
        windowsHide: true,
        timeout: 5_000,
        maxBuffer: 64 * 1024 * 1024,
      },
    );
    if (result.status !== 0 || result.error) {
      return {
        ok: false,
        detail: result.error ? result.error.message : `PowerShell CIM exited ${result.status}`,
        rows,
      };
    }
    for (const line of String(result.stdout || "").split(/\r?\n/)) {
      const match = /^\s*(\d+),(\d+),(.+?)\s*$/.exec(line);
      if (!match) continue;
      rows.set(Number(match[1]), {
        pid: Number(match[1]),
        parentPid: Number(match[2]),
        identity: match[3].trim(),
      });
    }
  } else {
    const result = spawnSync("ps", ["-axww", "-o", "pid=,ppid=,lstart="], {
      encoding: "utf8",
      timeout: 5_000,
      maxBuffer: 64 * 1024 * 1024,
    });
    if (result.status !== 0 || result.error) {
      return {
        ok: false,
        detail: result.error ? result.error.message : `ps exited ${result.status}`,
        rows,
      };
    }
    for (const line of String(result.stdout || "").split(/\r?\n/)) {
      const match = /^\s*(\d+)\s+(\d+)\s+(.+?)\s*$/.exec(line);
      if (!match) continue;
      rows.set(Number(match[1]), {
        pid: Number(match[1]),
        parentPid: Number(match[2]),
        identity: match[3].trim().replace(/\s+/g, " "),
      });
    }
  }
  return { ok: true, rows };
}

export function processForestFromSnapshot(snapshot, registration) {
  if (!snapshot.ok) return { ok: false, detail: snapshot.detail, rows: [] };
  const root = snapshot.rows.get(registration.pid);
  const rootIdentityMismatch = Boolean(
    root && registration.rootIdentity && root.identity !== registration.rootIdentity,
  );
  const included = new Set();
  if (root && !rootIdentityMismatch && !exactChildTerminal(registration)) {
    included.add(registration.pid);
  }
  for (const owned of registration.ownedIdentities || []) {
    const row = snapshot.rows.get(Number(owned.pid));
    if (row?.identity && owned.identity && row.identity === owned.identity) {
      included.add(Number(owned.pid));
    }
  }
  let changed = true;
  while (changed) {
    changed = false;
    for (const [pid, row] of snapshot.rows) {
      if (!included.has(pid) && included.has(row.parentPid)) {
        included.add(pid);
        changed = true;
      }
    }
  }
  const depth = new Map([...included].map((pid) => [pid, pid === registration.pid ? 0 : 1]));
  changed = true;
  while (changed) {
    changed = false;
    for (const pid of included) {
      if (depth.has(pid)) continue;
      const parentDepth = depth.get(snapshot.rows.get(pid)?.parentPid);
      if (parentDepth !== undefined) {
        depth.set(pid, parentDepth + 1);
        changed = true;
      }
    }
  }
  return {
    ok: true,
    rootIdentityMismatch,
    rows: [...included]
      .map((pid) => ({ ...snapshot.rows.get(pid), depth: depth.get(pid) || 0 }))
      .sort((a, b) => b.depth - a.depth || b.pid - a.pid),
  };
}

function exactIdentityAlive(row) {
  const identity = procIdentity(row.pid);
  return Boolean(identity) && identity === row.identity;
}

async function reapRegisteredRootFallback(registration, graceMs, verifyMs) {
  if (!registration.pid) {
    return { reaped: false, confirmedDead: true, wasLive: false };
  }
  if (!registration.rootIdentity) {
    return {
      reaped: false,
      confirmedDead: false,
      wasLive: !exactChildTerminal(registration),
      identityRefused: true,
    };
  }
  if ((registration.ownedIdentities || []).length > 0) {
    // A failed snapshot cannot prove or enumerate retained descendants. Refuse to claim clean teardown,
    // even if the original root is gone or its PID now belongs to a replacement process.
    return { reaped: false, confirmedDead: false, wasLive: true };
  }
  if (exactChildTerminal(registration)) {
    return { reaped: false, confirmedDead: true, wasLive: false };
  }
  const liveIdentity = procIdentity(registration.pid);
  const rootIsExact = Boolean(
    registration.rootIdentity && liveIdentity && liveIdentity === registration.rootIdentity,
  );
  if (!rootIsExact) {
    // The admitted root is gone, reused, or uncheckable. Never turn its numeric PID into authority.
    return {
      reaped: false,
      confirmedDead: !liveIdentity || liveIdentity !== registration.rootIdentity,
      wasLive: false,
      identityRefused: Boolean(liveIdentity),
    };
  }
  const wasLive = true;
  if (IS_WINDOWS) {
    // The ChildProcess object is the exact admission identity even before CIM exposes the new process.
    // /T remains scoped to that registered root's current descendants.
    spawnSync("taskkill.exe", ["/PID", String(registration.pid), "/T", "/F"], {
      stdio: "ignore",
      windowsHide: true,
    });
  } else {
    try {
      registration.child.kill("SIGTERM");
    } catch {
      /* already exited */
    }
    const graceDeadline = nowMs() + Math.max(0, graceMs);
    while (nowMs() < graceDeadline && pidAlive(registration.pid)) await delay(50);
    if (pidAlive(registration.pid)) {
      try {
        registration.child.kill("SIGKILL");
      } catch {
        /* already exited */
      }
    }
  }
  const verifyDeadline = nowMs() + Math.max(0, verifyMs);
  while (nowMs() < verifyDeadline) {
    if (exactChildTerminal(registration) || !pidAlive(registration.pid)) {
      // Windows taskkill /T is a tree primitive. On POSIX a failed snapshot means descendants in their
      // own process groups could not be enumerated, so report that uncertainty rather than claim them dead.
      return { reaped: true, confirmedDead: IS_WINDOWS, wasLive };
    }
    await delay(50);
  }
  return { reaped: true, confirmedDead: false, wasLive };
}

async function reapRegisteredForest(registration, graceMs, verifyMs = 4_000) {
  if (!registration.pid) return { reaped: false, confirmedDead: true, wasLive: false };
  let forest = processForestFromSnapshot(processIdentitySnapshot(), registration);
  if (!forest.ok) {
    return reapRegisteredRootFallback(registration, graceMs, verifyMs);
  }
  if (forest.rootIdentityMismatch && forest.rows.length === 0) {
    return { reaped: false, confirmedDead: true, wasLive: false, identityRefused: true };
  }
  if (forest.rows.length === 0) {
    if (!registration.rootIdentity) {
      return reapRegisteredRootFallback(registration, graceMs, verifyMs);
    }
    if (exactChildTerminal(registration) || !pidAlive(registration.pid)) {
      return { reaped: false, confirmedDead: true, wasLive: false };
    }
    // Newly spawned roots can briefly precede their process-table row. Retry once, then use only the exact
    // ChildProcess identity as a fail-safe so close cannot hang without signalling anything.
    await delay(25);
    forest = processForestFromSnapshot(processIdentitySnapshot(), registration);
    if (forest.rootIdentityMismatch && forest.rows.length === 0) {
      return { reaped: false, confirmedDead: true, wasLive: false, identityRefused: true };
    }
    if (!forest.ok || forest.rows.length === 0) {
      return reapRegisteredRootFallback(registration, graceMs, verifyMs);
    }
  }
  const wasLive = true;
  const known = new Map(forest.rows.map((row) => [row.pid, row]));

  if (IS_WINDOWS) {
    // Each taskkill tree root is revalidated immediately before signalling. Retained descendants remain
    // individually authoritative even after the admitted root has exited or its PID has been reused.
    for (const row of forest.rows) {
      if (!exactIdentityAlive(row)) continue;
      spawnSync("taskkill.exe", ["/PID", String(row.pid), "/T", "/F"], {
        stdio: "ignore",
        windowsHide: true,
      });
    }
  } else {
    for (const row of forest.rows) {
      if (!exactIdentityAlive(row)) continue;
      try {
        process.kill(row.pid, "SIGTERM");
      } catch {
        /* already exited */
      }
    }
    const gracefulDeadline = nowMs() + Math.max(0, graceMs);
    while (nowMs() < gracefulDeadline) {
      if ([...known.values()].every((row) => !exactIdentityAlive(row))) break;
      await delay(50);
    }
    forest = processForestFromSnapshot(processIdentitySnapshot(), registration);
    if (forest.ok) {
      for (const row of forest.rows) known.set(row.pid, row);
    }
    for (const row of [...known.values()].sort((a, b) => b.depth - a.depth || b.pid - a.pid)) {
      if (!exactIdentityAlive(row)) continue;
      try {
        process.kill(row.pid, "SIGKILL");
      } catch {
        /* already exited */
      }
    }
  }

  const verifyDeadline = nowMs() + Math.max(0, verifyMs);
  while (nowMs() < verifyDeadline) {
    if ([...known.values()].every((row) => !exactIdentityAlive(row))) {
      return { reaped: true, confirmedDead: true, wasLive };
    }
    await delay(50);
  }
  return {
    reaped: true,
    confirmedDead: [...known.values()].every((row) => !exactIdentityAlive(row)),
    wasLive,
  };
}

function cancelledStepResult(reason = "CANCELLED", cancellationReason = "") {
  return {
    code: 128,
    reason,
    cancellationReason,
    durationMs: 0,
    stdout: "",
    stderr: "",
    spawnError: false,
    reapConfirmedDead: true,
    signalName: "",
    peakRssBytes: 0,
    memoryLimitBytes: 0,
    peakRssProcessCount: 0,
    memoryProcessCount: 0,
    memorySampleFailures: 0,
    memorySampleFailureDetail: "",
  };
}

function normalizedProvenanceRoot(root, windows) {
  if (typeof root !== "string" || root.trim() === "") {
    throw new TypeError("supervisor ownership roots must be non-empty paths");
  }
  const pathApi = windows ? win32 : posix;
  let path = pathApi.normalize(root);
  const filesystemRoot = pathApi.parse(path).root;
  while (path.length > filesystemRoot.length && path.endsWith(pathApi.sep)) {
    path = path.slice(0, -1);
  }
  return { path, key: windows ? path.toLowerCase() : path };
}

// Reduce exact runner-owned provenance authorities without inventing a broader common parent. Containment
// is path-segment-aware (`gate-runner` never owns sibling `gate-runner2`) and Windows comparisons follow the
// platform's case-insensitive path semantics. The first normalized spelling of each retained input wins.
export function minimizeProvenanceRoots(roots, { windows = IS_WINDOWS } = {}) {
  if (!Array.isArray(roots)) throw new TypeError("provenance roots must be an array");
  const pathApi = windows ? win32 : posix;
  const unique = [];
  const seen = new Set();
  for (const root of roots) {
    const normalized = normalizedProvenanceRoot(root, windows);
    if (seen.has(normalized.key)) continue;
    seen.add(normalized.key);
    unique.push(normalized);
  }
  return unique
    .filter((candidate, candidateIndex) =>
      unique.every((ancestor, ancestorIndex) => {
        if (ancestorIndex === candidateIndex) return true;
        const relativePath = pathApi.relative(ancestor.key, candidate.key);
        const contained =
          relativePath === "" ||
          (relativePath !== ".." &&
            !relativePath.startsWith(`..${pathApi.sep}`) &&
            !pathApi.isAbsolute(relativePath));
        return !contained;
      }),
    )
    .map((entry) => entry.path);
}

// One gate-owned authority for every registered process forest. Production currently admits steps
// through this supervisor; post-list Surface/shipped overlap uses the same authority and polling interval.
export function createGateRunSupervisor(options = {}) {
  const {
    deadlineMs = 0,
    stallMs = 0,
    memoryLimitBytes = 0,
    memoryPollMs = 1000,
    memorySampleFailureLimit = 3,
    killGraceMs = 5000,
    memoryKillGraceMs = MEMORY_KILL_GRACE_MS,
    now = nowMs,
    setIntervalFn = setInterval,
    clearIntervalFn = clearInterval,
    spawnFn = spawn,
    processIdentityFn = null,
    processAliveFn = pidAlive,
    sampleProcessForestRssFn = sampleProcessForestRssBytes,
    reapRegisteredForestFn = reapRegisteredForest,
    provenanceSweepFn = provenanceSweep,
    artifactSignatureFn = artifactSignature,
    beforeCloseSnapshot = null,
    ownershipRoots = [],
  } = options;

  let nextTokenId = 1;
  let closing = false;
  let closePromise = null;
  let globalAbortReason = "";
  const cancelledLanes = new Map();
  const active = new Map();
  const forests = new Map();
  const knownRegistrations = new Map();
  const knownOwnershipRoots = new Map();
  for (const root of ownershipRoots) {
    const normalized = normalizedProvenanceRoot(root, IS_WINDOWS);
    knownOwnershipRoots.set(normalized.key, normalized.path);
  }
  let lastProgressMs = now();
  let progressFingerprint = "";
  let lastMemorySampleMs = Number.NEGATIVE_INFINITY;
  let memorySampleFailures = 0;
  let memorySampleFailureDetail = "";
  let aggregatePeakRssBytes = 0;
  let aggregatePeakProcessCount = 0;
  let aggregatePeakPerLane = {};
  const perLane = {};
  let watchdogRunning = false;

  const ensureLane = (laneId) => {
    if (!perLane[laneId]) {
      perLane[laneId] = {
        peakRssBytes: 0,
        peakRssProcessCount: 0,
        lastRssBytes: 0,
        lastProcessCount: 0,
      };
    }
    return perLane[laneId];
  };

  const retire = (entry) => {
    if (active.get(entry.tokenId) !== entry) return;
    active.delete(entry.tokenId);
    lastProgressMs = now();
    progressFingerprint = "";
  };

  const refuseUnpublishedIdentity = (entry) => {
    if (entry.identityPublished) return false;
    entry.identityPending = false;
    entry.rootIdentity = "";
    entry.terminalBeforeIdentity = true;
    forests.delete(entry.tokenId);
    knownRegistrations.delete(entry.tokenId);
    return true;
  };

  const refuseUnpublishedTerminal = (entry) =>
    exactChildTerminal(entry) && refuseUnpublishedIdentity(entry);

  const requestAbort = (entry, reason, cancellationReason = "") => {
    if (entry.abortPromise) return entry.abortPromise;
    entry.reason = reason;
    entry.cancellationReason = cancellationReason;
    entry.watchdogSignaledLive = !exactChildTerminal(entry) && groupOrPidAlive(entry.pid);
    const grace =
      reason === "MEMORY" || reason === "MEMORY_MONITOR" ? memoryKillGraceMs : entry.killGraceMs;
    entry.abortPromise = (async () => {
      refuseUnpublishedTerminal(entry);
      if (entry.identityPending) {
        const sample = sampleProcessForestRssBytes([
          {
            tokenId: entry.tokenId,
            laneId: entry.laneId,
            pid: entry.pid,
            pendingIdentity: true,
            ownedIdentities: entry.ownedIdentities,
            closed: exactChildTerminal(entry) || !processAliveFn(entry.pid),
          },
        ]);
        const identity = sample.ok
          ? sample.perRoot?.[0]?.identities?.find((row) => row.pid === entry.pid)?.identity || ""
          : "";
        if (refuseUnpublishedTerminal(entry)) {
          // The admitted ChildProcess became terminal while the native snapshot was in flight.
        } else if (identity) {
          entry.rootIdentity = identity;
          entry.identityPending = false;
          entry.identityPublished = true;
          knownRegistrations.set(entry.tokenId, entry);
        } else if (exactChildTerminal(entry) || !processAliveFn(entry.pid)) {
          refuseUnpublishedIdentity(entry);
        }
      }
      if (entry.terminalBeforeIdentity) {
        entry.reapConfirmedDead = true;
        return {
          reaped: false,
          confirmedDead: true,
          wasLive: false,
          terminalBeforeIdentity: true,
        };
      }
      if (!entry.rootIdentity) {
        entry.reapConfirmedDead = false;
        return { reaped: false, confirmedDead: false, wasLive: false, identityRefused: true };
      }
      let outcome;
      try {
        outcome = await reapRegisteredForestFn(entry, grace);
      } catch {
        outcome = { reaped: true, confirmedDead: false, wasLive: true };
      }
      entry.reapConfirmedDead = outcome.confirmedDead;
      if (typeof outcome.wasLive === "boolean") entry.watchdogSignaledLive = outcome.wasLive;
      return outcome;
    })();
    return entry.abortPromise;
  };

  const abortAll = (reason, cancellationReason = "") => {
    if (reason !== "CANCELLED") {
      globalAbortReason = reason;
      closing = true;
    }
    return Promise.all(
      [...forests.values()].map((entry) => requestAbort(entry, reason, cancellationReason)),
    );
  };

  const watchdogTick = async () => {
    if (watchdogRunning) return;
    watchdogRunning = true;
    try {
      if (globalAbortReason) return;
      const cur = now();
      const registrations = [...forests.values()];
      if (registrations.length === 0) return;

      if (memoryLimitBytes > 0 && cur - lastMemorySampleMs >= memoryPollMs) {
        lastMemorySampleMs = cur;
        const roots = registrations.map((entry) => ({
          tokenId: entry.tokenId,
          laneId: entry.laneId,
          pid: entry.pid,
          pendingIdentity: entry.identityPending,
          ...(entry.identityPending ? {} : { identity: entry.rootIdentity }),
          ownedIdentities: entry.ownedIdentities,
          // A short command can exit while the one native OS snapshot is running, before Node dispatches
          // its close event. Mark a now-dead native root closed so strict forest parsing does not turn that
          // benign sampling race into three MEMORY_MONITOR failures. Injected samplers retain their exact
          // scripted roots (fake pids are intentionally not OS-live).
          closed:
            exactChildTerminal(entry) ||
            (sampleProcessForestRssFn === sampleProcessForestRssBytes && !pidAlive(entry.pid)),
        }));
        let sample;
        try {
          sample = sampleProcessForestRssFn(roots);
          if (sample && typeof sample.then === "function") sample = await sample;
        } catch (error) {
          sample = { ok: false, detail: error?.message || String(error) };
        }
        if (sample?.ok) {
          memorySampleFailures = 0;
          memorySampleFailureDetail = "";
          for (const entry of registrations) {
            const row = sample.perRoot?.find((candidate) => candidate.tokenId === entry.tokenId);
            if (entry.identityPending) {
              const rootIdentity = row?.identities?.find(
                (identity) => identity.pid === entry.pid && identity.identity,
              );
              if (refuseUnpublishedTerminal(entry)) {
                // The exact child exited while this sample was in flight; never publish the sampled PID.
              } else if (rootIdentity) {
                entry.rootIdentity = rootIdentity.identity;
                entry.identityPending = false;
                entry.identityPublished = true;
                knownRegistrations.set(entry.tokenId, entry);
              }
            }
            if (!entry.terminalBeforeIdentity && Array.isArray(row?.identities)) {
              entry.ownedIdentities = row.identities.filter(
                (identity) =>
                  identity.pid !== entry.pid && identity.identity && Number.isInteger(identity.pid),
              );
            }
            if (exactChildTerminal(entry) && (!row || (row.processCount || 0) === 0)) {
              forests.delete(entry.tokenId);
            }
            if (!row) continue;
            entry.memoryProcessCount = row.processCount || 0;
            if ((row.rssBytes || 0) >= entry.peakRssBytes) {
              entry.peakRssBytes = row.rssBytes || 0;
              entry.peakRssProcessCount = row.processCount || 0;
            }
          }
          for (const [laneId, contribution] of Object.entries(sample.perLane || {})) {
            const lane = ensureLane(laneId);
            lane.lastRssBytes = contribution.rssBytes || 0;
            lane.lastProcessCount = contribution.processCount || 0;
            if (lane.lastRssBytes >= lane.peakRssBytes) {
              lane.peakRssBytes = lane.lastRssBytes;
              lane.peakRssProcessCount = lane.lastProcessCount;
            }
          }
          if ((sample.rssBytes || 0) >= aggregatePeakRssBytes) {
            aggregatePeakRssBytes = sample.rssBytes || 0;
            aggregatePeakProcessCount = sample.processCount || 0;
            aggregatePeakPerLane = Object.fromEntries(
              Object.entries(sample.perLane || {}).map(([laneId, contribution]) => [
                laneId,
                { ...contribution },
              ]),
            );
          }
          if ((sample.rssBytes || 0) >= memoryLimitBytes) {
            err(
              `ABORTED — memory ceiling: aggregate active process-forest RSS ${formatMemorySize(
                sample.rssBytes,
              )} across ${sample.processCount || 0} process(es) reached the ${formatMemorySize(
                memoryLimitBytes,
              )} limit; terminating every registered process tree. No gate verdict was produced.`,
            );
            await abortAll("MEMORY");
            return;
          }
        } else {
          memorySampleFailures += 1;
          memorySampleFailureDetail = sample?.detail || "unknown sampler failure";
          if (memorySampleFailures >= memorySampleFailureLimit) {
            err(
              `ABORTED — memory safety monitor unavailable after ${memorySampleFailures} consecutive ` +
                `samples (${memorySampleFailureDetail}); terminating every registered process tree rather ` +
                "than running without an enforceable ceiling. No gate verdict was produced.",
            );
            await abortAll("MEMORY_MONITOR");
            return;
          }
        }
      }

      if (deadlineMs > 0 && cur >= deadlineMs) {
        await abortAll("TIMEOUT");
        return;
      }

      if (stallMs > 0) {
        const vector = [...active.values()]
          .map((entry) => {
            const artifact = entry.phase === "build" ? artifactSignatureFn(entry.targetDir) : "";
            return `${entry.tokenId}:${entry.totalBytes}:${artifact}`;
          })
          .sort()
          .join("|");
        if (vector !== progressFingerprint) {
          progressFingerprint = vector;
          lastProgressMs = cur;
        } else if (cur - lastProgressMs >= stallMs) {
          await abortAll("STALL");
        }
      }
    } finally {
      watchdogRunning = false;
    }
  };

  const intervalMs = memoryLimitBytes > 0 ? Math.max(50, Math.min(1000, memoryPollMs)) : 1000;
  const watchdog = setIntervalFn(watchdogTick, intervalMs);

  const supervisor = {
    runStep(laneId, stepOptions) {
      const deniedReason =
        closing || globalAbortReason
          ? globalAbortReason || "CANCELLED"
          : cancelledLanes.has(laneId)
            ? "CANCELLED"
            : deadlineMs > 0 && now() >= deadlineMs
              ? "TIMEOUT"
              : "";
      if (deniedReason) {
        return Promise.resolve(
          cancelledStepResult(
            deniedReason === "TIMEOUT" ? "TIMEOUT" : "CANCELLED",
            cancelledLanes.get(laneId) || globalAbortReason || "SUPERVISOR_CLOSED",
          ),
        );
      }

      const tokenId = nextTokenId++;
      const {
        cmd = stepOptions.command,
        args = [],
        cwd,
        env,
        phase = "test",
        targetDir,
        captureStdoutSeparately = false,
        windowsVerbatimArguments = false,
        mirrorOutput = true,
      } = stepOptions;
      let child;
      const startMs = now();
      try {
        child = spawnFn(cmd, args, {
          cwd,
          env,
          shell: false,
          detached: !IS_WINDOWS,
          windowsHide: true,
          windowsVerbatimArguments,
          stdio: ["ignore", "pipe", "pipe"],
        });
      } catch (error) {
        return Promise.resolve({
          ...cancelledStepResult("", ""),
          code: 127,
          spawnError: true,
          stderr: error?.message || String(error),
          reapConfirmedDead: undefined,
        });
      }

      const identityPending = processIdentityFn === null;
      let identityProbe = "";
      if (!identityPending) {
        try {
          identityProbe = child.pid ? processIdentityFn(child.pid) : "";
        } catch {
          identityProbe = "";
        }
      }
      const rootIdentity = String(identityProbe || "");

      const entry = {
        tokenId,
        laneId,
        name: stepOptions.name || cmd,
        pid: child.pid,
        // The exact ChildProcess is pending admission authority until the first gate-owned process-table
        // snapshot publishes its start identity. No numeric PID signal is permitted while it is pending;
        // every later snapshot/reap must match the published identity.
        rootIdentity,
        ownedIdentities: [],
        identityPending,
        identityPublished: false,
        identityPublication: null,
        terminalBeforeIdentity: false,
        child,
        phase,
        targetDir,
        killGraceMs: stepOptions.killGraceMs ?? killGraceMs,
        stdoutBuf: "",
        stderrBuf: "",
        totalBytes: 0,
        peakRssBytes: 0,
        peakRssProcessCount: 0,
        memoryProcessCount: 0,
        reason: "",
        cancellationReason: "",
        watchdogSignaledLive: false,
        reapConfirmedDead: undefined,
        abortPromise: null,
        childClosed: false,
      };
      active.set(tokenId, entry);
      forests.set(tokenId, entry);
      ensureLane(laneId);
      lastProgressMs = now();
      progressFingerprint = "";

      child.stdout?.on("data", (data) => {
        const value = data.toString();
        entry.totalBytes += data.length;
        entry.stdoutBuf += value;
        if (mirrorOutput && !captureStdoutSeparately) process.stderr.write(value);
      });
      child.stderr?.on("data", (data) => {
        const value = data.toString();
        entry.totalBytes += data.length;
        entry.stderrBuf += value;
        if (mirrorOutput) process.stderr.write(value);
      });

      let spawnError = false;
      let signalName = "";
      let resolved = false;
      const completion = new Promise((resolve) => {
        const finish = (code) => {
          if (resolved) return;
          resolved = true;
          resolve(code);
        };
        child.on("error", () => {
          spawnError = true;
          entry.childClosed = true;
          refuseUnpublishedTerminal(entry);
          finish(127);
        });
        child.on("close", (code, signal) => {
          entry.childClosed = true;
          refuseUnpublishedTerminal(entry);
          if (code === null && signal) {
            signalName = signal;
            finish(128);
          } else {
            finish(code === null ? 1 : code);
          }
        });
      });

      const publishIdentity = async () => {
        let published = entry.rootIdentity;
        const terminal = exactChildTerminal(entry) || !child.pid || !processAliveFn(child.pid);
        if (published && !terminal) {
          entry.rootIdentity = published;
          entry.identityPublished = true;
          knownRegistrations.set(tokenId, entry);
          return;
        }
        if (terminal) {
          // The exact admitted ChildProcess finished before identity publication. It can report its real
          // status, but it never becomes numeric-PID reaper or provenance authority.
          refuseUnpublishedIdentity(entry);
          return;
        }
        globalAbortReason = "MEMORY_MONITOR";
        closing = true;
        memorySampleFailures = Math.max(memorySampleFailures, memorySampleFailureLimit);
        memorySampleFailureDetail = `spawned live root pid ${entry.pid || "<missing>"} has no checkable process identity`;
        queueMicrotask(() => abortAll("MEMORY_MONITOR"));
      };
      entry.identityPublication = entry.identityPending ? Promise.resolve() : publishIdentity();

      entry.settled = (async () => {
        const code = await completion;
        if (!entry.terminalBeforeIdentity) await entry.identityPublication;
        if (entry.abortPromise) {
          try {
            await entry.abortPromise;
          } catch {
            /* best effort reap */
          }
        }
        if (
          (entry.reason === "TIMEOUT" || entry.reason === "STALL") &&
          code === 0 &&
          !entry.watchdogSignaledLive
        ) {
          entry.reason = "";
        }
        retire(entry);
        return {
          code,
          reason: entry.reason,
          cancellationReason: entry.cancellationReason,
          durationMs: now() - startMs,
          stdout: entry.stdoutBuf,
          stderr: entry.stderrBuf,
          spawnError,
          reapConfirmedDead: entry.reapConfirmedDead,
          signalName,
          peakRssBytes: entry.peakRssBytes,
          memoryLimitBytes,
          peakRssProcessCount: entry.peakRssProcessCount,
          memoryProcessCount: entry.memoryProcessCount,
          memorySampleFailures,
          memorySampleFailureDetail,
        };
      })();
      return entry.settled;
    },

    async cancelLane(laneId, reason = "LANE_CANCELLED") {
      cancelledLanes.set(laneId, reason);
      const entries = [...forests.values()].filter((entry) => entry.laneId === laneId);
      const outcomes = await Promise.all(
        entries.map((entry) => requestAbort(entry, "CANCELLED", reason)),
      );
      return {
        laneId,
        reason,
        reaped: outcomes.some((outcome) => outcome.reaped),
        confirmedDead: outcomes.every((outcome) => outcome.confirmedDead),
      };
    },

    closeAndReapAll(reason = "SUPERVISOR_CLOSED") {
      if (closePromise) return closePromise;
      closing = true;
      closePromise = (async () => {
        if (beforeCloseSnapshot) await beforeCloseSnapshot();
        const entries = [...active.values()];
        const outcomes = await Promise.all(
          entries.map((entry) => requestAbort(entry, "CANCELLED", reason)),
        );
        await Promise.allSettled(entries.map((entry) => entry.settled));
        for (const entry of forests.values()) {
          if (!entry.pid) continue;
          const outcome = await requestAbort(entry, "CANCELLED", reason);
          outcomes.push(outcome);
        }
        const targets = new Map(
          [...knownOwnershipRoots].map(([key, targetDir]) => [
            key,
            { targetDir, graceMs: killGraceMs },
          ]),
        );
        for (const entry of knownRegistrations.values()) {
          if (!entry.targetDir) continue;
          const normalized = normalizedProvenanceRoot(entry.targetDir, IS_WINDOWS);
          if (targets.has(normalized.key)) continue;
          targets.set(normalized.key, {
            targetDir: normalized.path,
            graceMs: entry.killGraceMs,
          });
        }
        const minimizedTargets = minimizeProvenanceRoots(
          [...targets.values()].map(({ targetDir }) => targetDir),
        );
        for (const targetDir of minimizedTargets) {
          const { key } = normalizedProvenanceRoot(targetDir, IS_WINDOWS);
          const { graceMs } = targets.get(key);
          try {
            await provenanceSweepFn(targetDir, graceMs);
          } catch {
            /* best effort within each exact runner-owned target root */
          }
        }
        clearIntervalFn(watchdog);
        return {
          reason,
          reaped: outcomes.some((outcome) => outcome.reaped),
          confirmedDead: outcomes.every((outcome) => outcome.confirmedDead),
        };
      })();
      return closePromise;
    },

    snapshotTelemetry() {
      return {
        closing,
        globalAbortReason,
        deadlineMs,
        stallMs,
        memoryLimitBytes,
        aggregatePeakRssBytes,
        aggregatePeakProcessCount,
        aggregatePeakPerLane: structuredClone(aggregatePeakPerLane),
        perLane: structuredClone(perLane),
        memorySampleFailures,
        memorySampleFailureDetail,
        active: [...active.values()].map((entry) => ({
          tokenId: entry.tokenId,
          laneId: entry.laneId,
          pid: entry.pid,
          name: entry.name,
          rootIdentity: entry.rootIdentity,
        })),
        forests: [...forests.values()].map((entry) => ({
          tokenId: entry.tokenId,
          laneId: entry.laneId,
          rootPid: entry.pid,
          rootIdentity: entry.rootIdentity,
          childClosed: entry.childClosed,
          ownedIdentityCount: entry.ownedIdentities.length,
        })),
        cancelledLanes: Object.fromEntries(cancelledLanes),
      };
    },
  };

  return supervisor;
}

// Compatibility/self-test entry point over the same multi-registration engine.
export async function runContainedStep(opts) {
  const {
    deadlineMs = 0,
    stallMs = 0,
    memoryLimitBytes = 0,
    memoryPollMs = 1000,
    memorySampleFailureLimit = 3,
    memorySampler = sampleProcessTreeRssBytes,
    killGraceMs = 5000,
    memoryKillGraceMs = MEMORY_KILL_GRACE_MS,
    processIdentityFn = null,
    processAliveFn = pidAlive,
  } = opts;
  const sampleProcessForestRssFn =
    memorySampler === sampleProcessTreeRssBytes
      ? sampleProcessForestRssBytes
      : (roots) => {
          const root = roots[0];
          const sample = memorySampler(root?.pid);
          if (!sample?.ok) return sample;
          const perRoot = root
            ? [{ ...root, rssBytes: sample.rssBytes || 0, processCount: sample.processCount || 0 }]
            : [];
          return {
            ok: true,
            rssBytes: sample.rssBytes || 0,
            processCount: sample.processCount || 0,
            perRoot,
            perLane: root
              ? {
                  [root.laneId]: {
                    rssBytes: sample.rssBytes || 0,
                    processCount: sample.processCount || 0,
                  },
                }
              : {},
          };
        };
  const supervisor = createGateRunSupervisor({
    deadlineMs,
    stallMs,
    memoryLimitBytes,
    memoryPollMs,
    memorySampleFailureLimit,
    killGraceMs,
    memoryKillGraceMs,
    sampleProcessForestRssFn,
    processIdentityFn,
    processAliveFn,
  });
  try {
    return await supervisor.runStep("single-step", opts);
  } finally {
    await supervisor.closeAndReapAll("SINGLE_STEP_FINALLY");
  }
}

// nextest result-line parsing.
//
// nextest prints one terminal status per test: "    <STATUS> [   0.123s] <binary> <test::path::name>".
// A plain assertion failure is `FAIL`, but a CRASH renders under a DIFFERENT status — a signal abort
// (`SIGABRT` / `SIGSEGV` / `SIGBUS` / `SIGILL` / `SIGFPE` / `ABORT`), a leak (`LEAK` under
// leak-fail-mode / `LEAK-FAIL`), or a `TIMEOUT`. Those are NOT `FAIL` lines yet nextest still counts them
// in its summary `failed` total and exits non-zero. Parsing only `FAIL [` would let an aborting/leaking
// test in ANY crate pass the gate green, so the live SURFACE-1 path treats the summary `failed` count +
// the run exit code as authoritative (see analyzeNextestSurface), and the classifier below recognizes the
// non-`FAIL` failure statuses too so the in-process classifier agrees with the live path.
// ----------------------------------------------------------------------------------------------------

// Terminal status tokens nextest uses for a FAILED test — a test that did NOT pass. Informational
// prefixes (SLOW / TRY / RETRY / START / SETUP / TERMINATING) are NOT terminal statuses and are excluded.
//
// `LEAK` is deliberately NOT here. Outside leak-fail-mode nextest reports a test that leaked a handle or
// subprocess as `LEAK [ … ]` while COUNTING IT AS PASSED — the summary reads `… 2 passed (1 leaky) …` and
// the run exits 0. Treating `LEAK` as a failure turns a green run red (a false positive this repo would
// hit: several suites launch tsgo/tsserver children). Leak-fail-mode renders the DISTINCT `LEAK-FAIL`
// status, which IS a failure and IS in the set.
export const NEXTEST_FAILURE_STATUSES = new Set([
  "FAIL",
  "ABORT",
  "SIGABRT",
  "SIGSEGV",
  "SIGBUS",
  "SIGILL",
  "SIGFPE",
  "SIGHUP",
  "SIGINT",
  "SIGQUIT",
  "SIGTERM",
  "SIGKILL",
  "LEAK-FAIL",
  "TIMEOUT",
  // Abbreviated spellings nextest uses in its compact status column. Taken from the status literal blob
  // in the cargo-nextest binary itself (`status: LK FAIL FL+LK TMT TM PASS`), not inferred: `FL` is the
  // abbreviated FAIL (observed live as the compound `FL+LK`, which decomposes to `FL` + `LK`), and
  // `TMT`/`TM` are the abbreviated TIMEOUT. `LK` and `PASS` are NOT here - see the passing-token set.
  "FL",
  "TMT",
  "TM",
]);

// Terminal statuses for a test that DID pass. `XFAIL` is an EXPECTED failure, i.e. a success.
const NEXTEST_PASSING_STATUS_TOKENS = new Set(["PASS", "LEAK", "LK", "XFAIL"]);

// Non-terminal progress statuses. They are parsed (so the resolution below sees them in order) but never
// classify as a failure; a real terminal line for the same test always follows.
const NEXTEST_PROGRESS_STATUS_TOKENS = new Set(["SLOW", "TERMINATING", "START", "SETUP"]);

// nextest right-aligns the status into a FIXED-WIDTH column, so the text before the timing bracket is
// always exactly this many characters. Verified 44/44 across every real cargo-nextest 0.9.130 log captured
// for this work, from a 2-test run to a 41-test run:
//
//     "        PASS [   0.015s] (1/5) gb6status::t plain_pass"        8 spaces + "PASS"    = 12
//     " FAIL + LEAK [   1.019s] (4/5) gb6status::t leak_and_fail"     1 space  + 11 chars  = 12
//     "  TRY 3 FAIL [   0.008s] (2/3) gb6status::t always_fails"      2 spaces + 10 chars  = 12
//     " TERMINATING [>  2.000s] (___) gb6status::t hangs"             1 space  + 11 chars  = 12
//
// Requiring the EXACT width is a cheap, load-bearing guard against captured test OUTPUT impersonating a
// status line: nextest indents captured output by 4 spaces, so an echoed status line lands at width 16 and
// is rejected outright. It is a hardening layer, NOT the correctness argument - see the transition rule
// below for that. If a future nextest changes this width the failure mode is LOUD, never silent: no line
// parses and every failure falls through to the unaccounted tripwire, so a run WITH failures goes red with
// an opaque reason rather than green. Stated precisely, because the weaker half matters: a fully GREEN
// run would still pass with zero named lines, since there is nothing to name. The loudness is on failing
// runs only - it is not a claim that a width change is self-announcing on a passing tree.
const NEXTEST_STATUS_FIELD_WIDTH = 12;

// The status LINE grammar: <status field, right-aligned to NEXTEST_STATUS_FIELD_WIDTH> [<timing>] <rest>.
// The field is NOT a single uppercase token (which is what a `^([A-Z][A-Z-]*) \[` scan assumed) - it may
// carry spaces, digits and `+`, as the compound and retry spellings above show.
const NEXTEST_STATUS_LINE = /^(.{12}) \[[^\]]*\]\s+(.+)$/;
const NEXTEST_STATUS_FIELD = /^\s*[A-Z][A-Z0-9+\-/ ]*$/;

// Classify a raw status FIELD as "fail" | "pass" | "progress" | "unknown".
//
// A `TRY <n> ` prefix is stripped first: it names the ATTEMPT, not the outcome. The remainder splits on
// whitespace and `+`, so `FAIL + LEAK` and its abbreviation `FL+LK` both decompose, and any failure token
// makes the whole field a failure.
//
// An UNRECOGNIZED field returns "unknown" and is deliberately NOT named. This is the honest boundary of
// the naming claim: the summary-derived `runCount - passed` count remains the authority for WHETHER a
// test failed, so a status spelling this parser has never seen still fails the gate through the
// unaccounted tripwire, just without its name. Guessing here could only add a false positive on a green
// run; it could never hide a failure.
function classifyNextestStatusField(field) {
  const bare = field.replace(/^TRY\s+\d+\s+/, "").trim();
  if (!bare) return "unknown";
  const tokens = bare.split(/[\s+]+/).filter(Boolean);
  let sawPass = false;
  let sawProgress = false;
  for (const tok of tokens) {
    if (tok.startsWith("SIG")) return "fail";
    if (NEXTEST_FAILURE_STATUSES.has(tok)) return "fail";
    if (NEXTEST_PASSING_STATUS_TOKENS.has(tok)) sawPass = true;
    else if (NEXTEST_PROGRESS_STATUS_TOKENS.has(tok)) sawProgress = true;
    else return "unknown";
  }
  if (sawPass) return "pass";
  return sawProgress ? "progress" : "unknown";
}

// The attempt number of a `TRY <n> …` field, or null when the field is not retry-tagged.
function nextestRetryAttempt(field) {
  const m = /^TRY\s+(\d+)\s+/.exec(field.trim());
  return m ? parseInt(m[1], 10) : null;
}

// May `next` replace `prev` as a test's terminal status?
//
// A HARDENING RULE, NOT THE LOAD-BEARING ONE. Read this before relying on it: it stops the BARE-`PASS`
// forgery only, and it does NOT stop a forged retry. The parser reads a stream that is not exclusively
// the runner's - nextest relays a test's CAPTURED OUTPUT alongside its own status lines - so a test can
// print BOTH halves of a `TRY 1 FAIL` / `TRY 2 PASS` pair, which is exactly the transition this rule
// permits. That was demonstrated, twice, with two different token spellings.
//
// What actually holds the verdict: the summary-derived COUNT (a cleared failure leaves a shortfall) and,
// for the one path that turns `failures exist` into green, TOLERANCE-REFUSAL whenever any failure was
// superseded by a pass. See analyzeNextestSurface and GI-19.
//
// The rule itself: a FAILURE is not cleared by a PASS unless the transition looks like a RETRY - both
// sides `TRY <n>`-tagged with a strictly INCREASING attempt number - which is the only fail-to-pass
// transition nextest itself produces. It is kept because it raises the cost of the trivial forgery and
// because a blanket "a PASS may never follow a FAIL" rule would break the genuine flaky case. With
// `NEXTEST_RETRIES` now pinned to 0 by buildCargoEnv, no GENUINE supersession occurs in this gate at all.
//
// Every other transition stays permissive, because none of them can HIDE a failure: progress -> anything,
// pass -> fail (surfacing a failure is always allowed; an invented one is caught by the named-count
// reconciliation), and fail -> fail (a later attempt or the trailing recap restating the same failure).
function nextestStatusSupersedes(prev, next) {
  if (prev.kind !== "fail") return true;
  if (next.kind === "fail") return true;
  if (next.kind === "progress") return false;
  const prevTry = nextestRetryAttempt(prev.status);
  const nextTry = nextestRetryAttempt(next.status);
  return prevTry !== null && nextTry !== null && nextTry > prevTry;
}

// Every terminal failure in a nextest log as `{ status, name, binaryId }`, across EVERY failure status
// (not just `FAIL`). `name` is the EXACT test name (final whitespace token after the timing bracket).
//
// Tests are keyed by `<binary-id> <test-name>`, so one test name appearing in two binaries stays two
// distinct tests. Resolution is last-status-per-test SUBJECT TO `nextestStatusSupersedes`, which is what
// makes retries, progress lines and the trailing recap all resolve correctly: `SLOW` then `PASS`,
// `TERMINATING` then `TIMEOUT`, `TRY 1 FAIL` / `TRY 2 FAIL` / `TRY 3 FAIL` collapsing to ONE failure, and
// the recap restating a failure onto the same entry.
export function extractNextestTerminalFailures(text) {
  const terminal = new Map();
  // Every test whose FAILURE was superseded by a PASS. Captured output can supply BOTH sides of a
  // `TRY 1 FAIL` / `TRY 2 PASS` pair, so a supersession is NOT evidence of a genuine retry - it is
  // evidence that the named set was REDUCED and the reduction cannot be attributed. Callers use this
  // to refuse tolerance, the one path from `failures exist in the log` to a green verdict.
  const clearedFailures = [];
  for (const line of text.split("\n")) {
    const m = NEXTEST_STATUS_LINE.exec(line);
    if (!m) continue;
    if (!NEXTEST_STATUS_FIELD.test(m[1])) continue;
    const status = m[1].trim();
    const kind = classifyNextestStatusField(status);
    if (kind === "unknown") continue;
    const parts = m[2].trim().split(/\s+/);
    if (!parts.length) continue;
    const name = parts[parts.length - 1];
    // Prefer `<binary-id> <name>` as the identity. The progress column (`(3/5)`, `(___)`) is dropped: it
    // is not part of a test's identity and it changes between attempts of the same test.
    const prev = parts.length >= 2 ? parts[parts.length - 2] : "";
    const binaryId = /^\(.*\)$/.test(prev) ? "" : prev;
    const key = `${binaryId} ${name}`;
    const existing = terminal.get(key);
    const entry = { status, name, binaryId, kind };
    if (existing && !nextestStatusSupersedes(existing, entry)) continue;
    if (existing && existing.kind === "fail" && kind !== "fail") {
      clearedFailures.push({ name: existing.name, from: existing.status, to: status });
    }
    terminal.set(key, entry);
  }
  const out = [];
  for (const entry of terminal.values()) {
    if (entry.kind === "fail") {
      out.push({ status: entry.status, name: entry.name, binaryId: entry.binaryId });
    }
  }
  return { failures: out, clearedFailures };
}

// The deduped failed-test names across EVERY terminal failure status (not just `FAIL`).
export function extractNextestFailureStatusNames(text) {
  return extractNextestTerminalFailures(text).failures.map((f) => f.name);
}

// Classify a nextest log's failures (used by the in-process classifier so the testable path mirrors the
// live SURFACE-1 verdict). It recognizes the SAME non-`FAIL` failure statuses + summary-count tripwire the
// live path uses:
//   "regression" — >=1 NON-`FAIL` failure status line (a crash/leak/timeout is never tolerated), OR the
//                  summary `failed` count exceeds the accounted `FAIL` names (an unaccounted failure
//                  class), OR >=1 parsed `FAIL` name is not allowlisted.
//   "tolerated"  — >=1 `FAIL` line, EVERY parsed `FAIL` name is an EXACT allowlisted name, NO non-`FAIL`
//                  failure status line, and the summary count does not exceed the accounted names —
//                  AND `freshnessToleranceAllowed === true` (the freshness-tooling preflight permitted it).
//   "none"       — no failure status lines parsed AND the summary reports zero failures.
//
// `freshnessToleranceAllowed` gates the allowlist consultation (see `TOLERATED_TEST_NAMES`). It defaults to
// `false` (fail-closed): when the freshness tools are present or were installed, an allowlisted `FAIL` name
// is treated as a regression, NOT tolerated — so a stale-binding break is caught instead of swallowed.
export function classifyNextestFailures(text, freshnessToleranceAllowed = false) {
  const { failures: terminal, clearedFailures } = extractNextestTerminalFailures(text);
  const plainFails = terminal.filter((f) => f.status === "FAIL");
  // A non-`FAIL` terminal status (SIGABRT/SIGSEGV/LEAK-FAIL/TIMEOUT/…) is a crash, a hang, or an
  // unexecutable test — never tolerated.
  const nonFailFailures = terminal.filter((f) => f.status !== "FAIL").length;
  const summary = parseNextestSummary(text);
  // The summary's authoritative run-but-did-not-pass total vs what we could NAME. A shortfall means a
  // failure class hides in the counts beyond the named lines (a `timed out` / `exec failed` count with no
  // parseable status line, say) — unaccounted, so a regression. `unrun > 0` means the run was cancelled or
  // interrupted and some selected tests never executed at all.
  const unaccounted = summary.runCountFound ? summary.nonPassed - terminal.length : 0;
  if (nonFailFailures > 0 || unaccounted > 0 || summary.unrun > 0) return "regression";
  if (plainFails.length === 0) return "none";
  // A failure cleared by a supersession means the named set was reduced and the reduction cannot be
  // attributed to a genuine retry rather than to forged output - so tolerance is unreachable.
  if (clearedFailures.length > 0) return "regression";
  for (const f of plainFails) {
    // Tolerated ONLY when the preflight allowed it AND the failure is the exempt pair in its OWN
    // binary; otherwise it is a regression.
    if (!(freshnessToleranceAllowed && isToleratedIdentity(f.binaryId, f.name)))
      return "regression";
  }
  return "tolerated";
}

// The SHARED SURFACE-1 verdict logic. The live gate and the in-process classifier both call this so the
// testable path is byte-identical to the live aggregation. Given a nextest log + the run exit code, it
// returns the non-tolerated `failures` (each {surface,name}), the tolerated count, and the parsed summary.
// The load-bearing rule: a crash renders under a NON-`FAIL` status and a nextest setup/harness error exits
// non-zero with NO `FAIL [` line — both would pass green if only `FAIL [` lines were trusted. The summary
// `failed` total counts every failure class, so any shortfall vs the accounted `FAIL` names is an
// unaccounted failure; trip a hard failure when the run exited non-zero AND (there is such a shortfall OR
// no `FAIL` name parsed at all). The tolerated env pair has summary.failed == the two accounted `FAIL`
// names, so unaccounted == 0 and this never fires for it.
//
// `freshnessToleranceAllowed` gates the allowlist consultation (default `false`, fail-closed): when the
// freshness tools are present or were installed, an allowlisted `FAIL` name is pushed to `failures` (a hard
// regression) instead of counted tolerated. The crash and the non-zero-exit summary-accounting tripwire
// (the missing-summary / count-mismatch hard-fail path below) are INDEPENDENT of this flag — a tolerated
// allowlisted name never lowers them. Note the deliberate exit-code precondition the summary-accounting
// tripwire carries (and that the (xiv) self-test locks in): it gates a NON-ZERO nextest exit only — a
// clean exit-0 run is never forced to FAIL by the summary requirement, so a code-0 tolerated `FAIL` log
// with no/contradictory summary stays tolerated. A crash (non-`FAIL` status line) is always hard
// regardless of exit code or flag.
export function analyzeNextestSurface(text, code, freshnessToleranceAllowed = false) {
  const failures = [];
  let toleratedCount = 0;
  const { failures: terminal, clearedFailures } = extractNextestTerminalFailures(text);
  // TOLERANCE TRUST. `nonPassed` is nextest's own count and cannot be lowered by anything printed into
  // the stream, so a cleared failure normally shows up as a shortfall and fails the run. The one way to
  // hide it is to also supply a replacement name that balances the count - and a replacement only
  // produces a GREEN verdict if it is ALLOWLISTED, because any other name is itself a named failure.
  // That makes the tolerance path the entire residual attack surface, so tolerance is refused whenever
  // any failure was superseded by a pass. Refusing costs nothing real here (`retries = 0`, so no genuine
  // supersession occurs) and it fails closed when the gate cannot prove the supersession was genuine.
  const toleranceTrusted = clearedFailures.length === 0;
  // `terminal` is already one entry per test (keyed by `<binary-id> <name>`), so it must NOT be
  // re-deduplicated by bare NAME here: two binaries can each own a test called `cases::shared::x`, and
  // collapsing them would drop one real failure and leave an opaque unaccounted entry standing in for it.
  const plainFails = terminal.filter((f) => f.status === "FAIL");
  const nonFail = terminal.filter((f) => f.status !== "FAIL");
  const summary = parseNextestSummary(text);
  let refusedTolerations = 0;
  for (const f of plainFails) {
    const allowlisted = freshnessToleranceAllowed && isToleratedIdentity(f.binaryId, f.name);
    if (allowlisted && toleranceTrusted) toleratedCount++;
    else {
      if (allowlisted) refusedTolerations++;
      failures.push({ surface: "nextest", name: f.name });
    }
  }
  if (refusedTolerations > 0) {
    const cleared = clearedFailures.map((c) => `${c.name} (${c.from} -> ${c.to})`).join(", ");
    failures.push({
      surface: "nextest",
      name: `<tolerance refused: ${clearedFailures.length} failure(s) were superseded by a pass, so the named set cannot be trusted: ${cleared}>`,
    });
  }
  // A NON-`FAIL` terminal status is a crash, a hang (TIMEOUT), or an unexecutable test. It is NEVER
  // tolerance-eligible and it is NAMED with the same visibility as an ordinary failure — the operator has
  // to be told WHICH test hung, not merely that the run was "unaccounted". The status rides on the surface
  // tag so the verdict line reads e.g. `[nextest:TIMEOUT] cases::foo::hangs`.
  for (const f of nonFail) failures.push({ surface: `nextest:${f.status}`, name: f.name });
  const namedCount = plainFails.length + nonFail.length;

  // ACCOUNTING. The summary's `runCount - passed` is the authoritative count of tests that ran and did not
  // pass, independent of how nextest labelled each one. Anything it counts beyond what we could NAME is an
  // unaccounted failure — that is what catches a `timed out` / `exec failed` count with no parseable status
  // line, and any future outcome class this parser has never heard of.
  //
  // The non-zero-exit precondition is deliberate and load-bearing (locked in by the (xiv) self-test): a
  // CLEAN exit-0 run is never forced to FAIL by the summary requirement, so a code-0 tolerated `FAIL` log
  // with no or contradictory summary stays tolerated. A crash/timeout status line is always hard, above,
  // regardless of exit code.
  const unaccounted = summary.runCountFound ? summary.nonPassed - namedCount : 0;
  if (code !== 0) {
    if (!summary.found || !summary.runCountFound) {
      // A missing/unparseable summary (a setup or harness error, a killed or interrupted run) cannot prove
      // what failed, so a non-zero exit must FAIL even if every parsed name is allowlisted.
      failures.push({
        surface: "nextest",
        name: `<run exit ${code}; unaccounted failure(s) (summary MISSING or unparseable, named failures=${namedCount})>`,
      });
    } else if (unaccounted > 0 || namedCount === 0) {
      // The summary counts more non-passing tests than we could name, or a non-zero exit reported no
      // failure at all — either way the run is not accounted for.
      failures.push({
        surface: "nextest",
        name:
          `<run exit ${code}; unaccounted failure(s) (summary ${summary.runCount} run / ${summary.passed} passed` +
          `${summary.failed ? `, ${summary.failed} failed` : ""}` +
          `${summary.timedOut ? `, ${summary.timedOut} timed out` : ""}` +
          `${summary.execFailed ? `, ${summary.execFailed} exec failed` : ""}` +
          ` => ${summary.nonPassed} did not pass; named failures=${namedCount})>`,
      });
    }
  }
  // More than one layout-valid Summary line. nextest emits exactly one per run, so a second is not an
  // ambiguity to resolve by position - it means something other than the runner authored a Summary, and
  // the run's accounting cannot be trusted at all.
  if (summary.count > 1) {
    failures.push({
      surface: "nextest",
      name: `<${summary.count} Summary lines in one run; nextest emits exactly one, so the run accounting was forged or interleaved>`,
    });
  }
  // The log NAMED more failures than nextest COUNTED. `nonPassed` comes from nextest's own accounting;
  // `namedCount` is read out of a stream that also carries captured test output. An excess means
  // something which is not a runner status line was parsed as one, so this run's naming cannot be
  // trusted - surface it rather than quietly keeping the extra name.
  if (summary.runCountFound && namedCount > summary.nonPassed) {
    failures.push({
      surface: "nextest",
      name:
        `<log named ${namedCount} failing test(s) but nextest counted ${summary.nonPassed} non-passing ` +
        `(${summary.runCount} run / ${summary.passed} passed); a non-status line parsed as a status line>`,
    });
  }
  // A cancelled/interrupted run (`A/B tests run`) left B-A selected tests UNEXECUTED. Those tests have no
  // result at all, so the run cannot certify the tree no matter what the executed ones did. Checked
  // independently of the exit code: a run that did not run its own test universe is never a pass.
  if (summary.runCountFound && summary.unrun > 0) {
    failures.push({
      surface: "nextest",
      name: `<run did not complete: ${summary.unrun} of ${summary.initialCount} selected test(s) never ran (cancelled or interrupted)>`,
    });
  }
  return { failures, toleratedCount, summary, namedCount, unaccounted };
}

// ----------------------------------------------------------------------------------------------------
// GATE TELEMETRY (report-only — nothing here is consulted by any verdict path above). nextest's default
// terminal reporter prints one status line PER TEST as it completes — the SAME status-line grammar
// `extractNextestTerminalFailures` parses above, but for every test (not just failing ones) and carrying
// that test's own `[ <duration> ]` timing bracket. This walks the identical grammar to recover per-test
// wall time from output the gate was already capturing and then discarding, instead of asking nextest for
// a second (JUnit) output format or re-running anything.
// ----------------------------------------------------------------------------------------------------
export const GATE_TELEMETRY_SCHEMA = "verter-gate-telemetry/v1";
export const GATE_TELEMETRY_SCHEMA_VERSION = 1;

export const GATE_TELEMETRY_PHASE_IDS = Object.freeze([
  "build-prerequisite",
  "oracle-cache",
  "harness-smoke-vapor",
  "harness-smoke-typescript",
  "freshness-tooling",
  "vue-macro-oracle-check",
  "vue-macro-oracle-tests",
  "dev-archive",
  "dev-list",
  "surface-1",
  "shipped-check",
  "shipped-contract",
  "advisory",
  "teardown",
]);

export const PREPARE_TELEMETRY_PHASE_IDS = Object.freeze([
  "dev-archive",
  "dev-list",
  "prepare-warm",
  "advisory",
  "teardown",
]);

// A deliberately small, pure accumulator. The live gate owns all scheduling and verdict decisions; this
// object only records observations handed to it. Tests inject a fake clock, and live callers may discard
// any telemetry exception without changing the gate's existing exit code.
export function createGateTelemetry({
  mode = "gate",
  now = nowMs,
  startedUtc = new Date().toISOString(),
  expectedPhaseIds = mode === "prepare" ? PREPARE_TELEMETRY_PHASE_IDS : GATE_TELEMETRY_PHASE_IDS,
} = {}) {
  const startMs = now();
  return {
    schema: GATE_TELEMETRY_SCHEMA,
    schemaVersion: GATE_TELEMETRY_SCHEMA_VERSION,
    mode,
    startedUtc,
    environment: null,
    lanes: null,
    cargoTimings: [],
    nextest: {},
    warnings: [],
    _reportingPartial: false,
    _now: now,
    _startMs: startMs,
    _expectedPhaseIds: Array.from(expectedPhaseIds),
    _phaseMap: new Map(),
    _aggregateForestPeak: null,
  };
}

export function recordGatePhase(
  telemetry,
  phaseId,
  {
    status = "ok",
    startedAtMs = null,
    durationMs = null,
    peakRssBytes = 0,
    peakRssProcessCount = 0,
    detail = null,
  } = {},
) {
  if (!telemetry || !(telemetry._phaseMap instanceof Map)) {
    throw new TypeError("invalid GateTelemetry accumulator");
  }
  if (telemetry._phaseMap.has(phaseId)) {
    throw new RangeError(`gate telemetry phase recorded twice: ${phaseId}`);
  }
  const saneDuration = Number.isFinite(durationMs)
    ? Math.max(0, durationMs)
    : Number.isFinite(startedAtMs)
      ? Math.max(0, telemetry._now() - startedAtMs)
      : null;
  const row = {
    id: phaseId,
    status,
    durationMs: saneDuration,
    peakRssBytes: Number.isFinite(peakRssBytes) ? Math.max(0, peakRssBytes) : 0,
    peakRssProcessCount: Number.isFinite(peakRssProcessCount)
      ? Math.max(0, peakRssProcessCount)
      : 0,
    ...(detail ? { detail: String(detail) } : {}),
  };
  telemetry._phaseMap.set(phaseId, row);
  return row;
}

export function gatePhaseStatusFromStep(result) {
  if (result && result.reason) return "aborted";
  return result && result.code === 0 ? "ok" : "failed";
}

// Capture the supervisor's highest same-snapshot sum. This is additive/report-only: the lane-local phase
// observations remain intact, while the whole-gate peak can no longer under-report two concurrent forests
// by selecting only the larger child peak.
export function recordGateAggregateForestPeak(telemetry, snapshot) {
  if (!telemetry || !(telemetry._phaseMap instanceof Map)) {
    throw new TypeError("invalid GateTelemetry accumulator");
  }
  const rssBytes = Number.isFinite(snapshot?.aggregatePeakRssBytes)
    ? Math.max(0, snapshot.aggregatePeakRssBytes)
    : 0;
  const processCount = Number.isFinite(snapshot?.aggregatePeakProcessCount)
    ? Math.max(0, snapshot.aggregatePeakProcessCount)
    : 0;
  const laneContributions = {};
  for (const [laneId, contribution] of Object.entries(snapshot?.aggregatePeakPerLane || {})) {
    if (
      !contribution ||
      typeof contribution !== "object" ||
      !Number.isFinite(contribution.rssBytes) ||
      contribution.rssBytes < 0 ||
      !Number.isFinite(contribution.processCount) ||
      contribution.processCount < 0
    ) {
      continue;
    }
    laneContributions[laneId] = {
      rssBytes: contribution.rssBytes,
      processCount: contribution.processCount,
    };
  }
  telemetry._aggregateForestPeak = {
    observation: "supervisor-same-snapshot",
    rssBytes,
    processCount,
    laneContributions,
  };
  return telemetry._aggregateForestPeak;
}

export function summarizeGateTelemetry(
  telemetry,
  { terminalReached = false, exitCode = null, endMs = telemetry._now() } = {},
) {
  const expected = telemetry._expectedPhaseIds;
  const phases = expected.map(
    (id) =>
      telemetry._phaseMap.get(id) || {
        id,
        status: "not-run",
        durationMs: null,
        peakRssBytes: 0,
        peakRssProcessCount: 0,
      },
  );
  for (const [id, row] of telemetry._phaseMap) {
    if (!expected.includes(id)) phases.push(row);
  }

  let peak = { phaseId: null, rssBytes: 0, processCount: 0 };
  for (const row of phases) {
    // Ties intentionally use the latest phase, mirroring runContainedStep's latest-sample tie behavior.
    if (row.peakRssBytes >= peak.rssBytes && row.peakRssBytes > 0) {
      peak = {
        phaseId: row.id,
        rssBytes: row.peakRssBytes,
        processCount: row.peakRssProcessCount,
      };
    }
  }
  const aggregate = telemetry._aggregateForestPeak;
  if (aggregate && aggregate.rssBytes >= peak.rssBytes && aggregate.rssBytes > 0) {
    peak = {
      phaseId: "supervisor-aggregate",
      observation: aggregate.observation,
      rssBytes: aggregate.rssBytes,
      processCount: aggregate.processCount,
      laneContributions: structuredClone(aggregate.laneContributions),
    };
  }
  const measuredEveryApplicablePhase = phases.every(
    (row) => row.status !== "not-run" && row.status !== "aborted",
  );
  const completeness =
    terminalReached && measuredEveryApplicablePhase && !telemetry._reportingPartial
      ? "complete"
      : "partial";
  return {
    schema: GATE_TELEMETRY_SCHEMA,
    schemaVersion: GATE_TELEMETRY_SCHEMA_VERSION,
    mode: telemetry.mode,
    completeness,
    startedUtc: telemetry.startedUtc,
    terminal: { reached: Boolean(terminalReached), exitCode },
    whole: {
      elapsedMs: Math.max(0, endMs - telemetry._startMs),
      containedChildTreePeak: peak,
    },
    environment: telemetry.environment,
    lanes: telemetry.lanes ? structuredClone(telemetry.lanes) : null,
    phases,
    cargoTimings: telemetry.cargoTimings.map((entry) => ({ ...entry })),
    nextest: { ...telemetry.nextest },
    warnings: telemetry.warnings.slice(),
  };
}

export function formatGateTelemetryText(summary) {
  const lines = [
    `GATE TELEMETRY: schema ${summary.schemaVersion}, measurement ${summary.completeness}; whole elapsed ${(
      summary.whole.elapsedMs / 1000
    ).toFixed(3)}s`,
  ];
  const peak = summary.whole.containedChildTreePeak;
  lines.push(
    `GATE TELEMETRY: whole monitored contained-child-tree peak RSS ${formatMemorySize(
      peak.rssBytes,
    )} across ${peak.processCount} process(es) in phase ${peak.phaseId || "none"}` +
      (peak.laneContributions
        ? `; lane contributions ${JSON.stringify(peak.laneContributions)}`
        : ""),
  );
  if (summary.environment) {
    lines.push(`GATE ENVIRONMENT: ${JSON.stringify(summary.environment)}`);
  }
  for (const row of summary.phases) {
    lines.push(
      `GATE PHASE [${row.id}]: status=${row.status}, duration=${
        row.durationMs === null ? "unavailable" : `${(row.durationMs / 1000).toFixed(3)}s`
      }, peak RSS ${formatMemorySize(row.peakRssBytes)} across ${row.peakRssProcessCount} process(es)`,
    );
  }
  for (const artifact of summary.cargoTimings) {
    lines.push(
      `CARGO TIMING [${artifact.phaseId}]: ${artifact.available ? `available at ${artifact.relativePath}` : `unavailable (${artifact.status}: ${artifact.error})`}`,
    );
  }
  return lines.join("\n");
}

// Bounded command probe. No error message or executable path is returned: the fingerprint reports only a
// small availability classification and bounded stdout, never usernames/paths from a spawn exception.
export const GATE_TELEMETRY_PROBE_KILL_SIGNAL = "SIGKILL";
export const GATE_TELEMETRY_PROBE_MAX_MS = 2_000;
// All synchronous startup reporting probes share this one allowance. The canonical build/test timeout is
// established only after startup collection settles, so report-only work is bounded without spending any
// of the verdict-bearing gate budget.
export const GATE_TELEMETRY_STARTUP_MAX_MS = 2_000;

export function runBoundedVersionProbe(
  command,
  args,
  {
    spawnSyncFn = spawnSync,
    timeoutMs = GATE_TELEMETRY_PROBE_MAX_MS,
    deadlineMs = Number.POSITIVE_INFINITY,
    now = nowMs,
  } = {},
) {
  const remainingMs = deadlineMs - now();
  const effectiveTimeoutMs = Number.isFinite(remainingMs)
    ? Math.min(timeoutMs, remainingMs)
    : timeoutMs;
  // Node treats a non-positive spawnSync timeout as "no timeout". Once the parent deadline is spent,
  // refuse before spawning instead of converting the expired gate into an unbounded mutex-held probe.
  if (!(effectiveTimeoutMs > 0)) {
    return { available: false, stdout: "", error: "timeout" };
  }
  try {
    const result = spawnSyncFn(command, args, {
      encoding: "utf8",
      windowsHide: true,
      timeout: effectiveTimeoutMs,
      // A version/help probe owns no transaction or buffered result worth a graceful shutdown. SIGTERM is
      // catchable on POSIX and can leave spawnSync blocked after its timeout; SIGKILL maps to direct,
      // unignorable termination on supported platforms and never broadens the kill beyond this child.
      killSignal: GATE_TELEMETRY_PROBE_KILL_SIGNAL,
      maxBuffer: 1024 * 1024,
    });
    if (result.error) {
      return {
        available: false,
        stdout: "",
        error: result.error.code === "ETIMEDOUT" ? "timeout" : "spawn-unavailable",
      };
    }
    if (result.status !== 0) {
      return { available: false, stdout: "", error: `exit-${result.status ?? "unknown"}` };
    }
    return {
      available: true,
      stdout: String(result.stdout || result.stderr || "").trim(),
      error: null,
    };
  } catch {
    return { available: false, stdout: "", error: "probe-error" };
  }
}

function probeSummary(probe, { host = false } = {}) {
  if (!probe || !probe.available) {
    return {
      available: false,
      version: null,
      ...(host ? { host: null } : {}),
      error: probe?.error || "unavailable",
    };
  }
  const lines = String(probe.stdout || "")
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean);
  const release = lines.find((line) => /^release:\s*/.test(line));
  const hostLine = lines.find((line) => /^host:\s*/.test(line));
  return {
    available: true,
    version: release ? release.replace(/^release:\s*/, "") : lines[0] || "unknown",
    ...(host ? { host: hostLine ? hostLine.replace(/^host:\s*/, "") : null } : {}),
    error: null,
  };
}

function sumNumericLeaves(value) {
  if (typeof value === "number" && Number.isFinite(value)) return value;
  if (!value || typeof value !== "object") return 0;
  return Object.values(value).reduce((sum, child) => sum + sumNumericLeaves(child), 0);
}

function sccacheHitRate(probe) {
  if (!probe || !probe.available) return null;
  try {
    const parsed = JSON.parse(probe.stdout);
    const stats = parsed.stats || parsed;
    const hits = sumNumericLeaves(stats.cache_hits ?? stats.cacheHits);
    const misses = sumNumericLeaves(stats.cache_misses ?? stats.cacheMisses);
    return hits + misses > 0 ? hits / (hits + misses) : null;
  } catch {
    return null;
  }
}

export function classifyGateTargetState(
  targetDir,
  { existsFn = existsSync, readdirFn = readdirSync } = {},
) {
  try {
    if (!existsFn(targetDir)) return "absent";
    return readdirFn(targetDir).length === 0 ? "empty" : "nonempty";
  } catch {
    return "unavailable";
  }
}

export function collectEnvironmentFingerprint({
  instantUtc = new Date().toISOString(),
  os = {
    type: osType(),
    platform: osPlatform(),
    arch: osArch(),
    release: osRelease(),
    version: typeof osVersion === "function" ? osVersion() : "unavailable",
    cpuModel: cpus()[0]?.model || "unavailable",
    logicalCpuCount: cpus().length,
    availableCpuCount:
      typeof availableParallelism === "function" ? availableParallelism() : cpus().length,
    totalMemoryBytes: totalmem(),
  },
  node = { version: process.version, v8: process.versions.v8 },
  targetState = "unavailable",
  resources = {},
  env = {},
  runVersionProbe = (command, args) => runBoundedVersionProbe(command, args),
} = {}) {
  const safeProbe = (command, args) => {
    try {
      return runVersionProbe(command, args);
    } catch {
      return { available: false, stdout: "", error: "probe-error" };
    }
  };
  const rustc = probeSummary(safeProbe("rustc", ["--version", "--verbose"]), { host: true });
  const cargo = probeSummary(safeProbe("cargo", ["--version", "--verbose"]), { host: true });
  const cargoNextest = probeSummary(safeProbe("cargo", ["nextest", "--version"]));

  const rawWrapper = typeof env.RUSTC_WRAPPER === "string" ? env.RUSTC_WRAPPER : "";
  const wrapperName = rawWrapper ? basename(rawWrapper.replace(/^['"]|['"]$/g, "")) : null;
  const wrapperIsSccache = Boolean(wrapperName && /^sccache(?:\.exe)?$/i.test(wrapperName));
  const sccacheVersion = safeProbe("sccache", ["--version"]);
  const sccacheStats =
    wrapperIsSccache || sccacheVersion.available
      ? safeProbe("sccache", ["--show-stats", "--stats-format", "json"])
      : { available: false, stdout: "", error: "not-present" };

  return {
    instantUtc,
    os: {
      type: os.type,
      platform: os.platform,
      arch: os.arch,
      release: os.release,
      version: os.version,
    },
    cpu: {
      model: os.cpuModel,
      logicalCount: os.logicalCpuCount,
      availableCount: os.availableCpuCount,
    },
    totalMemoryBytes: os.totalMemoryBytes,
    node: { version: node.version, v8: node.v8 },
    rustc,
    cargo,
    cargoNextest,
    targetState,
    resources: {
      buildJobs: resources.buildJobs ?? null,
      testThreads: resources.testThreads ?? null,
      memoryLimitBytes: resources.memoryLimitBytes ?? null,
      profiles: Array.isArray(resources.profiles) ? resources.profiles.slice() : [],
      nextestProfile: typeof env.NEXTEST_PROFILE === "string" ? env.NEXTEST_PROFILE : "default",
    },
    incremental:
      env.CARGO_INCREMENTAL === "0"
        ? "disabled"
        : env.CARGO_INCREMENTAL === "1"
          ? "enabled"
          : "cargo-default",
    wrapper: { present: wrapperName !== null, basename: wrapperName },
    sccache: {
      present: wrapperIsSccache || Boolean(sccacheVersion.available),
      hitRate: sccacheHitRate(sccacheStats),
      statsError: sccacheStats.available ? null : sccacheStats.error || "unavailable",
    },
  };
}

export function collectCargoTimingCapabilities(
  runProbe = (command, args) => runBoundedVersionProbe(command, args),
) {
  const one = (args) => {
    let probe;
    try {
      probe = runProbe("cargo", args);
    } catch {
      probe = { available: false, stdout: "", error: "probe-error" };
    }
    if (!probe || !probe.available) {
      return { supported: false, error: probe?.error || "probe-unavailable" };
    }
    if (!String(probe.stdout || "").includes("--timings")) {
      return { supported: false, error: "flag-not-advertised" };
    }
    return { supported: true, error: null };
  };
  return {
    devArchive: one(["nextest", "archive", "--help"]),
    shippedCheck: one(["check", "--help"]),
    shippedContract: one(["nextest", "run", "--help"]),
  };
}

export function cargoTimingArtifactPaths(runnerTarget, gateDir) {
  const cargoTimingDir = join(gateDir, "cargo-timings");
  return {
    source: join(runnerTarget, "cargo-timings", "cargo-timing.html"),
    destinations: {
      "dev-archive": join(cargoTimingDir, "dev-nextest-archive.html"),
      "shipped-check": join(cargoTimingDir, "shipped-cfg-check.html"),
      "shipped-contract": join(cargoTimingDir, "shipped-cfg-contract.html"),
    },
  };
}

// Clears only the one Cargo-owned overwrite target and the one phase-owned prior snapshot. No directory
// deletion is performed. Failures are retained as warnings and never thrown to a verdict caller.
export function prepareCargoTimingArtifact({
  source,
  destination,
  now = Date.now,
  rmFileFn = (path) => rmSync(path, { force: true }),
  existsFn = existsSync,
  statFn = statSync,
  readFileFn = readFileSync,
} = {}) {
  const warnings = [];
  let priorSourceIdentity = null;
  try {
    if (existsFn(source)) {
      const sourceStat = statFn(source);
      if (sourceStat?.isFile?.()) {
        const bytes = readFileFn(source);
        priorSourceIdentity = {
          size: sourceStat.size,
          sha256: createHash("sha256").update(bytes).digest("hex"),
        };
      }
    }
  } catch {
    warnings.push("source-identity-unavailable-before-clear");
  }
  let sourceCleared = false;
  try {
    rmFileFn(source);
  } catch {
    warnings.push("source-clear-failed");
  }
  try {
    sourceCleared = !existsFn(source);
    if (!sourceCleared) warnings.push("source-still-present-after-clear");
  } catch {
    warnings.push("source-absence-check-failed");
  }
  try {
    rmFileFn(destination);
  } catch {
    warnings.push("destination-clear-failed");
  }
  return {
    source,
    destination,
    relativePath: `cargo-timings/${basename(destination)}`,
    preparedAtMs: now(),
    sourceCleared,
    priorSourceIdentity,
    warnings,
  };
}

export function snapshotCargoTimingArtifact(
  capture,
  {
    existsFn = existsSync,
    statFn = statSync,
    readFileFn = readFileSync,
    mkdirFn = (path) => mkdirSync(path, { recursive: true }),
    copyFileFn = copyFileSync,
  } = {},
) {
  const unavailable = (status, error) => ({
    phaseId: null,
    available: false,
    status,
    relativePath: capture.relativePath,
    error,
    warnings: capture.warnings.slice(),
  });
  try {
    if (!existsFn(capture.source))
      return unavailable("missing", "Cargo timing source was not produced");
    const sourceStat = statFn(capture.source);
    if (!sourceStat || !sourceStat.isFile?.() || sourceStat.size <= 0) {
      return unavailable("invalid", "Cargo timing source is not a non-empty file");
    }
    if (!capture.sourceCleared) {
      let currentSourceIdentity = null;
      try {
        const bytes = readFileFn(capture.source);
        currentSourceIdentity = {
          size: sourceStat.size,
          sha256: createHash("sha256").update(bytes).digest("hex"),
        };
      } catch {
        capture.warnings.push("source-identity-unavailable-after-command");
      }
      const prior = capture.priorSourceIdentity;
      const identityChanged = Boolean(
        prior &&
        currentSourceIdentity &&
        (prior.size !== currentSourceIdentity.size ||
          prior.sha256 !== currentSourceIdentity.sha256),
      );
      if (!identityChanged) {
        capture.warnings.push("source-identity-unchanged-or-ambiguous-after-failed-clear");
        return unavailable(
          "stale",
          "Cargo timing source clear was not proven and its file identity did not change",
        );
      }
    }
    // Two-second tolerance accommodates coarse filesystem timestamp resolution. Clearing the exact source
    // before launch remains the identity root; an older mtime cannot be reused as this command's report.
    if (!Number.isFinite(sourceStat.mtimeMs) || sourceStat.mtimeMs + 2000 < capture.preparedAtMs) {
      return unavailable("stale", "Cargo timing source predates the producing command");
    }
    mkdirFn(dirname(capture.destination));
    copyFileFn(capture.source, capture.destination);
    const destinationStat = statFn(capture.destination);
    if (
      !destinationStat ||
      !destinationStat.isFile?.() ||
      destinationStat.size !== sourceStat.size
    ) {
      return unavailable("copy-invalid", "Cargo timing snapshot identity/size validation failed");
    }
    return {
      phaseId: null,
      available: true,
      status: "fresh",
      relativePath: capture.relativePath,
      error: null,
      sizeBytes: destinationStat.size,
      warnings: capture.warnings.slice(),
    };
  } catch {
    return unavailable("copy-failed", "Cargo timing snapshot could not be copied or validated");
  }
}

const CARGO_TIMING_CAPABILITY_KEYS = Object.freeze({
  "dev-archive": "devArchive",
  "shipped-check": "shippedCheck",
  "shipped-contract": "shippedContract",
});

function markGateTelemetryReportingPartial(telemetry, detail) {
  if (!telemetry) return;
  telemetry._reportingPartial = true;
  telemetry.warnings.push(detail);
}

// The production report-only orchestration boundary. Gate code talks to telemetry through this object;
// reporting failures are converted to warnings + partial measurement and never receive the canonical
// failures/tolerance accumulator that determines the gate verdict.
export function createGateTelemetryReporter({
  telemetry,
  deadlineMs,
  targetState,
  resources = {},
  env = {},
  runnerTarget,
  gateDir,
  now = nowMs,
  warnFn = warn,
  logFn = log,
  runVersionProbeFn = runBoundedVersionProbe,
  collectCargoTimingCapabilitiesFn = collectCargoTimingCapabilities,
  collectEnvironmentFingerprintFn = collectEnvironmentFingerprint,
  prepareCargoTimingArtifactFn = prepareCargoTimingArtifact,
  snapshotCargoTimingArtifactFn = snapshotCargoTimingArtifact,
} = {}) {
  let cargoTimingCapabilities = {
    devArchive: { supported: false, error: "probe-error" },
    shippedCheck: { supported: false, error: "probe-error" },
    shippedContract: { supported: false, error: "probe-error" },
  };

  const reportingFailure = (detail, operatorMessage = detail) => {
    markGateTelemetryReportingPartial(telemetry, detail);
    warnFn(operatorMessage);
  };
  const boundedProbe = (command, args) => {
    let probe;
    try {
      probe = runVersionProbeFn(command, args, {
        deadlineMs,
        now,
        timeoutMs: GATE_TELEMETRY_PROBE_MAX_MS,
      });
    } catch {
      probe = { available: false, stdout: "", error: "probe-error" };
    }
    if (!probe || !probe.available) {
      const error = probe?.error || "unavailable";
      reportingFailure(
        `version probe ${command} ${args.join(" ")}: ${error}`,
        `GATE TELEMETRY WARNING: ${command} probe unavailable (${error}); gate verdict unchanged`,
      );
    }
    return probe;
  };

  const collectStartup = () => {
    try {
      cargoTimingCapabilities = collectCargoTimingCapabilitiesFn(boundedProbe);
    } catch {
      reportingFailure(
        "cargo timing capability probes unavailable",
        "GATE TELEMETRY WARNING: Cargo timing capability probes unavailable; old argv retained",
      );
    }

    try {
      telemetry.environment = collectEnvironmentFingerprintFn({
        targetState,
        resources,
        env,
        runVersionProbe: boundedProbe,
      });
      telemetry.environment.cargoTimingCapabilities = cargoTimingCapabilities;
    } catch {
      telemetry.environment = {
        available: false,
        error: "fingerprint-unavailable",
        cargoTimingCapabilities,
      };
      reportingFailure(
        "environment fingerprint unavailable",
        "GATE TELEMETRY WARNING: environment fingerprint unavailable; gate verdict unchanged",
      );
    }
    return { cargoTimingCapabilities, environment: telemetry.environment };
  };

  const cargoTimingEnabled = (phaseId) => {
    const key = CARGO_TIMING_CAPABILITY_KEYS[phaseId];
    return Boolean(cargoTimingCapabilities?.[key]?.supported);
  };

  const beginCargoTiming = (phaseId, sourceTargetDir = runnerTarget) => {
    const key = CARGO_TIMING_CAPABILITY_KEYS[phaseId];
    const capability = cargoTimingCapabilities?.[key];
    const paths = cargoTimingArtifactPaths(sourceTargetDir, gateDir);
    if (!capability?.supported) {
      const error = capability?.error || "capability probe unavailable";
      telemetry?.cargoTimings.push({
        phaseId,
        available: false,
        status: "unsupported",
        relativePath: `cargo-timings/${basename(paths.destinations[phaseId])}`,
        error,
      });
      warnFn(
        `CARGO TIMING [${phaseId}] unavailable: ${error}; --timings omitted and the historical command argv retained`,
      );
      return null;
    }
    let capture;
    try {
      capture = prepareCargoTimingArtifactFn({
        source: paths.source,
        destination: paths.destinations[phaseId],
      });
    } catch {
      reportingFailure(
        `cargo timing ${phaseId}: prepare-failed`,
        `CARGO TIMING [${phaseId}] WARNING: timing source could not be prepared`,
      );
      return null;
    }
    for (const warning of capture.warnings) {
      telemetry?.warnings.push(`cargo timing ${phaseId}: ${warning}`);
      warnFn(`CARGO TIMING [${phaseId}] WARNING: ${warning}`);
    }
    return capture;
  };

  const finishCargoTiming = (phaseId, capture) => {
    if (!capture) return;
    let artifact;
    try {
      artifact = snapshotCargoTimingArtifactFn(capture);
    } catch {
      artifact = {
        phaseId,
        available: false,
        status: "copy-failed",
        relativePath: capture.relativePath,
        error: "Cargo timing snapshot could not be copied or validated",
        warnings: capture.warnings?.slice() || [],
      };
    }
    artifact.phaseId = phaseId;
    telemetry?.cargoTimings.push(artifact);
    if (artifact.available) {
      logFn(`CARGO TIMING [${phaseId}]: available at ${artifact.relativePath}`);
    } else {
      const detail = `${artifact.status}: ${artifact.error}`;
      reportingFailure(
        `cargo timing ${phaseId}: ${detail}`,
        `CARGO TIMING [${phaseId}] unavailable: ${detail}`,
      );
    }
  };

  const recordPhase = (phaseId, observation) => {
    try {
      return recordGatePhase(telemetry, phaseId, observation);
    } catch (error) {
      const detail = `phase ${phaseId} could not be recorded (${error?.message || "unknown"})`;
      reportingFailure(detail, `GATE TELEMETRY WARNING: ${detail}`);
      return null;
    }
  };

  return {
    collectStartup,
    cargoTimingEnabled,
    beginCargoTiming,
    finishCargoTiming,
    recordPhase,
    get cargoTimingCapabilities() {
      return cargoTimingCapabilities;
    },
  };
}

const NEXTEST_STATUS_LINE_TIMED = /^(.{12}) \[([^\]]*)\]\s+(.+)$/;

function parseTimingBracket(bracket) {
  const m = /([\d.]+)\s*s/.exec(bracket);
  return m ? parseFloat(m[1]) : null;
}

// Every test's TERMINAL (pass or fail) status line as `{ status, name, binaryId, kind, durationSec }`,
// resolved by the SAME last-status-wins supersession rule `extractNextestTerminalFailures` uses (so a
// retried test, or a `SLOW` progress line followed by its `PASS`, is counted once at its final duration —
// not once per progress line). `durationSec` is `null` when the bracket carried no parseable `Ns` value;
// callers skip those rather than treat them as zero.
export function collectNextestTestTimings(text) {
  const terminal = new Map();
  for (const line of text.split("\n")) {
    const m = NEXTEST_STATUS_LINE_TIMED.exec(line);
    if (!m) continue;
    if (!NEXTEST_STATUS_FIELD.test(m[1])) continue;
    const status = m[1].trim();
    const kind = classifyNextestStatusField(status);
    if (kind === "unknown") continue;
    const parts = m[3].trim().split(/\s+/);
    if (!parts.length) continue;
    const name = parts[parts.length - 1];
    const prev = parts.length >= 2 ? parts[parts.length - 2] : "";
    const binaryId = /^\(.*\)$/.test(prev) ? "" : prev;
    const key = `${binaryId} ${name}`;
    const durationSec = parseTimingBracket(m[2]);
    const existing = terminal.get(key);
    const entry = { status, name, binaryId, kind, durationSec };
    if (existing && !nextestStatusSupersedes(existing, entry)) continue;
    terminal.set(key, entry);
  }
  const out = [];
  for (const entry of terminal.values()) {
    if (entry.kind === "pass" || entry.kind === "fail") out.push(entry);
  }
  return out;
}

// A test's reporting "family": its module path with the final `::segment` (the leaf test function)
// stripped, qualified by binary-id so two binaries' same-named module never merge into one row. A bare
// (non-`::`-qualified) test name is its own family. This grouping exists for the report only — it plays no
// part in pass/fail classification.
function testFamilyKey(binaryId, name) {
  const idx = name.lastIndexOf("::");
  const family = idx === -1 ? name : name.slice(0, idx);
  return `${binaryId} ${family}`;
}

// Aggregates raw per-test timings (see collectNextestTestTimings) into the shapes the gate reports:
// cumulative duration + count per binary, per package (via the archive's OWN `binary-id -> package-name`
// listing, so this can never drift from what actually ran), and the top-N cumulative-time test families.
// Report-only: nothing here feeds the verdict.
export function summarizeNextestTimings(timings, allSuites, topN = 50) {
  const binaryToPackage = new Map();
  for (const s of allSuites || []) {
    if (s && s["binary-id"] !== undefined) {
      binaryToPackage.set(s["binary-id"], s["package-name"] || "?");
    }
  }
  const perBinary = new Map();
  const perPackage = new Map();
  const perFamily = new Map();
  let totalSec = 0;
  let timedCount = 0;
  const bump = (map, key, durationSec) => {
    const cur = map.get(key) || { processCount: 0, timedCount: 0, count: 0, totalSec: 0 };
    cur.processCount++;
    if (durationSec !== null) {
      cur.timedCount++;
      cur.count++; // legacy alias: count has always meant parseably timed tests.
      cur.totalSec += durationSec;
    }
    map.set(key, cur);
  };
  for (const t of timings) {
    const pkg = binaryToPackage.get(t.binaryId) || t.binaryId || "?";
    bump(perBinary, t.binaryId || "?", t.durationSec);
    bump(perPackage, pkg, t.durationSec);
    bump(perFamily, testFamilyKey(t.binaryId, t.name), t.durationSec);
    if (t.durationSec === null) continue;
    timedCount++;
    totalSec += t.durationSec;
  }
  const sortDesc = (map) =>
    Array.from(map.entries())
      .map(([key, v]) => ({ key, ...v }))
      .sort((a, b) => b.totalSec - a.totalSec || a.key.localeCompare(b.key));
  const perPackageRows = sortDesc(perPackage);
  const perFamilyRows = sortDesc(perFamily);
  return {
    totalTests: timings.length,
    processCount: timings.length,
    timedCount,
    count: timedCount,
    totalSec,
    perBinary: sortDesc(perBinary),
    perPackage: perPackageRows,
    perCrate: perPackageRows,
    perFamily: perFamilyRows,
    topFamilies: perFamilyRows.slice(0, topN),
  };
}

// ----------------------------------------------------------------------------------------------------
// libtest stdout parsing — the EXACT failed-test names from a direct `cargo test`-style binary run.
// libtest prints a trailing "failures:\n    <name>\n    <name>\n" block; also each failing test emits
// "test <name> ... FAILED". We parse the "test … FAILED" lines (stable across libtest versions).
// ----------------------------------------------------------------------------------------------------
export function extractLibtestFailedNames(text) {
  const names = [];
  for (const line of text.split("\n")) {
    const m = /^test\s+(.+?)\s+\.\.\.\s+FAILED\s*$/.exec(line);
    if (m) names.push(m[1]);
  }
  return names;
}

// Parse libtest's trailing "test result: FAILED. N passed; M failed; …" (or "ok. …") line. Returns
// `{ found, ok, passed, failed }`. `found` is whether a `test result:` line was present at all — a missing
// summary means the binary did NOT complete its run normally (a panic in the harness, an abort/signal, a
// truncated capture), which the tolerance gate treats as UNACCOUNTED (a hard failure). libtest prints
// EXACTLY one such line per binary at the end of a normal run.
export function parseLibtestSummary(text) {
  let found = false;
  let ok = false;
  let passed = 0;
  let failed = 0;
  for (const line of text.split("\n")) {
    const m = /^test result:\s+(ok|FAILED)\.\s+(\d+)\s+passed;\s+(\d+)\s+failed/.exec(line.trim());
    if (m) {
      found = true;
      ok = m[1] === "ok";
      passed = parseInt(m[2], 10);
      failed = parseInt(m[3], 10);
      // Keep scanning; the LAST `test result:` line wins (a binary prints one, but be defensive).
    }
  }
  return { found, ok, passed, failed };
}

// Legacy Surface-2 (direct libtest binary) verdict retained only as a selftest regression fixture;
// production gate.mjs no longer calls it. Given a binary's combined stdout+stderr `text`, its
// process exit `code`, and the suite `binaryId` (for name qualification), returns:
//   { verdict: "pass" | "tolerated" | "fail", failures: [{ surface, name }], toleratedNames: [name…] }
//
// A tolerated failure in this legacy classifier is admitted ONLY under NORMAL libtest failure semantics.
// Concretely, ALL of:
//   - the process exited with code 101 — libtest's canonical "some tests failed" exit. A SIGNAL/ABORT
//     (SIGABRT/SIGSEGV/… surface as code 128+signal here, or any non-101 code) is a CRASH, never tolerated;
//   - a `test result: FAILED. P passed; M failed` summary line WAS parsed (a missing summary means the run
//     did not complete normally — a harness panic, an abort, a truncated capture — which is unaccounted);
//   - the summary `M failed` EXACTLY equals the number of parsed `test … FAILED` names (no extra failure
//     hides in the count beyond the accounted names);
//   - EVERY parsed FAILED name is an EXACT allowlisted name (bare or suite-qualified).
// Any deviation — a non-101/signal exit, a missing summary, a count mismatch, or a non-allowlisted name —
// is a HARD FAILURE. A clean run (code 0, zero FAILED names, summary `ok`/absent) is a pass.
//
// `freshnessToleranceAllowed` gates the allowlist consultation (default `false`, fail-closed): when it is
// false, `isTolerated` returns false for EVERY name, so the `tolerated` verdict path is unreachable and an
// allowlisted FAILED name becomes a hard `failures` entry. The crash / non-101-exit / missing-summary /
// count-mismatch hard-fail accounting is UNAFFECTED by the flag — those are always hard.
export function analyzeLibtestSurface(text, code, binaryId, freshnessToleranceAllowed = false) {
  const failNames = extractLibtestFailedNames(text);
  const summary = parseLibtestSummary(text);
  // Scoped to the OWNING binary on this surface too: a verter_session suite that happens to define a
  // test at the allowlisted path is a real failure, not an inherited exemption.
  const isTolerated = (nm) => freshnessToleranceAllowed && isToleratedIdentity(binaryId, nm);

  // Clean pass: exited 0 with no parsed FAILED lines. (A non-zero exit with zero FAILED names is a
  // crash/abort and falls through to the hard-fail accounting below.)
  if (code === 0 && failNames.length === 0) {
    return { verdict: "pass", failures: [], toleratedNames: [] };
  }

  // From here the run is non-clean. It is tolerable ONLY under exact, summary-proven, allowlisted libtest
  // failure semantics. Determine whether the accounting holds.
  const normalFailureExit = code === 101; // libtest's "tests failed" exit; a signal => 128+sig, never 101
  const summaryAccounts =
    summary.found && summary.failed === failNames.length && failNames.length > 0;
  const allNamesTolerated = failNames.length > 0 && failNames.every(isTolerated);

  if (normalFailureExit && summaryAccounts && allNamesTolerated) {
    return { verdict: "tolerated", failures: [], toleratedNames: failNames.slice() };
  }

  // HARD FAILURE. Surface a precise, accounted reason.
  const failures = [];
  for (const nm of failNames) {
    if (!isTolerated(nm)) failures.push({ surface: `libtest:${binaryId}`, name: nm });
  }
  if (!normalFailureExit) {
    // A crash/abort/signal or any non-101 exit — never tolerated, even if every parsed name is allowlisted.
    const signalled = code >= 128;
    failures.push({
      surface: `libtest:${binaryId}`,
      name: `<abnormal libtest exit ${code}${signalled ? " (signal/abort)" : ""}; not the normal 101 test-failure exit — crash not tolerated>`,
    });
  } else if (!summary.found) {
    failures.push({
      surface: `libtest:${binaryId}`,
      name: `<exit 101 but NO 'test result:' summary parsed — run did not complete normally; failures unaccounted>`,
    });
  } else if (summary.failed !== failNames.length) {
    failures.push({
      surface: `libtest:${binaryId}`,
      name: `<summary failed=${summary.failed} != ${failNames.length} parsed FAILED names — unaccounted failure(s)>`,
    });
  } else if (failNames.length === 0) {
    failures.push({
      surface: `libtest:${binaryId}`,
      name: `<exit ${code} with no parseable FAILED line>`,
    });
  }
  return { verdict: "fail", failures, toleratedNames: [] };
}

// ----------------------------------------------------------------------------------------------------
// Resolve a suite binary path from a nextest archive listing. With `--extract-to <dir>`, nextest rewrites
// `binary-path` to the extract location. We defend against either layout: if the listed path exists, use
// it; else try rebasing the `target-directory`-relative tail under the extract dir.
// ----------------------------------------------------------------------------------------------------
export function resolveSuiteBinary(binaryPath, buildMetaTargetDir, extractDir) {
  if (binaryPath && existsSync(binaryPath)) return binaryPath;
  // Rebase: binaryPath is typically "<target-directory>/debug/deps/<bin>"; strip the leading
  // target-directory and re-root under <extractDir>/target.
  if (buildMetaTargetDir && binaryPath && binaryPath.startsWith(buildMetaTargetDir)) {
    let tail = binaryPath.slice(buildMetaTargetDir.length);
    if (tail.startsWith(sep) || tail.startsWith("/") || tail.startsWith("\\")) tail = tail.slice(1);
    const candidate = join(extractDir, "target", tail);
    if (existsSync(candidate)) return candidate;
    const candidate2 = join(extractDir, tail);
    if (existsSync(candidate2)) return candidate2;
  }
  // Last resort: search the extract dir for a deps binary with the same basename.
  if (binaryPath) {
    const want = basename(binaryPath);
    const found = findFileByName(extractDir, want, 8);
    if (found) return found;
  }
  return binaryPath; // give back the original; the exec will fail loudly if it does not exist
}

export function findFileByName(root, name, maxDepth) {
  if (!existsSync(root)) return null;
  const stack = [{ dir: root, depth: 0 }];
  while (stack.length) {
    const { dir, depth } = stack.pop();
    let entries;
    try {
      entries = readdirSync(dir, { withFileTypes: true });
    } catch {
      continue;
    }
    for (const ent of entries) {
      const full = join(dir, ent.name);
      if (ent.isFile() && ent.name === name) return full;
      if (ent.isDirectory() && depth < maxDepth) stack.push({ dir: full, depth: depth + 1 });
    }
  }
  return null;
}

// ----------------------------------------------------------------------------------------------------
// Independent expected-test-count discovery for the shipped-cfg guard's package-scoped nextest run
// (deletion-bar row "shipped configuration silently selects zero tests" -> required detector
// "independent expected inventory" — NOT a `runCount !== 0` check, which a regression that compiles out
// every behavioral test while leaving unrelated `#[test]` fns intact would still satisfy). Counts
// `#[test]` attributes by walking a source tree directly — not a hand-maintained name list (CLAUDE.md's
// "Verification Must Prove Execution": "a hand-maintained filename list may not define the primary
// universe unless generated from independent discovery"). Depth-bounded like `findFileByName` above,
// not identity-tracked: this walks small, first-party, non-symlinked crate source trees.
//
// DELIBERATE, ACCEPTED LIMITATION: this is a TEXTUAL scan, not CFG-aware — it does not parse Rust, so it
// can miscount a `#[test]` that appears inside a block comment or raw string, or under a `#[cfg(test)]`
// module gated out of the compiled target, and it requires `#[test]` alone on its own line (whitespace
// aside) — `#[test] fn foo() { .. }` on one line is NOT counted. Hardening this into a real tokenizer was
// weighed against just documenting the gap: this scanner only ever walks
// `crates/verter_shipped_cfg_contract/src` — one small, human-reviewed crate whose sole purpose IS this
// guard's own test inventory, not the general workspace — so pulling in a real Rust parser for it is not
// worth the dependency. If this crate ever grows enough that the risk stops being tolerable, replace this
// with a real tokenizer rather than patching the regex further.
// ----------------------------------------------------------------------------------------------------
const TEST_ATTRIBUTE_LINE = /^[ \t]*#\[test\][ \t]*$/gm;

export function countTestAttributesInDir(root, maxDepth = 16) {
  if (!existsSync(root)) return 0;
  let count = 0;
  const stack = [{ dir: root, depth: 0 }];
  while (stack.length) {
    const { dir, depth } = stack.pop();
    let entries;
    try {
      entries = readdirSync(dir, { withFileTypes: true });
    } catch {
      continue;
    }
    for (const ent of entries) {
      const full = join(dir, ent.name);
      if (ent.isDirectory()) {
        if (depth < maxDepth) stack.push({ dir: full, depth: depth + 1 });
        continue;
      }
      if (!ent.isFile() || !ent.name.endsWith(".rs")) continue;
      let source;
      try {
        source = readFileSync(full, "utf8");
      } catch {
        continue;
      }
      const matches = source.match(TEST_ATTRIBUTE_LINE);
      if (matches) count += matches.length;
    }
  }
  return count;
}

// ----------------------------------------------------------------------------------------------------
// Shipped-cfg guard: the expected-vs-selected test-count VERDICT, pulled out of `runShippedCfgLane`
// (gate.mjs) as the SOLE place this comparison is made. `runShippedCfgLane` calls this function directly
// rather than inlining the comparison — so a regression that "reverts the live guard's check back to a
// bare `runCount !== 0`" necessarily reverts THIS function (there is no separate inline copy left in
// gate.mjs to revert instead while leaving `countTestAttributesInDir` and this decision untouched, which
// is exactly the gap a round-2 review found in the self-test: it exercised `countTestAttributesInDir` in
// isolation but never drove the actual comparison gate.mjs branches on). (GB12.3) in gate-selftest.mjs
// calls this function directly with the same fixture-tree-derived count (GB12.2) exercises, including a
// mutation that simulates "nextest selected fewer tests than the independent scanner counted" — the named
// regression class this guard exists to catch.
//
// Returns `null` when the counts are reconcilable (proceed); otherwise `{ exit, message }` — the exact
// exit code and error text `runShippedCfgLane` records in its structured receipt.
// ----------------------------------------------------------------------------------------------------
export function decideShippedCfgGuardExpectedCountMatch(runCount, expectedTestCount) {
  if (expectedTestCount === 0) {
    return {
      exit: EXIT_USAGE,
      message:
        "SHIPPED-CFG GUARD SETUP FAILURE: the independent source scan of " +
        "crates/verter_shipped_cfg_contract/src found ZERO #[test] attributes. Either the crate's tests " +
        "were deleted/moved, or the scanner itself is broken — refusing to trust a guard whose own " +
        "expected-inventory check cannot count anything.",
    };
  }
  if (runCount !== expectedTestCount) {
    return {
      exit: EXIT_USAGE,
      message:
        `SHIPPED-CFG GUARD SETUP FAILURE: verter_shipped_cfg_contract selected ${runCount} ` +
        `test(s) to run, but an independent scan of crates/verter_shipped_cfg_contract/src found ` +
        `${expectedTestCount} #[test] attribute(s). A guard that silently selects fewer tests than its ` +
        "own source declares proves less than it claims to; refusing to pass it. (A superset — more " +
        "selected than declared — is equally untrusted: it means the scan missed a source file nextest " +
        "did compile.)",
    };
  }
  return null;
}

// ----------------------------------------------------------------------------------------------------
// Extract the trailing JSON object from a nextest `--message-format json` stdout capture. nextest emits a
// single JSON object on stdout (build/compile progress goes to STDERR), but a defensive parse handles a
// capture that prepended log noise: find the first '{' at column 0 (or the first '{'), parse to EOF.
// ----------------------------------------------------------------------------------------------------
export function parseNextestListJson(stdout) {
  const trimmed = stdout.trim();
  // Fast path: the whole capture is the JSON object.
  try {
    return JSON.parse(trimmed);
  } catch {
    /* fall through to a tolerant scan */
  }
  // Tolerant: find the first '{' and parse the balanced object from there.
  const start = trimmed.indexOf("{");
  if (start < 0) throw new Error("no JSON object found in nextest list output");
  // Walk braces honoring strings to find the matching close.
  let depth = 0;
  let inStr = false;
  let escape = false;
  for (let i = start; i < trimmed.length; i++) {
    const ch = trimmed[i];
    if (inStr) {
      if (escape) escape = false;
      else if (ch === "\\") escape = true;
      else if (ch === '"') inStr = false;
      continue;
    }
    if (ch === '"') inStr = true;
    else if (ch === "{") depth++;
    else if (ch === "}") {
      depth--;
      if (depth === 0) {
        const obj = trimmed.slice(start, i + 1);
        return JSON.parse(obj);
      }
    }
  }
  throw new Error("unbalanced JSON object in nextest list output");
}

// ----------------------------------------------------------------------------------------------------
// Setup: repo root, runner target dir, lock path, env.
// ----------------------------------------------------------------------------------------------------
export function resolveRepoRoot(scriptDir) {
  const r = spawnSync("git", ["-C", scriptDir, "rev-parse", "--show-toplevel"], {
    encoding: "utf8",
  });
  const top = (r.stdout || "").trim();
  if (top) {
    try {
      return realpathSync(top);
    } catch {
      return top;
    }
  }
  return "";
}

export function defaultLockDir(repoRealpath) {
  // OS temp dir keyed by repo realpath hash (cross-platform, stable per checkout).
  const h = createHash("sha256").update(repoRealpath).digest("hex").slice(0, 16);
  return join(tmpdir(), `verter-gate.lock.${h}.d`);
}

// ----------------------------------------------------------------------------------------------------
// SCOPE (authoritative). The gate sanitizes a PATH-style value to its CWD-INDEPENDENT ABSOLUTE components
// ONLY — every component that is not a fully-qualified, cwd-independent absolute path is DROPPED — and
// deletes PATH entirely when no absolute component remains. The freshness preflight resolver AND the
// executed cargo/nextest/libtest test children both consume this SAME sanitized PATH view, so the preflight
// verdict and the test execution AGREE on tool resolvability with NO cwd-relative disagreement: the
// preflight can never allow tolerance because it skipped a cwd-relative component the Rust freshness test
// would have used to run `buf`, and the test child can never resolve a tool from a cwd-relative entry the
// preflight never saw. This is the CLOSED invariant — absolute-only is the bounded fix (no filesystem IO, no
// symlink-equivalence normalization, no absolutize-against-base): a real stale-binding regression still
// FAILS because both sides resolve `buf` identically from the cwd-independent absolute PATH.
//
// Sanitize a PATH-style value to its cwd-independent absolute components. POSIX shell / the Rust
// `std::env::split_paths`-based lookup treat an EMPTY path component (from a leading/trailing/doubled
// delimiter — `:/bin`, `/bin:`, `/bin::/usr/bin`) and a bare `.` as the current working directory, and a
// non-dot relative entry (`bin`, `tools/x`), a `..`-relative entry, and a Windows drive-relative `C:foo` /
// root-relative `\x` likewise resolve against a CURRENT directory (or current drive) — so any of them lets
// the repo/CWD control tool resolution. The gate must NOT let that happen: the verdict preflight's JS
// resolver (`resolvePathShim`) and the EXECUTED tests (`std::env::split_paths(PATH) + dir.join(tool)`, where
// `join("", tool) === tool` and a relative `dir.join(tool)` is itself cwd-relative) must resolve from the
// SAME absolute-only PATH. Sanitizing the cargo env PATH here to its absolute components makes both sides
// obey the identical rule — closing the empty-PATH fail-open AND every cwd-relative resolution divergence in
// the test run itself.
//
// We KEEP a component ONLY if `isCwdIndependentAbsolute(component, windows)` (a leading-`/` absolute path on
// POSIX, INCLUDING the bare root `/`; a drive-ROOTED `C:\x` / `C:/x`, a UNC `\\server\share`, or a device
// `\\?\…` / `\\.\…` path on Windows) and DROP everything else: empty entries, the syntactic CWD forms
// `.` / `./` / `.\` (and `./.` etc.), non-dot relative dirs (`bin`, `tools/x`), `..`-relative entries, the
// Windows drive-RELATIVE `C:foo`, and the Windows root-relative `\x` / `/x` (these resolve against the
// CURRENT drive, so they are NOT cwd-independent — dropped, not "proven safe"). A value with NO absolute
// component sanitizes to `""`; the CALLER (`buildCargoEnv`) must then DELETE the env key rather than assign
// `""` — because Rust's `var_os("PATH")?` treats a PRESENT `""` as `split_paths("") == [""]` (an empty
// PathBuf ⇒ a CWD source via `"".join(tool) == tool`), so only removing the key removes the CWD source; an
// assigned `""` would leave it live. An all-relative PATH therefore sanitizes to `""` and the key is deleted
// — that is correct and intended.
// ----------------------------------------------------------------------------------------------------
export function sanitizePathValue(pathValue, windows = IS_WINDOWS) {
  // The LITERAL platform delimiter selected by the `windows` flag (`pathDelimiterFor(windows)` — `;` on
  // Windows, `:` on POSIX), host-INDEPENDENT (NOT the ambient `node:path.delimiter`), so a `windows:false`
  // sanitize splits a POSIX PATH on `:` even on a Windows host (and `windows:true` on `;` even on POSIX) —
  // matching `resolvePathShim`, which splits on the same `pathDelimiterFor(windows)`.
  const delim = pathDelimiterFor(windows);
  return pathValue
    .split(delim)
    .filter((dir) => isCwdIndependentAbsolute(dir, windows))
    .join(delim);
}

// A PATH component is KEPT iff it is a fully-qualified, CWD-INDEPENDENT ABSOLUTE path; every other shape is
// dropped. This is a PURE SYNTACTIC classification — no filesystem IO, no absolutize-against-base, no symlink
// normalization — and it is platform-PARAMETERIZED (the caller's `windows` flag, NOT the ambient
// `node:path.isAbsolute`) so a Windows-mode self-test on a POSIX host classifies Windows path shapes
// correctly. KEEP / DROP:
//   - DROP empty `""` (a leading/trailing/doubled delimiter ⇒ an implicit-CWD source under
//     `std::env::split_paths`).
//   - POSIX: KEEP a leading-`/` absolute path (`/abs/bin`), INCLUDING the bare root `/` (an explicit
//     directory). DROP everything without a leading `/` — dot-only (`.`, `./`, `./.`), non-dot relative
//     (`bin`, `tools/x`), and `..`-relative (`..`, `../tools`) are all CWD-dependent.
//   - WINDOWS: KEEP a drive-ROOTED absolute path (`C:\x` / `C:/x`), a UNC path (`\\server\share`), and a
//     device path (`\\?\…` / `\\.\…`). DROP a drive-RELATIVE `C:foo` (drive + colon WITHOUT a following
//     separator — it resolves against that drive's CURRENT directory), a root-relative `\x` / `/x` (it
//     resolves against the CURRENT drive — drive-current-directory dependent, NOT cwd-independent, so it is
//     dropped, not "proven safe"), and any non-rooted relative form (`tools\x`, `..\x`, `.`).
function isCwdIndependentAbsolute(dir, windows) {
  if (dir === "") return false; // empty component => implicit-CWD source
  if (windows) {
    // UNC (`\\server\share`) and device (`\\?\…` / `\\.\…`) absolute paths — accept either separator on the
    // leading pair. (`\\?\` / `\\.\` are subsumed by the leading double-separator check.)
    if (/^[\\/]{2}/.test(dir)) return true;
    // Drive-ROOTED absolute `C:\x` / `C:/x` (a drive letter + colon + a separator). A drive-RELATIVE `C:foo`
    // (no separator after the colon) is cwd-of-drive dependent => NOT matched, so it is dropped.
    if (/^[A-Za-z]:[\\/]/.test(dir)) return true;
    // A bare root-relative `\x` / `/x` (single leading separator, NOT a drive root, NOT UNC) resolves
    // against the CURRENT drive => drive-current-directory dependent => DROP.
    return false;
  }
  // POSIX: only a leading `/` is absolute (the bare root `/` included). Everything else is cwd-dependent.
  return dir.startsWith("/");
}

// ----------------------------------------------------------------------------------------------------
// Build the cargo env: scrub user target overrides, force the runner-owned dir + non-TTY output, and
// sanitize the PATH var to its CWD-INDEPENDENT ABSOLUTE components ONLY so neither the preflight resolver NOR
// the executed cargo/nextest/libtest tests can source a tool from any cwd-relative PATH entry (empty,
// dot-only, non-dot relative, `..`-relative, or Windows drive-relative / root-relative) — the CLOSED
// invariant that the preflight verdict and the test execution AGREE on tool resolvability (see the SCOPE
// note on `sanitizePathValue`). `windows` (defaulting to the live platform) selects how the PATH var is
// identified AND how duplicate
// case-variants are handled: case-EXACT `PATH` on POSIX vs the case-INSENSITIVE PATH key on Windows — the
// production gate.mjs call uses the default (real platform); the self-test passes `true` to exercise the
// Windows key handling on a POSIX host.
//
// WINDOWS CASE-VARIANT COLLAPSE (load-bearing): Windows folds env-var names case-INSENSITIVELY, so a child
// cargo/nextest process that inherits the returned env sees ONE effective PATH even if the object carries
// several PATH-ish keys (`PATH` ALONGSIDE `Path`/`PaTh`) holding DIFFERENT values. Sanitizing only the one
// `findPathEnvKey` result would leave the other case-variants live, and the child could observe a DIFFERENT
// effective PATH than the JS preflight sanitized/read (the preflight could conclude `buf` absent ⇒ tolerance
// ON while the Rust child resolves+runs `buf` from the unsanitized other-cased variant). So on Windows we:
// (1) collect EVERY key whose `toUpperCase() === "PATH"`; (2) pick the EFFECTIVE value with the SAME policy
// as `findPathEnvKey` (PATH > Path > any-other-case); (3) sanitize THAT value; (4) DELETE every PATH-ish
// case-variant; (5) write back exactly ONE canonical `PATH` key with the sanitized value — UNLESS the
// sanitized value is empty, in which case write NOTHING (the same delete-on-empty invariant, so Rust's
// `var_os("PATH")?` early-returns None). On POSIX there is NO collapse: `var_os("PATH")` is case-EXACT, so a
// `Path` is a DIFFERENT var Rust never reads and MUST be left untouched — the POSIX branch sanitizes exactly
// the single `findPathEnvKey(env, false)` key (`PATH` only) and never touches a `Path`.
// ----------------------------------------------------------------------------------------------------
export function buildCargoEnv(baseEnv, runnerTarget, windows = IS_WINDOWS, buildJobs = null) {
  const env = { ...baseEnv };
  delete env.CARGO_TARGET_DIR;
  delete env.CARGO_BUILD_TARGET_DIR;
  delete env.CARGO_BUILD_BUILD_DIR;
  env.CARGO_TARGET_DIR = runnerTarget;
  if (buildJobs !== null && buildJobs !== undefined) {
    const jobs = positiveInteger(buildJobs, "build jobs");
    // Windows folds environment names, so remove every casing before installing the one canonical cap.
    for (const key of Object.keys(env)) {
      if ((windows ? key.toUpperCase() : key) === "CARGO_BUILD_JOBS") delete env[key];
    }
    env.CARGO_BUILD_JOBS = String(jobs);
  }
  // Force non-TTY / CI-style output so progress lands in the captured log, not a TTY spinner.
  env.CARGO_TERM_COLOR = "never";
  env.CARGO_TERM_PROGRESS_WHEN = "never";
  // RUNNER ENVIRONMENT: CONSTRUCTED, NOT INHERITED.
  //
  // The SURFACE-1 verdict is parsed out of nextest's output, so any variable that changes that output
  // changes the verdict. Pinning them one at a time as each is discovered is a DENYLIST and it lost
  // three times running: `NEXTEST_FINAL_STATUS_LEVEL` (suppresses the failure recap), then
  // `NEXTEST_NO_OUTPUT_INDENT` (strips the capture indent the layout rule rests on), then
  // `NEXTEST_FAILURE_OUTPUT`/`NEXTEST_SUCCESS_OUTPUT` (move captured output AFTER the real Summary, so a
  // test can print a Summary that the parser reads instead) and `NEXTEST_RETRIES` (makes a genuine
  // supersession possible, turning tolerance-refusal into a false RED on a legitimate flaky run).
  //
  // So the `NEXTEST_*` namespace is STRIPPED and exactly the required set is written back. A variable
  // nobody has named yet is closed by construction rather than by the next review round.
  //
  // BUT THE STRIP OWNS OUTPUT FORMAT, NOT CALLER INTENT. `NEXTEST_PROFILE` selects WHICH CONFIGURATION
  // RUNS; it does not change how output is formatted. That is the same category as `CARGO_*`, `PATH`,
  // `TMPDIR` and `RUST*`, none of which are stripped either. Swallowing it broke a contract in the
  // WORSE direction - a broken GREEN run: CI runs this gate with `NEXTEST_PROFILE: ci`, junit is
  // declared ONLY under `[profile.ci.junit]`, and the workflow step after the gate locates that file
  // and fails when it is missing, so every passing CI run would exit 1 on a missing artifact.
  //
  // Preserving it cannot reopen the parse, and that is MEASURED rather than assumed: against the real
  // binary, a hostile profile (`status-level`/`final-status-level = none`, `failure-output = final`,
  // `retries = 3`) yields ZERO `FAIL [` lines unopposed, and the SAME profile under the pins below
  // yields the correct FAIL lines, one Summary and no TRY lines. The pins beat the profile for every
  // parser-facing setting, so the profile decides which config runs and the pins decide what is
  // printed. Guarded by the CI junit contract in the self-test, derived from ci.yml + nextest.toml so
  // it follows a profile rename and still catches a future strip.
  // The fold is PLATFORM-ACCURATE, exactly as the PATH handling below already is. Windows env names
  // fold case-INSENSITIVELY, so `Nextest_Profile` and `NEXTEST_PROFILE` are ONE variable to a Windows
  // child and a case-SENSITIVE strip would leave every mixed-case spelling alive - the allowlist would
  // have exactly the hole it exists to close, on the one platform this host cannot run. POSIX is
  // case-EXACT: there `Nextest_Profile` is a genuinely DIFFERENT variable that nextest never reads, so
  // deleting it would be this gate reaching outside its own contract.
  const inNamespace = (k) => (windows ? k.toUpperCase() : k).startsWith("NEXTEST_");
  const isProfileKey = (k) => (windows ? k.toUpperCase() : k) === "NEXTEST_PROFILE";
  // Read the caller's profile from whatever spelling they used, then collapse every variant onto the
  // ONE canonical key, so a Windows child never sees two colliding spellings of it.
  let callerProfile;
  for (const k of Object.keys(env)) {
    if (isProfileKey(k)) callerProfile = env[k];
  }
  for (const k of Object.keys(env)) {
    if (inNamespace(k)) delete env[k];
  }
  if (callerProfile !== undefined) env.NEXTEST_PROFILE = callerProfile;
  // Colour FORCING would inject ANSI escapes into the status column and break the 12-column field the
  // parser gates on. These only ever force colour ON, so deleting them is enough; `NO_COLOR` is
  // deliberately NOT set, because that is a variable the TESTS themselves may legitimately read.
  const COLOUR_FORCING = new Set(["CLICOLOR_FORCE", "CLICOLOR", "FORCE_COLOR"]);
  for (const k of Object.keys(env)) {
    if (COLOUR_FORCING.has(windows ? k.toUpperCase() : k)) delete env[k];
  }
  // The declared set, each with the reason the parser depends on it:
  env.NEXTEST_HIDE_PROGRESS_BAR = "1"; // no spinner interleaved into the captured log
  env.NEXTEST_NO_OUTPUT_INDENT = "0"; // keep the 4-space capture indent (the layout layer's basis)
  // "pass" (nextest ordering: none < fail < retry < slow < leak < pass < skip < all) prints a terminal
  // status line for every FAIL/retry AND every plain PASS. "retry" (the prior pin) sits BELOW "pass" in
  // that ordering, so it printed fail/retry lines only — on an all-passing run that is ZERO status lines,
  // which is why the per-test timing telemetry below read 0/0 parseable durations against a real gate run:
  // there was nothing to parse. "pass" is a strict superset of "retry" for parser-facing content (every
  // FAIL/TRY line "retry" produced still prints identically), so this changes nothing the verdict path
  // reads — it only adds PASS lines, which `extractNextestTerminalFailures` already classifies as
  // `kind: "pass"` and ignores for failure purposes. Not "all": that would also add SKIP lines, which carry
  // no timing and the telemetry below has no use for.
  env.NEXTEST_STATUS_LEVEL = "pass"; // terminal status for every test, including plain passes (telemetry)
  env.NEXTEST_FINAL_STATUS_LEVEL = "fail"; // the trailing failure recap the parser reads
  env.NEXTEST_RETRIES = "0"; // no genuine fail->pass supersession, so tolerance-refusal cannot false-RED
  env.NEXTEST_SUCCESS_OUTPUT = "never"; // captured output of PASSING tests never enters the stream
  env.NEXTEST_FAILURE_OUTPUT = "immediate"; // and failing output stays INLINE, never after the Summary
  // NOTHING else from the namespace is passed through. `NEXTEST_PROFILE` is the single exception above,
  // and it is an exception on a stated ground: it expresses which configuration the caller asked to
  // run, and every parser-facing setting is pinned here regardless of what that configuration says.

  // Sanitize the PATH var to its CWD-INDEPENDENT ABSOLUTE components so the executed tests obey the SAME
  // no-CWD-tool-source rule the verdict preflight resolver uses (closes every cwd-relative fail-open, not just
  // the empty/CWD-buf one). On both platforms the sanitized value is computed from the PATH var the Rust
  // freshness test reads via `var_os("PATH")` — identified by `findPathEnvKey` (PATH > Path > any-other-case
  // on Windows; case-EXACT `PATH` on POSIX). When sanitization yields a NON-empty value it is written back;
  // when it yields the EMPTY STRING (a PATH with NO absolute component — e.g. ":" / "." / ":." / "./:." or an
  // all-relative "bin:tools") the PATH key is DELETED instead of assigning "". This is load-bearing:
  // Rust's `std::env::var_os("PATH")?` early-returns `None` ONLY when the key is ABSENT — a PRESENT empty
  // value (`Some("")`) is NOT None, so it reaches `std::env::split_paths("")`, which yields ONE empty PathBuf,
  // and `"".join("buf") == "buf"` (relative ⇒ CWD) resolves a CWD `buf` that RUNS. Assigning "" would leave
  // the CWD source live; deleting the key is the only form that removes it.
  if (windows) {
    // Windows env names fold case-INSENSITIVELY, so the inherited child sees ONE effective PATH even with
    // several PATH-ish keys. Collect EVERY case-variant, pick the effective value with the SAME policy as
    // `findPathEnvKey` (reuse it), sanitize THAT, DELETE every variant, then write back exactly ONE canonical
    // `PATH` key with the sanitized value (or NOTHING on empty). This prevents an unsanitized other-cased
    // variant from surviving as an executable-child PATH source the preflight never saw.
    const pathishKeys = Object.keys(env).filter((k) => k.toUpperCase() === "PATH");
    const effectiveKey = findPathEnvKey(env, true);
    const sanitized = effectiveKey !== null ? sanitizePathValue(env[effectiveKey], true) : null;
    for (const k of pathishKeys) delete env[k];
    if (sanitized !== null && sanitized !== "") env.PATH = sanitized;
  } else {
    // POSIX: `var_os("PATH")` is case-EXACT, so ONLY the exact `PATH` is the var Rust reads — a `Path` is a
    // DIFFERENT var that must be LEFT UNTOUCHED. Sanitize exactly that single key (and only if present);
    // never collapse case-variants here (that would wrongly delete a legitimate POSIX `Path`).
    const pathKey = findPathEnvKey(env, false);
    if (pathKey !== null) {
      const sanitized = sanitizePathValue(env[pathKey], false);
      if (sanitized === "") delete env[pathKey];
      else env[pathKey] = sanitized;
    }
  }
  return env;
}

// ----------------------------------------------------------------------------------------------------
// Dormant legacy Surface-2 helper retained alongside the selftest classifier; production gate.mjs does not
// call it. It derives per-suite package identity from archive list JSON without a separate `cargo metadata`
// subprocess, preserving the retired direct-libtest fixture's original environment construction.
// ----------------------------------------------------------------------------------------------------
export function deriveSuitePkgInfo(suite) {
  const name = suite["package-name"] || "";
  // package-id forms: "path+file:///…/crates/verter_session#0.0.1-beta.1" (version after the LAST '#'),
  // or the older "verter_session 0.0.1-beta.1 (path+file://…)" form. Extract the semver defensively.
  const id = suite["package-id"] || "";
  let version = "";
  const hash = id.lastIndexOf("#");
  if (hash >= 0) {
    const tail = id.slice(hash + 1);
    // "name@version" or just "version".
    const at = tail.lastIndexOf("@");
    version = at >= 0 ? tail.slice(at + 1) : tail;
  } else {
    const m = /\s(\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?)\s/.exec(` ${id} `);
    if (m) version = m[1];
  }
  return { name, version };
}

// ----------------------------------------------------------------------------------------------------
// Legacy Surface-2 suite-selection selftest fixture. Production gate.mjs no longer calls this helper; the
// current shared-process contracts are ordinary tests inside archive-backed Surface 1. The retained frozen
// classifier mirrors the retired `cargo test -p verter_session --tests` selection: the lib unit-test binary
// plus every `tests/*.rs` integration binary, excluding bins/benches. It returns `{ suites, lib, test,
// error }` so gate-selftest.mjs can preserve the historical zero/partial-selection regression proof without
// implying that a second live surface still exists.
// ----------------------------------------------------------------------------------------------------
export function selectSessionSuites(allSuites) {
  const suites = (allSuites || []).filter(
    (s) => s["package-name"] === "verter_session" && (s.kind === "lib" || s.kind === "test"),
  );
  const lib = suites.filter((s) => s.kind === "lib").length;
  const test = suites.filter((s) => s.kind === "test").length;
  let error = null;
  if (suites.length === 0) {
    error =
      "zero verter_session lib/test suites found in the archive listing — the retired shared-process " +
      "surface would have been silently skipped. Legacy classifier refuses the selection.";
  } else if (lib < 1 || test < 1) {
    error =
      `verter_session suite filter is incomplete (lib=${lib}, test=${test}; expected >=1 of each). ` +
      "A partial filter would have under-covered the retired shared-process surface. Legacy classifier refuses.";
  }
  return { suites, lib, test, error };
}

// Dormant legacy Surface-2 helper; production gate.mjs does not call it. Per-package Cargo env for the
// retired directly executed test binary. This injects the runtime Cargo env the
// verter_session integration tests ACTUALLY read — CARGO_MANIFEST_DIR and CARGO_TARGET_DIR — verified
// complete for this suite (the only runtime `std::env::var(_os)` Cargo lookups in the verter_session test
// sources are `CARGO_MANIFEST_DIR` and `CARGO_TARGET_DIR`; `CARGO_TARGET_DIR` is already forced on the base
// cargo env to the runner-owned dir, and the manifest dir IS the suite cwd). It does NOT claim to
// reproduce the FULL env Cargo passes (it omits e.g. dynamic-library search-path setup and per-test
// tmp/bin vars) — only the subset this suite reads. The CARGO_PKG_NAME/VERSION pair is a faithful extra
// derived from the same archive list JSON (NOT a subprocess); it is not load-bearing for this suite.
export function buildSuiteEnv(baseCargoEnv, manifestDir, pkgInfo, crateName) {
  const env = { ...baseCargoEnv };
  // Load-bearing: the package manifest dir Cargo sets for the test process (tests read it via
  // std::env::var("CARGO_MANIFEST_DIR") to resolve the repo root + corpus fixtures). cwd IS the manifest
  // dir. CARGO_TARGET_DIR is already present on baseCargoEnv (forced to the runner-owned target).
  env.CARGO_MANIFEST_DIR = manifestDir;
  if (crateName) env.CARGO_CRATE_NAME = crateName.replace(/-/g, "_");
  if (pkgInfo) {
    if (pkgInfo.name) env.CARGO_PKG_NAME = pkgInfo.name;
    if (pkgInfo.version) {
      env.CARGO_PKG_VERSION = pkgInfo.version;
      const m = /^(\d+)\.(\d+)\.(\d+)(?:[-+](.*))?$/.exec(String(pkgInfo.version));
      env.CARGO_PKG_VERSION_MAJOR = m ? m[1] : "";
      env.CARGO_PKG_VERSION_MINOR = m ? m[2] : "";
      env.CARGO_PKG_VERSION_PATCH = m ? m[3] : "";
      env.CARGO_PKG_VERSION_PRE = m && m[4] ? m[4] : "";
    }
  }
  return env;
}

// Map a contained-step result to an exit code. A watchdog reason wins; otherwise the child's own exit
// (0 => PASS, non-zero => FAIL).
export function mapStepReason(res) {
  if (res.reason === "MEMORY" || res.reason === "MEMORY_MONITOR") return EXIT_MEMORY;
  if (res.reason === "TIMEOUT") return EXIT_TIMEOUT;
  if (res.reason === "STALL") return EXIT_STALL;
  if (res.code === 0) return EXIT_PASS;
  return EXIT_FAIL;
}

// Parse nextest's trailing "Summary [   …s] N tests run: P passed, S skipped" line for counts.
// Returns `found`: whether a `Summary [` line was actually present. The live SURFACE-1 accounting REQUIRES
// `found === true` to treat a non-zero run exit as accounted-for — a missing/unparseable Summary (a setup
// or harness error, a killed run) cannot prove the failures are accounted for, so it must FAIL the gate
// rather than fall through to PASS-WITH-TOLERATED on the parsed `FAIL` names alone.
// nextest emits, at the end of a run:
//     Summary [  63.890s] 15543 tests run: 15541 passed, 547 skipped
//     Summary [ 900.014s] 12 tests run: 8 passed, 2 failed, 2 timed out, 5 skipped
//     Summary [   1.003s] 4 tests run: 2 passed (1 leaky), 2 timed out, 2 skipped
//     Summary [   1.516s] 2/41 tests run: 1 passed, 1 failed, 0 skipped
//
// THREE facts that the naive `(\d+) failed` read gets wrong, each a way for the gate to certify a tree it
// must not:
//   1. `failed` is the PLAIN-assertion-failure count only. A test that TIMED OUT or that nextest could not
//      EXECUTE is reported under its own separate count (`N timed out`, `N exec failed`) and is NOT in
//      `failed`. A timed-out test has not passed — it has not even finished.
//   2. `A/B tests run` (the cancelled/interrupted form) means B-A tests NEVER RAN. `A tests run` alone
//      means every selected test finished.
//   3. `N passed (M leaky)` / `(M slow)` / `(M flaky)` are ANNOTATIONS on the passed count, not extra
//      outcomes — those tests are inside `passed`.
//
// So the authoritative failure total is derived, not label-matched: `nonPassed = runCount - passed` counts
// every test that ran and did not pass, WHATEVER nextest labelled it. A failure class this parser has never
// heard of still lands in `nonPassed`, which is what makes the accounting fail-closed rather than a
// perpetually-incomplete list of label regexes. `unrun = initialCount - runCount` is the never-executed
// count. `failed` / `timedOut` / `execFailed` are kept for reporting only.
//
// Returns `found`: whether a `Summary [` line was present at all — the live SURFACE-1 accounting REQUIRES
// `found === true` to treat a non-zero run exit as accounted-for, since a missing/unparseable Summary (a
// setup or harness error, a killed run) cannot prove the failures are accounted for. `runCountFound` is the
// same guarantee for the `N tests run` clause the derivation above depends on.
export function parseNextestSummary(text) {
  let passed = 0;
  let skipped = 0;
  let failed = 0;
  let timedOut = 0;
  let execFailed = 0;
  let runCount = 0;
  let initialCount = 0;
  let runCountFound = false;
  // AUTHORSHIP. A `Summary [` substring match anywhere on any line trusted whatever the stream happened
  // to contain: with captured output placed after the run, a failing test printing its own Summary line
  // REPLACED the runner's accounting and `nonPassed` went to 0 with a real FAIL still in the log. The
  // real Summary occupies the SAME 12-column field as a status line (verified 8/8 on real runs), so it
  // carries the SAME layout gate, which rejects the 4-space-indented captured copy. And a run emits
  // EXACTLY ONE Summary (also 8/8), so a second layout-valid one is not something to disambiguate by
  // position - it is proof the accounting was forged, reported via `count` for the caller to fail on.
  const lines = text.split("\n").filter((l) => {
    const m = NEXTEST_STATUS_LINE.exec(l);
    return m !== null && m[1].trim() === "Summary";
  });
  const found = lines.length > 0;
  // REFUSE, do not choose. With more than one layout-valid Summary the parser has no basis to prefer
  // either, so it derives NO accounting from them: `runCountFound` stays false and every derived count
  // stays zero. `count` is reported so the caller fails with a Summary-specific reason. Taking the last
  // one was a positional choice the parser was not entitled to make, even though the live path already
  // failed closed on the duplicate.
  if (lines.length > 1) {
    return {
      count: lines.length,
      passed: 0,
      skipped: 0,
      failed: 0,
      timedOut: 0,
      execFailed: 0,
      runCount: 0,
      initialCount: 0,
      runCountFound: false,
      nonPassed: 0,
      unrun: 0,
      found,
    };
  }
  const line = found ? lines[0] : "";
  let m = /(\d+)\s+passed/.exec(line);
  if (m) passed = parseInt(m[1], 10);
  m = /(\d+)\s+skipped/.exec(line);
  if (m) skipped = parseInt(m[1], 10);
  // `(\d+)\s+failed` cannot match inside "3 exec failed" (the digits are followed by " exec ", not by
  // whitespace + "failed"), so the plain and exec-failed counts stay distinct.
  m = /(\d+)\s+failed/.exec(line);
  if (m) failed = parseInt(m[1], 10);
  m = /(\d+)\s+timed\s+out/.exec(line);
  if (m) timedOut = parseInt(m[1], 10);
  m = /(\d+)\s+exec\s+failed/.exec(line);
  if (m) execFailed = parseInt(m[1], 10);
  m = /(\d+)(?:\/(\d+))?\s+tests?\s+run/.exec(line);
  if (m) {
    runCountFound = true;
    runCount = parseInt(m[1], 10);
    initialCount = m[2] != null ? parseInt(m[2], 10) : runCount;
  }
  const nonPassed = Math.max(0, runCount - passed);
  const unrun = Math.max(0, initialCount - runCount);
  return {
    count: lines.length,
    passed,
    skipped,
    failed,
    timedOut,
    execFailed,
    runCount,
    initialCount,
    runCountFound,
    nonPassed,
    unrun,
    found,
  };
}

// ----------------------------------------------------------------------------------------------------
// Shell invocation for an arbitrary command STRING (used by the multi-step seam's `name|cmd` specs). The
// command is run via `bash -c <string>` (POSIX) / `cmd /c <string>` (Windows) so a shell snippet's
// `&`/`wait` work. A seam step is a TEST-phase step (byte-growth-only liveness). This is a SEAM/test
// primitive — the PRODUCTION gate never runs an arbitrary command string; it only ever spawns cargo +
// the archived libtest binaries with explicit argv arrays.
// ----------------------------------------------------------------------------------------------------
export function shellInvocation(cmdString) {
  if (IS_WINDOWS) return { cmd: "cmd.exe", args: ["/d", "/s", "/c", cmdString] };
  return { cmd: "bash", args: ["-c", cmdString] };
}

// ----------------------------------------------------------------------------------------------------
// Multi-step seam runner — drives the REAL whole-gate budget bound (the shared `deadlineMs` across every
// step) with `name|cmd` stand-in steps so the budget/stall/timeout semantics can be proven cargo-free.
// This is a TEST/SEAM building block used ONLY by the self-test harness (which calls it in-process with a
// crafted step list); it is NOT wired into any production CLI path, so no environment variable or argv on
// the production gate can reach it. `steps` is an array of "<name>|<cmdString>" specs.
// ----------------------------------------------------------------------------------------------------
export async function runMultiStepSeam(ctx) {
  const { steps, cargoEnv, repoRealpath, runnerTarget, deadlineMs, supervisor } = ctx;
  const specs = (steps || []).filter((l) => String(l).trim());
  let overall = EXIT_PASS;
  for (const spec of specs) {
    const bar = spec.indexOf("|");
    const name = bar >= 0 ? spec.slice(0, bar) : spec;
    const cmdStr = bar >= 0 ? spec.slice(bar + 1) : spec;
    const remaining = deadlineMs - nowMs();
    if (remaining <= 0) {
      warn(`whole-gate budget exhausted before step '${name}' => TIMEOUT`);
      overall = EXIT_TIMEOUT;
      break;
    }
    const inv = shellInvocation(cmdStr);
    const res = await supervisor.runStep("selftest-seam", {
      cmd: inv.cmd,
      args: inv.args,
      cwd: repoRealpath,
      env: cargoEnv,
      phase: "test",
      deadlineMs,
      targetDir: runnerTarget,
    });
    log(
      `step ${name}: exit=${res.code} reason=${res.reason || "-"} secs=${Math.round(res.durationMs / 1000)}`,
    );
    const rc = mapStepReason(res);
    if (rc !== EXIT_PASS) {
      overall = rc;
      break;
    }
  }
  return overall;
}

// Re-export the path helpers the production gate composes with so gate.mjs imports a single module.
export { join, dirname, basename, sep, isAbsolute };
export {
  mkdirSync,
  writeFileSync,
  readFileSync,
  existsSync,
  rmSync,
  renameSync,
  statSync,
} from "node:fs";
export { spawn, spawnSync } from "node:child_process";

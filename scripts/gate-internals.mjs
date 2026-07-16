// gate-internals.mjs — the reusable internals of the canonical agent Rust gate.
//
// This module holds the load-bearing primitives, classifiers, parsers, the single-flight mutex, the
// contained-step runner, and the multi-step seam runner that BOTH the production gate CLI (`gate.mjs`)
// and the self-test (`gate-selftest.mjs`) build on. It contains NO CLI dispatch, NO argv parsing, NO
// `process.exit`, and NO top-level side effects: importing it runs nothing. The production gate composes
// these into the real gate (archive → nextest → direct libtest → verdict); the self-test imports the
// classifiers/primitives DIRECTLY (as functions) to drive its scenarios in-process — so the production
// gate binary never has to expose a test-seam / classifier-hook / custom-command mode that could exit 0
// without actually building and running the test suite.
//
// SECURITY INVARIANT: the production CLI (`gate.mjs`) imports the gate-execution pieces here but NEVER
// exposes a CLI mode that returns the gate success contract without running the real gate. The seam
// runner (`runMultiStepSeam`) and the contained-step runner (`runContainedStep`) are reusable building
// blocks; only the SELF-TEST script drives the cargo-free seam, and only via its OWN dedicated harness —
// never via a magic flag on the production gate.

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
import { tmpdir } from "node:os";
import { join, dirname, basename, sep, isAbsolute, win32, posix } from "node:path";
import { createHash } from "node:crypto";

// ----------------------------------------------------------------------------------------------------
// Exit-code constants (distinct, documented). Shared with gate.mjs and gate-selftest.mjs.
//   0   PASS / PASS-WITH-TOLERATED
//   1   FAIL          (a build/test command failed / a non-tolerated test failed)
//   124 TIMEOUT       (whole-gate wallclock deadline tripped)
//   125 STALL         (no progress within the stall window)
//   126 LOCK-REFUSED  (another gate holds the single-flight mutex and is alive / lock uninspectable)
//   127 USAGE/SETUP   (bad arguments, repo root not found, archive/list setup failure)
// ----------------------------------------------------------------------------------------------------
export const EXIT_PASS = 0;
export const EXIT_FAIL = 1;
export const EXIT_TIMEOUT = 124;
export const EXIT_STALL = 125;
export const EXIT_LOCK_REFUSED = 126;
export const EXIT_USAGE = 127;

export const IS_WINDOWS = process.platform === "win32";
export const IS_MAC = process.platform === "darwin";

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
  windows = IS_WINDOWS,
  existsFn = existsSync,
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
    const destinationPdb = toPdb(resolvedBinary);
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
//           · watchdog reason (TIMEOUT/STALL)       → action "watchdog" (PROPAGATED via `mapStepReason`,
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

  // A watchdog kill (TIMEOUT/STALL) from the install step is PROPAGATED, never tolerated.
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
export const TOLERATED_TEST_NAMES = new Set([
  // Post-consolidation, both env-only freshness tests live in the single `verter_protocol::main`
  // integration binary under the module path `cases::typeinfo_proto_ts_freshness::<fn>`. nextest renders
  // a run line as "<STATUS> [   …s] (n/m) verter_protocol::main cases::typeinfo_proto_ts_freshness::<fn>"
  // (the last whitespace token is the bare libtest path), and a direct libtest run prints
  // "test cases::typeinfo_proto_ts_freshness::<fn> ... FAILED" — so the EXACT name on BOTH surfaces is the
  // `cases::`-prefixed module path. (Pre-consolidation these were a standalone `typeinfo_proto_ts_freshness`
  // binary; that bare/`typeinfo_proto_ts_freshness::`-qualified form no longer exists in the archive.)
  "cases::typeinfo_proto_ts_freshness::typeinfo_ts_bindings_are_byte_equal_to_regenerated_buf_output",
  "cases::typeinfo_proto_ts_freshness::proto_ts_bindings_byte_pinned_repo_wide",
]);

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

export function preparedSuccessLines(suiteCount, warmed, warmFailures, missing) {
  // NB: no line below may contain the token "PASS" — a CI `grep PASS` of prepare's output must find nothing
  // that looks like a gate verdict. `assertNoPassToken` enforces this on the assembled array.
  const lines = [
    `prepare: archived + listed ${suiteCount} suites; warmed first-launch assessment for ${warmed} ` +
      `binaries (${warmFailures} warm-list failure(s), ${missing} missing binary/-ies)`,
    "prepare is a PRE-WARM (it moves the legitimate first-launch assessment earlier); it does NOT disable " +
      "Gatekeeper or remove the cost, and it is NOT a gate verdict — the gate is `node scripts/gate.mjs`.",
    `${PREPARE_SUCCESS_MARKER}: tests were NOT run — run the gate (\`node scripts/gate.mjs\`, no mode flag) ` +
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
export function procIdentity(pid) {
  if (!/^\d+$/.test(String(pid))) return "";
  if (IS_WINDOWS) {
    // PowerShell CIM creation date (preferred), falling back to wmic.
    let r = spawnSync(
      "powershell",
      [
        "-NoProfile",
        "-Command",
        `(Get-CimInstance Win32_Process -Filter 'ProcessId=${pid}').CreationDate`,
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
      out = (r.stdout || "").trim();
    }
    return out.replace(/\s+/g, " ").trim();
  }
  const r = spawnSync("ps", ["-o", "lstart=", "-p", String(pid)], { encoding: "utf8" });
  return (r.stdout || "").trim().replace(/\s+/g, " ");
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
  // Returns [{ pid, cmd }]. POSIX: `ps -axww -o pid=,command=`. Windows: `wmic process get ...` or CIM.
  if (IS_WINDOWS) {
    let r = spawnSync(
      "powershell",
      [
        "-NoProfile",
        "-Command",
        'Get-CimInstance Win32_Process | ForEach-Object { "$($_.ProcessId)`t$($_.CommandLine)" }',
      ],
      { encoding: "utf8", windowsHide: true, maxBuffer: 64 * 1024 * 1024 },
    );
    let out = r.stdout || "";
    if (!out.trim()) {
      r = spawnSync("wmic", ["process", "get", "ProcessId,CommandLine", "/format:csv"], {
        encoding: "utf8",
        windowsHide: true,
        maxBuffer: 64 * 1024 * 1024,
      });
      out = r.stdout || "";
    }
    const rows = [];
    for (const line of out.split(/\r?\n/)) {
      const tabIdx = line.indexOf("\t");
      if (tabIdx > 0) {
        const pid = line.slice(0, tabIdx).trim();
        const cmd = line.slice(tabIdx + 1).trim();
        if (/^\d+$/.test(pid)) rows.push({ pid: parseInt(pid, 10), cmd });
        continue;
      }
      // wmic CSV fallback: Node,CommandLine,ProcessId
      const parts = line.split(",");
      if (parts.length >= 3) {
        const pid = parts[parts.length - 1].trim();
        const cmd = parts
          .slice(1, parts.length - 1)
          .join(",")
          .trim();
        if (/^\d+$/.test(pid)) rows.push({ pid: parseInt(pid, 10), cmd });
      }
    }
    return rows;
  }
  const r = spawnSync("ps", ["-axww", "-o", "pid=,command="], {
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
  });
  const out = r.stdout || "";
  const rows = [];
  for (const line of out.split("\n")) {
    const trimmed = line.replace(/^\s+/, "");
    if (!trimmed) continue;
    const sp = trimmed.indexOf(" ");
    if (sp < 0) continue;
    const pidTok = trimmed.slice(0, sp);
    if (!/^\d+$/.test(pidTok)) continue;
    const cmd = trimmed.slice(sp + 1);
    rows.push({ pid: parseInt(pidTok, 10), cmd });
  }
  return rows;
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

export async function provenanceSweep(targetDir, graceMs) {
  if (!targetDir) return;
  const self = process.pid;
  const term = (pid) => {
    if (IS_WINDOWS) {
      // /T tears down the whole tree (a swept cargo.exe may have spawned rustc.exe children), /F forces it.
      spawnSync("taskkill.exe", ["/PID", String(pid), "/T", "/F"], {
        windowsHide: true,
        stdio: "ignore",
      });
    } else {
      try {
        process.kill(pid, "SIGTERM");
      } catch {
        /* ignore */
      }
    }
  };
  const kill = (pid) => {
    if (IS_WINDOWS) {
      spawnSync("taskkill.exe", ["/PID", String(pid), "/T", "/F"], {
        windowsHide: true,
        stdio: "ignore",
      });
    } else {
      try {
        process.kill(pid, "SIGKILL");
      } catch {
        /* ignore */
      }
    }
  };
  const matches = () =>
    listProcesses().filter(
      (p) => p.pid !== self && isBuildTool(p.cmd) && cmdReferencesTargetDir(p.cmd, targetDir),
    );
  // TERM pass.
  for (const p of matches()) term(p.pid);
  await delay(Math.min(graceMs, 1500));
  // KILL pass.
  for (const p of matches()) kill(p.pid);
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
        const proveReuse = holderIdent && liveIdent && holderIdent !== liveIdent;
        if (!proveReuse) {
          const ageS = Math.round((nowMs() - (owner.createdAtMs || this._lockdirBirthMs())) / 1000);
          if (holderIdent && liveIdent) {
            // Identities both present and equal => genuinely the same live holder.
            this.refuseDetail = `live holder pid=${holderPid} age=${ageS}s targetDir=${owner.targetDir || "?"}`;
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
// Module-level ACTIVE-STEP handle. runContainedStep registers the CURRENT live child (its pid IS the PGID
// on POSIX; the tree root on Windows) here for the duration of that step and clears it on completion. The
// SIGINT/SIGTERM teardown reads this (via reapActiveStep) and reaps the SAME tree (the negative-PGID
// TERM→grace→KILL / Windows `taskkill /T /F`) BEFORE running the provenance sweep and releasing the mutex —
// so an external signal to ONLY the gate pid (not the whole group) cannot leave a running
// cargo/nextest/libtest tree orphaned while the lock is released and a second gate starts. Without this,
// the signal handler ran only the provenance sweep, which skips direct libtest binaries and any
// non-build-tool child, leaving the active test tree alive past lock release.
// ----------------------------------------------------------------------------------------------------
let ACTIVE_STEP = null; // { pid, targetDir, killGraceMs } | null

// Reap the active step's whole tree, returning the VERIFIED reap outcome (so teardown can record whether
// the tree was confirmed dead before release). Returns null when there is no active step.
export async function reapActiveStep() {
  const active = ACTIVE_STEP;
  if (!active || !active.pid) return null;
  try {
    return await reapTree(active.pid, active.killGraceMs);
  } catch {
    /* best-effort reap */
    return { reaped: true, confirmedDead: false, wasLive: true };
  }
}

// ----------------------------------------------------------------------------------------------------
// runContainedStep — launch one external command in a NEW process group (POSIX) / job-tree (Windows) under
// the whole-gate deadline + the phase-appropriate stall detector, capturing combined stdout+stderr to a
// growing buffer (also mirrored to our stderr). Returns
// { code, reason, durationMs, stdout, stderr, spawnError, reapConfirmedDead, signalName }.
//   reason: "TIMEOUT" | "STALL" | "" (empty when not a watchdog kill).
//   reapConfirmedDead: when a watchdog reap ran, whether the child tree was VERIFIED dead afterward
//     (true), false if death could not be confirmed within the bound. Undefined when no reap ran.
//   signalName: the SIGNAL name the child was terminated by (e.g. "SIGABRT") when it was signal-killed
//     (code===128 stand-in), "" when it exited normally. Lets a caller report the real signal instead of
//     the misleading synthesized "exit 128".
//
//   phase: "build" => progress is byte growth OR target-tree artifact growth.
//          "test"  => progress is byte growth ONLY (a silent test binary is a hang).
//
//   deadlineMs: the WHOLE-GATE absolute deadline (ms epoch). The step is bounded by it; when it passes the
//               step is reaped as TIMEOUT. (The same deadline is shared across every step so the budget is
//               whole-gate, not per-step.)
// ----------------------------------------------------------------------------------------------------
export async function runContainedStep(opts) {
  const {
    cmd,
    args,
    cwd,
    env,
    phase,
    deadlineMs,
    stallMs,
    targetDir,
    killGraceMs = 5000,
    captureStdoutSeparately = false,
    windowsVerbatimArguments = false,
  } = opts;

  const child = spawn(cmd, args, {
    cwd,
    env,
    shell: false,
    detached: !IS_WINDOWS, // POSIX: new process group (setsid). Windows: taskkill /T is the tree primitive.
    windowsHide: true,
    // Forwarded for the `cmd /d /s /c "<quoted>"` pnpm-install launch (the args carry one pre-quoted
    // verbatim element); default false leaves every other step's normal Node arg-quoting untouched.
    windowsVerbatimArguments,
    stdio: ["ignore", "pipe", "pipe"],
  });
  // Publish the live child as the active step so the signal teardown can reap its WHOLE tree (not just the
  // sweep's build-tool subset) before releasing the lock. Cleared in the close handler below.
  ACTIVE_STEP = { pid: child.pid, targetDir, killGraceMs };

  let stdoutBuf = "";
  let stderrBuf = "";
  let totalBytes = 0;
  let lastGrowthMs = nowMs();
  let lastSize = -1;
  let lastArtifact = "";

  child.stdout.on("data", (d) => {
    const s = d.toString();
    totalBytes += d.length;
    if (captureStdoutSeparately) {
      stdoutBuf += s;
    } else {
      stdoutBuf += s;
      process.stderr.write(s);
    }
  });
  child.stderr.on("data", (d) => {
    const s = d.toString();
    totalBytes += d.length;
    stderrBuf += s;
    process.stderr.write(s);
  });

  let reason = "";
  let reaped = false;
  // Did the watchdog's reap actually signal a LIVE child/process group? Set SYNCHRONOUSLY by reapNow at the
  // instant it begins the reap (before any await), so the close handler reads a settled value even when the
  // child resolves `close` in the same tick. A real TIMEOUT/STALL reap hits a live group (true); a one-tick
  // race where the child had already exited before we signaled gets ESRCH (false). The close handler clears
  // a spurious `reason` ONLY when this is false — so a process that TRAPS SIGTERM and exits(0)
  // (watchdogSignaledLive=true) keeps its TIMEOUT/STALL verdict.
  let watchdogSignaledLive = false;
  // Whether the watchdog reap CONFIRMED the tree dead (from reapTree's verification poll). Surfaced to the
  // caller so a teardown can record reap certainty.
  let reapConfirmedDead;
  // The in-flight reap promise (reapTree's grace-loop + SIGKILL + verification poll, then the provenance
  // sweep). The close handler awaits it so the tree is fully torn down before we return.
  let reapPromise = null;
  const startMs = nowMs();

  const reapNow = (why) => {
    if (reaped) return;
    reaped = true;
    reason = why;
    // Capture the signaled-live discriminator SYNCHRONOUSLY, before the awaited reap can interleave with the
    // child's `close`.
    watchdogSignaledLive = groupOrPidAlive(child.pid);
    reapPromise = (async () => {
      const outcome = await reapTree(child.pid, killGraceMs);
      reapConfirmedDead = outcome.confirmedDead;
      // reapTree already captured wasLive synchronously; prefer its richer signal when present.
      if (typeof outcome.wasLive === "boolean") watchdogSignaledLive = outcome.wasLive;
      await provenanceSweep(targetDir, killGraceMs);
    })();
  };

  // Watchdog: owns BOTH the whole-gate deadline and the phase stall detector.
  const watchdog = setInterval(() => {
    const cur = nowMs();
    // Whole-gate hard deadline.
    if (deadlineMs > 0 && cur >= deadlineMs) {
      reapNow("TIMEOUT");
      return;
    }
    // Stall.
    if (stallMs > 0) {
      const size = totalBytes;
      let artifact = "";
      if (phase === "build") artifact = artifactSignature(targetDir);
      if (size !== lastSize || artifact !== lastArtifact) {
        lastSize = size;
        lastArtifact = artifact;
        lastGrowthMs = cur;
      } else if (cur - lastGrowthMs >= stallMs) {
        reapNow("STALL");
      }
    }
  }, 1000);

  // `spawnError` distinguishes "the OS could not launch the command at all" (ENOENT / EACCES — a
  // setup/usage condition, exit 127) from "the command RAN and exited non-zero" (a real build/test
  // failure). A bare non-zero close code MUST NOT be conflated with a launch failure: a cargo build that
  // compiled and FAILED can exit with any code (including 127 of its own), and that is a GATE FAILURE
  // (exit 1), not a setup error. The caller keys its 127-vs-1 mapping on this flag, never on the code.
  let spawnError = false;
  // When the child is terminated by a SIGNAL (close `code === null` + a signal name), the synthesized exit
  // code is 128 — but that 128 is NOT a real exit code the program chose, it is a stand-in for "killed by a
  // signal". Capture the signal NAME so a caller can report e.g. "terminated by signal SIGABRT" instead of
  // the misleading "exit 128" (a flaky test binary SIGABRTing during `--list` is a signal-kill, not a
  // nextest exit 128). Empty when the child exited normally.
  let signalName = "";
  const code = await new Promise((resolve) => {
    child.on("error", () => {
      spawnError = true;
      resolve(127);
    });
    child.on("close", (c, signal) => {
      if (c === null && signal) {
        signalName = signal;
        resolve(128);
      } else {
        resolve(c === null ? 1 : c);
      }
    });
  });

  clearInterval(watchdog);
  // If the watchdog fired (reapNow set reapPromise), let its reap — the grace loop + SIGKILL + verification
  // poll + provenance sweep — settle before we read its flags or return. This is the SINGLE authoritative
  // teardown: the tree is fully torn down here and watchdogSignaledLive/reapConfirmedDead are final.
  if (reapPromise) {
    try {
      await reapPromise;
    } catch {
      /* best-effort reap */
    }
  }
  // One-tick race vs a REAL trapped-SIGTERM exit-0. The 1s watchdog can set `reason` (TIMEOUT/STALL) in the
  // same tick the child resolves `close`. Two cases must be told apart:
  //   (a) PURE RACE — the child had ALREADY finished (cleanly, code 0) before the reap signaled, so the
  //       reap found NOTHING live (watchdogSignaledLive=false). The step genuinely completed in time; the
  //       watchdog reason is spurious and must be cleared.
  //   (b) REAL REAP, trapped-exit-0 — the watchdog fired on a genuine deadline/stall and found a LIVE
  //       process group (watchdogSignaledLive=true), but that process TRAPPED SIGTERM and exit(0)'d before
  //       SIGKILL. The close code is 0, yet this was a REAL TIMEOUT/STALL — the verdict STANDS.
  // Keying on `code === 0` alone (the prior logic) masked case (b) as a PASS. We key on whether the reap
  // actually found a live target instead: clear `reason` ONLY for the proven no-op race (not signaled
  // live). If it WAS signaled live, the TIMEOUT/STALL reason survives regardless of the trapped exit code.
  if (reason && code === 0 && !watchdogSignaledLive) {
    reason = "";
  }

  // The step is fully settled (the child closed and any watchdog reap completed), so retire the active-step
  // handle: the teardown must not reap a torn-down tree, nor a later/unrelated child's pid. Only clear OUR
  // own registration in case a concurrent step (there is none today, steps are sequential) replaced it.
  if (ACTIVE_STEP && ACTIVE_STEP.pid === child.pid) ACTIVE_STEP = null;

  const durationMs = nowMs() - startMs;
  return {
    code,
    reason,
    durationMs,
    stdout: stdoutBuf,
    stderr: stderrBuf,
    spawnError,
    reapConfirmedDead,
    signalName,
  };
}

// ----------------------------------------------------------------------------------------------------
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

// Terminal status tokens nextest uses for a FAILED test (anything that is not PASS and counts toward the
// summary `failed` total). Informational prefixes (SLOW / TRY / RETRY / START / SETUP) are NOT terminal
// failure statuses and are excluded.
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
  "LEAK",
  "LEAK-FAIL",
  "TIMEOUT",
]);

// All failed-test names from a nextest log, across EVERY terminal failure status (not just `FAIL`).
// Returns the EXACT test name (final whitespace token after the timing bracket) for each failure line.
export function extractNextestFailureStatusNames(text) {
  const names = [];
  for (const line of text.split("\n")) {
    const m = /^\s*([A-Z][A-Z-]*) \[/.exec(line);
    if (!m) continue;
    if (!NEXTEST_FAILURE_STATUSES.has(m[1])) continue;
    const idx = line.indexOf("] ");
    if (idx < 0) continue;
    const after = line.slice(idx + 2).trim();
    if (!after) continue;
    const parts = after.split(/\s+/);
    names.push(parts[parts.length - 1]);
  }
  return names;
}

// The EXACT failed-test names from the plain `FAIL [` lines only — the names the tolerated-allowlist
// accounting operates over on the live path. A crash status (SIGABRT/LEAK/…) is intentionally NOT in this
// set: a crash is never tolerated, and it is surfaced via the summary-count tripwire.
export function extractNextestFailedNames(text) {
  const names = [];
  for (const line of text.split("\n")) {
    if (!/^\s*FAIL \[/.test(line)) continue;
    // Drop everything up to and including the "] " that closes the timing bracket, then take the LAST
    // whitespace token.
    const idx = line.indexOf("] ");
    if (idx < 0) continue;
    const after = line.slice(idx + 2).trim();
    if (!after) continue;
    const parts = after.split(/\s+/);
    names.push(parts[parts.length - 1]);
  }
  return names;
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
  const failNames = extractNextestFailedNames(text);
  const allFailureNames = extractNextestFailureStatusNames(text);
  // A non-`FAIL` failure status (SIGABRT/SIGSEGV/LEAK/TIMEOUT/…) is present whenever the broader scan
  // finds more failure lines than the `FAIL`-only scan — those extras are crashes, never tolerated.
  const nonFailFailures = allFailureNames.length - failNames.length;
  const summary = parseNextestSummary(text);
  const unaccounted = summary.failed - failNames.length;
  if (nonFailFailures > 0 || unaccounted > 0) return "regression";
  if (failNames.length === 0) return "none";
  for (const nm of failNames) {
    // An allowlisted name is tolerated ONLY when the preflight allowed it; otherwise it is a regression.
    if (!(freshnessToleranceAllowed && TOLERATED_TEST_NAMES.has(nm))) return "regression";
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
  const failNames = [...new Set(extractNextestFailedNames(text))];
  const summary = parseNextestSummary(text);
  for (const nm of failNames) {
    if (freshnessToleranceAllowed && TOLERATED_TEST_NAMES.has(nm)) toleratedCount++;
    else failures.push({ surface: "nextest", name: nm });
  }
  const unaccounted = summary.failed - failNames.length;
  // A non-zero run exit is authoritative: it must be EXACTLY accounted for by the parsed `FAIL` names,
  // PROVEN by the summary. We accept a non-zero exit as accounted-for ONLY when ALL of:
  //   - a `Summary [` line was actually parsed (summary.found) — a missing/unparseable summary (a setup or
  //     harness error, or a killed/interrupted run) cannot prove what failed, so it must FAIL; AND
  //   - the summary `failed` count EQUALS the parsed `FAIL` name count (no crash/leak/timeout class hides in
  //     the summary total beyond the accounted `FAIL [` lines), AND that count is non-zero (a non-zero exit
  //     with zero parsed failures is unexplained — fail it).
  // Otherwise the run is unaccounted and we trip a hard failure. Keying the prior tripwire on
  // `unaccounted > 0` swallowed the no-summary case: with tolerated `FAIL` lines and no summary,
  // summary.failed defaulted to 0 so `unaccounted` went NEGATIVE and never fired. Requiring an exact,
  // summary-proven accounting closes that swallow.
  const accounted = summary.found && summary.failed === failNames.length && failNames.length > 0;
  if (code !== 0 && !accounted) {
    failures.push({
      surface: "nextest",
      name: `<run exit ${code}; unaccounted failure(s) (summary ${summary.found ? `failed=${summary.failed}` : "MISSING"}, parsed FAIL names=${failNames.length})>`,
    });
  }
  return { failures, toleratedCount, summary, namedCount: failNames.length, unaccounted };
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

// SURFACE-2 (direct libtest binary) verdict — shared by the live gate and the in-process classifier so the
// testable path is byte-identical to the live one. Given a binary's combined stdout+stderr `text`, its
// process exit `code`, and the suite `binaryId` (for name qualification), returns:
//   { verdict: "pass" | "tolerated" | "fail", failures: [{ surface, name }], toleratedNames: [name…] }
//
// A tolerated SURFACE-2 failure is admitted ONLY under NORMAL libtest failure semantics. Concretely, ALL of:
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
  const qualify = (nm) => `${String(binaryId || "").replace(/^verter_session::?/, "")}::${nm}`;
  const isTolerated = (nm) =>
    freshnessToleranceAllowed &&
    (TOLERATED_TEST_NAMES.has(nm) || TOLERATED_TEST_NAMES.has(qualify(nm)));

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
export function buildCargoEnv(baseEnv, runnerTarget, windows = IS_WINDOWS) {
  const env = { ...baseEnv };
  delete env.CARGO_TARGET_DIR;
  delete env.CARGO_BUILD_TARGET_DIR;
  delete env.CARGO_BUILD_BUILD_DIR;
  env.CARGO_TARGET_DIR = runnerTarget;
  // Force non-TTY / CI-style output so progress lands in the captured log, not a TTY spinner.
  env.CARGO_TERM_COLOR = "never";
  env.CARGO_TERM_PROGRESS_WHEN = "never";
  env.NEXTEST_HIDE_PROGRESS_BAR = "1";
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
// Per-suite package identity, derived ENTIRELY from the nextest archive list JSON we already parsed inside
// the contained/watchdogged list step — `package-name` and `package-id` (the part after `#` is the
// semver). This deliberately avoids a SEPARATE `cargo metadata` subprocess: a second synchronous cargo
// call would run OUTSIDE the whole-gate watchdog, the process containment, and the scrubbed/runner-owned
// cargo env, so a hang in it would bypass the gate deadline. The list JSON already carries everything the
// direct-libtest env needs.
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
// SURFACE-2 suite selection + integrity gate (shared by the live gate and the in-process classifier). The
// filter mirrors `cargo test -p verter_session --tests`: the lib unit-test binary + every `tests/*.rs`
// integration binary, i.e. kind ∈ {lib, test}; bins/benches are excluded. SURFACE 2 IS the shared-process
// surface — the whole reason this gate exists — so a filter/archive regression that finds NOTHING must NOT
// let the gate quietly pass on surface 1 alone. Returns `{ suites, lib, test, error }`: `error` is a
// non-null setup-failure message when zero suites are found OR a kind is missing (verter_session always
// has exactly one `lib` plus its integration `test` targets, so we assert >=1 of EACH — a partial filter
// that keeps only one kind is surfaced as a regression, not passed as a half-covered surface).
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
      "zero verter_session lib/test suites found in the archive listing — the shared-process surface " +
      "would be silently skipped. Refusing to pass on surface 1 alone.";
  } else if (lib < 1 || test < 1) {
    error =
      `verter_session suite filter is incomplete (lib=${lib}, test=${test}; expected >=1 of each). ` +
      "A partial filter would under-cover the shared-process surface. Refusing to pass.";
  }
  return { suites, lib, test, error };
}

// Per-package Cargo env for a DIRECTLY-executed test binary. This injects the runtime Cargo env the
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

// Map a contained-step result to an exit code. A watchdog reason wins (TIMEOUT/STALL); otherwise the
// child's own exit (0 => PASS, non-zero => FAIL).
export function mapStepReason(res) {
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
export function parseNextestSummary(text) {
  let passed = 0;
  let skipped = 0;
  let failed = 0;
  // nextest emits: "Summary [  63.890s] 15543 tests run: 15541 passed, 547 skipped" and may include
  // "N failed" when there are failures.
  const lines = text.split("\n").filter((l) => /Summary \[/.test(l));
  const found = lines.length > 0;
  const line = found ? lines[lines.length - 1] : "";
  let m = /(\d+)\s+passed/.exec(line);
  if (m) passed = parseInt(m[1], 10);
  m = /(\d+)\s+skipped/.exec(line);
  if (m) skipped = parseInt(m[1], 10);
  m = /(\d+)\s+failed/.exec(line);
  if (m) failed = parseInt(m[1], 10);
  return { passed, skipped, failed, found };
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
  const { steps, cargoEnv, repoRealpath, runnerTarget, deadlineMs, stallMs } = ctx;
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
    const res = await runContainedStep({
      cmd: inv.cmd,
      args: inv.args,
      cwd: repoRealpath,
      env: cargoEnv,
      phase: "test",
      deadlineMs,
      stallMs,
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

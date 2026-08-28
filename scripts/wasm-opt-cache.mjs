#!/usr/bin/env node
// Content-addressed cache wrapper around `wasm-opt`.
//
// Key on the actual optimization inputs — the post-wasm-bindgen `.wasm` bytes
// (NOT the raw Cargo/wasm32 artifact: binding generation changes the binary,
// and its output is what wasm-opt actually receives), the `wasm-opt` version,
// the actual optimizer binary's own content, its exact arguments, and a cache
// schema version. Never key on timestamps or commit ids.
//
// Usage:
//   node scripts/wasm-opt-cache.mjs <input.wasm> <output.wasm> -- <wasm-opt args...>
//
// `input.wasm` and `output.wasm` may be the same path (in-place optimization,
// matching the prior `wasm-opt -Os in.wasm -o in.wasm` invocation). On a cache
// hit, the cached optimized bytes are copied to a temp file next to
// `output.wasm` and atomically renamed into place, and `wasm-opt` never runs.
// On a miss, `wasm-opt` writes to a temp file which is then atomically
// renamed into both the cache and the requested output path, so an
// interrupted run can never leave a torn cache entry or a torn output.
//
// The cache key includes wasm-opt's own reported version, so a second
// identical run must not even SPAWN `wasm-opt --version` to learn it (that
// is still spawning wasm-opt) — see the maintainer directive's literal
// acceptance criterion: "a second identical `pnpm dist` must not spawn
// wasm-opt". The version string is memoized keyed on the resolved binary's
// own CONTENT hash (not path + mtime + size — a binary can be replaced
// in place preserving both, which would silently reuse a stale version and
// therefore a stale cache digest); the probe only re-runs when that hash
// changes (a different/updated wasm-opt), never on a repeat run against the
// same binary. Reading and hashing the resolved file is disk I/O, not a
// subprocess spawn, so it does not violate the no-spawn-on-repeat rule.

import { createHash } from "node:crypto";
import {
  closeSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  openSync,
  readFileSync,
  readSync,
  renameSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import path from "node:path";
import process from "node:process";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

// Bump this when the cache entry FORMAT changes (not when wasm-opt's own
// version/args/binary identity change — those are already part of the
// hashed input).
const CACHE_SCHEMA_VERSION = 1;

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.dirname(scriptDir);

// Resolve `wasm-opt` to a concrete file path by walking PATH ourselves,
// without spawning anything — mirrors OS command-lookup closely enough to
// key a version memo on, without paying for a subprocess just to find one.
export function resolveWasmOptPath() {
  const pathEnv = process.env.PATH || process.env.Path || "";
  const dirs = pathEnv.split(path.delimiter).filter(Boolean);
  const windows = process.platform === "win32";
  const names = windows ? ["wasm-opt.exe", "wasm-opt.cmd", "wasm-opt.bat"] : ["wasm-opt"];
  for (const dir of dirs) {
    for (const name of names) {
      const candidate = path.join(dir, name);
      try {
        if (statSync(candidate).isFile()) return candidate;
      } catch {
        // not present in this dir — keep searching
      }
    }
  }
  return null;
}

// On Windows, a resolved `.cmd`/`.bat` name is a shim: Node's
// `spawn(..., { shell: false })` (the default) cannot launch it directly
// (documented Node behaviour — see scripts/build-host.mjs's identical
// rationale for the pnpm shim). POSIX dispatches the resolved path directly
// and is unaffected.
export function isWindowsShim(resolvedPath) {
  if (process.platform !== "win32" || !resolvedPath) return false;
  const lower = resolvedPath.toLowerCase();
  return lower.endsWith(".cmd") || lower.endsWith(".bat");
}

// --- Windows cmd.exe-safe invocation construction (Finding 1) --------------
//
// Ported from `cross-spawn`'s `lib/util/escape.js` + the
// `isCmdShimRegExp`/`needsDoubleEscapeMetaChars` logic in
// `lib/parse.js` (MIT licensed, https://github.com/moxystudio/node-cross-spawn).
// Reimplemented inline here rather than adding a dependency.
//
// Node's `child_process.spawnSync(command, args, { shell: true })` on win32
// joins `[command, ...args]` with spaces and hands the result to
// `cmd.exe /d /s /c "<joined>"` — critically, per Node's own docs, the
// individual `args` array elements are NOT escaped in this join; only
// `command` is ever wrapped/escaped by anything we control. Passing
// CLI-controlled wasm-opt arguments or absolute checkout paths straight
// through `args` under `shell: true` therefore lets any shell metacharacter
// (`&`, `|`, `^`, `%`, `<`, `>`, `"`) in them be interpreted by cmd.exe
// instead of passed through literally — a real injection primitive.
//
// The fix: build ONE fully pre-escaped command-line string ourselves (command
// + every argument, each escaped for both the target program's own argv-split
// quoting convention AND cmd.exe's own metacharacter layer) and hand that
// single string to `spawnSync` as `command` with `args: []`. Node's win32
// shell handling then does exactly `[ourString].concat([]).join(' ')` (a
// no-op) before its own outer `"..."` wrap — so our escaping is the only
// escaping that happens, and it happens exactly once, correctly, for both
// parser layers.
const CMD_META_CHARS_RE = /([()\][%!^"`<>&|;, *?])/g;
// Whether a `.cmd`/`.bat` needs its meta chars escaped TWICE is NOT
// guessable from where it lives (a prior version of this file guessed via
// `node_modules[\\/]\.bin[\\/][^\\/]+\.cmd$`, matching `cross-spawn`'s own
// `isCmdShimRegExp` — but global npm/pnpm `.cmd` shims and bespoke `.bat`
// wrappers outside `node_modules/.bin` commonly relay through the same `%*`
// shape, and a location-based guess can't tell). A shim needs the second
// escape layer if and only if its OWN BODY relays received arguments to a
// nested invocation via a raw `%*` expansion: cmd.exe's own `^`-escaping of
// the outer command line is interpreted when cmd.exe first parses it to
// launch the `.cmd`/`.bat`, and the shim's `%*` re-expansion inside its body
// then needs a SECOND layer of escaping to survive that reparse. A
// `.cmd`/`.bat` that does NOT relay via `%*` (e.g. one consuming `%1`/`%2`
// individually with its own quoting) does not go through that second
// reparse, so double-escaping it would corrupt the argument rather than
// merely over-protect it — see `shimRelaysArgsViaPercentStar` below, which
// reads the shim's actual body to decide.

export function escapeCmdMetaChars(text) {
  return text.replace(CMD_META_CHARS_RE, "^$1");
}

export function escapeCmdArgument(arg, doubleEscapeMetaChars) {
  let escaped = `${arg}`;

  // Algorithm below is based on https://qntm.org/cmd (backtracking disabled
  // to avoid hanging on adversarial input — see the cross-spawn PR this was
  // ported from: https://github.com/moxystudio/node-cross-spawn/pull/160).
  //
  // Sequence of backslashes followed by a double quote: double up all the
  // backslashes and escape the double quote.
  escaped = escaped.replace(/(?=(\\+?)?)\1"/g, '$1$1\\"');
  // Sequence of backslashes followed by the end of the string (which will
  // become a double quote once wrapped below): double up all the backslashes.
  escaped = escaped.replace(/(?=(\\+?)?)\1$/, "$1$1");

  // Quote the whole thing.
  escaped = `"${escaped}"`;

  // Escape cmd.exe meta chars, possibly twice (see `shimRelaysArgsViaPercentStar` below).
  escaped = escapeCmdMetaChars(escaped);
  if (doubleEscapeMetaChars) escaped = escapeCmdMetaChars(escaped);

  return escaped;
}

/**
 * Builds a single, fully pre-escaped cmd.exe command-line STRING for a
 * resolved Windows `.cmd`/`.bat` shim plus its argv, safe to hand to
 * `spawnSync(command, [], { shell: true })` — see the block comment above
 * `CMD_META_CHARS_RE` for why `args` must stay empty. `doubleEscape` must be
 * determined structurally by the caller (see `shimRelaysArgsViaPercentStar`
 * / `determineDoubleEscape`) — this function does not guess it from
 * `resolvedPath`'s location.
 */
export function buildWindowsShimInvocation(resolvedPath, argv, doubleEscape) {
  // `resolvedPath` is a Windows path (backslash-separated) by construction —
  // normalize it with the win32 path implementation explicitly rather than
  // the default (host-dependent) `path`, so this is correct even when
  // exercised directly (e.g. by tests) on a non-Windows host.
  const command = escapeCmdMetaChars(path.win32.normalize(resolvedPath));
  const parts = [command, ...argv.map((arg) => escapeCmdArgument(arg, doubleEscape))];
  return { command: parts.join(" "), args: [], shell: true };
}

/**
 * Every actual wasm-opt invocation (version probe or optimize) must run the
 * SAME resolved binary the cache key was computed against — resolving once
 * and spawning a bare "wasm-opt" (re-triggering a fresh, possibly different,
 * PATH search) would silently run a different executable than the one that
 * was hashed/identified. On a Windows shim this returns the safely-escaped
 * single-string invocation (see `buildWindowsShimInvocation`) using the
 * caller-supplied, structurally-determined `doubleEscape`; everywhere else
 * the resolved path is dispatched directly, no shell involved (`doubleEscape`
 * is ignored).
 */
export function resolveInvocation(resolvedPath, argv, doubleEscape = false) {
  if (isWindowsShim(resolvedPath))
    return buildWindowsShimInvocation(resolvedPath, argv, doubleEscape);
  return { command: resolvedPath ?? "wasm-opt", args: argv, shell: false };
}

// --- Optimizer binary identity resolution (Finding 2) ----------------------
//
// A resolved PATH entry for `wasm-opt` is very often a pnpm/npm-generated
// SHIM, not the actual optimizer binary — on Windows a `.cmd`/`.bat`, on
// POSIX a `#!/bin/sh` wrapper (verified in this checkout:
// `node_modules/.bin/wasm-opt` under any package that depends on `binaryen`
// is a ~1.3KB POSIX shell script, not the ~10MB real binary). Hashing the
// shim's own bytes unchanged would let two DIFFERENT underlying `wasm-opt`
// builds (a patched/rebuilt binary, a vendored fork) that happen to produce
// an identically-shaped shim collide onto the same cache key — and since
// pnpm generates exactly this shim shape on EVERY platform, this is the
// common case on POSIX too, not a Windows-only edge case.
//
// pnpm/npm-generated `.cmd` shims have a well-known shape (see e.g. any
// `node_modules/.bin/*.cmd`):
//   @IF EXIST "%~dp0\node.exe" (
//     "%~dp0\node.exe"  "%~dp0\..\<pkg>\bin\<entry>" %*
//   ) ELSE ( ... node  "%~dp0\..\<pkg>\bin\<entry>" %* )
// — a `node`/`node.exe`/`%_prog%` launcher followed by a quoted JS entry
// path. pnpm-generated POSIX shims have an equally well-known shape (see
// `node_modules/.bin/wasm-opt` in this checkout):
//   #!/bin/sh
//   basedir=$(dirname "$(echo "$0" | sed -e 's,\\,/,g')")
//   ...
//   if [ -x "$basedir/node" ]; then
//     exec "$basedir/node"  "$basedir/../<pkg>/bin/<entry>" "$@"
//   else
//     exec node  "$basedir/../<pkg>/bin/<entry>" "$@"
//   fi
// — a `node`/`$basedir/node` launcher followed by the quoted entry path. For
// `binaryen` (the npm package providing `wasm-opt` in this repo) that entry
// (`binaryen/bin/wasm-opt`) is ITSELF the actual optimizer implementation (a
// script emitted by Emscripten, not a further native-binary trampoline) — so
// hashing that resolved file's content is hashing the real, running
// optimizer identity, on either platform.

const SHIM_TARGET_RE = /"(?:[^"]*\\node(?:\.exe)?|%[^%]+%)"\s+"([^"]+)"/i;
const DP0_TOKEN_RE = /%~dp0%?|%dp0%/gi;

/**
 * Pure text-parsing half of Windows shim resolution: given a shim file's
 * contents and the directory it lives in, returns the absolute path to the
 * real program it execs. Split out from file I/O so it is unit-testable
 * against fixture shim text without needing a real file on disk.
 */
export function extractShimTargetPath(shimText, shimDir) {
  const match = shimText.match(SHIM_TARGET_RE);
  if (!match) {
    throw new Error(
      "wasm-opt-cache: could not locate the real optimizer target inside the Windows shim " +
        '— its shape changed from the expected pnpm/npm `node "<entry>" %*` form; update the resolver.',
    );
  }
  // Use the replacer-FUNCTION form of `.replace()` — `shimDir` is inserted
  // literally regardless of its content. The string-replacement form treats
  // `$&`/`$$`/`` $` ``/`$'`/`$1`-`$9` specially inside the replacement
  // argument, so an absolute path that happens to contain one of those
  // sequences (unlikely but real — a checkout dir literally named with
  // `$&` in it) would silently corrupt the resolved target instead of
  // being inserted verbatim.
  const rawTarget = match[1].replace(DP0_TOKEN_RE, () => shimDir);
  // Windows paths throughout (backslash-separated) — resolve with the win32
  // path implementation explicitly, not the default (host-dependent) `path`,
  // so this is correct on a non-Windows host too (e.g. under test).
  return path.win32.resolve(rawTarget);
}

const SHIM_ARG_RELAY_RE = /(?<!%)%\*/;
// A `REM ...` or `:: ...` line is a cmd.exe comment — never executed, so a
// `%*` mention inside one (e.g. `REM do not relay via %*`) must not count as
// a live argument relay. `@` is cmd.exe's general echo-suppression prefix —
// applicable to ANY command, not glued to `REM` — so it may be followed by
// optional whitespace before `REM`/`::` (`@ REM ...` is legal cmd.exe syntax,
// not just `@REM ...`). `rem` is a comment keyword only when it is the actual
// command TOKEN — followed by whitespace, `.`, `:`, `/`, or end-of-line — not
// a generic `\b` word boundary, which also fires between "rem" and a
// following hyphen: a LIVE command merely named `rem-wrapper` must not be
// misread as a comment (that would suppress a required double-escape
// decision for a genuinely relaying live command). `/` is included per
// Microsoft's own documented `REM` syntax, which allows a `/` separator
// (e.g. `Rem/||(` is a documented valid REM construct) — without it a
// `REM/ note %*` comment line would be misclassified as a live relay.
// Matched per-line (`m` flag) so only the comment's OWN line is stripped,
// not the rest of the shim body.
const CMD_COMMENT_LINE_RE = /^[ \t]*@?[ \t]*(?:rem(?=[ \t./:]|$)|::).*$/gim;

/**
 * Structurally determines whether a Windows `.cmd`/`.bat` shim relays its
 * received arguments to a nested invocation via a raw `%*` expansion — the
 * shape that needs the second cmd.exe meta-char escape layer (see the block
 * comment above `CMD_META_CHARS_RE`). Reads the shim's own body instead of
 * guessing from its PATH location — a `.cmd`/`.bat` outside
 * `node_modules/.bin` can relay via `%*` too, and one that consumes
 * `%1`/`%2` individually with its own quoting does not need the extra
 * escape layer at all.
 *
 * `REM`/`::` comment lines are stripped before testing — a shim that merely
 * MENTIONS `%*` in a comment (e.g. `REM do not relay via %*`) while actually
 * invoking via `%1`/`%~1` must not be misclassified as relay-via-`%*`: that
 * would apply a second escape layer the shim's real (single) reparse never
 * consumes, CORRUPTING the argument rather than merely over-protecting it.
 */
export function shimRelaysArgsViaPercentStar(shimText) {
  const withoutComments = shimText.replace(CMD_COMMENT_LINE_RE, "");
  return SHIM_ARG_RELAY_RE.test(withoutComments);
}

const POSIX_SHIM_SHEBANG_RE = /^#!\s*\/(?:bin\/sh\b|usr\/bin\/env\s+sh\b)/;
const POSIX_SHIM_TARGET_RE = /exec\s+(?:"[^"]*"|\S+)\s+"([^"]+)"\s+"\$@"/;
const POSIX_BASEDIR_TOKEN_RE = /\$\{?basedir\}?/g;

/**
 * Pure text-parsing half of POSIX shim resolution — mirrors
 * `extractShimTargetPath` for the `#!/bin/sh` shim shape pnpm/npm emit on
 * POSIX (see the block comment above). Split out from file I/O so it is
 * unit-testable against fixture shim text without needing a real file on
 * disk.
 */
export function extractPosixShimTargetPath(shimText, shimDir) {
  const match = shimText.match(POSIX_SHIM_TARGET_RE);
  if (!match) {
    throw new Error(
      "wasm-opt-cache: could not locate the real optimizer target inside the POSIX shim " +
        '— its shape changed from the expected pnpm/npm `exec ... "<entry>" "$@"` form; update the resolver.',
    );
  }
  // Replacer-FUNCTION form (see the identical comment in
  // `extractShimTargetPath` above) — `shimDir` is inserted literally even if
  // it contains a `$&`/`$1`-shaped substring (a real, if unlikely,
  // possibility for an absolute filesystem path).
  const rawTarget = match[1].replace(POSIX_BASEDIR_TOKEN_RE, () => shimDir);
  return path.resolve(rawTarget);
}

/**
 * Whether `resolvedPath` is a POSIX text shim rather than the real optimizer
 * binary — determined structurally (a `#!/bin/sh`-style shebang in the
 * file's own first bytes), not by name/extension: unlike Windows, a POSIX
 * pnpm/npm shim (`node_modules/.bin/wasm-opt`) carries no distinguishing
 * extension at all. Reads only a small header prefix, not the whole file —
 * the real optimizer binary this must NOT mistake for a shim is ~10MB.
 */
export function isPosixShellShim(resolvedPath) {
  if (process.platform === "win32" || !resolvedPath) return false;
  let head;
  try {
    const fd = openSync(resolvedPath, "r");
    try {
      const buf = Buffer.alloc(64);
      const bytesRead = readSync(fd, buf, 0, buf.length, 0);
      head = buf.subarray(0, bytesRead).toString("utf8");
    } finally {
      closeSync(fd);
    }
  } catch {
    return false;
  }
  return POSIX_SHIM_SHEBANG_RE.test(head);
}

function readShimText(resolvedPath, whatFor) {
  try {
    return readFileSync(resolvedPath, "utf8");
  } catch (err) {
    throw new Error(
      `wasm-opt-cache: could not read shim '${resolvedPath}' ${whatFor}: ${err.message}`,
    );
  }
}

/**
 * Resolves a Windows shim path to the real optimizer file it invokes, plus
 * whether that shim's own body relays arguments via `%*` (Finding 1). Reads
 * the shim body once for both. Fails loudly (throws) rather than silently
 * falling back to hashing the shim wrapper — a resolution failure here must
 * not quietly reproduce the bug this function exists to close.
 */
export function inspectWindowsShim(resolvedPath) {
  const shimText = readShimText(resolvedPath, "to resolve the real optimizer binary it invokes");
  const targetPath = extractShimTargetPath(shimText, path.win32.dirname(resolvedPath));
  if (!existsSync(targetPath)) {
    throw new Error(
      `wasm-opt-cache: resolved optimizer target '${targetPath}' from shim '${resolvedPath}' does not exist.`,
    );
  }
  return { targetPath, needsDoubleEscape: shimRelaysArgsViaPercentStar(shimText) };
}

/**
 * Resolves a POSIX shim path to the real optimizer file it invokes. Fails
 * loudly (throws) rather than silently falling back to hashing the shim
 * wrapper, mirroring `inspectWindowsShim`.
 */
export function resolvePosixShimTarget(resolvedPath) {
  const shimText = readShimText(resolvedPath, "to resolve the real optimizer binary it invokes");
  const targetPath = extractPosixShimTargetPath(shimText, path.dirname(resolvedPath));
  if (!existsSync(targetPath)) {
    throw new Error(
      `wasm-opt-cache: resolved optimizer target '${targetPath}' from POSIX shim '${resolvedPath}' does not exist.`,
    );
  }
  return targetPath;
}

/**
 * Resolves ANY resolved `wasm-opt` PATH entry — Windows shim, POSIX shim, or
 * a plain executable — to the real optimizer file whose content should be
 * hashed for cache-key identity. A plain executable (no shim at all, e.g. a
 * real native `wasm-opt` binary) resolves to itself unchanged.
 */
export function resolveActualOptimizerFile(resolvedPath) {
  if (isWindowsShim(resolvedPath)) return inspectWindowsShim(resolvedPath).targetPath;
  if (isPosixShellShim(resolvedPath)) return resolvePosixShimTarget(resolvedPath);
  return resolvedPath;
}

/**
 * Whether the resolved `wasm-opt` invocation needs the second cmd.exe
 * meta-char escape layer (Finding 1) — always `false` off Windows or for a
 * plain (non-shim) executable.
 */
export function determineDoubleEscape(resolvedPath) {
  if (!isWindowsShim(resolvedPath)) return false;
  return inspectWindowsShim(resolvedPath).needsDoubleEscape;
}

/**
 * Combined optimizer-identity resolution: the real optimizer file to hash
 * PLUS whether that shim relays arguments via `%*` — computed from a SINGLE
 * shim-body read. `resolveActualOptimizerFile` and `determineDoubleEscape`
 * each independently call `inspectWindowsShim` (which rereads the shim file
 * from disk), so a caller that needs both answers by calling them
 * separately reads the Windows shim TWICE; if the shim's content changes
 * between those two reads (e.g. a package manager rewriting it mid-build),
 * the hashed target (from the first read) and the actual invocation target/
 * escape decision (from the second) can diverge — the cache digest would
 * bind to one binary while a different one actually runs and gets cached
 * under that digest. Every call site that needs both the target path and
 * the double-escape decision (i.e. `main()`) MUST use this function instead
 * of calling `resolveActualOptimizerFile` + `determineDoubleEscape`
 * separately.
 */
export function resolveOptimizerIdentity(resolvedPath) {
  if (isWindowsShim(resolvedPath)) {
    const { targetPath, needsDoubleEscape } = inspectWindowsShim(resolvedPath);
    return { targetPath, needsDoubleEscape };
  }
  if (isPosixShellShim(resolvedPath)) {
    return { targetPath: resolvePosixShimTarget(resolvedPath), needsDoubleEscape: false };
  }
  return { targetPath: resolvedPath, needsDoubleEscape: false };
}

export function hashFile(filePath) {
  return createHash("sha256").update(readFileSync(filePath)).digest("hex");
}

// Read/write a tiny memo mapping a resolved wasm-opt PATH entry to the
// content hash of the actual optimizer file it resolves to (post
// shim-resolution) and its already-probed `--version` output, so an
// unchanged binary never needs a repeat `--version` spawn.
function readVersionMemo(memoPath) {
  try {
    return JSON.parse(readFileSync(memoPath, "utf8"));
  } catch {
    return {};
  }
}

// Both callers of `resolveWasmOptVersion` (main()'s only call site) always
// hold a resolved path AND a resolved content hash by the time this runs —
// `main()` bails out to an uncached run before ever reaching here when
// either is unavailable (see the "could not resolve wasm-opt's own binary
// identity" branch below), so there is no defensive no-hash fallback here:
// a cache-key ingredient computed without a resolved identity must never be
// memoized as though it were one.
function resolveWasmOptVersion(cacheDir, resolvedPath, optimizerContentHash, doubleEscape) {
  const memoPath = path.join(cacheDir, "version-memo.json");
  const memo = readVersionMemo(memoPath);
  const entry = memo[resolvedPath];
  if (entry && entry.contentHash === optimizerContentHash) {
    return entry.version;
  }
  const version = probeWasmOptVersion(resolvedPath, doubleEscape);
  memo[resolvedPath] = { contentHash: optimizerContentHash, version };
  writeFileSync(memoPath, JSON.stringify(memo));
  return version;
}

function probeWasmOptVersion(resolvedPath, doubleEscape) {
  const { command, args, shell } = resolveInvocation(resolvedPath, ["--version"], doubleEscape);
  const versionProbe = spawnSync(command, args, { encoding: "utf8", shell });
  if (versionProbe.status !== 0 || versionProbe.error) {
    process.stderr.write(
      `wasm-opt-cache: failed to run '${command} --version': ${versionProbe.error ?? versionProbe.stderr}\n`,
    );
    process.exit(versionProbe.status ?? 1);
  }
  return versionProbe.stdout.trim();
}

/**
 * The cache digest computation, extracted so it is unit-testable without a
 * real wasm-opt binary, a real subprocess spawn, or a real optimizer file —
 * callers pass in already-computed bytes/hashes/strings. `optimizerContentHash`
 * is REQUIRED — a cache entry must never be keyed on a placeholder identity
 * (an unresolvable optimizer binary is handled by `main()` running wasm-opt
 * directly, uncached, before this is ever called; see the "could not resolve
 * wasm-opt's own binary identity" branch there).
 */
export function computeCacheDigest({
  inputBytes,
  wasmOptVersion,
  optimizerContentHash,
  wasmOptArgs,
  schemaVersion = CACHE_SCHEMA_VERSION,
}) {
  if (!optimizerContentHash) {
    throw new Error(
      "wasm-opt-cache: computeCacheDigest requires a resolved optimizerContentHash — " +
        "a cache entry must never be keyed on an unresolved/placeholder optimizer identity.",
    );
  }
  return createHash("sha256")
    .update(inputBytes)
    .update("\0")
    .update(wasmOptVersion)
    .update("\0")
    .update(optimizerContentHash)
    .update("\0")
    .update(JSON.stringify(wasmOptArgs))
    .update("\0")
    .update(`schema=${schemaVersion}`)
    .digest("hex");
}

/**
 * Spawns the optimizer command so it writes to `tmpOutput`, cleaning up the
 * temp file on failure. Shared by both the ordinary cache-miss path and the
 * "could not resolve wasm-opt's own binary identity" uncached fallback
 * (Finding 2) so BOTH honor the same write-temp-then-atomically-rename
 * invariant this file's header comment states as a hard guarantee — an
 * interrupted run must never leave a torn file at the final destination.
 * Does not itself call `process.exit` (so it stays directly unit-testable
 * without killing the test process) — the caller inspects `.ok` and exits.
 */
export function spawnOptimizerToTempOutput({ command, args, shell = false, tmpOutput }) {
  const result = spawnSync(command, args, { stdio: "inherit", shell });
  if (result.error || result.status !== 0) {
    rmSync(tmpOutput, { force: true });
    return { ok: false, status: result.status ?? 1 };
  }
  return { ok: true };
}

/**
 * Finalizes a successful cache-miss wasm-opt run (Finding 1, round 6; closed
 * more fully at Finding 1, round 7 below).
 *
 * `resolveOptimizerIdentity` reads the shim ONCE to compute the cache-key
 * identity (`optimizerFile` + `optimizerContentHash`), but the actual
 * `wasm-opt` invocation always re-resolves the invocation through
 * `resolvedWasmOptPath` — the ORIGINAL, still-mutable PATH entry, not the
 * already-resolved `optimizerFile` — so within one build invocation there is
 * a narrow window (milliseconds) between that identity read and the
 * optimizer run actually completing in which the shim could change.
 *
 * A round-6 fix re-hashed `optimizerFile` and compared it against the
 * identity-resolution-time `optimizerContentHash` — but that only detects
 * the resolved TARGET's own content changing. It cannot see the shim being
 * RETARGETED to point at a completely different file while the original
 * target stays byte-identical: `optimizerFile` is fixed at the value
 * resolved before the run, so re-hashing that same path can never notice
 * the shim now resolves somewhere else. This function instead re-runs FULL
 * identity resolution against `resolvedWasmOptPath` (the original PATH
 * entry, not the already-resolved target) and compares BOTH the resolved
 * target path and its content hash against the identity-resolution-time
 * values — covering content mutation AND retargeting with one check.
 *
 *  - MATCH (the overwhelmingly common — in practice universal — case):
 *    populate the cache (temp file + atomic rename) then satisfy
 *    `outputPath` from the same optimized bytes, exactly as before.
 *  - MISMATCH (different target path, or the same path with different
 *    content), OR the re-resolution itself throws/fails (e.g. the earlier
 *    target vanished from disk): the produced output is still a genuine,
 *    correct result of a real run, so `outputPath` is still satisfied from
 *    it — but the CACHE ENTRY is skipped (no `cachedEntryPath` write),
 *    since it would be keyed on an identity that could no longer be
 *    confirmed. A failure to CONFIRM the identity must never be read as a
 *    reason to fail the whole build — only as a reason not to cache. Logged
 *    to stderr since this should never fire in practice (nothing races to
 *    rewrite an installed `node_modules/.bin` shim mid-build) — a real
 *    occurrence is worth being visible, not silent.
 *
 * Does not attempt to close the resolve-to-spawn race any more tightly than
 * this (no file locking, no re-parsing the shim before every spawn, no
 * bypassing the shim to invoke the resolved target directly — that would
 * change execution semantics, e.g. losing `NODE_PATH` the POSIX pnpm shim
 * sets before invoking node) — this checkout has no realistic adversarial
 * threat model against its own just-installed `node_modules/.bin` shims, so
 * a cache-write-time consistency check is the agreed scope.
 */
export function finalizeCacheMissOutput({
  tmpOutput,
  tmpCacheEntry,
  cachedEntryPath,
  outputPath,
  optimizerFile,
  optimizerContentHash,
  resolvedWasmOptPath,
}) {
  let identityConfirmed = false;
  let confirmationFailure = null;
  try {
    const { targetPath: postRunTargetPath } = resolveOptimizerIdentity(resolvedWasmOptPath);
    const postRunContentHash = hashFile(postRunTargetPath);
    identityConfirmed =
      postRunTargetPath === optimizerFile && postRunContentHash === optimizerContentHash;
  } catch (err) {
    confirmationFailure = err;
  }

  if (identityConfirmed) {
    try {
      copyFileSync(tmpOutput, tmpCacheEntry);
      renameSync(tmpCacheEntry, cachedEntryPath);
    } catch (err) {
      // The optimizer identity is confirmed and the run genuinely succeeded —
      // a failure populating the cache (disk full, permissions, EIO, ...) is
      // never a reason to discard a correct result. Clean up any partial
      // cache-entry temp file and fall through to deliver `outputPath`
      // unconditionally below, exactly as the identity-recheck failure paths
      // already do.
      if (existsSync(tmpCacheEntry)) {
        // Best-effort cleanup only: `force: true` suppresses "already doesn't
        // exist" but not a genuine I/O error (EIO/EPERM/...) on removal. A
        // leftover `.wasm-opt-cache.<digest>.<pid>.tmp` file is a harmless,
        // recoverable artifact (a future cache-miss run mints its own temp
        // file under the same naming convention; an operator can also clean
        // `.cache/wasm-opt` manually) — it must never propagate out of this
        // handler and skip the final `renameSync(tmpOutput, outputPath)`
        // below, which is exactly the successful-output-gets-stranded bug
        // this whole cache-population-failure path exists to avoid.
        try {
          rmSync(tmpCacheEntry, { force: true });
        } catch {
          // Swallowed — see comment above.
        }
      }
      process.stderr.write(
        "wasm-opt-cache: confirmed the optimizer's identity but failed to persist this " +
          `result to the cache (${err.message}) — skipping cache population for this result ` +
          "(the produced output is still correct and used).\n",
      );
    }
  } else if (confirmationFailure) {
    process.stderr.write(
      "wasm-opt-cache: could not re-confirm the optimizer's identity after this run completed " +
        `(${confirmationFailure.message}) — skipping cache population for this result (the ` +
        "produced output is still correct and used) rather than caching an entry keyed on an " +
        "unconfirmed optimizer identity.\n",
    );
  } else {
    process.stderr.write(
      "wasm-opt-cache: the optimizer binary — or the shim's resolved target — changed between " +
        `identity resolution and this run completing ('${optimizerFile}') — skipping cache ` +
        "population for this result (the produced output is still correct and used) rather " +
        "than caching an entry keyed on stale optimizer identity.\n",
    );
  }
  renameSync(tmpOutput, outputPath);
}

/**
 * The "could not resolve wasm-opt's own binary identity on PATH" uncached
 * fallback: runs `wasm-opt` directly (relying on the OS's own PATH lookup,
 * via a bare `"wasm-opt"` command under `shell: false`) and satisfies
 * `outputPath` from its output — never touching `.cache/wasm-opt` (there is
 * no resolved identity to key a cache entry on). Still honors the
 * write-temp-then-atomically-rename invariant via `spawnOptimizerToTempOutput`
 * — an interrupted run must never leave a torn/partial file at `outputPath`.
 *
 * Extracted from `main()` so it is directly unit-testable: `main()` itself
 * parses `process.argv` and calls `process.exit`, which makes it impractical
 * to drive in-process from a test.
 */
export function runUncachedFallback({ wasmOptArgs, inputPath, outputPath }) {
  const pid = process.pid;
  const tmpOutput = path.join(path.dirname(outputPath), `.wasm-opt-cache.${pid}.tmp`);
  const spawnResult = spawnOptimizerToTempOutput({
    command: "wasm-opt",
    args: [...wasmOptArgs, inputPath, "-o", tmpOutput],
    tmpOutput,
  });
  if (!spawnResult.ok) {
    process.stderr.write(`wasm-opt-cache: wasm-opt failed (status ${spawnResult.status})\n`);
    process.exit(spawnResult.status);
  }
  renameSync(tmpOutput, outputPath);
}

function usageError(message) {
  process.stderr.write(
    `${message}\nusage: node scripts/wasm-opt-cache.mjs <input.wasm> <output.wasm> -- <wasm-opt args...>\n`,
  );
  process.exit(2);
}

function main() {
  const argv = process.argv.slice(2);
  const sepIdx = argv.indexOf("--");
  if (sepIdx === -1) usageError("missing '--' separator before wasm-opt arguments");

  const positional = argv.slice(0, sepIdx);
  const wasmOptArgs = argv.slice(sepIdx + 1);
  const [inputPathArg, outputPathArg] = positional;
  if (!inputPathArg || !outputPathArg)
    usageError("both <input.wasm> and <output.wasm> are required");
  if (wasmOptArgs.length === 0) usageError("no wasm-opt arguments given after '--'");

  const inputPath = path.resolve(inputPathArg);
  const outputPath = path.resolve(outputPathArg);

  if (!existsSync(inputPath)) {
    process.stderr.write(`wasm-opt-cache: input not found: ${inputPath}\n`);
    process.exit(1);
  }

  // Read the input BEFORE anything else touches it — this is the
  // post-wasm-bindgen artifact wasm-opt would actually consume.
  const inputBytes = readFileSync(inputPath);

  const resolvedWasmOptPath = resolveWasmOptPath();

  if (!resolvedWasmOptPath) {
    // PATH search came up empty but the OS's own lookup rules might still
    // find `wasm-opt` via some mechanism we didn't model — that's a
    // legitimate (if unusual) case, but there is then no concrete binary to
    // bind a cache-key identity to. Never cache under a non-identity
    // sentinel (that would let a genuinely different underlying optimizer
    // silently collide onto the same cache slot forever) — run wasm-opt
    // directly, uncached, instead. This branch never touches `.cache/wasm-opt`
    // at all (no cache dir created, no cache entry written) — it just skips
    // the cache-copy step, not the write-temp-then-atomically-rename
    // invariant every other path in this file honors: an interrupted run
    // here must not leave a torn/partial file at `outputPath` either.
    process.stderr.write(
      "wasm-opt-cache: could not resolve wasm-opt's own binary identity on PATH — running " +
        "wasm-opt directly, UNCACHED, rather than caching a result under a non-identity placeholder.\n",
    );
    runUncachedFallback({ wasmOptArgs, inputPath, outputPath });
    return;
  }

  const cacheDir = path.join(repoRoot, ".cache", "wasm-opt");
  mkdirSync(cacheDir, { recursive: true });

  const { targetPath: optimizerFile, needsDoubleEscape: doubleEscape } =
    resolveOptimizerIdentity(resolvedWasmOptPath);
  const optimizerContentHash = hashFile(optimizerFile);
  const wasmOptVersion = resolveWasmOptVersion(
    cacheDir,
    resolvedWasmOptPath,
    optimizerContentHash,
    doubleEscape,
  );

  const digest = computeCacheDigest({
    inputBytes,
    wasmOptVersion,
    optimizerContentHash,
    wasmOptArgs,
  });

  const cachedEntryPath = path.join(cacheDir, `${digest}.wasm`);

  if (existsSync(cachedEntryPath)) {
    const pid = process.pid;
    const tmpHitOutput = path.join(path.dirname(outputPath), `.wasm-opt-cache.${pid}.tmp`);
    copyFileSync(cachedEntryPath, tmpHitOutput);
    renameSync(tmpHitOutput, outputPath);
    process.stdout.write(
      `wasm-opt cache hit  <${digest}> — ${path.relative(repoRoot, outputPath)}\n`,
    );
    return;
  }

  process.stdout.write(`wasm-opt cache miss <${digest}> — running wasm-opt\n`);

  const pid = process.pid;
  const tmpOutput = path.join(path.dirname(outputPath), `.wasm-opt-cache.${pid}.tmp`);
  const tmpCacheEntry = path.join(cacheDir, `.wasm-opt-cache.${digest}.${pid}.tmp`);

  const {
    command: wasmOptCmd,
    args: wasmOptSpawnArgs,
    shell: wasmOptShell,
  } = resolveInvocation(
    resolvedWasmOptPath,
    [...wasmOptArgs, inputPath, "-o", tmpOutput],
    doubleEscape,
  );
  const spawnResult = spawnOptimizerToTempOutput({
    command: wasmOptCmd,
    args: wasmOptSpawnArgs,
    shell: wasmOptShell,
    tmpOutput,
  });
  if (!spawnResult.ok) {
    process.stderr.write(`wasm-opt-cache: wasm-opt failed (status ${spawnResult.status})\n`);
    process.exit(spawnResult.status);
  }

  // Populate the cache first (temp file + atomic rename), then satisfy the
  // requested output from the same optimized bytes — unless the optimizer
  // identity changed (or could not be re-confirmed) mid-run (Finding 1,
  // round 6/7), in which case cache population is skipped but the output is
  // still satisfied. Never leave a partially written entry at the final
  // cache path or output path.
  finalizeCacheMissOutput({
    tmpOutput,
    tmpCacheEntry,
    cachedEntryPath,
    outputPath,
    optimizerFile,
    optimizerContentHash,
    resolvedWasmOptPath,
  });
}

// Run only when invoked directly (`node scripts/wasm-opt-cache.mjs ...`) — an
// import (e.g. vitest self-tests importing the helpers above) must never
// trigger arg parsing or process.exit.
if (
  process.argv[1] !== undefined &&
  path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  main();
}

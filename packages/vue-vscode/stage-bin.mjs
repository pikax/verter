/**
 * Stage the `verter-relay-shim` binary into the VS Code extension VSIX `bin/`.
 *
 * The editor is pointed at the bundled shim as its `tsgo`; the shim spawns the
 * REAL tsgo and relays the `--lsp` stdio. The VSIX therefore ships ONLY the
 * Verter shim — NEVER tsgo itself (tsgo is discovered / supplied separately).
 *
 * This module is PURE (no import-time side effects) so it is unit-testable in
 * isolation from the full `package.mjs` VSIX pipeline. `package.mjs` imports
 * {@link stageShimBinary} and calls it before invoking vsce.
 *
 * Resolution is fail-closed: packaging NEVER auto-builds the shim (an auto-build
 * would compile the HOST binary and poison a cross-target VSIX). The binary must
 * already exist, and for a platform (`--target`) build it must live under the
 * matching Rust target dir — the host `target/{release,debug}` dir is used ONLY
 * for a universal (targetless) build.
 */
import { cpSync, existsSync, mkdirSync, readdirSync, rmSync } from "node:fs";
import path from "node:path";

/** The fixed on-disk stem of the shim binary (never tsgo-shaped). */
export const SHIM_STEM = "verter-relay-shim";

/**
 * Validate a RESOLVED source path's basename before copying it: it must be EXACTLY the
 * expected shim basename for the packaged target, never tsgo-shaped or otherwise mismatched.
 * The no-tsgo defense must gate the SOURCE bytes (an env override or a target-dir lookup can
 * point anywhere), not only the destination name — otherwise `VERTER_RELAY_SHIM_BINARY=…/tsgo`
 * would copy tsgo bytes renamed as the shim. Fail closed.
 *
 * @param {string} source - the resolved candidate path about to be copied.
 * @param {string} expectedBasename - `shimBinaryBasename(vsceTarget, hostPlatform)`.
 */
export function assertShimSourceBasename(source, expectedBasename) {
  const base = path.basename(source);
  if (/tsgo/i.test(base)) {
    throw new Error(
      `verter-relay-shim packaging: refusing to stage tsgo-shaped source ${JSON.stringify(base)} ` +
        `(${JSON.stringify(source)}) — tsgo is NEVER packaged, only ${SHIM_STEM}.`,
    );
  }
  if (base !== expectedBasename) {
    throw new Error(
      `verter-relay-shim packaging: source basename ${JSON.stringify(base)} ` +
        `(${JSON.stringify(source)}) does not match the expected shim ${JSON.stringify(expectedBasename)} ` +
        `for this target — refusing to stage a mismatched binary.`,
    );
  }
}

/** VSCE `--target` → Rust target triple, for every platform VSIX this package builds. */
const VSCE_TO_RUST_TARGET = {
  "win32-x64": "x86_64-pc-windows-msvc",
  "win32-arm64": "aarch64-pc-windows-msvc",
  "linux-x64": "x86_64-unknown-linux-gnu",
  "linux-arm64": "aarch64-unknown-linux-gnu",
  "darwin-x64": "x86_64-apple-darwin",
  "darwin-arm64": "aarch64-apple-darwin",
};

/**
 * Map a VSCE `--target` to its Rust target triple. Returns `null` for a universal
 * (targetless) build. Throws on an unrecognized non-empty target — fail closed, so
 * a typo can never silently stage the host binary into a cross-target VSIX.
 *
 * @param {string | undefined} vsceTarget
 * @returns {string | null}
 */
export function vsceTargetToRustTarget(vsceTarget) {
  if (!vsceTarget) return null;
  const rust = VSCE_TO_RUST_TARGET[vsceTarget];
  if (!rust) {
    throw new Error(
      `verter-relay-shim packaging: unrecognized --target ${JSON.stringify(vsceTarget)}. ` +
        `Add it to VSCE_TO_RUST_TARGET (stage-bin.mjs) before packaging this platform VSIX.`,
    );
  }
  return rust;
}

/**
 * Whether the packaged TARGET is Windows (decides the `.exe` suffix). For a universal
 * build (no `--target`) this is the HOST platform.
 *
 * @param {string | undefined} vsceTarget
 * @param {string} hostPlatform - a `process.platform` value.
 * @returns {boolean}
 */
export function targetIsWindows(vsceTarget, hostPlatform) {
  if (!vsceTarget) return hostPlatform === "win32";
  return vsceTarget.startsWith("win32");
}

/**
 * The shim binary's on-disk basename for the packaged target. The `.exe` suffix
 * follows the TARGET platform, not the host, so a cross-target build names the
 * right file.
 *
 * @param {string | undefined} vsceTarget
 * @param {string} [hostPlatform] - defaults to `process.platform`.
 * @returns {string}
 */
export function shimBinaryBasename(vsceTarget, hostPlatform = process.platform) {
  return targetIsWindows(vsceTarget, hostPlatform) ? `${SHIM_STEM}.exe` : SHIM_STEM;
}

/**
 * The ordered list of candidate source paths for the shim binary:
 *   1. `VERTER_RELAY_SHIM_BINARY` (the CI seam — an explicit absolute path), else
 *   2. platform build → `<repoRoot>/target/<rust-target>/{release,debug}/<basename>`, else
 *   3. universal build → `<repoRoot>/target/{release,debug}/<basename>` (host dir).
 *
 * A platform build NEVER lists the host `target/{release,debug}` dir — that would
 * poison the cross-target VSIX with a host binary.
 *
 * @param {{ vsceTarget?: string, env?: Record<string, string | undefined>, repoRoot: string, hostPlatform?: string }} opts
 * @returns {string[]}
 */
export function shimBinaryCandidates({
  vsceTarget,
  env = process.env,
  repoRoot,
  hostPlatform = process.platform,
}) {
  // Validate the target FIRST (fail closed on an unrecognized non-empty `--target`) so an env
  // override can never bypass the unknown-target mapping — a typo must fail closed even when
  // VERTER_RELAY_SHIM_BINARY is set. `vsceTargetToRustTarget` throws on an unknown target and
  // returns null for a universal (targetless) build.
  const rustTarget = vsceTargetToRustTarget(vsceTarget);

  const explicit = env && env.VERTER_RELAY_SHIM_BINARY;
  if (explicit) return [explicit];

  const basename = shimBinaryBasename(vsceTarget, hostPlatform);
  const targetDir = path.join(repoRoot, "target");
  if (rustTarget) {
    return [
      path.join(targetDir, rustTarget, "release", basename),
      path.join(targetDir, rustTarget, "debug", basename),
    ];
  }
  return [path.join(targetDir, "release", basename), path.join(targetDir, "debug", basename)];
}

/**
 * Resolve the first existing shim-binary candidate, or throw an actionable
 * fail-closed error (packaging never auto-builds).
 *
 * @param {{ vsceTarget?: string, env?: Record<string, string | undefined>, repoRoot: string, hostPlatform?: string, exists?: (p: string) => boolean }} opts
 * @returns {string} the resolved source path.
 */
export function resolveShimBinarySource({
  vsceTarget,
  env = process.env,
  repoRoot,
  hostPlatform = process.platform,
  exists = existsSync,
}) {
  const candidates = shimBinaryCandidates({ vsceTarget, env, repoRoot, hostPlatform });
  const expectedBasename = shimBinaryBasename(vsceTarget, hostPlatform);
  for (const candidate of candidates) {
    if (exists(candidate)) {
      // Validate the RESOLVED source basename for EVERY candidate (env override AND
      // target-dir lookup) before returning it — never copy tsgo-shaped or mismatched bytes.
      assertShimSourceBasename(candidate, expectedBasename);
      return candidate;
    }
  }
  const scope = vsceTarget ? `target ${vsceTarget}` : "the universal build";
  throw new Error(
    `verter-relay-shim binary not found for ${scope}. Looked in:\n` +
      candidates.map((c) => `  - ${c}`).join("\n") +
      `\nBuild it first (cargo build -p verter_relay_shim [--release] [--target <triple>]) ` +
      `or set VERTER_RELAY_SHIM_BINARY to its absolute path. Packaging will NOT auto-build ` +
      `— that would stage a host binary into a cross-target VSIX.`,
  );
}

/**
 * Copy the resolved shim binary into `<extensionDir>/bin/<basename>`. Stages ONLY
 * the Verter shim; a tsgo-shaped basename is refused as a packaging bug.
 *
 * @param {{ vsceTarget?: string, env?: Record<string, string | undefined>, repoRoot: string, extensionDir: string, hostPlatform?: string, exists?: (p: string) => boolean, copy?: typeof cpSync, mkdir?: typeof mkdirSync }} opts
 * @returns {{ source: string, dest: string, basename: string }}
 */
export function stageShimBinary({
  vsceTarget,
  env = process.env,
  repoRoot,
  extensionDir,
  hostPlatform = process.platform,
  exists = existsSync,
  copy = cpSync,
  mkdir = mkdirSync,
  readdir = readdirSync,
  remove = rmSync,
}) {
  const source = resolveShimBinarySource({ vsceTarget, env, repoRoot, hostPlatform, exists });
  const basename = shimBinaryBasename(vsceTarget, hostPlatform);

  // Defense in depth: only the Verter shim is ever staged — NEVER tsgo.
  if (!basename.startsWith(SHIM_STEM) || /tsgo/i.test(basename)) {
    throw new Error(
      `verter-relay-shim packaging: refusing to stage ${JSON.stringify(basename)} ` +
        `— only ${SHIM_STEM} is bundled and tsgo is never packaged.`,
    );
  }

  const binDir = path.join(extensionDir, "bin");
  mkdir(binDir, { recursive: true });

  // Prune STALE artifacts so the FINAL bin/ — not just the copied file — satisfies the
  // no-tsgo + single-shim invariant: any tsgo-shaped file, and any OTHER verter-relay-shim*
  // variant (a prior build's opposite-platform artifact), is removed before we copy.
  for (const entry of readdirSafe(readdir, binDir)) {
    const isTsgo = /tsgo/i.test(entry);
    const isStaleShim = entry.startsWith(SHIM_STEM) && entry !== basename;
    if (isTsgo || isStaleShim) {
      remove(path.join(binDir, entry), { force: true });
    }
  }

  const dest = path.join(binDir, basename);
  copy(source, dest);

  // Assert the FINAL bin/ contents: exactly the one expected shim, and NO tsgo — the shipped
  // invariant verified on the directory itself, not merely on the name we copied.
  const finalEntries = readdirSafe(readdir, binDir);
  const tsgoLeak = finalEntries.filter((f) => /tsgo/i.test(f));
  if (tsgoLeak.length) {
    throw new Error(
      `verter-relay-shim packaging: bin/ must NEVER contain tsgo, found ${JSON.stringify(tsgoLeak)} ` +
        `after staging — refusing to package.`,
    );
  }
  if (!finalEntries.includes(basename)) {
    throw new Error(
      `verter-relay-shim packaging: expected ${JSON.stringify(basename)} in bin/ after staging, ` +
        `found ${JSON.stringify(finalEntries)}.`,
    );
  }
  return { source, dest, basename };
}

/** Read a directory's entries, returning `[]` if it does not exist yet. */
function readdirSafe(readdir, dir) {
  try {
    return readdir(dir);
  } catch {
    return [];
  }
}

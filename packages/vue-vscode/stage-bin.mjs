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
 * matching Rust target dir. The host `target/{release,debug}` dir is used ONLY for a
 * universal (targetless) build, which is DEV-ONLY (behind `allowUniversal`): production
 * packaging requires `--target`.
 */
import {
  chmodSync,
  cpSync,
  existsSync,
  lstatSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  realpathSync,
  rmSync,
} from "node:fs";
import path from "node:path";

/** The fixed on-disk stem of the shim binary (never tsgo-shaped). */
export const SHIM_STEM = "verter-relay-shim";

/**
 * bin/ entries the VSIX is allowed to ship ALONGSIDE the staged shim. EMPTY today — bin/
 * is shim-only. A future extension-owned binary must be added here EXPLICITLY: staging
 * enforces a strict `bin/` whitelist (`[shim basename, ...EXTRA_ALLOWED_BIN_ENTRIES]`) and
 * prunes everything else, so an unlisted file is never silently tolerated.
 */
export const EXTRA_ALLOWED_BIN_ENTRIES = [];

/**
 * The ASCII identity-marker prefix the shim binary embeds in its `.rodata` (see
 * `crates/verter_relay_shim/src/main.rs`, `SHIM_IDENTITY`). Staging greps the candidate file's
 * BYTES for this prefix as an ACCIDENTAL-MIXUP guard — it catches a renamed `tsgo`, a wrong-arch or
 * wrong-target binary, or an unrelated artifact a mis-set path points at. It is NOT a defense
 * against a deliberately forged marker: the prefix is a public, trivially-copyable literal, so an
 * adversary can embed it in any file (a signed / hashed manifest would be a separate scheme). This
 * is a CLOSED cross-language contract: the Rust and JS literals must stay byte-identical (guarded by
 * `shim_identity_marker_prefix_matches_rust_and_js`); do NOT drift it.
 */
export const SHIM_IDENTITY_MARKER = "VERTER_RELAY_SHIM_IDENTITY:v1:";

/**
 * The executable format + CPU arch each VSCE `--target` must produce. A staged binary
 * whose parsed header does not match its target's row poisons the cross-target VSIX, so
 * staging fails closed on a mismatch.
 */
const VSCE_TO_FORMAT_ARCH = {
  "win32-x64": { format: "PE", arch: "x86_64" },
  "win32-arm64": { format: "PE", arch: "aarch64" },
  "linux-x64": { format: "ELF", arch: "x86_64" },
  "linux-arm64": { format: "ELF", arch: "aarch64" },
  "darwin-x64": { format: "MachO", arch: "x86_64" },
  "darwin-arm64": { format: "MachO", arch: "aarch64" },
};

/**
 * Normalize a `process.arch`-style token to the canonical arch used for comparison
 * (`x86_64` / `aarch64`). An unrecognized value is returned verbatim so it fails the
 * later equality check (fail closed) rather than matching by accident.
 *
 * @param {string} arch
 * @returns {string}
 */
function normalizeArch(arch) {
  if (arch === "x64" || arch === "x86_64") return "x86_64";
  if (arch === "arm64" || arch === "aarch64") return "aarch64";
  return arch;
}

/**
 * The executable format a HOST platform produces (used for a universal, targetless dev
 * build, whose staged binary is the host binary).
 *
 * @param {string} hostPlatform - a `process.platform` value.
 * @returns {"PE" | "MachO" | "ELF"}
 */
function hostExecutableFormat(hostPlatform) {
  if (hostPlatform === "win32") return "PE";
  if (hostPlatform === "darwin") return "MachO";
  return "ELF";
}

/**
 * The (format, arch) a staged binary must match: the packaged target's row, or — for a
 * universal (targetless, dev-only) build — the HOST platform/arch.
 *
 * @param {string | undefined} vsceTarget
 * @param {string} hostPlatform
 * @param {string} hostArch - a `process.arch` value.
 * @returns {{ format: string, arch: string }}
 */
function expectedFormatArch(vsceTarget, hostPlatform, hostArch) {
  if (vsceTarget) {
    const fa = VSCE_TO_FORMAT_ARCH[vsceTarget];
    if (!fa) {
      // Unreachable in practice: `vsceTargetToRustTarget` already rejects an unknown
      // target upstream. Kept as a fail-closed backstop rather than a silent host match.
      throw new Error(
        `verter-relay-shim packaging: no expected arch for --target ${JSON.stringify(vsceTarget)}.`,
      );
    }
    return fa;
  }
  return { format: hostExecutableFormat(hostPlatform), arch: normalizeArch(hostArch) };
}

/**
 * Parse an executable file's header bytes into its `{ format, arch }`, or `null` if the
 * bytes are not a recognized ELF / PE / thin little-endian Mach-O 64-bit image. A
 * recognized format with an UNKNOWN machine returns `arch: null`, which fails the later
 * arch-equality check (fail closed).
 *
 * @param {Buffer} buf
 * @returns {{ format: string, arch: string | null } | null}
 */
function detectExecutableFormatArch(buf) {
  // ELF: 7F 45 4C 46; e_machine = LE u16 at offset 18.
  if (
    buf.length >= 20 &&
    buf[0] === 0x7f &&
    buf[1] === 0x45 &&
    buf[2] === 0x4c &&
    buf[3] === 0x46
  ) {
    const machine = buf.readUInt16LE(18);
    const arch = machine === 0x3e ? "x86_64" : machine === 0xb7 ? "aarch64" : null;
    return { format: "ELF", arch };
  }
  // PE: 'MZ' at 0; PE header offset = LE u32 at 0x3C; 'PE\0\0' there; Machine = LE u16 at +4.
  if (buf.length >= 0x40 && buf[0] === 0x4d && buf[1] === 0x5a) {
    const peOff = buf.readUInt32LE(0x3c);
    if (
      buf.length >= peOff + 6 &&
      buf[peOff] === 0x50 &&
      buf[peOff + 1] === 0x45 &&
      buf[peOff + 2] === 0x00 &&
      buf[peOff + 3] === 0x00
    ) {
      const machine = buf.readUInt16LE(peOff + 4);
      const arch = machine === 0x8664 ? "x86_64" : machine === 0xaa64 ? "aarch64" : null;
      return { format: "PE", arch };
    }
    return null;
  }
  // Mach-O thin, little-endian, 64-bit: MH_MAGIC_64 (CF FA ED FE); cputype = LE u32 at offset 4.
  if (buf.length >= 8 && buf[0] === 0xcf && buf[1] === 0xfa && buf[2] === 0xed && buf[3] === 0xfe) {
    const cputype = buf.readUInt32LE(4);
    const arch = cputype === 0x01000007 ? "x86_64" : cputype === 0x0100000c ? "aarch64" : null;
    return { format: "MachO", arch };
  }
  return null;
}

/**
 * Guard a RESOLVED source path's BYTES against a WRONG-ARTIFACT MIXUP (a renamed tsgo, a
 * wrong-arch / wrong-target binary, or an unrelated file), before it is copied into the VSIX.
 * Basename validation ({@link assertShimSourceBasename}) alone cannot: an env override or a wrong
 * target-dir lookup can point a correctly-named path at the wrong bytes. This asserts, fail-closed:
 *   1. an ABSOLUTE, REGULAR, non-symlink source whose realpath resolves;
 *   2. an executable format + CPU arch matching the packaged target (or the host, for a
 *      universal build);
 *   3. the embedded {@link SHIM_IDENTITY_MARKER} in the file's bytes.
 *
 * This is an accidental-mixup / wrong-artifact guard, NOT forgery-proof provenance: the marker is a
 * public literal an adversary can copy into any file (a signed / hashed manifest is a separate,
 * out-of-scope scheme).
 *
 * @param {{ source: string, vsceTarget?: string, hostPlatform?: string, hostArch?: string, lstat?: (p: string) => { isSymbolicLink(): boolean, isFile(): boolean }, realpath?: (p: string) => string, readBytes?: (p: string) => Buffer }} opts
 */
export function assertShimSourceProvenance({
  source,
  vsceTarget,
  hostPlatform = process.platform,
  hostArch = process.arch,
  lstat = (p) => lstatSync(p),
  realpath = (p) => realpathSync(p),
  readBytes = (p) => readFileSync(p),
}) {
  // 1a. Absolute path — a relative source cannot be reliably resolved during packaging.
  if (!path.isAbsolute(source)) {
    throw new Error(
      `verter-relay-shim packaging: source ${JSON.stringify(source)} must be an ABSOLUTE path ` +
        `(a relative VERTER_RELAY_SHIM_BINARY is refused).`,
    );
  }

  // 1b. Regular, non-symlink file. lstat does NOT follow symlinks, so a symlink is
  // caught here rather than silently resolving to (and staging) some other file.
  let stat;
  try {
    stat = lstat(source);
  } catch (e) {
    throw new Error(
      `verter-relay-shim packaging: cannot stat source ${JSON.stringify(source)}: ${e}`,
    );
  }
  if (stat.isSymbolicLink()) {
    throw new Error(
      `verter-relay-shim packaging: source ${JSON.stringify(source)} is a symlink — refusing ` +
        `to stage a symbolic-link source; point VERTER_RELAY_SHIM_BINARY at the real binary.`,
    );
  }
  if (!stat.isFile()) {
    throw new Error(
      `verter-relay-shim packaging: source ${JSON.stringify(source)} is not a regular file.`,
    );
  }

  // 1c. realpath must resolve (a dangling/broken path is refused).
  try {
    realpath(source);
  } catch (e) {
    throw new Error(
      `verter-relay-shim packaging: could not resolve the realpath of source ` +
        `${JSON.stringify(source)}: ${e}`,
    );
  }

  // 2. Read the bytes once (packaging is a one-shot step, so reading the whole file to
  // parse the header AND scan for the identity marker is fine).
  let buf;
  try {
    buf = readBytes(source);
  } catch (e) {
    throw new Error(
      `verter-relay-shim packaging: could not read source ${JSON.stringify(source)}: ${e}`,
    );
  }

  // 3. Executable format + CPU arch must match the packaged target.
  const detected = detectExecutableFormatArch(buf);
  if (!detected) {
    throw new Error(
      `verter-relay-shim packaging: source ${JSON.stringify(source)} is not a recognized ` +
        `executable format (ELF / PE / Mach-O) — refusing to stage an unrecognized binary.`,
    );
  }
  const expected = expectedFormatArch(vsceTarget, hostPlatform, hostArch);
  if (detected.format !== expected.format || detected.arch !== expected.arch) {
    const scope = vsceTarget ? `--target ${vsceTarget}` : "the universal (host) build";
    throw new Error(
      `verter-relay-shim packaging: source ${JSON.stringify(source)} is ` +
        `${detected.format}/${detected.arch ?? "unknown"} but ${scope} requires ` +
        `${expected.format}/${expected.arch} — refusing to stage a wrong-architecture binary.`,
    );
  }

  // 4. Embedded identity marker — the accidental-mixup guard (a renamed tsgo / wrong artifact); not
  //    forgery-proof, since the marker is a public literal.
  if (buf.indexOf(SHIM_IDENTITY_MARKER) < 0) {
    throw new Error(
      `verter-relay-shim packaging: source ${JSON.stringify(source)} does not embed the shim ` +
        `identity marker ${JSON.stringify(SHIM_IDENTITY_MARKER)} — refusing to stage a binary ` +
        `that is not the Verter relay shim (a renamed tsgo / unrelated binary).`,
    );
  }
}

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
 *   2. platform build → `<repoRoot>/target/<rust-target>/release/<basename>`, else
 *   3. universal (DEV-ONLY) build → `<repoRoot>/target/release/<basename>` (host dir),
 *      reachable ONLY behind `allowUniversal: true`.
 *
 * Candidates are RELEASE-ONLY by default: a production VSIX must never silently ship an
 * unoptimized debug binary. The `debug` profile is a DEV-ONLY opt-in fallback, appended
 * AFTER `release` only when `allowDebug: true` (never set by `package.mjs`).
 *
 * A platform build NEVER lists the host `target/` dir — that would poison the
 * cross-target VSIX with a host binary. A targetless build has no platform target, so
 * its VSIX would install anywhere; production packaging must therefore pass `--target`,
 * and the host-dir fallback fails closed unless `allowUniversal` is set.
 *
 * @param {{ vsceTarget?: string, env?: Record<string, string | undefined>, repoRoot: string, hostPlatform?: string, allowUniversal?: boolean, allowDebug?: boolean }} opts
 * @returns {string[]}
 */
export function shimBinaryCandidates({
  vsceTarget,
  env = process.env,
  repoRoot,
  hostPlatform = process.platform,
  allowUniversal = false,
  allowDebug = false,
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
  // Release-only by default; `debug` is a dev-only fallback appended AFTER release.
  const profiles = allowDebug ? ["release", "debug"] : ["release"];
  if (rustTarget) {
    return profiles.map((profile) => path.join(targetDir, rustTarget, profile, basename));
  }
  // Universal (targetless) build → the HOST target/ dir. A VSIX with no platform target
  // installs anywhere, so this is a DEV-ONLY path: production packaging must pass --target.
  // Fail closed unless the caller explicitly opts into a universal dev build.
  if (!allowUniversal) {
    throw new Error(
      `verter-relay-shim packaging: production packaging requires --target; ` +
        `pass --allow-universal (allowUniversal: true) for a local dev build.`,
    );
  }
  return profiles.map((profile) => path.join(targetDir, profile, basename));
}

/**
 * Resolve the first existing shim-binary candidate, or throw an actionable
 * fail-closed error (packaging never auto-builds).
 *
 * @param {{ vsceTarget?: string, env?: Record<string, string | undefined>, repoRoot: string, hostPlatform?: string, allowUniversal?: boolean, allowDebug?: boolean, exists?: (p: string) => boolean }} opts
 * @returns {string} the resolved source path.
 */
export function resolveShimBinarySource({
  vsceTarget,
  env = process.env,
  repoRoot,
  hostPlatform = process.platform,
  allowUniversal = false,
  allowDebug = false,
  exists = existsSync,
}) {
  const candidates = shimBinaryCandidates({
    vsceTarget,
    env,
    repoRoot,
    hostPlatform,
    allowUniversal,
    allowDebug,
  });
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
 * Copy the resolved shim binary into `<extensionDir>/bin/<basename>`. Stages ONLY the
 * Verter shim: the resolved source's basename AND bytes are validated (arch + embedded
 * identity), bin/ is pruned to the strict whitelist, and the staged binary is made
 * executable on a Unix target. A tsgo-shaped basename is refused as a packaging bug.
 *
 * @param {{ vsceTarget?: string, env?: Record<string, string | undefined>, repoRoot: string, extensionDir: string, hostPlatform?: string, hostArch?: string, allowUniversal?: boolean, allowDebug?: boolean, exists?: (p: string) => boolean, copy?: typeof cpSync, mkdir?: typeof mkdirSync, readdir?: typeof readdirSync, remove?: typeof rmSync, lstat?: (p: string) => { isSymbolicLink(): boolean, isFile(): boolean }, realpath?: (p: string) => string, readBytes?: (p: string) => Buffer, chmod?: (p: string, mode: number) => void }} opts
 * @returns {{ source: string, dest: string, basename: string }}
 */
export function stageShimBinary({
  vsceTarget,
  env = process.env,
  repoRoot,
  extensionDir,
  hostPlatform = process.platform,
  hostArch = process.arch,
  allowUniversal = false,
  allowDebug = false,
  exists = existsSync,
  copy = cpSync,
  mkdir = mkdirSync,
  readdir = readdirSync,
  remove = rmSync,
  lstat = (p) => lstatSync(p),
  realpath = (p) => realpathSync(p),
  readBytes = (p) => readFileSync(p),
  chmod = (p, mode) => chmodSync(p, mode),
}) {
  const source = resolveShimBinarySource({
    vsceTarget,
    env,
    repoRoot,
    hostPlatform,
    allowUniversal,
    allowDebug,
    exists,
  });
  const basename = shimBinaryBasename(vsceTarget, hostPlatform);

  // Defense in depth: only the Verter shim is ever staged — NEVER tsgo.
  if (!basename.startsWith(SHIM_STEM) || /tsgo/i.test(basename)) {
    throw new Error(
      `verter-relay-shim packaging: refusing to stage ${JSON.stringify(basename)} ` +
        `— only ${SHIM_STEM} is bundled and tsgo is never packaged.`,
    );
  }

  // Guard the resolved source's BYTES against a wrong-artifact mixup (absolute regular non-symlink
  // source, matching executable format + CPU arch, embedded identity marker) BEFORE any bin/
  // mutation — a mismatched / renamed / wrong-arch binary fails closed with nothing staged. This
  // catches an accidental mixup, not a deliberately forged marker.
  assertShimSourceProvenance({
    source,
    vsceTarget,
    hostPlatform,
    hostArch,
    lstat,
    realpath,
    readBytes,
  });

  // The strict bin/ whitelist: exactly the staged shim plus any explicitly-allowed extras
  // (none today). EVERYTHING else — a stale tsgo, an opposite-platform shim, an unrelated
  // leftover — is pruned so the FINAL bin/ is exactly this set.
  const allowedBinEntries = [basename, ...EXTRA_ALLOWED_BIN_ENTRIES];

  const binDir = path.join(extensionDir, "bin");
  mkdir(binDir, { recursive: true });

  // Prune any pre-existing bin/ entry NOT in the whitelist BEFORE we copy, so the FINAL
  // bin/ — not just the copied file — satisfies the invariant. `recursive: true` so a stale
  // DIRECTORY entry is removed too (a bare `rmSync` throws EISDIR on a directory), never left to
  // survive the whitelist.
  for (const entry of readdirSafe(readdir, binDir)) {
    if (!allowedBinEntries.includes(entry)) {
      remove(path.join(binDir, entry), { force: true, recursive: true });
    }
  }

  const dest = path.join(binDir, basename);
  copy(source, dest);

  // Re-validate the FINAL staged bytes AT `dest`, not just the pre-copy source. Validating the
  // source and then copying is a TOCTOU: if the source file is swapped between the source check and
  // the copy, the bytes that actually land in the VSIX are never proven. Re-running the same
  // wrong-artifact guard on the copied dest closes that gap, so the FINAL shipped bytes are the ones
  // checked. The injectable `readBytes` seam is reused, so hermetic tests can model a swap.
  assertShimSourceProvenance({
    source: dest,
    vsceTarget,
    hostPlatform,
    hostArch,
    lstat,
    realpath,
    readBytes,
  });

  // Ensure the staged binary is executable on a Unix TARGET — vsce preserves the mode it
  // finds on disk, and a copy (or a source lacking +x) can drop the bit, shipping a shim
  // the editor cannot spawn. No-op for a Windows target (the +x bit is meaningless on NTFS).
  if (!targetIsWindows(vsceTarget, hostPlatform)) {
    chmod(dest, 0o755);
  }

  // Assert the FINAL bin/ contents equal the whitelist EXACTLY — the shipped invariant
  // verified on the directory itself, not merely on the name we copied.
  const finalEntries = readdirSafe(readdir, binDir).slice().sort();
  const tsgoLeak = finalEntries.filter((f) => /tsgo/i.test(f));
  if (tsgoLeak.length) {
    throw new Error(
      `verter-relay-shim packaging: bin/ must NEVER contain tsgo, found ${JSON.stringify(tsgoLeak)} ` +
        `after staging — refusing to package.`,
    );
  }
  const expectedEntries = allowedBinEntries.slice().sort();
  if (
    finalEntries.length !== expectedEntries.length ||
    finalEntries.some((entry, i) => entry !== expectedEntries[i])
  ) {
    throw new Error(
      `verter-relay-shim packaging: bin/ must contain EXACTLY ${JSON.stringify(expectedEntries)} ` +
        `after staging, found ${JSON.stringify(finalEntries)} — refusing to package.`,
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

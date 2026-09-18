#!/usr/bin/env node

/**
 * release-publish.mjs — pure helpers behind scripts/release-publish.mjs, the
 * one publish path release.yml and a local release both run.
 *
 * Nothing here touches the filesystem, the network or a child process. The
 * CLI hands these functions plain data (artifact listings, package manifests,
 * child-process output, tar bytes) and acts on what comes back, so every
 * discrimination the release depends on is unit-testable against small
 * synthetic fixtures:
 *
 *   - which CI artifact feeds which platform package, and what is missing;
 *   - how an npm / cargo publish attempt is classified (published, already
 *     on the registry, one-time password needed, provenance-log conflict,
 *     failed);
 *   - the executable bit on shipped binaries, which a tarball packed on a
 *     filesystem without POSIX modes (Windows) would otherwise lose;
 *   - the publish loop itself, including the OTP re-prompt.
 */

import { gunzipSync, gzipSync } from "node:zlib";

// ---------------------------------------------------------------------------
// Artifact → platform-package staging
// ---------------------------------------------------------------------------

/**
 * Binary families: the `packages/<family>/npm/<platform>` sub-package trees
 * and the release-workflow artifact that feeds each one.
 *
 * `locate` says how a platform package finds its artifact:
 *   - `platform`: the artifact is named after the npm platform package
 *     (`lsp-linux-x64-gnu`) and holds the binary under the file name the
 *     package's `files` list names;
 *   - `file`: the artifact is named after the Rust target triple
 *     (`native-aarch64-apple-darwin`) and the binary file name itself carries
 *     the platform (`verter-native.darwin-arm64.node`), so the file name is
 *     the lookup key and must be unique across the family's artifacts.
 *
 * `executable` marks families whose shipped file is spawned as a process and
 * therefore must carry the executable bit inside the published tarball. A
 * `.node` addon is loaded with dlopen and needs no bit.
 */
export const BINARY_FAMILIES = Object.freeze({
  native: Object.freeze({ artifactPrefix: "native-", locate: "file", executable: false }),
  "verter-lsp": Object.freeze({ artifactPrefix: "lsp-", locate: "platform", executable: true }),
  "verter-mcp": Object.freeze({ artifactPrefix: "mcp-", locate: "platform", executable: true }),
  "verter-tsc": Object.freeze({ artifactPrefix: "tsc-", locate: "platform", executable: true }),
});

export const WASM_ARTIFACT = "wasm";
/** The two files the wasm artifact must carry for `@verter/wasm` to load. */
export const WASM_REQUIRED_FILES = Object.freeze([
  "wasm/verter_wasm_bg.wasm",
  "wasm/verter_wasm.js",
]);
export const WASM_PACKAGE_DIR = "packages/wasm";

export const NATIVE_LOADER_ARTIFACT = "native-loader";
export const NATIVE_LOADER_FILE = "index.js";
export const NATIVE_LOADER_DESTINATION = "packages/native/dist/index.js";

/**
 * Split a repo-relative platform package dir into its family and platform.
 * `packages/native/npm/darwin-arm64` → `{ family: "native", platform: "darwin-arm64" }`.
 * Returns null for anything else.
 */
export function parsePlatformDir(dir) {
  const match = /^packages\/([^/]+)\/npm\/([^/]+)$/.exec(dir.split("\\").join("/"));
  if (!match) return null;
  return { family: match[1], platform: match[2] };
}

function basename(posixPath) {
  const idx = posixPath.lastIndexOf("/");
  return idx === -1 ? posixPath : posixPath.slice(idx + 1);
}

/**
 * Plan the copies that put every platform package's binary in place.
 *
 * @param {Array<{dir: string, files: string[]}>} platformPackages
 *   repo-relative platform package dirs with their manifest `files` list — the
 *   package's own declaration of what it ships is the authority for the file
 *   names expected from the artifact.
 * @param {Record<string, string[]>} artifacts
 *   artifact name → posix-relative paths of the files it contains.
 * @returns {{copies: Array<{artifact: string, source: string, destination: string, executable: boolean}>, problems: string[]}}
 *   `problems` is non-empty when any platform package cannot be fed — the
 *   caller must refuse to publish rather than ship a package whose install
 *   resolves and then cannot start.
 */
export function planPlatformStaging(platformPackages, artifacts) {
  const copies = [];
  const problems = [];
  const artifactNames = Object.keys(artifacts);

  for (const { dir, files } of platformPackages) {
    const parsed = parsePlatformDir(dir);
    if (!parsed) {
      problems.push(`${dir}: not a packages/<family>/npm/<platform> directory`);
      continue;
    }
    const family = BINARY_FAMILIES[parsed.family];
    if (!family) {
      problems.push(
        `${dir}: no binary family maps packages/${parsed.family} to a release artifact (known: ${Object.keys(BINARY_FAMILIES).join(", ")})`,
      );
      continue;
    }
    if (!Array.isArray(files) || files.length === 0) {
      problems.push(`${dir}: package.json declares no "files" — nothing to stage`);
      continue;
    }

    for (const file of files) {
      if (family.locate === "platform") {
        const artifact = `${family.artifactPrefix}${parsed.platform}`;
        const entries = artifacts[artifact];
        if (!entries) {
          problems.push(`${dir}: artifact "${artifact}" was not downloaded`);
          continue;
        }
        const source = entries.find((entry) => basename(entry) === file);
        if (!source) {
          problems.push(
            `${dir}: artifact "${artifact}" has no "${file}" (has: ${entries.join(", ") || "nothing"})`,
          );
          continue;
        }
        copies.push({
          artifact,
          source,
          destination: `${dir}/${file}`,
          executable: family.executable,
        });
      } else {
        const matches = [];
        for (const artifact of artifactNames) {
          if (!artifact.startsWith(family.artifactPrefix)) continue;
          for (const entry of artifacts[artifact]) {
            if (basename(entry) === file) matches.push({ artifact, source: entry });
          }
        }
        if (matches.length === 0) {
          problems.push(`${dir}: no "${family.artifactPrefix}*" artifact contains "${file}"`);
          continue;
        }
        if (matches.length > 1) {
          problems.push(
            `${dir}: "${file}" is ambiguous across artifacts ${matches.map((m) => m.artifact).join(", ")}`,
          );
          continue;
        }
        copies.push({
          artifact: matches[0].artifact,
          source: matches[0].source,
          destination: `${dir}/${file}`,
          executable: family.executable,
        });
      }
    }
  }

  return { copies, problems };
}

/**
 * Plan the non-platform staging: the wasm build into `packages/wasm/` and the
 * napi-generated loader into the main `@verter/native` package's `dist/`.
 *
 * @param {Record<string, string[]>} artifacts artifact name → posix-relative file paths
 */
export function planCoreStaging(artifacts) {
  const copies = [];
  const problems = [];

  const wasm = artifacts[WASM_ARTIFACT];
  if (!wasm) {
    problems.push(`artifact "${WASM_ARTIFACT}" was not downloaded`);
  } else {
    for (const required of WASM_REQUIRED_FILES) {
      if (!wasm.includes(required)) {
        problems.push(`artifact "${WASM_ARTIFACT}" has no "${required}"`);
      }
    }
    for (const entry of wasm) {
      copies.push({
        artifact: WASM_ARTIFACT,
        source: entry,
        destination: `${WASM_PACKAGE_DIR}/${entry}`,
        executable: false,
      });
    }
  }

  const loader = artifacts[NATIVE_LOADER_ARTIFACT];
  if (!loader) {
    problems.push(`artifact "${NATIVE_LOADER_ARTIFACT}" was not downloaded`);
  } else if (!loader.includes(NATIVE_LOADER_FILE)) {
    problems.push(`artifact "${NATIVE_LOADER_ARTIFACT}" has no "${NATIVE_LOADER_FILE}"`);
  } else {
    copies.push({
      artifact: NATIVE_LOADER_ARTIFACT,
      source: NATIVE_LOADER_FILE,
      destination: NATIVE_LOADER_DESTINATION,
      executable: false,
    });
  }

  return { copies, problems };
}

// ---------------------------------------------------------------------------
// Publish-attempt classification
// ---------------------------------------------------------------------------

/**
 * Classify one `npm publish` attempt.
 *
 * Only "already on the registry" is tolerated — a re-run after a partial
 * publish must skip what landed and continue. `TLOG_CREATE_ENTRY_ERROR` means
 * a previous attempt already signed provenance for this exact tarball and the
 * caller retries once without `--provenance`. `otp-required` (npm's `EOTP`)
 * is the batching signal: the one-time password expired or was never given.
 *
 * @returns {"published"|"already-published"|"otp-required"|"tlog-conflict"|"failed"}
 */
export function classifyNpmPublishOutcome(exitCode, output) {
  if (exitCode === 0) return "published";
  const text = String(output ?? "");
  if (/\bEOTP\b|one-time pass(?:word)?/i.test(text)) return "otp-required";
  if (/E403|cannot publish over|previously published|EPUBLISHCONFLICT/i.test(text)) {
    return "already-published";
  }
  if (/TLOG_CREATE_ENTRY_ERROR/.test(text)) return "tlog-conflict";
  return "failed";
}

/**
 * Classify one `cargo publish` attempt. crates.io reports an existing version
 * as "already uploaded"; older toolchains said "already exists".
 *
 * @returns {"published"|"already-published"|"failed"}
 */
export function classifyCargoPublishOutcome(exitCode, output) {
  if (exitCode === 0) return "published";
  if (/already (?:exists|uploaded)/i.test(String(output ?? ""))) return "already-published";
  return "failed";
}

/**
 * Arguments for `npm publish <tarball>`. A pre-release channel publishes
 * under its dist-tag so it never moves `latest`; `latest` itself is npm's
 * default and is not passed explicitly.
 */
export function npmPublishArgs(
  tarball,
  { distTag, provenance = false, otp = null, dryRun = false },
) {
  const args = ["publish", tarball, "--access", "public"];
  if (distTag && distTag !== "latest") args.push("--tag", distTag);
  if (provenance) args.push("--provenance");
  if (otp) args.push("--otp", otp);
  if (dryRun) args.push("--dry-run");
  return args;
}

/** npm dist-tag for a version: its pre-release channel, else `latest`. */
export function distTagForVersion(version) {
  const match = /-(alpha|beta|rc)(?:\.|$)/.exec(version);
  return match ? match[1] : "latest";
}

// ---------------------------------------------------------------------------
// Tarball executable bits
// ---------------------------------------------------------------------------

const BLOCK = 512;
const MODE_OFFSET = 100;
const SIZE_OFFSET = 124;
const CHKSUM_OFFSET = 148;
const TYPEFLAG_OFFSET = 156;
const MAGIC_OFFSET = 257;
const PREFIX_OFFSET = 345;

function readString(buf, offset, length) {
  const slice = buf.subarray(offset, offset + length);
  const nul = slice.indexOf(0);
  return slice.subarray(0, nul === -1 ? length : nul).toString("utf8");
}

function readOctal(buf, offset, length) {
  const raw = readString(buf, offset, length).trim();
  if (raw === "") return 0;
  if (!/^[0-7]+$/.test(raw)) {
    throw new Error(`tar header field at ${offset} is not octal: ${JSON.stringify(raw)}`);
  }
  return parseInt(raw, 8);
}

function writeChecksum(header) {
  header.fill(0x20, CHKSUM_OFFSET, CHKSUM_OFFSET + 8);
  let sum = 0;
  for (let i = 0; i < BLOCK; i++) sum += header[i];
  header.write(`${sum.toString(8).padStart(6, "0")}\0 `, CHKSUM_OFFSET, 8, "latin1");
}

function entryName(header, pendingLongName) {
  if (pendingLongName !== null) return pendingLongName;
  const name = readString(header, 0, 100);
  const magic = readString(header, MAGIC_OFFSET, 6);
  const prefix = magic.startsWith("ustar") ? readString(header, PREFIX_OFFSET, 155) : "";
  return prefix ? `${prefix}/${name}` : name;
}

function paxPath(data) {
  // pax extended header: repeated "<len> <key>=<value>\n" records.
  let offset = 0;
  const text = data.toString("utf8");
  while (offset < text.length) {
    const space = text.indexOf(" ", offset);
    if (space === -1) break;
    const length = Number(text.slice(offset, space));
    if (!Number.isFinite(length) || length <= 0) break;
    const record = text.slice(space + 1, offset + length - 1);
    const eq = record.indexOf("=");
    if (eq !== -1 && record.slice(0, eq) === "path") return record.slice(eq + 1);
    offset += length;
  }
  return null;
}

/**
 * Set mode 0755 on the named regular-file entries of an UNCOMPRESSED tar and
 * fix each patched header's checksum. Names are relative to the npm `package/`
 * root (`verter-lsp`, not `package/verter-lsp`).
 *
 * The rest of the archive is copied byte for byte; GNU long-name (`L`) and pax
 * (`x`) headers are honoured when resolving an entry's name.
 *
 * @returns {{bytes: Buffer, patched: string[]}} the rewritten archive and the
 *   entry names that were actually found and patched — the caller compares
 *   this against what it asked for and fails closed on a miss.
 */
export function markTarEntriesExecutable(tarBytes, names) {
  const wanted = new Set(names);
  const out = Buffer.from(tarBytes);
  const patched = [];
  let offset = 0;
  let pendingLongName = null;

  while (offset + BLOCK <= out.length) {
    const header = out.subarray(offset, offset + BLOCK);
    if (header.every((b) => b === 0)) break; // end-of-archive marker

    const size = readOctal(header, SIZE_OFFSET, 12);
    const typeflag = String.fromCharCode(header[TYPEFLAG_OFFSET] || 0x30);
    const dataStart = offset + BLOCK;
    const dataBlocks = Math.ceil(size / BLOCK);
    const data = out.subarray(dataStart, dataStart + size);

    if (typeflag === "L") {
      pendingLongName = readString(data, 0, size);
    } else if (typeflag === "x") {
      pendingLongName = paxPath(data) ?? pendingLongName;
    } else {
      const name = entryName(header, pendingLongName);
      pendingLongName = null;
      const isRegular = typeflag === "0" || typeflag === "\0";
      const relative = name.startsWith("package/") ? name.slice("package/".length) : name;
      if (isRegular && wanted.has(relative)) {
        header.write("0000755\0", MODE_OFFSET, 8, "latin1");
        writeChecksum(header);
        patched.push(relative);
      }
    }

    offset = dataStart + dataBlocks * BLOCK;
  }

  return { bytes: out, patched };
}

/**
 * `markTarEntriesExecutable` over a gzipped npm tarball. Returns the rewritten
 * `.tgz` bytes plus the names patched.
 */
export function markTarballEntriesExecutable(tgzBytes, names) {
  const { bytes, patched } = markTarEntriesExecutable(gunzipSync(tgzBytes), names);
  return { bytes: gzipSync(bytes), patched };
}

/** Modes of the regular-file entries in an uncompressed tar, by name. */
export function listTarModes(tarBytes) {
  const modes = new Map();
  let offset = 0;
  let pendingLongName = null;
  while (offset + BLOCK <= tarBytes.length) {
    const header = tarBytes.subarray(offset, offset + BLOCK);
    if (header.every((b) => b === 0)) break;
    const size = readOctal(header, SIZE_OFFSET, 12);
    const typeflag = String.fromCharCode(header[TYPEFLAG_OFFSET] || 0x30);
    const data = tarBytes.subarray(offset + BLOCK, offset + BLOCK + size);
    if (typeflag === "L") {
      pendingLongName = readString(data, 0, size);
    } else if (typeflag === "x") {
      pendingLongName = paxPath(data) ?? pendingLongName;
    } else {
      const name = entryName(header, pendingLongName);
      pendingLongName = null;
      modes.set(name, readOctal(header, MODE_OFFSET, 8));
    }
    offset += BLOCK + Math.ceil(size / BLOCK) * BLOCK;
  }
  return modes;
}

// ---------------------------------------------------------------------------
// The publish loop
// ---------------------------------------------------------------------------

/** Consecutive one-time-password rejections tolerated for ONE target. */
export const MAX_OTP_RETRIES_PER_TARGET = 3;

/**
 * Publish targets in order, one at a time, classifying every attempt.
 *
 * Effects are injected so the loop is testable:
 *   - `publish(target, { otp, provenance })` → `{ exitCode, output }`
 *   - `promptOtp(reason)` → a fresh code, or `null` when no code can be
 *     obtained (non-interactive), which fails the target;
 *   - `log(line)`.
 *
 * A one-time password stays in use until the registry rejects it (`EOTP`);
 * the loop then asks for a new one and retries the SAME target, so a batch of
 * publishes shares one code and a fresh code is requested exactly when the
 * registry demands it. Every other failure is recorded and the loop moves on,
 * so a partial publish is reported in full instead of stopping at the first
 * problem; the caller fails the run when `failed` is non-empty.
 */
export async function publishSequentially(targets, effects) {
  const { publish, promptOtp = async () => null, provenance = false, log = () => {} } = effects;
  const summary = { published: [], skipped: [], failed: [], otpPrompts: 0 };
  let otp = effects.initialOtp ?? null;

  for (const target of targets) {
    let useProvenance = provenance;
    let otpRejections = 0;
    let settled = false;

    while (!settled) {
      const { exitCode, output } = await publish(target, { otp, provenance: useProvenance });
      const outcome = classifyNpmPublishOutcome(exitCode, output);

      switch (outcome) {
        case "published":
          log(
            `  OK: ${target.label} published${useProvenance ? "" : provenance ? " (without provenance)" : ""}`,
          );
          summary.published.push(target.label);
          settled = true;
          break;
        case "already-published":
          log(`  SKIP: ${target.label} already on the registry`);
          summary.skipped.push(target.label);
          settled = true;
          break;
        case "tlog-conflict":
          if (useProvenance) {
            log(
              `  RETRY: ${target.label} (provenance log conflict — retrying without --provenance)`,
            );
            useProvenance = false;
            break;
          }
          summary.failed.push({ label: target.label, output });
          settled = true;
          break;
        case "otp-required": {
          otpRejections += 1;
          if (otpRejections > MAX_OTP_RETRIES_PER_TARGET) {
            summary.failed.push({
              label: target.label,
              output: `one-time password rejected ${otpRejections} times`,
            });
            settled = true;
            break;
          }
          summary.otpPrompts += 1;
          const fresh = await promptOtp(
            otp === null
              ? `${target.label}: the registry requires a one-time password`
              : `${target.label}: the one-time password was rejected (expired?)`,
          );
          if (!fresh) {
            summary.failed.push({
              label: target.label,
              output: "one-time password required and none was provided",
            });
            settled = true;
            break;
          }
          otp = fresh;
          break;
        }
        default:
          log(`  FAIL: ${target.label} (exit ${exitCode})`);
          summary.failed.push({ label: target.label, output });
          settled = true;
      }
    }
  }

  return summary;
}

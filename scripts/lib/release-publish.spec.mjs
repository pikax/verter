#!/usr/bin/env node

// Tests for release-publish.mjs. Run: node --test scripts/lib/release-publish.spec.mjs
//
// Every helper is pure, so the discriminations the release depends on are
// exercised against small synthetic fixtures: a platform package whose
// artifact is missing or ambiguous, each npm/cargo publish outcome, the
// executable-bit rewrite on a hand-built ustar archive (including a pax
// header), and the publish loop's one-time-password re-prompt.

import assert from "node:assert/strict";
import { resolve } from "node:path";
import test from "node:test";
import { pathToFileURL } from "node:url";
import { gunzipSync, gzipSync } from "node:zlib";

import {
  classifyCargoPublishOutcome,
  invokedAsEntrypoint,
  classifyNpmPublishOutcome,
  distTagForVersion,
  listTarModes,
  markTarballEntriesExecutable,
  markTarEntriesExecutable,
  MAX_OTP_RETRIES_PER_TARGET,
  npmPublishArgs,
  parsePlatformDir,
  planCoreStaging,
  planPlatformStaging,
  publishSequentially,
} from "./release-publish.mjs";

// ---------------------------------------------------------------------------
// Staging plans
// ---------------------------------------------------------------------------

const PLATFORM_PACKAGES = [
  { dir: "packages/native/npm/darwin-arm64", files: ["verter-native.darwin-arm64.node"] },
  { dir: "packages/native/npm/win32-x64-msvc", files: ["verter-native.win32-x64-msvc.node"] },
  { dir: "packages/verter-lsp/npm/darwin-arm64", files: ["verter-lsp"] },
  { dir: "packages/verter-lsp/npm/win32-x64-msvc", files: ["verter-lsp.exe"] },
  { dir: "packages/verter-mcp/npm/darwin-arm64", files: ["verter-mcp"] },
  { dir: "packages/verter-tsc/npm/darwin-arm64", files: ["verter-tsc"] },
];

function completeArtifacts() {
  return {
    "native-aarch64-apple-darwin": ["verter-native.darwin-arm64.node"],
    "native-x86_64-pc-windows-msvc": ["verter-native.win32-x64-msvc.node"],
    "lsp-darwin-arm64": ["verter-lsp", "verter-relay-shim"],
    "lsp-win32-x64-msvc": ["verter-lsp.exe", "verter-relay-shim.exe"],
    "mcp-darwin-arm64": ["verter-mcp"],
    "tsc-darwin-arm64": ["verter-tsc"],
    wasm: ["wasm/verter_wasm_bg.wasm", "wasm/verter_wasm.js", "dist/index.mjs"],
    "native-loader": ["index.js"],
  };
}

test("parsePlatformDir splits a platform package dir and rejects anything else", () => {
  assert.deepEqual(parsePlatformDir("packages/native/npm/darwin-arm64"), {
    family: "native",
    platform: "darwin-arm64",
  });
  assert.deepEqual(parsePlatformDir("packages\\verter-lsp\\npm\\linux-x64-gnu"), {
    family: "verter-lsp",
    platform: "linux-x64-gnu",
  });
  assert.equal(parsePlatformDir("packages/verter-lsp"), null);
  assert.equal(parsePlatformDir("packages/verter-lsp/npm"), null);
});

test("planPlatformStaging feeds every platform package from its artifact, by platform name or by file name", () => {
  const { copies, problems } = planPlatformStaging(PLATFORM_PACKAGES, completeArtifacts());
  assert.deepEqual(problems, []);
  assert.deepEqual(
    copies.map((c) => [c.artifact, c.source, c.destination, c.executable]),
    [
      [
        "native-aarch64-apple-darwin",
        "verter-native.darwin-arm64.node",
        "packages/native/npm/darwin-arm64/verter-native.darwin-arm64.node",
        false,
      ],
      [
        "native-x86_64-pc-windows-msvc",
        "verter-native.win32-x64-msvc.node",
        "packages/native/npm/win32-x64-msvc/verter-native.win32-x64-msvc.node",
        false,
      ],
      ["lsp-darwin-arm64", "verter-lsp", "packages/verter-lsp/npm/darwin-arm64/verter-lsp", true],
      [
        "lsp-win32-x64-msvc",
        "verter-lsp.exe",
        "packages/verter-lsp/npm/win32-x64-msvc/verter-lsp.exe",
        true,
      ],
      ["mcp-darwin-arm64", "verter-mcp", "packages/verter-mcp/npm/darwin-arm64/verter-mcp", true],
      ["tsc-darwin-arm64", "verter-tsc", "packages/verter-tsc/npm/darwin-arm64/verter-tsc", true],
    ],
  );
});

test("planPlatformStaging reports a missing artifact, a missing file, an ambiguous file and an unknown family — and never plans those copies", () => {
  const artifacts = completeArtifacts();
  delete artifacts["mcp-darwin-arm64"]; // whole artifact missing
  artifacts["tsc-darwin-arm64"] = ["README.md"]; // artifact present, binary absent
  artifacts["native-extra"] = ["verter-native.darwin-arm64.node"]; // duplicate native file
  const packages = [
    ...PLATFORM_PACKAGES,
    { dir: "packages/unknown-family/npm/darwin-arm64", files: ["thing"] },
    { dir: "packages/verter-lsp/npm/linux-x64-gnu", files: [] },
  ];

  const { copies, problems } = planPlatformStaging(packages, artifacts);

  const destinations = copies.map((c) => c.destination);
  assert.ok(!destinations.some((d) => d.startsWith("packages/verter-mcp/")));
  assert.ok(!destinations.some((d) => d.startsWith("packages/verter-tsc/")));
  assert.ok(
    !destinations.includes("packages/native/npm/darwin-arm64/verter-native.darwin-arm64.node"),
  );
  assert.ok(
    destinations.includes("packages/native/npm/win32-x64-msvc/verter-native.win32-x64-msvc.node"),
  );

  assert.equal(problems.length, 5, problems.join("\n"));
  assert.match(
    problems.find((p) => p.includes("verter-mcp")),
    /artifact "mcp-darwin-arm64" was not downloaded/,
  );
  assert.match(
    problems.find((p) => p.includes("verter-tsc")),
    /has no "verter-tsc"/,
  );
  assert.match(
    problems.find((p) => p.includes("native/npm/darwin-arm64")),
    /ambiguous across artifacts/,
  );
  assert.match(
    problems.find((p) => p.includes("unknown-family")),
    /no binary family/,
  );
  assert.match(
    problems.find((p) => p.includes("linux-x64-gnu")),
    /declares no "files"/,
  );
});

test("planCoreStaging copies the whole wasm artifact plus the napi loader, and names each missing prerequisite", () => {
  const ok = planCoreStaging(completeArtifacts());
  assert.deepEqual(ok.problems, []);
  assert.deepEqual(
    ok.copies.map((c) => c.destination),
    [
      "packages/wasm/wasm/verter_wasm_bg.wasm",
      "packages/wasm/wasm/verter_wasm.js",
      "packages/wasm/dist/index.mjs",
      "packages/native/dist/index.js",
    ],
  );

  const noWasm = planCoreStaging({ "native-loader": ["index.js"] });
  assert.deepEqual(noWasm.problems, ['artifact "wasm" was not downloaded']);

  const partialWasm = planCoreStaging({ wasm: ["wasm/verter_wasm.js"], "native-loader": [] });
  assert.deepEqual(partialWasm.problems, [
    'artifact "wasm" has no "wasm/verter_wasm_bg.wasm"',
    'artifact "native-loader" has no "index.js"',
  ]);
});

// ---------------------------------------------------------------------------
// Outcome classification and arguments
// ---------------------------------------------------------------------------

test("classifyNpmPublishOutcome distinguishes the five outcomes, OTP first", () => {
  assert.equal(classifyNpmPublishOutcome(0, "anything"), "published");
  assert.equal(
    classifyNpmPublishOutcome(
      1,
      "npm error code EOTP\nnpm error This operation requires a one-time password",
    ),
    "otp-required",
  );
  assert.equal(
    classifyNpmPublishOutcome(
      1,
      "npm error code E403\ncannot publish over the previously published versions",
    ),
    "already-published",
  );
  assert.equal(classifyNpmPublishOutcome(1, "EPUBLISHCONFLICT"), "already-published");
  assert.equal(classifyNpmPublishOutcome(1, "sigstore: TLOG_CREATE_ENTRY_ERROR"), "tlog-conflict");
  assert.equal(classifyNpmPublishOutcome(1, "npm error code E401 Unauthorized"), "failed");
  // An expired OTP surfaces alongside a 403-ish message; OTP wins so the loop re-prompts.
  assert.equal(classifyNpmPublishOutcome(1, "E403 … one-time password …"), "otp-required");
});

test("classifyCargoPublishOutcome tolerates only an already-uploaded version", () => {
  assert.equal(classifyCargoPublishOutcome(0, ""), "published");
  assert.equal(
    classifyCargoPublishOutcome(101, "crate version `0.0.1-beta.5` is already uploaded"),
    "already-published",
  );
  assert.equal(
    classifyCargoPublishOutcome(101, "error: crate verter_span@0.0.1-beta.5 already exists"),
    "already-published",
  );
  assert.equal(
    classifyCargoPublishOutcome(101, "error: failed to verify package tarball"),
    "failed",
  );
});

test("npmPublishArgs passes the dist-tag only for a pre-release channel and adds each optional flag exactly once", () => {
  assert.deepEqual(npmPublishArgs("a.tgz", { distTag: "beta" }), [
    "publish",
    "a.tgz",
    "--access",
    "public",
    "--tag",
    "beta",
  ]);
  assert.deepEqual(npmPublishArgs("a.tgz", { distTag: "latest" }), [
    "publish",
    "a.tgz",
    "--access",
    "public",
  ]);
  assert.deepEqual(
    npmPublishArgs("a.tgz", { distTag: "rc", provenance: true, otp: "123456", dryRun: true }),
    [
      "publish",
      "a.tgz",
      "--access",
      "public",
      "--tag",
      "rc",
      "--provenance",
      "--otp",
      "123456",
      "--dry-run",
    ],
  );
});

test("distTagForVersion maps the pre-release channel to the dist-tag and stable to latest", () => {
  assert.equal(distTagForVersion("0.0.1-alpha.3"), "alpha");
  assert.equal(distTagForVersion("0.0.1-beta.5"), "beta");
  assert.equal(distTagForVersion("1.0.0-rc.1"), "rc");
  assert.equal(distTagForVersion("1.0.0"), "latest");
});

// ---------------------------------------------------------------------------
// Tarball executable bits
// ---------------------------------------------------------------------------

const BLOCK = 512;

function tarHeader({ name, size, mode = 0o644, typeflag = "0", prefix = "" }) {
  const header = Buffer.alloc(BLOCK);
  header.write(name, 0, 100, "utf8");
  header.write(`${mode.toString(8).padStart(7, "0")}\0`, 100, 8, "latin1");
  header.write("0000000\0", 108, 8, "latin1");
  header.write("0000000\0", 116, 8, "latin1");
  header.write(`${size.toString(8).padStart(11, "0")}\0`, 124, 12, "latin1");
  header.write("00000000000\0", 136, 12, "latin1");
  header.write(typeflag, 156, 1, "latin1");
  header.write("ustar\0", 257, 6, "latin1");
  header.write("00", 263, 2, "latin1");
  if (prefix) header.write(prefix, 345, 155, "utf8");
  header.fill(0x20, 148, 156);
  let sum = 0;
  for (const b of header) sum += b;
  header.write(`${sum.toString(8).padStart(6, "0")}\0 `, 148, 8, "latin1");
  return header;
}

function tarEntry(fields, data) {
  const body = Buffer.from(data);
  const padded = Buffer.alloc(Math.ceil(body.length / BLOCK) * BLOCK);
  body.copy(padded);
  return Buffer.concat([tarHeader({ ...fields, size: body.length }), padded]);
}

function paxRecord(key, value) {
  // "<len> key=value\n" where len counts the whole record including itself.
  let len = `${key}=${value}\n`.length + 1;
  len += String(len).length;
  return `${len} ${key}=${value}\n`;
}

function buildArchive(entries) {
  return Buffer.concat([...entries, Buffer.alloc(BLOCK * 2)]);
}

function storedChecksum(header) {
  return parseInt(
    header
      .subarray(148, 156)
      .toString("latin1")
      .replace(/[\0 ]+$/, ""),
    8,
  );
}

function computedChecksum(header) {
  const copy = Buffer.from(header);
  copy.fill(0x20, 148, 156);
  let sum = 0;
  for (const b of copy) sum += b;
  return sum;
}

test("markTarEntriesExecutable sets 0755 on exactly the named binaries, rewrites their checksums and leaves every other byte alone", () => {
  const pax = paxRecord("path", "package/deeply/nested/verter-relay-shim");
  const archive = buildArchive([
    tarEntry({ name: "package/package.json" }, '{"name":"@verter/lsp-linux-x64-gnu"}'),
    tarEntry({ name: "package/verter-lsp" }, "ELF..."),
    tarEntry({ name: "package/LICENSE" }, "MIT"),
    // A pax extended header carrying the real path of the entry that follows.
    tarEntry({ name: "PaxHeader/shim", typeflag: "x" }, pax),
    tarEntry({ name: "package/verter-relay-shim" }, "ELF-shim"),
    tarEntry({ name: "package/docs", typeflag: "5" }, ""),
  ]);

  const before = listTarModes(archive);
  assert.equal(before.get("package/verter-lsp"), 0o644);

  const { bytes, patched } = markTarEntriesExecutable(archive, [
    "verter-lsp",
    "deeply/nested/verter-relay-shim",
    "not-in-archive",
  ]);
  assert.deepEqual(patched, ["verter-lsp", "deeply/nested/verter-relay-shim"]);
  assert.equal(bytes.length, archive.length);

  const after = listTarModes(bytes);
  assert.equal(after.get("package/verter-lsp"), 0o755);
  assert.equal(after.get("package/deeply/nested/verter-relay-shim"), 0o755);
  assert.equal(after.get("package/package.json"), 0o644);
  assert.equal(after.get("package/LICENSE"), 0o644);

  // Every header in the rewritten archive carries a valid checksum.
  let offset = 0;
  while (offset + BLOCK <= bytes.length) {
    const header = bytes.subarray(offset, offset + BLOCK);
    if (header.every((b) => b === 0)) break;
    assert.equal(storedChecksum(header), computedChecksum(header), `checksum at ${offset}`);
    const size =
      parseInt(header.subarray(124, 136).toString("latin1").replace(/\0.*$/, ""), 8) || 0;
    offset += BLOCK + Math.ceil(size / BLOCK) * BLOCK;
  }

  // Untouched entries are byte-identical: only the two patched headers differ.
  let differingBlocks = 0;
  for (let i = 0; i < archive.length; i += BLOCK) {
    if (!archive.subarray(i, i + BLOCK).equals(bytes.subarray(i, i + BLOCK))) differingBlocks += 1;
  }
  assert.equal(differingBlocks, 2);

  // The original is not mutated.
  assert.equal(listTarModes(archive).get("package/verter-lsp"), 0o644);
});

test("markTarballEntriesExecutable round-trips through gzip", () => {
  const archive = buildArchive([tarEntry({ name: "package/verter-mcp" }, "ELF")]);
  const { bytes, patched } = markTarballEntriesExecutable(gzipSync(archive), ["verter-mcp"]);
  assert.deepEqual(patched, ["verter-mcp"]);
  assert.equal(listTarModes(gunzipSync(bytes)).get("package/verter-mcp"), 0o755);
});

// ---------------------------------------------------------------------------
// The publish loop
// ---------------------------------------------------------------------------

function scripted(responses) {
  // responses: label → array of {exitCode, output} consumed in order.
  const calls = [];
  const remaining = new Map(Object.entries(responses).map(([k, v]) => [k, [...v]]));
  const publish = async (target, opts) => {
    calls.push({ label: target.label, otp: opts.otp, provenance: opts.provenance });
    const queue = remaining.get(target.label);
    assert.ok(queue && queue.length > 0, `unexpected publish attempt for ${target.label}`);
    return queue.shift();
  };
  return { publish, calls };
}

const OK = { exitCode: 0, output: "" };
const EOTP = { exitCode: 1, output: "npm error code EOTP" };
const E403 = { exitCode: 1, output: "npm error code E403 cannot publish over" };
const TLOG = { exitCode: 1, output: "TLOG_CREATE_ENTRY_ERROR" };
const E401 = { exitCode: 1, output: "npm error code E401" };

test("publishSequentially shares one OTP across a batch and asks for a fresh one exactly when the registry rejects it, retrying the same target", async () => {
  const { publish, calls } = scripted({ a: [OK], b: [EOTP, OK], c: [E403], d: [OK] });
  const prompts = [];
  const codes = ["222222"];
  const summary = await publishSequentially(
    [{ label: "a" }, { label: "b" }, { label: "c" }, { label: "d" }],
    {
      publish,
      initialOtp: "111111",
      promptOtp: async (reason) => {
        prompts.push(reason);
        return codes.shift();
      },
    },
  );

  assert.deepEqual(summary.published, ["a", "b", "d"]);
  assert.deepEqual(summary.skipped, ["c"]);
  assert.deepEqual(summary.failed, []);
  assert.equal(summary.otpPrompts, 1);
  assert.deepEqual(prompts, ["b: the one-time password was rejected (expired?)"]);
  assert.deepEqual(
    calls.map((c) => [c.label, c.otp]),
    [
      ["a", "111111"],
      ["b", "111111"],
      ["b", "222222"],
      ["c", "222222"],
      ["d", "222222"],
    ],
  );
});

test("publishSequentially retries a provenance-log conflict once without provenance", async () => {
  const { publish, calls } = scripted({ a: [TLOG, OK], b: [TLOG, TLOG] });
  const summary = await publishSequentially([{ label: "a" }, { label: "b" }], {
    publish,
    provenance: true,
  });
  assert.deepEqual(summary.published, ["a"]);
  assert.equal(summary.failed.length, 1);
  assert.equal(summary.failed[0].label, "b");
  assert.deepEqual(
    calls.map((c) => [c.label, c.provenance]),
    [
      ["a", true],
      ["a", false],
      ["b", true],
      ["b", false],
    ],
  );
});

test("publishSequentially fails a target when no OTP can be obtained or the code keeps being rejected, and still continues", async () => {
  const rejections = Array.from({ length: MAX_OTP_RETRIES_PER_TARGET + 1 }, () => EOTP);
  const { publish } = scripted({ a: [EOTP], b: rejections, c: [E401], d: [OK] });
  let prompted = 0;
  const summary = await publishSequentially(
    [{ label: "a" }, { label: "b" }, { label: "c" }, { label: "d" }],
    {
      publish,
      promptOtp: async () => {
        prompted += 1;
        // a: refuse; b: keep handing out a code that the registry keeps rejecting.
        return prompted === 1 ? null : "999999";
      },
    },
  );

  assert.deepEqual(summary.published, ["d"]);
  assert.deepEqual(
    summary.failed.map((f) => f.label),
    ["a", "b", "c"],
  );
  assert.match(summary.failed[0].output, /none was provided/);
  assert.match(summary.failed[1].output, /rejected 4 times/);
  assert.match(summary.failed[2].output, /E401/);
});

// ---------------------------------------------------------------------------
// Entry-point guard
// ---------------------------------------------------------------------------

test("invokedAsEntrypoint recognises the script through a symlinked argv path", () => {
  const modulePath = resolve(process.cwd(), "scripts", "release-publish.mjs");
  const moduleUrl = pathToFileURL(modulePath).href;
  const linkPath = resolve(process.cwd(), "bin", "release-publish");
  const realpath = (candidate) => (candidate === linkPath ? modulePath : candidate);

  assert.equal(invokedAsEntrypoint(linkPath, moduleUrl, { realpath }), true);
  assert.equal(invokedAsEntrypoint(modulePath, moduleUrl, { realpath }), true);
  assert.equal(
    invokedAsEntrypoint(resolve(process.cwd(), "other.mjs"), moduleUrl, { realpath }),
    false,
  );
  assert.equal(invokedAsEntrypoint(undefined, moduleUrl, { realpath }), false);
  // A path the filesystem cannot resolve falls back to plain path resolution.
  const throwing = () => {
    throw new Error("ENOENT");
  };
  assert.equal(invokedAsEntrypoint(modulePath, moduleUrl, { realpath: throwing }), true);
  assert.equal(invokedAsEntrypoint(linkPath, moduleUrl, { realpath: throwing }), false);
});

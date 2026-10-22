#!/usr/bin/env node

/**
 * typescript-pin.mjs
 *
 * One TypeScript pin, every spelling checked.
 *
 * The workspace `package.json` `devDependencies.typescript` is the authority:
 * it is what `pnpm install` provisions. Every other place that spells the
 * pinned version — the Rust `typescript_pin!` macro behind `TYPESCRIPT_PIN`,
 * the oracle harness pin, the oracle toolchain record, nested fixture
 * manifests, the products that record the engine they run — is listed in
 * `PIN_SITES` below and must agree with it.
 *
 * Usage:
 *   node scripts/typescript-pin.mjs --check
 *       Exit 1 when any pin site disagrees with the authority.
 *   node scripts/typescript-pin.mjs --set <version> [--allow-line-change]
 *       Rewrite every writable pin site to <version> and print the follow-up
 *       steps (install, regenerate the derived records). Moving to another
 *       minor (a new release line) also moves the Rust line/ceiling macros and
 *       is refused without --allow-line-change.
 *   node scripts/typescript-pin.mjs --write-toolchain-record
 *       Regenerate the oracle toolchain record for the pinned version: every
 *       platform package it lists is fetched with `npm pack` into a temp
 *       directory and hashed (the binaries are never executed).
 *   node scripts/typescript-pin.mjs --relock-evidence-manifest
 *       Re-lock the signature-kernel evidence manifest's derived digests (the
 *       contract bytes and the oracle section, from the toolchain record).
 */

import { execFileSync, execSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { gunzipSync } from "node:zlib";
import { pathToFileURL } from "node:url";

export const ROOT = resolve(import.meta.dirname, "..");

const POLICY_RS = "crates/verter_tsgo_api/src/toolchain/policy.rs";
const TOOLCHAIN_RECORD = "crates/verter_session/src/typeinfo/typeinfo_tests/oracle_toolchain.json";
const NVIM_FIXTURE = "editors/nvim/tests/fixtures/real-client";
const EVIDENCE_MANIFEST = "docs/evidence/signature-kernel/manifest.json";
const SIGNATURE_CONTRACT = "docs/arch/signature-kernel.md";

/** The authority every other site is checked against. */
export const AUTHORITY = { path: "package.json", json: "devDependencies.typescript" };

/**
 * Every place that spells the pinned TypeScript version.
 *
 * - `json`: a selector into a JSON file. Dotted keys; `*` matches every key
 *   of an object; `[field=value]` picks the array element whose `field` is
 *   `value`.
 * - `regex`: a pattern with exactly one capture group, the version.
 * - `format`: how the site spells the version (default: the version itself).
 * - `regenerate`: the site is a derived record that `--set` must not rewrite
 *   by hand (it carries digests or resolved metadata); the string is the
 *   command that regenerates it.
 */
export const PIN_SITES = [
  { path: "docs/package.json", json: "devDependencies.typescript" },
  { path: "packages/vue-conformance-oracle/package.json", json: "devDependencies.typescript" },
  {
    path: POLICY_RS,
    regex: /macro_rules! typescript_pin \{\s*\(\) => \{\s*"([^"]+)"/,
  },
  {
    path: "crates/verter_session/src/typeinfo/oracle_core/identity.rs",
    regex: /pub\(crate\) const TSGO_VERSION: &str = "([^"]+)";/,
  },
  {
    path: TOOLCHAIN_RECORD,
    json: "typescript",
    regenerate: "node scripts/typescript-pin.mjs --write-toolchain-record",
  },
  {
    path: TOOLCHAIN_RECORD,
    json: "engine_version_report",
    format: (v) => `Version ${v}`,
    regenerate: "node scripts/typescript-pin.mjs --write-toolchain-record",
  },
  {
    path: TOOLCHAIN_RECORD,
    json: "platforms.*.version",
    regenerate: "node scripts/typescript-pin.mjs --write-toolchain-record",
  },
  { path: `${NVIM_FIXTURE}/package.json`, json: "dependencies.typescript" },
  {
    path: `${NVIM_FIXTURE}/package-lock.json`,
    json: "packages.node_modules/typescript.version",
    regenerate: `npm install --package-lock-only --ignore-scripts --prefix ${NVIM_FIXTURE}`,
  },
  { path: "examples/reference/manifest.json", json: "pin.typescript" },
  {
    path: "tests/sfc-projection/STP1/products/engine-matrix.json",
    json: "engines[id=ts-native].version",
  },
  {
    path: "tests/architecture-health/ARH0/products/capability-matrix.json",
    json: "rows[capability=typescript-tsgo-plane].version",
  },
  {
    path: EVIDENCE_MANIFEST,
    json: "oracle.typescript",
    regenerate: "node scripts/typescript-pin.mjs --relock-evidence-manifest",
  },
  {
    path: EVIDENCE_MANIFEST,
    json: "oracle.engine_version_report",
    format: (v) => `Version ${v}`,
    regenerate: "node scripts/typescript-pin.mjs --relock-evidence-manifest",
  },
];

/** The Rust release-line macros `--set` moves only on a line change. */
const LINE_MACROS = {
  line: /macro_rules! typescript_line \{\s*\(\) => \{\s*"([^"]+)"/,
  ceiling: /macro_rules! typescript_line_ceiling \{\s*\(\) => \{\s*"([^"]+)"/,
};

// ---------------------------------------------------------------------------
// Versions
// ---------------------------------------------------------------------------

const SEMVER =
  /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-((?:0|[1-9]\d*|\d*[A-Za-z-][0-9A-Za-z-]*)(?:\.(?:0|[1-9]\d*|\d*[A-Za-z-][0-9A-Za-z-]*))*))?$/;

/** Parse a strict SemVer version without build metadata. */
export function parseVersion(text) {
  const m = SEMVER.exec(text);
  if (!m) throw new Error(`\`${text}\` is not a strict SemVer version without build metadata`);
  return {
    major: Number(m[1]),
    minor: Number(m[2]),
    patch: Number(m[3]),
    prerelease: m[4] ?? null,
  };
}

// ---------------------------------------------------------------------------
// JSON string locations
// ---------------------------------------------------------------------------

/**
 * Every string VALUE in a JSON document with its key path and the offsets of
 * its literal (quotes included), so a value can be replaced without
 * re-serialising the file.
 */
export function jsonStringValues(text) {
  const out = [];
  let i = 0;
  const ws = () => {
    while (i < text.length && /\s/.test(text[i])) i++;
  };
  const str = () => {
    const start = i;
    i++;
    while (text[i] !== '"') i += text[i] === "\\" ? 2 : 1;
    i++;
    return { start, end: i, value: JSON.parse(text.slice(start, i)) };
  };
  const value = (path) => {
    ws();
    const c = text[i];
    if (c === "{") {
      i++;
      ws();
      if (text[i] === "}") return i++;
      for (;;) {
        ws();
        const key = str().value;
        ws();
        i++; // ':'
        value([...path, key]);
        ws();
        if (text[i++] === "}") return;
      }
    }
    if (c === "[") {
      i++;
      ws();
      if (text[i] === "]") return i++;
      for (let index = 0; ; index++) {
        value([...path, index]);
        ws();
        if (text[i++] === "]") return;
      }
    }
    if (c === '"') {
      out.push({ path, ...str() });
      return;
    }
    while (i < text.length && !/[\s,\]}]/.test(text[i])) i++;
  };
  value([]);
  return out;
}

function selectorParts(selector) {
  const parts = [];
  for (const raw of selector.split(/\.(?![^[]*\])/)) {
    const m = /^([^[]*)\[([^=\]]+)=([^\]]+)\]$/.exec(raw);
    if (m) {
      if (m[1]) parts.push({ key: m[1] });
      parts.push({ where: [m[2], m[3]] });
    } else parts.push({ key: raw });
  }
  return parts;
}

/** The concrete key paths a selector names in `doc`. */
function resolveSelector(doc, selector) {
  let frontier = [{ node: doc, path: [] }];
  for (const part of selectorParts(selector)) {
    const next = [];
    for (const { node, path } of frontier) {
      if (node === null || typeof node !== "object") continue;
      if (part.where) {
        if (!Array.isArray(node)) continue;
        node.forEach((element, index) => {
          if (element && String(element[part.where[0]]) === part.where[1]) {
            next.push({ node: element, path: [...path, index] });
          }
        });
      } else if (part.key === "*") {
        for (const key of Object.keys(node)) next.push({ node: node[key], path: [...path, key] });
      } else if (Object.hasOwn(node, part.key)) {
        next.push({ node: node[part.key], path: [...path, part.key] });
      }
    }
    frontier = next;
  }
  return frontier.map((f) => f.path);
}

// ---------------------------------------------------------------------------
// Pin sites
// ---------------------------------------------------------------------------

/**
 * The spans of every version occurrence a site names:
 * `[{ start, end, found }]`, where `[start, end)` covers the version as the
 * file spells it (for JSON, the whole string literal). Throws when the site
 * cannot be located.
 */
export function locateSite(root, site) {
  const file = join(root, site.path);
  if (!existsSync(file)) throw new Error(`${site.path}: file not found`);
  const text = readFileSync(file, "utf8");
  if (site.regex) {
    const m = site.regex.exec(text);
    if (!m) throw new Error(`${site.path}: pattern ${site.regex} not found`);
    const start = m.index + m[0].indexOf(m[1]);
    return { text, spans: [{ start, end: start + m[1].length, found: m[1], literal: false }] };
  }
  const paths = resolveSelector(JSON.parse(text), site.json);
  if (paths.length === 0)
    throw new Error(`${site.path}: selector \`${site.json}\` matches nothing`);
  const values = jsonStringValues(text);
  const spans = paths.map((path) => {
    const hit = values.find((v) => JSON.stringify(v.path) === JSON.stringify(path));
    if (!hit) throw new Error(`${site.path}: \`${path.join(".")}\` is not a string`);
    return { start: hit.start, end: hit.end, found: hit.value, literal: true };
  });
  return { text, spans };
}

/** The authority's pinned version. */
export function readPin(root = ROOT) {
  const doc = JSON.parse(readFileSync(join(root, AUTHORITY.path), "utf8"));
  const pin = AUTHORITY.json.split(".").reduce((node, key) => node?.[key], doc);
  if (typeof pin !== "string") throw new Error(`${AUTHORITY.path}: no ${AUTHORITY.json}`);
  parseVersion(pin);
  return pin;
}

/** Every pin site that disagrees with the authority (or cannot be located). */
export function checkPins(root = ROOT) {
  const pin = readPin(root);
  const problems = [];
  for (const site of PIN_SITES) {
    const where = `${site.path} (${site.json ?? "pattern"})`;
    let located;
    try {
      located = locateSite(root, site);
    } catch (error) {
      problems.push({ site: where, problem: error.message });
      continue;
    }
    const expected = site.format ? site.format(pin) : pin;
    const wrong = [...new Set(located.spans.map((s) => s.found).filter((f) => f !== expected))];
    if (wrong.length > 0) {
      problems.push({
        site: where,
        problem: `spells ${wrong.map((w) => `\`${w}\``).join(", ")}, expected \`${expected}\``,
        regenerate: site.regenerate,
      });
    }
  }
  return { pin, problems };
}

function rewrite(text, spans, value) {
  let out = text;
  for (const span of [...spans].sort((a, b) => b.start - a.start)) {
    const spelled = span.literal ? JSON.stringify(value) : value;
    out = out.slice(0, span.start) + spelled + out.slice(span.end);
  }
  return out;
}

/**
 * Move the pin: rewrite the authority and every writable site. Returns the
 * changed paths and the follow-up steps. Refuses a release-line change unless
 * `allowLineChange`.
 */
export function setPin(root, version, { allowLineChange = false } = {}) {
  const next = parseVersion(version);
  const policyPath = join(root, POLICY_RS);
  let policy = readFileSync(policyPath, "utf8");
  const line = LINE_MACROS.line.exec(policy)?.[1];
  if (!line) throw new Error(`${POLICY_RS}: typescript_line! macro not found`);
  const nextLine = `${next.major}.${next.minor}`;
  const lineChange = line !== nextLine;
  if (lineChange && !allowLineChange) {
    throw new Error(
      `${version} is on release line ${nextLine}, not the supported ${line}; a new line needs ` +
        `explicit support (re-verify the --api codec) — pass --allow-line-change to move it`,
    );
  }

  const changed = new Set();
  const edits = new Map();
  const fileText = (path) => edits.get(path) ?? readFileSync(join(root, path), "utf8");

  const authority = { path: AUTHORITY.path, json: AUTHORITY.json };
  for (const site of [authority, ...PIN_SITES]) {
    if (site.regenerate) continue;
    const located = locateSite(root, site);
    // Re-locate against pending edits of the same file.
    const text = fileText(site.path);
    const spans = text === located.text ? located.spans : locateInText(text, site);
    const value = site.format ? site.format(version) : version;
    const out = rewrite(text, spans, value);
    if (out !== text) {
      edits.set(site.path, out);
      changed.add(site.path);
    }
  }
  if (lineChange) {
    policy = edits.get(POLICY_RS) ?? policy;
    policy = policy
      .replace(LINE_MACROS.line, (m, old) => m.replace(`"${old}"`, `"${nextLine}"`))
      .replace(LINE_MACROS.ceiling, (m, old) =>
        m.replace(`"${old}"`, `"${next.major}.${next.minor + 1}.0"`),
      );
    edits.set(POLICY_RS, policy);
    changed.add(POLICY_RS);
  }
  for (const [path, text] of edits) writeFileSync(join(root, path), text);

  const regenerate = [...new Set(PIN_SITES.filter((s) => s.regenerate).map((s) => s.regenerate))];
  const followUp = [
    "pnpm install",
    ...regenerate,
    "pnpm gen:oracle-snapshots",
    "node scripts/typescript-pin.mjs --check",
  ];
  return { changed: [...changed], followUp, lineChange };
}

function locateInText(text, site) {
  if (site.regex) {
    const m = site.regex.exec(text);
    const start = m.index + m[0].indexOf(m[1]);
    return [{ start, end: start + m[1].length, found: m[1], literal: false }];
  }
  const values = jsonStringValues(text);
  return resolveSelector(JSON.parse(text), site.json).map((path) => {
    const hit = values.find((v) => JSON.stringify(v.path) === JSON.stringify(path));
    return { start: hit.start, end: hit.end, found: hit.value, literal: true };
  });
}

// ---------------------------------------------------------------------------
// Oracle toolchain record
// ---------------------------------------------------------------------------

/**
 * Unpack an npm tarball's regular files under `dest`, dropping the leading
 * `package/` directory (ustar names with prefix, and pax `path` records).
 * The files are only written, never executed.
 */
export function extractTarball(tgz, dest) {
  const tar = gunzipSync(tgz);
  const field = (block, at, len) => {
    const raw = block.toString("latin1", at, at + len);
    const nul = raw.indexOf("\0");
    return nul < 0 ? raw : raw.slice(0, nul);
  };
  let paxPath = null;
  for (let at = 0; at + 512 <= tar.length; ) {
    const header = tar.subarray(at, at + 512);
    if (header.every((b) => b === 0)) break;
    const size = parseInt(field(header, 124, 12).trim() || "0", 8);
    const type = header[156] === 0 ? "0" : String.fromCharCode(header[156]);
    const body = tar.subarray(at + 512, at + 512 + size);
    at += 512 + Math.ceil(size / 512) * 512;
    if (type === "x") {
      const m = /\d+ path=([^\n]*)\n/.exec(body.toString("utf8"));
      paxPath = m ? m[1] : null;
      continue;
    }
    const prefix = field(header, 345, 155);
    const base = field(header, 0, 100);
    const name = paxPath ?? (prefix ? `${prefix}/${base}` : base);
    paxPath = null;
    if (type !== "0") continue;
    const parts = name.split("/").slice(1);
    if (parts.length === 0 || parts.some((part) => part === ".." || part === "")) {
      throw new Error(`unsafe tarball entry \`${name}\``);
    }
    const out = join(dest, ...parts);
    mkdirSync(dirname(out), { recursive: true });
    writeFileSync(out, body);
  }
}

function sha256(bytes) {
  return `sha256:${createHash("sha256").update(bytes).digest("hex")}`;
}

/**
 * The toolchain record for `version`: for every platform key, the platform
 * package is fetched with `npm pack` (never executed) and its engine binary
 * and every `lib/*.d.ts` are hashed.
 */
export function buildToolchainRecord(version, platformKeys) {
  parseVersion(version);
  for (const key of platformKeys) {
    if (!/^[a-z0-9]+-[a-z0-9]+$/.test(key)) throw new Error(`bad platform key "${key}"`);
  }
  const work = mkdtempSync(join(tmpdir(), "verter-ts-record-"));
  try {
    const platforms = {};
    for (const key of [...platformKeys].sort()) {
      const pkg = `@typescript/typescript-${key}`;
      const dir = join(work, key);
      // `version` is strict SemVer and `key` a platform token, so the spec
      // needs no shell escaping; the destination is quoted.
      execSync(`npm pack ${pkg}@${version} --pack-destination "${work}" --silent`, {
        cwd: work,
        stdio: ["ignore", "pipe", "inherit"],
      });
      const tarball = readdirSync(work).find((f) => f.startsWith(`typescript-typescript-${key}-`));
      if (!tarball) throw new Error(`npm pack ${pkg}@${version} produced no tarball`);
      extractTarball(readFileSync(join(work, tarball)), dir);
      rmSync(join(work, tarball));
      const manifest = JSON.parse(readFileSync(join(dir, "package.json"), "utf8"));
      if (manifest.name !== pkg || manifest.version !== version) {
        throw new Error(
          `${tarball} is ${manifest.name}@${manifest.version}, not ${pkg}@${version}`,
        );
      }
      const executable = key.startsWith("win32-") ? "lib/tsc.exe" : "lib/tsc";
      const libs = readdirSync(join(dir, "lib"))
        .filter((f) => f.endsWith(".d.ts"))
        .sort()
        .map((f) => ({ path: `lib/${f}`, sha256: sha256(readFileSync(join(dir, "lib", f))) }));
      platforms[key] = {
        package: pkg,
        version,
        executable,
        executable_sha256: sha256(readFileSync(join(dir, executable))),
        lib_d_ts: libs,
      };
    }
    return { typescript: version, engine_version_report: `Version ${version}`, platforms };
  } finally {
    rmSync(work, { recursive: true, force: true });
  }
}

/**
 * Re-lock the signature-kernel evidence manifest's derived digests: the
 * contract bytes, and the `oracle` section from the toolchain record (its
 * version, its digest, its platforms and lib inventory size). The manifest's
 * records of past measurements (corpus identity, port witness) are not
 * touched — they say what was measured, and only a re-measurement moves them.
 */
export function relockEvidenceManifest(root = ROOT) {
  const hex = (bytes) => createHash("sha256").update(bytes).digest("hex");
  const path = join(root, EVIDENCE_MANIFEST);
  const manifest = JSON.parse(readFileSync(path, "utf8"));
  const recordBytes = readFileSync(join(root, TOOLCHAIN_RECORD));
  const record = JSON.parse(recordBytes.toString("utf8"));
  const libCounts = new Set(Object.values(record.platforms).map((p) => p.lib_d_ts.length));
  if (libCounts.size !== 1)
    throw new Error(`${TOOLCHAIN_RECORD}: platforms disagree on the lib inventory size`);
  manifest.contract.sha256 = hex(readFileSync(join(root, SIGNATURE_CONTRACT)));
  manifest.oracle.typescript = record.typescript;
  manifest.oracle.engine_version_report = record.engine_version_report;
  manifest.oracle.toolchain_record.sha256 = hex(recordBytes);
  manifest.oracle.toolchain_record.platforms = Object.keys(record.platforms);
  manifest.oracle.toolchain_record.per_platform = `executable + ${[...libCounts][0]} lib/*.d.ts sha256 digests (see the record)`;
  writeFileSync(path, `${JSON.stringify(manifest, null, 2)}\n`);
  return path;
}

export function writeToolchainRecord(root = ROOT) {
  const pin = readPin(root);
  const path = join(root, TOOLCHAIN_RECORD);
  const keys = Object.keys(JSON.parse(readFileSync(path, "utf8")).platforms);
  writeFileSync(path, `${JSON.stringify(buildToolchainRecord(pin, keys), null, 2)}\n`);
  return path;
}

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

function main(argv) {
  if (argv.includes("--set")) {
    const version = argv[argv.indexOf("--set") + 1];
    if (!version)
      throw new Error("Usage: typescript-pin.mjs --set <version> [--allow-line-change]");
    const { changed, followUp, lineChange } = setPin(ROOT, version, {
      allowLineChange: argv.includes("--allow-line-change"),
    });
    console.log(`TypeScript pin -> ${version}${lineChange ? " (new release line)" : ""}`);
    for (const path of changed) console.log(`  rewrote ${path}`);
    console.log("Then run, in order:");
    for (const step of followUp) console.log(`  ${step}`);
    return 0;
  }
  if (argv.includes("--write-toolchain-record")) {
    console.log(`wrote ${writeToolchainRecord(ROOT)}`);
    return 0;
  }
  if (argv.includes("--relock-evidence-manifest")) {
    console.log(`wrote ${relockEvidenceManifest(ROOT)}`);
    return 0;
  }
  if (argv.includes("--check")) {
    const { pin, problems } = checkPins(ROOT);
    for (const p of problems) {
      console.error(
        `typescript-pin: ${p.site}: ${p.problem}${p.regenerate ? ` (regenerate: ${p.regenerate})` : ""}`,
      );
    }
    if (problems.length === 0) console.log(`typescript-pin: every pin site spells ${pin}`);
    return problems.length ? 1 : 0;
  }
  console.error(
    "Usage: typescript-pin.mjs --check | --set <version> [--allow-line-change] | " +
      "--write-toolchain-record | --relock-evidence-manifest",
  );
  return 2;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  process.exitCode = main(process.argv.slice(2));
}

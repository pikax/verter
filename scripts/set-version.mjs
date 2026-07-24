#!/usr/bin/env node

/**
 * set-version.mjs — write (or verify) ONE version across the whole release surface.
 *
 * Usage:
 *   node scripts/set-version.mjs <version>          # write the version everywhere
 *   node scripts/set-version.mjs --check <version>  # verify only, write nothing
 *
 * Target set — derived from scripts/lib/publish-set.mjs, the same authority
 * release.yml publishes from, so this cannot drift from what actually ships:
 *   - Cargo.toml              [workspace.package] version (every crate inherits it)
 *   - Cargo.lock              workspace-member entries (packages with no `source =`)
 *   - packages/*              every package in the npm publish set
 *   - packages/<pkg>/npm/*    every platform sub-package in the publish set
 *
 * Private packages are never in the publish set by construction, so they are
 * never touched. Both modes fail loudly — exit 1, naming each offender — if
 * any target does not hold exactly <version>.
 */

import { readFileSync, writeFileSync } from "node:fs";
import { join, relative, resolve } from "node:path";
import { computePublishSet, scanWorkspacePackages } from "./lib/publish-set.mjs";
import { isValidSemver } from "./lib/semver.mjs";

const ROOT = resolve(import.meta.dirname, "..");

// ---------------------------------------------------------------------------
// Arguments
// ---------------------------------------------------------------------------

const args = process.argv.slice(2);
const checkMode = args.includes("--check");
const positional = args.filter((a) => a !== "--check");

if (positional.length !== 1 || !isValidSemver(positional[0])) {
  console.error("usage: node scripts/set-version.mjs [--check] <version>");
  console.error("       <version> must be strict semver, e.g. 0.0.1-beta.2");
  process.exit(2);
}
const version = positional[0];

// ---------------------------------------------------------------------------
// Target enumeration (the publish authority decides what is in the set)
// ---------------------------------------------------------------------------

const publishSet = computePublishSet();
const workspace = scanWorkspacePackages(join(ROOT, "packages"));

/** package.json targets: publish-set packages + platform sub-packages. */
const pkgTargets = [
  ...publishSet.npm.map((name) => {
    const entry = workspace.get(name);
    return { label: name, path: join(entry.dir, "package.json") };
  }),
  ...publishSet.platform.map((dir) => {
    const path = join(ROOT, dir, "package.json");
    const name = JSON.parse(readFileSync(path, "utf8")).name;
    return { label: `${name} (${dir})`, path };
  }),
];

if (pkgTargets.length === 0) {
  console.error("set-version: the publish set is empty — refusing to do nothing");
  process.exit(1);
}

// ---------------------------------------------------------------------------
// Readers / writers
// ---------------------------------------------------------------------------

function readPkgVersion(path) {
  return JSON.parse(readFileSync(path, "utf8")).version;
}

function writePkgVersion(path, next) {
  const text = readFileSync(path, "utf8");
  const m = text.match(/"version"\s*:\s*"[^"]*"/);
  if (!m) throw new Error(`set-version: no "version" field in ${path}`);
  writeFileSync(path, text.replace(m[0], `"version": "${next}"`));
}

const CARGO_TOML = join(ROOT, "Cargo.toml");
const CARGO_LOCK = join(ROOT, "Cargo.lock");

/** Locate the `version = "..."` line inside [workspace.package]. */
function workspaceVersionLine(lines) {
  const start = lines.findIndex((l) => l.trim() === "[workspace.package]");
  if (start === -1) throw new Error("set-version: no [workspace.package] section in Cargo.toml");
  for (let i = start + 1; i < lines.length; i++) {
    if (lines[i].trim().startsWith("[")) break;
    if (/^\s*version\s*=/.test(lines[i])) return i;
  }
  throw new Error("set-version: no version in [workspace.package] in Cargo.toml");
}

function readWorkspaceVersion() {
  const lines = readFileSync(CARGO_TOML, "utf8").split("\n");
  const i = workspaceVersionLine(lines);
  return lines[i].match(/"([^"]+)"/)[1];
}

function writeWorkspaceVersion(next) {
  const lines = readFileSync(CARGO_TOML, "utf8").split("\n");
  const i = workspaceVersionLine(lines);
  lines[i] = lines[i].replace(/"[^"]+"/, `"${next}"`);
  writeFileSync(CARGO_TOML, lines.join("\n"));
}

/**
 * Cargo.lock: workspace members are the [[package]] blocks without a
 * `source =` line. Only blocks still at the old workspace version are
 * rewritten, so a member with its own pinned version is left alone.
 */
function lockBlocks(text) {
  const blocks = text.split("[[package]]");
  return { header: blocks[0], packages: blocks.slice(1) };
}

function readLockVersions(text) {
  // name -> version for workspace-member blocks
  const out = new Map();
  for (const block of lockBlocks(text).packages) {
    if (block.includes("source =")) continue;
    const name = block.match(/name = "([^"]+)"/)[1];
    const ver = block.match(/version = "([^"]+)"/)[1];
    out.set(name, ver);
  }
  return out;
}

function writeLockVersions(text, oldVersion, next) {
  const { header, packages } = lockBlocks(text);
  const rewritten = packages.map((block) => {
    if (block.includes("source =")) return block;
    return block.replace(/version = "([^"]+)"/, (whole, v) =>
      v === oldVersion ? `version = "${next}"` : whole,
    );
  });
  return header + rewritten.map((b) => `[[package]]${b}`).join("");
}

// ---------------------------------------------------------------------------
// Run
// ---------------------------------------------------------------------------

if (!checkMode) {
  const oldVersion = readWorkspaceVersion();
  for (const target of pkgTargets) writePkgVersion(target.path, version);
  writeWorkspaceVersion(version);
  writeFileSync(
    CARGO_LOCK,
    writeLockVersions(readFileSync(CARGO_LOCK, "utf8"), oldVersion, version),
  );
  console.log(`set-version: wrote ${version} (${oldVersion} -> ${version})`);
}

// Verify — in write mode this is the fail-loud guarantee that nothing in the
// set was missed; in --check mode it is the whole point.
const offenders = [];
for (const target of pkgTargets) {
  const actual = readPkgVersion(target.path);
  if (actual !== version) offenders.push(`  ${target.label}: ${actual} (expected ${version})`);
}
const workspaceVersion = readWorkspaceVersion();
if (workspaceVersion !== version) {
  offenders.push(`  Cargo.toml [workspace.package]: ${workspaceVersion} (expected ${version})`);
}
for (const [name, v] of readLockVersions(readFileSync(CARGO_LOCK, "utf8"))) {
  if (v !== version) offenders.push(`  Cargo.lock ${name}: ${v} (expected ${version})`);
}

if (offenders.length > 0) {
  console.error(
    `set-version: ${checkMode ? "check failed" : "incomplete write"} — ${offenders.length} target(s) not at ${version}:`,
  );
  for (const line of offenders) console.error(line);
  process.exit(1);
}

const total = pkgTargets.length + 2;
console.log(
  `set-version: ${checkMode ? "check passed" : "verified"} — all ${total} targets at ${version}`,
);

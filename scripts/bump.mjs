#!/usr/bin/env node

/**
 * bump.mjs — compute the next release version and commit it. Local use only.
 *
 *   pnpm bump                          next version from conventional commits
 *                                      since the last v* tag
 *   pnpm bump -- 0.2.0                 explicit version
 *   pnpm bump -- --prerelease rc       pre-release channel (alpha|beta|rc)
 *   pnpm bump -- --dry-run             print what would happen, change nothing
 *
 * Version source, in order of preference:
 *   1. an explicit positional version,
 *   2. `git-cliff --bumped-version` when git-cliff is installed,
 *   3. a local conventional-commit derivation (feat -> minor, fix/perf ->
 *      patch, BREAKING CHANGE / `!` -> major). A pre-release stays in its
 *      channel and increments its counter (0.0.1-beta.1 -> 0.0.1-beta.2);
 *      a breaking change graduates it (0.0.1-beta.1 -> 1.0.0).
 *
 * The script writes the version across the release surface via
 * scripts/set-version.mjs, requires scripts/check-versions.mjs to pass, and
 * creates exactly ONE commit: `release: v<version>` (the message
 * release-tag.yml recognises on main). It never creates a tag and never
 * pushes — review the commit, push to main, and CI tags the release.
 */

import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { isValidSemver, parseSemver, semverGt } from "./lib/semver.mjs";

const ROOT = resolve(import.meta.dirname, "..");

// ---------------------------------------------------------------------------
// Arguments
// ---------------------------------------------------------------------------

// Tolerate a literal `--` separator so `node scripts/bump.mjs -- ...` behaves
// like `pnpm bump -- ...` (pnpm consumes the separator itself).
const args = process.argv.slice(2).filter((a) => a !== "--");
const dryRun = args.includes("--dry-run");
const preIdx = args.indexOf("--prerelease");
const prereleaseChannel = preIdx === -1 ? null : args[preIdx + 1];
const positional = args.filter(
  (a, i) => a !== "--dry-run" && a !== "--prerelease" && (preIdx === -1 || i !== preIdx + 1),
);

function fail(message) {
  console.error(`bump: ${message}`);
  process.exit(1);
}

if (positional.length > 1) {
  fail(`unexpected arguments: ${positional.slice(1).join(" ")}`);
}
const explicitVersion = positional[0] ?? null;
if (explicitVersion && !isValidSemver(explicitVersion)) {
  fail(`"${explicitVersion}" is not strict semver (e.g. 0.0.1-beta.2)`);
}
if (prereleaseChannel !== null) {
  if (!["alpha", "beta", "rc"].includes(prereleaseChannel)) {
    fail(`--prerelease expects alpha|beta|rc, got "${prereleaseChannel ?? "(nothing)"}"`);
  }
  if (explicitVersion) fail("pass either an explicit version or --prerelease, not both");
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function git(gitArgs) {
  return execFileSync("git", gitArgs, { cwd: ROOT, encoding: "utf8" }).trim();
}

function readWorkspaceVersion() {
  const lines = readFileSync(join(ROOT, "Cargo.toml"), "utf8").split("\n");
  const start = lines.findIndex((l) => l.trim() === "[workspace.package]");
  if (start === -1) fail("no [workspace.package] section in Cargo.toml");
  for (let i = start + 1; i < lines.length; i++) {
    if (lines[i].trim().startsWith("[")) break;
    const m = lines[i].match(/^\s*version\s*=\s*"([^"]+)"/);
    if (m) return m[1];
  }
  fail("no version in [workspace.package] in Cargo.toml");
}

/** Nearest reachable v* tag, or null on a tag-less history. */
function lastTag() {
  try {
    return git(["describe", "--tags", "--abbrev=0", "--match", "v*", "HEAD"]);
  } catch {
    return null;
  }
}

/** 3 = major (breaking), 2 = minor (feat), 1 = patch (fix/perf/default). */
function conventionalBumpLevel(tag) {
  const range = tag ? `${tag}..HEAD` : "HEAD";
  const log = git(["log", range, "--format=%B%x00"]);
  let level = 0;
  for (const body of log.split("\0")) {
    const subject = body.trim().split("\n")[0] ?? "";
    if (!subject) continue;
    if (/^[A-Za-z]+(\([^)]*\))?!:/.test(subject) || /^BREAKING[ -]CHANGE:/m.test(body)) {
      return 3;
    }
    if (/^feat(\([^)]*\))?:/.test(subject)) level = Math.max(level, 2);
    else if (/^(fix|perf)(\([^)]*\))?:/.test(subject)) level = Math.max(level, 1);
  }
  return level || 1;
}

/** git-cliff --bumped-version, or null when git-cliff is missing/fails. */
function cliffBumpedVersion() {
  try {
    const out = execFileSync("git-cliff", ["--bumped-version"], {
      cwd: ROOT,
      encoding: "utf8",
      stdio: ["pipe", "pipe", "pipe"],
    })
      .trim()
      .replace(/^v/, "");
    return isValidSemver(out) ? out : null;
  } catch {
    return null;
  }
}

function nextBase({ major, minor, patch }, level) {
  if (level === 3) return [major + 1, 0, 0];
  if (level === 2) return [major, minor + 1, 0];
  return [major, minor, patch + 1];
}

/** Next version from the conventional-commit level, no git-cliff involved. */
function deriveNext(current, level) {
  const c = parseSemver(current);
  if (c.prerelease) {
    if (level === 3) {
      // Breaking change: graduate the pre-release line to the next major.
      return nextBase(c, 3).join(".");
    }
    // Stay in the channel: increment the trailing counter (beta.1 -> beta.2).
    const ids = [...c.prerelease];
    const last = ids.length - 1;
    if (/^\d+$/.test(ids[last])) ids[last] = String(Number(ids[last]) + 1);
    else ids.push("1");
    return `${c.major}.${c.minor}.${c.patch}-${ids.join(".")}`;
  }
  return nextBase(c, level).join(".");
}

/** Apply a --prerelease channel to the in-flight (or next) release. */
function withPrereleaseChannel(current, level, channel) {
  const c = parseSemver(current);
  const base = c.prerelease ? [c.major, c.minor, c.patch] : nextBase(c, level);
  const [curChannel, curCounter] = c.prerelease ?? [];
  if (curChannel === channel && /^\d+$/.test(curCounter ?? "")) {
    return `${base.join(".")}-${channel}.${Number(curCounter) + 1}`;
  }
  return `${base.join(".")}-${channel}.1`;
}

// ---------------------------------------------------------------------------
// Compute the next version
// ---------------------------------------------------------------------------

const current = readWorkspaceVersion();
let next;
let source;

if (explicitVersion) {
  next = explicitVersion;
  source = "explicit argument";
} else if (prereleaseChannel) {
  next = withPrereleaseChannel(current, conventionalBumpLevel(lastTag()), prereleaseChannel);
  source = `--prerelease ${prereleaseChannel}`;
} else {
  const fromCliff = cliffBumpedVersion();
  if (fromCliff) {
    next = fromCliff;
    source = "git-cliff --bumped-version";
  } else {
    next = deriveNext(current, conventionalBumpLevel(lastTag()));
    source = "conventional commits (git-cliff unavailable)";
  }
}

if (!semverGt(next, current)) {
  fail(
    `computed version ${next} is not greater than the current ${current} — ` +
      "pass an explicit version: pnpm bump -- <version>",
  );
}

console.log(`bump: ${current} -> ${next} (${source})`);

if (dryRun) {
  console.log("bump: dry run — no files changed, no commit created");
  console.log(`bump: a real run would commit the version change as "release: v${next}"`);
  process.exit(0);
}

// ---------------------------------------------------------------------------
// Write, verify, commit
// ---------------------------------------------------------------------------

const dirty = git(["status", "--porcelain"]);
if (dirty) {
  console.error("bump: refusing to run on a dirty tree — commit or stash first:");
  console.error(dirty);
  process.exit(1);
}

function run(scriptArgs, what) {
  try {
    execFileSync(process.execPath, scriptArgs, { cwd: ROOT, stdio: "inherit" });
  } catch {
    fail(`${what} failed — the version change is left in the working tree for inspection`);
  }
}

run([join(ROOT, "scripts/set-version.mjs"), next], "set-version");
// The publish-set comparison against npm/crates.io must pass before committing.
run([join(ROOT, "scripts/check-versions.mjs")], "check-versions");

// The tree was clean before set-version ran, so every modification is ours.
git(["add", "-u"]);
git(["commit", "-m", `release: v${next}`]);

console.log("");
console.log(`bump: committed "release: v${next}" (${git(["rev-parse", "--short", "HEAD"])})`);
console.log("bump: no tag created, nothing pushed.");
console.log("next step: review the commit, then push to main —");
console.log("  release-tag.yml will tag v" + next + " and release.yml will publish it.");

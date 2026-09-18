#!/usr/bin/env node

/**
 * bump-ide.mjs — compute the editor distribution's next version and commit it.
 *
 *   pnpm bump:ide                   next version from conventional commits
 *                                   touching the editor payload since the last
 *                                   ide/v* tag
 *   pnpm bump:ide -- 0.2.0          explicit version
 *   pnpm bump:ide -- --dry-run      print what would happen, change nothing
 *
 * Every editor package in this tree is a launcher for the same `verter-lsp`
 * binary, so they ship on ONE version line: the VS Code extension, the Zed
 * extension and the Lapce volt all take this version, and the tag it produces
 * is what says which engine a given editor release carries. `pnpm bump` moves
 * the monorepo's npm+crates version and never touches any of them.
 *
 * Same shape as scripts/bump.mjs: write the version, verify it, and create
 * exactly ONE commit — `release(ide): v<version>`, the subject release-tag.yml
 * recognises on main. It never tags and never pushes; review the commit, push
 * to main, and CI tags `ide/v<version>`, which starts release-ide.yml (the
 * Marketplace publish and the editor binaries).
 *
 * The lane — its commit subject, its tag, where its version is read from — is
 * scripts/release-lanes.mjs, the same table release-tag.yml resolves against,
 * so this script and the tagging job cannot disagree about any of them.
 *
 * Marketplace versions are plain MAJOR.MINOR.PATCH — vsce rejects a semver
 * prerelease — so there is no `--prerelease` here. VS Code's own pre-release
 * channel is an odd minor, which is an explicit version this takes as an
 * argument like any other.
 */

import { execFileSync } from "node:child_process";
import { join, resolve } from "node:path";
import { isValidSemver, parseSemver, semverGt } from "./lib/semver.mjs";
import { releaseLane, releaseSubject } from "./release-lanes.mjs";
import { ideVersionTargets } from "./set-ide-version.mjs";

const ROOT = resolve(import.meta.dirname, "..");

/** This script's lane, from the table release-tag.yml reads. */
const LANE = "ide";
const lane = releaseLane(LANE);
/** `ide/v` — the prefix its tags carry, and what `git describe` matches on. */
const TAG_PREFIX = lane.tag("");
const COMMIT_SUBJECT = (version) => releaseSubject(LANE, version);

// ---------------------------------------------------------------------------
// Arguments
// ---------------------------------------------------------------------------

const args = process.argv.slice(2).filter((a) => a !== "--");
const dryRun = args.includes("--dry-run");
const positional = args.filter((a) => a !== "--dry-run");

function fail(message) {
  console.error(`bump-ide: ${message}`);
  process.exit(1);
}

if (positional.length > 1) fail(`unexpected arguments: ${positional.slice(1).join(" ")}`);
const explicitVersion = positional[0] ?? null;
if (explicitVersion && !isValidSemver(explicitVersion)) {
  fail(`"${explicitVersion}" is not strict semver (e.g. 0.1.0)`);
}
if (explicitVersion && parseSemver(explicitVersion).prerelease) {
  fail(
    `"${explicitVersion}" is a prerelease — the VS Code Marketplace only accepts ` +
      "MAJOR.MINOR.PATCH. Use an odd minor for a pre-release build.",
  );
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function git(gitArgs) {
  return execFileSync("git", gitArgs, { cwd: ROOT, encoding: "utf8" }).trim();
}

/**
 * The version the editor distribution is AT — the highest any of its manifests
 * holds.
 *
 * They are normally equal; taking the highest is what lets the first unified
 * release absorb manifests that were versioned separately before the lane
 * existed, without the next version silently going backwards for one of them.
 */
function readEditorVersion() {
  const held = ideVersionTargets().map((target) => ({
    label: target.label,
    version: target.read(target.path),
  }));
  const highest = held.reduce((a, b) => (semverGt(b.version, a.version) ? b : a));
  const behind = held.filter((entry) => entry.version !== highest.version);
  if (behind.length > 0) {
    console.log(
      `bump-ide: editor manifests disagree — taking the highest (${highest.version}, ${highest.label}); ` +
        behind.map((entry) => `${entry.label} ${entry.version}`).join(", "),
    );
  }
  return highest.version;
}

/** Nearest reachable ide/v* tag, or null before the first one. */
function lastTag() {
  try {
    // stderr is piped: before the first tag, git describe writes "No names
    // found" to the terminal, which is an answer here, not a failure.
    return execFileSync(
      "git",
      ["describe", "--tags", "--abbrev=0", "--match", `${TAG_PREFIX}*`, "HEAD"],
      {
        cwd: ROOT,
        encoding: "utf8",
        stdio: ["pipe", "pipe", "pipe"],
      },
    ).trim();
  } catch {
    return null;
  }
}

/**
 * 3 = major (breaking), 2 = minor (feat), 1 = patch (fix/perf/default), over
 * the commits that touched what the editors actually ship.
 *
 * Every editor package bundles or launches the LSP, the MCP server and the
 * TypeScript plugin, so a Rust-only change to the engine is a change to all of
 * them: the paths below are the distribution's payload, not one package's own
 * source tree.
 */
const EXTENSION_PATHS = ["crates", "packages", "extensions", "scripts/set-ide-version.mjs"];

function conventionalBumpLevel(tag) {
  const range = tag ? `${tag}..HEAD` : "HEAD";
  const log = git(["log", range, "--format=%B%x00", "--", ...EXTENSION_PATHS]);
  let level = 0;
  for (const body of log.split("\0")) {
    const subject = body.trim().split("\n")[0] ?? "";
    if (!subject) continue;
    if (/^[A-Za-z]+(\([^)]*\))?!:/.test(subject) || /^BREAKING[ -]CHANGE:/m.test(body)) return 3;
    if (/^feat(\([^)]*\))?:/.test(subject)) level = Math.max(level, 2);
    else if (/^(fix|perf)(\([^)]*\))?:/.test(subject)) level = Math.max(level, 1);
  }
  return level || 1;
}

function nextVersion(current, level) {
  const { major, minor, patch } = parseSemver(current);
  if (level === 3) return `${major + 1}.0.0`;
  if (level === 2) return `${major}.${minor + 1}.0`;
  return `${major}.${minor}.${patch + 1}`;
}

// ---------------------------------------------------------------------------
// Compute the next version
// ---------------------------------------------------------------------------

const current = readEditorVersion();
let next;
let source;

if (explicitVersion) {
  next = explicitVersion;
  source = "explicit argument";
} else {
  const tag = lastTag();
  next = nextVersion(current, conventionalBumpLevel(tag));
  source = `conventional commits since ${tag ?? "the start of history"}`;
}

if (!semverGt(next, current)) {
  fail(
    `computed version ${next} is not greater than the current ${current} — ` +
      "pass an explicit version: pnpm bump:ide -- <version>",
  );
}

console.log(`bump-ide: editor distribution ${current} -> ${next} (${source})`);

if (dryRun) {
  console.log("bump-ide: dry run — no files changed, no commit created");
  console.log(`bump-ide: a real run would commit it as "${COMMIT_SUBJECT(next)}"`);
  process.exit(0);
}

// ---------------------------------------------------------------------------
// Write, verify, commit
// ---------------------------------------------------------------------------

const dirty = git(["status", "--porcelain"]);
if (dirty) {
  console.error("bump-ide: refusing to run on a dirty tree — commit or stash first:");
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

run([join(ROOT, "scripts/set-ide-version.mjs"), next], "set-ide-version");
run([join(ROOT, "scripts/set-ide-version.mjs"), "--check", next], "set-ide-version --check");

// The tree was clean before set-ide-version ran, so every modification is ours.
git(["add", "-u"]);
git(["commit", "-m", COMMIT_SUBJECT(next)]);

console.log("");
console.log(
  `bump-ide: committed "${COMMIT_SUBJECT(next)}" (${git(["rev-parse", "--short", "HEAD"])})`,
);
console.log("bump-ide: no tag created, nothing pushed.");
console.log("next step: review the commit, then push to main —");
console.log(`  release-tag.yml will tag ${lane.tag(next)} and release-ide.yml will publish it.`);

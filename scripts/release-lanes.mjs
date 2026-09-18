#!/usr/bin/env node

/**
 * release-lanes.mjs — the release lanes this repository publishes on.
 *
 * One repository, several things to ship, each on its own version line. A lane
 * is named by the SCOPE of its release commit and tagged under its own prefix:
 *
 *   commit                       tag              workflow
 *   --------------------------   --------------   ---------------
 *   release: v<version>          v<version>       release.yml
 *   release(ide): v<version>     ide/v<version>   release-ide.yml
 *
 * The unscoped lane is the monorepo itself — crates.io, npm, and the VSIX built
 * from that same version. A scoped lane ships one thing on a version line that
 * moves independently of it. `release-tag.yml` reads this table to turn a
 * version commit on main into the right tag, so adding a lane is an entry here
 * plus the workflow that listens on its tag — never workflow surgery.
 *
 * Each lane owns:
 *   - `version()`  where its version is READ FROM. The tree is the truth; the
 *                  commit message is only a signal, and the two must agree.
 *   - `verify`     the checks that prove its whole publish surface holds that
 *                  version, run before anything is tagged. A partial bump must
 *                  never reach a tag.
 *   - `tag()`      the tag its release workflow listens on.
 *
 * Usage:
 *   node scripts/release-lanes.mjs list
 *   node scripts/release-lanes.mjs resolve <lane>           # prints "<version> <tag>"
 *   node scripts/release-lanes.mjs verify <lane> <version>
 *
 * `<lane>` is the commit scope, and "" (or the literal `-`) is the monorepo.
 */

import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { join, resolve } from "node:path";

const ROOT = resolve(import.meta.dirname, "..");

/** The subject of a release commit for a lane, and the pattern that reads one back. */
export const releaseSubject = (lane, version) =>
  lane ? `release(${lane}): v${version}` : `release: v${version}`;

/** `^release(\(<lane>\))?: v<semver>$` — the scope is the lane, absent means the monorepo. */
export const RELEASE_SUBJECT_RE =
  /^release(?:\(([a-z0-9][a-z0-9-]*)\))?: v(\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?)$/;

function readJsonVersion(relativePath) {
  return JSON.parse(readFileSync(join(ROOT, relativePath), "utf8")).version;
}

/** `[workspace.package] version` from Cargo.toml — every crate inherits it. */
function readWorkspaceVersion() {
  const lines = readFileSync(join(ROOT, "Cargo.toml"), "utf8").split("\n");
  const start = lines.findIndex((l) => l.trim() === "[workspace.package]");
  if (start === -1) throw new Error("no [workspace.package] section in Cargo.toml");
  for (let i = start + 1; i < lines.length; i++) {
    if (lines[i].trim().startsWith("[")) break;
    const m = lines[i].match(/^\s*version\s*=\s*"([^"]+)"/);
    if (m) return m[1];
  }
  throw new Error("no version in [workspace.package] in Cargo.toml");
}

export const RELEASE_LANES = {
  "": {
    name: "monorepo",
    source: "Cargo.toml [workspace.package]",
    bump: "pnpm bump",
    workflow: "release.yml",
    publishes: "crates.io, npm, and the VS Code extension built from that same version",
    tag: (version) => `v${version}`,
    version: readWorkspaceVersion,
    verify: [["scripts/set-version.mjs", "--check", "{version}"], ["scripts/check-versions.mjs"]],
  },
  ide: {
    name: "editor distribution",
    // Every editor package is a launcher for the same verter-lsp binary, so
    // they share one version; the VSIX is the one with a registry to read it
    // back from, which is why it names the lane's version here.
    source: "packages/vue-vscode/package.json (with extensions/{zed,lapce})",
    bump: "pnpm bump:ide",
    workflow: "release-ide.yml",
    publishes:
      "the VS Code Marketplace (verter.verter-vscode), and the per-platform verter-lsp / " +
      "verter-mcp binaries every other editor launches",
    tag: (version) => `ide/v${version}`,
    version: () => readJsonVersion("packages/vue-vscode/package.json"),
    // Covers all three editor manifests, and refuses a prerelease version,
    // which vsce rejects outright.
    verify: [["scripts/set-ide-version.mjs", "--check", "{version}"]],
  },
};

/** A lane by its commit scope. "" and "-" both mean the monorepo. */
export function releaseLane(scope) {
  const key = scope === "-" ? "" : (scope ?? "");
  return Object.hasOwn(RELEASE_LANES, key) ? { id: key, ...RELEASE_LANES[key] } : null;
}

/** Parse a release commit subject into `{ lane, version }`, or null when it is not one. */
export function parseReleaseSubject(subject) {
  const m = RELEASE_SUBJECT_RE.exec(subject.trim());
  return m ? { lane: m[1] ?? "", version: m[2] } : null;
}

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

if (process.argv[1] && resolve(process.argv[1]) === resolve(import.meta.filename)) {
  const [command, scope, version] = process.argv.slice(2);

  const fail = (message) => {
    console.error(`release-lanes: ${message}`);
    process.exit(1);
  };

  const known = () =>
    Object.keys(RELEASE_LANES)
      .map((key) => key || "(monorepo)")
      .join(", ");

  if (command === "list") {
    for (const [key, lane] of Object.entries(RELEASE_LANES)) {
      console.log(`${key || "(none)"}\t${lane.name}`);
      console.log(`  commit    ${releaseSubject(key, "<version>")}`);
      console.log(`  tag       ${lane.tag("<version>")}`);
      console.log(`  version   ${lane.source}`);
      console.log(`  bump      ${lane.bump}`);
      console.log(`  publishes ${lane.publishes} (${lane.workflow})`);
    }
    process.exit(0);
  }

  const lane = releaseLane(scope);
  if (!lane) fail(`unknown release lane "${scope}" — known lanes: ${known()}`);

  if (command === "resolve") {
    let current;
    try {
      current = lane.version();
    } catch (error) {
      fail(`could not read the ${lane.name} version from ${lane.source}: ${error.message}`);
    }
    if (!current) fail(`could not read the ${lane.name} version from ${lane.source}`);
    // One line, two fields: `<version> <tag>`. Shell reads it without a parser.
    console.log(`${current} ${lane.tag(current)}`);
    process.exit(0);
  }

  if (command === "verify") {
    if (!version) fail("usage: release-lanes.mjs verify <lane> <version>");
    for (const step of lane.verify) {
      const args = step.map((part) => part.replaceAll("{version}", version));
      console.log(`release-lanes: ${lane.name} — node ${args.join(" ")}`);
      try {
        execFileSync(process.execPath, [join(ROOT, args[0]), ...args.slice(1)], {
          cwd: ROOT,
          stdio: "inherit",
        });
      } catch {
        fail(`${args[0]} failed for ${lane.name} ${version}`);
      }
    }
    process.exit(0);
  }

  console.error("usage: release-lanes.mjs list | resolve <lane> | verify <lane> <version>");
  process.exit(2);
}

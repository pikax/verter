#!/usr/bin/env node

/**
 * set-ide-version.mjs — write (or verify) the editor distribution's version.
 *
 * Usage:
 *   node scripts/set-ide-version.mjs <version>          # write it
 *   node scripts/set-ide-version.mjs --check <version>  # verify only, write nothing
 *
 * Every editor package in this tree is a launcher for the SAME `verter-lsp`
 * binary: the VSIX embeds it, the Zed extension and the Lapce volt spawn it, and
 * Helix and nvim download it. One version across all of them is what makes
 * "which engine is in my editor?" answerable — the `ide/v<version>` tag.
 *
 * Target set:
 *   - every `MARKETPLACE_ONLY` package (scripts/lib/publish-set.mjs) —
 *     packages/vue-vscode/package.json, the version vsce publishes
 *   - extensions/zed/extension.toml
 *   - extensions/lapce/volt.toml
 *
 * The monorepo's own version line (crates.io + npm, `pnpm bump` /
 * scripts/set-version.mjs) is separate and untouched: the extension is
 * `private: true`, so the npm publish set never contains it, and the editor
 * packages are not npm packages at all.
 *
 * Editor versions are plain `MAJOR.MINOR.PATCH`: vsce rejects a semver
 * prerelease (`0.1.0-beta.1`) outright, and VS Code's own pre-release channel
 * uses an odd minor rather than a prerelease identifier. Both modes refuse one
 * here rather than at `vsce package`, an hour into a release.
 *
 * Both modes fail loudly — exit 1, naming each offender — if any target does
 * not hold exactly <version>.
 */

import { readFileSync, writeFileSync } from "node:fs";
import { join, relative, resolve } from "node:path";
import { computePublishSet, scanWorkspacePackages } from "./lib/publish-set.mjs";
import { isValidSemver, parseSemver } from "./lib/semver.mjs";

const ROOT = resolve(import.meta.dirname, "..");

/** Editor packages that are not npm packages: their manifest is TOML. */
const TOML_TARGETS = [
  { label: "zed extension", path: "extensions/zed/extension.toml" },
  { label: "lapce volt", path: "extensions/lapce/volt.toml" },
];

// ---------------------------------------------------------------------------
// Readers / writers
// ---------------------------------------------------------------------------

function readPkgVersion(path) {
  return JSON.parse(readFileSync(path, "utf8")).version;
}

function writePkgVersion(path, next) {
  const text = readFileSync(path, "utf8");
  const m = text.match(/"version"\s*:\s*"[^"]*"/);
  if (!m) throw new Error(`set-ide-version: no "version" field in ${path}`);
  writeFileSync(path, text.replace(m[0], `"version": "${next}"`));
}

/**
 * The manifest's own `version`, which is the one before any `[section]` header.
 * A `version` inside a table belongs to that table (a dependency, a config
 * default) and is never the package's.
 */
function tomlVersionLine(text) {
  const lines = text.split("\n");
  for (const [index, line] of lines.entries()) {
    if (line.trimStart().startsWith("[")) break;
    const m = line.match(/^(\s*version\s*=\s*)"([^"]*)"(.*)$/);
    if (m) return { index, prefix: m[1], value: m[2], suffix: m[3], lines };
  }
  return null;
}

function readTomlVersion(path) {
  const found = tomlVersionLine(readFileSync(path, "utf8"));
  if (!found) throw new Error(`set-ide-version: no top-level version in ${path}`);
  return found.value;
}

function writeTomlVersion(path, next) {
  const found = tomlVersionLine(readFileSync(path, "utf8"));
  if (!found) throw new Error(`set-ide-version: no top-level version in ${path}`);
  found.lines[found.index] = `${found.prefix}"${next}"${found.suffix}`;
  writeFileSync(path, found.lines.join("\n"));
}

/**
 * Every manifest the editor distribution versions, in publish order: the
 * Marketplace package first, then the editor packages.
 */
export function ideVersionTargets() {
  const publishSet = computePublishSet();
  const workspace = scanWorkspacePackages(join(ROOT, "packages"));
  const targets = publishSet.marketplaceOnly.map((name) => {
    const entry = workspace.get(name);
    if (!entry)
      throw new Error(`set-ide-version: marketplace package "${name}" is not in packages/`);
    // A package published to BOTH registries would have two version lines
    // claiming one file; the monorepo bump and this one would fight over it.
    if (publishSet.npm.includes(name)) {
      throw new Error(
        `set-ide-version: "${name}" is in the npm publish set as well — it cannot carry an independent editor version`,
      );
    }
    return {
      label: name,
      path: join(entry.dir, "package.json"),
      read: readPkgVersion,
      write: writePkgVersion,
    };
  });
  for (const target of TOML_TARGETS) {
    targets.push({
      label: target.label,
      path: join(ROOT, target.path),
      read: readTomlVersion,
      write: writeTomlVersion,
    });
  }
  return targets;
}

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

if (process.argv[1] && resolve(process.argv[1]) === resolve(import.meta.filename)) {
  const args = process.argv.slice(2).filter((a) => a !== "--");
  const checkMode = args.includes("--check");
  const positional = args.filter((a) => a !== "--check");

  if (positional.length !== 1 || !isValidSemver(positional[0])) {
    console.error("usage: node scripts/set-ide-version.mjs [--check] <version>");
    console.error("       <version> must be strict semver, e.g. 0.2.0");
    process.exit(2);
  }
  const version = positional[0];

  // The Marketplace takes three integers and nothing else.
  if (parseSemver(version).prerelease) {
    console.error(
      `set-ide-version: "${version}" is a prerelease — the VS Code Marketplace ` +
        "only accepts MAJOR.MINOR.PATCH. Use an odd minor for a pre-release build.",
    );
    process.exit(1);
  }

  const targets = ideVersionTargets();
  if (targets.length === 0) {
    console.error("set-ide-version: the editor target set is empty — refusing to do nothing");
    process.exit(1);
  }

  const offenders = [];
  for (const target of targets) {
    const current = target.read(target.path);
    if (current === version) continue;
    if (checkMode) {
      offenders.push(`${target.label} (${relative(ROOT, target.path)}): ${current}`);
      continue;
    }
    target.write(target.path, version);
    console.log(`set-ide-version: ${target.label} ${current} -> ${version}`);
  }

  if (checkMode) {
    if (offenders.length > 0) {
      console.error(`set-ide-version: ${offenders.length} target(s) are not at ${version}:`);
      for (const offender of offenders) console.error(`  ${offender}`);
      process.exit(1);
    }
    console.log(`set-ide-version: every editor target is at ${version}`);
    process.exit(0);
  }

  // Writing must land: re-read every target rather than trust the replacement.
  const missed = targets.filter((target) => target.read(target.path) !== version);
  if (missed.length > 0) {
    console.error("set-ide-version: these targets did not take the version:");
    for (const target of missed) console.error(`  ${relative(ROOT, target.path)}`);
    process.exit(1);
  }
  console.log(`set-ide-version: ${targets.length} target(s) at ${version}`);
}

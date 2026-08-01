#!/usr/bin/env node

/**
 * check-versions.mjs
 *
 * Compares local package versions against published versions on npm/crates.io.
 * Detects pre-release channels (alpha, beta, rc) and computes topological
 * publish order from workspace dependencies.
 *
 * Usage:
 *   node scripts/check-versions.mjs          # Human-readable output
 *   node scripts/check-versions.mjs --json   # JSON output for CI
 */

import { execSync } from "node:child_process";
import { readFileSync, existsSync } from "node:fs";
import { join, relative, resolve } from "node:path";
import { computePublishSet, scanWorkspacePackages } from "./lib/publish-set.mjs";

const ROOT = resolve(import.meta.dirname, "..");
const PACKAGES_DIR = join(ROOT, "packages");
const jsonMode = process.argv.includes("--json");

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function readJson(filePath) {
  return JSON.parse(readFileSync(filePath, "utf8"));
}

/** Fetch the published npm version. Returns null if never published. */
function getNpmVersion(name) {
  try {
    const out = execSync(`npm view ${name} version 2>/dev/null`, {
      encoding: "utf8",
      stdio: ["pipe", "pipe", "pipe"],
    }).trim();
    return out || null;
  } catch {
    return null; // 404 = never published
  }
}

/** Fetch the published crates.io version for a crate. */
function getCrateVersion(name) {
  try {
    const out = execSync(`cargo search ${name} --limit 1`, {
      encoding: "utf8",
      stdio: ["pipe", "pipe", "pipe"],
    }).trim();
    const match = out.match(new RegExp(`^${name}\\s*=\\s*"([^"]+)"`));
    return match ? match[1] : null;
  } catch {
    return null;
  }
}

/** Detect pre-release channel from a semver version string. */
function detectChannel(version) {
  const match = version.match(/-(alpha|beta|rc)\./);
  return match ? match[1] : null;
}

/**
 * Simple semver comparison.
 * Returns true if a > b.
 */
function semverGt(a, b) {
  if (!b) return true; // never published
  const normalize = (v) =>
    v
      .replace(/-(alpha|beta|rc)\.(\d+)/, "")
      .split(".")
      .map(Number);
  const pa = normalize(a);
  const pb = normalize(b);
  for (let i = 0; i < 3; i++) {
    if ((pa[i] || 0) > (pb[i] || 0)) return true;
    if ((pa[i] || 0) < (pb[i] || 0)) return false;
  }
  // Same base version — compare pre-release
  const chA = a.includes("-") ? a.split("-")[1] : "zzz"; // stable > pre-release
  const chB = b.includes("-") ? b.split("-")[1] : "zzz";
  return chA > chB;
}

// ---------------------------------------------------------------------------
// Gather npm packages — derived from the product dependency closure
// (scripts/lib/publish-set.mjs), NOT "every non-private package".
// Marketplace-only packages (verter-vscode) and platform sub-packages are
// excluded here; the release workflow publishes platform packages separately.
// ---------------------------------------------------------------------------

const publishSet = computePublishSet();
const workspacePackages = scanWorkspacePackages(PACKAGES_DIR);

const packages = [];

for (const name of publishSet.npm) {
  const entry = workspacePackages.get(name);
  packages.push({
    name,
    dir: relative(PACKAGES_DIR, entry.dir),
    localVersion: entry.pkg.version,
    publishedVersion: null,
    needsPublish: false,
    distTag: null,
  });
}

// Fetch published versions
for (const pkg of packages) {
  pkg.publishedVersion = getNpmVersion(pkg.name);
  pkg.needsPublish = semverGt(pkg.localVersion, pkg.publishedVersion);
  pkg.distTag = detectChannel(pkg.localVersion);
}

// ---------------------------------------------------------------------------
// Publish order — the derived topological order (dependencies first),
// restricted to packages that need publishing. Directory names, as
// release.yml expects (`cd "packages/$pkg_dir"`).
// ---------------------------------------------------------------------------

const needsPublish = new Set(packages.filter((p) => p.needsPublish).map((p) => p.name));
const dirByName = new Map(packages.map((p) => [p.name, p.dir]));
const order = publishSet.order
  .filter((name) => needsPublish.has(name))
  .map((name) => dirByName.get(name));

// ---------------------------------------------------------------------------
// Rust crates
// ---------------------------------------------------------------------------

const cargoToml = readFileSync(join(ROOT, "Cargo.toml"), "utf8");
const cargoVersionMatch = cargoToml.match(/version\s*=\s*"([^"]+)"/);
const cargoVersion = cargoVersionMatch ? cargoVersionMatch[1] : null;

/** Crates published to crates.io, in dependency order (see .github/workflows/release.yml) */
const PUBLISHED_CRATES = ["verter_span", "verter_compiler"];

const rustCrates = [];
for (const crate of PUBLISHED_CRATES) {
  const published = getCrateVersion(crate);
  rustCrates.push({
    crate,
    localVersion: cargoVersion,
    publishedVersion: published,
    needsPublish: cargoVersion ? semverGt(cargoVersion, published) : false,
  });
}

// ---------------------------------------------------------------------------
// VS Code extension validation
// ---------------------------------------------------------------------------

const vscodeWarnings = [];
const vscodePkgPath = join(PACKAGES_DIR, "vue-vscode", "package.json");
if (existsSync(vscodePkgPath)) {
  const vscodePkg = readJson(vscodePkgPath);
  const typesVscodeRaw = vscodePkg.devDependencies?.["@types/vscode"];
  const enginesVscodeRaw = vscodePkg.engines?.vscode;
  if (typesVscodeRaw && enginesVscodeRaw) {
    const extractMinVersion = (range) => range.replace(/^[\^~>=<\s]+/, "").split(" ")[0];
    const typesMin = extractMinVersion(typesVscodeRaw);
    const enginesMin = extractMinVersion(enginesVscodeRaw);
    const parseVer = (v) => v.split(".").map(Number);
    const tv = parseVer(typesMin);
    const ev = parseVer(enginesMin);
    const typesExceedsEngines =
      tv[0] > ev[0] ||
      (tv[0] === ev[0] && tv[1] > ev[1]) ||
      (tv[0] === ev[0] && tv[1] === ev[1] && (tv[2] || 0) > (ev[2] || 0));
    if (typesExceedsEngines) {
      vscodeWarnings.push(
        `@types/vscode ${typesVscodeRaw} exceeds engines.vscode ${enginesVscodeRaw} — vsce will reject this`,
      );
    }
  }
}

// ---------------------------------------------------------------------------
// Output
// ---------------------------------------------------------------------------

const isPrerelease = packages.some((p) => detectChannel(p.localVersion));
const channel = detectChannel(packages[0]?.localVersion);

const result = {
  packages: packages.map((p) => ({
    name: p.name,
    dir: p.dir,
    localVersion: p.localVersion,
    publishedVersion: p.publishedVersion,
    needsPublish: p.needsPublish,
    distTag: p.distTag,
  })),
  order,
  isPrerelease,
  channel,
  rust: rustCrates,
  vscodeWarnings,
};

if (jsonMode) {
  process.stdout.write(JSON.stringify(result, null, 2) + "\n");
} else {
  console.log("=== npm packages ===");
  for (const p of result.packages) {
    const status = p.needsPublish ? "PUBLISH" : "skip";
    const tag = p.distTag ? ` (--tag ${p.distTag})` : "";
    console.log(
      `  [${status}] ${p.name}: ${p.publishedVersion || "(none)"} -> ${p.localVersion}${tag}`,
    );
  }
  console.log(`\n  Publish order: ${order.join(" -> ")}`);
  console.log(`  Pre-release: ${isPrerelease} (channel: ${channel || "stable"})`);

  console.log("\n=== Rust crates ===");
  for (const r of rustCrates) {
    const rStatus = r.needsPublish ? "PUBLISH" : "skip";
    console.log(
      `  [${rStatus}] ${r.crate}: ${r.publishedVersion || "(none)"} -> ${r.localVersion}`,
    );
  }

  if (vscodeWarnings.length > 0) {
    console.log("\n=== VS Code Extension Warnings ===");
    for (const w of vscodeWarnings) {
      console.log(`  WARNING: ${w}`);
    }
  }
}

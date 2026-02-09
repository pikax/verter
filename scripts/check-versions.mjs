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
import { readFileSync, readdirSync, existsSync } from "node:fs";
import { join, resolve } from "node:path";

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
// Gather npm packages
// ---------------------------------------------------------------------------

const packages = [];
const packagesByName = new Map();

for (const dir of readdirSync(PACKAGES_DIR)) {
  const pkgPath = join(PACKAGES_DIR, dir, "package.json");
  if (!existsSync(pkgPath)) continue;
  const pkg = readJson(pkgPath);
  if (pkg.private) continue;

  const info = {
    name: pkg.name,
    dir,
    localVersion: pkg.version,
    publishedVersion: null,
    needsPublish: false,
    distTag: null,
    workspaceDeps: [],
  };

  // Collect workspace deps for topological ordering
  for (const depField of ["dependencies", "peerDependencies"]) {
    if (!pkg[depField]) continue;
    for (const [dep, range] of Object.entries(pkg[depField])) {
      if (typeof range === "string" && range.startsWith("workspace:")) {
        info.workspaceDeps.push(dep);
      }
    }
  }

  packages.push(info);
  packagesByName.set(pkg.name, info);
}

// Fetch published versions
for (const pkg of packages) {
  pkg.publishedVersion = getNpmVersion(pkg.name);
  pkg.needsPublish = semverGt(pkg.localVersion, pkg.publishedVersion);
  pkg.distTag = detectChannel(pkg.localVersion);
}

// ---------------------------------------------------------------------------
// Topological sort
// ---------------------------------------------------------------------------

function topoSort(pkgs) {
  const visited = new Set();
  const order = [];
  const nameSet = new Set(pkgs.map((p) => p.name));

  function visit(pkg) {
    if (visited.has(pkg.name)) return;
    visited.add(pkg.name);
    for (const dep of pkg.workspaceDeps) {
      if (nameSet.has(dep)) {
        visit(packagesByName.get(dep));
      }
    }
    order.push(pkg.dir);
  }

  for (const pkg of pkgs) visit(pkg);
  return order;
}

const order = topoSort(packages.filter((p) => p.needsPublish));

// ---------------------------------------------------------------------------
// Rust crates
// ---------------------------------------------------------------------------

const cargoToml = readFileSync(join(ROOT, "Cargo.toml"), "utf8");
const cargoVersionMatch = cargoToml.match(/version\s*=\s*"([^"]+)"/);
const cargoVersion = cargoVersionMatch ? cargoVersionMatch[1] : null;
const cratePublished = getCrateVersion("verter_core");
const crateNeedsPublish = cargoVersion ? semverGt(cargoVersion, cratePublished) : false;

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
  rust: {
    crate: "verter_core",
    localVersion: cargoVersion,
    publishedVersion: cratePublished,
    needsPublish: crateNeedsPublish,
  },
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
  const rStatus = crateNeedsPublish ? "PUBLISH" : "skip";
  console.log(`  [${rStatus}] verter_core: ${cratePublished || "(none)"} -> ${cargoVersion}`);
}

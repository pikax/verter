#!/usr/bin/env node

// Dry-run publish verification: runs `npm pack --dry-run` on each package in
// the derived publish set and validates the tarball contents.

import { execSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { join, relative, resolve } from "node:path";
import { computePublishSet, scanWorkspacePackages } from "./lib/publish-set.mjs";

const ROOT = resolve(import.meta.dirname, "..");

// The publish set is derived from the product dependency closure — the same
// authority the release workflow publishes from — so tarball verification
// covers exactly what gets published.
const publishSet = computePublishSet();
const workspacePackages = scanWorkspacePackages(join(ROOT, "packages"));
const PUBLISHABLE = [
  ...publishSet.npm.map((name) => relative(ROOT, workspacePackages.get(name).dir)),
  ...publishSet.platform,
];

/** File patterns that should NOT appear in a tarball. */
const FORBIDDEN_PATTERNS = [
  /\.spec\.[jt]sx?$/,
  /\.test\.[jt]sx?$/,
  /__tests__\//,
  /tsconfig\..*\.json$/,
  /\.eslintrc/,
  /vitest\.config/,
];

let failed = false;

for (const pkg of PUBLISHABLE) {
  const pkgDir = join(ROOT, pkg);
  const pkgJson = JSON.parse(readFileSync(join(pkgDir, "package.json"), "utf-8"));

  if (pkgJson.private) continue;

  const name = pkgJson.name;
  process.stdout.write(`\n--- ${name} ---\n`);

  try {
    const output = execSync("npm pack --dry-run --json 2>/dev/null", {
      cwd: pkgDir,
      encoding: "utf-8",
      timeout: 30_000,
    });

    let packInfo;
    try {
      packInfo = JSON.parse(output);
    } catch {
      // npm pack --dry-run --json sometimes outputs non-JSON warnings before the JSON
      const jsonStart = output.indexOf("[");
      if (jsonStart >= 0) {
        packInfo = JSON.parse(output.slice(jsonStart));
      } else {
        console.error(`  WARN: could not parse npm pack output for ${name}`);
        continue;
      }
    }

    const entry = Array.isArray(packInfo) ? packInfo[0] : packInfo;
    const files = entry.files || [];
    const totalBytes = entry.unpackedSize || 0;

    // Check for forbidden files
    for (const f of files) {
      const path = f.path || f;
      for (const pat of FORBIDDEN_PATTERNS) {
        if (pat.test(path)) {
          console.error(`  FAIL: forbidden file in tarball: ${path}`);
          failed = true;
        }
      }
    }

    // Check that dist/ is present (for packages that should have it)
    if (pkgJson.files?.includes("dist") || pkgJson.main?.startsWith("dist/")) {
      const hasDist = files.some((f) => (f.path || f).startsWith("dist/"));
      if (!hasDist) {
        console.error(`  WARN: no dist/ files found — package may not be built yet`);
      }
    }

    // Size sanity check (warn if > 5MB unpacked)
    const sizeMB = (totalBytes / (1024 * 1024)).toFixed(2);
    if (totalBytes > 5 * 1024 * 1024) {
      console.error(`  WARN: large package (${sizeMB} MB unpacked)`);
    }

    console.log(`  OK: ${files.length} files, ${sizeMB} MB unpacked`);
  } catch (err) {
    console.error(`  FAIL: npm pack failed for ${name}: ${err.message}`);
    failed = true;
  }
}

if (failed) {
  console.error("\nVerification FAILED — see errors above.");
  process.exit(1);
} else {
  console.log("\nAll packages verified successfully.");
}

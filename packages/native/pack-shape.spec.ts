/**
 * Pack-shape guard for GitHub issue pikax/verter#90.
 *
 * The published `@verter/native` tarball MUST contain the thin root
 * wrapper (`index.js`) plus the NAPI-generated loader (`dist/index.js`)
 * and its types (`dist/index.d.ts`), and MUST NOT contain any `.node`
 * binary (the real binaries ship ONLY in the per-platform
 * `@verter/native-<triple>` optional-dependency packages).
 *
 * This guard prevents the bug class from reappearing in either of its
 * two original forms:
 *   1. A root `index.js` that re-grows a hand-written platform-resolution
 *      table (the dist-only loader with no optional-dependency fallback).
 *   2. A tarball that omits `dist/index.js` (so the wrapper's
 *      `require('./dist/index.js')` throws MODULE_NOT_FOUND), or that
 *      accidentally ships a `.node` (bloating the main package and
 *      masking the optional-dependency mechanism).
 *
 * The ship list comes from `npm pack --dry-run --json`, which reports the
 * EXACT set of files that would publish (driven by `package.json#files`)
 * without writing a tarball — so it characterizes precisely what ships.
 * File CONTENT is asserted against the on-disk built artifacts, which the
 * dry-run confirms are in the ship set and which `pack` copies verbatim.
 */

import { beforeAll, describe, expect, it } from "vitest";
import { execSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const __filename = fileURLToPath(import.meta.url);
const packageDir = dirname(__filename);

// NOTE: per-triple proof that the shipped generated loader actually
// REQUIRES `@verter/native-<triple>` for each platform lives in
// `loader-vm-fallback.spec.ts`, which executes the real loader in a VM and
// asserts the exact package it falls back to. That replaces the previous
// weak `loaderSource.includes("@verter/native-…")` substring check here
// (a substring proves a name appears somewhere; it does not prove the
// loader uses it for that platform). This file keeps the genuine
// ship-list + wrapper-shape guards, which characterize what actually
// publishes.

// `npm pack --dry-run` is the expensive step (npm cold-start); run it
// ONCE for the whole suite. Paths are package-root-relative (no
// `package/` prefix).
let shipList: string[] = [];

beforeAll(() => {
  const raw = execSync("npm pack --dry-run --json", {
    cwd: packageDir,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "ignore"],
  });
  const parsed = JSON.parse(raw) as Array<{ files?: Array<{ path: string }> }>;
  shipList = (parsed[0]?.files ?? []).map((f) => f.path.replace(/\\/g, "/"));
}, 60_000);

describe("issue #90 — @verter/native pack shape", () => {
  it("ship list contains the wrapper + generated loader + types, and NO .node", () => {
    // MUST contain the thin wrapper, the generated loader, and types.
    expect(shipList).toContain("index.js");
    expect(shipList).toContain("dist/index.js");
    expect(shipList).toContain("dist/index.d.ts");

    // MUST NOT contain any native binary anywhere in the ship set.
    expect(shipList.filter((f) => f.endsWith(".node"))).toEqual([]);

    // Every shipped dist entry is enumerated (no bare `dist/` raking in a
    // `.node`).
    const distEntries = shipList.filter((f) => f.startsWith("dist/"));
    expect(distEntries.length).toBeGreaterThan(0);
    expect(distEntries.every((f) => !f.endsWith(".node"))).toBe(true);
  });

  it("the root index.js is the thin wrapper (no platform-resolution table, no .node filename list)", () => {
    expect(shipList).toContain("index.js");
    const rootSource = readFileSync(join(packageDir, "index.js"), "utf8");

    // It delegates platform/optional-dep resolution to the generated loader.
    expect(rootSource).toMatch(/require\(["']\.\/dist\/index\.js["']\)/);

    // It must NOT re-grow the hand-written platform-resolution machinery.
    expect(rootSource).not.toMatch(/Failed to load native binding/);
    expect(rootSource).not.toMatch(/switch\s*\(\s*platform\s*\)/);
    expect(rootSource).not.toMatch(/function\s+tryLoad/);
    // It must NOT enumerate any `.node` filename itself.
    expect(rootSource).not.toMatch(/\.node['"]/);
  });
});

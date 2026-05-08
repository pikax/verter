/**
 * Phase 4 Test #4 — Architecture guard.
 *
 * Plan §6.4 row 4 (Claude P1-13 explicit body): the
 * `@verter/typeinfo` package MUST NOT depend on
 * `@verter/component-meta`. The dependency direction is
 * typeinfo (foundation) → component-meta (specialisation), never the
 * other way.
 *
 * REGRESSION — discriminating: the assertions FAIL pre-cutover
 * (when typeinfo would naturally have inherited from component-meta
 * by re-using its `nativeToDescriptor`) and PASS post-cutover.
 */

import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";
import { glob } from "glob";

const __dirname = dirname(fileURLToPath(import.meta.url));
const PACKAGE_ROOT = resolve(__dirname, "..");

describe("typeinfo architecture guards", () => {
  it("package.json has zero @verter/component-meta deps", () => {
    const pkg = JSON.parse(readFileSync(resolve(PACKAGE_ROOT, "package.json"), "utf-8")) as {
      dependencies?: Record<string, string>;
      devDependencies?: Record<string, string>;
      peerDependencies?: Record<string, string>;
    };
    const allDeps = {
      ...pkg.dependencies,
      ...pkg.devDependencies,
      ...pkg.peerDependencies,
    };
    expect(Object.keys(allDeps)).not.toContain("@verter/component-meta");
  });

  it('source has zero "@verter/component-meta" imports', () => {
    const sources = glob.sync("src/**/*.ts", { cwd: PACKAGE_ROOT });
    expect(sources.length).toBeGreaterThan(0);
    for (const file of sources) {
      const text = readFileSync(resolve(PACKAGE_ROOT, file), "utf-8");
      expect(
        /from\s+['"]@verter\/component-meta['"]/.test(text),
        `${file} must not import from @verter/component-meta`,
      ).toBe(false);
      // Catch the rare `import "@verter/component-meta"` (side-effect)
      // and dynamic `import("@verter/component-meta")` form too.
      expect(
        /\bimport\s*\(\s*['"]@verter\/component-meta['"]\s*\)/.test(text),
        `${file} must not dynamic-import @verter/component-meta`,
      ).toBe(false);
      expect(
        /\bimport\s+['"]@verter\/component-meta['"]/.test(text),
        `${file} must not side-effect import @verter/component-meta`,
      ).toBe(false);
    }
  });

  it("source has at least one @verter/native and @verter/type-ir reference (positive sanity)", () => {
    // Negative asserts only are fragile — confirm the substrate is
    // wired through the expected packages. If this fails, the package
    // is empty / scaffold-only and the architecture guards above
    // pass trivially.
    const sources = glob.sync("src/**/*.ts", { cwd: PACKAGE_ROOT });
    let nativeRefs = 0;
    let typeIrRefs = 0;
    for (const file of sources) {
      const text = readFileSync(resolve(PACKAGE_ROOT, file), "utf-8");
      if (/from\s+['"]@verter\/native['"]/.test(text)) {
        nativeRefs += 1;
      }
      if (/from\s+['"]@verter\/type-ir['"]/.test(text)) {
        typeIrRefs += 1;
      }
    }
    expect(nativeRefs, "expected at least one @verter/native import").toBeGreaterThan(0);
    expect(typeIrRefs, "expected at least one @verter/type-ir import").toBeGreaterThan(0);
  });
});

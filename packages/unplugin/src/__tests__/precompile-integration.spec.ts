/**
 * @ai-generated - Integration tests for preCompile with a real-world Vue project.
 * Uses the coreui-free-vue-admin-template test repo to verify that preCompile
 * works end-to-end with real .vue files.
 */
import { describe, it, expect, beforeEach, afterEach } from "vitest";
import { existsSync, readFileSync } from "fs";
import { join } from "path";
import { unpluginFactory } from "../index";
import { resetHost } from "../core/compiler";
import { scanVueFiles } from "../core/scanner";

const COREUI_ROOT = process.env.VERTER_TEST_REPOS
  ? `${process.env.VERTER_TEST_REPOS}/coreui-free-vue-admin-template`
  : "";
const hasCoreui = existsSync(join(COREUI_ROOT, "src", "App.vue"));

describe.skipIf(!hasCoreui)("preCompile integration: coreui-free-vue-admin-template", () => {
  beforeEach(() => {
    resetHost();
  });

  afterEach(() => {
    resetHost();
  });

  // @ai-generated - scanVueFiles finds all .vue files in a real project
  it("scanVueFiles discovers .vue files in the coreui project", async () => {
    const srcRoot = join(COREUI_ROOT, "src");
    const files = await scanVueFiles(srcRoot, (f) => f.endsWith(".vue"));

    expect(files.size).toBeGreaterThan(5);

    // Verify well-known files are found
    const keys = [...files.keys()];
    expect(keys.some((k) => k.endsWith("/App.vue"))).toBe(true);

    // Verify node_modules inside src (if any) are excluded
    expect(keys.every((k) => !k.includes("node_modules"))).toBe(true);
  });

  // @ai-generated - preCompile builds all project .vue files without errors
  it("preCompile compiles all .vue files without errors", async () => {
    const origCwd = process.cwd;
    process.cwd = () => join(COREUI_ROOT, "src");
    const plugin = unpluginFactory(
      { preCompile: true },
      { framework: "rollup", versions: { unplugin: "0.0.0", rollup: "0.0.0" } } as any,
    ) as any;

    // buildStart should complete without throwing
    await plugin.buildStart();

    process.cwd = origCwd;
  });

  // @ai-generated - preCompile + transform produces same output as transform-only
  it("preCompile then transform produces same output as transform alone", async () => {
    const appVuePath = join(COREUI_ROOT, "src", "App.vue").replace(/\\/g, "/");
    const appVueSource = readFileSync(appVuePath, "utf-8");

    // First: compile without preCompile
    resetHost();
    const pluginDirect = unpluginFactory(undefined, {
      framework: "rollup",
      versions: { unplugin: "0.0.0", rollup: "0.0.0" },
    } as any) as any;
    const directResult = await pluginDirect.transform(appVueSource, appVuePath);

    // Second: compile with preCompile
    resetHost();
    const origCwd = process.cwd;
    process.cwd = () => join(COREUI_ROOT, "src");
    const pluginPre = unpluginFactory(
      { preCompile: true },
      { framework: "rollup", versions: { unplugin: "0.0.0", rollup: "0.0.0" } } as any,
    ) as any;
    await pluginPre.buildStart();
    const preResult = await pluginPre.transform(appVueSource, appVuePath);
    process.cwd = origCwd;

    // Both should produce the same compiled output
    expect(preResult).toBeDefined();
    expect(directResult).toBeDefined();
    expect(preResult.code).toBe(directResult.code);
  });

  // @ai-generated - Benchmark: preCompile timing for a real project
  it("benchmark: preCompile timing for coreui src", async () => {
    const origCwd = process.cwd;
    process.cwd = () => join(COREUI_ROOT, "src");
    const plugin = unpluginFactory(
      { preCompile: true },
      { framework: "rollup", versions: { unplugin: "0.0.0", rollup: "0.0.0" } } as any,
    ) as any;

    const start = performance.now();
    await plugin.buildStart();
    const elapsed = performance.now() - start;

    process.cwd = origCwd;

    const srcRoot = join(COREUI_ROOT, "src");
    const files = await scanVueFiles(srcRoot, (f) => f.endsWith(".vue"));

    console.log(`[benchmark] preCompile coreui (${files.size} .vue files): ${elapsed.toFixed(1)}ms (${(elapsed / files.size).toFixed(2)}ms/file)`);
    expect(elapsed).toBeGreaterThan(0);
  });
});

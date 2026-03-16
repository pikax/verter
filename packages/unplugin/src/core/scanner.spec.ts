/**
 * @ai-generated - Tests for the Vue file scanner utility.
 * Covers recursive directory walking, node_modules/dot-dir exclusion,
 * filter function, content reading, and empty directory handling.
 */
import { describe, it, expect, beforeEach, afterEach } from "vitest";
import { mkdirSync, writeFileSync, rmSync } from "fs";
import { join } from "path";
import { tmpdir } from "os";
import { scanVueFiles } from "./scanner";

function createTempDir(): string {
  const dir = join(tmpdir(), `verter-scanner-test-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`);
  mkdirSync(dir, { recursive: true });
  return dir;
}

describe("scanVueFiles", () => {
  let tempDir: string;

  beforeEach(() => {
    tempDir = createTempDir();
  });

  afterEach(() => {
    rmSync(tempDir, { recursive: true, force: true });
  });

  it("finds .vue files in root and subdirectories", async () => {
    writeFileSync(join(tempDir, "App.vue"), "<template><div/></template>");
    mkdirSync(join(tempDir, "components"), { recursive: true });
    writeFileSync(join(tempDir, "components", "Btn.vue"), "<template><button/></template>");

    const files = await scanVueFiles(tempDir, (f) => f.endsWith(".vue"));

    expect(files.size).toBe(2);
    // Keys should be forward-slash normalized absolute paths
    const keys = [...files.keys()];
    expect(keys.some((k) => k.endsWith("/App.vue"))).toBe(true);
    expect(keys.some((k) => k.endsWith("/components/Btn.vue"))).toBe(true);
  });

  it("excludes node_modules directories", async () => {
    writeFileSync(join(tempDir, "App.vue"), "<template/>");
    mkdirSync(join(tempDir, "node_modules", "some-lib"), { recursive: true });
    writeFileSync(join(tempDir, "node_modules", "some-lib", "Comp.vue"), "<template/>");

    const files = await scanVueFiles(tempDir, (f) => f.endsWith(".vue"));

    expect(files.size).toBe(1);
    const keys = [...files.keys()];
    expect(keys.some((k) => k.includes("node_modules"))).toBe(false);
  });

  it("excludes dot-directories (.git, .vite, etc.)", async () => {
    writeFileSync(join(tempDir, "App.vue"), "<template/>");
    mkdirSync(join(tempDir, ".git"), { recursive: true });
    writeFileSync(join(tempDir, ".git", "hooks.vue"), "<template/>");
    mkdirSync(join(tempDir, ".vite"), { recursive: true });
    writeFileSync(join(tempDir, ".vite", "cached.vue"), "<template/>");

    const files = await scanVueFiles(tempDir, (f) => f.endsWith(".vue"));

    expect(files.size).toBe(1);
    const keys = [...files.keys()];
    expect(keys.some((k) => k.includes(".git"))).toBe(false);
    expect(keys.some((k) => k.includes(".vite"))).toBe(false);
  });

  it("respects custom filter function", async () => {
    writeFileSync(join(tempDir, "App.vue"), "<template/>");
    writeFileSync(join(tempDir, "Page.vue"), "<template/>");
    writeFileSync(join(tempDir, "style.css"), "body{}");

    // Filter that only matches files whose basename starts with "App"
    const files = await scanVueFiles(tempDir, (f) => f.endsWith(".vue") && f.endsWith("/App.vue"));

    expect(files.size).toBe(1);
    const keys = [...files.keys()];
    expect(keys.some((k) => k.endsWith("/App.vue"))).toBe(true);
  });

  it("returns correct file contents", async () => {
    const content = "<template><div>hello</div></template>";
    writeFileSync(join(tempDir, "App.vue"), content);

    const files = await scanVueFiles(tempDir, (f) => f.endsWith(".vue"));

    expect(files.size).toBe(1);
    const value = [...files.values()][0];
    expect(value).toBe(content);
  });

  it("returns empty map for empty directory", async () => {
    const files = await scanVueFiles(tempDir, (f) => f.endsWith(".vue"));
    expect(files.size).toBe(0);
  });

  it("normalizes paths to forward slashes", async () => {
    mkdirSync(join(tempDir, "src", "views"), { recursive: true });
    writeFileSync(join(tempDir, "src", "views", "Home.vue"), "<template/>");

    const files = await scanVueFiles(tempDir, (f) => f.endsWith(".vue"));

    const keys = [...files.keys()];
    for (const key of keys) {
      expect(key).not.toContain("\\");
    }
  });
});

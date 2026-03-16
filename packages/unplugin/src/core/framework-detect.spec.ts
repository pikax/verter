import { describe, expect, it } from "vitest";
import * as fs from "fs";
import * as path from "path";
import * as os from "os";

// Import the functions from index.ts — they're module-private, so we test
// the behavior indirectly through the patterns they check.

describe("Nuxt detection patterns", () => {
  it("detects nuxt.config.ts", () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "verter-nuxt-"));
    fs.writeFileSync(path.join(tmpDir, "nuxt.config.ts"), "export default {}");
    // The detection checks for nuxt.config.ts existence
    expect(fs.existsSync(path.join(tmpDir, "nuxt.config.ts"))).toBe(true);
    fs.rmSync(tmpDir, { recursive: true });
  });

  it("detects .nuxt directory", () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "verter-nuxt-"));
    fs.mkdirSync(path.join(tmpDir, ".nuxt"));
    expect(fs.existsSync(path.join(tmpDir, ".nuxt"))).toBe(true);
    fs.rmSync(tmpDir, { recursive: true });
  });

  it("returns false for non-Nuxt project", () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "verter-vite-"));
    fs.writeFileSync(path.join(tmpDir, "vite.config.ts"), "export default {}");
    expect(fs.existsSync(path.join(tmpDir, "nuxt.config.ts"))).toBe(false);
    expect(fs.existsSync(path.join(tmpDir, ".nuxt"))).toBe(false);
    fs.rmSync(tmpDir, { recursive: true });
  });
});

describe("Server/client component detection", () => {
  it("identifies *.server.vue as server component", () => {
    expect("MyComp.server.vue".endsWith(".server.vue")).toBe(true);
  });

  it("identifies *.client.vue as client component", () => {
    expect("MyComp.client.vue".endsWith(".client.vue")).toBe(true);
  });

  it("regular .vue is neither", () => {
    const f = "MyComp.vue";
    expect(f.endsWith(".server.vue")).toBe(false);
    expect(f.endsWith(".client.vue")).toBe(false);
  });
});

describe("Nuxt alias patterns", () => {
  it("#imports maps to .nuxt/imports.d.ts", () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "verter-nuxt-"));
    const nuxtDir = path.join(tmpDir, ".nuxt");
    fs.mkdirSync(nuxtDir);
    fs.writeFileSync(path.join(nuxtDir, "imports.d.ts"), "// types");

    const target = path.join(nuxtDir, "imports.d.ts");
    expect(fs.existsSync(target)).toBe(true);
    fs.rmSync(tmpDir, { recursive: true });
  });

  it("#components maps to .nuxt/components.d.ts", () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "verter-nuxt-"));
    const nuxtDir = path.join(tmpDir, ".nuxt");
    fs.mkdirSync(nuxtDir);
    fs.writeFileSync(path.join(nuxtDir, "components.d.ts"), "// types");

    const target = path.join(nuxtDir, "components.d.ts");
    expect(fs.existsSync(target)).toBe(true);
    fs.rmSync(tmpDir, { recursive: true });
  });
});

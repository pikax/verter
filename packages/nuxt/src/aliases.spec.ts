import { describe, it, expect, beforeEach, afterEach } from "vitest";
import fs from "fs";
import path from "path";
import os from "os";
import { loadNuxtPathAliases, resolveNuxtAlias } from "./aliases";

describe("loadNuxtPathAliases", () => {
  let tmpDir: string;

  beforeEach(() => {
    tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "verter-nuxt-test-"));
  });

  afterEach(() => {
    fs.rmSync(tmpDir, { recursive: true, force: true });
  });

  it("reads .nuxt/tsconfig.json paths and resolves them absolutely", () => {
    const nuxtDir = path.join(tmpDir, ".nuxt");
    fs.mkdirSync(nuxtDir, { recursive: true });
    fs.writeFileSync(
      path.join(nuxtDir, "tsconfig.json"),
      JSON.stringify({
        compilerOptions: {
          baseUrl: "..",
          paths: {
            "#imports": ["./.nuxt/types/imports.d.ts"],
            "#ui/*": ["./node_modules/@nuxt/ui/runtime/*"],
            "#shared/*": ["./shared/*"],
          },
        },
      }),
    );

    const aliases = loadNuxtPathAliases(tmpDir);
    expect(aliases).not.toBeNull();
    expect(aliases!.has("#imports")).toBe(true);
    expect(aliases!.has("#ui/*")).toBe(true);
    expect(aliases!.has("#shared/*")).toBe(true);

    // Paths should be absolute
    const importsPath = aliases!.get("#imports")![0];
    expect(path.isAbsolute(importsPath)).toBe(true);
    expect(importsPath).toContain(".nuxt");
  });

  it("returns null when .nuxt/tsconfig.json is missing", () => {
    const aliases = loadNuxtPathAliases(tmpDir);
    expect(aliases).toBeNull();
  });

  it("handles JSONC (comments in tsconfig)", () => {
    const nuxtDir = path.join(tmpDir, ".nuxt");
    fs.mkdirSync(nuxtDir, { recursive: true });
    fs.writeFileSync(
      path.join(nuxtDir, "tsconfig.json"),
      `{
  // This is a comment
  "compilerOptions": {
    "baseUrl": "..",
    /* block comment */
    "paths": {
      "#imports": ["./.nuxt/types/imports.d.ts"]
    }
  }
}`,
    );

    const aliases = loadNuxtPathAliases(tmpDir);
    expect(aliases).not.toBeNull();
    expect(aliases!.has("#imports")).toBe(true);
  });
});

describe("resolveNuxtAlias", () => {
  let tmpDir: string;

  beforeEach(() => {
    tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "verter-nuxt-alias-"));
  });

  afterEach(() => {
    fs.rmSync(tmpDir, { recursive: true, force: true });
  });

  it("resolves exact match (#imports)", () => {
    const targetFile = path.join(tmpDir, ".nuxt", "types", "imports.d.ts");
    fs.mkdirSync(path.dirname(targetFile), { recursive: true });
    fs.writeFileSync(targetFile, "export {}");

    const aliases = new Map<string, string[]>();
    aliases.set("#imports", [targetFile]);

    const result = resolveNuxtAlias("#imports", aliases, tmpDir);
    expect(result).toBe(targetFile);
  });

  it("resolves wildcard match (#ui/components/Button.vue)", () => {
    const buttonFile = path.join(tmpDir, "runtime", "components", "Button.vue");
    fs.mkdirSync(path.dirname(buttonFile), { recursive: true });
    fs.writeFileSync(buttonFile, "<template><button/></template>");

    const aliases = new Map<string, string[]>();
    aliases.set("#ui/*", [path.join(tmpDir, "runtime") + "/*"]);

    const result = resolveNuxtAlias("#ui/components/Button.vue", aliases, tmpDir);
    expect(result).toBe(buttonFile);
  });

  it("resolves wildcard match with extension guessing (.ts)", () => {
    const utilsFile = path.join(tmpDir, "shared", "utils.ts");
    fs.mkdirSync(path.dirname(utilsFile), { recursive: true });
    fs.writeFileSync(utilsFile, "export const x = 1;");

    const aliases = new Map<string, string[]>();
    aliases.set("#shared/*", [path.join(tmpDir, "shared") + "/*"]);

    const result = resolveNuxtAlias("#shared/utils", aliases, tmpDir);
    expect(result).toBe(utilsFile);
  });

  it("resolves wildcard match with index file", () => {
    const indexFile = path.join(tmpDir, "shared", "auth", "index.ts");
    fs.mkdirSync(path.dirname(indexFile), { recursive: true });
    fs.writeFileSync(indexFile, "export const login = () => {};");

    const aliases = new Map<string, string[]>();
    aliases.set("#shared/*", [path.join(tmpDir, "shared") + "/*"]);

    const result = resolveNuxtAlias("#shared/auth", aliases, tmpDir);
    expect(result).toBe(indexFile);
  });

  it("returns null for unknown alias (negative)", () => {
    const aliases = new Map<string, string[]>();
    aliases.set("#imports", [path.join(tmpDir, "imports.d.ts")]);

    const result = resolveNuxtAlias("#nonexistent", aliases, tmpDir);
    expect(result).toBeNull();
  });

  it("returns null for unknown sub-path under valid alias (negative)", () => {
    const aliases = new Map<string, string[]>();
    aliases.set("#shared/*", [path.join(tmpDir, "shared") + "/*"]);

    const result = resolveNuxtAlias("#shared/nonexistent/Nope", aliases, tmpDir);
    expect(result).toBeNull();
  });

  it("falls back to hardcoded aliases when map is null", () => {
    // Create .nuxt directory with expected files for hardcoded fallback
    const nuxtDir = path.join(tmpDir, ".nuxt");
    fs.mkdirSync(nuxtDir, { recursive: true });
    fs.writeFileSync(path.join(nuxtDir, "imports.d.ts"), "export {}");

    const result = resolveNuxtAlias("#imports", null, tmpDir);
    expect(result).toBe(path.join(nuxtDir, "imports.d.ts"));
  });

  it("hardcoded fallback returns null for unknown alias", () => {
    const result = resolveNuxtAlias("#unknown", null, tmpDir);
    expect(result).toBeNull();
  });
});

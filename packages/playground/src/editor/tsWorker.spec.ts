import { describe, it, expect } from "vitest";
import { resolvePath, resolveVueModulePath } from "./vueModuleResolver";

describe("resolvePath", () => {
  it("resolves ./Foo relative to /", () => {
    expect(resolvePath("/", "./Foo.vue.ts")).toBe("/Foo.vue.ts");
  });

  it("resolves ./Foo relative to /src/", () => {
    expect(resolvePath("/src/", "./Foo.vue.ts")).toBe("/src/Foo.vue.ts");
  });

  it("resolves ../Foo from nested directory", () => {
    expect(resolvePath("/src/components/", "../Foo.vue.ts")).toBe("/src/Foo.vue.ts");
  });

  it("resolves ./components/Bar from root", () => {
    expect(resolvePath("/", "./components/Bar.vue.ts")).toBe("/components/Bar.vue.ts");
  });

  it("resolves multiple .. segments", () => {
    expect(resolvePath("/a/b/c/", "../../Foo.vue.ts")).toBe("/a/Foo.vue.ts");
  });

  it("resolves . segment (current dir)", () => {
    expect(resolvePath("/src/", ".")).toBe("/src");
  });

  it("does not go above root", () => {
    expect(resolvePath("/", "../Foo.vue.ts")).toBe("/Foo.vue.ts");
  });
});

describe("resolveVueModulePath", () => {
  const makeFileExists = (existingFiles: string[]) => (path: string) =>
    existingFiles.includes(path);

  it("resolves ./Foo.vue.ts → /Foo.vue.d.ts (primary fix)", () => {
    const fileExists = makeFileExists(["/Foo.vue.d.ts"]);
    expect(resolveVueModulePath("./Foo.vue.ts", "/App.vue.tsx", fileExists)).toBe("/Foo.vue.d.ts");
  });

  it("resolves ./Foo.vue → /Foo.vue.d.ts (backward compat)", () => {
    const fileExists = makeFileExists(["/Foo.vue.d.ts"]);
    expect(resolveVueModulePath("./Foo.vue", "/App.vue.tsx", fileExists)).toBe("/Foo.vue.d.ts");
  });

  it("resolves ../Foo.vue.ts from nested containing file", () => {
    const fileExists = makeFileExists(["/Foo.vue.d.ts"]);
    expect(resolveVueModulePath("../Foo.vue.ts", "/components/App.vue.tsx", fileExists)).toBe(
      "/Foo.vue.d.ts",
    );
  });

  it("resolves ./components/Bar.vue.ts → /components/Bar.vue.d.ts", () => {
    const fileExists = makeFileExists(["/components/Bar.vue.d.ts"]);
    expect(resolveVueModulePath("./components/Bar.vue.ts", "/App.vue.tsx", fileExists)).toBe(
      "/components/Bar.vue.d.ts",
    );
  });

  it("returns null for non-vue imports like 'vue'", () => {
    const fileExists = makeFileExists([]);
    expect(resolveVueModulePath("vue", "/App.vue.tsx", fileExists)).toBeNull();
  });

  it("returns null for non-vue relative imports like ./utils.ts", () => {
    const fileExists = makeFileExists([]);
    expect(resolveVueModulePath("./utils.ts", "/App.vue.tsx", fileExists)).toBeNull();
  });

  it("returns null for non-relative .vue imports", () => {
    const fileExists = makeFileExists([]);
    expect(resolveVueModulePath("@/components/Foo.vue", "/App.vue.tsx", fileExists)).toBeNull();
  });

  it("returns null when .d.ts file does not exist", () => {
    const fileExists = makeFileExists([]); // no files
    expect(resolveVueModulePath("./Foo.vue.ts", "/App.vue.tsx", fileExists)).toBeNull();
  });

  it("returns null for bare .vue.ts without relative prefix", () => {
    const fileExists = makeFileExists(["/Foo.vue.d.ts"]);
    expect(resolveVueModulePath("Foo.vue.ts", "/App.vue.tsx", fileExists)).toBeNull();
  });
});

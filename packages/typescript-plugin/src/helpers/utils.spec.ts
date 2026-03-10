import { describe, it, expect } from "vitest";
import {
  getVueVirtualFileInfo,
  isLikelyTestFileName,
  isRelativeVue,
  isRelativeVueTs,
  isVue,
  isVueTs,
  resolveVuePublicApiMode,
} from "./utils";

describe("isVue", () => {
  it("matches .vue suffix", () => {
    expect(isVue("./Foo.vue")).toBe(true);
    expect(isVue("../components/Bar.vue")).toBe(true);
    expect(isVue("Comp.vue")).toBe(true);
  });
  it("does not match .vue.ts", () => {
    expect(isVue("./Foo.vue.ts")).toBe(false);
  });
  it("does not match .vue.d.ts", () => {
    expect(isVue("./Foo.vue.d.ts")).toBe(false);
  });
  it("does not match .ts", () => {
    expect(isVue("./Foo.ts")).toBe(false);
  });
});

describe("isRelativeVue", () => {
  it("matches relative .vue", () => {
    expect(isRelativeVue("./Foo.vue")).toBe(true);
    expect(isRelativeVue("../Foo.vue")).toBe(true);
  });
  it("does not match non-relative .vue", () => {
    expect(isRelativeVue("@/Foo.vue")).toBe(false);
    expect(isRelativeVue("vue")).toBe(false);
  });
});

describe("isVueTs", () => {
  it("matches .vue.ts suffix", () => {
    expect(isVueTs("./Foo.vue.ts")).toBe(true);
    expect(isVueTs("../components/Bar.vue.ts")).toBe(true);
    expect(isVueTs("Comp.vue.ts")).toBe(true);
  });
  it("does not match plain .vue", () => {
    expect(isVueTs("./Foo.vue")).toBe(false);
  });
  it("does not match plain .ts", () => {
    expect(isVueTs("./Foo.ts")).toBe(false);
  });
  it("does not match .vue.d.ts", () => {
    expect(isVueTs("./Foo.vue.d.ts")).toBe(false);
  });
});

describe("isRelativeVueTs", () => {
  it("matches relative .vue.ts", () => {
    expect(isRelativeVueTs("./Foo.vue.ts")).toBe(true);
    expect(isRelativeVueTs("../Foo.vue.ts")).toBe(true);
  });
  it("does not match non-relative .vue.ts", () => {
    expect(isRelativeVueTs("@/Foo.vue.ts")).toBe(false);
    expect(isRelativeVueTs("vue.vue.ts")).toBe(false);
  });
});

describe("getVueVirtualFileInfo", () => {
  it("parses the public virtual suffixes", () => {
    expect(getVueVirtualFileInfo("/src/Foo.vue.ts")).toEqual({
      sourceFileName: "/src/Foo.vue",
      mode: "public",
    });
    expect(getVueVirtualFileInfo("/src/Foo.vue.d.ts")).toEqual({
      sourceFileName: "/src/Foo.vue",
      mode: "public",
    });
  });

  it("parses the testing virtual suffix", () => {
    expect(getVueVirtualFileInfo("/src/Foo.vue.__verter_test.ts")).toEqual({
      sourceFileName: "/src/Foo.vue",
      mode: "testing",
    });
  });

  it("returns null for non-virtual paths", () => {
    expect(getVueVirtualFileInfo("/src/Foo.vue")).toBeNull();
    expect(getVueVirtualFileInfo("/src/Foo.ts")).toBeNull();
  });
});

describe("isLikelyTestFileName", () => {
  it("matches common spec and test file names", () => {
    expect(isLikelyTestFileName("/src/App.spec.ts")).toBe(true);
    expect(isLikelyTestFileName("/src/App.test.tsx")).toBe(true);
    expect(isLikelyTestFileName("/src/__tests__/App.ts")).toBe(true);
  });

  it("does not flag normal source files", () => {
    expect(isLikelyTestFileName("/src/App.ts")).toBe(false);
    expect(isLikelyTestFileName("/src/components/Foo.vue")).toBe(false);
  });
});

describe("resolveVuePublicApiMode", () => {
  it("stays public when testing bindings are disabled", () => {
    expect(
      resolveVuePublicApiMode(false, "/src/App.spec.ts", () => true),
    ).toBe("public");
  });

  it("stays public for non-test importers when testing bindings are enabled", () => {
    expect(
      resolveVuePublicApiMode(true, "/src/components/App.vue", () => false),
    ).toBe("public");
  });

  it("switches to testing for test importers when enabled", () => {
    expect(
      resolveVuePublicApiMode(
        true,
        "/src/__tests__/App.spec.ts",
        (fileName) => fileName === "/src/__tests__/App.spec.ts",
      ),
    ).toBe("testing");
  });
});

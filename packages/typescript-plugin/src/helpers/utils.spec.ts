import { describe, it, expect } from "vitest";
import { isVue, isRelativeVue, isVueTs, isRelativeVueTs } from "./utils";

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

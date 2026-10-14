import { describe, it, expect } from "vitest";
import {
  computeEngineKey,
  dirnamePath,
  normalizePath,
  resolvePath,
  inlineConfigHash,
  stableHash,
  tsconfigConfigHash,
} from "./engine-key.js";
import type { EngineKeyInput } from "./engine-key.js";

describe("normalizePath", () => {
  it("converts backslashes to forward slashes", () => {
    expect(normalizePath("C:\\Users\\foo\\bar")).toBe("c:/Users/foo/bar");
  });

  it("lowercases Windows drive letter", () => {
    expect(normalizePath("D:/dev/project")).toBe("d:/dev/project");
  });

  it("strips trailing slash", () => {
    expect(normalizePath("/home/user/project/")).toBe("/home/user/project");
  });

  it("passes through already-normalized Unix paths", () => {
    expect(normalizePath("/home/user/project")).toBe("/home/user/project");
  });
});

describe("resolvePath", () => {
  it("resolves Windows-style roots consistently on non-Windows hosts", () => {
    expect(resolvePath("C:\\project", "src\\App.vue")).toBe("c:/project/src/App.vue");
  });

  it("resolves Unix-style roots normally", () => {
    expect(resolvePath("/project", "src/App.vue")).toBe("/project/src/App.vue");
  });
});

describe("dirnamePath", () => {
  it("returns a normalized dirname for Windows-style paths", () => {
    expect(dirnamePath("C:\\project\\tsconfig.json")).toBe("c:/project");
  });
});

describe("stableHash", () => {
  it("produces same hash for same input", () => {
    const input = { a: 1, b: "hello" };
    expect(stableHash(input)).toBe(stableHash(input));
  });

  it("is stable regardless of key order", () => {
    const a = { x: 1, y: 2 };
    const b = { y: 2, x: 1 };
    expect(stableHash(a)).toBe(stableHash(b));
  });

  it("is stable regardless of nested key order", () => {
    const a = { outer: { x: 1, y: 2 }, list: [{ b: 2, a: 1 }] };
    const b = { list: [{ a: 1, b: 2 }], outer: { y: 2, x: 1 } };
    expect(stableHash(a)).toBe(stableHash(b));
  });

  it("produces different hash for different input", () => {
    expect(stableHash({ a: 1 })).not.toBe(stableHash({ a: 2 }));
  });
});

describe("computeEngineKey", () => {
  const base: EngineKeyInput = {
    backend: "napi",
    root: "D:\\dev\\project",
    configKind: "tsconfig",
    tsconfigPath: "D:\\dev\\project\\tsconfig.json",
    configHash: "abc123",
    nativeFlags: {
      analysisLevel: "full",
    },
  };

  it("produces deterministic keys", () => {
    expect(computeEngineKey(base)).toBe(computeEngineKey(base));
  });

  it("normalizes paths in the key", () => {
    const withBackslash = { ...base, root: "D:\\dev\\project" };
    const withForward = { ...base, root: "d:/dev/project" };
    expect(computeEngineKey(withBackslash)).toBe(computeEngineKey(withForward));
  });

  it("different config hash produces different key", () => {
    const other = { ...base, configHash: "xyz789" };
    expect(computeEngineKey(base)).not.toBe(computeEngineKey(other));
  });

  it("different backend produces different key", () => {
    const other: EngineKeyInput = { ...base, backend: "wasm" };
    expect(computeEngineKey(base)).not.toBe(computeEngineKey(other));
  });

  it("different analysis levels produce different key", () => {
    const other = {
      ...base,
      nativeFlags: { ...base.nativeFlags, analysisLevel: "lite" },
    };
    expect(computeEngineKey(base)).not.toBe(computeEngineKey(other));
  });

  it("different audit flags produce different key", () => {
    const withoutAudit = {
      ...base,
      nativeFlags: { ...base.nativeFlags, auditEnabled: false },
    };
    const withAudit = {
      ...base,
      nativeFlags: { ...base.nativeFlags, auditEnabled: true },
    };

    expect(computeEngineKey(withoutAudit)).not.toBe(computeEngineKey(withAudit));
  });
});

describe("inlineConfigHash", () => {
  it("ignores include: an inline project is match-all, include only filters discovery", () => {
    const hashA = inlineConfigHash({
      include: ["src/A.vue"],
      compilerOptions: { baseUrl: "." },
    });
    const hashB = inlineConfigHash({
      include: ["src/B.vue"],
      compilerOptions: { baseUrl: "." },
    });

    expect(hashA).toBe(hashB);
  });

  it("still changes when analysis-affecting config changes", () => {
    const hashA = inlineConfigHash({
      compilerOptions: { baseUrl: "." },
    });
    const hashB = inlineConfigHash({
      compilerOptions: { baseUrl: "./src" },
    });

    expect(hashA).not.toBe(hashB);
  });
});

describe("tsconfigConfigHash", () => {
  it("distinguishes the order of a tsconfig's files: they are the program's roots", () => {
    const forward = tsconfigConfigHash({ files: ["a.ts", "b.ts"] });
    const reverse = tsconfigConfigHash({ files: ["b.ts", "a.ts"] });

    expect(forward).not.toBe(reverse);
  });

  it("distinguishes include and exclude", () => {
    const base = { compilerOptions: { baseUrl: "." }, include: ["src/**/*.vue"] };

    expect(tsconfigConfigHash(base)).not.toBe(
      tsconfigConfigHash({ ...base, include: ["src/**/*.ts", "src/**/*.vue"] }),
    );
    expect(tsconfigConfigHash(base)).not.toBe(
      tsconfigConfigHash({ ...base, exclude: ["src/legacy"] }),
    );
  });
});

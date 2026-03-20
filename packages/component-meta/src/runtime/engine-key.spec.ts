import { describe, it, expect } from "vitest";
import { computeEngineKey, normalizePath, stableHash, getWorkspaceIdentity } from "./engine-key.js";
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
      deepMacroResolutionType: true,
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

  it("different analysis flags produce different key", () => {
    const other = {
      ...base,
      nativeFlags: { ...base.nativeFlags, deepMacroResolutionType: false },
    };
    expect(computeEngineKey(base)).not.toBe(computeEngineKey(other));
  });

  it("workspace identity is included when present", () => {
    const withWs = { ...base, workspaceIdentity: "ws-1" };
    const withWs2 = { ...base, workspaceIdentity: "ws-2" };
    expect(computeEngineKey(withWs)).not.toBe(computeEngineKey(withWs2));
  });
});

describe("getWorkspaceIdentity", () => {
  it("returns same identity for same object", () => {
    const ws = {};
    expect(getWorkspaceIdentity(ws)).toBe(getWorkspaceIdentity(ws));
  });

  it("returns different identity for different objects", () => {
    const ws1 = {};
    const ws2 = {};
    expect(getWorkspaceIdentity(ws1)).not.toBe(getWorkspaceIdentity(ws2));
  });

  it("identity format is ws-<number>", () => {
    const ws = {};
    expect(getWorkspaceIdentity(ws)).toMatch(/^ws-\d+$/);
  });
});

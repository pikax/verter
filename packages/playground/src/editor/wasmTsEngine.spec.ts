/**
 * Guards for the hermetic pinned-TS6 engine + libs: the
 * in-context worker bundles `typescript@6.0.3` and its `lib.*.d.ts` set from
 * local `node_modules` — the CDN engine/lib loading is DELETED, IndexedDB is
 * a runtime cache only, keyed by the TS version string — plus the fail-closed
 * `capabilityForWasm` gate (in-context LS serves TS<7 only).
 */
import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import ts from "typescript";
import playgroundPkg from "../../package.json";
import tsWorkerSource from "./tsWorker.ts?raw";
import { bundledWorkerFiles, libCacheKey } from "./tsLibs";
import { capabilityForWasm, createGatedInContextLanguageService, tsMajorOf } from "./inContextLs";

const thisDir = dirname(fileURLToPath(import.meta.url));

describe("wasm_ts_lib_version_skew (#7)", () => {
  it("the engine is the pinned local typescript@6 devDependency — no version skew", () => {
    expect(playgroundPkg.devDependencies.typescript).toBe("6.0.3");
    expect(ts.version).toBe(playgroundPkg.devDependencies.typescript);
    expect(tsMajorOf(ts.version)).toBe(6);
  });

  it("the worker has NO CDN engine/lib path and imports the bundled typescript statically", () => {
    expect(tsWorkerSource).not.toContain("cdn.jsdelivr");
    expect(tsWorkerSource).not.toContain("typescript@5");
    expect(tsWorkerSource).not.toContain("TS_CDN_BASE");
    // The engine arrives as a static bundled import, not a dynamic URL import.
    expect(tsWorkerSource).toMatch(/from\s+"typescript"/);
    expect(tsWorkerSource).not.toMatch(/import\(\s*["']https?:/);
  });

  it("the bundled lib set IS the pinned package's lib.*.d.ts set", () => {
    const files = bundledWorkerFiles();
    const libNames = [...files.keys()].filter((name) =>
      name.startsWith("/node_modules/typescript/lib/lib."),
    );
    expect(libNames.length).toBeGreaterThan(50);

    // Byte-parity with the pinned package on a representative pair.
    const realLibDir = resolve(thisDir, "../../node_modules/typescript/lib");
    for (const lib of ["lib.es5.d.ts", "lib.esnext.full.d.ts"]) {
      const bundled = files.get(`/node_modules/typescript/lib/${lib}`);
      expect(bundled, lib).toBeDefined();
      expect(bundled).toBe(readFileSync(resolve(realLibDir, lib), "utf8"));
    }
  });

  it("the IndexedDB lib cache is keyed by the TS version string", () => {
    const key = libCacheKey(ts.version, "lib.es5.d.ts");
    expect(key).toContain(ts.version);
    expect(key).toContain("lib.es5.d.ts");
    // A version bump changes EVERY key — stale engine libs can never serve.
    expect(libCacheKey("7.0.0", "lib.es5.d.ts")).not.toBe(key);
  });
});

describe("capabilityForWasm fail-closed gate", () => {
  it("serves the in-context LS for TS<7 only", () => {
    expect(capabilityForWasm(6)).toEqual({ inContextLS: true, tsgo: false });
    expect(capabilityForWasm(7)).toEqual({ inContextLS: false, tsgo: false });
    expect(capabilityForWasm(8)).toEqual({ inContextLS: false, tsgo: false });
    expect(tsMajorOf("7.0.1-rc")).toBe(7);
    // The shipped pin keeps the gate OPEN.
    expect(capabilityForWasm(tsMajorOf(ts.version)).inContextLS).toBe(true);
  });

  it("never invokes produce when the gate is closed: createLanguageService is not touched for TS>=7", () => {
    // A trap facade: reading ANY member except `version` throws — proving
    // the gated factory checks capability BEFORE touching the engine.
    const trapTs = new Proxy(
      { version: "7.0.1-rc" },
      {
        get(target, prop) {
          if (prop === "version") return target.version;
          throw new Error(`gate leak: ts.${String(prop)} accessed while gate is closed`);
        },
      },
    );
    const gated = createGatedInContextLanguageService({
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      ts: trapTs as any,
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      store: null as any,
      userFiles: new Map(),
      currentDirectory: "/",
    });
    expect(gated).toBeNull();
  });
});

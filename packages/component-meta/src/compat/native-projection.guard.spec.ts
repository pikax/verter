/**
 * Guard tests for the declared-only native-projection surface.
 *
 * These tests assert the public contract of `@verter/component-meta/compat`:
 *
 * - `getDeclaredComponentMeta` is fully absent from the project session
 *   surface (instance, prototype, and TS interface declaration).
 * - The two declared-only projection helpers are reachable through the
 *   published `@verter/component-meta/compat` subpath barrel and visible
 *   in the regenerated `dist/compat/{index.d.ts,index.js}` artifacts.
 * - Helper return types are correctly typed at the type level.
 * - The decoded helper plus the existing mappers produce a Volar-shaped
 *   result equivalent to the legacy declared-only path.
 */

import { existsSync, readFileSync } from "node:fs";
import { resolve } from "node:path";

import { describe, expect, expectTypeOf, it } from "vitest";

import type { NativeComponentMetaResult } from "../native-component-meta.js";
import { ProjectEngine } from "../runtime/project-engine.js";
import { ProjectSession } from "../runtime/project-session.js";
import { encodeTestComponentMetaPayload } from "../type-graph.test-utils.js";
import { mapComponentMeta } from "./checker.js";
import {
  nativeComponentMetaToComponentMeta,
  nativeTypeRegistryToMap,
} from "../native-component-meta.js";

import { projectDeclaredOnlyFromNativePayload, projectDeclaredOnlyNativeResult } from "./index.js";

function createMockNativeProject(overrides: Record<string, unknown> = {}) {
  return {
    upsertBase() {},
    ensureLoaded() {
      return false;
    },
    refreshBase() {
      return false;
    },
    configureProjects() {},
    openSession() {
      throw new Error("not used");
    },
    clearCaches() {},
    shutdown() {},
    get isShutdown() {
      return false;
    },
    get sessionCount() {
      return 1;
    },
    baseFileIds() {
      return [];
    },
    ...overrides,
  };
}

function createMockNativeSession(overrides: Record<string, unknown> = {}) {
  return {
    upsert() {},
    delete() {},
    reset() {},
    getEffectiveSource() {
      return "<template />" as string | null;
    },
    hasFile() {
      return true;
    },
    trackedFileIds() {
      return [];
    },
    close() {},
    get isClosed() {
      return false;
    },
    get overlayGeneration() {
      return 0;
    },
    getComponentMeta() {
      return null;
    },
    getProvenance() {
      return "{}";
    },
    ...overrides,
  };
}

const compatDistDir = resolve(import.meta.dirname, "..", "..", "dist", "compat");

describe("session_surface_has_no_declared_component_meta", () => {
  it("ProjectSession instance does not expose getDeclaredComponentMeta", () => {
    const nativeProject = createMockNativeProject();
    const nativeSession = createMockNativeSession();
    const engine = new ProjectEngine("engine", "/project", nativeProject as any);
    const session = new ProjectSession(engine, "lease-1", nativeSession as any);

    expect((session as Record<string, unknown>).getDeclaredComponentMeta).toBeUndefined();
  });

  it("ProjectSession prototype does not expose getDeclaredComponentMeta", () => {
    const proto = Object.getPrototypeOf(
      new ProjectSession(
        new ProjectEngine("engine", "/project", createMockNativeProject() as any),
        "lease-1",
        createMockNativeSession() as any,
      ),
    );
    expect("getDeclaredComponentMeta" in proto).toBe(false);
  });

  it("ProjectSession TS surface does not declare getDeclaredComponentMeta", () => {
    const session = new ProjectSession(
      new ProjectEngine("engine", "/project", createMockNativeProject() as any),
      "lease-1",
      createMockNativeSession() as any,
    );
    type SessionShape = typeof session;
    expectTypeOf<SessionShape>().not.toHaveProperty("getDeclaredComponentMeta");
  });
});

describe("compat_projection_imports_via_published_export_map", () => {
  it("@verter/component-meta/compat barrel re-exports both helpers as functions", () => {
    expect(typeof projectDeclaredOnlyNativeResult).toBe("function");
    expect(typeof projectDeclaredOnlyFromNativePayload).toBe("function");
  });

  it("decoded helper is typed (NativeComponentMetaResult | null) -> NativeComponentMetaResult | null", () => {
    expectTypeOf(projectDeclaredOnlyNativeResult).parameters.toEqualTypeOf<
      [NativeComponentMetaResult | null]
    >();
    expectTypeOf(
      projectDeclaredOnlyNativeResult,
    ).returns.toEqualTypeOf<NativeComponentMetaResult | null>();
  });

  it("buffer helper is typed (Buffer | null) -> NativeComponentMetaResult | null", () => {
    expectTypeOf(projectDeclaredOnlyFromNativePayload).parameters.toEqualTypeOf<[Buffer | null]>();
    expectTypeOf(
      projectDeclaredOnlyFromNativePayload,
    ).returns.toEqualTypeOf<NativeComponentMetaResult | null>();
  });

  it("both helpers null-pass when fed null", () => {
    expect(projectDeclaredOnlyNativeResult(null)).toBeNull();
    expect(projectDeclaredOnlyFromNativePayload(null)).toBeNull();
  });

  it.runIf(existsSync(resolve(compatDistDir, "index.js")))(
    "regenerated dist/compat/index.js contains both helper exports",
    () => {
      const compatJs = readFileSync(resolve(compatDistDir, "index.js"), "utf8");
      expect(compatJs).toContain("projectDeclaredOnlyNativeResult");
      expect(compatJs).toContain("projectDeclaredOnlyFromNativePayload");
    },
  );

  it.runIf(existsSync(resolve(compatDistDir, "index.d.ts")))(
    "regenerated dist/compat/index.d.ts declares both helpers",
    () => {
      const compatDts = readFileSync(resolve(compatDistDir, "index.d.ts"), "utf8");
      expect(compatDts).toContain("projectDeclaredOnlyNativeResult");
      expect(compatDts).toContain("projectDeclaredOnlyFromNativePayload");
    },
  );
});

describe("compat_projection_round_trips_through_existing_mappers", () => {
  it("decoded helper + nativeComponentMetaToComponentMeta + mapComponentMeta produces Volar shape", () => {
    const payload = encodeTestComponentMetaPayload({
      filePath: "/project/src/Button.vue",
      props: [
        {
          name: "label",
          type: { kind: "primitive", name: "string" },
          rawType: "string",
          required: true,
          hasDefault: false,
        },
      ],
      slots: [
        {
          name: "default",
          isScoped: false,
          bindings: [],
          isRequired: false,
        },
      ],
    });

    const decoded = projectDeclaredOnlyFromNativePayload(payload);
    expect(decoded).not.toBeNull();
    if (!decoded) {
      throw new Error("decoded must be non-null");
    }

    const typeRegistry = nativeTypeRegistryToMap(decoded);
    const mapped = nativeComponentMetaToComponentMeta(decoded);
    const volar = mapComponentMeta(mapped, undefined, typeRegistry);

    expect(volar.props.map((prop) => prop.name)).toEqual(["label"]);
    expect(volar.slots.map((slot) => slot.name)).toEqual(["default"]);
    // _verter sidecar carries the underlying ComponentMeta shape.
    expect(volar._verter?.filePath).toBe("/project/src/Button.vue");
  });
});

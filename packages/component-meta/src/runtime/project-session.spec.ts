/**
 * @ai-generated - Verifies ProjectSession decodes binary component-meta payloads from the native session.
 */

import { describe, expect, it, vi } from "vitest";

import { ProjectEngine } from "./project-engine.js";
import { ProjectSession } from "./project-session.js";
import { encodeTestComponentMetaPayload } from "../type-graph.test-utils.js";
import { nativeComponentMetaToComponentMeta } from "../native-component-meta.js";

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

describe("ProjectSession", () => {
  it("decodes Buffer payloads from the native session instead of JSON strings", () => {
    const payload = encodeTestComponentMetaPayload({
      filePath: "/project/src/Button.vue",
      props: [{ name: "label", type: { kind: "primitive", name: "string" }, required: true }],
      slots: [{ name: "default", returnType: "VNode[]" }],
    });
    const nativeProject = createMockNativeProject();
    const nativeSession = createMockNativeSession({
      getComponentMeta() {
        return payload;
      },
    });
    const engine = new ProjectEngine("engine", "/project", nativeProject as any);
    const session = new ProjectSession(engine, "lease-1", nativeSession as any);

    const native = session.getComponentMeta("/project/src/Button.vue");
    const compat = nativeComponentMetaToComponentMeta(native as any);

    expect(compat.props[0]?.type).toEqual({ kind: "primitive", name: "string" });
    expect(compat.slots[0]?.returnType).toBe("VNode[]");
  });

  it("decodes resolved native payloads through a dedicated session method", () => {
    const resolvedPayload = encodeTestComponentMetaPayload({
      filePath: "/project/src/Button.vue",
      props: [
        { name: "resolvedLabel", type: { kind: "primitive", name: "string" }, required: true },
      ],
    });
    const nativeProject = createMockNativeProject();
    const nativeSession = createMockNativeSession({
      getResolvedComponentMeta() {
        return resolvedPayload;
      },
    });
    const engine = new ProjectEngine("engine", "/project", nativeProject as any);
    const session = new ProjectSession(engine, "lease-1", nativeSession as any);

    const native = session.getResolvedComponentMeta("/project/src/Button.vue") as any;

    expect(native?.props[0]?.name).toBe("resolvedLabel");
  });

  it("does not fall back to the plain native query when resolved metadata is unavailable", () => {
    const payload = encodeTestComponentMetaPayload({
      filePath: "/project/src/Button.vue",
      props: [{ name: "label", type: { kind: "primitive", name: "string" }, required: true }],
    });
    const getComponentMeta = vi.fn(() => payload);
    const nativeProject = createMockNativeProject();
    const nativeSession = createMockNativeSession({
      getComponentMeta,
    });
    const engine = new ProjectEngine("engine", "/project", nativeProject as any);
    const session = new ProjectSession(engine, "lease-1", nativeSession as any);

    expect(() => session.getResolvedComponentMeta("/project/src/Button.vue")).toThrow(
      /resolved component-meta query/i,
    );
    expect(getComponentMeta).not.toHaveBeenCalled();
  });

  describe("decoded-result memo", () => {
    it("repeated getComponentMeta decodes only once", () => {
      const payload = encodeTestComponentMetaPayload({
        filePath: "/project/src/Button.vue",
        props: [{ name: "label", type: { kind: "primitive", name: "string" }, required: true }],
      });
      const getComponentMeta = vi.fn(() => payload);
      const nativeProject = createMockNativeProject();
      const nativeSession = createMockNativeSession({ getComponentMeta });
      const engine = new ProjectEngine("engine", "/project", nativeProject as any);
      const session = new ProjectSession(engine, "lease-1", nativeSession as any);

      const result1 = session.getComponentMeta("/project/src/Button.vue");
      const result2 = session.getComponentMeta("/project/src/Button.vue");

      expect(getComponentMeta).toHaveBeenCalledTimes(1);
      expect(result1).toBe(result2);
    });

    it("refreshBaseFile bumps baseGeneration and forces re-decode", () => {
      const payload = encodeTestComponentMetaPayload({
        filePath: "/project/src/Button.vue",
        props: [{ name: "label", type: { kind: "primitive", name: "string" }, required: true }],
      });
      const getComponentMeta = vi.fn(() => payload);
      const nativeProject = createMockNativeProject({
        refreshBase() {
          return true;
        },
      });
      const nativeSession = createMockNativeSession({ getComponentMeta });
      const engine = new ProjectEngine("engine", "/project", nativeProject as any);
      const session = new ProjectSession(engine, "lease-1", nativeSession as any);

      // First decode
      session.getComponentMeta("/project/src/Button.vue");
      expect(getComponentMeta).toHaveBeenCalledTimes(1);

      const genBefore = engine.baseGeneration;
      session.refreshBaseFile("other.vue");
      expect(engine.baseGeneration).toBeGreaterThan(genBefore);

      // Second decode after base change — must call native again
      session.getComponentMeta("/project/src/Button.vue");
      expect(getComponentMeta).toHaveBeenCalledTimes(2);
    });

    it("ensureBaseFile bumps baseGeneration when loaded, does not when already loaded", () => {
      let loadCount = 0;
      const nativeProject = createMockNativeProject({
        ensureLoaded() {
          loadCount++;
          // First call returns true (newly loaded), second returns false (already loaded)
          return loadCount <= 1;
        },
      });
      const nativeSession = createMockNativeSession();
      const engine = new ProjectEngine("engine", "/project", nativeProject as any);
      const session = new ProjectSession(engine, "lease-1", nativeSession as any);

      const gen0 = engine.baseGeneration;
      session.ensureBaseFile("a.vue");
      expect(engine.baseGeneration).toBe(gen0 + 1);

      const gen1 = engine.baseGeneration;
      session.ensureBaseFile("b.vue");
      expect(engine.baseGeneration).toBe(gen1);
    });

    it("clearCaches bumps baseGeneration", () => {
      const nativeProject = createMockNativeProject();
      const engine = new ProjectEngine("engine", "/project", nativeProject as any);

      const genBefore = engine.baseGeneration;
      engine.clearCaches();
      expect(engine.baseGeneration).toBeGreaterThan(genBefore);
    });

    it("overlay upsert invalidates memo for that session only", () => {
      const payload = encodeTestComponentMetaPayload({
        filePath: "/project/src/Button.vue",
        props: [{ name: "label", type: { kind: "primitive", name: "string" }, required: true }],
      });
      const getMeta1 = vi.fn(() => payload);
      const getMeta2 = vi.fn(() => payload);
      const nativeProject = createMockNativeProject({
        openSession() {
          throw new Error("not used");
        },
      });
      const nativeSession1 = createMockNativeSession({
        getComponentMeta: getMeta1,
      });
      const nativeSession2 = createMockNativeSession({
        getComponentMeta: getMeta2,
      });
      const engine = new ProjectEngine("engine", "/project", nativeProject as any);
      const session1 = new ProjectSession(engine, "lease-1", nativeSession1 as any);
      const session2 = new ProjectSession(engine, "lease-2", nativeSession2 as any);

      // Both sessions decode once
      session1.getComponentMeta("/project/src/Button.vue");
      session2.getComponentMeta("/project/src/Button.vue");
      expect(getMeta1).toHaveBeenCalledTimes(1);
      expect(getMeta2).toHaveBeenCalledTimes(1);

      // session1 overlay upsert invalidates session1's memo
      session1.upsert("test.vue", "new source");

      // session1 must re-decode
      session1.getComponentMeta("/project/src/Button.vue");
      expect(getMeta1).toHaveBeenCalledTimes(2);

      // session2 should still use its memo (no overlay change, no base change)
      session2.getComponentMeta("/project/src/Button.vue");
      expect(getMeta2).toHaveBeenCalledTimes(1);
    });

    it("restoreBaseFile bumps baseGeneration when it reloads base content", () => {
      const payload = encodeTestComponentMetaPayload({
        filePath: "/project/src/Button.vue",
        props: [{ name: "label", type: { kind: "primitive", name: "string" }, required: true }],
      });
      const getMeta1 = vi.fn(() => payload);
      const getMeta2 = vi.fn(() => payload);
      const nativeProject = createMockNativeProject({
        ensureLoaded() {
          return true;
        },
      });
      const nativeSession1 = createMockNativeSession({
        getComponentMeta: getMeta1,
        getEffectiveSource(canonicalId: string) {
          return canonicalId === "/project/src/Base.vue" ? null : "<template />";
        },
      });
      const nativeSession2 = createMockNativeSession({
        getComponentMeta: getMeta2,
      });
      const engine = new ProjectEngine("engine", "/project", nativeProject as any);
      const session1 = new ProjectSession(engine, "lease-1", nativeSession1 as any);
      const session2 = new ProjectSession(engine, "lease-2", nativeSession2 as any);

      session2.getComponentMeta("/project/src/Button.vue");
      expect(getMeta2).toHaveBeenCalledTimes(1);

      const genBefore = engine.baseGeneration;
      session1.restoreBaseFile("/project/src/Base.vue");
      expect(engine.baseGeneration).toBeGreaterThan(genBefore);

      session2.getComponentMeta("/project/src/Button.vue");
      expect(getMeta2).toHaveBeenCalledTimes(2);
    });

    it("frozen memoized result prevents mutation", () => {
      const payload = encodeTestComponentMetaPayload({
        filePath: "/project/src/Button.vue",
        props: [{ name: "label", type: { kind: "primitive", name: "string" }, required: true }],
      });
      const nativeProject = createMockNativeProject();
      const nativeSession = createMockNativeSession({
        getComponentMeta() {
          return payload;
        },
      });
      const engine = new ProjectEngine("engine", "/project", nativeProject as any);
      const session = new ProjectSession(engine, "lease-1", nativeSession as any);

      const result = session.getComponentMeta("/project/src/Button.vue") as any;

      expect(Object.isFrozen(result)).toBe(true);
      expect(Object.isFrozen(result.props)).toBe(true);
      expect(Object.isFrozen(result.props[0])).toBe(true);

      // Attempting to mutate should throw in strict mode
      expect(() => {
        "use strict";
        result.props = [];
      }).toThrow();
    });
  });
});

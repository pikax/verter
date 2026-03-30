/**
 * @ai-generated - Verifies ProjectSession decodes binary component-meta payloads from the native session.
 */

import { describe, expect, it } from "vitest";

import { ProjectEngine } from "./project-engine.js";
import { ProjectSession } from "./project-session.js";
import { encodeTestComponentMetaPayload } from "../type-graph.test-utils.js";
import { nativeComponentMetaToComponentMeta } from "../native-component-meta.js";

describe("ProjectSession", () => {
  it("decodes Buffer payloads from the native session instead of JSON strings", () => {
    const payload = encodeTestComponentMetaPayload({
      filePath: "/project/src/Button.vue",
      props: [{ name: "label", type: { kind: "primitive", name: "string" }, required: true }],
      slots: [{ name: "default", returnType: "VNode[]" }],
    });
    const nativeProject = {
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
    };
    const nativeSession = {
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
        return payload;
      },
      getDeclaredComponentMeta() {
        return payload;
      },
      getProvenance() {
        return "{}";
      },
    };
    const engine = new ProjectEngine("engine", "/project", nativeProject as any);
    const session = new ProjectSession(engine, "lease-1", nativeSession as any);

    const native = session.getComponentMeta("/project/src/Button.vue");
    const compat = nativeComponentMetaToComponentMeta(native as any);

    expect(compat.props[0]?.type).toEqual({ kind: "primitive", name: "string" });
    expect(compat.slots[0]?.returnType).toBe("VNode[]");
  });
});

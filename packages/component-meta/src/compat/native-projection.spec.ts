/**
 * Tests for the declared-only native-projection helpers exposed by
 * `@verter/component-meta/compat`. The helpers project a fully resolved
 * `NativeComponentMetaResult` (or its raw NAPI Buffer payload) down to
 * the declared-only surface used by compat output.
 *
 * Both helpers MUST return `NativeComponentMetaResult | null` — NEVER a
 * Volar shape. Volar mapping stays at the caller via
 * `mapComponentMeta(options, typeRegistry)`.
 */

import { describe, expect, it, vi } from "vitest";

import type { NativeComponentMetaResult } from "../native-component-meta.js";
import {
  projectDeclaredOnlyFromNativePayload,
  projectDeclaredOnlyNativeResult,
} from "./native-projection.js";
import { encodeTestComponentMetaPayload } from "../type-graph.test-utils.js";

function fullNativeMeta(): NativeComponentMetaResult {
  return {
    filePath: "/project/src/Button.vue",
    optionsApi: false,
    props: [
      {
        name: "label",
        type: { kind: "primitive", name: "string" },
        required: true,
        hasDefault: false,
      } as any,
    ],
    events: [
      {
        name: "click",
        payload: { kind: "primitive", name: "void" },
      } as any,
    ],
    slots: [
      {
        name: "default",
        isScoped: false,
        bindings: [],
        isRequired: false,
      } as any,
    ],
    models: [],
    exposed: [
      {
        name: "focus",
        type: { kind: "primitive", name: "void" },
      } as any,
    ],
    components: [],
    templateRefs: [],
    imports: [],
    bindings: [],
    vueApiCalls: [],
    styles: [],
    flags: {
      asyncSetup: false,
      hasReactiveState: false,
      hasComputed: false,
      hasWatchers: false,
      hasLifecycleHooks: false,
      hasProvide: false,
      hasInject: false,
      hasInheritAttrsFalse: false,
      hasStoreUsage: false,
    },
    acceptedProps: [
      {
        name: "label",
        type: { kind: "primitive", name: "string" },
        required: true,
        provenance: { kind: "declared" },
        availability: { kind: "always" },
        kind: "declaredProp",
      } as any,
      {
        name: "id",
        type: { kind: "primitive", name: "string" },
        required: false,
        provenance: { kind: "inherited", sources: [{ kind: "nativeTag", tag: "div" }] },
        availability: { kind: "always" },
        kind: "attr",
      } as any,
    ],
    acceptedEvents: [
      {
        name: "click",
        payload: { kind: "primitive", name: "void" },
        provenance: { kind: "declared" },
        availability: { kind: "always" },
        kind: "declaredEmit",
      } as any,
      {
        name: "focus",
        payload: { kind: "primitive", name: "void" },
        provenance: { kind: "inherited", sources: [{ kind: "nativeTag", tag: "div" }] },
        availability: { kind: "always" },
        kind: "listener",
      } as any,
    ],
    acceptedSurfaceCompleteness: "exact",
    rootReachability: { kind: "noFallthrough", reason: "noTemplate" },
    fallthroughSurface: {
      kind: "branches",
      branches: [
        {
          branchKey: "b0",
          props: [],
          events: [],
          rootChain: [],
          status: { kind: "resolved" },
        } as any,
      ],
    },
  };
}

describe("projectDeclaredOnlyNativeResult", () => {
  it("returns null when fed null", () => {
    expect(projectDeclaredOnlyNativeResult(null)).toBeNull();
  });

  it("returns NativeComponentMetaResult shape (NOT Volar) with declared-only surface", () => {
    const meta = fullNativeMeta();
    const result = projectDeclaredOnlyNativeResult(meta);

    expect(result).not.toBeNull();
    // Shape check: native fields preserved at top level (not Volar's `props/events/slots/exposed`-only).
    expect(result?.filePath).toBe("/project/src/Button.vue");
    expect(result?.flags).toBeDefined();

    // Declared surface preserved.
    expect(result?.props.map((p) => p.name)).toEqual(["label"]);
    expect(result?.events.map((e) => e.name)).toEqual(["click"]);
    expect(result?.slots.map((s) => s.name)).toEqual(["default"]);
    expect(result?.exposed.map((e) => e.name)).toEqual(["focus"]);

    // Inherited members dropped from acceptedProps / acceptedEvents.
    expect(result?.acceptedProps.map((p) => p.name)).toEqual(["label"]);
    expect(result?.acceptedEvents.map((e) => e.name)).toEqual(["click"]);

    // Fallthrough surface reset.
    expect(result?.fallthroughSurface.kind).toBe("none");
  });

  it("does not invoke nativeComponentMetaToComponentMeta or mapComponentMeta", async () => {
    const native = await import("../native-component-meta.js");
    const compat = await import("./checker.js");
    const nativeSpy = vi.spyOn(native, "nativeComponentMetaToComponentMeta");
    const mapSpy = vi.spyOn(compat, "mapComponentMeta");

    try {
      projectDeclaredOnlyNativeResult(fullNativeMeta());
      expect(nativeSpy).not.toHaveBeenCalled();
      expect(mapSpy).not.toHaveBeenCalled();
    } finally {
      nativeSpy.mockRestore();
      mapSpy.mockRestore();
    }
  });
});

describe("projectDeclaredOnlyFromNativePayload", () => {
  it("returns null when fed null", () => {
    expect(projectDeclaredOnlyFromNativePayload(null)).toBeNull();
  });

  it("decodes the buffer then runs the declared-only projection", async () => {
    const payload = encodeTestComponentMetaPayload({
      filePath: "/project/src/Button.vue",
      props: [{ name: "label", type: { kind: "primitive", name: "string" }, required: true }],
    });

    const decodeMod = await import("../type-graph.js");
    const decodeSpy = vi.spyOn(decodeMod, "decodeComponentMetaPayload");

    try {
      const result = projectDeclaredOnlyFromNativePayload(payload);

      // Decode happens (buffer route).
      expect(decodeSpy).toHaveBeenCalledTimes(1);
      // Result has been projected to declared-only shape.
      expect(result).not.toBeNull();
      expect(result?.filePath).toBe("/project/src/Button.vue");
      // fallthroughSurface forced to "none" by the declared-only projection.
      expect(result?.fallthroughSurface.kind).toBe("none");
      // No inherited entries leaked through.
      expect(result?.acceptedProps.every((p) => p.provenance.kind === "declared")).toBe(true);
      expect(result?.acceptedEvents.every((e) => e.provenance.kind === "declared")).toBe(true);
    } finally {
      decodeSpy.mockRestore();
    }
  });

  it("does not invoke nativeComponentMetaToComponentMeta or mapComponentMeta", async () => {
    const native = await import("../native-component-meta.js");
    const compat = await import("./checker.js");
    const nativeSpy = vi.spyOn(native, "nativeComponentMetaToComponentMeta");
    const mapSpy = vi.spyOn(compat, "mapComponentMeta");

    try {
      const payload = encodeTestComponentMetaPayload({
        filePath: "/project/src/Button.vue",
        props: [{ name: "label", type: { kind: "primitive", name: "string" }, required: true }],
      });
      projectDeclaredOnlyFromNativePayload(payload);

      expect(nativeSpy).not.toHaveBeenCalled();
      expect(mapSpy).not.toHaveBeenCalled();
    } finally {
      nativeSpy.mockRestore();
      mapSpy.mockRestore();
    }
  });
});

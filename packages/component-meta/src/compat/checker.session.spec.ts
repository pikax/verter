/**
 * @ai-generated - Guards the compat checker against falling back to the removed JS semantic pipeline.
 */

import { describe, expect, it, vi } from "vitest";
import { ComponentMetaChecker } from "./checker.js";

function resolvedTypeRow(display: string) {
  return {
    publication: {
      kind: "published",
      semanticAuthority: "resolved",
      exactness: "exactConcrete",
      reason: { kind: "resolvedExactConcrete" },
      provenance: { kind: "resolved", value: "semanticEvaluator" },
    } as const,
    terminalDisplay: { text: display },
  };
}

describe("ComponentMetaChecker session requirement", () => {
  it("rejects adapter-only getComponentMeta calls instead of rebuilding metadata from snapshots", async () => {
    const checker = new ComponentMetaChecker(
      {
        upsert: vi.fn(),
      },
      "/project",
    );

    checker.updateFile(
      "App.vue",
      `<script setup lang="ts">defineProps<{ label: string }>()</script><template><div /></template>`,
    );

    await expect(checker.getComponentMeta("App.vue")).rejects.toThrow(/runtime session/i);
  });

  it("normalizes session-backed canonical ids before querying canonical native metadata", async () => {
    const getComponentMeta = vi.fn(() => ({
      filePath: "C:/project/src/App.vue",
      optionsApi: false,
      props: [],
      events: [],
      slots: [],
      models: [],
      exposed: [],
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
      acceptedProps: [],
      acceptedEvents: [],
      acceptedSurfaceCompleteness: "exact",
      rootReachability: { kind: "noFallthrough", reason: "noTemplate" },
      fallthroughSurface: { kind: "none", reason: "noTemplate" },
      orderedSfcStructure: {
        schemaVersion: 1,
        artifactToken: "a".repeat(43),
        blocks: [],
        markupNodes: [],
      },
    }));

    const checker = new ComponentMetaChecker(
      {
        upsert: vi.fn(),
      },
      "C:\\project",
      {},
      {
        closed: false,
        engine: { state: "active" as const },
        upsert() {},
        delete() {},
        getComponentMeta,
        getProvenance() {
          return "{}";
        },
        getEffectiveSource() {
          return `<script setup lang="ts">defineProps<{ label: string }>()</script>`;
        },
        hasFile() {
          return true;
        },
        trackedFileIds() {
          return [];
        },
        close() {},
      } as any,
    );

    checker.updateFile(
      "src\\App.vue",
      `<script setup lang="ts">defineProps<{ label: string }>()</script><template><div /></template>`,
    );
    await checker.getComponentMeta("src\\App.vue");

    expect(getComponentMeta).toHaveBeenCalledWith("c:/project/src/App.vue");
  });

  // @ai-generated - Proves checker batch dispatch stays one native call and preserves positional slots.
  it("dispatches one positional native batch without scalar component-meta calls", async () => {
    const nativeMeta = (filePath: string, propName: string): any => ({
      filePath,
      optionsApi: false,
      props: [
        {
          name: propName,
          type: { kind: "primitive", name: "string" },
          ...resolvedTypeRow("string"),
          required: true,
          hasDefault: false,
        },
      ],
      events: [],
      slots: [],
      models: [],
      exposed: [],
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
      acceptedProps: [],
      acceptedEvents: [],
      acceptedSurfaceCompleteness: "exact",
      rootReachability: { kind: "noFallthrough", reason: "noTemplate" },
      fallthroughSurface: { kind: "none", reason: "noTemplate" },
      orderedSfcStructure: {
        schemaVersion: 1,
        artifactToken: "a".repeat(43),
        blocks: [],
        markupNodes: [],
      },
    });
    const getComponentMeta = vi.fn();
    const getComponentMetaBatch = vi.fn(() => [
      nativeMeta("/project/src/Zebra.vue", "first"),
      null,
      nativeMeta("/project/src/Alpha.vue", "third"),
    ]);
    const getEffectiveSource = vi.fn(() => `<script setup lang="ts"></script>`);
    const checker = new ComponentMetaChecker({ upsert: vi.fn() }, "/project", {}, {
      closed: false,
      engine: { state: "active" as const },
      upsert() {},
      delete() {},
      getComponentMeta,
      getComponentMetaBatch,
      getProvenance() {
        return "{}";
      },
      getEffectiveSource,
      hasFile() {
        return true;
      },
      trackedFileIds() {
        return [];
      },
      close() {},
    } as any);

    // Deliberately not lexicographically sorted: the call assertions must catch
    // an implementation that reorders inputs while keeping the missing slot in the middle.
    const result = await checker.getComponentMetaBatch([
      "src/Zebra.vue",
      "src/Missing.vue",
      "src/Alpha.vue",
    ]);

    expect(getComponentMetaBatch).toHaveBeenCalledTimes(1);
    expect(getComponentMetaBatch).toHaveBeenCalledWith([
      "/project/src/Zebra.vue",
      "/project/src/Missing.vue",
      "/project/src/Alpha.vue",
    ]);
    expect(getComponentMeta).toHaveBeenCalledTimes(0);
    expect(getEffectiveSource).toHaveBeenCalledTimes(3);
    expect(getEffectiveSource.mock.calls).toEqual([
      ["/project/src/Zebra.vue"],
      ["/project/src/Missing.vue"],
      ["/project/src/Alpha.vue"],
    ]);
    expect(result.map((meta) => meta.props.map((prop) => prop.name))).toEqual([
      ["first"],
      [],
      ["third"],
    ]);
    expect(result[1]).toEqual({
      type: 0,
      props: [],
      events: [],
      slots: [],
      exposed: [],
    });
  });

  it("propagates native component-meta budget errors to callers", async () => {
    const checker = new ComponentMetaChecker(
      {
        upsert: vi.fn(),
      },
      "/project",
      {},
      {
        closed: false,
        engine: { state: "active" as const },
        upsert() {},
        delete() {},
        getResolvedComponentMeta() {
          throw new Error(
            "component-meta external type resolution step budget exceeded (maxSteps=2000)",
          );
        },
        getComponentMeta() {
          throw new Error(
            "component-meta external type resolution step budget exceeded (maxSteps=2000)",
          );
        },
        getProvenance() {
          return "{}";
        },
        getEffectiveSource() {
          return `<script setup lang="ts">defineProps<{ label: string }>()</script>`;
        },
        hasFile() {
          return true;
        },
        trackedFileIds() {
          return [];
        },
        close() {},
      } as any,
    );

    checker.updateFile(
      "App.vue",
      `<script setup lang="ts">defineProps<{ label: string }>()</script><template><div /></template>`,
    );

    await expect(checker.getComponentMeta("App.vue")).rejects.toThrow(/step budget exceeded/i);
  });

  // @ai-generated - Proves a native per-id batch failure rejects the whole compat batch.
  it("propagates native component-meta batch budget errors to callers", async () => {
    const checker = new ComponentMetaChecker({ upsert: vi.fn() }, "/project", {}, {
      closed: false,
      engine: { state: "active" as const },
      upsert() {},
      delete() {},
      getComponentMeta: vi.fn(),
      getComponentMetaBatch() {
        throw new Error(
          "component-meta external type resolution step budget exceeded (maxSteps=2000)",
        );
      },
      getProvenance() {
        return "{}";
      },
      getEffectiveSource() {
        return `<script setup lang="ts">defineProps<{ label: string }>()</script>`;
      },
      hasFile() {
        return true;
      },
      trackedFileIds() {
        return [];
      },
      close() {},
    } as any);

    await expect(checker.getComponentMetaBatch(["src/App.vue"])).rejects.toThrow(
      /step budget exceeded/i,
    );
  });

  it("uses one canonical native query for public Verter compat output and _verter", async () => {
    const fullMeta: any = {
      filePath: "C:/project/src/App.vue",
      optionsApi: false,
      props: [
        {
          name: "label",
          type: { kind: "primitive", name: "string" },
          ...resolvedTypeRow("string"),
          required: true,
          hasDefault: false,
        },
      ],
      events: [],
      slots: [],
      models: [],
      exposed: [],
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
          ...resolvedTypeRow("string"),
          required: true,
          provenance: { kind: "declared" },
          availability: { kind: "always" },
          kind: "declaredProp",
        },
        {
          name: "id",
          type: { kind: "primitive", name: "string" },
          ...resolvedTypeRow("string"),
          required: false,
          provenance: { kind: "inherited", sources: [{ kind: "nativeTag", tag: "div" }] },
          availability: { kind: "always" },
          kind: "attr",
        },
      ],
      acceptedEvents: [],
      acceptedSurfaceCompleteness: "exact",
      rootReachability: { kind: "noFallthrough", reason: "noTemplate" },
      fallthroughSurface: { kind: "none", reason: "noTemplate" },
      orderedSfcStructure: {
        schemaVersion: 1,
        artifactToken: "a".repeat(43),
        blocks: [],
        markupNodes: [],
      },
    };
    const getComponentMeta = vi.fn(() => fullMeta);

    const checker = new ComponentMetaChecker(
      {
        upsert: vi.fn(),
      },
      "C:\\project",
      {},
      {
        closed: false,
        engine: { state: "active" as const },
        upsert() {},
        delete() {},
        getComponentMeta,
        getProvenance() {
          return "{}";
        },
        getEffectiveSource() {
          return `<script setup lang="ts">defineProps<{ label: string }>()</script>`;
        },
        hasFile() {
          return true;
        },
        trackedFileIds() {
          return [];
        },
        close() {},
      } as any,
    );

    checker.updateFile(
      "src\\App.vue",
      `<script setup lang="ts">defineProps<{ label: string }>()</script><template><div /></template>`,
    );

    const meta = await checker.getComponentMeta("src\\App.vue");

    expect(getComponentMeta).toHaveBeenCalledTimes(1);
    expect(getComponentMeta).toHaveBeenCalledWith("c:/project/src/App.vue");
    expect(meta.props.map((prop) => prop.name)).toEqual(["label"]);
    // Inherited members were dropped by the declared-only projection.
    expect(meta._verter?.acceptedProps?.map((p: any) => p.name)).toEqual(["label"]);
    expect(meta._verter?.acceptedSurfaceCompleteness).toBe("exact");
  });

  // @ai-generated - Proves terminal display text cannot rewrite a structurally rendered schema.
  it("keeps Booleanish terminal display text from rewriting a non-Booleanish schema", async () => {
    const nativeMeta: any = {
      filePath: "C:/project/src/App.vue",
      optionsApi: false,
      props: [
        {
          name: "decoy",
          type: { kind: "primitive", name: "string" },
          ...resolvedTypeRow("Booleanish"),
          required: true,
          hasDefault: false,
        },
      ],
      events: [],
      slots: [],
      models: [],
      exposed: [],
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
      acceptedProps: [],
      acceptedEvents: [],
      acceptedSurfaceCompleteness: "exact",
      rootReachability: { kind: "noFallthrough", reason: "noTemplate" },
      fallthroughSurface: { kind: "none", reason: "noTemplate" },
      orderedSfcStructure: {
        schemaVersion: 1,
        artifactToken: "a".repeat(43),
        blocks: [],
        markupNodes: [],
      },
    };
    const checker = new ComponentMetaChecker({ upsert: vi.fn() }, "C:\\project", {}, {
      closed: false,
      engine: { state: "active" as const },
      upsert() {},
      delete() {},
      getComponentMeta() {
        return nativeMeta;
      },
      getProvenance() {
        return "{}";
      },
      getEffectiveSource() {
        return `<script setup lang="ts">defineProps<{ decoy: string }>()</script>`;
      },
      hasFile() {
        return true;
      },
      trackedFileIds() {
        return [];
      },
      close() {},
    } as any);

    checker.updateFile(
      "src\\App.vue",
      `<script setup lang="ts">defineProps<{ decoy: string }>()</script>`,
    );

    const meta = await checker.getComponentMeta("src\\App.vue");

    expect(meta.props[0]).toMatchObject({
      name: "decoy",
      type: "Booleanish",
      schema: "string",
    });
  });
});

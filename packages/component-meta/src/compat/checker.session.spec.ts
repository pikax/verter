/**
 * @ai-generated - Guards the compat checker against falling back to the removed JS semantic pipeline.
 */

import { describe, expect, it, vi } from "vitest";
import { ComponentMetaChecker } from "./checker.js";

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

  it("normalizes session-backed canonical ids before querying declared native metadata", async () => {
    const getDeclaredComponentMeta = vi.fn(() => ({
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
        getDeclaredComponentMeta,
        getComponentMeta() {
          throw new Error("full native query should not be used for default Verter compat");
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
      "src\\App.vue",
      `<script setup lang="ts">defineProps<{ label: string }>()</script><template><div /></template>`,
    );
    await checker.getComponentMeta("src\\App.vue");

    expect(getDeclaredComponentMeta).toHaveBeenCalledWith("c:/project/src/App.vue");
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
        getDeclaredComponentMeta() {
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

  it("uses one declared native query for Verter compat output and _verter", async () => {
    const declaredMeta: any = {
      filePath: "C:/project/src/App.vue",
      optionsApi: false,
      props: [
        {
          name: "label",
          type: { kind: "primitive", name: "string" },
          rawType: "string",
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
          name: "id",
          type: { kind: "primitive", name: "string" },
          rawType: "string",
          required: false,
          provenance: { kind: "inherited", sources: [{ kind: "nativeTag", tag: "div" }] },
          availability: { kind: "always" },
          kind: "attr",
        },
      ],
      acceptedEvents: [],
      acceptedSurfaceCompleteness: "exact",
      rootReachability: { kind: "branches", branches: [] },
      fallthroughSurface: { kind: "branches", branches: [] },
    };
    const getDeclaredComponentMeta = vi.fn(() => declaredMeta);
    const getComponentMeta = vi.fn(() => {
      throw new Error("full native query should not be used for default Verter compat");
    });

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
        getDeclaredComponentMeta,
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

    expect(getDeclaredComponentMeta).toHaveBeenCalledTimes(1);
    expect(getDeclaredComponentMeta).toHaveBeenCalledWith("c:/project/src/App.vue");
    expect(getComponentMeta).not.toHaveBeenCalled();
    expect(meta.props.map((prop) => prop.name)).toEqual(["label"]);
    expect(meta._verter?.acceptedProps.map((prop) => prop.name)).toEqual(["id"]);
    expect(meta._verter?.acceptedSurfaceCompleteness).toBe("exact");
  });

  it("checks declared native metadata first, then retries once with the full native query on non-Verter backends", async () => {
    const declaredMeta: any = {
      filePath: "C:/project/src/App.vue",
      optionsApi: false,
      props: [
        {
          name: "label",
          type: { kind: "primitive", name: "string" },
          rawType: "string",
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
      acceptedSurfaceCompleteness: "lowerBound",
      rootReachability: { kind: "noFallthrough", reason: "noTemplate" },
      fallthroughSurface: { kind: "none", reason: "noTemplate" },
    };
    const fullMeta: any = {
      ...declaredMeta,
      props: [
        ...declaredMeta.props,
        {
          name: "collapsible",
          type: { kind: "primitive", name: "boolean" },
          rawType: "boolean | undefined",
          required: false,
          hasDefault: true,
          defaultValue: "true",
        },
      ],
    };
    const getDeclaredComponentMeta = vi.fn(() => declaredMeta);
    const getComponentMeta = vi.fn(() => fullMeta);

    const checker = new ComponentMetaChecker(
      {
        upsert: vi.fn(),
      },
      "C:\\project",
      {
        typeExpansionBackend: "tsserver",
      },
      {
        closed: false,
        engine: { state: "active" as const },
        upsert() {},
        delete() {},
        getComponentMeta,
        getDeclaredComponentMeta,
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

    expect(getDeclaredComponentMeta).toHaveBeenCalledTimes(1);
    expect(getDeclaredComponentMeta).toHaveBeenCalledWith("c:/project/src/App.vue");
    expect(getComponentMeta).toHaveBeenCalledWith("c:/project/src/App.vue");
    expect(meta.props.map((prop) => prop.name)).toEqual(["label", "collapsible"]);
  });

  it("retries one full native query on non-Verter backends and keeps the richer result", async () => {
    const partialMeta: any = {
      filePath: "C:/project/src/App.vue",
      optionsApi: false,
      props: [
        {
          name: "label",
          type: { kind: "primitive", name: "string" },
          rawType: "string",
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
      acceptedSurfaceCompleteness: "lowerBound",
      rootReachability: { kind: "noFallthrough", reason: "noTemplate" },
      fallthroughSurface: { kind: "none", reason: "noTemplate" },
    };
    const richerMeta: any = {
      ...partialMeta,
      props: [
        ...partialMeta.props,
        {
          name: "collapsible",
          type: { kind: "primitive", name: "boolean" },
          rawType: "boolean | undefined",
          required: false,
          hasDefault: true,
          defaultValue: "true",
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
    };
    const getDeclaredComponentMeta = vi.fn(() => partialMeta);
    const getComponentMeta = vi.fn(() => richerMeta);

    const checker = new ComponentMetaChecker(
      {
        upsert: vi.fn(),
      },
      "C:\\project",
      {
        typeExpansionBackend: "tsserver",
      },
      {
        closed: false,
        engine: { state: "active" as const },
        upsert() {},
        delete() {},
        getDeclaredComponentMeta,
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

    expect(getDeclaredComponentMeta).toHaveBeenCalledTimes(1);
    expect(getComponentMeta).toHaveBeenCalledTimes(1);
    expect(meta.props.map((prop) => prop.name)).toEqual(["label", "collapsible"]);
    expect(meta.slots.map((slot) => slot.name)).toEqual(["default"]);
  });
});

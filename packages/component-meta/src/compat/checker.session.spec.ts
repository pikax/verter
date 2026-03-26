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

  it("normalizes session-backed canonical ids before querying native metadata", async () => {
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

  it("falls back to a session overlay when ensureBaseFile declines a readable workspace file", async () => {
    const store = new Map<string, string>();
    const checker = new ComponentMetaChecker(
      {
        upsert: vi.fn(),
      },
      "/project",
      {},
      {
        closed: false,
        engine: { state: "active" as const, markActivity() {} },
        upsert(canonicalId: string, source: string) {
          store.set(canonicalId, source);
        },
        delete() {},
        getComponentMeta(canonicalId: string) {
          if (!store.has(canonicalId)) {
            return null;
          }
          return {
            filePath: canonicalId,
            optionsApi: false,
            props: [
              {
                name: "label",
                required: false,
                type: { kind: "string" },
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
          };
        },
        getEffectiveSource(canonicalId: string) {
          return store.get(canonicalId);
        },
        hasFile(canonicalId: string) {
          return store.has(canonicalId);
        },
        trackedFileIds() {
          return Array.from(store.keys());
        },
        ensureBaseFile() {
          return false;
        },
        close() {},
      } as any,
      {
        readFile: vi
          .fn()
          .mockResolvedValue(
            `<script setup lang="ts">defineProps<{ label: string }>()</script><template><div /></template>`,
          ),
        fileExists: vi.fn().mockResolvedValue(true),
        isDir: vi.fn().mockResolvedValue(false),
        readDir: vi.fn().mockResolvedValue([]),
        walk: vi.fn().mockResolvedValue([]),
        configureProjects: vi.fn(),
      },
    );

    const meta = await checker.getComponentMeta("src/App.vue");

    expect(meta.props.map((prop) => prop.name)).toContain("label");
    expect(store.size).toBe(1);
    expect(Array.from(store.keys())[0]).toContain("/project/src/App.vue");
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
        getComponentMeta() {
          throw new Error(
            "component-meta external type resolution step budget exceeded (maxSteps=2000)",
          );
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

  it("uses declared component metadata when available and fetches full _verter lazily", async () => {
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
      acceptedSurfaceCompleteness: "exact",
      rootReachability: { kind: "branches", branches: [] },
      fallthroughSurface: { kind: "branches", branches: [] },
    };
    const getDeclaredComponentMeta = vi.fn(() => declaredMeta);
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
        getDeclaredComponentMeta,
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

    expect(getDeclaredComponentMeta).toHaveBeenCalledWith("c:/project/src/App.vue");
    expect(getComponentMeta).not.toHaveBeenCalled();
    expect(meta.props.map((prop) => prop.name)).toEqual(["label"]);

    expect(meta._verter?.acceptedProps.map((prop) => prop.name)).toEqual(["id"]);
    expect(getComponentMeta).toHaveBeenCalledWith("c:/project/src/App.vue");
  });
});

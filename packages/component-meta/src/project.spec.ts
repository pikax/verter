import { resolve } from "node:path";
import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { ComponentMetaSession, evictComponentMetaSession, shutdownMetaRuntime } from "./project.js";
import { getMetaRuntime } from "./runtime/index.js";
import type { NativeMetaProject, NativeMetaSession } from "./runtime/index.js";

function nativeMetaPayload(filePath: string) {
  return {
    filePath,
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
    acceptedSurfaceCompleteness: "exact",
    rootReachability: { kind: "noFallthrough", reason: "noTemplate" },
    fallthroughSurface: { kind: "none", reason: "noTemplate" },
  };
}

/** Mock native session with in-memory file tracking. */
function createMockSession(baseFiles: Map<string, string>): NativeMetaSession {
  const overlays = new Map<string, string | null>();
  let closed = false;
  let gen = 0;

  return {
    upsert(id: string, source: string | Buffer) {
      overlays.set(id, String(source));
      gen++;
    },
    delete(id: string) {
      overlays.set(id, null);
      gen++;
    },
    reset(id: string) {
      overlays.delete(id);
      gen++;
    },
    getDeclaredComponentMeta(id: string) {
      const overlay = overlays.get(id);
      if (overlay === null) return null;
      const source = overlay ?? baseFiles.get(id);
      if (!source) return null;
      return JSON.stringify(nativeMetaPayload(id));
    },
    getProvenance() {
      return JSON.stringify({});
    },
    getComponentMeta(id: string) {
      const overlay = overlays.get(id);
      if (overlay === null) return null; // tombstoned
      const source = overlay ?? baseFiles.get(id);
      if (!source) return null;
      return JSON.stringify(nativeMetaPayload(id));
    },
    getEffectiveSource(id: string) {
      if (overlays.has(id)) {
        const v = overlays.get(id);
        return v === null ? null : v;
      }
      return baseFiles.get(id) ?? null;
    },
    hasFile(id: string) {
      if (overlays.has(id)) return overlays.get(id) !== null;
      return baseFiles.has(id);
    },
    trackedFileIds() {
      const ids = new Set([...baseFiles.keys()]);
      for (const [k, v] of overlays) {
        if (v === null) ids.delete(k);
        else ids.add(k);
      }
      return [...ids];
    },
    close() {
      closed = true;
    },
    get isClosed() {
      return closed;
    },
    get overlayGeneration() {
      return gen;
    },
  };
}

function createMockProject(baseFiles: Map<string, string>): NativeMetaProject {
  let _shutdown = false;
  return {
    upsertBase(id: string, source: string | Buffer) {
      baseFiles.set(id, String(source));
    },
    ensureLoaded(id: string) {
      return baseFiles.has(id);
    },
    refreshBase(id: string) {
      return baseFiles.has(id);
    },
    configureProjects() {},
    setHtmlIntrinsicsCatalog() {},
    openSession: () => createMockSession(baseFiles),
    clearCaches() {},
    shutdown() {
      _shutdown = true;
    },
    get isShutdown() {
      return _shutdown;
    },
    get sessionCount() {
      return 0;
    },
    baseFileIds: () => [...baseFiles.keys()],
  };
}

describe("ComponentMetaSession public API", () => {
  beforeEach(() => {
    shutdownMetaRuntime();
  });
  afterEach(() => {
    shutdownMetaRuntime();
  });

  it("repeated openComponentMetaSession shares one engine", async () => {
    const runtime = getMetaRuntime();
    const baseFiles = new Map<string, string>();
    const bootstrap = async () => ({
      nativeProject: createMockProject(baseFiles),
      baseFileIds: [],
    });

    const input = {
      backend: "napi" as const,
      root: "/test",
      configKind: "tsconfig" as const,
      configHash: "h1",
      tsconfigPath: "/test/tsconfig.json",
      nativeFlags: { analysisLevel: "full" },
    };

    const engine1 = await runtime.getOrCreateEngine(input, bootstrap);
    const engine2 = await runtime.getOrCreateEngine(input, bootstrap);
    expect(engine1).toBe(engine2);
  });

  it("dropping one handle does not break another", async () => {
    const runtime = getMetaRuntime();
    const baseFiles = new Map([["A.vue", "<template>A</template>"]]);
    const bootstrap = async () => ({
      nativeProject: createMockProject(baseFiles),
      baseFileIds: ["A.vue"],
    });

    const input = {
      backend: "napi" as const,
      root: "/test",
      configKind: "inline" as const,
      configHash: "h2",
      nativeFlags: { analysisLevel: "full" },
    };

    const engine = await runtime.getOrCreateEngine(input, bootstrap);
    const s1 = runtime.openSession(engine);
    const s2 = runtime.openSession(engine);

    const p1 = new ComponentMetaSession(s1, "/test");
    const p2 = new ComponentMetaSession(s2, "/test");

    p1.close();
    expect(s1.closed).toBe(true);

    // p2 should still work
    expect(() => p2.updateFile("A.vue", "modified")).not.toThrow();
    p2.close();
  });

  it("close is optional and idempotent", async () => {
    const runtime = getMetaRuntime();
    const bootstrap = async () => ({
      nativeProject: createMockProject(new Map()),
      baseFileIds: [],
    });

    const input = {
      backend: "napi" as const,
      root: "/test",
      configKind: "inline" as const,
      configHash: "h3",
      nativeFlags: { analysisLevel: "full" },
    };

    const engine = await runtime.getOrCreateEngine(input, bootstrap);
    const session = runtime.openSession(engine);
    const project = new ComponentMetaSession(session, "/test");

    project.close();
    project.close(); // idempotent

    expect(() => project.updateFile("A.vue", "x")).toThrow("ComponentMetaSession is closed");
  });

  it("shutdownMetaRuntime invalidates all handles", async () => {
    const runtime = getMetaRuntime();
    const bootstrap = async () => ({
      nativeProject: createMockProject(new Map()),
      baseFileIds: [],
    });

    const input = {
      backend: "napi" as const,
      root: "/test",
      configKind: "inline" as const,
      configHash: "h4",
      nativeFlags: { analysisLevel: "full" },
    };

    const engine = await runtime.getOrCreateEngine(input, bootstrap);
    const session = runtime.openSession(engine);
    const project = new ComponentMetaSession(session, "/test");

    shutdownMetaRuntime();
    expect(() => project.updateFile("A.vue", "x")).toThrow();
  });

  it("getExportNames always returns ['default']", async () => {
    const runtime = getMetaRuntime();
    const bootstrap = async () => ({
      nativeProject: createMockProject(new Map()),
      baseFileIds: [],
    });

    const input = {
      backend: "napi" as const,
      root: "/test",
      configKind: "inline" as const,
      configHash: "h6",
      nativeFlags: { analysisLevel: "full" },
    };

    const engine = await runtime.getOrCreateEngine(input, bootstrap);
    const session = runtime.openSession(engine);
    const project = new ComponentMetaSession(session, "/test");

    const names = await project.getExportNames("anything.vue");
    expect(names).toEqual(["default"]);
    project.close();
  });

  it("uses the native component-meta query instead of rebuilding from session analysis helpers", async () => {
    const session = {
      closed: false,
      engine: { state: "active" as const, clearCaches() {} },
      upsert() {},
      delete() {},
      getComponentMeta(id: string) {
        return nativeMetaPayload(id);
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
    };
    const project = new ComponentMetaSession(session as any, "/test");

    const meta = await project.getComponentMeta("Button.vue");

    expect(meta.props.map((prop) => prop.name)).toEqual(["label"]);
  });

  it("exposes raw native component-meta with resolution provenance", async () => {
    const session = {
      closed: false,
      engine: { state: "active" as const, clearCaches() {} },
      upsert() {},
      delete() {},
      getComponentMeta(id: string) {
        return {
          ...nativeMetaPayload(id),
          typeRegistry: [
            {
              name: "Props",
              type: {
                kind: "object",
                properties: [
                  { name: "visible", optional: false, type: { kind: "primitive", name: "string" } },
                ],
              },
              rawType: "export class Props { visible!: string; protected hidden!: boolean }",
              declaration: {
                requestedName: "Props",
                resolvedName: "Props",
                canonicalSource: "/test/types.ts",
                spanStart: 12,
                spanEnd: 81,
                kind: "class",
                text: "export class Props { visible!: string; protected hidden!: boolean }",
              },
            },
          ],
          resolution: {
            mode: "expanded",
            macros: [
              {
                macroIndex: 0,
                macroKind: "defineProps",
                typeName: "Props",
                importSource: "./types",
                declaration: {
                  requestedName: "Props",
                  resolvedName: "Props",
                  canonicalSource: "/test/types.ts",
                  spanStart: 12,
                  spanEnd: 48,
                  kind: "class",
                  text: "export class Props { visible!: string; protected hidden!: boolean }",
                },
                nativeProps: [
                  {
                    name: "visible",
                    isOptional: false,
                    typeAnnotation: "string",
                    visibility: "public",
                    spanStart: 30,
                    spanEnd: 45,
                  },
                  {
                    name: "hidden",
                    isOptional: false,
                    typeAnnotation: "boolean",
                    visibility: "protected",
                    spanStart: 46,
                    spanEnd: 64,
                  },
                ],
                props: [
                  {
                    name: "visible",
                    isOptional: false,
                    typeAnnotation: "string",
                  },
                ],
                emits: [],
                slots: [],
              },
            ],
          },
        };
      },
      getEffectiveSource() {
        return `<script setup lang="ts">defineProps<Props>()</script>`;
      },
      hasFile() {
        return true;
      },
      trackedFileIds() {
        return [];
      },
      close() {},
    };
    const project = new ComponentMetaSession(session as any, "/test");

    const nativeMeta = await project.getNativeComponentMeta("Button.vue");

    expect(nativeMeta?.resolution?.mode).toBe("expanded");
    expect(nativeMeta?.resolution?.macros[0]?.declaration.canonicalSource).toBe("/test/types.ts");
    expect(nativeMeta?.resolution?.macros[0]?.nativeProps?.map((prop) => prop.name)).toEqual([
      "visible",
      "hidden",
    ]);
    expect(nativeMeta?.typeRegistry?.[0]?.rawType).toContain("export class Props");
    expect(nativeMeta?.typeRegistry?.[0]?.declaration?.canonicalSource).toBe("/test/types.ts");
  });

  it("getNativeComponentMeta returns undefined for missing files", async () => {
    const session = {
      closed: false,
      engine: { state: "active" as const, clearCaches() {} },
      upsert() {},
      delete() {},
      ensureBaseFile: vi.fn(() => false),
      getComponentMeta() {
        return null;
      },
      getEffectiveSource() {
        return undefined;
      },
      hasFile() {
        return false;
      },
      trackedFileIds() {
        return [];
      },
      close() {},
    };
    const project = new ComponentMetaSession(session as any, "/test");

    const nativeMeta = await project.getNativeComponentMeta("NonExistent.vue");

    // Assert+: should return undefined for a file that doesn't exist
    expect(nativeMeta).toBeUndefined();
  });

  it("getNativeComponentMeta returns undefined for deleted files", async () => {
    const getComponentMeta = vi
      .fn()
      .mockReturnValueOnce(nativeMetaPayload("/test/Button.vue"))
      .mockReturnValue(null);
    const session = {
      closed: false,
      engine: { state: "active" as const, clearCaches() {} },
      upsert() {},
      delete() {},
      ensureBaseFile: vi.fn(() => true),
      getComponentMeta,
      getEffectiveSource() {
        return `<script setup lang="ts">defineProps<{ label: string }>()</script>`;
      },
      hasFile: vi.fn().mockReturnValueOnce(true).mockReturnValue(false),
      trackedFileIds() {
        return [];
      },
      close() {},
    };
    const project = new ComponentMetaSession(session as any, "/test");

    // First call succeeds
    const firstMeta = await project.getNativeComponentMeta("Button.vue");
    expect(firstMeta).toBeDefined();

    // Simulate deletion — second call should return undefined
    const secondMeta = await project.getNativeComponentMeta("Deleted.vue");
    expect(secondMeta).toBeUndefined();
  });

  it("promotes lazy disk-backed files into the shared native project instead of session overlays", async () => {
    const canonicalId = resolve("/test", "Button.vue")
      .replace(/\\/g, "/")
      .replace(/^([A-Z]):/, (_, drive: string) => `${drive.toLowerCase()}:`);
    const ensureBaseFile = vi.fn(() => true);
    const upsert = vi.fn();
    const session = {
      closed: false,
      engine: { state: "active" as const, clearCaches() {} },
      upsert,
      delete() {},
      ensureBaseFile,
      getComponentMeta(id: string) {
        return nativeMetaPayload(id);
      },
      getEffectiveSource(id: string) {
        if (id === canonicalId && ensureBaseFile.mock.calls.length > 0) {
          return `<script setup lang="ts">defineProps<{ label: string }>()</script>`;
        }
        return undefined;
      },
      hasFile() {
        return false;
      },
      trackedFileIds() {
        return [];
      },
      close() {},
    };
    const workspace = {
      readFile: vi.fn(async () => {
        throw new Error("JS workspace read should not be used for lazy native loading");
      }),
    };
    const project = new ComponentMetaSession(session as any, "/test", {}, workspace as any);

    const meta = await project.getComponentMeta("Button.vue");

    expect(ensureBaseFile).toHaveBeenCalledWith(canonicalId);
    expect(upsert).not.toHaveBeenCalled();
    expect(workspace.readFile).not.toHaveBeenCalled();
    expect(meta.props.map((prop) => prop.name)).toEqual(["label"]);
  });

  it("reload does not throw on open project", async () => {
    const runtime = getMetaRuntime();
    const bootstrap = async () => ({
      nativeProject: createMockProject(new Map()),
      baseFileIds: [],
    });

    const input = {
      backend: "napi" as const,
      root: "/test",
      configKind: "inline" as const,
      configHash: "h7",
      nativeFlags: { analysisLevel: "full" },
    };

    const engine = await runtime.getOrCreateEngine(input, bootstrap);
    const session = runtime.openSession(engine);
    const project = new ComponentMetaSession(session, "/test");

    await expect(project.reload()).resolves.toBeUndefined();
    project.close();
  });

  it("evictComponentMetaSession invalidates handles for that engine", async () => {
    const runtime = getMetaRuntime();
    const bootstrap = async () => ({
      nativeProject: createMockProject(new Map()),
      baseFileIds: [],
    });

    const input = {
      backend: "napi" as const,
      root: "/test",
      configKind: "inline" as const,
      configHash: "h5",
      nativeFlags: { analysisLevel: "full" },
    };

    const engine = await runtime.getOrCreateEngine(input, bootstrap);
    const session = runtime.openSession(engine);
    const project = new ComponentMetaSession(session, "/test");

    runtime.evictEngine(engine.key);
    expect(engine.state).toBe("closed");
    expect(() => project.updateFile("A.vue", "x")).toThrow();
  });
});

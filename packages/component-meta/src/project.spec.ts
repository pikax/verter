import { resolve } from "node:path";
import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { MetaProject, shutdownMetaRuntime } from "./project.js";
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

describe("MetaProject public API", () => {
  beforeEach(() => {
    shutdownMetaRuntime();
  });
  afterEach(() => {
    shutdownMetaRuntime();
  });

  it("repeated openMetaProject shares one engine", async () => {
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
      nativeFlags: { analysisLevel: "full", deepMacroResolutionType: true },
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
      nativeFlags: { analysisLevel: "full", deepMacroResolutionType: true },
    };

    const engine = await runtime.getOrCreateEngine(input, bootstrap);
    const s1 = runtime.openSession(engine);
    const s2 = runtime.openSession(engine);

    const p1 = new MetaProject(s1, "/test");
    const p2 = new MetaProject(s2, "/test");

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
      nativeFlags: { analysisLevel: "full", deepMacroResolutionType: true },
    };

    const engine = await runtime.getOrCreateEngine(input, bootstrap);
    const session = runtime.openSession(engine);
    const project = new MetaProject(session, "/test");

    project.close();
    project.close(); // idempotent

    expect(() => project.updateFile("A.vue", "x")).toThrow("MetaProject is closed");
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
      nativeFlags: { analysisLevel: "full", deepMacroResolutionType: true },
    };

    const engine = await runtime.getOrCreateEngine(input, bootstrap);
    const session = runtime.openSession(engine);
    const project = new MetaProject(session, "/test");

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
      nativeFlags: { analysisLevel: "full", deepMacroResolutionType: true },
    };

    const engine = await runtime.getOrCreateEngine(input, bootstrap);
    const session = runtime.openSession(engine);
    const project = new MetaProject(session, "/test");

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
    const project = new MetaProject(session as any, "/test");

    const meta = await project.getComponentMeta("Button.vue");

    expect(meta.props.map((prop) => prop.name)).toEqual(["label"]);
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
    const project = new MetaProject(session as any, "/test", {}, workspace as any);

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
      nativeFlags: { analysisLevel: "full", deepMacroResolutionType: true },
    };

    const engine = await runtime.getOrCreateEngine(input, bootstrap);
    const session = runtime.openSession(engine);
    const project = new MetaProject(session, "/test");

    await expect(project.reload()).resolves.toBeUndefined();
    project.close();
  });

  it("evictMetaProject invalidates handles for that engine", async () => {
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
      nativeFlags: { analysisLevel: "full", deepMacroResolutionType: true },
    };

    const engine = await runtime.getOrCreateEngine(input, bootstrap);
    const session = runtime.openSession(engine);
    const project = new MetaProject(session, "/test");

    runtime.evictEngine(engine.key);
    expect(engine.state).toBe("closed");
    expect(() => project.updateFile("A.vue", "x")).toThrow();
  });
});

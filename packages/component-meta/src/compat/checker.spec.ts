import { mkdtempSync, mkdirSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { resolve } from "node:path";
import { describe, it, expect, vi } from "vitest";
import {
  ComponentMetaChecker,
  createChecker,
  createCheckerByJson,
  mapPropMeta,
  mapEventMeta,
  mapSlotMeta,
  mapExposedMeta,
  mapComponentMeta,
} from "./checker.js";
import { openMetaProject } from "../project.js";
import { getMetaRuntime, shutdownMetaRuntime } from "../runtime/index.js";
import { primitive, unknown } from "../type-ir.js";
import type { PropMeta, EventMeta, SlotMeta, ExposedMeta } from "../types.js";

let nextProjectRootId = 1;
const itWithForcedGc =
  typeof (globalThis as typeof globalThis & { gc?: () => void }).gc === "function" ? it : it.skip;

async function createRuntimeChecker(
  name = "component-meta-checker",
): Promise<ComponentMetaChecker> {
  const projectRoot = resolve(process.env.TEMP ?? "/tmp", `${name}-${nextProjectRootId++}`);
  return createCheckerByJson(projectRoot, {});
}

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

// ── Mapper unit tests ───────────────────────────────────────────────

describe("mapPropMeta", () => {
  it("maps a prop with description and tags", () => {
    const prop: PropMeta = {
      name: "label",
      type: primitive("string"),
      required: true,
      hasDefault: false,
      rawType: "string",
      description: "The label text",
      tags: [{ name: "default", text: "'hello'" }],
    };

    const result = mapPropMeta(prop);

    expect(result.name).toBe("label");
    expect(result.description).toBe("The label text");
    expect(result.type).toBe("string");
    expect(result.required).toBe(true);
    expect(result.global).toBe(false);
    expect(result.tags).toEqual([{ name: "default", text: "'hello'" }]);
    expect(result.schema).toBe("string");
  });

  it("defaults description to empty string when missing", () => {
    const prop: PropMeta = {
      name: "count",
      type: primitive("number"),
      required: false,
      hasDefault: true,
    };

    const result = mapPropMeta(prop);

    expect(result.description).toBe("");
    expect(result.tags).toEqual([]);
  });
});

describe("mapEventMeta", () => {
  it("maps an event with jsdoc", () => {
    const event: EventMeta = {
      name: "click",
      payload: unknown("unknown"),
      hasValidator: false,
      isDeclared: true,
      description: "Fired on click",
      tags: [{ name: "deprecated" }],
    };

    const result = mapEventMeta(event);

    expect(result.name).toBe("click");
    expect(result.description).toBe("Fired on click");
    expect(result.required).toBe(false);
    expect(result.tags).toEqual([{ name: "deprecated" }]);
  });
});

describe("mapSlotMeta", () => {
  it("maps a slot with bindings", () => {
    const slot: SlotMeta = {
      name: "default",
      isScoped: true,
      bindings: [{ name: "item", type: primitive("string"), rawType: "string" }],
      isRequired: true,
      description: "Main content",
    };

    const result = mapSlotMeta(slot);

    expect(result.name).toBe("default");
    expect(result.description).toBe("Main content");
    expect(result.type).toBe("{ item: string }");
    expect(result.required).toBe(true);
  });
});

describe("mapExposedMeta", () => {
  it("maps exposed member", () => {
    const exposed: ExposedMeta = {
      name: "focus",
      type: unknown("() => void"),
      description: "Focus the input",
    };

    const result = mapExposedMeta(exposed);

    expect(result.name).toBe("focus");
    expect(result.description).toBe("Focus the input");
    expect(result.type).toBe("() => void");
  });
});

describe("mapComponentMeta", () => {
  it("produces VolarComponentMeta shape with _verter", () => {
    const meta = {
      filePath: "test.vue",
      componentName: "Test",
      optionsApi: false,
      props: [
        {
          name: "label",
          type: primitive("string"),
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

    const result = mapComponentMeta(meta);

    expect(result.type).toBe(0);
    expect(result.props).toHaveLength(1);
    expect(result.props[0].name).toBe("label");
    expect(result.events).toHaveLength(0);
    expect(result.slots).toHaveLength(0);
    expect(result.exposed).toHaveLength(0);
    expect(result._verter).toBe(meta);

    // Negative: compat shape should NOT have Verter-only fields at top level
    expect("filePath" in result).toBe(false);
    expect("componentName" in result).toBe(false);
    expect("models" in result).toBe(false);
    expect("flags" in result).toBe(false);
  });
});

// ── Checker integration tests ───────────────────────────────────────

describe("ComponentMetaChecker", () => {
  it("uses the native component-meta query instead of rebuilding from analysis snapshots", async () => {
    const getAnalysis = vi.fn(() => {
      throw new Error("legacy getAnalysis should not be called");
    });
    const resolveImportedTypes = vi.fn(() => {
      throw new Error("legacy resolveImportedTypes should not be called");
    });
    const evaluateTypes = vi.fn(() => {
      throw new Error("legacy evaluateTypes should not be called");
    });
    const getComponentMeta = vi.fn((canonicalId: string) => nativeMetaPayload(canonicalId));
    const session = {
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
    };
    const checker = new ComponentMetaChecker(
      {
        upsert: vi.fn(),
        getAnalysis,
        resolveImportedTypes,
        evaluateTypes,
      },
      "/tmp",
      {},
      session as any,
    );

    checker.updateFile(
      "Single.vue",
      `<script setup lang="ts">defineProps<{ label: string }>()</script><template><div /></template>`,
    );
    const meta = await checker.getComponentMeta("Single.vue");

    expect(meta.props.some((prop) => prop.name === "label")).toBe(true);
    expect(getComponentMeta).toHaveBeenCalledTimes(1);
    expect(getAnalysis).not.toHaveBeenCalled();
    expect(resolveImportedTypes).not.toHaveBeenCalled();
    expect(evaluateTypes).not.toHaveBeenCalled();
  });

  it("createCheckerByJson owns a dedicated engine and tears it down on close", async () => {
    shutdownMetaRuntime();
    const runtime = getMetaRuntime();
    const projectRoot = mkdtempSync(resolve(tmpdir(), "verter-checker-dedicated-json-"));
    mkdirSync(resolve(projectRoot, "src"), { recursive: true });
    writeFileSync(
      resolve(projectRoot, "src", "App.vue"),
      `<script setup lang="ts">defineProps<{ label: string }>()</script><template><div /></template>`,
      "utf8",
    );

    const checker = await createCheckerByJson(projectRoot, {
      include: ["src/**/*.vue"],
      compilerOptions: { baseUrl: "." },
    });
    const engine = (checker as any)._session.engine;

    const meta = await checker.getComponentMeta(resolve(projectRoot, "src", "App.vue"));

    expect(meta.props.map((prop) => prop.name)).toEqual(["label"]);
    expect(engine.state).toBe("active");
    expect(runtime.engineCount).toBe(0);

    checker.close();

    expect(engine.state).toBe("closed");
    expect(engine.leaseCount).toBe(0);
    expect(runtime.engineCount).toBe(0);
    shutdownMetaRuntime();
  });

  it("createCheckerByJson can be created and disposed sequentially without pooling", async () => {
    shutdownMetaRuntime();
    const runtime = getMetaRuntime();
    const projectRoot = mkdtempSync(resolve(tmpdir(), "verter-checker-sequential-json-"));
    mkdirSync(resolve(projectRoot, "src"), { recursive: true });
    writeFileSync(
      resolve(projectRoot, "src", "App.vue"),
      `<script setup lang="ts">defineProps<{ label: string }>()</script><template><div /></template>`,
      "utf8",
    );

    for (let index = 0; index < 4; index++) {
      const checker = await createCheckerByJson(projectRoot, {
        include: ["src/**/*.vue"],
        compilerOptions: { baseUrl: "." },
      });
      const engine = (checker as any)._session.engine;

      const meta = await checker.getComponentMeta(resolve(projectRoot, "src", "App.vue"));

      expect(meta.props.map((prop) => prop.name)).toEqual(["label"]);
      expect(engine.state).toBe("active");
      expect(runtime.engineCount).toBe(0);

      checker.dispose();

      expect(engine.state).toBe("closed");
      expect(engine.leaseCount).toBe(0);
      expect(runtime.engineCount).toBe(0);
    }

    shutdownMetaRuntime();
  });

  // @ai-generated - Verifies the owned cleanup path used for abandoned dedicated checkers.
  it("createCheckerByJson exposes owned resource cleanup for forgotten checkers", async () => {
    shutdownMetaRuntime();
    const runtime = getMetaRuntime();
    const projectRoot = mkdtempSync(resolve(tmpdir(), "verter-checker-owned-cleanup-json-"));
    mkdirSync(resolve(projectRoot, "src"), { recursive: true });
    writeFileSync(
      resolve(projectRoot, "src", "App.vue"),
      `<script setup lang="ts">defineProps<{ label: string }>()</script><template><div /></template>`,
      "utf8",
    );

    const checker = await createCheckerByJson(projectRoot, {
      include: ["src/**/*.vue"],
      compilerOptions: { baseUrl: "." },
    });
    const engine = ((checker as any)._session as any).engine;
    const ownedResources = (checker as any)._ownedResources;

    expect(ownedResources).toBeDefined();
    expect(engine.state).toBe("active");
    expect(engine.leaseCount).toBe(1);
    expect(runtime.engineCount).toBe(0);

    ownedResources.release();

    expect(engine.state).toBe("closed");
    expect(engine.leaseCount).toBe(0);
    expect(runtime.engineCount).toBe(0);
    shutdownMetaRuntime();
  });

  // @ai-generated - Verifies abandoned dedicated compat checkers still release native resources.
  itWithForcedGc(
    "createCheckerByJson finalizes dedicated engines when callers forget close",
    async () => {
      shutdownMetaRuntime();
      const runtime = getMetaRuntime();
      const forceGc = (globalThis as typeof globalThis & { gc?: () => void }).gc!;
      const projectRoot = mkdtempSync(resolve(tmpdir(), "verter-checker-finalize-json-"));
      mkdirSync(resolve(projectRoot, "src"), { recursive: true });
      writeFileSync(
        resolve(projectRoot, "src", "App.vue"),
        `<script setup lang="ts">defineProps<{ label: string }>()</script><template><div /></template>`,
        "utf8",
      );

      let checker: ComponentMetaChecker | null = await createCheckerByJson(projectRoot, {
        include: ["src/**/*.vue"],
        compilerOptions: { baseUrl: "." },
      });
      const engine = ((checker as any)._session as any).engine;

      await checker.getComponentMeta(resolve(projectRoot, "src", "App.vue"));
      expect(engine.state).toBe("active");
      expect(engine.leaseCount).toBe(1);
      expect(runtime.engineCount).toBe(0);

      checker = null;

      for (let attempt = 0; attempt < 20 && engine.state !== "closed"; attempt++) {
        forceGc();
        await new Promise((resolve) => setTimeout(resolve, 0));
      }

      expect(engine.state).toBe("closed");
      expect(engine.leaseCount).toBe(0);
      expect(runtime.engineCount).toBe(0);
      shutdownMetaRuntime();
    },
  );

  it("createChecker owns a dedicated engine and tears it down on close", async () => {
    shutdownMetaRuntime();
    const runtime = getMetaRuntime();
    const projectRoot = mkdtempSync(resolve(tmpdir(), "verter-checker-dedicated-tsconfig-"));
    mkdirSync(resolve(projectRoot, "src"), { recursive: true });
    writeFileSync(
      resolve(projectRoot, "tsconfig.json"),
      JSON.stringify({
        include: ["src/**/*.vue"],
        compilerOptions: { baseUrl: "." },
      }),
      "utf8",
    );
    writeFileSync(
      resolve(projectRoot, "src", "App.vue"),
      `<script setup lang="ts">defineProps<{ label: string }>()</script><template><div /></template>`,
      "utf8",
    );

    const checker = await createChecker(resolve(projectRoot, "tsconfig.json"));
    const engine = (checker as any)._session.engine;

    const meta = await checker.getComponentMeta("./src/App.vue");

    expect(meta.props.map((prop) => prop.name)).toEqual(["label"]);
    expect(engine.state).toBe("active");
    expect(runtime.engineCount).toBe(0);

    checker.close();

    expect(engine.state).toBe("closed");
    expect(engine.leaseCount).toBe(0);
    expect(runtime.engineCount).toBe(0);
    shutdownMetaRuntime();
  });

  it("createChecker does not share pooled runtime state with openMetaProject", async () => {
    shutdownMetaRuntime();
    const runtime = getMetaRuntime();
    const projectRoot = mkdtempSync(resolve(tmpdir(), "verter-checker-project-isolated-"));
    mkdirSync(resolve(projectRoot, "src"), { recursive: true });
    writeFileSync(
      resolve(projectRoot, "tsconfig.json"),
      JSON.stringify({
        include: ["src/**/*.vue"],
        compilerOptions: { baseUrl: "." },
      }),
      "utf8",
    );
    writeFileSync(
      resolve(projectRoot, "src", "App.vue"),
      `<script setup lang="ts">defineProps<{ label: string }>()</script><template><div /></template>`,
      "utf8",
    );

    const project = await openMetaProject({
      root: projectRoot,
      tsconfig: resolve(projectRoot, "tsconfig.json"),
    });
    const projectEngine = (project as any)._session.engine;
    const checker = await createChecker(resolve(projectRoot, "tsconfig.json"));
    const checkerEngine = (checker as any)._session.engine;

    expect(runtime.engineCount).toBe(1);
    expect(projectEngine).not.toBe(checkerEngine);
    expect(projectEngine.state).toBe("active");
    expect(checkerEngine.state).toBe("active");

    checker.close();

    expect(checkerEngine.state).toBe("closed");
    expect(projectEngine.state).toBe("active");
    expect(runtime.engineCount).toBe(1);

    project.close();
    shutdownMetaRuntime();
  });

  // @ai-generated - Verifies the public drop-in compat entrypoint creates its own workspace.
  it("createChecker accepts a tsconfig path without an injected workspace", async () => {
    shutdownMetaRuntime();
    const projectRoot = mkdtempSync(resolve(tmpdir(), "verter-checker-tsconfig-"));
    mkdirSync(resolve(projectRoot, "src"), { recursive: true });
    writeFileSync(
      resolve(projectRoot, "tsconfig.json"),
      JSON.stringify({
        include: ["src/**/*.vue"],
        compilerOptions: { baseUrl: "." },
      }),
      "utf8",
    );
    writeFileSync(
      resolve(projectRoot, "src", "App.vue"),
      `<script setup lang="ts">defineProps<{ label: string }>()</script><template><div /></template>`,
      "utf8",
    );

    const checker = await createChecker(resolve(projectRoot, "tsconfig.json"));
    const meta = await checker.getComponentMeta("./src/App.vue");

    expect(meta.props.some((prop) => prop.name === "label")).toBe(true);

    checker.close();
    shutdownMetaRuntime();
  });

  it("promotes lazy workspace files into the shared native project", async () => {
    const canonicalId = resolve("/tmp", "Lazy.vue")
      .replace(/\\/g, "/")
      .replace(/^([A-Z]):/, (_, drive: string) => `${drive.toLowerCase()}:`);
    const ensureBaseFile = vi.fn(() => true);
    const getComponentMeta = vi.fn((canonicalId: string) => nativeMetaPayload(canonicalId));
    const workspace = {
      readFile: vi.fn(async () => {
        throw new Error("JS workspace read should not be used for lazy native loading");
      }),
      fileExists: vi.fn(async () => true),
      isDir: vi.fn(async () => false),
      readDir: vi.fn(async () => []),
      walk: vi.fn(async () => []),
      configureProjects: vi.fn(),
    };
    const checker = new ComponentMetaChecker(
      {
        upsert: vi.fn(),
        getAnalysis: vi.fn(),
      },
      "/tmp",
      {},
      {
        closed: false,
        engine: { state: "active" as const },
        upsert: vi.fn(),
        delete: vi.fn(),
        ensureBaseFile,
        getComponentMeta,
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
      } as any,
      workspace as any,
    );

    const meta = await checker.getComponentMeta("Lazy.vue");

    expect(ensureBaseFile).toHaveBeenCalledWith(canonicalId);
    expect(workspace.readFile).not.toHaveBeenCalled();
    expect(getComponentMeta).toHaveBeenCalledWith(canonicalId);
    expect(meta.props.map((prop) => prop.name)).toEqual(["label"]);
  });

  it("getComponentMeta returns Volar-shaped output", async () => {
    const checker = await createRuntimeChecker("checker-volar-shape");

    const source = `<script setup lang="ts">
/** The label text */
defineProps<{
  /** Display label */
  label: string
  /** @deprecated use label instead */
  title?: string
}>()

defineEmits<{
  /** Fired on click */
  click: []
}>()
</script>
<template><div><slot /></div></template>`;

    checker.updateFile("Test.vue", source);
    const meta = await checker.getComponentMeta("Test.vue");

    // Shape checks
    expect(meta.type).toBe(0);
    expect(Array.isArray(meta.props)).toBe(true);
    expect(Array.isArray(meta.events)).toBe(true);
    expect(Array.isArray(meta.slots)).toBe(true);
    expect(Array.isArray(meta.exposed)).toBe(true);

    // Props
    expect(meta.props.length).toBeGreaterThanOrEqual(1);
    const labelProp = meta.props.find((p) => p.name === "label");
    expect(labelProp).toBeDefined();
    expect(labelProp!.description).toBe("Display label");
    expect(labelProp!.type).toBe("string");
    expect(labelProp!.required).toBe(true);
    expect(labelProp!.tags).toEqual([]);
    expect(typeof labelProp!.schema).toBeDefined();

    // Title prop with @deprecated tag
    const titleProp = meta.props.find((p) => p.name === "title");
    if (titleProp) {
      expect(titleProp.tags.length).toBeGreaterThanOrEqual(1);
      expect(titleProp.tags[0].name).toBe("deprecated");
    }

    // Events
    const clickEvent = meta.events.find((e) => e.name === "click");
    if (clickEvent) {
      expect(clickEvent.description).toBe("Fired on click");
    }

    // _verter field has full metadata
    expect(meta._verter).toBeDefined();
    expect(meta._verter!.filePath).toBeDefined();
    expect(meta._verter!.componentName).toBeDefined();
  });

  it("getExportNames returns ['default'] for SFC", async () => {
    const checker = await createRuntimeChecker("checker-export-names");
    expect(await checker.getExportNames("Test.vue")).toEqual(["default"]);
  });

  it("updateFile is reflected in next getComponentMeta", async () => {
    const checker = await createRuntimeChecker("checker-update-file");

    checker.updateFile(
      "Test.vue",
      `<script setup lang="ts">defineProps<{ a: string }>()</script><template><div /></template>`,
    );
    let meta = await checker.getComponentMeta("Test.vue");
    expect(meta.props.some((p) => p.name === "a")).toBe(true);

    checker.updateFile(
      "Test.vue",
      `<script setup lang="ts">defineProps<{ b: number }>()</script><template><div /></template>`,
    );
    meta = await checker.getComponentMeta("Test.vue");
    expect(meta.props.some((p) => p.name === "b")).toBe(true);
    expect(meta.props.some((p) => p.name === "a")).toBe(false);
  });

  it("deleteFile clears metadata", async () => {
    const checker = await createRuntimeChecker("checker-delete-file");

    checker.updateFile(
      "Test.vue",
      `<script setup lang="ts">defineProps<{ a: string }>()</script><template><div /></template>`,
    );
    checker.deleteFile("Test.vue");
    const meta = await checker.getComponentMeta("Test.vue");
    expect(meta.props).toHaveLength(0);
  });

  it("deleteFile does not lazily rehydrate a tombstoned base file", async () => {
    let workspaceReads = 0;
    const workspace = {
      readFile: async () => {
        workspaceReads++;
        return `<script setup lang="ts">defineProps<{ a: string }>()</script>`;
      },
      fileExists: async () => true,
      isDir: async () => false,
      readDir: async () => [],
      walk: async () => [],
      configureProjects() {},
    };
    const session = {
      closed: false,
      engine: { state: "active" as const },
      upsert() {},
      delete() {},
      getComponentMeta() {
        return null;
      },
      getAnalysis() {
        return null;
      },
      resolveImportedTypes() {
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
    const checker = new ComponentMetaChecker(
      {
        upsert() {},
        remove() {},
        getAnalysis() {
          return null;
        },
      },
      "/tmp",
      {},
      session as any,
      workspace,
      { closeSession() {} } as any,
    );

    checker.deleteFile("Base.vue");
    const meta = await checker.getComponentMeta("Base.vue");

    expect(meta.props).toHaveLength(0);
    expect(workspaceReads).toBe(0);
  });

  it("getProgram throws", () => {
    const checker = new ComponentMetaChecker(
      {
        upsert() {},
        getAnalysis() {
          return null;
        },
      },
      "/tmp",
      {},
    );
    expect(() => checker.getProgram()).toThrow();
  });

  it("runtime defineProps preserves JSDoc descriptions and tags", async () => {
    const checker = await createRuntimeChecker("checker-runtime-props");

    const source = `<script setup lang="ts">
defineProps({
  /** The display label */
  label: String,
  /** Size variant
   * @default 'md'
   */
  size: { type: String, default: 'md' },
  noDoc: Number,
})
</script>
<template><div /></template>`;

    checker.updateFile("Runtime.vue", source);
    const meta = await checker.getComponentMeta("Runtime.vue");

    // Positive: label has JSDoc description
    const labelProp = meta.props.find((p) => p.name === "label");
    expect(labelProp).toBeDefined();
    expect(labelProp!.description).toBe("The display label");
    expect(labelProp!.tags).toEqual([]);

    // Positive: size has JSDoc description and @default tag
    const sizeProp = meta.props.find((p) => p.name === "size");
    expect(sizeProp).toBeDefined();
    expect(sizeProp!.description).toBe("Size variant");
    expect(sizeProp!.tags.length).toBeGreaterThanOrEqual(1);
    expect(sizeProp!.tags[0].name).toBe("default");

    // Negative: noDoc has empty description (compat maps null → "")
    const noDocProp = meta.props.find((p) => p.name === "noDoc");
    expect(noDocProp).toBeDefined();
    expect(noDocProp!.description).toBe("");
    expect(noDocProp!.tags).toEqual([]);
  });

  it("enum schema uses array format (Volar parity)", async () => {
    const checker = await createRuntimeChecker("checker-enum-schema");

    const source = `<script setup lang="ts">
defineProps<{
  color?: 'red' | 'blue'
}>()
</script>
<template><div /></template>`;

    checker.updateFile("Enum.vue", source);
    const meta = await checker.getComponentMeta("Enum.vue");

    const colorProp = meta.props.find((p) => p.name === "color");
    expect(colorProp).toBeDefined();
    const schema = colorProp!.schema;
    // Should be enum with array schema, not numeric-keyed object
    expect(typeof schema).not.toBe("string");
    if (typeof schema !== "string") {
      expect(schema.kind).toBe("enum");
      expect(Array.isArray(schema.schema)).toBe(true);
    }
  });

  it("resolves imported type interfaces from dependency .ts files", async () => {
    // Use resolve() to get consistent absolute paths on all platforms
    const projectRoot = resolve(
      process.env.TEMP ?? "/tmp",
      `verter-test-crossfile-${nextProjectRootId++}`,
    );
    const checker = await createCheckerByJson(projectRoot, {});

    // Upsert the .ts dependency as non-SFC using resolved path
    checker.updateFile("types.ts", "export interface ButtonProps { label: string; size?: number }");

    // Upsert the .vue file that imports from the dependency
    checker.updateFile(
      "Button.vue",
      `<script setup lang="ts">
import type { ButtonProps } from './types'
defineProps<ButtonProps>()
</script><template><div /></template>`,
    );

    const meta = await checker.getComponentMeta("Button.vue");

    // Positive: should resolve props from the imported interface
    const labelProp = meta.props.find((p) => p.name === "label");
    expect(labelProp).toBeDefined();
    expect(labelProp!.type).toContain("string");

    const sizeProp = meta.props.find((p) => p.name === "size");
    expect(sizeProp).toBeDefined();

    // Negative: should not have the interface name as a prop
    expect(meta.props.some((p) => p.name === "ButtonProps")).toBe(false);
  });

  // ReturnType<typeof fn> resolved by native evaluator via body inference.
  it("expands ReturnType utility props into structured object schema", async () => {
    const checker = await createRuntimeChecker("checker-return-type");

    checker.updateFile(
      "ReturnType.vue",
      `<script setup lang="ts">
function createConfig() {
  return { theme: 'dark' as string, debug: false }
}

defineProps<{
  config: ReturnType<typeof createConfig>
}>()
</script>
<template><div /></template>`,
    );

    const meta = await checker.getComponentMeta("ReturnType.vue");
    const configProp = meta.props.find((p) => p.name === "config");

    expect(configProp).toBeDefined();
    expect(typeof configProp!.schema).not.toBe("string");
    if (typeof configProp!.schema === "string") return;

    expect(configProp!.schema.kind).toBe("object");
    expect(configProp!.schema.schema).toEqual(
      expect.objectContaining({
        theme: expect.any(Object),
        debug: expect.any(Object),
      }),
    );
  });

  // Pick/Omit resolved by the native lightweight evaluator.
  it("expands Pick and Omit utility props into narrowed object schemas", async () => {
    const checker = await createRuntimeChecker("checker-pick-omit");

    checker.updateFile(
      "PickOmit.vue",
      `<script setup lang="ts">
interface FullUser {
  id: number
  name: string
  email: string
  password: string
}

defineProps<{
  display: Pick<FullUser, 'id' | 'name'>
  safe: Omit<FullUser, 'password'>
}>()
</script>
<template><div /></template>`,
    );

    const meta = await checker.getComponentMeta("PickOmit.vue");
    const displayProp = meta.props.find((p) => p.name === "display");
    const safeProp = meta.props.find((p) => p.name === "safe");

    expect(displayProp).toBeDefined();
    expect(safeProp).toBeDefined();
    expect(typeof displayProp!.schema).not.toBe("string");
    expect(typeof safeProp!.schema).not.toBe("string");
    if (typeof displayProp!.schema === "string" || typeof safeProp!.schema === "string") return;

    expect(displayProp!.schema.kind).toBe("object");
    expect(displayProp!.schema.schema).toEqual(
      expect.objectContaining({
        id: expect.any(Object),
        name: expect.any(Object),
      }),
    );
    expect(displayProp!.schema.schema).not.toHaveProperty("email");
    expect(displayProp!.schema.schema).not.toHaveProperty("password");

    expect(safeProp!.schema.kind).toBe("object");
    expect(safeProp!.schema.schema).toEqual(
      expect.objectContaining({
        id: expect.any(Object),
        name: expect.any(Object),
        email: expect.any(Object),
      }),
    );
    expect(safeProp!.schema.schema).not.toHaveProperty("password");
  });

  it("expands utilities that target imported types", async () => {
    const projectRoot = resolve(
      process.env.TEMP ?? "/tmp",
      `verter-test-imported-utilities-${nextProjectRootId++}`,
    );
    const checker = await createCheckerByJson(projectRoot, {});

    checker.updateFile(
      "types.ts",
      `export interface ImportedUser {
  id: number
  name: string
  password: string
}`,
    );

    checker.updateFile(
      "ImportedPick.vue",
      `<script setup lang="ts">
import type { ImportedUser } from './types'

defineProps<{
  user: Pick<ImportedUser, 'id' | 'name'>
}>()
</script>
<template><div /></template>`,
    );

    const meta = await checker.getComponentMeta("ImportedPick.vue");
    const userProp = meta.props.find((p) => p.name === "user");

    expect(userProp).toBeDefined();
    expect(typeof userProp!.schema).not.toBe("string");
    if (typeof userProp!.schema === "string") return;

    expect(userProp!.schema.kind).toBe("object");
    expect(userProp!.schema.schema).toEqual(
      expect.objectContaining({
        id: expect.any(Object),
        name: expect.any(Object),
      }),
    );
    expect(userProp!.schema.schema).not.toHaveProperty("password");
  });

  it("preserves index signature text inside intersection schemas", async () => {
    const checker = await createRuntimeChecker("checker-index-signature");

    checker.updateFile(
      "Typed.vue",
      `<script setup lang="ts">
defineProps<{
  partialImage: string | (Partial<HTMLImageElement> & { [key: string]: any })
}>()
</script>
<template><div /></template>`,
    );

    const meta = await checker.getComponentMeta("Typed.vue");
    const partialImage = meta.props.find((p) => p.name === "partialImage");

    expect(partialImage).toBeDefined();
    expect(typeof partialImage!.schema).not.toBe("string");
    if (typeof partialImage!.schema === "string") return;

    expect(partialImage!.schema.kind).toBe("enum");
    const objectArm = (partialImage!.schema.schema as any[]).find(
      (entry) =>
        typeof entry !== "string" && entry?.kind === "object" && Array.isArray(entry?.schema),
    );
    expect(objectArm).toBeDefined();
    expect(objectArm.schema).toEqual([
      expect.objectContaining({
        kind: "object",
        type: "Partial<HTMLImageElement>",
      }),
      expect.objectContaining({
        kind: "object",
        type: "{ [key: string]: any; }",
      }),
    ]);
  });

  it("Options API props preserve JSDoc descriptions and tags", async () => {
    const checker = await createRuntimeChecker("checker-options-api");

    const source = `<script>
import { defineComponent } from 'vue'
export default defineComponent({
  props: {
    /** The display label */
    label: String,
    /** Size variant
     * @default 'md'
     */
    size: { type: String, default: 'md' },
    noDoc: Number,
  }
})
</script>
<template><div /></template>`;

    checker.updateFile("Options.vue", source);
    const meta = await checker.getComponentMeta("Options.vue");

    // Positive: label has JSDoc description
    const labelProp = meta.props.find((p) => p.name === "label");
    expect(labelProp).toBeDefined();
    expect(labelProp!.description).toBe("The display label");
    expect(labelProp!.tags).toEqual([]);

    // Positive: size has JSDoc description and @default tag
    const sizeProp = meta.props.find((p) => p.name === "size");
    expect(sizeProp).toBeDefined();
    expect(sizeProp!.description).toBe("Size variant");
    expect(sizeProp!.tags.length).toBeGreaterThanOrEqual(1);
    expect(sizeProp!.tags[0].name).toBe("default");

    // Negative: noDoc has empty description (compat maps null → "")
    const noDocProp = meta.props.find((p) => p.name === "noDoc");
    expect(noDocProp).toBeDefined();
    expect(noDocProp!.description).toBe("");
  });
});

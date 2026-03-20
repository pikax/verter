import { resolve } from "node:path";
import { describe, it, expect } from "vitest";
import {
  ComponentMetaChecker,
  mapPropMeta,
  mapEventMeta,
  mapSlotMeta,
  mapExposedMeta,
  mapComponentMeta,
} from "./checker.js";
import { createNapiAdapter } from "../host-adapter.js";
import { primitive, unknown } from "../type-ir.js";
import type { PropMeta, EventMeta, SlotMeta, ExposedMeta } from "../types.js";

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
  it("getComponentMeta returns Volar-shaped output", async () => {
    const adapter = createNapiAdapter();
    const checker = new ComponentMetaChecker(adapter, "/tmp", {});

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
    const adapter = createNapiAdapter();
    const checker = new ComponentMetaChecker(adapter, "/tmp", {});
    expect(await checker.getExportNames("Test.vue")).toEqual(["default"]);
  });

  it("updateFile is reflected in next getComponentMeta", async () => {
    const adapter = createNapiAdapter();
    const checker = new ComponentMetaChecker(adapter, "/tmp", {});

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
    const adapter = createNapiAdapter();
    const checker = new ComponentMetaChecker(adapter, "/tmp", {});

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
    const adapter = createNapiAdapter();
    const checker = new ComponentMetaChecker(adapter, "/tmp", {});
    expect(() => checker.getProgram()).toThrow();
  });

  it("runtime defineProps preserves JSDoc descriptions and tags", async () => {
    const adapter = createNapiAdapter();
    const checker = new ComponentMetaChecker(adapter, "/tmp", {});

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
    const adapter = createNapiAdapter();
    const checker = new ComponentMetaChecker(adapter, "/tmp", {});

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
    const adapter = createNapiAdapter();
    // Use resolve() to get consistent absolute paths on all platforms
    const projectRoot = resolve(process.env.TEMP ?? "/tmp", "verter-test-crossfile");
    const checker = new ComponentMetaChecker(adapter, projectRoot, {});

    // Upsert the .ts dependency as non-SFC using resolved path
    const typesPath = resolve(projectRoot, "types.ts");
    adapter.upsert({
      inputId: typesPath,
      source: "export interface ButtonProps { label: string; size?: number }",
      fileKind: "non_sfc",
    });

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

  // @ai-generated - Utility types like ReturnType should be expanded by a TS-backed
  // resolver rather than degrading to an opaque ref/object shell.
  it.fails("expands ReturnType utility props into structured object schema", async () => {
    const adapter = createNapiAdapter();
    const checker = new ComponentMetaChecker(adapter, "/tmp", {});

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

  // @ai-generated - Pick/Omit are a good boundary case: simple enough for TS to
  // resolve precisely, but not worth teaching to the lightweight parser.
  it.fails("expands Pick and Omit utility props into narrowed object schemas", async () => {
    const adapter = createNapiAdapter();
    const checker = new ComponentMetaChecker(adapter, "/tmp", {});

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

  it("Options API props preserve JSDoc descriptions and tags", async () => {
    const adapter = createNapiAdapter();
    const checker = new ComponentMetaChecker(adapter, "/tmp", {});

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

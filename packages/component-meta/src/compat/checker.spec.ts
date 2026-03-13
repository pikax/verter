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
  it("getComponentMeta returns Volar-shaped output", () => {
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
    const meta = checker.getComponentMeta("Test.vue");

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

  it("getExportNames returns ['default'] for SFC", () => {
    const adapter = createNapiAdapter();
    const checker = new ComponentMetaChecker(adapter, "/tmp", {});
    expect(checker.getExportNames("Test.vue")).toEqual(["default"]);
  });

  it("updateFile is reflected in next getComponentMeta", () => {
    const adapter = createNapiAdapter();
    const checker = new ComponentMetaChecker(adapter, "/tmp", {});

    checker.updateFile(
      "Test.vue",
      `<script setup lang="ts">defineProps<{ a: string }>()</script><template><div /></template>`,
    );
    let meta = checker.getComponentMeta("Test.vue");
    expect(meta.props.some((p) => p.name === "a")).toBe(true);

    checker.updateFile(
      "Test.vue",
      `<script setup lang="ts">defineProps<{ b: number }>()</script><template><div /></template>`,
    );
    meta = checker.getComponentMeta("Test.vue");
    expect(meta.props.some((p) => p.name === "b")).toBe(true);
    expect(meta.props.some((p) => p.name === "a")).toBe(false);
  });

  it("deleteFile clears metadata", () => {
    const adapter = createNapiAdapter();
    const checker = new ComponentMetaChecker(adapter, "/tmp", {});

    checker.updateFile(
      "Test.vue",
      `<script setup lang="ts">defineProps<{ a: string }>()</script><template><div /></template>`,
    );
    checker.deleteFile("Test.vue");
    const meta = checker.getComponentMeta("Test.vue");
    expect(meta.props).toHaveLength(0);
  });

  it("getProgram throws", () => {
    const adapter = createNapiAdapter();
    const checker = new ComponentMetaChecker(adapter, "/tmp", {});
    expect(() => checker.getProgram()).toThrow();
  });
});

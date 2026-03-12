import { describe, it, expect } from "vitest";
import { snapshotToMeta } from "./extractor.js";

// ── Helpers ──────────────────────────────────────────────────

function makeSnapshot(overrides: Record<string, unknown> = {}) {
  return {
    imports: [],
    bindings: [],
    macros: [],
    macroTypeDeps: [],
    scriptFlags: 0,
    styles: [],
    template: null,
    ...overrides,
  };
}

function makeCompositionSnapshot(
  propFields: Array<{ name: string; typeAnnotation?: string }> = [],
  emitFields: Array<{ name: string }> = [],
  template: Record<string, unknown> | null = null,
  extraMacros: unknown[] = [],
) {
  const macros: unknown[] = [
    ...(propFields.length > 0
      ? [
          {
            kind: "DefineProps",
            isTypeBased: true,
            typeReferences: [],
            bindingName: null,
            hasInheritAttrsFalse: false,
            propFields: propFields.map((f) => ({
              name: f.name,
              typeAnnotation: f.typeAnnotation ?? null,
              spanStart: 0,
              spanEnd: 0,
            })),
            spanStart: 0,
            spanEnd: 0,
          },
        ]
      : []),
    ...(emitFields.length > 0
      ? [
          {
            kind: "DefineEmits",
            isTypeBased: true,
            typeReferences: [],
            bindingName: null,
            hasInheritAttrsFalse: false,
            emitFields: emitFields.map((f) => ({
              name: f.name,
              spanStart: 0,
              spanEnd: 0,
            })),
            spanStart: 0,
            spanEnd: 0,
          },
        ]
      : []),
    ...extraMacros,
  ];

  return makeSnapshot({
    macros,
    scriptFlags: (propFields.length > 0 ? 1 << 1 : 0) | (emitFields.length > 0 ? 1 << 2 : 0),
    template,
  });
}

// ── Tests ────────────────────────────────────────────────────

describe("snapshotToMeta", () => {
  describe("basic metadata", () => {
    it("extracts filePath and componentName", () => {
      const meta = snapshotToMeta(makeSnapshot(), "/src/components/MyButton.vue");
      expect(meta.filePath).toBe("/src/components/MyButton.vue");
      expect(meta.componentName).toBe("MyButton");
    });

    it("strips .vue extension from componentName", () => {
      const meta = snapshotToMeta(makeSnapshot(), "/app/Header.vue");
      expect(meta.componentName).toBe("Header");
      // Should not contain .vue
      expect(meta.componentName).not.toContain(".vue");
    });

    it("handles Windows paths", () => {
      const meta = snapshotToMeta(makeSnapshot(), "C:\\src\\MyComp.vue");
      expect(meta.componentName).toBe("MyComp");
    });
  });

  describe("optionsApi detection", () => {
    it("detects composition API as optionsApi: false", () => {
      const snapshot = makeCompositionSnapshot([{ name: "msg", typeAnnotation: "string" }]);
      const meta = snapshotToMeta(snapshot, "Comp.vue");
      expect(meta.optionsApi).toBe(false);
    });

    it("detects options API as optionsApi: true", () => {
      const snapshot = makeSnapshot({
        scriptFlags: 1 << 19, // HAS_OPTIONS_API
        optionsApi: {
          isDefineComponent: true,
          props: [{ name: "msg", typeConstructor: "String", isRequired: true, hasDefault: false }],
        },
      });
      const meta = snapshotToMeta(snapshot, "Comp.vue");
      expect(meta.optionsApi).toBe(true);
    });

    it("defaults to false for empty snapshot", () => {
      const meta = snapshotToMeta(makeSnapshot(), "Comp.vue");
      expect(meta.optionsApi).toBe(false);
    });
  });

  describe("composition API props", () => {
    it("extracts typed props from defineProps", () => {
      const snapshot = makeCompositionSnapshot(
        [
          { name: "msg", typeAnnotation: "string" },
          { name: "count", typeAnnotation: "number" },
        ],
        [],
        {
          propDefinitions: [
            {
              name: "msg",
              typeAnnotation: "string",
              hasDefault: false,
              isRequired: true,
              isBoolean: false,
              usedInTemplate: true,
              usedInScript: false,
            },
            {
              name: "count",
              typeAnnotation: "number",
              hasDefault: true,
              isRequired: false,
              isBoolean: false,
              usedInTemplate: true,
              usedInScript: false,
            },
          ],
        },
      );
      const meta = snapshotToMeta(snapshot, "Comp.vue");

      expect(meta.props).toHaveLength(2);
      expect(meta.props[0].name).toBe("msg");
      expect(meta.props[0].type).toEqual({ kind: "primitive", name: "string" });
      expect(meta.props[0].required).toBe(true);
      expect(meta.props[0].hasDefault).toBe(false);
      expect(meta.props[0].rawType).toBe("string");

      expect(meta.props[1].name).toBe("count");
      expect(meta.props[1].required).toBe(false);
      expect(meta.props[1].hasDefault).toBe(true);

      // Negative: should not have runtimeTypes for type-based props
      expect(meta.props[0].runtimeTypes).toBeUndefined();
    });

    it("resolves union type annotations", () => {
      const snapshot = makeCompositionSnapshot(
        [{ name: "size", typeAnnotation: "'sm' | 'md' | 'lg'" }],
        [],
        {
          propDefinitions: [
            {
              name: "size",
              typeAnnotation: "'sm' | 'md' | 'lg'",
              hasDefault: true,
              isRequired: false,
              isBoolean: false,
              usedInTemplate: true,
              usedInScript: false,
            },
          ],
        },
      );
      const meta = snapshotToMeta(snapshot, "Comp.vue");

      expect(meta.props[0].type.kind).toBe("union");
      // Should not be unknown
      expect(meta.props[0].type.kind).not.toBe("unknown");
    });

    it("returns empty props when no defineProps", () => {
      const meta = snapshotToMeta(makeSnapshot(), "Comp.vue");
      expect(meta.props).toEqual([]);
    });
  });

  describe("composition API events", () => {
    it("extracts events from defineEmits", () => {
      const snapshot = makeCompositionSnapshot([], [{ name: "click" }, { name: "update" }], {
        emitDefinitions: [
          { eventName: "click", hasValidator: false, isDeclared: true },
          { eventName: "update", hasValidator: true, isDeclared: true },
        ],
      });
      const meta = snapshotToMeta(snapshot, "Comp.vue");

      expect(meta.events).toHaveLength(2);
      expect(meta.events[0].name).toBe("click");
      expect(meta.events[0].hasValidator).toBe(false);
      expect(meta.events[1].hasValidator).toBe(true);

      // Negative: should not have stale event names
      expect(meta.events.map((e) => e.name)).not.toContain("hover");
    });

    it("returns empty events when no defineEmits", () => {
      const meta = snapshotToMeta(makeSnapshot(), "Comp.vue");
      expect(meta.events).toEqual([]);
    });
  });

  describe("slots", () => {
    it("extracts basic default slot", () => {
      const snapshot = makeCompositionSnapshot([], [], {
        definedSlots: [{ name: "default", hasBindings: false }],
      });
      const meta = snapshotToMeta(snapshot, "Comp.vue");

      expect(meta.slots).toHaveLength(1);
      expect(meta.slots[0].name).toBe("default");
      expect(meta.slots[0].isScoped).toBe(false);
      // Negative: bindings should be empty
      expect(meta.slots[0].bindings).toEqual([]);
    });

    it("extracts named slots", () => {
      const snapshot = makeCompositionSnapshot([], [], {
        definedSlots: [
          { name: "header", hasBindings: false },
          { name: "footer", hasBindings: false },
          { name: "sidebar", hasBindings: false },
        ],
      });
      const meta = snapshotToMeta(snapshot, "Comp.vue");

      expect(meta.slots).toHaveLength(3);
      expect(meta.slots[0].name).toBe("header");
      expect(meta.slots[1].name).toBe("footer");
      expect(meta.slots[2].name).toBe("sidebar");
      // Negative: no "default" slot when not declared
      expect(meta.slots.map((s) => s.name)).not.toContain("default");
    });

    it("extracts scoped slot with bindings", () => {
      const snapshot = makeCompositionSnapshot([], [], {
        definedSlots: [{ name: "item", hasBindings: true, bindingNames: ["row", "index"] }],
      });
      const meta = snapshotToMeta(snapshot, "Comp.vue");

      expect(meta.slots[0].isScoped).toBe(true);
      expect(meta.slots[0].bindings).toHaveLength(2);
      expect(meta.slots[0].bindings[0].name).toBe("row");
      expect(meta.slots[0].bindings[1].name).toBe("index");
      // Negative: should not include undeclared binding names
      expect(meta.slots[0].bindings.map((b) => b.name)).not.toContain("column");
    });

    it("extracts multiple scoped slots with different bindings", () => {
      const snapshot = makeCompositionSnapshot([], [], {
        definedSlots: [
          { name: "header", hasBindings: true, bindingNames: ["title"] },
          { name: "body", hasBindings: true, bindingNames: ["items", "loading"] },
        ],
      });
      const meta = snapshotToMeta(snapshot, "Comp.vue");

      expect(meta.slots).toHaveLength(2);
      expect(meta.slots[0].bindings).toHaveLength(1);
      expect(meta.slots[0].bindings[0].name).toBe("title");
      expect(meta.slots[1].bindings).toHaveLength(2);
      // Negative: bindings should not leak between slots
      expect(meta.slots[0].bindings.map((b) => b.name)).not.toContain("items");
      expect(meta.slots[1].bindings.map((b) => b.name)).not.toContain("title");
    });

    it("handles slot with single binding", () => {
      const snapshot = makeCompositionSnapshot([], [], {
        definedSlots: [{ name: "default", hasBindings: true, bindingNames: ["item"] }],
      });
      const meta = snapshotToMeta(snapshot, "Comp.vue");

      expect(meta.slots[0].isScoped).toBe(true);
      expect(meta.slots[0].bindings).toHaveLength(1);
      expect(meta.slots[0].bindings[0].name).toBe("item");
    });

    it("handles empty template (no slots)", () => {
      const snapshot = makeCompositionSnapshot([], [], {});
      const meta = snapshotToMeta(snapshot, "Comp.vue");

      expect(meta.slots).toEqual([]);
      // Negative: not undefined/null
      expect(meta.slots).not.toBeNull();
      expect(meta.slots).not.toBeUndefined();
    });

    it("handles null template", () => {
      const meta = snapshotToMeta(makeSnapshot(), "Comp.vue");

      expect(meta.slots).toEqual([]);
    });

    it("extracts slot with many bindings", () => {
      const snapshot = makeCompositionSnapshot([], [], {
        definedSlots: [{ name: "row", hasBindings: true, bindingNames: ["a", "b", "c", "d"] }],
      });
      const meta = snapshotToMeta(snapshot, "Comp.vue");

      expect(meta.slots[0].bindings).toHaveLength(4);
      expect(meta.slots[0].bindings.map((b) => b.name)).toEqual(["a", "b", "c", "d"]);
    });
  });

  describe("models", () => {
    it("extracts defineModel macros", () => {
      const snapshot = makeCompositionSnapshot([], [], null, [
        {
          kind: "DefineModel",
          isTypeBased: false,
          typeReferences: [],
          bindingName: "model",
          modelName: null,
          hasInheritAttrsFalse: false,
          propFields: [{ name: "modelValue", typeAnnotation: "string", spanStart: 0, spanEnd: 0 }],
          spanStart: 0,
          spanEnd: 0,
        },
        {
          kind: "DefineModel",
          isTypeBased: false,
          typeReferences: [],
          bindingName: "count",
          modelName: "count",
          hasInheritAttrsFalse: false,
          propFields: [{ name: "count", typeAnnotation: "number", spanStart: 0, spanEnd: 0 }],
          spanStart: 0,
          spanEnd: 0,
        },
      ]);
      const meta = snapshotToMeta(snapshot, "Comp.vue");

      expect(meta.models).toHaveLength(2);
      expect(meta.models[0].name).toBe("modelValue");
      expect(meta.models[0].type).toEqual({ kind: "primitive", name: "string" });
      expect(meta.models[1].name).toBe("count");
      expect(meta.models[1].type).toEqual({ kind: "primitive", name: "number" });

      // Negative: model names should not include "DefineModel"
      expect(meta.models.map((m) => m.name)).not.toContain("DefineModel");
    });
  });

  describe("options API", () => {
    it("extracts options API props with runtime types", () => {
      const snapshot = makeSnapshot({
        scriptFlags: 1 << 19,
        optionsApi: {
          isDefineComponent: true,
          props: [
            { name: "title", typeConstructor: "String", isRequired: true, hasDefault: false },
            { name: "count", typeConstructor: "Number", isRequired: false, hasDefault: true },
          ],
        },
      });
      const meta = snapshotToMeta(snapshot, "Comp.vue");

      expect(meta.props).toHaveLength(2);
      expect(meta.props[0].name).toBe("title");
      expect(meta.props[0].type).toEqual({ kind: "primitive", name: "string" });
      expect(meta.props[0].runtimeTypes).toEqual(["String"]);
      expect(meta.props[0].required).toBe(true);

      expect(meta.props[1].name).toBe("count");
      expect(meta.props[1].type).toEqual({ kind: "primitive", name: "number" });
      expect(meta.props[1].required).toBe(false);
      expect(meta.props[1].hasDefault).toBe(true);

      // Negative: should not have rawType for runtime-only props
      expect(meta.props[0].rawType).toBeUndefined();
    });

    it("extracts options API expose", () => {
      const snapshot = makeSnapshot({
        scriptFlags: 1 << 19,
        optionsApi: {
          isDefineComponent: true,
          expose: [{ name: "focus" }, { name: "reset" }],
        },
      });
      const meta = snapshotToMeta(snapshot, "Comp.vue");

      expect(meta.exposed).toHaveLength(2);
      expect(meta.exposed[0].name).toBe("focus");
      expect(meta.exposed[1].name).toBe("reset");
    });
  });

  // ── New extraction tests ───────────────────────────────────

  describe("components", () => {
    it("extracts child component usages from template", () => {
      const snapshot = makeSnapshot({
        template: {
          components: [
            {
              name: "MyButton",
              importSource: "./MyButton.vue",
              isDynamic: false,
              props: [{ name: "label" }, { name: "disabled" }],
              hasSpread: false,
              slotsUsed: ["default"],
              staticClasses: ["btn", "primary"],
              hasDynamicClass: false,
              vModels: [],
            },
            {
              name: "Icon",
              isDynamic: false,
              props: [{ name: "name" }],
              hasSpread: false,
              slotsUsed: [],
              staticClasses: [],
              hasDynamicClass: true,
              vModels: [],
            },
          ],
        },
      });
      const meta = snapshotToMeta(snapshot, "Comp.vue");

      expect(meta.components).toHaveLength(2);
      expect(meta.components[0].name).toBe("MyButton");
      expect(meta.components[0].importSource).toBe("./MyButton.vue");
      expect(meta.components[0].isDynamic).toBe(false);
      expect(meta.components[0].props).toEqual(["label", "disabled"]);
      expect(meta.components[0].slotsUsed).toEqual(["default"]);
      expect(meta.components[0].staticClasses).toEqual(["btn", "primary"]);
      expect(meta.components[0].hasDynamicClass).toBe(false);

      expect(meta.components[1].name).toBe("Icon");
      expect(meta.components[1].importSource).toBeUndefined();
      expect(meta.components[1].hasDynamicClass).toBe(true);

      // Negative: importSource should not be present for unresolved components
      expect(meta.components[1]).not.toHaveProperty("importSource");
    });

    it("extracts dynamic component", () => {
      const snapshot = makeSnapshot({
        template: {
          components: [
            {
              name: "component",
              isDynamic: true,
              props: [],
              hasSpread: false,
              slotsUsed: [],
              staticClasses: [],
              hasDynamicClass: false,
              vModels: [],
            },
          ],
        },
      });
      const meta = snapshotToMeta(snapshot, "Comp.vue");

      expect(meta.components[0].isDynamic).toBe(true);
    });

    it("extracts v-model bindings on components", () => {
      const snapshot = makeSnapshot({
        template: {
          components: [
            {
              name: "MyInput",
              isDynamic: false,
              props: [],
              hasSpread: false,
              slotsUsed: [],
              staticClasses: [],
              hasDynamicClass: false,
              vModels: [{ name: "modelValue" }, { name: "search" }],
            },
          ],
        },
      });
      const meta = snapshotToMeta(snapshot, "Comp.vue");

      expect(meta.components[0].vModels).toEqual(["modelValue", "search"]);
      // Negative: should not contain prop names in vModels
      expect(meta.components[0].vModels).not.toContain("label");
    });

    it("returns empty array for null template", () => {
      const meta = snapshotToMeta(makeSnapshot(), "Comp.vue");
      expect(meta.components).toEqual([]);
    });
  });

  describe("templateRefs", () => {
    it("extracts template refs", () => {
      const snapshot = makeSnapshot({
        template: {
          templateRefs: [
            { name: "inputEl", isDynamic: false, targetTag: "input" },
            { name: "container", isDynamic: false, targetTag: "div" },
          ],
        },
      });
      const meta = snapshotToMeta(snapshot, "Comp.vue");

      expect(meta.templateRefs).toHaveLength(2);
      expect(meta.templateRefs[0].name).toBe("inputEl");
      expect(meta.templateRefs[0].isDynamic).toBe(false);
      expect(meta.templateRefs[1].name).toBe("container");
    });

    it("extracts dynamic refs", () => {
      const snapshot = makeSnapshot({
        template: {
          templateRefs: [{ name: "dynamic", isDynamic: true, targetTag: "div" }],
        },
      });
      const meta = snapshotToMeta(snapshot, "Comp.vue");

      expect(meta.templateRefs[0].isDynamic).toBe(true);
    });

    it("returns empty array for null template", () => {
      const meta = snapshotToMeta(makeSnapshot(), "Comp.vue");
      expect(meta.templateRefs).toEqual([]);
    });
  });

  describe("imports", () => {
    it("extracts import metadata", () => {
      const snapshot = makeSnapshot({
        imports: [
          {
            source: "vue",
            isTypeOnly: false,
            bindings: [
              { name: "ref", isTypeOnly: false },
              { name: "computed", isTypeOnly: false },
            ],
          },
          {
            source: "./types",
            isTypeOnly: true,
            bindings: [{ name: "UserType", isTypeOnly: true }],
          },
        ],
      });
      const meta = snapshotToMeta(snapshot, "Comp.vue");

      expect(meta.imports).toHaveLength(2);
      expect(meta.imports[0].source).toBe("vue");
      expect(meta.imports[0].isTypeOnly).toBe(false);
      expect(meta.imports[0].bindings).toHaveLength(2);
      expect(meta.imports[0].bindings[0].name).toBe("ref");

      expect(meta.imports[1].source).toBe("./types");
      expect(meta.imports[1].isTypeOnly).toBe(true);

      // Negative: type-only import bindings should not be mixed
      expect(meta.imports[0].bindings.every((b) => !b.isTypeOnly)).toBe(true);
    });

    it("returns empty array for no imports", () => {
      const meta = snapshotToMeta(makeSnapshot(), "Comp.vue");
      expect(meta.imports).toEqual([]);
    });
  });

  describe("bindings", () => {
    it("extracts script bindings with reactivity", () => {
      const snapshot = makeSnapshot({
        bindings: [
          {
            name: "count",
            kind: "Const",
            reactivityKind: "Ref",
            usedInScript: true,
            usedInStyle: false,
          },
          {
            name: "items",
            kind: "Const",
            reactivityKind: "Reactive",
            usedInScript: true,
            usedInStyle: false,
          },
          {
            name: "doubled",
            kind: "Const",
            reactivityKind: "Computed",
            usedInScript: false,
            usedInStyle: false,
          },
          {
            name: "onClick",
            kind: "Function",
            reactivityKind: "None",
            usedInScript: false,
            usedInStyle: false,
          },
        ],
        template: {
          bindingOccurrences: [{ name: "count" }, { name: "doubled" }],
        },
      });
      const meta = snapshotToMeta(snapshot, "Comp.vue");

      expect(meta.bindings).toHaveLength(4);
      expect(meta.bindings[0].name).toBe("count");
      expect(meta.bindings[0].reactivityKind).toBe("ref");
      expect(meta.bindings[0].usedInTemplate).toBe(true);
      expect(meta.bindings[0].usedInStyle).toBe(false);

      expect(meta.bindings[1].reactivityKind).toBe("reactive");
      expect(meta.bindings[1].usedInTemplate).toBe(false);

      expect(meta.bindings[2].reactivityKind).toBe("computed");
      expect(meta.bindings[2].usedInTemplate).toBe(true);

      expect(meta.bindings[3].reactivityKind).toBe("none");

      // Negative: items is not used in template
      expect(meta.bindings[1].usedInTemplate).toBe(false);
    });

    it("detects usedInStyle", () => {
      const snapshot = makeSnapshot({
        bindings: [
          {
            name: "color",
            kind: "Const",
            reactivityKind: "Ref",
            usedInScript: false,
            usedInStyle: true,
          },
        ],
      });
      const meta = snapshotToMeta(snapshot, "Comp.vue");

      expect(meta.bindings[0].usedInStyle).toBe(true);
    });
  });

  describe("vueApiCalls", () => {
    it("extracts Vue API call sites", () => {
      const snapshot = makeSnapshot({
        vueApiCalls: [
          { api: "OnMounted", spanStart: 0, spanEnd: 10 },
          { api: "Watch", spanStart: 20, spanEnd: 40, argValue: "count" },
          { api: "Provide", spanStart: 50, spanEnd: 60, argValue: "theme" },
        ],
      });
      const meta = snapshotToMeta(snapshot, "Comp.vue");

      expect(meta.vueApiCalls).toHaveLength(3);
      expect(meta.vueApiCalls[0].api).toBe("OnMounted");
      expect(meta.vueApiCalls[0].argValue).toBeUndefined();

      expect(meta.vueApiCalls[1].api).toBe("Watch");
      expect(meta.vueApiCalls[1].argValue).toBe("count");

      expect(meta.vueApiCalls[2].api).toBe("Provide");
      expect(meta.vueApiCalls[2].argValue).toBe("theme");

      // Negative: should not include span information in the output
      expect(meta.vueApiCalls[0]).not.toHaveProperty("spanStart");
      expect(meta.vueApiCalls[0]).not.toHaveProperty("spanEnd");
    });

    it("returns empty array when no vueApiCalls", () => {
      const meta = snapshotToMeta(makeSnapshot(), "Comp.vue");
      expect(meta.vueApiCalls).toEqual([]);
    });
  });

  describe("styles", () => {
    it("extracts style block analysis", () => {
      const snapshot = makeSnapshot({
        styles: [
          {
            lang: "Scss",
            scoped: true,
            isModule: false,
            vBinds: [{ expression: "color" }],
            css: {
              selectors: [
                { text: ".btn", specificity: [0, 1, 0] },
                { text: "#app", specificity: [1, 0, 0] },
              ],
              classes: [{ name: "btn" }, { name: "active" }],
              ids: [{ name: "app" }],
              customProperties: [{ name: "--primary-color" }],
            },
          },
        ],
      });
      const meta = snapshotToMeta(snapshot, "Comp.vue");

      expect(meta.styles).toHaveLength(1);
      expect(meta.styles[0].lang).toBe("Scss");
      expect(meta.styles[0].scoped).toBe(true);
      expect(meta.styles[0].isModule).toBe(false);
      expect(meta.styles[0].classes).toEqual(["btn", "active"]);
      expect(meta.styles[0].ids).toEqual(["app"]);
      expect(meta.styles[0].customProperties).toEqual(["--primary-color"]);
      expect(meta.styles[0].vBinds).toEqual(["color"]);
      expect(meta.styles[0].selectors).toHaveLength(2);
      expect(meta.styles[0].selectors[0]).toEqual({ text: ".btn", specificity: [0, 1, 0] });

      // Negative: moduleName should not be present when not a module
      expect(meta.styles[0].moduleName).toBeUndefined();
    });

    it("extracts CSS module style block", () => {
      const snapshot = makeSnapshot({
        styles: [
          {
            lang: "Css",
            scoped: false,
            isModule: true,
            moduleName: "styles",
            css: {
              classes: [{ name: "wrapper" }],
              selectors: [],
              ids: [],
              customProperties: [],
            },
          },
        ],
      });
      const meta = snapshotToMeta(snapshot, "Comp.vue");

      expect(meta.styles[0].isModule).toBe(true);
      expect(meta.styles[0].moduleName).toBe("styles");
    });

    it("handles style block with no CSS analysis", () => {
      const snapshot = makeSnapshot({
        styles: [
          {
            lang: "Stylus",
            scoped: false,
            isModule: false,
            css: null,
          },
        ],
      });
      const meta = snapshotToMeta(snapshot, "Comp.vue");

      expect(meta.styles[0].lang).toBe("Stylus");
      expect(meta.styles[0].classes).toEqual([]);
      expect(meta.styles[0].selectors).toEqual([]);
      expect(meta.styles[0].ids).toEqual([]);
      expect(meta.styles[0].customProperties).toEqual([]);
    });

    it("returns empty array for no styles", () => {
      const meta = snapshotToMeta(makeSnapshot(), "Comp.vue");
      expect(meta.styles).toEqual([]);
    });
  });

  describe("flags", () => {
    it("extracts component flags from scriptFlags", () => {
      const scriptFlags =
        (1 << 0) | // ASYNC_SETUP
        (1 << 11) | // HAS_REACTIVE_STATE
        (1 << 12) | // HAS_COMPUTED
        (1 << 14) | // HAS_LIFECYCLE_HOOKS
        (1 << 15); // HAS_PROVIDE
      const snapshot = makeSnapshot({ scriptFlags });
      const meta = snapshotToMeta(snapshot, "Comp.vue");

      expect(meta.flags.asyncSetup).toBe(true);
      expect(meta.flags.hasReactiveState).toBe(true);
      expect(meta.flags.hasComputed).toBe(true);
      expect(meta.flags.hasLifecycleHooks).toBe(true);
      expect(meta.flags.hasProvide).toBe(true);

      // Negative: flags not set should be false
      expect(meta.flags.hasWatchers).toBe(false);
      expect(meta.flags.hasInject).toBe(false);
      expect(meta.flags.hasInheritAttrsFalse).toBe(false);
      expect(meta.flags.hasStoreUsage).toBe(false);
    });

    it("all flags false for empty scriptFlags", () => {
      const meta = snapshotToMeta(makeSnapshot(), "Comp.vue");

      expect(meta.flags.asyncSetup).toBe(false);
      expect(meta.flags.hasReactiveState).toBe(false);
      expect(meta.flags.hasComputed).toBe(false);
      expect(meta.flags.hasWatchers).toBe(false);
      expect(meta.flags.hasLifecycleHooks).toBe(false);
      expect(meta.flags.hasProvide).toBe(false);
      expect(meta.flags.hasInject).toBe(false);
      expect(meta.flags.hasInheritAttrsFalse).toBe(false);
      expect(meta.flags.hasStoreUsage).toBe(false);
    });

    it("detects store usage and watchers", () => {
      const scriptFlags = (1 << 13) | (1 << 20); // HAS_WATCHERS | HAS_STORE_USAGE
      const snapshot = makeSnapshot({ scriptFlags });
      const meta = snapshotToMeta(snapshot, "Comp.vue");

      expect(meta.flags.hasWatchers).toBe(true);
      expect(meta.flags.hasStoreUsage).toBe(true);
    });
  });
});

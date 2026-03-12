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

    it("mixed options + composition prefers composition path", () => {
      const snapshot = makeSnapshot({
        scriptFlags: (1 << 19) | (1 << 1), // HAS_OPTIONS_API | HAS_DEFINE_PROPS
        optionsApi: {
          isDefineComponent: true,
          props: [
            { name: "optProp", typeConstructor: "String", isRequired: true, hasDefault: false },
          ],
        },
        macros: [
          {
            kind: "DefineProps",
            isTypeBased: true,
            typeReferences: [],
            bindingName: null,
            hasInheritAttrsFalse: false,
            propFields: [{ name: "compProp", typeAnnotation: "string", spanStart: 0, spanEnd: 0 }],
            spanStart: 0,
            spanEnd: 0,
          },
        ],
        template: {
          propDefinitions: [
            {
              name: "compProp",
              typeAnnotation: "string",
              hasDefault: false,
              isRequired: true,
              isBoolean: false,
              usedInTemplate: true,
              usedInScript: false,
            },
          ],
        },
      });
      const meta = snapshotToMeta(snapshot, "Comp.vue");

      // optionsApi flag is true (flag is set)
      expect(meta.optionsApi).toBe(true);
      // But props come from composition (defineProps), not options
      expect(meta.props).toHaveLength(1);
      expect(meta.props[0].name).toBe("compProp");
      // Negative: options prop should not leak through
      expect(meta.props.map((p) => p.name)).not.toContain("optProp");
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

    it("marks props as hasDefault when withDefaults macro is present", () => {
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
          ],
        },
        [
          {
            kind: "WithDefaults",
            isTypeBased: false,
            typeReferences: [],
            bindingName: null,
            hasInheritAttrsFalse: false,
            spanStart: 0,
            spanEnd: 0,
          },
        ],
      );
      const meta = snapshotToMeta(snapshot, "Comp.vue");

      // msg has explicit propDefinition with hasDefault:false + isRequired:true → required
      expect(meta.props[0].name).toBe("msg");
      expect(meta.props[0].required).toBe(true);
      expect(meta.props[0].hasDefault).toBe(false);

      // count has no propDefinition, so withDefaults presence sets hasDefault
      expect(meta.props[1].name).toBe("count");
      expect(meta.props[1].hasDefault).toBe(true);
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

    it("includes binding expressions on scoped slots", () => {
      const snapshot = makeCompositionSnapshot([], [], {
        definedSlots: [
          {
            name: "item",
            hasBindings: true,
            bindingNames: ["row", "index"],
            bindingExpressions: ["row", "i"],
          },
        ],
      });
      const meta = snapshotToMeta(snapshot, "Comp.vue");

      expect(meta.slots[0].bindings[0].expression).toBe("row");
      expect(meta.slots[0].bindings[1].expression).toBe("i");
      // Negative: expression should not be the binding name when they differ
      expect(meta.slots[0].bindings[1].expression).not.toBe("index");
    });

    it("handles scoped slot with hasBindings true but no bindingNames", () => {
      const snapshot = makeCompositionSnapshot([], [], {
        definedSlots: [{ name: "fallback", hasBindings: true }],
      });
      const meta = snapshotToMeta(snapshot, "Comp.vue");

      expect(meta.slots[0].isScoped).toBe(true);
      expect(meta.slots[0].bindings).toEqual([]);
      // Negative: bindings should not be undefined
      expect(meta.slots[0].bindings).not.toBeUndefined();
    });

    it("resolves binding types from defineSlots slotFields bindings", () => {
      const snapshot = makeCompositionSnapshot(
        [],
        [],
        {
          definedSlots: [{ name: "default", hasBindings: true, bindingNames: ["item", "index"] }],
        },
        [
          {
            kind: "DefineSlots",
            isTypeBased: true,
            typeReferences: [],
            bindingName: null,
            hasInheritAttrsFalse: false,
            slotFields: [
              {
                name: "default",
                isRequired: true,
                spanStart: 0,
                spanEnd: 0,
                bindings: [
                  { name: "item", typeAnnotation: "string" },
                  { name: "index", typeAnnotation: "number" },
                ],
              },
            ],
            spanStart: 0,
            spanEnd: 0,
          },
        ],
      );
      const meta = snapshotToMeta(snapshot, "Comp.vue");

      expect(meta.slots[0].bindings).toHaveLength(2);
      expect(meta.slots[0].bindings[0].name).toBe("item");
      expect(meta.slots[0].bindings[0].rawType).toBe("string");
      // Negative: type kind should NOT be "unknown" when type info is available
      expect(meta.slots[0].bindings[0].type.kind).not.toBe("unknown");
      expect(meta.slots[0].bindings[1].name).toBe("index");
      expect(meta.slots[0].bindings[1].rawType).toBe("number");
      expect(meta.slots[0].bindings[1].type.kind).not.toBe("unknown");
    });

    it("falls back to script bindings for slot binding types", () => {
      const snapshot = makeCompositionSnapshot(
        [],
        [],
        {
          definedSlots: [
            {
              name: "default",
              hasBindings: true,
              bindingNames: ["row"],
              bindingExpressions: ["row"],
            },
          ],
        },
        [],
      );
      // Add script bindings with type annotations
      (snapshot as Record<string, unknown>).bindings = [
        { name: "row", kind: "Const", typeAnnotation: "MyRowType" },
      ];
      const meta = snapshotToMeta(snapshot, "Comp.vue");

      expect(meta.slots[0].bindings[0].name).toBe("row");
      expect(meta.slots[0].bindings[0].rawType).toBe("MyRowType");
      // Negative: should not be unknown
      expect(meta.slots[0].bindings[0].type.kind).not.toBe("unknown");
    });

    it("prefers defineSlots bindings over script binding fallback", () => {
      const snapshot = makeCompositionSnapshot(
        [],
        [],
        {
          definedSlots: [
            {
              name: "default",
              hasBindings: true,
              bindingNames: ["item"],
              bindingExpressions: ["item"],
            },
          ],
        },
        [
          {
            kind: "DefineSlots",
            isTypeBased: true,
            typeReferences: [],
            bindingName: null,
            hasInheritAttrsFalse: false,
            slotFields: [
              {
                name: "default",
                isRequired: true,
                spanStart: 0,
                spanEnd: 0,
                bindings: [{ name: "item", typeAnnotation: "string" }],
              },
            ],
            spanStart: 0,
            spanEnd: 0,
          },
        ],
      );
      // Script binding has a different type
      (snapshot as Record<string, unknown>).bindings = [
        { name: "item", kind: "Const", typeAnnotation: "DifferentType" },
      ];
      const meta = snapshotToMeta(snapshot, "Comp.vue");

      // defineSlots type should win
      expect(meta.slots[0].bindings[0].rawType).toBe("string");
      // Negative: should NOT use the script binding type
      expect(meta.slots[0].bindings[0].rawType).not.toBe("DifferentType");
    });

    it("keeps unknown type when no type info available", () => {
      const snapshot = makeCompositionSnapshot([], [], {
        definedSlots: [{ name: "default", hasBindings: true, bindingNames: ["item"] }],
      });
      const meta = snapshotToMeta(snapshot, "Comp.vue");

      expect(meta.slots[0].bindings[0].name).toBe("item");
      expect(meta.slots[0].bindings[0].type.kind).toBe("unknown");
      // Negative: rawType should be undefined when no info
      expect(meta.slots[0].bindings[0].rawType).toBeUndefined();
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
      expect(meta.components[0].props).toEqual([
        { name: "label", isBound: false, constness: "unknown" },
        { name: "disabled", isBound: false, constness: "unknown" },
      ]);
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

    it("extracts v-model bindings on components using Rust bindingName field", () => {
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
              vModels: [{ bindingName: "modelValue" }, { bindingName: "search" }],
            },
          ],
        },
      });
      const meta = snapshotToMeta(snapshot, "Comp.vue");

      expect(meta.components[0].vModels).toEqual(["modelValue", "search"]);
      // Negative: should not contain prop names in vModels
      expect(meta.components[0].vModels).not.toContain("label");
    });

    it("extracts props with isBound and constness", () => {
      const snapshot = makeSnapshot({
        template: {
          components: [
            {
              name: "MyButton",
              isDynamic: false,
              props: [
                { name: "label", isBound: false, constness: "Const" },
                { name: "disabled", isBound: true, constness: "Dynamic" },
                { name: "variant", isBound: true, constness: "Unknown" },
              ],
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

      expect(meta.components[0].props).toHaveLength(3);
      expect(meta.components[0].props[0]).toEqual({
        name: "label",
        isBound: false,
        constness: "const",
      });
      expect(meta.components[0].props[1]).toEqual({
        name: "disabled",
        isBound: true,
        constness: "dynamic",
      });
      expect(meta.components[0].props[2]).toEqual({
        name: "variant",
        isBound: true,
        constness: "unknown",
      });

      // Negative: props should not be plain strings anymore
      expect(typeof meta.components[0].props[0]).not.toBe("string");
    });

    it("handles components with empty optional fields", () => {
      const snapshot = makeSnapshot({
        template: {
          components: [
            {
              name: "Empty",
              isDynamic: false,
              hasDynamicClass: false,
            },
          ],
        },
      });
      const meta = snapshotToMeta(snapshot, "Comp.vue");

      expect(meta.components[0].name).toBe("Empty");
      expect(meta.components[0].props).toEqual([]);
      expect(meta.components[0].slotsUsed).toEqual([]);
      expect(meta.components[0].staticClasses).toEqual([]);
      expect(meta.components[0].vModels).toEqual([]);
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

    it("exposes targetTag on template refs", () => {
      const snapshot = makeSnapshot({
        template: {
          templateRefs: [
            { name: "inputEl", isDynamic: false, targetTag: "input" },
            { name: "modal", isDynamic: false, targetTag: "Modal" },
          ],
        },
      });
      const meta = snapshotToMeta(snapshot, "Comp.vue");

      expect(meta.templateRefs[0].targetTag).toBe("input");
      expect(meta.templateRefs[1].targetTag).toBe("Modal");
      // Negative: targetTag should not be undefined
      expect(meta.templateRefs[0].targetTag).not.toBeUndefined();
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

    it("handles mixed type-only and runtime bindings in a single import", () => {
      const snapshot = makeSnapshot({
        imports: [
          {
            source: "vue",
            isTypeOnly: false,
            bindings: [
              { name: "ref", isTypeOnly: false },
              { name: "PropType", isTypeOnly: true },
              { name: "computed", isTypeOnly: false },
            ],
          },
        ],
      });
      const meta = snapshotToMeta(snapshot, "Comp.vue");

      expect(meta.imports[0].bindings).toHaveLength(3);
      expect(meta.imports[0].bindings[0].isTypeOnly).toBe(false);
      expect(meta.imports[0].bindings[1].isTypeOnly).toBe(true);
      expect(meta.imports[0].bindings[2].isTypeOnly).toBe(false);
      // Negative: overall import should not be type-only
      expect(meta.imports[0].isTypeOnly).toBe(false);
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

    it("maps maybeRef reactivity kind", () => {
      const snapshot = makeSnapshot({
        bindings: [
          {
            name: "val",
            kind: "Const",
            reactivityKind: "MaybeRef",
            usedInScript: false,
            usedInStyle: false,
          },
        ],
      });
      const meta = snapshotToMeta(snapshot, "Comp.vue");
      expect(meta.bindings[0].reactivityKind).toBe("maybeRef");
    });

    it("maps mutable reactivity kind", () => {
      const snapshot = makeSnapshot({
        bindings: [
          {
            name: "val",
            kind: "Let",
            reactivityKind: "Mutable",
            usedInScript: false,
            usedInStyle: false,
          },
        ],
      });
      const meta = snapshotToMeta(snapshot, "Comp.vue");
      expect(meta.bindings[0].reactivityKind).toBe("mutable");
    });

    it("defaults to none for missing reactivityKind", () => {
      const snapshot = makeSnapshot({
        bindings: [{ name: "val", kind: "Const", usedInScript: false, usedInStyle: false }],
      });
      const meta = snapshotToMeta(snapshot, "Comp.vue");
      expect(meta.bindings[0].reactivityKind).toBe("none");
    });

    it("extracts kind and typeAnnotation", () => {
      const snapshot = makeSnapshot({
        bindings: [
          {
            name: "count",
            kind: "Const",
            reactivityKind: "Ref",
            typeAnnotation: "number",
            usedInScript: true,
            usedInStyle: false,
          },
          {
            name: "name",
            kind: "Let",
            reactivityKind: "None",
            typeAnnotation: "string",
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
          {
            name: "fetchData",
            kind: "AsyncFunction",
            reactivityKind: "None",
            usedInScript: false,
            usedInStyle: false,
          },
          {
            name: "MyClass",
            kind: "Class",
            reactivityKind: "None",
            usedInScript: false,
            usedInStyle: false,
          },
          {
            name: "x",
            kind: "Var",
            reactivityKind: "None",
            usedInScript: false,
            usedInStyle: false,
          },
        ],
      });
      const meta = snapshotToMeta(snapshot, "Comp.vue");

      expect(meta.bindings[0].kind).toBe("const");
      expect(meta.bindings[0].typeAnnotation).toBe("number");
      expect(meta.bindings[1].kind).toBe("let");
      expect(meta.bindings[1].typeAnnotation).toBe("string");
      expect(meta.bindings[2].kind).toBe("function");
      expect(meta.bindings[2].typeAnnotation).toBeUndefined();
      expect(meta.bindings[3].kind).toBe("asyncFunction");
      expect(meta.bindings[4].kind).toBe("class");
      expect(meta.bindings[5].kind).toBe("var");

      // Negative: kind should not be the raw Rust PascalCase form
      expect(meta.bindings[0].kind).not.toBe("Const");
      expect(meta.bindings[2].kind).not.toBe("Function");
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

    it("treats null argValue same as absent", () => {
      const snapshot = makeSnapshot({
        vueApiCalls: [
          { api: "OnMounted", argValue: null },
          { api: "Watch", argValue: "count" },
        ],
      });
      const meta = snapshotToMeta(snapshot, "Comp.vue");

      expect(meta.vueApiCalls[0].argValue).toBeUndefined();
      expect(meta.vueApiCalls[1].argValue).toBe("count");
      // Negative: null should not appear
      expect(meta.vueApiCalls[0]).not.toHaveProperty("argValue");
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

    it("extracts multiple style blocks with different configs", () => {
      const snapshot = makeSnapshot({
        styles: [
          {
            lang: "Scss",
            scoped: true,
            isModule: false,
            css: { classes: [{ name: "a" }], selectors: [], ids: [], customProperties: [] },
          },
          {
            lang: "Css",
            scoped: false,
            isModule: true,
            moduleName: "classes",
            css: { classes: [{ name: "b" }], selectors: [], ids: [], customProperties: [] },
          },
        ],
      });
      const meta = snapshotToMeta(snapshot, "Comp.vue");

      expect(meta.styles).toHaveLength(2);
      expect(meta.styles[0].lang).toBe("Scss");
      expect(meta.styles[0].scoped).toBe(true);
      expect(meta.styles[0].isModule).toBe(false);
      expect(meta.styles[1].lang).toBe("Css");
      expect(meta.styles[1].isModule).toBe(true);
      expect(meta.styles[1].moduleName).toBe("classes");
      // Negative: first block should not have moduleName
      expect(meta.styles[0].moduleName).toBeUndefined();
    });

    it("extracts v-binds when CSS analysis is null", () => {
      const snapshot = makeSnapshot({
        styles: [
          {
            lang: "Css",
            scoped: true,
            isModule: false,
            vBinds: [{ expression: "color" }, { expression: "fontSize" }],
            css: null,
          },
        ],
      });
      const meta = snapshotToMeta(snapshot, "Comp.vue");

      expect(meta.styles[0].vBinds).toEqual(["color", "fontSize"]);
      expect(meta.styles[0].classes).toEqual([]);
    });

    it("defaults lang to Css when field is missing", () => {
      const snapshot = makeSnapshot({
        styles: [
          {
            scoped: false,
            isModule: false,
            css: null,
          },
        ],
      });
      const meta = snapshotToMeta(snapshot, "Comp.vue");

      expect(meta.styles[0].lang).toBe("Css");
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

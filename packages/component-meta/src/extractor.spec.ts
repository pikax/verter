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
    scriptFlags: (propFields.length > 0 ? 1 : 0) | (emitFields.length > 0 ? 2 : 0),
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

  describe("API style detection", () => {
    it("detects composition API", () => {
      const snapshot = makeCompositionSnapshot([{ name: "msg", typeAnnotation: "string" }]);
      const meta = snapshotToMeta(snapshot, "Comp.vue");
      expect(meta.apiStyle).toBe("composition");
      expect(meta.apiStyle).not.toBe("options");
    });

    it("detects options API", () => {
      const snapshot = makeSnapshot({
        scriptFlags: 1 << 16, // HAS_OPTIONS_API
        optionsApi: {
          isDefineComponent: true,
          props: [{ name: "msg", typeConstructor: "String", isRequired: true, hasDefault: false }],
        },
      });
      const meta = snapshotToMeta(snapshot, "Comp.vue");
      expect(meta.apiStyle).toBe("options");
      expect(meta.apiStyle).not.toBe("composition");
    });

    it("defaults to composition for empty snapshot", () => {
      const meta = snapshotToMeta(makeSnapshot(), "Comp.vue");
      expect(meta.apiStyle).toBe("composition");
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
    it("extracts slots from template analysis", () => {
      const snapshot = makeCompositionSnapshot([], [], {
        definedSlots: [
          { name: "default", hasBindings: false },
          { name: "header", hasBindings: true, bindingNames: ["title", "subtitle"] },
        ],
      });
      const meta = snapshotToMeta(snapshot, "Comp.vue");

      expect(meta.slots).toHaveLength(2);
      expect(meta.slots[0].name).toBe("default");
      expect(meta.slots[0].isScoped).toBe(false);
      expect(meta.slots[0].bindings).toEqual([]);

      expect(meta.slots[1].name).toBe("header");
      expect(meta.slots[1].isScoped).toBe(true);
      expect(meta.slots[1].bindings).toHaveLength(2);
      expect(meta.slots[1].bindings[0].name).toBe("title");

      // Negative: non-scoped slot should not have bindings
      expect(meta.slots[0].bindings).toHaveLength(0);
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
        scriptFlags: 1 << 16,
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
        scriptFlags: 1 << 16,
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
});

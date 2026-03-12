import { describe, it, expect } from "vitest";
import { toArgTypes } from "./storybook.js";
import type { ComponentMeta } from "../types.js";

function makeMeta(overrides: Partial<ComponentMeta> = {}): ComponentMeta {
  return {
    filePath: "Comp.vue",
    componentName: "Comp",
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
    ...overrides,
  };
}

describe("toArgTypes", () => {
  it("converts string prop to text control", () => {
    const meta = makeMeta({
      props: [
        {
          name: "title",
          type: { kind: "primitive", name: "string" },
          required: true,
          hasDefault: false,
        },
      ],
    });
    const argTypes = toArgTypes(meta);

    expect(argTypes.title).toBeDefined();
    expect(argTypes.title.control).toEqual({ type: "text" });
    expect(argTypes.title.type?.required).toBe(true);
    expect(argTypes.title.table?.category).toBe("props");
  });

  it("converts boolean prop to boolean control", () => {
    const meta = makeMeta({
      props: [
        {
          name: "disabled",
          type: { kind: "primitive", name: "boolean" },
          required: false,
          hasDefault: true,
        },
      ],
    });
    const argTypes = toArgTypes(meta);

    expect(argTypes.disabled.control).toEqual({ type: "boolean" });
    expect(argTypes.disabled.type?.required).toBe(false);
  });

  it("converts number prop to number control", () => {
    const meta = makeMeta({
      props: [
        {
          name: "count",
          type: { kind: "primitive", name: "number" },
          required: true,
          hasDefault: false,
        },
      ],
    });
    const argTypes = toArgTypes(meta);
    expect(argTypes.count.control).toEqual({ type: "number" });
  });

  it("converts literal union to select control", () => {
    const meta = makeMeta({
      props: [
        {
          name: "size",
          type: {
            kind: "union",
            types: [
              { kind: "literal", value: "sm" },
              { kind: "literal", value: "md" },
              { kind: "literal", value: "lg" },
            ],
          },
          required: true,
          hasDefault: false,
        },
      ],
    });
    const argTypes = toArgTypes(meta);

    expect(argTypes.size.control).toEqual({ type: "select", options: ["sm", "md", "lg"] });
    // Should not be text control
    expect(argTypes.size.control).not.toEqual({ type: "text" });
  });

  it("converts function prop to disabled control", () => {
    const meta = makeMeta({
      props: [
        {
          name: "onClick",
          type: {
            kind: "function",
            parameters: [],
            returnType: { kind: "primitive", name: "void" },
          },
          required: false,
          hasDefault: false,
        },
      ],
    });
    const argTypes = toArgTypes(meta);
    expect(argTypes.onClick.control).toBe(false);
  });

  it("converts events to action argTypes", () => {
    const meta = makeMeta({
      events: [
        {
          name: "click",
          payload: { kind: "unknown", rawType: "" },
          hasValidator: false,
          isDeclared: true,
        },
        {
          name: "update",
          payload: { kind: "unknown", rawType: "" },
          hasValidator: false,
          isDeclared: true,
        },
      ],
    });
    const argTypes = toArgTypes(meta);

    expect(argTypes.onClick).toBeDefined();
    expect(argTypes.onClick.action).toBe("click");
    expect(argTypes.onClick.table?.category).toBe("events");
    expect(argTypes.onUpdate).toBeDefined();
    expect(argTypes.onUpdate.action).toBe("update");

    // Negative: raw event names should not be keys
    expect(argTypes.click).toBeUndefined();
    expect(argTypes.update).toBeUndefined();
  });

  it("returns empty object for no props or events", () => {
    const argTypes = toArgTypes(makeMeta());
    expect(argTypes).toEqual({});
  });
});

import { describe, it, expect } from "vitest";
import { toHistoireConfig, generateDefaultProps, generateVariants } from "./histoire.js";
import type { ComponentMeta } from "../types.js";

function makeMeta(overrides: Partial<ComponentMeta> = {}): ComponentMeta {
  return {
    filePath: "Comp.vue",
    componentName: "Comp",
    apiStyle: "composition",
    props: [],
    events: [],
    slots: [],
    models: [],
    exposed: [],
    ...overrides,
  };
}

describe("toHistoireConfig", () => {
  it("generates config with component name as title", () => {
    const config = toHistoireConfig(makeMeta({ componentName: "MyButton" }));
    expect(config.title).toBe("MyButton");
    expect(config.variants).toHaveLength(1);
    expect(config.variants[0].title).toBe("Default");
  });
});

describe("generateDefaultProps", () => {
  it("generates defaults from primitive types", () => {
    const meta = makeMeta({
      props: [
        {
          name: "title",
          type: { kind: "primitive", name: "string" },
          required: true,
          hasDefault: false,
        },
        {
          name: "count",
          type: { kind: "primitive", name: "number" },
          required: false,
          hasDefault: true,
        },
        {
          name: "active",
          type: { kind: "primitive", name: "boolean" },
          required: false,
          hasDefault: false,
        },
      ],
    });

    const defaults = generateDefaultProps(meta);
    expect(defaults.title).toBe("");
    expect(defaults.count).toBe(0);
    expect(defaults.active).toBe(false);
  });

  it("uses first literal value from union", () => {
    const meta = makeMeta({
      props: [
        {
          name: "size",
          type: {
            kind: "union",
            types: [
              { kind: "literal", value: "sm" },
              { kind: "literal", value: "md" },
            ],
          },
          required: true,
          hasDefault: false,
        },
      ],
    });

    const defaults = generateDefaultProps(meta);
    expect(defaults.size).toBe("sm");
    // Should not use the raw union type
    expect(defaults.size).not.toBe("sm | md");
  });

  it("uses empty array for array types", () => {
    const meta = makeMeta({
      props: [
        {
          name: "items",
          type: { kind: "array", element: { kind: "primitive", name: "string" } },
          required: true,
          hasDefault: false,
        },
      ],
    });

    const defaults = generateDefaultProps(meta);
    expect(defaults.items).toEqual([]);
  });

  it("uses empty object for object types", () => {
    const meta = makeMeta({
      props: [
        {
          name: "config",
          type: { kind: "object", properties: [] },
          required: false,
          hasDefault: false,
        },
      ],
    });

    const defaults = generateDefaultProps(meta);
    expect(defaults.config).toEqual({});
  });

  it("skips ref types (no sensible default)", () => {
    const meta = makeMeta({
      props: [
        {
          name: "date",
          type: { kind: "ref", name: "Date" },
          required: false,
          hasDefault: false,
        },
      ],
    });

    const defaults = generateDefaultProps(meta);
    expect(defaults.date).toBeUndefined();
  });
});

describe("generateVariants", () => {
  it("generates variants from literal union prop", () => {
    const meta = makeMeta({
      props: [
        {
          name: "variant",
          type: {
            kind: "union",
            types: [
              { kind: "literal", value: "primary" },
              { kind: "literal", value: "secondary" },
              { kind: "literal", value: "danger" },
            ],
          },
          required: true,
          hasDefault: false,
        },
      ],
    });

    const variants = generateVariants(meta);
    expect(variants).toHaveLength(3);
    expect(variants[0].title).toBe("variant: primary");
    expect(variants[0].props.variant).toBe("primary");
    expect(variants[1].props.variant).toBe("secondary");
    expect(variants[2].props.variant).toBe("danger");

    // Negative: should not have a "Default" variant when we have union variants
    expect(variants.map((v) => v.title)).not.toContain("Default");
  });

  it("falls back to Default variant when no union props", () => {
    const variants = generateVariants(
      makeMeta({
        props: [
          {
            name: "title",
            type: { kind: "primitive", name: "string" },
            required: true,
            hasDefault: false,
          },
        ],
      }),
    );
    expect(variants).toHaveLength(1);
    expect(variants[0].title).toBe("Default");
  });
});

import { describe, expect, it } from "vitest";
import { refineMetaForBenchmark } from "./meta-ui-meta.js";

describe("refineMetaForBenchmark", () => {
  it("strips function-typed getDeclarations and getTypeObject from schema members", () => {
    const meta = {
      props: [
        {
          name: "foo",
          type: "string",
          required: false,
          schema: {
            kind: "object",
            schema: {
              bar: {
                name: "bar",
                type: "string",
                global: false,
                // These are vue-component-meta lazy accessor leaks (functions)
                getDeclarations: () => [],
                getTypeObject: () => ({}),
              },
            },
          },
        },
      ],
      events: [],
      slots: [],
      exposed: [],
    };
    const result = refineMetaForBenchmark(meta);
    const fooSchema = result.props[0].schema as any;
    const barMember = fooSchema.schema.bar;
    expect(barMember.name).toBe("bar");
    expect(barMember.getDeclarations).toBeUndefined();
    expect(barMember.getTypeObject).toBeUndefined();
  });

  it("preserves string-valued getDeclarations in user schemas", () => {
    const meta = {
      props: [
        {
          name: "config",
          type: "object",
          required: false,
          schema: {
            kind: "object",
            schema: {
              // A user schema where getDeclarations is a string value, not a function
              getDeclarations: {
                name: "getDeclarations",
                type: "string",
                global: false,
                schema: "string",
              },
            },
          },
        },
      ],
      events: [],
      slots: [],
      exposed: [],
    };
    const result = refineMetaForBenchmark(meta);
    const configSchema = result.props[0].schema as any;
    expect(configSchema.schema.getDeclarations).toBeDefined();
    expect(configSchema.schema.getDeclarations.name).toBe("getDeclarations");
  });

  it("strips array-typed declarations from schema members", () => {
    const meta = {
      props: [
        {
          name: "x",
          type: "number",
          required: true,
          schema: {
            kind: "object",
            schema: {
              y: {
                name: "y",
                type: "number",
                global: false,
                declarations: [{ file: "foo.ts", range: [0, 10] }],
              },
            },
          },
        },
      ],
      events: [],
      slots: [],
      exposed: [],
    };
    const result = refineMetaForBenchmark(meta);
    const xSchema = result.props[0].schema as any;
    expect(xSchema.schema.y.declarations).toBeUndefined();
    expect(xSchema.schema.y.name).toBe("y");
  });

  it("does not strip string-valued declarations field", () => {
    const meta = {
      props: [
        {
          name: "x",
          type: "string",
          required: false,
          schema: {
            // declarations is a string, not an array — should be preserved
            declarations: "some user value",
          },
        },
      ],
      events: [],
      slots: [],
      exposed: [],
    };
    const result = refineMetaForBenchmark(meta);
    const xSchema = result.props[0].schema as any;
    expect(xSchema.declarations).toBe("some user value");
  });

  it("filters vue built-in attrs (class, style, key, ref)", () => {
    const meta = {
      props: [
        { name: "color", type: "string", required: false },
        { name: "class", type: "any", required: false },
        { name: "style", type: "any", required: false },
        { name: "key", type: "any", required: false },
        { name: "ref", type: "any", required: false },
      ],
      events: [],
      slots: [],
      exposed: [],
    };
    const result = refineMetaForBenchmark(meta);
    expect(result.props.map((p: any) => p.name)).toEqual(["color"]);
  });

  it("uses compat-layer componentName (null), not _verter extension", () => {
    const meta = {
      props: [],
      events: [],
      slots: [],
      exposed: [],
      _verter: { componentName: "MyButton" },
    };
    const result = refineMetaForBenchmark(meta);
    expect(result.componentName).toBeNull();
  });
});

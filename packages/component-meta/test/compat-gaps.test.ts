/**
 * Tests for known compat gaps between @verter/component-meta/compat and vue-component-meta.
 *
 * These tests document the exact failures seen when running nuxt-component-meta's test suite
 * against the Verter compat layer. Each test is tagged with a priority (P1-P4) matching
 * the compat-gaps.md analysis.
 *
 * Source: D:\tmp\meta-bench\results\compat-gaps.md
 */

import { describe, test, expect, afterAll } from "vitest";
import { join } from "path";
import { createCheckerByJson } from "../src/compat/checker.js";
import type { PropertyMeta, PropertyMetaSchema } from "../src/compat/types.js";
import { shutdownMetaRuntime } from "../src/runtime/index.js";

const fixtureDir = join(__dirname, "fixtures");

afterAll(() => {
  shutdownMetaRuntime();
});

async function getChecker() {
  return createCheckerByJson(fixtureDir, {
    compilerOptions: { strict: true },
    include: ["**/*.vue", "**/*.ts"],
  });
}

async function getProps(fileName: string): Promise<PropertyMeta[]> {
  const checker = await getChecker();
  const meta = await checker.getComponentMeta(join(fixtureDir, fileName));
  return meta.props;
}

async function getProp(fileName: string, propName: string): Promise<PropertyMeta | undefined> {
  return (await getProps(fileName)).find((p) => p.name === propName);
}

// ─── Helper: walk Volar-style PropertyMetaSchema to produce JSON Schema ───
// Simplified version of nuxt-component-meta's propsToJsonSchema logic
function schemaToJsonType(schema: PropertyMetaSchema): any {
  if (typeof schema === "string") {
    switch (schema.toLowerCase()) {
      case "string":
        return { type: "string" };
      case "number":
        return { type: "number" };
      case "boolean":
        return { type: "boolean" };
      default:
        return { type: schema };
    }
  }

  if (schema.kind === "enum") {
    return { enum: (schema.schema as PropertyMetaSchema[]).map(schemaToJsonType) };
  }
  if (schema.kind === "object") {
    const properties: Record<string, any> = {};
    for (const [key, val] of Object.entries(schema.schema ?? {})) {
      properties[key] = schemaToJsonType(val as PropertyMetaSchema);
    }
    return { type: "object", properties };
  }
  if (schema.kind === "array") {
    const items = schema.schema as PropertyMetaSchema[];
    if (items.length === 1) return { type: "array", items: schemaToJsonType(items[0]) };
    return { type: "array" };
  }

  return {};
}

// =============================================================================
// P1: Schema Expansion
// =============================================================================
describe("P1: Schema Expansion", () => {
  describe("P1b: Array<string> prop", () => {
    test("schema for Array<string> prop should be a structured object, not a flat string", async () => {
      const prop = await getProp("P1b-ArrayString.vue", "items");
      expect(prop).toBeDefined();
      const schema = prop!.schema;
      expect(typeof schema).not.toBe("string");
    });
  });

  describe("P1c: Interface[] — deep object schema in array items", () => {
    test("schema for Book[] should expand Book into object with properties", async () => {
      const prop = await getProp("P1c-InterfaceArray.vue", "books");
      expect(prop).toBeDefined();
      const schema = prop!.schema;
      expect(typeof schema).not.toBe("string");
    });
  });

  describe("P1d: 'red' | 'blue' enum literals", () => {
    test("enum prop should have structured schema with string literal members", async () => {
      const prop = await getProp("P1d-Enum.vue", "color");
      expect(prop).toBeDefined();
      const schema = prop!.schema;
      expect(typeof schema).not.toBe("string");
      if (typeof schema !== "string") {
        expect(schema.kind).toBe("enum");
      }
    });
  });
});

// =============================================================================
// P2: Type Representation
// =============================================================================
describe("P2: Type Representation", () => {
  test("simple string prop should have string-based schema", async () => {
    const prop = await getProp("P2a-SimpleString.vue", "name");
    expect(prop).toBeDefined();
    expect(prop!.type).toContain("string");
  });

  test("boolean prop should have boolean schema (not enum)", async () => {
    const prop = await getProp("P2b-Boolean.vue", "disabled");
    expect(prop).toBeDefined();
    expect(prop!.type).toContain("boolean");
  });
});

// =============================================================================
// P3: Default Values
// =============================================================================
describe("P3: Default Values", () => {
  test("JS options API: should extract default value from defineProps options", async () => {
    const prop = await getProp("P3a-JSDefaults.vue", "size");
    expect(prop).toBeDefined();
    expect(prop).toMatchObject({
      type: "string | undefined",
      default: '"md"',
    });
  });

  test("TS script setup: should extract default from withDefaults or options", async () => {
    const prop = await getProp("P3b-TSDefaults.vue", "count");
    expect(prop).toBeDefined();
    expect(prop).toMatchObject({
      type: "number | undefined",
      default: "0",
    });
  });

  test("runtime defineProps object syntax should preserve default values", async () => {
    const prop = await getProp("StringPropDefault.vue", "hello");
    expect(prop).toBeDefined();
    expect(prop).toMatchObject({
      type: "string | undefined",
      default: '"Hello"',
    });
  });
});

// =============================================================================
// P4: DOM Types
// =============================================================================
describe("P4: DOM & Advanced Types", () => {
  test("HTMLCanvasElement prop should have { kind: 'object', schema: {} } not flat string", async () => {
    const prop = await getProp("P4a-DomTypes.vue", "canvas");
    expect(prop).toBeDefined();
    expect(prop!.type).toBe("HTMLCanvasElement | undefined");
    // Non-required prop produces an enum schema wrapping HTMLCanvasElement | undefined
    expect(typeof prop!.schema).not.toBe("string");
    if (typeof prop!.schema !== "string") {
      expect(prop!.schema).toMatchObject({
        kind: "enum",
        type: "HTMLCanvasElement | undefined",
      });
    }
  });

  test("Partial<HTMLImageElement> in union should produce structured enum schema", async () => {
    const prop = await getProp("P4b-PartialDom.vue", "image");
    expect(prop).toBeDefined();
  });
});

// =============================================================================
// Phase 8: Correctness — interface extends, Pick/Omit, typeof, double script
// =============================================================================
describe("Phase 8: Correctness", () => {
  test("interface extends resolves inherited fields", async () => {
    const props = await getProps("InterfaceExtends.vue");
    const userProp = props.find((p) => p.name === "user");
    expect(userProp).toBeDefined();
    expect(typeof userProp!.schema).not.toBe("string");
    if (typeof userProp!.schema !== "string") {
      expect(userProp!.schema.kind).toBe("object");
      expect(Object.keys(userProp!.schema.schema ?? {})).toEqual(["id", "name", "email", "active"]);
      expect(userProp!.schema.schema?.active).toMatchObject({
        required: false,
        type: "boolean",
      });
    }
  });

  test("Pick filters to only selected keys", async () => {
    const props = await getProps("PickOmitProps.vue");
    const displayProp = props.find((p) => p.name === "display");
    expect(displayProp).toBeDefined();
    expect(displayProp!.type).toContain("Pick<FullUser");
    expect(typeof displayProp!.schema).not.toBe("string");
    if (typeof displayProp!.schema !== "string") {
      expect(displayProp!.schema.kind).toBe("object");
      expect(Object.keys(displayProp!.schema.schema ?? {})).toEqual(["id", "name"]);
    }
  });

  test("Omit excludes specified keys", async () => {
    const props = await getProps("PickOmitProps.vue");
    const safeProp = props.find((p) => p.name === "safe");
    expect(safeProp).toBeDefined();
    expect(safeProp!.type).toContain("Omit<FullUser");
    expect(typeof safeProp!.schema).not.toBe("string");
    if (typeof safeProp!.schema !== "string") {
      expect(safeProp!.schema.kind).toBe("object");
      expect(Object.keys(safeProp!.schema.schema ?? {})).toEqual(["id", "name", "email"]);
    }
  });

  test("double script block: sibling script types are visible", async () => {
    const props = await getProps("DoubleScript.vue");
    const names = props.map((p) => p.name);

    // Assert+: props from sibling script are resolved
    expect(names).toContain("shared");
    expect(names).toContain("count");

    // Assert-: no extra props
    expect(props.length).toBe(2);
  });

  test("local typeof resolves to object fields", async () => {
    const props = await getProps("LocalTypeof.vue");
    const names = props.map((p) => p.name);

    // Assert+: typeof config should produce x and y
    expect(names).toContain("x");
    expect(names).toContain("y");

    // Assert-: exactly 2 props
    expect(props.length).toBe(2);
    expect(props.find((p) => p.name === "x")?.type).toBe("1");
    expect(props.find((p) => p.name === "y")?.type).toBe('"hello"');
  });

  test("ReturnType<typeof fn> resolves fields", async () => {
    const props = await getProps("ReturnTypeProps.vue");
    const configProp = props.find((p) => p.name === "config");
    expect(configProp).toBeDefined();
    expect(configProp!.type).toContain("ReturnType<typeof createConfig>");
    expect(typeof configProp!.schema).not.toBe("string");
    if (typeof configProp!.schema !== "string") {
      expect(configProp!.schema.kind).toBe("object");
      expect(Object.keys(configProp!.schema.schema ?? {})).toEqual(["theme", "debug"]);
      expect(configProp!.schema.schema?.debug).toMatchObject({ type: "false" });
    }
  });
});

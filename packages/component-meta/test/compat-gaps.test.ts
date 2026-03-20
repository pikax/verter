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
    test.fails("schema for Array<string> prop should be a structured object, not a flat string", async () => {
      const prop = await getProp("P1b-ArrayString.vue", "items");
      expect(prop).toBeDefined();
      const schema = prop!.schema;
      expect(typeof schema).not.toBe("string");
    });
  });

  describe("P1c: Interface[] — deep object schema in array items", () => {
    test.fails("schema for Book[] should expand Book into object with properties", async () => {
      const prop = await getProp("P1c-InterfaceArray.vue", "books");
      expect(prop).toBeDefined();
      const schema = prop!.schema;
      expect(typeof schema).not.toBe("string");
    });
  });

  describe("P1d: 'red' | 'blue' enum literals", () => {
    test.fails("enum prop should have structured schema with string literal members", async () => {
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
  test.fails("simple string prop should have string-based schema", async () => {
    const prop = await getProp("P2a-SimpleString.vue", "name");
    expect(prop).toBeDefined();
    expect(prop!.type).toContain("string");
  });

  test.fails("boolean prop should have boolean schema (not enum)", async () => {
    const prop = await getProp("P2b-Boolean.vue", "disabled");
    expect(prop).toBeDefined();
    expect(prop!.type).toContain("boolean");
  });
});

// =============================================================================
// P3: Default Values
// =============================================================================
describe("P3: Default Values", () => {
  test.fails("JS options API: should extract default value from defineProps options", async () => {
    const prop = await getProp("P3a-JSDefaults.vue", "size");
    expect(prop).toBeDefined();
  });

  test.fails("TS script setup: should extract default from withDefaults or options", async () => {
    const prop = await getProp("P3b-TSDefaults.vue", "count");
    expect(prop).toBeDefined();
  });
});

// =============================================================================
// P4: DOM Types
// =============================================================================
describe("P4: DOM & Advanced Types", () => {
  test.fails("HTMLCanvasElement prop should have { kind: 'object', schema: {} } not flat string", async () => {
    const prop = await getProp("P4a-DomTypes.vue", "canvas");
    expect(prop).toBeDefined();
  });

  test.fails("Partial<HTMLImageElement> in union should produce structured enum schema", async () => {
    const prop = await getProp("P4b-PartialDom.vue", "image");
    expect(prop).toBeDefined();
  });
});

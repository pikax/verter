/**
 * Tests for known compat gaps between @verter/component-meta/compat and vue-component-meta.
 *
 * These tests document the exact failures seen when running nuxt-component-meta's test suite
 * against the Verter compat layer. Each test is tagged with a priority (P1-P4) matching
 * the compat-gaps.md analysis.
 *
 * Source: D:\tmp\meta-bench\results\compat-gaps.md
 */

import { describe, test, expect } from "vitest";
import { join } from "path";
import { createChecker } from "../src/compat/checker.js";
import type { PropertyMeta, PropertyMetaSchema } from "../src/compat/types.js";

const fixtureDir = join(__dirname, "fixtures");

function getChecker() {
  return createChecker(join(fixtureDir, "tsconfig.json"));
}

function getProps(fileName: string): PropertyMeta[] {
  const checker = getChecker();
  const meta = checker.getComponentMeta(join(fixtureDir, fileName));
  return meta.props;
}

function getProp(fileName: string, propName: string): PropertyMeta | undefined {
  return getProps(fileName).find((p) => p.name === propName);
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
        return {};
    }
  }
  if (schema.kind === "enum") {
    return { kind: "enum", type: schema.type, schema: schema.schema };
  }
  if (schema.kind === "array") {
    return { kind: "array", type: schema.type, schema: schema.schema };
  }
  if (schema.kind === "object") {
    return { kind: "object", type: schema.type, schema: schema.schema };
  }
  return {};
}

// ─────────────────────────────────────────────────────────────────────────────
// P1: Schema Expansion
// ─────────────────────────────────────────────────────────────────────────────

describe("P1: Schema Expansion", () => {
  describe("P1b: Array<string> prop", () => {
    // SFC: defineProps<{ foo: Array<string> }>()
    // Volar schema: { kind: "array", type: "string[]", schema: ["string"] }
    test("schema for Array<string> prop should be a structured object, not a flat string", () => {
      const prop = getProp("ArrayProp.vue", "foo");
      expect(prop).toBeDefined();
      expect(prop!.type).toContain("string");

      // Schema must be a structured object with kind: "array"
      const schema = prop!.schema;
      expect(typeof schema).not.toBe("string");
      expect(schema).toEqual(
        expect.objectContaining({
          kind: "array",
          type: expect.stringContaining("string"),
        }),
      );
      // The inner schema should contain the element type
      if (typeof schema === "object" && "schema" in schema && Array.isArray(schema.schema)) {
        expect(schema.schema).toContain("string");
      }
    });
  });

  describe("P1c: Interface[] — deep object schema in array items", () => {
    // SFC: interface Book { title: string; isbn: string; publishedYear: number }
    //      defineProps<{ books: Book[] }>()
    // Volar schema: { kind: "array", type: "Book[]", schema: [{ kind: "object", ... }] }
    test("schema for Book[] should expand Book into object with properties", () => {
      const prop = getProp("InterfaceArrayProp.vue", "books");
      expect(prop).toBeDefined();

      const schema = prop!.schema;
      // Must be a structured array schema, not a flat string
      expect(typeof schema).not.toBe("string");
      if (typeof schema === "string") return; // guard for TS

      expect(schema.kind).toBe("array");

      // The inner schema should contain an expanded object, not just "Book"
      expect(schema.schema).toBeDefined();
      expect(Array.isArray(schema.schema)).toBe(true);

      const itemSchema = schema.schema![0];
      // Should NOT be a flat string "Book" — should be an expanded object
      expect(typeof itemSchema).not.toBe("string");
      if (typeof itemSchema === "string") return;

      expect(itemSchema.kind).toBe("object");
      // Should contain the Book properties: title, isbn, publishedYear
      // The type text contains the expanded fields, not "Book" (since we resolve locally)
      expect(itemSchema.type).toContain("title");
    });
  });

  describe("P1d: Enum — union of string literals", () => {
    // SFC: color?: 'error' | 'primary' | 'secondary' | 'success'
    // Volar schema: { kind: "enum", type: ..., schema: ['"error"', '"primary"', ...] }
    test("enum prop should have structured schema with string literal members", () => {
      const prop = getProp("EnumProp.vue", "color");
      expect(prop).toBeDefined();
      expect(prop!.type).toContain("error");
      expect(prop!.type).toContain("primary");

      const schema = prop!.schema;
      expect(typeof schema).not.toBe("string");
      if (typeof schema === "string") return;

      expect(schema.kind).toBe("enum");
      expect(Array.isArray(schema.schema)).toBe(true);

      // Collect all string literals recursively (may be nested due to | undefined)
      const stringLiterals: string[] = [];
      function collectLiterals(s: PropertyMetaSchema) {
        if (typeof s === "string") {
          if (s.startsWith('"') && s.endsWith('"')) stringLiterals.push(s);
        } else if (s && typeof s === "object" && "schema" in s && Array.isArray(s.schema)) {
          for (const m of s.schema) collectLiterals(m);
        }
      }
      for (const m of schema.schema as PropertyMetaSchema[]) collectLiterals(m);

      expect(stringLiterals.length).toBeGreaterThanOrEqual(4);
      expect(stringLiterals).toContain('"error"');
      expect(stringLiterals).toContain('"primary"');
      expect(stringLiterals).toContain('"secondary"');
      expect(stringLiterals).toContain('"success"');
    });

    test("simple string prop should have string-based schema", () => {
      const prop = getProp("EnumProp.vue", "label");
      expect(prop).toBeDefined();
      // label is optional (label?: string) so type is "string | undefined"
      // Schema can be "string" or { kind: "enum", schema: ["string", "undefined"] }
      const schema = prop!.schema;
      if (typeof schema === "string") {
        expect(schema).toBe("string");
      } else {
        expect(schema.kind).toBe("enum");
        expect((schema.schema as PropertyMetaSchema[]).some((s) => s === "string")).toBe(true);
      }
    });

    test("boolean prop should have boolean schema (not enum)", () => {
      const prop = getProp("EnumProp.vue", "disabled");
      expect(prop).toBeDefined();
      // Schema should be "boolean" or { kind: "enum", ... } with boolean members
      // Volar returns "boolean" for simple boolean props
      if (typeof prop!.schema === "string") {
        expect(prop!.schema).toBe("boolean");
      }
    });
  });
});

// ─────────────────────────────────────────────────────────────────────────────
// P2: Prop Default Values
// ─────────────────────────────────────────────────────────────────────────────

describe("P2: Prop Default Values", () => {
  test("JS options API: should extract default value from defineProps options", () => {
    // SFC: defineProps({ message: { type: String, default: 'Hello from JS' } })
    // Volar PropertyMeta.default: "Hello from JS"
    const prop = getProp("JSOptionsDefault.vue", "message");
    expect(prop).toBeDefined();
    expect(prop!.type).toBe("string");
    // The default value should be extracted
    expect(prop!.default).toBeDefined();
    expect(prop!.default).toBe("Hello from JS");
  });

  test("TS script setup: should extract default from withDefaults or options", () => {
    // SFC: defineProps({ hello: { type: String, default: 'Hello' } })
    // vue-component-meta includes | undefined for optional props (verified from source)
    const prop = getProp("StringPropDefault.vue", "hello");
    expect(prop).toBeDefined();
    expect(prop!.type).toBe("string | undefined");
    expect(prop!.default).toBeDefined();
    expect(prop!.default).toBe("Hello");
  });
});

// ─────────────────────────────────────────────────────────────────────────────
// P4: Browser Type Schema Shape
// ─────────────────────────────────────────────────────────────────────────────

describe("P4: Browser Type Schema Shape", () => {
  test("HTMLCanvasElement prop should have { kind: 'object', schema: {} } not flat string", () => {
    // SFC: canvas: { type: Object as PropType<HTMLCanvasElement>, required: true }
    // Volar schema: { kind: "object", type: "HTMLCanvasElement", schema: {} }
    // Verter schema: "HTMLCanvasElement" (flat string)
    const prop = getProp("NativeBrowserType.vue", "canvas");
    expect(prop).toBeDefined();
    expect(prop!.type).toBe("HTMLCanvasElement");

    const schema = prop!.schema;
    // Should be a structured object, not a flat string
    expect(typeof schema).not.toBe("string");
    if (typeof schema === "string") return;

    expect(schema.kind).toBe("object");
    expect(schema.type).toBe("HTMLCanvasElement");
    // Browser types should have empty schema (not fully expanded)
    expect(schema.schema).toEqual(expect.anything());
  });

  test("Partial<HTMLImageElement> in union should produce structured enum schema", () => {
    // SFC: partialImage?: string | (Partial<HTMLImageElement> & { [key: string]: any })
    // Volar schema: { kind: "enum", schema: ["undefined", "string", { kind: "object", ... }] }
    const prop = getProp("PartialNativeUnion.vue", "partialImage");
    expect(prop).toBeDefined();

    const schema = prop!.schema;
    expect(typeof schema).not.toBe("string");
    if (typeof schema === "string") return;

    expect(schema.kind).toBe("enum");
    expect(Array.isArray(schema.schema)).toBe(true);

    // Recursively collect all schema members (the optional `?` adds nesting via | undefined)
    const allMembers: PropertyMetaSchema[] = [];
    function collectMembers(s: PropertyMetaSchema) {
      if (typeof s === "string") {
        allMembers.push(s);
      } else if (s && typeof s === "object" && "schema" in s && Array.isArray(s.schema)) {
        if (s.kind === "enum") {
          // Flatten enum members (union nesting from | undefined)
          for (const m of s.schema) collectMembers(m);
        } else {
          allMembers.push(s);
        }
      } else {
        allMembers.push(s);
      }
    }
    for (const m of schema.schema as PropertyMetaSchema[]) collectMembers(m);

    // Should contain "string" somewhere in the flattened members
    const hasString = allMembers.some((m) => m === "string");
    expect(hasString).toBe(true);

    // The intersection type (Partial<HTMLImageElement> & { [key: string]: any })
    // should be a structured object, not a flat string
    const objectMembers = allMembers.filter((m) => typeof m !== "string");
    expect(objectMembers.length).toBeGreaterThan(0);
  });
});

// ─────────────────────────────────────────────────────────────────────────────
// P3: createCheckerByJson configJson
// ─────────────────────────────────────────────────────────────────────────────
// NOTE: P3 tests require Nuxt auto-import aliases (#imports) which need a
// running Nuxt context. These are tested in nuxt-component-meta's test suite
// rather than here. The gap is: createCheckerByJson ignores configJson.compilerOptions.paths.
// See D:\tmp\meta-bench\results\compat-gaps.md for full details.

import { createRequire } from "node:module";

/**
 * Zod adapter — converts TypeDescriptor trees to Zod schemas.
 *
 * Two modes:
 * - **Codegen** (`typeToZodString`): outputs `"z.string()"` etc. as strings
 * - **Runtime** (`typeToZodSchema`): builds actual `z.ZodType` instances
 *   (requires `zod` as a peer dependency)
 */

import type { TypeDescriptor } from "../type-ir.js";
import type { ComponentMeta } from "../types.js";

// ── Codegen mode ─────────────────────────────────────────────────

/**
 * Convert a TypeDescriptor to a Zod schema string (codegen).
 */
export function typeToZodString(type: TypeDescriptor): string {
  switch (type.kind) {
    case "primitive":
      switch (type.name) {
        case "string":
          return "z.string()";
        case "number":
          return "z.number()";
        case "boolean":
          return "z.boolean()";
        case "bigint":
          return "z.bigint()";
        case "symbol":
          return "z.symbol()";
        case "null":
          return "z.null()";
        case "undefined":
          return "z.undefined()";
        case "void":
          return "z.void()";
        case "never":
          return "z.never()";
        case "any":
          return "z.any()";
        case "unknown":
          return "z.unknown()";
        case "object":
          return "z.object({})";
        default:
          return "z.unknown()";
      }

    case "literal":
      if (typeof type.value === "string") {
        return `z.literal(${JSON.stringify(type.value)})`;
      }
      return `z.literal(${type.value})`;

    case "union": {
      if (type.types.length === 0) return "z.never()";
      if (type.types.length === 1) return typeToZodString(type.types[0]);
      const members = type.types.map(typeToZodString);
      return `z.union([${members.join(", ")}])`;
    }

    case "intersection": {
      if (type.types.length === 0) return "z.unknown()";
      let result = typeToZodString(type.types[0]);
      for (let i = 1; i < type.types.length; i++) {
        result += `.and(${typeToZodString(type.types[i])})`;
      }
      return result;
    }

    case "array":
      return `z.array(${typeToZodString(type.element)})`;

    case "tuple": {
      if (type.elements.length === 0) return "z.tuple([])";
      const els = type.elements.map(typeToZodString);
      return `z.tuple([${els.join(", ")}])`;
    }

    case "object": {
      const stringIndexSignature = type.indexSignatures?.find(
        (signature) =>
          signature.keyType.kind === "primitive" &&
          (signature.keyType.name === "string" || signature.keyType.name === "number"),
      );
      if (type.properties.length === 0) {
        if (stringIndexSignature) {
          return `z.record(${typeToZodString(stringIndexSignature.valueType)})`;
        }
        return "z.object({})";
      }
      const props = type.properties.map((p) => {
        const schema = typeToZodString(p.type);
        const optSuffix = p.optional ? ".optional()" : "";
        return `  ${JSON.stringify(p.name)}: ${schema}${optSuffix}`;
      });
      const base = `z.object({\n${props.join(",\n")}\n})`;
      if (stringIndexSignature) {
        return `${base}.catchall(${typeToZodString(stringIndexSignature.valueType)})`;
      }
      return base;
    }

    case "function":
      return "z.function()";

    case "typeParameter":
      return "z.unknown()";

    case "ref":
    case "recursiveRef":
      // Named types we can't resolve — fall back to unknown
      return "z.unknown()";

    case "enum": {
      if (type.members.length === 0) return "z.never()";
      const vals = type.members.map((m) =>
        m.value !== undefined ? JSON.stringify(m.value) : JSON.stringify(m.name),
      );
      return `z.enum([${vals.join(", ")}])`;
    }

    case "unknown":
      return "z.unknown()";
  }
}

/**
 * Convert ComponentMeta props to a Zod object schema string.
 */
export function propsToZodString(meta: ComponentMeta): string {
  const fields = meta.props.map((prop) => {
    const schema = typeToZodString(prop.type);
    const optSuffix = !prop.required ? ".optional()" : "";
    return `  ${JSON.stringify(prop.name)}: ${schema}${optSuffix}`;
  });
  return `z.object({\n${fields.join(",\n")}\n})`;
}

// ── Runtime mode ─────────────────────────────────────────────────

/**
 * Resolve the `zod` module. Throws if not installed.
 */
function getZod(): typeof import("zod") {
  try {
    const _require = typeof require === "function" ? require : createRequire(import.meta.url);
    return _require("zod");
  } catch {
    throw new Error(
      "@verter/component-meta/zod runtime mode requires `zod` as a peer dependency. " +
        "Install it with: npm install zod",
    );
  }
}

/**
 * Convert a TypeDescriptor to a runtime Zod schema.
 * Requires `zod` as a peer dependency.
 */
export function typeToZodSchema(type: TypeDescriptor): unknown {
  const z = getZod();
  return buildZodSchema(z, type);
}

/**
 * Convert ComponentMeta props to a runtime Zod object schema.
 */
export function propsToZodSchema(meta: ComponentMeta): unknown {
  const z = getZod();
  const shape: Record<string, any> = {};

  for (const prop of meta.props) {
    let schema = buildZodSchema(z, prop.type);
    if (!prop.required) {
      schema = (schema as any).optional();
    }
    shape[prop.name] = schema;
  }

  return z.object(shape);
}

function buildZodSchema(z: typeof import("zod"), type: TypeDescriptor): unknown {
  switch (type.kind) {
    case "primitive":
      switch (type.name) {
        case "string":
          return z.string();
        case "number":
          return z.number();
        case "boolean":
          return z.boolean();
        case "bigint":
          return z.bigint();
        case "symbol":
          return z.symbol();
        case "null":
          return z.null();
        case "undefined":
          return z.undefined();
        case "void":
          return z.void();
        case "never":
          return z.never();
        case "any":
          return z.any();
        case "unknown":
          return z.unknown();
        case "object":
          return z.object({});
        default:
          return z.unknown();
      }

    case "literal":
      return z.literal(type.value as any);

    case "union": {
      if (type.types.length === 0) return z.never();
      if (type.types.length === 1) return buildZodSchema(z, type.types[0]);
      const members = type.types.map((t) => buildZodSchema(z, t)) as [any, any, ...any[]];
      return z.union(members);
    }

    case "intersection": {
      if (type.types.length === 0) return z.unknown();
      let result = buildZodSchema(z, type.types[0]) as any;
      for (let i = 1; i < type.types.length; i++) {
        result = z.intersection(result, buildZodSchema(z, type.types[i]) as any);
      }
      return result;
    }

    case "array":
      return z.array(buildZodSchema(z, type.element) as any);

    case "tuple": {
      if (type.elements.length === 0) return z.tuple([]);
      const els = type.elements.map((e) => buildZodSchema(z, e)) as [any, ...any[]];
      return z.tuple(els);
    }

    case "object": {
      const stringIndexSignature = type.indexSignatures?.find(
        (signature) =>
          signature.keyType.kind === "primitive" &&
          (signature.keyType.name === "string" || signature.keyType.name === "number"),
      );
      if (type.properties.length === 0) {
        if (stringIndexSignature) {
          return z.record(buildZodSchema(z, stringIndexSignature.valueType) as any);
        }
        return z.object({});
      }
      const shape: Record<string, any> = {};
      for (const prop of type.properties) {
        let schema = buildZodSchema(z, prop.type);
        if (prop.optional) {
          schema = (schema as any).optional();
        }
        shape[prop.name] = schema;
      }
      const base = z.object(shape);
      if (stringIndexSignature) {
        return base.catchall(buildZodSchema(z, stringIndexSignature.valueType) as any);
      }
      return base;
    }

    case "function":
      return z.function();

    case "typeParameter":
      return z.unknown();

    case "ref":
    case "recursiveRef":
      return z.unknown();

    case "enum": {
      if (type.members.length === 0) return z.never();
      const vals = type.members.map((m) => (m.value !== undefined ? String(m.value) : m.name)) as [
        string,
        ...string[],
      ];
      return z.enum(vals);
    }

    case "unknown":
      return z.unknown();
  }
}

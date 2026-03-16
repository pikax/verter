/**
 * TypeDescriptor → PropertyMetaSchema conversion.
 *
 * Maps Verter's Type IR to Volar's `PropertyMetaSchema` shape.
 */

import type { TypeDescriptor } from "../type-ir.js";
import { parseType } from "../resolver.js";
import type { PropertyMetaSchema, MetaCheckerOptions } from "./types.js";

/**
 * Convert a TypeDescriptor to a PropertyMetaSchema.
 *
 * @param td       The type descriptor to convert
 * @param options  Checker options controlling schema generation
 * @param typeRegistry  Optional registry mapping type names to expanded type text
 */
export function typeDescriptorToSchema(
  td: TypeDescriptor,
  options?: MetaCheckerOptions,
  typeRegistry?: Map<string, string>,
): PropertyMetaSchema {
  if (options?.schema === false) {
    return "unknown";
  }

  const ignore = typeof options?.schema === "object" ? options.schema.ignore : undefined;

  return convertType(td, ignore, typeRegistry, new Set());
}

function convertType(
  td: TypeDescriptor,
  ignore?: (type: string) => boolean,
  typeRegistry?: Map<string, string>,
  visited?: Set<string>,
): PropertyMetaSchema {
  switch (td.kind) {
    case "primitive":
      return td.name;

    case "literal":
      return typeof td.value === "string" ? `"${td.value}"` : String(td.value);

    case "union": {
      const type = td.types.map(typeDescriptorToString).join(" | ");
      if (ignore?.(type)) return type;
      // Volar uses Record<string, PropertyMetaSchema> with numeric string keys
      const schema: Record<string, PropertyMetaSchema> = {};
      td.types.forEach((t, i) => {
        schema[String(i)] = convertType(t, ignore, typeRegistry, visited);
      });
      return {
        kind: "enum",
        type,
        schema,
      };
    }

    case "intersection": {
      const type = td.types.map(typeDescriptorToString).join(" & ");
      if (ignore?.(type)) return type;
      // Volar uses Record<string, PropertyMetaSchema> with numeric string keys
      const schema: Record<string, PropertyMetaSchema> = {};
      td.types.forEach((t, i) => {
        schema[String(i)] = convertType(t, ignore, typeRegistry, visited);
      });
      return {
        kind: "object",
        type,
        schema,
      };
    }

    case "array": {
      const type = `${typeDescriptorToString(td.element)}[]`;
      if (ignore?.(type)) return type;
      return {
        kind: "array",
        type,
        schema: [convertType(td.element, ignore, typeRegistry, visited)],
      };
    }

    case "tuple": {
      const type = `[${td.elements.map(typeDescriptorToString).join(", ")}]`;
      if (ignore?.(type)) return type;
      // Volar uses Record<string, PropertyMetaSchema> with numeric string keys for tuples
      const schema: Record<string, PropertyMetaSchema> = {};
      td.elements.forEach((t, i) => {
        schema[String(i)] = convertType(t, ignore, typeRegistry, visited);
      });
      return {
        kind: "array",
        type,
        schema,
      };
    }

    case "object": {
      const type = `{ ${td.properties.map((p) => `${p.name}${p.optional ? "?" : ""}: ${typeDescriptorToString(p.type)}`).join("; ")} }`;
      if (ignore?.(type)) return type;
      // Volar uses Record<string, PropertyMeta-like> with property names as keys.
      // Each value includes name, required, type, description, tags, global, schema.
      // The actual Volar runtime format is richer than the declared PropertyMetaSchema type.
      const schema: Record<string, PropertyMetaSchema> = {};
      td.properties.forEach((p) => {
        // Cast needed: Volar's runtime shape includes PropertyMeta fields
        // (name, required, etc.) beyond what PropertyMetaSchema declares.
        schema[p.name] = {
          name: p.name,
          global: false,
          description: "",
          tags: [],
          required: !p.optional,
          type: typeDescriptorToString(p.type),
          schema: convertType(p.type, ignore, typeRegistry, visited),
        } as unknown as PropertyMetaSchema;
      });
      return {
        kind: "object",
        type,
        schema,
      };
    }

    case "function": {
      const params = td.parameters
        .map((p) => `${p.name}${p.optional ? "?" : ""}: ${typeDescriptorToString(p.type)}`)
        .join(", ");
      return `(${params}) => ${typeDescriptorToString(td.returnType)}`;
    }

    case "enum": {
      if (ignore?.(td.name)) return td.name;
      return {
        kind: "enum",
        type: td.name,
        schema: td.members.map((m) => (m.value !== undefined ? String(m.value) : m.name)),
      };
    }

    case "ref": {
      const name = td.typeArguments
        ? `${td.name}<${td.typeArguments.map(typeDescriptorToString).join(", ")}>`
        : td.name;
      if (ignore?.(name)) return name;
      // Try to resolve from type registry
      if (typeRegistry && !visited?.has(td.name)) {
        const expanded = typeRegistry.get(td.name);
        if (expanded) {
          visited?.add(td.name);
          const resolved = parseType(expanded);
          const result = convertType(resolved, ignore, typeRegistry, visited);
          visited?.delete(td.name);
          return result;
        }
      }
      // Unresolved ref → structured object with empty schema (matches Volar for browser/external types)
      return { kind: "object" as const, type: name, schema: {} };
    }

    case "unknown":
      return td.rawType || "unknown";
  }
}

/**
 * Convert a TypeDescriptor to a human-readable string representation.
 *
 * Shared utility — also used by the storybook adapter's `typeToSummary`.
 */
export function typeDescriptorToString(td: TypeDescriptor): string {
  switch (td.kind) {
    case "primitive":
      return td.name;
    case "literal":
      return typeof td.value === "string" ? `"${td.value}"` : String(td.value);
    case "union":
      return td.types.map(typeDescriptorToString).join(" | ");
    case "intersection":
      return td.types.map(typeDescriptorToString).join(" & ");
    case "array":
      return `${typeDescriptorToString(td.element)}[]`;
    case "tuple":
      return `[${td.elements.map(typeDescriptorToString).join(", ")}]`;
    case "object":
      return "object";
    case "function":
      return "function";
    case "ref":
      return td.typeArguments
        ? `${td.name}<${td.typeArguments.map(typeDescriptorToString).join(", ")}>`
        : td.name;
    case "enum":
      return td.name;
    case "unknown":
      return td.rawType || "unknown";
  }
}

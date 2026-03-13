/**
 * TypeDescriptor → PropertyMetaSchema conversion.
 *
 * Maps Verter's Type IR to Volar's `PropertyMetaSchema` shape.
 */

import type { TypeDescriptor } from "../type-ir.js";
import type { PropertyMetaSchema, MetaCheckerOptions } from "./types.js";

/**
 * Convert a TypeDescriptor to a PropertyMetaSchema.
 *
 * @param td       The type descriptor to convert
 * @param options  Checker options controlling schema generation
 */
export function typeDescriptorToSchema(
  td: TypeDescriptor,
  options?: MetaCheckerOptions,
): PropertyMetaSchema {
  if (options?.schema === false) {
    return "unknown";
  }

  const ignore = typeof options?.schema === "object" ? options.schema.ignore : undefined;

  return convertType(td, ignore);
}

function convertType(td: TypeDescriptor, ignore?: (type: string) => boolean): PropertyMetaSchema {
  switch (td.kind) {
    case "primitive":
      return td.name;

    case "literal":
      return typeof td.value === "string" ? `"${td.value}"` : String(td.value);

    case "union": {
      const type = td.types.map(typeDescriptorToString).join(" | ");
      if (ignore?.(type)) return type;
      return {
        kind: "enum",
        type,
        schema: td.types.map((t) => convertType(t, ignore)),
      };
    }

    case "intersection": {
      const type = td.types.map(typeDescriptorToString).join(" & ");
      if (ignore?.(type)) return type;
      return {
        kind: "object",
        type,
        schema: td.types.map((t) => convertType(t, ignore)),
      };
    }

    case "array": {
      const type = `${typeDescriptorToString(td.element)}[]`;
      if (ignore?.(type)) return type;
      return {
        kind: "array",
        type,
        schema: [convertType(td.element, ignore)],
      };
    }

    case "tuple": {
      const type = `[${td.elements.map(typeDescriptorToString).join(", ")}]`;
      if (ignore?.(type)) return type;
      return {
        kind: "array",
        type,
        schema: td.elements.map((t) => convertType(t, ignore)),
      };
    }

    case "object": {
      const type = `{ ${td.properties.map((p) => `${p.name}${p.optional ? "?" : ""}: ${typeDescriptorToString(p.type)}`).join("; ")} }`;
      if (ignore?.(type)) return type;
      return {
        kind: "object",
        type,
        schema: td.properties.map((p) => convertType(p.type, ignore)),
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
      return name;
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

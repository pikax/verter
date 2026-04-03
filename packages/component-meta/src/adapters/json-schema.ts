/**
 * JSON Schema adapter — converts TypeDescriptor trees to JSON Schema (draft-07).
 */

import type { TypeDescriptor } from "../type-ir.js";
import type { ComponentMeta } from "../types.js";

export interface JSONSchema {
  type?: string | string[];
  const?: unknown;
  enum?: unknown[];
  items?: JSONSchema | JSONSchema[];
  properties?: Record<string, JSONSchema>;
  required?: string[];
  additionalProperties?: boolean | JSONSchema;
  anyOf?: JSONSchema[];
  allOf?: JSONSchema[];
  $ref?: string;
  description?: string;
  [key: string]: unknown;
}

/**
 * Convert a TypeDescriptor to a JSON Schema object.
 */
export function typeToJsonSchema(type: TypeDescriptor): JSONSchema {
  switch (type.kind) {
    case "primitive":
      return primitiveToJsonSchema(type.name);

    case "literal":
      return { const: type.value };

    case "union": {
      // Flatten nullable: if union contains null, use nullable pattern
      const nonNull = type.types.filter((t) => !(t.kind === "primitive" && t.name === "null"));
      const hasNull = nonNull.length < type.types.length;

      if (nonNull.length === 1 && hasNull) {
        const inner = typeToJsonSchema(nonNull[0]);
        return { anyOf: [inner, { type: "null" }] };
      }

      // Check if all literals of same type → use enum
      if (type.types.every((t) => t.kind === "literal")) {
        return {
          enum: type.types
            .filter(
              (t): t is { kind: "literal"; value: string | number | boolean } =>
                t.kind === "literal",
            )
            .map((t) => t.value),
        };
      }

      return { anyOf: type.types.map(typeToJsonSchema) };
    }

    case "intersection":
      return { allOf: type.types.map(typeToJsonSchema) };

    case "array":
      return { type: "array", items: typeToJsonSchema(type.element) };

    case "tuple":
      return {
        type: "array",
        items: type.elements.map(typeToJsonSchema),
        minItems: type.elements.length,
        maxItems: type.elements.length,
      };

    case "object": {
      const properties: Record<string, JSONSchema> = {};
      const required: string[] = [];

      for (const prop of type.properties) {
        properties[prop.name] = typeToJsonSchema(prop.type);
        if (!prop.optional) {
          required.push(prop.name);
        }
      }

      const stringIndexSignature = type.indexSignatures?.find(
        (signature) =>
          signature.keyType.kind === "primitive" &&
          (signature.keyType.name === "string" || signature.keyType.name === "number"),
      );

      return {
        type: "object",
        properties,
        ...(required.length > 0 && { required }),
        ...(stringIndexSignature
          ? { additionalProperties: typeToJsonSchema(stringIndexSignature.valueType) }
          : {}),
      };
    }

    case "function":
      // Functions can't be represented in JSON Schema
      return {};

    case "typeParameter":
      return { description: type.name };

    case "ref":
    case "recursiveRef":
      // Named type reference — cannot resolve without context
      return { description: type.name };

    case "enum": {
      const values = type.members.map((m) => m.value ?? m.name);
      return { enum: values };
    }

    case "unknown":
      return {};
  }
}

/**
 * Convert ComponentMeta props to a JSON Schema object schema.
 */
export function propsToJsonSchema(meta: ComponentMeta): JSONSchema {
  const properties: Record<string, JSONSchema> = {};
  const required: string[] = [];

  for (const prop of meta.props) {
    properties[prop.name] = typeToJsonSchema(prop.type);
    if (prop.required) {
      required.push(prop.name);
    }
  }

  return {
    type: "object",
    properties,
    ...(required.length > 0 && { required }),
  };
}

function primitiveToJsonSchema(name: string): JSONSchema {
  switch (name) {
    case "string":
      return { type: "string" };
    case "number":
      return { type: "number" };
    case "boolean":
      return { type: "boolean" };
    case "null":
      return { type: "null" };
    case "undefined":
      return {};
    case "bigint":
      return { type: "integer" };
    case "symbol":
      return { type: "string", description: "symbol" };
    case "any":
    case "unknown":
      return {};
    case "void":
      return { type: "null" };
    case "never":
      return { not: {} };
    case "object":
      return { type: "object" };
    default:
      return {};
  }
}

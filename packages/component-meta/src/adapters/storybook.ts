/**
 * Storybook adapter — converts ComponentMeta → Storybook argTypes/controls.
 */

import type { ComponentMeta, PropMeta, EventMeta } from "../types.js";
import type { TypeDescriptor } from "@verter/type-ir";

export interface StorybookArgType {
  name?: string;
  description?: string;
  type?: { name: string; required?: boolean };
  control?: { type: string; options?: (string | number | boolean)[] } | false;
  table?: {
    type?: { summary: string };
    defaultValue?: { summary: string };
    category?: string;
  };
  action?: string;
}

/**
 * Convert ComponentMeta to Storybook argTypes.
 *
 * Events are exposed as `onEventName` actions. Props use appropriate
 * control types (select for literal unions, boolean toggle, etc.).
 */
export function toArgTypes(meta: ComponentMeta): Record<string, StorybookArgType> {
  const argTypes: Record<string, StorybookArgType> = {};

  for (const prop of meta.props) {
    argTypes[prop.name] = propToArgType(prop);
  }

  for (const event of meta.events) {
    argTypes[eventToArgName(event.name)] = eventToArgType(event);
  }

  return argTypes;
}

function propToArgType(prop: PropMeta): StorybookArgType {
  return {
    type: { name: typeToStorybookName(prop.type), required: prop.required },
    control: typeToControl(prop.type),
    table: {
      type: { summary: prop.rawType ?? typeToSummary(prop.type) },
      category: "props",
    },
  };
}

function eventToArgType(event: EventMeta): StorybookArgType {
  return {
    action: event.name,
    table: { category: "events" },
  };
}

function eventToArgName(eventName: string): string {
  return "on" + eventName.charAt(0).toUpperCase() + eventName.slice(1);
}

function typeToControl(type: TypeDescriptor): StorybookArgType["control"] {
  switch (type.kind) {
    case "primitive":
      switch (type.name) {
        case "string":
          return { type: "text" };
        case "number":
          return { type: "number" };
        case "boolean":
          return { type: "boolean" };
        default:
          return false;
      }

    case "union": {
      // Check if all members are literals → use select
      const allLiterals = type.types.every(
        (t) => t.kind === "literal" || (t.kind === "primitive" && t.name === "null"),
      );
      if (allLiterals) {
        const options = type.types
          .filter(
            (t): t is { kind: "literal"; value: string | number | boolean } => t.kind === "literal",
          )
          .map((t) => t.value);
        if (options.length > 0) {
          return { type: "select", options };
        }
      }
      // Check if it's a union of primitive + null (nullable primitive)
      const nonNull = type.types.filter(
        (t) => !(t.kind === "primitive" && (t.name === "null" || t.name === "undefined")),
      );
      if (nonNull.length === 1) {
        return typeToControl(nonNull[0]);
      }
      return { type: "object" };
    }

    case "array":
      return { type: "object" };

    case "object":
      return { type: "object" };

    case "function":
      return false;

    case "typeParameter":
      return false;

    default:
      return { type: "text" };
  }
}

function typeToStorybookName(type: TypeDescriptor): string {
  switch (type.kind) {
    case "primitive":
      return type.name;
    case "literal":
      return typeof type.value;
    case "union":
      return "union";
    case "intersection":
      return "intersection";
    case "array":
      return "array";
    case "tuple":
      return "array";
    case "object":
      return "object";
    case "function":
      return "function";
    case "typeParameter":
      return type.name;
    case "ref":
      return type.name;
    default:
      return "other";
  }
}

function typeToSummary(type: TypeDescriptor): string {
  switch (type.kind) {
    case "primitive":
      return type.name;
    case "literal":
      return typeof type.value === "string" ? `'${type.value}'` : String(type.value);
    case "union":
      return type.types.map(typeToSummary).join(" | ");
    case "intersection":
      return type.types.map(typeToSummary).join(" & ");
    case "array":
      return `${typeToSummary(type.element)}[]`;
    case "tuple":
      return `[${type.elements.map(typeToSummary).join(", ")}]`;
    case "object":
      return "object";
    case "function":
      return "function";
    case "typeParameter":
      return type.name;
    case "ref":
      return type.typeArguments
        ? `${type.name}<${type.typeArguments.map(typeToSummary).join(", ")}>`
        : type.name;
    case "recursiveRef":
      return type.typeArguments.length > 0
        ? `${type.name}<${type.typeArguments.map(typeToSummary).join(", ")}>`
        : type.name;
    case "enum":
      return type.name;
    case "unknown":
      return type.rawType || "unknown";
  }
}

/**
 * Histoire adapter — converts ComponentMeta → Histoire story configuration.
 */

import type { ComponentMeta, PropMeta } from "../types.js";
import type { TypeDescriptor } from "@verter/type-ir";

export interface HistoireStoryConfig {
  title: string;
  variants: HistoireVariant[];
}

export interface HistoireVariant {
  title: string;
  props: Record<string, unknown>;
}

/**
 * Generate a Histoire story configuration from ComponentMeta.
 */
export function toHistoireConfig(meta: ComponentMeta): HistoireStoryConfig {
  return {
    title: meta.componentName,
    variants: [
      {
        title: "Default",
        props: generateDefaultProps(meta),
      },
    ],
  };
}

/**
 * Generate sensible default prop values from ComponentMeta.
 */
export function generateDefaultProps(meta: ComponentMeta): Record<string, unknown> {
  const defaults: Record<string, unknown> = {};

  for (const prop of meta.props) {
    const value = typeToDefaultValue(prop.type);
    if (value !== undefined) {
      defaults[prop.name] = value;
    }
  }

  return defaults;
}

function typeToDefaultValue(type: TypeDescriptor): unknown {
  switch (type.kind) {
    case "primitive":
      switch (type.name) {
        case "string":
          return "";
        case "number":
          return 0;
        case "boolean":
          return false;
        case "null":
        case "undefined":
          return null;
        default:
          return undefined;
      }

    case "literal":
      return type.value;

    case "union": {
      // Pick the first non-null/undefined member
      for (const member of type.types) {
        if (
          member.kind === "primitive" &&
          (member.name === "null" || member.name === "undefined")
        ) {
          continue;
        }
        return typeToDefaultValue(member);
      }
      return null;
    }

    case "array":
      return [];

    case "tuple":
      return type.elements.map(typeToDefaultValue);

    case "object":
      return {};

    case "ref":
      return undefined;

    default:
      return undefined;
  }
}

/**
 * Generate multiple variant configs from a ComponentMeta — one per
 * enum/union prop (if any).
 */
export function generateVariants(meta: ComponentMeta): HistoireVariant[] {
  const variants: HistoireVariant[] = [];
  const base = generateDefaultProps(meta);

  // Find props that have literal union types → generate a variant per value
  const unionProp = meta.props.find(
    (p) => p.type.kind === "union" && p.type.types.every((t) => t.kind === "literal"),
  );

  if (unionProp && unionProp.type.kind === "union") {
    for (const member of unionProp.type.types) {
      if (member.kind === "literal") {
        variants.push({
          title: `${unionProp.name}: ${member.value}`,
          props: { ...base, [unionProp.name]: member.value },
        });
      }
    }
  }

  if (variants.length === 0) {
    variants.push({ title: "Default", props: base });
  }

  return variants;
}

function camelCase(input: string): string {
  return input
    .replace(/^[^a-zA-Z0-9]+/, "")
    .replace(/[^a-zA-Z0-9]+([a-zA-Z0-9])/g, (_match, char: string) => char.toUpperCase())
    .replace(/^[A-Z]/, (char) => char.toLowerCase());
}

export function refineMetaForBenchmark(meta: any) {
  const eventProps = new Set(
    (meta?.events ?? []).map((event: any) => camelCase(`on_${event.name}`)),
  );
  // Vue built-in attrs that vue-component-meta excludes via global flag
  const vueBuiltinAttrs = new Set(["class", "style", "key", "ref"]);
  const props = (meta?.props ?? [])
    .filter(
      (prop: any) => !prop.global && !eventProps.has(prop.name) && !vueBuiltinAttrs.has(prop.name),
    )
    .sort((left: any, right: any) => {
      if (!left.required && right.required) {
        return 1;
      }
      if (left.required && !right.required) {
        return -1;
      }
      if (left.type === "boolean" && right.type !== "boolean") {
        return 1;
      }
      if (left.type !== "boolean" && right.type === "boolean") {
        return -1;
      }
      return String(left.name ?? "").localeCompare(String(right.name ?? ""));
    })
    .map((prop: any) => stripInternalSchemaNoise(prop));

  const sourceMeta = meta?._verter;

  return {
    // Use the compat-layer componentName (null for VolarComponentMeta)
    // rather than digging into _verter.componentName which is a verter
    // extension not present in vue-component-meta output.
    componentName: meta?.componentName ?? null,
    props,
    events: (meta?.events ?? []).map((event: any) => stripInternalSchemaNoise(event)),
    slots: (meta?.slots ?? []).map((slot: any) => stripInternalSchemaNoise(slot)),
    exposed: (meta?.exposed ?? []).map((member: any) => stripInternalSchemaNoise(member)),
    models: (sourceMeta?.models ?? []).map((model: any) => ({
      name: model.name,
      type: benchmarkTypeDescriptorToString(model.type),
      description: null,
      tags: [],
      schema: null,
    })),
  };
}

function benchmarkTypeDescriptorToString(type: any): string {
  if (!type || typeof type !== "object") {
    return "unknown";
  }

  switch (type.kind) {
    case "primitive":
      return String(type.name ?? "unknown");
    case "literal":
      return JSON.stringify(type.value);
    case "ref": {
      const args = Array.isArray(type.typeArguments)
        ? type.typeArguments.map((entry: any) => benchmarkTypeDescriptorToString(entry))
        : [];
      return args.length > 0 ? `${type.name}<${args.join(", ")}>` : String(type.name ?? "unknown");
    }
    case "array":
      return `${benchmarkTypeDescriptorToString(type.elementType)}[]`;
    case "tuple":
      return `[${(type.elements ?? [])
        .map((entry: any) => benchmarkTypeDescriptorToString(entry))
        .join(", ")}]`;
    case "union":
      return (type.types ?? [])
        .map((entry: any) => benchmarkTypeDescriptorToString(entry))
        .join(" | ");
    case "intersection":
      return (type.types ?? [])
        .map((entry: any) => benchmarkTypeDescriptorToString(entry))
        .join(" & ");
    default:
      return String(type.name ?? type.kind ?? "unknown");
  }
}

function stripInternalSchemaNoise(value: any): any {
  if (value == null) {
    return value;
  }
  if (Array.isArray(value)) {
    return value.map((entry) => stripInternalSchemaNoise(entry));
  }
  if (typeof value !== "object") {
    return value;
  }

  const next: Record<string, unknown> = {};
  for (const key of Object.keys(value)) {
    if (key === "declarations") {
      continue;
    }
    const entry = value[key];
    if (entry === undefined) {
      continue;
    }
    next[key] = stripInternalSchemaNoise(entry);
  }
  return next;
}

export function propsToJsonSchema(props: Array<any>): Record<string, unknown> {
  const properties: Record<string, unknown> = {};

  for (const prop of props ?? []) {
    const schema = convertPropertyMetaToJsonSchema(prop.type, prop.schema);
    if (!schema) {
      continue;
    }
    const next = {
      ...schema,
      ...(prop.description ? { description: prop.description } : {}),
    };
    const defaultValue = resolveDefaultValue(prop.default, prop.tags);
    if (defaultValue !== undefined) {
      (next as Record<string, unknown>).default = defaultValue;
    }
    properties[prop.name] = next;
  }

  return properties;
}

function convertPropertyMetaToJsonSchema(
  typeText: string | undefined,
  schema: any,
): Record<string, unknown> | null {
  const normalizedType = (typeText ?? "").replace(/\s+/g, " ").trim();

  if (schema && typeof schema === "object") {
    if (schema.kind === "enum" && Array.isArray(schema.schema)) {
      return {
        enum: schema.schema.map((entry: any) => (typeof entry === "object" ? entry.type : entry)),
        type: inferEnumType(schema.schema),
      };
    }

    if (schema.kind === "array") {
      const itemSchema =
        Array.isArray(schema.schema) && schema.schema.length > 0 ? schema.schema[0] : null;
      return {
        type: "array",
        ...(itemSchema
          ? {
              items: convertPropertyMetaToJsonSchema(itemSchema.type ?? normalizedType, itemSchema),
            }
          : {}),
      };
    }

    if (schema.kind === "object") {
      const nested = schema.schema && typeof schema.schema === "object" ? schema.schema : {};
      const properties: Record<string, unknown> = {};
      for (const key of Object.keys(nested).sort()) {
        const entry = nested[key];
        const next = convertPropertyMetaToJsonSchema(entry?.type, entry?.schema ?? entry);
        if (next) {
          properties[key] = {
            ...next,
            ...(entry?.description ? { description: entry.description } : {}),
          };
        }
      }
      return {
        type: "object",
        properties,
        additionalProperties: false,
      };
    }
  }

  if (normalizedType.includes("|")) {
    const types = normalizedType
      .split("|")
      .map((part) => part.trim())
      .filter((part) => part.length > 0 && part !== "undefined")
      .map((part) => mapPrimitiveType(part))
      .filter(Boolean);
    if (types.length === 0) {
      return null;
    }
    const uniqueTypes = [...new Set(types)];
    return uniqueTypes.length === 1 ? { type: uniqueTypes[0] } : { type: uniqueTypes };
  }

  const primitive = mapPrimitiveType(normalizedType);
  return primitive ? { type: primitive } : {};
}

function inferEnumType(entries: any[]): string {
  const normalized = entries
    .map((entry) => (typeof entry === "object" ? entry.type : entry))
    .map((entry) => String(entry));
  if (normalized.every((entry) => /^-?\d+(\.\d+)?$/.test(entry))) {
    return "number";
  }
  if (normalized.every((entry) => entry === "true" || entry === "false")) {
    return "boolean";
  }
  return "string";
}

function mapPrimitiveType(typeText: string): string | null {
  switch (typeText.toLowerCase()) {
    case "string":
      return "string";
    case "number":
      return "number";
    case "boolean":
      return "boolean";
    case "null":
      return "null";
    case "object":
      return "object";
    case "array":
      return "array";
    case "symbol":
      return "string";
    default:
      return null;
  }
}

function resolveDefaultValue(
  defaultValue: unknown,
  tags: Array<{ name: string; text?: string }> | undefined,
) {
  if (typeof defaultValue === "string") {
    return parseDefaultValue(defaultValue);
  }

  const tagValue = tags?.find((tag) => tag.name === "defaultValue")?.text;
  if (typeof tagValue === "string") {
    return parseDefaultValue(tagValue);
  }

  return undefined;
}

function parseDefaultValue(defaultValue: string): unknown {
  const trimmed = defaultValue.trim();
  if (
    (trimmed.startsWith('"') && trimmed.endsWith('"')) ||
    (trimmed.startsWith("'") && trimmed.endsWith("'"))
  ) {
    return trimmed.slice(1, -1);
  }
  if (trimmed === "true") {
    return true;
  }
  if (trimmed === "false") {
    return false;
  }
  if (/^-?\d+(\.\d+)?$/.test(trimmed)) {
    return Number(trimmed);
  }
  if (
    (trimmed.startsWith("{") && trimmed.endsWith("}")) ||
    (trimmed.startsWith("[") && trimmed.endsWith("]"))
  ) {
    try {
      return JSON.parse(trimmed);
    } catch {
      return trimmed;
    }
  }
  return trimmed;
}

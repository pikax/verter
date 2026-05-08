/**
 * TypeDescriptor → PropertyMetaSchema conversion.
 *
 * Maps Verter's Type IR to Volar's `PropertyMetaSchema` shape.
 */

import type { TypeDescriptor } from "@verter/type-ir";
import type { PropertyMetaSchema, MetaCheckerOptions } from "./types.js";

const MAX_SCHEMA_REGISTRY_RESOLUTION_DEPTH = 1;

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
  typeRegistry?: Map<string, TypeDescriptor>,
): PropertyMetaSchema {
  if (options?.schema === false) {
    return "unknown";
  }

  const schemaOptions = typeof options?.schema === "object" ? options.schema : undefined;

  return convertType(td, schemaOptions, typeRegistry, new Set());
}

function convertType(
  td: TypeDescriptor,
  options?: { ignore?: (type: string) => boolean; literalBooleanSchema?: boolean },
  typeRegistry?: Map<string, TypeDescriptor>,
  visited?: Set<string>,
  registryResolutionDepth = 0,
): PropertyMetaSchema {
  const ignore = options?.ignore;

  switch (td.kind) {
    case "primitive":
      if (td.name === "boolean" && options?.literalBooleanSchema) {
        return {
          kind: "enum",
          type: "boolean",
          schema: ["false", "true"],
        };
      }
      return td.name;

    case "literal":
      return typeof td.value === "string" ? `"${td.value}"` : String(td.value);

    case "union": {
      // Flatten nested unions so optional literal unions produce a flat array
      // e.g. ('error' | 'primary') | undefined → ['undefined', '"error"', '"primary"']
      // instead of [{ kind: 'enum', schema: [...] }, 'undefined']
      const flat = flattenUnionTypes(td.types);
      const type = flat
        .map((entry) =>
          schemaDescriptorToString(entry, typeRegistry, visited, registryResolutionDepth),
        )
        .join(" | ");
      if (ignore?.(type)) return type;
      return {
        kind: "enum",
        type,
        schema: flat.flatMap((t) =>
          flattenSchemaEnumEntries(
            convertType(t, options, typeRegistry, visited, registryResolutionDepth),
          ),
        ),
      };
    }

    case "intersection": {
      const type = td.types
        .map((entry) =>
          schemaDescriptorToString(entry, typeRegistry, visited, registryResolutionDepth),
        )
        .join(" & ");
      if (ignore?.(type)) return type;
      // Use array format for Volar compat — cast needed since TypeScript types
      // declare object schema as Record, but arrays work at runtime and match
      // how stripeTypeScriptInternalTypesSchema processes them.
      return {
        kind: "object" as const,
        type,
        schema: td.types.map((t) =>
          convertType(t, options, typeRegistry, visited, registryResolutionDepth),
        ) as unknown as Record<string, PropertyMetaSchema>,
      };
    }

    case "array": {
      const type = `${schemaDescriptorToString(
        td.element,
        typeRegistry,
        visited,
        registryResolutionDepth,
      )}[]`;
      if (ignore?.(type)) return type;
      return {
        kind: "array",
        type,
        schema: [convertType(td.element, options, typeRegistry, visited, registryResolutionDepth)],
      };
    }

    case "tuple": {
      const type = `[${td.elements
        .map((entry) =>
          schemaDescriptorToString(entry, typeRegistry, visited, registryResolutionDepth),
        )
        .join(", ")}]`;
      if (ignore?.(type)) return type;
      return {
        kind: "array",
        type,
        schema: td.elements.map((t) =>
          convertType(t, options, typeRegistry, visited, registryResolutionDepth),
        ),
      };
    }

    case "object": {
      const type = schemaObjectDescriptorToString(
        td,
        true,
        typeRegistry,
        visited,
        registryResolutionDepth,
      );
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
          type: schemaDescriptorToString(p.type, typeRegistry, visited, registryResolutionDepth),
          schema: convertType(p.type, options, typeRegistry, visited, registryResolutionDepth),
        } as unknown as PropertyMetaSchema;
      });
      return {
        kind: "object",
        type,
        schema,
      };
    }

    case "function": {
      const typeParams = td.typeParameters?.length
        ? `<${td.typeParameters.map(typeParameterToString).join(", ")}>`
        : "";
      const params = td.parameters
        .map((p) => `${p.name}${p.optional ? "?" : ""}: ${typeDescriptorToString(p.type)}`)
        .join(", ");
      return `${typeParams}(${params}) => ${typeDescriptorToString(td.returnType)}`;
    }

    case "typeParameter":
      return td.name;

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
      // Try to resolve from type registry (pre-parsed TypeDescriptor)
      if (
        typeRegistry &&
        registryResolutionDepth < MAX_SCHEMA_REGISTRY_RESOLUTION_DEPTH &&
        !visited?.has(td.name)
      ) {
        const resolved = typeRegistry.get(td.name);
        if (resolved) {
          visited?.add(td.name);
          const result = convertType(
            resolved,
            options,
            typeRegistry,
            visited,
            registryResolutionDepth + 1,
          );
          visited?.delete(td.name);
          return result;
        }
      }
      // Unresolved ref → structured object with empty schema (matches Volar for browser/external types)
      return { kind: "object" as const, type: name, schema: {} };
    }

    case "recursiveRef":
      return { kind: "object" as const, type: typeDescriptorToString(td), schema: {} };

    case "unknown":
      return td.rawType || "unknown";
  }
}

export function flattenSchemaEnumEntries(schema: PropertyMetaSchema): PropertyMetaSchema[] {
  if (
    typeof schema === "object" &&
    !Array.isArray(schema) &&
    schema !== null &&
    schema.kind === "enum" &&
    Array.isArray(schema.schema) &&
    schema.schema.every((entry) => typeof entry === "string")
  ) {
    return [...schema.schema];
  }

  return [schema];
}

function schemaDescriptorToString(
  td: TypeDescriptor,
  typeRegistry?: Map<string, TypeDescriptor>,
  visited: Set<string> = new Set(),
  registryResolutionDepth = 0,
): string {
  switch (td.kind) {
    case "primitive":
    case "literal":
    case "enum":
    case "unknown":
      return typeDescriptorToString(td);
    case "union":
      return flattenUnionTypes(td.types)
        .map((entry) =>
          schemaDescriptorToString(entry, typeRegistry, visited, registryResolutionDepth),
        )
        .join(" | ");
    case "intersection":
      return td.types
        .map((entry) =>
          schemaDescriptorToString(entry, typeRegistry, visited, registryResolutionDepth),
        )
        .join(" & ");
    case "array":
      return `${schemaDescriptorToString(
        td.element,
        typeRegistry,
        visited,
        registryResolutionDepth,
      )}[]`;
    case "tuple":
      return `[${td.elements
        .map((entry) =>
          schemaDescriptorToString(entry, typeRegistry, visited, registryResolutionDepth),
        )
        .join(", ")}]`;
    case "object":
      if (
        (td.indexSignatures?.length ?? 0) === 0 &&
        (td.callSignatures?.length ?? 0) === 0 &&
        (td.constructSignatures?.length ?? 0) === 0 &&
        td.properties.length === 0
      ) {
        return "{}";
      }
      return schemaObjectDescriptorToString(
        td,
        false,
        typeRegistry,
        visited,
        registryResolutionDepth,
      );
    case "function":
      return typeDescriptorToString(td);
    case "typeParameter":
      return td.name;
    case "ref": {
      if (
        typeRegistry &&
        registryResolutionDepth < MAX_SCHEMA_REGISTRY_RESOLUTION_DEPTH &&
        !td.typeArguments?.length &&
        !visited.has(td.name)
      ) {
        const resolved = typeRegistry.get(td.name);
        if (resolved) {
          visited.add(td.name);
          const result = schemaDescriptorToString(
            resolved,
            typeRegistry,
            visited,
            registryResolutionDepth + 1,
          );
          visited.delete(td.name);
          return result;
        }
      }
      return typeDescriptorToString(td);
    }
    case "recursiveRef":
      return typeDescriptorToString(td);
  }
}

function schemaDescriptorToSafeString(
  td: TypeDescriptor,
  typeRegistry?: Map<string, TypeDescriptor>,
  visited: Set<string> = new Set(),
  registryResolutionDepth = 0,
): string {
  switch (td.kind) {
    case "literal":
      return typeof td.value === "string" ? td.value : String(td.value);
    case "union":
      return flattenUnionTypes(td.types)
        .map((entry) =>
          schemaDescriptorToSafeString(entry, typeRegistry, visited, registryResolutionDepth),
        )
        .join(" | ");
    case "intersection":
      return td.types
        .map((entry) =>
          schemaDescriptorToSafeString(entry, typeRegistry, visited, registryResolutionDepth),
        )
        .join(" & ");
    default:
      return schemaDescriptorToString(td, typeRegistry, visited, registryResolutionDepth);
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
      return flattenUnionTypes(td.types).map(typeDescriptorToString).join(" | ");
    case "intersection":
      return td.types.map(typeDescriptorToString).join(" & ");
    case "array":
      return `${typeDescriptorToString(td.element)}[]`;
    case "tuple":
      return `[${td.elements.map(typeDescriptorToString).join(", ")}]`;
    case "object":
      if (
        (td.indexSignatures?.length ?? 0) === 0 &&
        (td.callSignatures?.length ?? 0) === 0 &&
        (td.constructSignatures?.length ?? 0) === 0
      ) {
        return "object";
      }
      return objectDescriptorToString(td, false);
    case "function":
      return "function";
    case "typeParameter":
      return td.name;
    case "ref":
      return td.typeArguments
        ? `${td.name}<${td.typeArguments.map(typeDescriptorToString).join(", ")}>`
        : td.name;
    case "recursiveRef":
      return td.typeArguments.length > 0
        ? `${td.name}<${td.typeArguments.map(typeDescriptorToString).join(", ")}>`
        : td.name;
    case "enum":
      return td.name;
    case "unknown":
      return td.rawType || "unknown";
  }
}

/**
 * Like typeDescriptorToString but renders literal values without quotes.
 * Used in object type strings to avoid triggering false enum detection
 * in downstream consumers that check for `"` and `|` in type strings.
 */
function typeDescriptorToSafeString(td: TypeDescriptor): string {
  switch (td.kind) {
    case "literal":
      return typeof td.value === "string" ? td.value : String(td.value);
    case "union":
      return flattenUnionTypes(td.types).map(typeDescriptorToSafeString).join(" | ");
    case "intersection":
      return td.types.map(typeDescriptorToSafeString).join(" & ");
    default:
      return typeDescriptorToString(td);
  }
}

function objectDescriptorToString(
  td: Extract<TypeDescriptor, { kind: "object" }>,
  safeValues: boolean,
): string {
  const members: string[] = [];
  let needsTrailingSemicolon = false;

  for (const prop of td.properties) {
    const renderValue = safeValues
      ? typeDescriptorToSafeString(prop.type)
      : typeDescriptorToString(prop.type);
    members.push(`${prop.name}${prop.optional ? "?" : ""}: ${renderValue}`);
  }

  for (const indexSignature of td.indexSignatures ?? []) {
    const renderValue = safeValues
      ? typeDescriptorToSafeString(indexSignature.valueType)
      : typeDescriptorToString(indexSignature.valueType);
    members.push(
      `${indexSignature.readonly ? "readonly " : ""}[${indexSignature.keyName}: ${typeDescriptorToString(indexSignature.keyType)}]: ${renderValue}`,
    );
    needsTrailingSemicolon = true;
  }

  for (const signature of td.callSignatures ?? []) {
    members.push(functionSignatureToString(signature, "call"));
    needsTrailingSemicolon = true;
  }

  for (const signature of td.constructSignatures ?? []) {
    members.push(functionSignatureToString(signature, "construct"));
    needsTrailingSemicolon = true;
  }

  if (members.length === 0) {
    return "{}";
  }

  return `{ ${members.join("; ")}${needsTrailingSemicolon ? ";" : ""} }`;
}

function schemaObjectDescriptorToString(
  td: Extract<TypeDescriptor, { kind: "object" }>,
  safeValues: boolean,
  typeRegistry?: Map<string, TypeDescriptor>,
  visited: Set<string> = new Set(),
  registryResolutionDepth = 0,
): string {
  const members: string[] = [];
  let needsTrailingSemicolon = false;

  for (const prop of td.properties) {
    const renderValue = safeValues
      ? schemaDescriptorToSafeString(prop.type, typeRegistry, visited, registryResolutionDepth)
      : schemaDescriptorToString(prop.type, typeRegistry, visited, registryResolutionDepth);
    members.push(`${prop.name}${prop.optional ? "?" : ""}: ${renderValue}`);
  }

  for (const indexSignature of td.indexSignatures ?? []) {
    const renderValue = safeValues
      ? schemaDescriptorToSafeString(
          indexSignature.valueType,
          typeRegistry,
          visited,
          registryResolutionDepth,
        )
      : schemaDescriptorToString(
          indexSignature.valueType,
          typeRegistry,
          visited,
          registryResolutionDepth,
        );
    members.push(
      `${indexSignature.readonly ? "readonly " : ""}[${indexSignature.keyName}: ${schemaDescriptorToString(indexSignature.keyType, typeRegistry, visited, registryResolutionDepth)}]: ${renderValue}`,
    );
    needsTrailingSemicolon = true;
  }

  for (const signature of td.callSignatures ?? []) {
    members.push(functionSignatureToString(signature, "call"));
    needsTrailingSemicolon = true;
  }

  for (const signature of td.constructSignatures ?? []) {
    members.push(functionSignatureToString(signature, "construct"));
    needsTrailingSemicolon = true;
  }

  if (members.length === 0) {
    return "{}";
  }

  return `{ ${members.join("; ")}${needsTrailingSemicolon ? ";" : ""} }`;
}

function functionSignatureToString(
  td: Extract<TypeDescriptor, { kind: "function" }>,
  mode: "call" | "construct",
): string {
  const typeParams = td.typeParameters?.length
    ? `<${td.typeParameters.map(typeParameterToString).join(", ")}>`
    : "";
  const params = td.parameters
    .map((p) => `${p.name}${p.optional ? "?" : ""}: ${typeDescriptorToString(p.type)}`)
    .join(", ");
  const prefix = mode === "construct" ? "new " : "";
  return `${prefix}${typeParams}(${params}): ${typeDescriptorToString(td.returnType)}`;
}

function typeParameterToString(td: Extract<TypeDescriptor, { kind: "typeParameter" }>): string {
  let rendered = td.name;
  if (td.constraint) {
    rendered += ` extends ${typeDescriptorToString(td.constraint)}`;
  }
  if (td.default) {
    rendered += ` = ${typeDescriptorToString(td.default)}`;
  }
  return rendered;
}

/**
 * Flatten nested unions into a single flat array of types.
 * `(A | B) | C` → `[A, B, C]`
 */
function flattenUnionTypes(types: TypeDescriptor[]): TypeDescriptor[] {
  const result: TypeDescriptor[] = [];
  for (const t of types) {
    if (t.kind === "union") {
      result.push(...flattenUnionTypes(t.types));
    } else {
      result.push(t);
    }
  }
  return result;
}

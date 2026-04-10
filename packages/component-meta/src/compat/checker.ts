/**
 * Volar-compatible ComponentMetaChecker — drop-in replacement for vue-component-meta.
 *
 * Usage:
 * ```ts
 * import { createChecker } from '@verter/component-meta/compat'
 * const checker = await createChecker('./tsconfig.json')
 * const meta = await checker.getComponentMeta('./src/MyButton.vue')
 * ```
 */

import { existsSync, mkdirSync } from "node:fs";
import { dirname } from "node:path";
import { createRequire } from "node:module";
import {
  nativeComponentMetaToComponentMeta,
  nativeTypeRegistryToMap,
} from "../native-component-meta.js";
import type { TypeDescriptor } from "../type-ir.js";
import type { VerterHostAdapter } from "../host-adapter.js";
import type {
  ComponentMeta,
  PropMeta,
  EventMeta,
  SlotMeta,
  ExposedMeta,
  PublicInstanceMemberMeta,
} from "../types.js";
import type {
  PropertyMeta,
  PropertyMetaSchema,
  VolarComponentMeta,
  MetaCheckerOptions,
} from "./types.js";
import { typeDescriptorToSchema, typeDescriptorToString } from "./schema.js";
import {
  createMetaRuntime,
  getMetaRuntime,
  stableSelectiveConfigHash,
  resolvePath as runtimeResolvePath,
  normalizePath as runtimeNormalizePath,
  parseTsconfig,
  extractPathAliases,
} from "../runtime/index.js";
import type {
  BootstrapFn,
  EngineKeyInput,
  NativeMetaProject,
  MetaRuntimeImpl,
  ProjectSession,
} from "../runtime/index.js";

const COMPAT_BLOCKED_SLOT_NAMES = new Set([
  "type",
  "props",
  "key",
  "ref",
  "scopeId",
  "children",
  "component",
  "dirs",
  "transition",
  "el",
  "placeholder",
  "anchor",
  "target",
  "targetStart",
  "targetAnchor",
  "suspense",
  "shapeFlag",
  "patchFlag",
  "appContext",
]);

/** Maximum descriptor text length before the compat layer considers it
 *  over-expanded and falls back to the raw type string. */
const COMPAT_MAX_RESOLVED_PROP_DISPLAY_LENGTH = 512;
/** Maximum depth for recursive registry ref resolution in compat display.
 *  Kept at 1 to preserve the shallow-resolution invariant. */
const COMPAT_MAX_REGISTRY_DISPLAY_DEPTH = 1;

function isCompatVisibleSlotName(name: string): boolean {
  return !COMPAT_BLOCKED_SLOT_NAMES.has(name);
}

/**
 * Minimal workspace interface used by the checker.
 * Matches the Workspace class from @verter/native.
 */
export interface CheckerWorkspace {
  readFile(path: string): Promise<string | null>;
  fileExists(path: string): Promise<boolean>;
  isDir(path: string): Promise<boolean>;
  readDir(dir: string): Promise<Array<{ path: string; isDir: boolean }>>;
  walk(root: string, excludeDirs: string[], extensions?: string[]): Promise<string[]>;
  configureProjects(
    projects: Array<{
      root: string;
      workspaceRoot: string;
      compilerOptions?: {
        baseUrl?: string;
        paths?: Array<{ pattern: string; targets: string[] }>;
      };
    }>,
  ): void;
}

/**
 * Create a workspace from @verter/native for the given root directory.
 */
function loadNative(): any {
  const _require = typeof require === "function" ? require : createRequire(import.meta.url);
  return _require("@verter/native");
}

function createWorkspace(rootDir: string): CheckerWorkspace {
  if (!existsSync(rootDir)) {
    mkdirSync(rootDir, { recursive: true });
  }
  const native = loadNative();
  return new native.Workspace([runtimeNormalizePath(rootDir)]);
}

/**
 * Read a file using workspace. Workspace is required.
 */
async function readFileSafe(absPath: string, ws: CheckerWorkspace): Promise<string | null> {
  return (await ws.readFile(runtimeNormalizePath(absPath))) ?? null;
}

/**
 * Map a Verter PropMeta to Volar PropertyMeta.
 */
export function mapPropMeta(
  prop: PropMeta,
  options?: MetaCheckerOptions,
  typeRegistry?: Map<string, TypeDescriptor>,
): PropertyMeta {
  const type = preferredCompatPropTypeText(prop, typeRegistry);
  const schema = normalizeOptionalPropSchema(
    applyRawTypeDisplayHintsToSchema(
      typeDescriptorToSchema(prop.type, options, typeRegistry),
      prop.rawType,
    ),
    type,
    prop.required,
  );
  return {
    name: prop.name,
    description: prop.description ?? "",
    type,
    required: prop.required,
    global: false,
    default: normalizeDefaultForCompat(type, evaluateDefault(prop.default)),
    tags: (prop.tags ?? []).map((t) => ({
      name: t.name,
      ...(t.text != null && { text: t.text }),
    })),
    schema,
  };
}

/**
 * Normalize Array<T> syntax to T[] for consistency with Volar output.
 */
function normalizeTypeString(type: string): string {
  const normalizedIndexedAccess = type.replace(
    /\['([^'\\]*(?:\\.[^'\\]*)*)'\]/g,
    (_match, value: string) => `[${JSON.stringify(value)}]`,
  );
  const match = normalizedIndexedAccess.match(/^Array<(.+)>$/);
  const normalized = match ? `${match[1]}[]` : normalizedIndexedAccess;
  return normalized.replace(/'([^'\\]*(?:\\.[^'\\]*)*)'/g, (_match, value: string) =>
    JSON.stringify(value),
  );
}

function preferredCompatPropTypeText(
  prop: PropMeta,
  typeRegistry?: Map<string, TypeDescriptor>,
): string {
  const descriptorText = normalizeOptionalCompatTypeText(
    normalizeTypeString(typeDescriptorToCompatDisplay(prop.type, typeRegistry)),
    prop.required,
  );
  const rawType = prop.rawType
    ? normalizeOptionalCompatTypeText(normalizeTypeString(prop.rawType), prop.required)
    : undefined;

  if (!rawType || compatRawTypeLooksLossy(rawType)) {
    return descriptorText;
  }

  if (shouldPreferDescriptorForProp(rawType, descriptorText)) {
    return descriptorText;
  }

  return rawType;
}

function preferredCompatTypeText(
  rawType: string | undefined,
  descriptor: TypeDescriptor,
  typeRegistry?: Map<string, TypeDescriptor>,
): string {
  const descriptorText = normalizeTypeString(
    typeDescriptorToCompatDisplay(descriptor, typeRegistry),
  );
  if (!rawType || compatRawTypeLooksLossy(rawType)) {
    return descriptorText;
  }

  const normalizedRawType = normalizeTypeString(rawType);
  if (shouldPreferDescriptorForProp(normalizedRawType, descriptorText)) {
    return descriptorText;
  }

  return normalizedRawType;
}

/** Heuristic: does the raw type string look like it lost structural detail
 *  (e.g. truncated code blocks, ellipsis, bare `object`)? */
function compatRawTypeLooksLossy(rawType: string): boolean {
  const normalized = rawType.trim();
  return (
    normalized.startsWith("```") ||
    normalized.includes("...") ||
    normalized.includes("/*") ||
    normalized.includes("*/") ||
    normalized === "object"
  );
}

function normalizeOptionalPropSchema(
  schema: PropertyMetaSchema,
  type: string,
  required: boolean,
): PropertyMetaSchema {
  if (required || compatSchemaIncludesTopLevelUndefined(schema)) {
    return schema;
  }

  if (typeof schema === "string") {
    return {
      kind: "enum",
      type,
      schema: [schema, "undefined"],
    };
  }

  if (Array.isArray(schema)) {
    return {
      kind: "enum",
      type,
      schema: [...schema, "undefined"],
    };
  }

  if (schema.kind === "enum") {
    return {
      ...schema,
      type,
      schema: [...(schema.schema ?? []), "undefined"],
    };
  }

  return {
    kind: "enum",
    type,
    schema: [schema, "undefined"],
  };
}

function compatSchemaIncludesTopLevelUndefined(schema: PropertyMetaSchema): boolean {
  if (typeof schema === "string") {
    return schema.trim() === "undefined";
  }
  if (Array.isArray(schema)) {
    return schema.some((entry) => compatSchemaIncludesTopLevelUndefined(entry));
  }
  if (schema.kind === "enum" || schema.kind === "array" || schema.kind === "event") {
    return (schema.schema ?? []).some((entry) => compatSchemaIncludesTopLevelUndefined(entry));
  }
  return false;
}

function normalizeOptionalCompatTypeText(type: string, required: boolean): string {
  if (required) return type;
  const stripped = stripTopLevelUndefinedFromTypeString(type).trim();
  if (stripped === "any") {
    return "any";
  }
  const parts = splitTopLevelTypeUnion(type);
  if (parts.some((part) => part.replace(/\s+/g, "") === "undefined")) {
    return type;
  }
  return `${type} | undefined`;
}

function stripTopLevelUndefinedFromTypeString(type: string): string {
  const parts = splitTopLevelTypeUnion(type);
  const kept = parts.filter((part) => part.replace(/\s+/g, "") !== "undefined");
  if (kept.length === parts.length || kept.length === 0) {
    return type;
  }
  return kept.join(" | ");
}

function splitTopLevelTypeUnion(type: string): string[] {
  return splitTopLevelTypeOperator(type, "|");
}

function splitTopLevelTypeIntersection(type: string): string[] {
  return splitTopLevelTypeOperator(type, "&");
}

function splitTopLevelTypeOperator(type: string, operator: "|" | "&"): string[] {
  const parts: string[] = [];
  let start = 0;
  let parenDepth = 0;
  let bracketDepth = 0;
  let braceDepth = 0;
  let angleDepth = 0;

  for (let index = 0; index < type.length; index++) {
    const ch = type[index];
    const prev = index > 0 ? type[index - 1] : "";
    switch (ch) {
      case "(":
        parenDepth++;
        break;
      case ")":
        parenDepth--;
        break;
      case "[":
        bracketDepth++;
        break;
      case "]":
        bracketDepth--;
        break;
      case "{":
        braceDepth++;
        break;
      case "}":
        braceDepth--;
        break;
      case "<":
        angleDepth++;
        break;
      case ">":
        // `=>` is an arrow function token, not a generic-depth close.
        if (prev !== "=") {
          angleDepth--;
        }
        break;
      case "|":
      case "&":
        if (parenDepth === 0 && bracketDepth === 0 && braceDepth === 0 && angleDepth === 0) {
          if (ch === operator) {
            parts.push(type.slice(start, index).trim());
            start = index + 1;
          }
        }
        break;
    }
  }

  parts.push(type.slice(start).trim());
  return parts.filter(Boolean);
}

function stripSingleOuterParens(type: string): string {
  const trimmed = type.trim();
  if (!trimmed.startsWith("(") || !trimmed.endsWith(")")) {
    return trimmed;
  }

  let depth = 0;
  for (let index = 0; index < trimmed.length; index++) {
    const ch = trimmed[index];
    if (ch === "(") depth++;
    if (ch === ")") depth--;
    if (depth === 0 && index < trimmed.length - 1) {
      return trimmed;
    }
  }

  return trimmed.slice(1, -1).trim();
}

function shouldPreferRawSchemaType(rawType: string, currentType: string | undefined): boolean {
  const normalizedRaw = normalizeTypeString(stripSingleOuterParens(rawType));
  const normalizedCurrent = currentType ? normalizeTypeString(currentType) : "";
  if (!normalizedRaw || normalizedRaw === normalizedCurrent) {
    return false;
  }
  if (normalizedCurrent && shouldPreferDescriptorForProp(normalizedRaw, normalizedCurrent)) {
    return false;
  }
  return (
    normalizedRaw.includes("<") ||
    looksLikeIndexedAccessType(normalizedRaw) ||
    looksLikeBareTypeReference(stripTopLevelUndefinedFromTypeString(normalizedRaw))
  );
}

function applyRawTypeDisplayHintsToSchema(
  schema: PropertyMetaSchema,
  rawType: string | undefined,
): PropertyMetaSchema {
  if (!rawType) {
    return schema;
  }
  return applyRawTypeDisplayHintsToSchemaInner(schema, normalizeTypeString(rawType));
}

function applyRawTypeDisplayHintsToSchemaInner(
  schema: PropertyMetaSchema,
  rawType: string,
): PropertyMetaSchema {
  if (typeof schema === "string" || Array.isArray(schema)) {
    return schema;
  }

  const raw = stripSingleOuterParens(rawType);

  if (schema.kind === "enum" && Array.isArray(schema.schema)) {
    const unionParts = splitTopLevelTypeUnion(raw);
    if (unionParts.length === schema.schema.length) {
      return {
        ...schema,
        ...(shouldPreferRawSchemaType(raw, schema.type) ? { type: normalizeTypeString(raw) } : {}),
        schema: schema.schema.map((entry, index) =>
          applyRawTypeDisplayHintsToSchemaInner(entry, unionParts[index] ?? raw),
        ),
      };
    }
  }

  if (schema.kind === "object" && Array.isArray(schema.schema)) {
    const intersectionParts = splitTopLevelTypeIntersection(raw);
    if (intersectionParts.length === schema.schema.length) {
      return {
        ...schema,
        ...(shouldPreferRawSchemaType(raw, schema.type) ? { type: normalizeTypeString(raw) } : {}),
        schema: schema.schema.map((entry, index) =>
          applyRawTypeDisplayHintsToSchemaInner(entry, intersectionParts[index] ?? raw),
        ),
      } as unknown as PropertyMetaSchema;
    }
  }

  if ("type" in schema && shouldPreferRawSchemaType(raw, schema.type)) {
    return {
      ...schema,
      type: normalizeTypeString(raw),
    };
  }

  return schema;
}

function shouldPreferDescriptorForProp(rawType: string, descriptorText: string): boolean {
  const normalizedRawType = stripTopLevelUndefinedFromTypeString(rawType);
  return (
    rawType !== descriptorText &&
    !compatDescriptorLooksLossy(descriptorText) &&
    !compatDescriptorLooksOverexpanded(descriptorText) &&
    (looksLikeBareTypeReference(normalizedRawType) || looksLikeIndexedAccessType(normalizedRawType))
  );
}

/** Heuristic: does the descriptor text contain solver artifacts (`@rec(`, bare
 *  kind keywords, `graphNode()` placeholders) or degenerate unions containing
 *  `any`, indicating the resolved form is less informative than the raw type? */
function compatDescriptorLooksLossy(descriptorText: string): boolean {
  const normalized = stripTopLevelUndefinedFromTypeString(descriptorText).trim();
  return (
    compatRawTypeLooksLossy(normalized) ||
    normalized.includes("@rec(") ||
    splitTopLevelTypeUnion(normalized).some((part) => part.trim() === "any") ||
    /^(indexedAccess|unknown|object|function|intersection|union|conditional)$/.test(normalized) ||
    /^graphNode\(\d+\)$/.test(normalized)
  );
}

/** Heuristic: does the descriptor text look like the solver over-expanded it
 *  (too long, or excessive identifier repetition indicating recursive inlining)? */
function compatDescriptorLooksOverexpanded(descriptorText: string): boolean {
  if (descriptorText.length > COMPAT_MAX_RESOLVED_PROP_DISPLAY_LENGTH) {
    return true;
  }

  if (!descriptorText.includes("{")) {
    return false;
  }

  const identifiers = descriptorText.match(/\b[A-Za-z_$][A-Za-z0-9_$]*\b/g) ?? [];
  const counts = new Map<string, number>();
  let maxRepeats = 0;
  for (const identifier of identifiers) {
    const next = (counts.get(identifier) ?? 0) + 1;
    counts.set(identifier, next);
    maxRepeats = Math.max(maxRepeats, next);
  }

  return maxRepeats >= 6;
}

function looksLikeBareTypeReference(type: string): boolean {
  return /^[A-Za-z_$][A-Za-z0-9_$]*(\.[A-Za-z_$][A-Za-z0-9_$]*)*$/.test(type);
}

function looksLikeIndexedAccessType(type: string): boolean {
  return /^[A-Za-z_$][A-Za-z0-9_$.<>, ]*(\[[^\]]+\])+$/.test(type.trim());
}

function normalizeDefaultForCompat(type: string, value: string | undefined): string | undefined {
  if (value === undefined) return undefined;
  const trimmed = value.trim();
  if (
    trimmed === "" ||
    trimmed === "null" ||
    trimmed === "undefined" ||
    trimmed === "true" ||
    trimmed === "false" ||
    /^-?\d+(\.\d+)?$/.test(trimmed) ||
    trimmed.startsWith('"') ||
    trimmed.startsWith("'") ||
    trimmed.startsWith("{") ||
    trimmed.startsWith("[") ||
    trimmed.startsWith("(")
  ) {
    return value;
  }
  if (looksLikeStringCompatibleType(type)) {
    return JSON.stringify(trimmed);
  }
  return value;
}

function looksLikeStringCompatibleType(type: string): boolean {
  return (
    type === "any" ||
    type.includes("string") ||
    type.includes('"') ||
    type.includes("(string & {})")
  );
}

function typeDescriptorToCompatDisplay(
  descriptor: TypeDescriptor,
  typeRegistry?: Map<string, TypeDescriptor>,
  visited: Set<string> = new Set(),
  registryResolutionDepth = 0,
): string {
  switch (descriptor.kind) {
    case "primitive":
    case "literal":
    case "enum":
    case "unknown":
      return typeDescriptorToString(descriptor);
    case "union":
      return descriptor.types
        .map((type) =>
          typeDescriptorToCompatDisplay(type, typeRegistry, visited, registryResolutionDepth),
        )
        .join(" | ");
    case "intersection":
      return descriptor.types
        .map((type) =>
          typeDescriptorToCompatDisplay(type, typeRegistry, visited, registryResolutionDepth),
        )
        .join(" & ");
    case "array":
      return `${typeDescriptorToCompatDisplay(
        descriptor.element,
        typeRegistry,
        visited,
        registryResolutionDepth,
      )}[]`;
    case "tuple":
      return `[${descriptor.elements
        .map((type) =>
          typeDescriptorToCompatDisplay(type, typeRegistry, visited, registryResolutionDepth),
        )
        .join(", ")}]`;
    case "function":
      return compatFunctionTypeToString(descriptor, typeRegistry, visited, registryResolutionDepth);
    case "object":
      return compatObjectTypeToString(descriptor, typeRegistry, visited, registryResolutionDepth);
    case "typeParameter":
      return descriptor.name;
    case "ref": {
      if (
        typeRegistry &&
        registryResolutionDepth < COMPAT_MAX_REGISTRY_DISPLAY_DEPTH &&
        !descriptor.typeArguments?.length &&
        !visited.has(descriptor.name)
      ) {
        const resolved = typeRegistry.get(descriptor.name);
        if (resolved) {
          visited.add(descriptor.name);
          const rendered = typeDescriptorToCompatDisplay(
            resolved,
            typeRegistry,
            visited,
            registryResolutionDepth + 1,
          );
          visited.delete(descriptor.name);
          return rendered;
        }
      }
      return typeDescriptorToString(descriptor);
    }
    case "recursiveRef":
      return typeDescriptorToString(descriptor);
  }
}

function compatObjectTypeToString(
  descriptor: Extract<TypeDescriptor, { kind: "object" }>,
  typeRegistry?: Map<string, TypeDescriptor>,
  visited: Set<string> = new Set(),
  registryResolutionDepth = 0,
): string {
  const members: string[] = [];

  for (const prop of descriptor.properties) {
    members.push(
      `${prop.name}${prop.optional ? "?" : ""}: ${typeDescriptorToCompatDisplay(prop.type, typeRegistry, visited, registryResolutionDepth)}`,
    );
  }

  for (const indexSignature of descriptor.indexSignatures ?? []) {
    members.push(
      `${indexSignature.readonly ? "readonly " : ""}[${indexSignature.keyName}: ${typeDescriptorToCompatDisplay(indexSignature.keyType, typeRegistry, visited, registryResolutionDepth)}]: ${typeDescriptorToCompatDisplay(indexSignature.valueType, typeRegistry, visited, registryResolutionDepth)}`,
    );
  }

  for (const signature of descriptor.callSignatures ?? []) {
    members.push(
      compatFunctionTypeToString(signature, typeRegistry, visited, registryResolutionDepth),
    );
  }

  for (const signature of descriptor.constructSignatures ?? []) {
    members.push(
      `new ${compatFunctionTypeToString(signature, typeRegistry, visited, registryResolutionDepth)}`,
    );
  }

  if (members.length === 0) {
    return "object";
  }

  return `{ ${members.join("; ")}; }`;
}

function compatFunctionTypeToString(
  descriptor: Extract<TypeDescriptor, { kind: "function" }>,
  typeRegistry?: Map<string, TypeDescriptor>,
  visited: Set<string> = new Set(),
  registryResolutionDepth = 0,
): string {
  const typeParams = descriptor.typeParameters?.length
    ? `<${descriptor.typeParameters
        .map((param) =>
          compatTypeParameterToString(param, typeRegistry, visited, registryResolutionDepth),
        )
        .join(", ")}>`
    : "";
  const params = descriptor.parameters
    .map(
      (param) =>
        `${param.name}${param.optional ? "?" : ""}: ${typeDescriptorToCompatDisplay(param.type, typeRegistry, visited, registryResolutionDepth)}`,
    )
    .join(", ");
  return `${typeParams}(${params}): ${typeDescriptorToCompatDisplay(descriptor.returnType, typeRegistry, visited, registryResolutionDepth)}`;
}

function compatTypeParameterToString(
  descriptor: Extract<TypeDescriptor, { kind: "typeParameter" }>,
  typeRegistry?: Map<string, TypeDescriptor>,
  visited: Set<string> = new Set(),
  registryResolutionDepth = 0,
): string {
  let rendered = descriptor.name;
  if (descriptor.constraint) {
    rendered += ` extends ${typeDescriptorToCompatDisplay(
      descriptor.constraint,
      typeRegistry,
      visited,
      registryResolutionDepth,
    )}`;
  }
  if (descriptor.default) {
    rendered += ` = ${typeDescriptorToCompatDisplay(
      descriptor.default,
      typeRegistry,
      visited,
      registryResolutionDepth,
    )}`;
  }
  return rendered;
}

/**
 * Evaluate common default value patterns to match Volar's behavior.
 * Volar evaluates simple defaults; verter stores the raw source text.
 */
function evaluateDefault(val: string | undefined): string | undefined {
  if (val === undefined) return undefined;
  // Arrow function returning empty object: () => ({})
  if (/^\(\)\s*=>\s*\(\s*\{\s*\}\s*\)$/.test(val)) return "{}";
  // Arrow function returning empty array: () => []
  if (/^\(\)\s*=>\s*\[\s*\]$/.test(val)) return "[]";
  // Arrow function returning array literal: () => ['a', 'b']
  const arrowArrMatch = val.match(/^\(\)\s*=>\s*(\[.*\])$/);
  if (arrowArrMatch) return arrowArrMatch[1];
  const stringLiteralMatch = val.match(/^'([^'\\]*(?:\\.[^'\\]*)*)'$/);
  if (stringLiteralMatch) {
    return JSON.stringify(stringLiteralMatch[1]);
  }
  return val;
}

function buildSlotBindingsType(slot: SlotMeta, typeRegistry?: Map<string, TypeDescriptor>): string {
  if (slot.bindings.length === 0) {
    return slot.isRequired === false ? "{} | undefined" : "{}";
  }

  return `{ ${slot.bindings
    .map(
      (binding) =>
        `${binding.name}: ${preferredCompatTypeText(binding.rawType, binding.type, typeRegistry)}`,
    )
    .join("; ")}; }`;
}

function buildSlotBindingsDescriptor(slot: SlotMeta): TypeDescriptor {
  return {
    kind: "object",
    properties: slot.bindings.map((binding) => ({
      name: binding.name,
      type: binding.type,
      optional: false,
    })),
  };
}

function wrapOptionalEmptySlotSchema(type: string, schema: PropertyMetaSchema): PropertyMetaSchema {
  return {
    kind: "enum",
    type,
    schema: [schema, "undefined"],
  };
}

function isVoidLikeEventPayload(payload: TypeDescriptor): boolean {
  return (
    payload.kind === "unknown" && /^(void|undefined|never)$/.test((payload.rawType ?? "").trim())
  );
}

function buildEventPayloadSchema(
  payload: TypeDescriptor,
  options?: MetaCheckerOptions,
  typeRegistry?: Map<string, TypeDescriptor>,
): PropertyMetaSchema[] {
  if (payload.kind === "tuple") {
    return payload.elements.map((element) =>
      typeDescriptorToSchema(element, options, typeRegistry),
    );
  }
  if (isVoidLikeEventPayload(payload)) {
    return [];
  }
  return [typeDescriptorToSchema(payload, options, typeRegistry)];
}

function buildEventPayloadType(
  event: EventMeta,
  typeRegistry?: Map<string, TypeDescriptor>,
): string {
  const fromSignature = extractEventTupleType(event.rawSignature);
  if (fromSignature) {
    return normalizeTypeString(fromSignature);
  }
  if (event.payload.kind === "tuple") {
    return normalizeTypeString(typeDescriptorToCompatDisplay(event.payload, typeRegistry));
  }
  if (isVoidLikeEventPayload(event.payload)) {
    return "[]";
  }
  return `[${normalizeTypeString(typeDescriptorToCompatDisplay(event.payload, typeRegistry))}]`;
}

function extractEventTupleType(rawSignature: string | undefined): string | undefined {
  if (!rawSignature) {
    return undefined;
  }
  const trimmed = rawSignature.trim();
  if (trimmed.startsWith("[") && trimmed.endsWith("]")) {
    return trimmed;
  }
  const paramsSource = extractFunctionParameterSource(rawSignature);
  if (paramsSource === undefined) {
    return undefined;
  }
  const params = splitTopLevelCommaList(paramsSource);
  if (params.length === 0) {
    return "[]";
  }
  const payloadParams =
    looksLikeEventNameParameter(params[0] ?? "") && params.length > 0 ? params.slice(1) : params;
  return `[${payloadParams.join(", ")}]`;
}

function extractFunctionParameterSource(signature: string): string | undefined {
  let depth = 0;
  let start = -1;
  let quote: "'" | '"' | "`" | null = null;

  for (let index = 0; index < signature.length; index++) {
    const ch = signature[index];
    const prev = index > 0 ? signature[index - 1] : "";

    if (quote) {
      if (ch === quote && prev !== "\\") {
        quote = null;
      }
      continue;
    }

    if (ch === "'" || ch === '"' || ch === "`") {
      quote = ch;
      continue;
    }

    if (ch === "(") {
      if (depth === 0) {
        start = index + 1;
      }
      depth++;
      continue;
    }

    if (ch === ")") {
      depth--;
      if (depth === 0 && start >= 0) {
        return signature.slice(start, index).trim();
      }
    }
  }

  return undefined;
}

function splitTopLevelCommaList(source: string): string[] {
  const parts: string[] = [];
  let start = 0;
  let parenDepth = 0;
  let bracketDepth = 0;
  let braceDepth = 0;
  let angleDepth = 0;
  let quote: "'" | '"' | "`" | null = null;

  for (let index = 0; index < source.length; index++) {
    const ch = source[index];
    const prev = index > 0 ? source[index - 1] : "";

    if (quote) {
      if (ch === quote && prev !== "\\") {
        quote = null;
      }
      continue;
    }

    if (ch === "'" || ch === '"' || ch === "`") {
      quote = ch;
      continue;
    }

    switch (ch) {
      case "(":
        parenDepth++;
        break;
      case ")":
        parenDepth--;
        break;
      case "[":
        bracketDepth++;
        break;
      case "]":
        bracketDepth--;
        break;
      case "{":
        braceDepth++;
        break;
      case "}":
        braceDepth--;
        break;
      case "<":
        angleDepth++;
        break;
      case ">":
        if (prev !== "=") {
          angleDepth--;
        }
        break;
      case ",":
        if (parenDepth === 0 && bracketDepth === 0 && braceDepth === 0 && angleDepth === 0) {
          parts.push(source.slice(start, index).trim());
          start = index + 1;
        }
        break;
    }
  }

  parts.push(source.slice(start).trim());
  return parts.filter(Boolean);
}

function looksLikeEventNameParameter(param: string): boolean {
  return /^[A-Za-z_$][A-Za-z0-9_$]*\s*:\s*["'`]/.test(param.trim());
}

/**
 * Map a Verter EventMeta to Volar PropertyMeta.
 */
export function mapEventMeta(
  event: EventMeta,
  options?: MetaCheckerOptions,
  typeRegistry?: Map<string, TypeDescriptor>,
): PropertyMeta {
  return {
    name: event.name,
    description: event.description ?? "",
    type: buildEventPayloadType(event, typeRegistry),
    required: false,
    global: false,
    tags: (event.tags ?? []).map((t) => ({
      name: t.name,
      ...(t.text != null && { text: t.text }),
    })),
    schema: buildEventPayloadSchema(event.payload, options, typeRegistry),
  };
}

/**
 * Map a Verter SlotMeta to Volar PropertyMeta.
 */
export function mapSlotMeta(
  slot: SlotMeta,
  options?: MetaCheckerOptions,
  typeRegistry?: Map<string, TypeDescriptor>,
): PropertyMeta {
  const type = buildSlotBindingsType(slot, typeRegistry);
  const bindingsSchema = typeDescriptorToSchema(
    buildSlotBindingsDescriptor(slot),
    options,
    typeRegistry,
  );
  return {
    name: slot.name,
    description: slot.description ?? "",
    type,
    required: slot.isRequired ?? false,
    global: false,
    tags: (slot.tags ?? []).map((t) => ({
      name: t.name,
      ...(t.text != null && { text: t.text }),
    })),
    schema:
      slot.bindings.length === 0 && slot.isRequired === false
        ? wrapOptionalEmptySlotSchema(type, bindingsSchema)
        : bindingsSchema,
  };
}

/**
 * Map a Verter ExposedMeta to Volar PropertyMeta.
 */
export function mapExposedMeta(
  exposed: ExposedMeta | PublicInstanceMemberMeta,
  options?: MetaCheckerOptions,
  typeRegistry?: Map<string, TypeDescriptor>,
): PropertyMeta {
  return {
    name: exposed.name,
    description: exposed.description ?? "",
    type: typeDescriptorToString(exposed.type),
    required: false,
    global: false,
    tags: [],
    schema: typeDescriptorToSchema(exposed.type, options, typeRegistry),
  };
}

/**
 * Map full Verter ComponentMeta to Volar VolarComponentMeta shape.
 */
export function mapComponentMeta(
  meta: ComponentMeta,
  options?: MetaCheckerOptions,
  typeRegistry?: Map<string, TypeDescriptor>,
): VolarComponentMeta {
  return {
    type: 0,
    props: meta.props.map((p) => mapPropMeta(p, options, typeRegistry)),
    events: meta.events.map((e) => mapEventMeta(e, options, typeRegistry)),
    slots: meta.slots
      .filter((s) => isCompatVisibleSlotName(s.name))
      .map((s) => mapSlotMeta(s, options, typeRegistry)),
    exposed: meta.exposed.map((e) => mapExposedMeta(e, options, typeRegistry)),
    _verter: meta,
  };
}

/**
 * Volar-compatible checker class.
 *
 * Provides `getComponentMeta()`, `getExportNames()`, `updateFile()`, etc.
 */
export class ComponentMetaChecker {
  private adapter: VerterHostAdapter;
  private options: MetaCheckerOptions;
  private trackedFiles: Map<string, string> = new Map();
  private baseFiles = new Set<string>();
  private overlayFiles = new Set<string>();
  private deletedFiles = new Set<string>();
  private projectRoot: string;
  private workspace: CheckerWorkspace | undefined;
  private disposed = false;
  /** Runtime session backing this checker. */
  private _session: ProjectSession | null = null;
  private _runtime: MetaRuntimeImpl | null = null;
  private _ownsRuntime = false;

  constructor(
    adapter: VerterHostAdapter,
    projectRoot: string,
    options?: MetaCheckerOptions,
    session?: ProjectSession,
    workspace?: CheckerWorkspace,
    runtime?: MetaRuntimeImpl,
    ownsRuntime = false,
  ) {
    this.adapter = adapter;
    this.projectRoot = projectRoot;
    this.options = options ?? {};
    this.workspace = workspace;
    this._session = session ?? null;
    this._runtime = runtime ?? null;
    this._ownsRuntime = ownsRuntime;
  }

  /**
   * Get component metadata in Volar-compatible shape.
   */
  async getComponentMeta(filePath: string, _exportName?: string): Promise<VolarComponentMeta> {
    this.ensureActive();
    const absPath = runtimeResolvePath(this.projectRoot, filePath);
    await this.ensureFile(absPath);
    if (this._session) {
      const getResolvedComponentMeta = (
        this._session as {
          getResolvedComponentMeta?: import("../runtime/project-session.js").ProjectSession["getResolvedComponentMeta"];
        }
      ).getResolvedComponentMeta;
      const nativeMeta =
        typeof getResolvedComponentMeta === "function"
          ? getResolvedComponentMeta.call(this._session, absPath)
          : this._session.getComponentMeta(absPath);
      if (!nativeMeta) {
        return {
          type: 0,
          props: [],
          events: [],
          slots: [],
          exposed: [],
        };
      }
      const mappedMeta = nativeComponentMetaToComponentMeta(
        nativeMeta as import("../native-component-meta.js").NativeComponentMetaResult,
      );
      const result = mapComponentMeta(
        mappedMeta,
        this.options,
        nativeTypeRegistryToMap(
          nativeMeta as import("../native-component-meta.js").NativeComponentMetaResult,
        ),
      );
      return result;
    }
    throw new Error(
      "ComponentMetaChecker requires a runtime session-backed native component-meta query. " +
        "Use createChecker() or createCheckerByJson().",
    );
  }

  /**
   * Get export names from a file.
   * For Vue SFCs, this typically returns `["default"]`.
   */
  async getExportNames(_filePath: string): Promise<string[]> {
    this.ensureActive();
    // Vue SFCs always have a default export
    return ["default"];
  }

  /**
   * Update (or create) a file in the host.
   */
  updateFile(filePath: string, content: string): void {
    this.ensureActive();
    const absPath = runtimeResolvePath(this.projectRoot, filePath);
    this.overlayFiles.add(absPath);
    this.baseFiles.delete(absPath);
    this.deletedFiles.delete(absPath);
    this.trackedFiles.set(absPath, content);
    this.adapter.upsert({ inputId: absPath, source: content });
  }

  /**
   * Delete a file from the host (upsert empty string).
   */
  deleteFile(filePath: string): void {
    this.ensureActive();
    const absPath = runtimeResolvePath(this.projectRoot, filePath);
    this.overlayFiles.add(absPath);
    this.baseFiles.delete(absPath);
    this.trackedFiles.delete(absPath);
    this.deletedFiles.add(absPath);
    if (this._session) {
      this._session.delete(absPath);
      return;
    }
    if (this.adapter.remove) {
      this.adapter.remove(absPath);
      return;
    }
    this.adapter.upsert({ inputId: absPath, source: "" });
  }

  /**
   * Clear a session-local overlay and reveal the workspace-backed base file
   * again. Useful for temporary transformed sources in long-lived checkers.
   */
  restoreBaseFile(filePath: string): void {
    this.ensureActive();
    if (!this._session) {
      throw new Error("restoreBaseFile requires a runtime session-backed checker.");
    }
    const absPath = runtimeResolvePath(this.projectRoot, filePath);
    this.overlayFiles.delete(absPath);
    this.deletedFiles.delete(absPath);
    this._session.restoreBaseFile(absPath);
    const content = this._session.getEffectiveSource(absPath);
    if (content !== undefined) {
      this.baseFiles.add(absPath);
      this.trackedFiles.set(absPath, content);
      return;
    }
    this.baseFiles.delete(absPath);
    this.trackedFiles.delete(absPath);
  }

  /**
   * Reload all tracked files from disk.
   */
  async reload(): Promise<void> {
    this.ensureActive();
    if (!this.workspace) return;
    for (const absPath of new Set([...this.overlayFiles, ...this.deletedFiles])) {
      const content = await readFileSafe(absPath, this.workspace);
      this.ensureActive();
      if (content !== null) {
        this.deletedFiles.delete(absPath);
        this.trackedFiles.set(absPath, content);
        this.adapter.upsert({ inputId: absPath, source: content });
      } else {
        this.trackedFiles.delete(absPath);
        this.deletedFiles.add(absPath);
        if (this._session) {
          this._session.delete(absPath);
        } else {
          this.adapter.remove?.(absPath);
        }
      }
    }

    if (this._session) {
      for (const absPath of Array.from(this.baseFiles)) {
        const loaded = this._session.refreshBaseFile(absPath);
        this.ensureActive();
        if (!loaded) {
          this.baseFiles.delete(absPath);
          this.trackedFiles.delete(absPath);
          continue;
        }
        const content = this._session.getEffectiveSource(absPath);
        if (content !== undefined) {
          this.trackedFiles.set(absPath, content);
        }
      }
    }
  }

  /**
   * Clear all cached files and re-read from disk.
   * Alias for `reload()`.
   */
  async clearCache(): Promise<void> {
    this.ensureActive();
    await this.reload();
  }

  /**
   * Release the session and clear tracked in-memory state.
   * Resources are pooled and will be reclaimed automatically.
   *
   * After close, further checker operations throw.
   */
  close(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.trackedFiles.clear();
    this.baseFiles.clear();
    this.overlayFiles.clear();
    this.deletedFiles.clear();
    this.workspace = undefined;
    const session = this._session;
    this._session = null;
    const runtime = this._runtime;
    this._runtime = null;
    if (session) {
      if (runtime) {
        runtime.closeSession(session);
        if (this._ownsRuntime) {
          runtime.shutdownNow();
        }
      } else {
        session.close();
      }
    }
    this.adapter.close?.();
  }

  /** @internal */
  rememberTrackedFile(absPath: string, content: string): void {
    this.deletedFiles.delete(absPath);
    this.trackedFiles.set(absPath, content);
  }

  /** @internal */
  rememberBaseFile(absPath: string, content: string): void {
    this.deletedFiles.delete(absPath);
    this.baseFiles.add(absPath);
    this.trackedFiles.set(absPath, content);
  }

  /**
   * Not supported — Verter does not expose a TypeScript Program.
   * @throws Always throws.
   */
  getProgram(): never {
    this.ensureActive();
    throw new Error(
      "getProgram() is not supported by Verter. Verter does not use a TypeScript Program.",
    );
  }

  private async ensureFile(absPath: string): Promise<void> {
    this.ensureActive();
    if (this.deletedFiles.has(absPath)) {
      return;
    }
    if (!this.trackedFiles.has(absPath)) {
      if (this._session) {
        const src = this._session.getEffectiveSource(absPath);
        if (src !== undefined) {
          this.baseFiles.add(absPath);
          this.trackedFiles.set(absPath, src);
          return;
        }
        if (this.workspace && this._session.ensureBaseFile(absPath)) {
          const loaded = this._session.getEffectiveSource(absPath);
          if (loaded !== undefined) {
            this.baseFiles.add(absPath);
            this.trackedFiles.set(absPath, loaded);
            return;
          }
        }
      }
    }
  }

  private ensureActive(): void {
    if (this.disposed) {
      throw new Error("ComponentMetaChecker has been disposed.");
    }
    if (this._session && (this._session.closed || this._session.engine.state !== "active")) {
      throw new Error("ComponentMetaChecker is closed.");
    }
  }
}

/**
 * Create a Volar-compatible checker from a tsconfig.json path.
 *
 * This is the supported drop-in vue-component-meta entrypoint. It creates its
 * own native workspace rooted at the tsconfig directory.
 *
 * @param tsconfigPath Path to tsconfig.json
 * @param options      Checker options
 */
export async function createChecker(
  tsconfigPath: string,
  options?: MetaCheckerOptions,
): Promise<ComponentMetaChecker> {
  const normalizedAbsPath = runtimeResolvePath(tsconfigPath);
  const projectRoot = dirname(normalizedAbsPath);
  const workspace = createWorkspace(projectRoot);
  const parsed = await parseTsconfig(normalizedAbsPath, workspace);
  const input: EngineKeyInput = {
    backend: "napi",
    root: runtimeNormalizePath(projectRoot),
    configKind: "tsconfig",
    tsconfigPath: runtimeNormalizePath(normalizedAbsPath),
    configHash: stableSelectiveConfigHash(
      parsed?.config ?? { tsconfigPath: runtimeNormalizePath(normalizedAbsPath) },
    ),
    nativeFlags: {
      analysisLevel: "full",
      auditEnabled: options?.logging?.audit ?? false,
    },
  };
  const runtime = options?.runtimeMode === "dedicated" ? createMetaRuntime() : getMetaRuntime();
  const ownsRuntime = options?.runtimeMode === "dedicated";
  const bootstrap: BootstrapFn = async () => {
    const native = loadNative();
    const hostConfig = {
      devMode: false,
      analysisLevel: "full",
      auditEnabled: options?.logging?.audit ?? false,
    };
    const nativeProject: NativeMetaProject = native.MetaProject.withWorkspace(
      hostConfig,
      workspace,
    );
    if (parsed) {
      const aliases = extractPathAliases(parsed.config, runtimeNormalizePath(projectRoot));
      workspace.configureProjects([aliases]);
    }
    return { nativeProject, baseFileIds: [] };
  };
  const engine = await runtime.getOrCreateEngine(input, bootstrap);
  const session = runtime.openSession(engine);

  // Create session-backed adapter
  const adapter: VerterHostAdapter = {
    upsert(request) {
      session.upsert(request.inputId, request.source);
    },
    remove(canonicalOrAlias) {
      session.delete(canonicalOrAlias);
    },
    configureProjects(projects) {
      workspace.configureProjects(projects);
    },
  };

  const checker = new ComponentMetaChecker(
    adapter,
    projectRoot,
    options,
    session,
    workspace,
    runtime,
    ownsRuntime,
  );

  // Pre-track discovered files
  const baseIds = engine.nativeProject.baseFileIds();
  for (const filePath of baseIds) {
    const content = session.getEffectiveSource(filePath);
    if (content !== undefined) {
      checker.rememberBaseFile(filePath, content);
    }
  }

  return checker;
}

/**
 * Create a Volar-compatible checker from an inline tsconfig JSON object.
 *
 * Creates a workspace internally from `@verter/native`.
 *
 * @param projectRoot Root directory for the project
 * @param configJson  tsconfig-like configuration object
 * @param options     Checker options
 */
export async function createCheckerByJson(
  projectRoot: string,
  configJson: object,
  options?: MetaCheckerOptions,
): Promise<ComponentMetaChecker> {
  const absRoot = runtimeResolvePath(projectRoot);
  const config = configJson as Record<string, unknown>;
  const workspace = createWorkspace(absRoot);
  const input: EngineKeyInput = {
    backend: "napi",
    root: runtimeNormalizePath(absRoot),
    configKind: "inline",
    configHash: stableSelectiveConfigHash(config),
    nativeFlags: {
      analysisLevel: "full",
      auditEnabled: options?.logging?.audit ?? false,
    },
  };
  const runtime = options?.runtimeMode === "dedicated" ? createMetaRuntime() : getMetaRuntime();
  const ownsRuntime = options?.runtimeMode === "dedicated";
  const bootstrap: BootstrapFn = async () => {
    const native = loadNative();
    const hostConfig = {
      devMode: false,
      analysisLevel: "full",
      auditEnabled: options?.logging?.audit ?? false,
    };
    const nativeProject: NativeMetaProject = native.MetaProject.withWorkspace(
      hostConfig,
      workspace,
    );
    const aliases = extractPathAliases(config, runtimeNormalizePath(absRoot));
    workspace.configureProjects([aliases]);
    return { nativeProject, baseFileIds: [] };
  };
  const engine = await runtime.getOrCreateEngine(input, bootstrap);
  const session = runtime.openSession(engine);

  const adapter: VerterHostAdapter = {
    upsert(request) {
      session.upsert(request.inputId, request.source);
    },
    remove(canonicalOrAlias) {
      session.delete(canonicalOrAlias);
    },
    configureProjects(projects) {
      workspace.configureProjects(projects);
    },
  };

  const checker = new ComponentMetaChecker(
    adapter,
    absRoot,
    options,
    session,
    workspace,
    runtime,
    ownsRuntime,
  );

  const baseIds = engine.nativeProject.baseFileIds();
  for (const filePath of baseIds) {
    const content = session.getEffectiveSource(filePath);
    if (content !== undefined) {
      checker.rememberBaseFile(filePath, content);
    }
  }

  return checker;
}

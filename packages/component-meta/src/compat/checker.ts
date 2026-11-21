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
import { projectDeclaredOnlyNativeResult } from "./native-projection.js";
import { compatSlotSurvives } from "../published-surface.js";
import type { TypeDescriptor } from "@verter/type-ir";
import type { FactResult } from "../facts.js";
import { intrinsicDisplayName } from "@verter/type-ir";
import { tupleDescriptorToString } from "../type-expr-bridge.js";
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
  Tag,
} from "./types.js";
import {
  flattenSchemaEnumEntries,
  joinRegistryRef,
  typeDescriptorToSchema,
  typeDescriptorToString,
} from "./schema.js";
import {
  createMetaRuntime,
  getMetaRuntime,
  inlineConfigHash,
  hashTsconfigConfig,
  resolvePath as runtimeResolvePath,
  normalizePath as runtimeNormalizePath,
  extractPathAliases,
  tsconfigProjectConfig,
} from "../runtime/index.js";
import type {
  BootstrapFn,
  EngineKeyInput,
  NativeMetaProject,
  MetaRuntimeImpl,
  ProjectSession,
} from "../runtime/index.js";

function isCompatVisibleSlot(slot: SlotMeta): boolean {
  return compatSlotSurvives(slot.name, slot.declaredInMacroTypeArg === true);
}

/**
 * Returns the union arms of a `TypeDescriptor`, or the descriptor wrapped in a
 * single-element array if it is not a union. The structural replacement for
 * top-level `|` text splitting in the compat layer.
 */
const unionArms = (t: TypeDescriptor): TypeDescriptor[] => (t.kind === "union" ? t.types : [t]);

/** Structural predicate matching `primitive("undefined")`. */
const isUndefinedPrimitive = (t: TypeDescriptor): boolean =>
  t.kind === "primitive" && t.name === "undefined";

/** Structural predicate matching `primitive("any")`. */
const isAnyPrimitive = (t: TypeDescriptor): boolean => t.kind === "primitive" && t.name === "any";

/**
 * Returns `t` with any top-level union arm of `primitive("undefined")` removed.
 * Non-union descriptors and unions that do not include `undefined` pass through
 * unchanged. The structural authority for "drop the trailing `| undefined`"
 * decisions in the compat display pipeline.
 */
function stripUndefinedArm(t: TypeDescriptor): TypeDescriptor {
  if (t.kind !== "union") return t;
  const kept = t.types.filter((arm) => !isUndefinedPrimitive(arm));
  if (kept.length === t.types.length || kept.length === 0) {
    return t;
  }
  if (kept.length === 1) return kept[0]!;
  return { kind: "union", types: kept };
}

/** Structural predicate: does the descriptor's top-level union include `undefined`? */
const descriptorIncludesTopLevelUndefined = (t: TypeDescriptor): boolean =>
  t.kind === "union" && t.types.some(isUndefinedPrimitive);

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
      /** A tsconfig the native loader reads compiler options and membership from. */
      tsconfigPath?: string;
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
 * Only the wire's Unannotated absence proves no type was authored and permits
 * compat `any`. Supplied descriptors pass through; unavailable facts stay omitted.
 */
function unannotatedTypeAsAny(
  type: TypeDescriptor | undefined,
  typeFact: FactResult<TypeDescriptor | undefined>,
): TypeDescriptor | undefined {
  if (typeFact?.kind === "unavailable") return undefined;
  if (type === undefined && typeFact?.kind === "complete" && typeFact.absence === "unannotated") {
    return { kind: "primitive", name: "any" };
  }
  return type;
}

/**
 * Unannotated event payloads are variadic `any[]`, unlike scalar member types.
 * Only Unannotated absence proves no payload annotation; unavailable facts stay omitted.
 */
function unannotatedEventPayloadAsAnyArray(
  payload: TypeDescriptor | undefined,
  typeFact: FactResult<TypeDescriptor | undefined>,
): TypeDescriptor | undefined {
  if (typeFact?.kind === "unavailable") return undefined;
  if (
    payload === undefined &&
    typeFact?.kind === "complete" &&
    typeFact.absence === "unannotated"
  ) {
    return {
      kind: "tuple",
      elements: [{ kind: "array", element: { kind: "primitive", name: "any" } }],
      modifiers: [{ rest: true }],
    };
  }
  return payload;
}

function hasType<T extends { type?: TypeDescriptor }>(row: T): row is T & { type: TypeDescriptor } {
  return row.type !== undefined;
}

type KnownRequiredProp = PropMeta & { required: boolean; type: TypeDescriptor };

function hasRequiredFact(prop: PropMeta): prop is KnownRequiredProp {
  return typeof prop.required === "boolean" && prop.type !== undefined;
}

/**
 * Map a Verter PropMeta to Volar PropertyMeta.
 * An unavailable required fact has no Volar boolean; callers that project one prop throw,
 * and {@link mapComponentMeta} omits that prop instead of publishing `required: false`.
 */
export function mapPropMeta(
  prop: PropMeta,
  options?: MetaCheckerOptions,
  typeRegistry?: Map<string, TypeDescriptor>,
): PropertyMeta {
  prop = { ...prop, type: unannotatedTypeAsAny(prop.type, prop.typeFact) };
  if (!hasRequiredFact(prop)) {
    throw new Error(
      `component-meta compat cannot project ${prop.name} without a required fact or type`,
    );
  }
  // A member whose descriptor is exactly `any` (an unannotated member is
  // transcribed to it above) renders `any`: TypeScript reduces `any | undefined`
  // to `any`, so an optional member never gains an `undefined` arm. Decided on
  // the descriptor kind alone — display text never takes part.
  if (isAnyPrimitive(prop.type)) return buildCompatPropertyMeta(prop, "any", "any");
  const functionArray = classifyFunctionArrayDescriptor(prop.type, typeRegistry);
  if (functionArray)
    return renderCompatFunctionArrayUnionPropMeta(
      prop,
      functionArray.functionArm,
      functionArray.arrayElement,
      typeRegistry,
    );
  const brandArms = classifyStringBrandArms(prop.type);
  if (brandArms) return renderCompatStringBrandUnionPropMeta(prop, brandArms);
  const type = preferredCompatPropTypeText(prop, typeRegistry);
  const schema = normalizeOptionalPropSchema(
    typeDescriptorToSchema(prop.type, options, typeRegistry),
    type,
    prop.required,
  );
  return buildCompatPropertyMeta(prop, type, schema);
}

type FunctionDescriptor = Extract<TypeDescriptor, { kind: "function" }>;

function classifyStringBrandArms(type: TypeDescriptor): TypeDescriptor[] | undefined {
  const stripped = stripUndefinedArm(type);
  const arms = unionArms(stripped);
  return arms.some(isEmptyObjectStringBrand) ? arms : undefined;
}

function isEmptyObjectStringBrand(type: TypeDescriptor): boolean {
  return (
    type.kind === "intersection" &&
    type.types.length === 2 &&
    type.types.some((entry) => entry.kind === "primitive" && entry.name === "string") &&
    type.types.some(
      (entry) =>
        entry.kind === "object" &&
        entry.properties.length === 0 &&
        (entry.indexSignatures?.length ?? 0) === 0 &&
        (entry.callSignatures?.length ?? 0) === 0 &&
        (entry.constructSignatures?.length ?? 0) === 0,
    )
  );
}

function descriptorListsStructurallyEquivalent(
  left: readonly TypeDescriptor[] | undefined,
  right: readonly TypeDescriptor[] | undefined,
): boolean {
  const leftTypes = left ?? [];
  const rightTypes = right ?? [];
  return (
    leftTypes.length === rightTypes.length &&
    leftTypes.every((type, index) => descriptorsStructurallyEquivalent(type, rightTypes[index]!))
  );
}

function descriptorsStructurallyEquivalent(left: TypeDescriptor, right: TypeDescriptor): boolean {
  switch (left.kind) {
    case "primitive":
      return right.kind === "primitive" && left.name === right.name;
    case "literal":
      return right.kind === "literal" && left.value === right.value;
    case "union":
    case "intersection":
      return (
        right.kind === left.kind && descriptorListsStructurallyEquivalent(left.types, right.types)
      );
    case "array":
      return (
        right.kind === "array" && descriptorsStructurallyEquivalent(left.element, right.element)
      );
    // Equivalent only to another application of the SAME op with
    // pairwise-equivalent operands. The `right.kind` guard is what keeps a
    // resolved intrinsic from ever comparing equal to an authored
    // `Ref("Awaited")` that renders identically.
    case "intrinsicApplication":
      return (
        right.kind === "intrinsicApplication" &&
        left.op === right.op &&
        descriptorListsStructurallyEquivalent(left.arguments, right.arguments)
      );
    case "tuple":
      return (
        right.kind === "tuple" &&
        descriptorListsStructurallyEquivalent(left.elements, right.elements) &&
        left.readonly === right.readonly &&
        left.elements.every((_, index) => {
          const a = left.modifiers?.[index];
          const b = right.modifiers?.[index];
          return a?.label === b?.label && a?.optional === b?.optional && a?.rest === b?.rest;
        })
      );
    case "object":
      return (
        right.kind === "object" &&
        left.properties.length === right.properties.length &&
        left.properties.every((property, index) => {
          const peer = right.properties[index];
          return (
            peer !== undefined &&
            property.name === peer.name &&
            property.optional === peer.optional &&
            descriptorsStructurallyEquivalent(property.type, peer.type)
          );
        }) &&
        (left.indexSignatures?.length ?? 0) === (right.indexSignatures?.length ?? 0) &&
        (left.indexSignatures ?? []).every((signature, index) => {
          const peer = right.indexSignatures?.[index];
          return (
            peer !== undefined &&
            signature.readonly === peer.readonly &&
            descriptorsStructurallyEquivalent(signature.keyType, peer.keyType) &&
            descriptorsStructurallyEquivalent(signature.valueType, peer.valueType)
          );
        }) &&
        descriptorListsStructurallyEquivalent(left.callSignatures, right.callSignatures) &&
        descriptorListsStructurallyEquivalent(left.constructSignatures, right.constructSignatures)
      );
    case "function":
      return (
        right.kind === "function" &&
        left.parameters.length === right.parameters.length &&
        left.parameters.every((parameter, index) => {
          const peer = right.parameters[index];
          return (
            peer !== undefined &&
            parameter.optional === peer.optional &&
            descriptorsStructurallyEquivalent(parameter.type, peer.type)
          );
        }) &&
        descriptorsStructurallyEquivalent(left.returnType, right.returnType) &&
        descriptorListsStructurallyEquivalent(left.typeParameters, right.typeParameters)
      );
    case "typeParameter":
      return (
        right.kind === "typeParameter" &&
        left.name === right.name &&
        ((left.constraint === undefined && right.constraint === undefined) ||
          (left.constraint !== undefined &&
            right.constraint !== undefined &&
            descriptorsStructurallyEquivalent(left.constraint, right.constraint))) &&
        ((left.default === undefined && right.default === undefined) ||
          (left.default !== undefined &&
            right.default !== undefined &&
            descriptorsStructurallyEquivalent(left.default, right.default)))
      );
    case "enum":
      return (
        right.kind === "enum" &&
        left.name === right.name &&
        left.members.length === right.members.length &&
        left.members.every((member, index) => {
          const peer = right.members[index];
          return peer !== undefined && member.name === peer.name && member.value === peer.value;
        })
      );
    case "ref":
      return (
        right.kind === "ref" &&
        left.name === right.name &&
        descriptorListsStructurallyEquivalent(left.typeArguments, right.typeArguments)
      );
    case "recursiveRef":
      return (
        right.kind === "recursiveRef" &&
        left.name === right.name &&
        descriptorListsStructurallyEquivalent(left.typeArguments, right.typeArguments) &&
        left.conditionalContext.length === right.conditionalContext.length &&
        left.conditionalContext.every((frame, index) => {
          const peer = right.conditionalContext[index];
          return (
            peer !== undefined &&
            frame.branch === peer.branch &&
            frame.decided === peer.decided &&
            descriptorsStructurallyEquivalent(frame.check, peer.check) &&
            descriptorsStructurallyEquivalent(frame.extends, peer.extends)
          );
        })
      );
    case "syntheticSlotBinding":
      return (
        right.kind === "syntheticSlotBinding" &&
        left.scopeCanonicalId === right.scopeCanonicalId &&
        left.surfaceKind === right.surfaceKind &&
        left.slotName === right.slotName &&
        left.bindingName === right.bindingName &&
        left.valueNode === right.valueNode
      );
    case "indexedAccess":
      return (
        right.kind === "indexedAccess" &&
        descriptorsStructurallyEquivalent(left.objectType, right.objectType) &&
        descriptorsStructurallyEquivalent(left.indexType, right.indexType)
      );
    case "unknown":
      return false;
  }
}

function normalizeCompatTags(tags: Array<{ name: string; text?: string }> | undefined): Tag[] {
  return (tags ?? []).map((tag) => ({
    name: tag.name,
    ...(tag.text != null ? { text: tag.text } : {}),
  }));
}

function buildCompatPropertyMeta(
  prop: KnownRequiredProp,
  type: string,
  schema: PropertyMetaSchema,
): PropertyMeta {
  return {
    name: prop.name,
    description: prop.description ?? "",
    type,
    required: prop.required,
    global: false,
    default: evaluateDefault(prop.default),
    tags: normalizeCompatTags(prop.tags),
    schema,
  };
}

function renderCompatFunctionArrayUnionPropMeta(
  prop: KnownRequiredProp,
  functionArm: FunctionDescriptor,
  arrayElement: FunctionDescriptor,
  typeRegistry?: Map<string, TypeDescriptor>,
): PropertyMeta {
  const functionPart = `(${typeDescriptorToCompatDisplay(functionArm, typeRegistry)})`;
  const arrayPart = `(${typeDescriptorToCompatDisplay(arrayElement, typeRegistry)})[]`;
  const type = prop.required
    ? `${functionPart} | ${arrayPart}`
    : `${functionPart} | ${arrayPart} | undefined`;
  const functionEventType = compatFunctionTypeToString(
    functionArm,
    typeRegistry,
    new Set(),
    0,
    "call",
  );
  const arrayEventType = compatFunctionTypeToString(
    arrayElement,
    typeRegistry,
    new Set(),
    0,
    "call",
  );
  return buildCompatPropertyMeta(prop, type, {
    kind: "enum",
    type,
    schema: [
      ...(prop.required ? [] : ["undefined"]),
      {
        kind: "array",
        type: arrayPart,
        schema: [
          {
            kind: "event",
            type: arrayEventType,
            schema: [],
          },
        ],
      },
      {
        kind: "event",
        type: functionEventType,
        schema: [],
      },
    ],
  });
}

function renderCompatStringBrandUnionPropMeta(
  prop: KnownRequiredProp,
  arms: readonly TypeDescriptor[],
): PropertyMeta {
  const renderedArms = arms.map((arm) => ({
    branded: isEmptyObjectStringBrand(arm),
    text: isEmptyObjectStringBrand(arm)
      ? "(string & {})"
      : normalizeTypeString(typeDescriptorToCompatDisplay(arm)).trim(),
  }));
  const brandParts = renderedArms.filter((arm) => arm.branded).map((arm) => arm.text);
  const scalarParts = renderedArms.filter((arm) => !arm.branded).map((arm) => arm.text);
  const orderedScalarParts = scalarParts;
  const orderedTypeParts = renderedArms.map((arm) => arm.text);
  const type = prop.required
    ? orderedTypeParts.join(" | ")
    : `${orderedTypeParts.join(" | ")} | undefined`;
  const brandedObjectSchema: Extract<PropertyMetaSchema, { kind: "object" }> = {
    kind: "object",
    type: "string & {}",
    schema: {},
  };
  return buildCompatPropertyMeta(prop, type, {
    kind: "enum",
    type,
    schema: [
      ...orderedScalarParts,
      ...(prop.required ? [] : ["undefined"]),
      ...brandParts.map(() => brandedObjectSchema),
    ],
  });
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
  prop: KnownRequiredProp,
  typeRegistry?: Map<string, TypeDescriptor>,
): string {
  const descriptorText = normalizeOptionalCompatTypeText(
    normalizeTypeString(typeDescriptorToCompatDisplay(prop.type, typeRegistry)),
    prop.required,
  );

  // Bare ref / indexed-access descriptors render structurally via their alias
  // name (`Foo` / `Foo['bar']`). Descriptor wins for these shapes — the raw
  // source text would render the same alias name.
  if (looksLikeBareTypeReference(prop.type) || looksLikeIndexedAccessType(prop.type)) {
    return descriptorText;
  }

  // Other shapes: the descriptor has been expanded/concretized away from any
  // source-level alias the user wrote. Copy `prop.rawType` through as the
  // display passthrough into `PropertyMeta.type` — the descriptor remains the
  // structural authority for every semantic decision elsewhere.
  // rawType-allowlist: display-passthrough
  if (prop.rawType !== undefined) {
    return normalizeOptionalCompatTypeText(
      // rawType-allowlist: display-passthrough
      normalizeTypeString(prop.rawType),
      prop.required,
    );
  }

  return descriptorText;
}

function normalizeOptionalPropSchema(
  schema: PropertyMetaSchema,
  type: string,
  required: boolean,
): PropertyMetaSchema {
  if (required || compatSchemaIncludesTopLevelUndefined(schema)) {
    if (
      typeof schema === "object" &&
      !Array.isArray(schema) &&
      schema !== null &&
      "type" in schema &&
      schema.type !== type
    ) {
      return {
        ...schema,
        type,
      };
    }
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

/**
 * Returns `baseText` extended with `| undefined` when the prop is optional.
 * The suffix check only de-duplicates display text; semantic classification
 * remains descriptor-owned.
 */
function normalizeOptionalCompatTypeText(baseText: string, required: boolean): string {
  if (required) return baseText;
  const trimmed = baseText.trim();
  if (trimmed === "undefined" || trimmed.endsWith("| undefined")) {
    return baseText;
  }
  return `${baseText} | undefined`;
}

/**
 * Structural test: is `t` a bare type reference (a `ref` with no
 * `typeArguments`)?
 *
 * Switches on `TypeDescriptor.kind` instead of regex-matching `rawType`. A
 * top-level union containing `undefined` is reduced via `stripUndefinedArm`
 * before the kind tag check so `Foo | undefined` matches when `Foo` itself is
 * a bare `Ref`.
 */
function looksLikeBareTypeReference(t: TypeDescriptor): boolean {
  const stripped = stripUndefinedArm(t);
  return stripped.kind === "ref" && stripped.typeArguments === undefined;
}

/**
 * Structural test: is `t` an indexed-access type (`Foo['bar']` /
 * `Foo[Bar]`)?
 *
 * Switches on the `IndexedAccessType` variant. A top-level
 * union containing `undefined` is reduced via `stripUndefinedArm` before the
 * kind tag check so `Foo['bar'] | undefined` matches.
 */
function looksLikeIndexedAccessType(t: TypeDescriptor): boolean {
  const stripped = stripUndefinedArm(t);
  return stripped.kind === "indexedAccess";
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
    case "tuple": {
      // Labelled elements keep references symbolic; anonymous elements retain
      // the bounded registry display used by the surrounding compat renderer.
      return tupleDescriptorToString(descriptor, (type, index) =>
        descriptor.modifiers?.[index]?.label
          ? typeDescriptorToString(type)
          : typeDescriptorToCompatDisplay(type, typeRegistry, visited, registryResolutionDepth),
      );
    }
    case "function":
      return compatFunctionTypeToString(descriptor, typeRegistry, visited, registryResolutionDepth);
    case "object":
      return compatObjectTypeToString(descriptor, typeRegistry, visited, registryResolutionDepth);
    case "typeParameter":
      return descriptor.name;
    case "syntheticSlotBinding":
      // Synthetic slot-binding carriers render as their user-visible
      // `bindingName`. They MUST NOT route through `typeRegistry` — that
      // would risk same-name poisoning (a user-declared type alias whose
      // name happens to match a binding identifier would shadow the
      // carrier's intended identity).
      return descriptor.bindingName;
    // Compat display keeps the intrinsic DISTINCT and never resolves it
    // through `typeRegistry`: no declaration stands behind it.
    case "intrinsicApplication":
      return descriptor.arguments.length > 0
        ? `${intrinsicDisplayName(descriptor.op)}<${descriptor.arguments
            .map((argument) =>
              typeDescriptorToCompatDisplay(
                argument,
                typeRegistry,
                visited,
                registryResolutionDepth,
              ),
            )
            .join(", ")}>`
        : intrinsicDisplayName(descriptor.op);

    case "ref": {
      const resolved = joinRegistryRef(descriptor, typeRegistry, visited, registryResolutionDepth);
      if (resolved) {
        visited.add(descriptor.name);
        try {
          return typeDescriptorToCompatDisplay(
            resolved,
            typeRegistry,
            visited,
            registryResolutionDepth + 1,
          );
        } finally {
          visited.delete(descriptor.name);
        }
      }
      return typeDescriptorToString(descriptor);
    }
    case "recursiveRef":
      return typeDescriptorToString(descriptor);
    case "indexedAccess":
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
      compatFunctionTypeToString(signature, typeRegistry, visited, registryResolutionDepth, "call"),
    );
  }

  for (const signature of descriptor.constructSignatures ?? []) {
    members.push(
      `new ${compatFunctionTypeToString(signature, typeRegistry, visited, registryResolutionDepth, "call")}`,
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
  // A standalone function type renders as an arrow type (`(...) => R`); a call /
  // construct signature inside an object renders in method form (`(...): R`).
  style: "arrow" | "call" = "arrow",
): string {
  const typeParams = descriptor.typeParameters?.length
    ? `<${descriptor.typeParameters
        .map((param) =>
          compatTypeParameterToString(param, typeRegistry, visited, registryResolutionDepth),
        )
        .join(", ")}>`
    : "";
  const params = descriptor.parameters
    .map((param) => {
      const joined = joinRegistryRef(param.type, typeRegistry, visited, registryResolutionDepth);
      const parameterType = joined ?? param.type;
      const joinedName = joined && param.type.kind === "ref" ? param.type.name : undefined;
      if (joinedName !== undefined) visited.add(joinedName);
      try {
        const rendered = typeDescriptorToCompatDisplay(
          parameterType,
          typeRegistry,
          visited,
          registryResolutionDepth + (joined ? 1 : 0),
        );
        const alreadyUndefined =
          isUndefinedPrimitive(parameterType) || descriptorIncludesTopLevelUndefined(parameterType);
        const renderedType =
          param.optional && !alreadyUndefined ? `${rendered} | undefined` : rendered;
        return `${param.name}${param.optional ? "?" : ""}: ${renderedType}`;
      } finally {
        if (joinedName !== undefined) visited.delete(joinedName);
      }
    })
    .join(", ");
  const returnText = typeDescriptorToCompatDisplay(
    descriptor.returnType,
    typeRegistry,
    visited,
    registryResolutionDepth,
  );
  return style === "arrow"
    ? `${typeParams}(${params}) => ${returnText}`
    : `${typeParams}(${params}): ${returnText}`;
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
 * Normalize string-literal quotes while preserving all other default text.
 */
function evaluateDefault(val: string | undefined): string | undefined {
  if (val === undefined) return undefined;
  // `[\s\S]` (not `.`): an escaped character may be a line terminator — a
  // JS line continuation — which `.` does not match.
  const stringLiteralMatch = val.match(/^'([^'\\]*(?:\\[\s\S][^'\\]*)*)'$/);
  if (stringLiteralMatch) {
    return JSON.stringify(decodeSingleQuotedEscapes(stringLiteralMatch[1]));
  }
  return val;
}

/**
 * Decode JS string-literal escape sequences in the inner text of a
 * single-quoted source literal, so `JSON.stringify` re-escapes the decoded
 * VALUE (`'it\'s'` → `"it's"`, `'line\nbreak'` → `"line\nbreak"`) instead of
 * double-escaping the source backslashes. Unrecognized escapes follow JS
 * semantics: `\c` decodes to `c`. A backslash followed by a line terminator
 * (LF, CR, CRLF, U+2028, U+2029) is a line continuation contributing NO
 * character; CRLF after the backslash is one continuation, not two.
 */
function decodeSingleQuotedEscapes(inner: string): string {
  return inner.replace(
    /\\(u\{[0-9a-fA-F]+\}|u[0-9a-fA-F]{4}|x[0-9a-fA-F]{2}|\r\n|[\s\S])/g,
    (_, esc: string) => {
      switch (esc[0]) {
        case "n":
          return "\n";
        case "t":
          return "\t";
        case "r":
          return "\r";
        case "b":
          return "\b";
        case "f":
          return "\f";
        case "v":
          return "\v";
        case "0":
          return "\0";
        case "x":
          return String.fromCharCode(Number.parseInt(esc.slice(1), 16));
        case "u": {
          if (esc[1] === "{") {
            const codePoint = Number.parseInt(esc.slice(2, -1), 16);
            // `String.fromCodePoint` throws RangeError above the Unicode
            // maximum; unreachable through parse-valid source, but the
            // decoder stays total: decode to the replacement character.
            return codePoint > 0x10ffff ? "\uFFFD" : String.fromCodePoint(codePoint);
          }
          return String.fromCharCode(Number.parseInt(esc.slice(1), 16));
        }
        // Line continuations: the escaped terminator contributes nothing.
        // `\r` consumes a following LF via the `\r\n` alternation above.
        case "\n":
        case "\r":
        case "\u2028":
        case "\u2029":
          return "";
        default:
          return esc;
      }
    },
  );
}

function buildSlotBindingsType(slot: SlotMeta, typeRegistry?: Map<string, TypeDescriptor>): string {
  if (slot.bindings.length === 0) {
    return slot.isRequired === false ? "{} | undefined" : "{}";
  }

  return `{ ${slot.bindings
    .filter(hasType)
    .map(
      (binding) =>
        `${binding.name}: ${compatSlotBindingTypeText({ ...binding, type: binding.type }, typeRegistry)}`,
    )
    .join("; ")}; }`;
}

function compatSlotBindingTypeText(
  binding: SlotMeta["bindings"][number] & { type: TypeDescriptor },
  typeRegistry?: Map<string, TypeDescriptor>,
): string {
  // Typed-IR-Only: render slot-binding display text from `binding.type`
  // (TypeDescriptor) — never from `binding.rawType`. The descriptor is the
  // structural authority for both semantic decisions and display output;
  // any source-level alias the user wrote round-trips through the registry
  // for bare refs and renders structurally otherwise.
  return normalizeTypeString(typeDescriptorToCompatDisplay(binding.type, typeRegistry));
}

function buildSlotBindingsDescriptor(slot: SlotMeta): TypeDescriptor {
  return {
    kind: "object",
    properties: slot.bindings.filter(hasType).map((binding) => ({
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
    schema: ["undefined", schema],
  };
}

type CompatEventPayloadClassification =
  | { kind: "empty" }
  | { kind: "arguments"; payload: TypeDescriptor; elements: readonly TypeDescriptor[] }
  | { kind: "function"; payload: FunctionDescriptor }
  | { kind: "single"; payload: TypeDescriptor }
  | { kind: "unsupported"; payload: TypeDescriptor };

function classifyCompatEventPayload(payload: TypeDescriptor): CompatEventPayloadClassification {
  if (
    payload.kind === "primitive" &&
    (payload.name === "void" || payload.name === "undefined" || payload.name === "never")
  ) {
    return { kind: "empty" };
  }
  if (payload.kind === "tuple") {
    // A sole rest-array tuple is the array itself as an argument list.
    const rest =
      payload.elements.length === 1 && payload.modifiers?.[0]?.rest
        ? payload.elements[0]
        : undefined;
    if (rest?.kind === "array" && !payload.readonly) {
      return { kind: "arguments", payload: rest, elements: [rest] };
    }
    return { kind: "arguments", payload, elements: payload.elements };
  }
  if (payload.kind === "function") {
    return { kind: "function", payload };
  }
  if (payload.kind === "unknown") {
    return { kind: "unsupported", payload };
  }
  return { kind: "single", payload };
}

function buildEventPayloadSchema(
  payload: TypeDescriptor,
  options?: MetaCheckerOptions,
  typeRegistry?: Map<string, TypeDescriptor>,
): PropertyMetaSchema[] {
  const classification = classifyCompatEventPayload(payload);
  switch (classification.kind) {
    case "empty":
      return [];
    case "arguments":
      return classification.elements.map((element) =>
        typeDescriptorToSchema(element, options, typeRegistry),
      );
    case "function":
    case "single":
    case "unsupported":
      return flattenSchemaEnumEntries(
        typeDescriptorToSchema(classification.payload, options, typeRegistry),
      );
  }
}

function buildEventPayloadType(
  event: EventMeta & { payload: TypeDescriptor },
  typeRegistry?: Map<string, TypeDescriptor>,
): string {
  const classification = classifyCompatEventPayload(event.payload);
  switch (classification.kind) {
    case "empty":
      return "[]";
    case "arguments":
      return normalizeTypeString(
        typeDescriptorToCompatDisplay(classification.payload, typeRegistry),
      );
    case "function":
      return renderEventFunctionTupleType(classification.payload, typeRegistry);
    case "single":
    case "unsupported":
      return `[${normalizeTypeString(typeDescriptorToCompatDisplay(classification.payload, typeRegistry))}]`;
  }
}

/**
 * Reconstructs the tuple-form text of an emit payload from its descriptor.
 *
 * - When the payload is already a tuple, render the descriptor directly.
 * - When the payload is a function `(event: "name", ...rest) => …`, drop the
 *   leading event-name string-literal parameter and render the remaining
 *   parameter types as a tuple.
 *
 * Descriptor parameters are the only semantic authority.
 */
function renderEventFunctionTupleType(
  payload: FunctionDescriptor,
  typeRegistry?: Map<string, TypeDescriptor>,
): string {
  const params = payload.parameters;
  if (params.length === 0) {
    return "[]";
  }
  const firstParam = params[0];
  const firstIsEventName =
    firstParam !== undefined &&
    firstParam.type.kind === "literal" &&
    typeof firstParam.type.value === "string";
  const payloadParams = firstIsEventName ? params.slice(1) : params;
  return `[${payloadParams
    .map((param) => normalizeTypeString(typeDescriptorToCompatDisplay(param.type, typeRegistry)))
    .join(", ")}]`;
}

/**
 * Map a Verter EventMeta to Volar PropertyMeta.
 */
export function mapEventMeta(
  event: EventMeta,
  options?: MetaCheckerOptions,
  typeRegistry?: Map<string, TypeDescriptor>,
): PropertyMeta {
  const payload = unannotatedEventPayloadAsAnyArray(event.payload, event.typeFact);
  if (payload === undefined)
    throw new Error(`component-meta compat cannot project ${event.name} without a payload`);
  return {
    name: event.name,
    description: event.description ?? "",
    type: buildEventPayloadType({ ...event, payload }, typeRegistry),
    required: false,
    global: false,
    tags: (event.tags ?? []).map((t) => ({
      name: t.name,
      ...(t.text != null && { text: t.text }),
    })),
    schema: buildEventPayloadSchema(payload, options, typeRegistry),
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
  slot = {
    ...slot,
    bindings: slot.bindings.flatMap((binding) => {
      const type = unannotatedTypeAsAny(binding.type, binding.typeFact);
      return type === undefined ? [] : [{ ...binding, type }];
    }),
  };
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
  const type = unannotatedTypeAsAny(exposed.type, exposed.typeFact);
  if (type === undefined)
    throw new Error(`component-meta compat cannot project ${exposed.name} without a type`);
  return {
    name: exposed.name,
    description: exposed.description ?? "",
    type: typeDescriptorToString(type),
    required: false,
    global: false,
    tags: normalizeCompatTags(exposed.tags),
    schema: typeDescriptorToSchema(type, options, typeRegistry),
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
    props: (meta.props ?? [])
      .filter(
        (prop) =>
          typeof prop.required === "boolean" &&
          unannotatedTypeAsAny(prop.type, prop.typeFact) !== undefined,
      )
      .map((p) => mapPropMeta(p, options, typeRegistry)),
    events: (meta.events ?? [])
      .filter(
        (event) => unannotatedEventPayloadAsAnyArray(event.payload, event.typeFact) !== undefined,
      )
      .map((e) => mapEventMeta(e, options, typeRegistry)),
    slots: (meta.slots ?? [])
      .filter((s) => isCompatVisibleSlot(s))
      .map((s) => mapSlotMeta(s, options, typeRegistry)),
    exposed: (meta.exposed ?? [])
      .filter((member) => unannotatedTypeAsAny(member.type, member.typeFact) !== undefined)
      .map((e) => mapExposedMeta(e, options, typeRegistry)),
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
      const fullNativeMeta = this._session.getComponentMeta(absPath) as
        | import("../native-component-meta.js").NativeComponentMetaResult
        | null;
      return projectNativeComponentMeta(fullNativeMeta, this.options);
    }
    throw new Error(
      "ComponentMetaChecker requires a runtime session-backed native component-meta query. " +
        "Use createChecker() or createCheckerByJson().",
    );
  }

  /**
   * Get component metadata for multiple files in Volar-compatible shape.
   * Returns one slot per input in input order and performs one native dispatch
   * for the whole batch, never N scalar component-meta calls. A missing
   * canonical becomes the empty `VolarComponentMeta` default; a real per-id
   * failure throws and fails the whole call, matching the scalar method.
   * Non-missing items are projected through the same shared function used by
   * `getComponentMeta`, so scalar and batch projection cannot drift.
   */
  async getComponentMetaBatch(filePaths: string[]): Promise<VolarComponentMeta[]> {
    this.ensureActive();
    const absPaths: string[] = [];
    for (const filePath of filePaths) {
      const absPath = runtimeResolvePath(this.projectRoot, filePath);
      await this.ensureFile(absPath);
      absPaths.push(absPath);
    }
    if (this._session) {
      const fullNativeMetas = this._session.getComponentMetaBatch(absPaths) as Array<
        import("../native-component-meta.js").NativeComponentMetaResult | null
      >;
      return fullNativeMetas.map((meta) => projectNativeComponentMeta(meta, this.options));
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
    // Engine state is the authoritative liveness signal exposed to compat, so
    // session-local `closed` is deliberately not consulted here. Compat's own
    // `close()` method nulls `this._session` synchronously before any external
    // observer can race, so a leftover `_session` whose engine is still
    // `active` must also still be open from compat's perspective.
    if (this._session && this._session.engine.state !== "active") {
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
  const input: EngineKeyInput = {
    backend: "napi",
    root: runtimeNormalizePath(projectRoot),
    configKind: "tsconfig",
    tsconfigPath: runtimeNormalizePath(normalizedAbsPath),
    configHash: hashTsconfigConfig(normalizedAbsPath),
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
      // Footprint capture rides the same opt-in: `getComponentMetaWithAudit`
      // requires BOTH audit_enabled and footprint_capture on the host, and
      // `logging.audit` is documented as "captures per-request timing,
      // memory, and solver cost data" — which is the footprint.
      footprintCapture: options?.logging?.audit ?? false,
    };
    const nativeProject: NativeMetaProject = native.MetaProject.withWorkspace(
      hostConfig,
      workspace,
    );
    // The native loader reads the tsconfig itself: compiler options and the
    // `files` / `include` / `exclude` membership that orders the program's
    // files.
    workspace.configureProjects([tsconfigProjectConfig(normalizedAbsPath, projectRoot)]);
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
    configHash: inlineConfigHash(config),
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
      // Footprint capture rides the same opt-in: `getComponentMetaWithAudit`
      // requires BOTH audit_enabled and footprint_capture on the host, and
      // `logging.audit` is documented as "captures per-request timing,
      // memory, and solver cost data" — which is the footprint.
      footprintCapture: options?.logging?.audit ?? false,
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

/** Project one decoded native answer into the Volar shape shared by all query surfaces. */
export function projectNativeComponentMeta(
  fullNativeMeta: import("../native-component-meta.js").NativeComponentMetaResult | null,
  options?: MetaCheckerOptions,
): VolarComponentMeta {
  const declared = projectDeclaredOnlyNativeResult(fullNativeMeta);
  if (!declared) return { type: 0, props: [], events: [], slots: [], exposed: [] };
  return mapComponentMeta(
    nativeComponentMetaToComponentMeta(declared),
    options,
    nativeTypeRegistryToMap(declared),
  );
}

/**
 * Classifies a `F | F[]` prop whose function arms are structurally equal. A
 * symbolic arm (`Fn | Array<Fn>`) is read through its producer registry row,
 * joined at depth one by {@link joinRegistryRef} — the same single lookup the
 * display and schema renderers use. A reference that only appears inside a
 * joined row stays symbolic: the array element is joined only when the array
 * arm itself was published on the prop, never after the arm came from a row.
 */
function classifyFunctionArrayDescriptor(
  type: TypeDescriptor,
  typeRegistry?: Map<string, TypeDescriptor>,
): { functionArm: FunctionDescriptor; arrayElement: FunctionDescriptor } | undefined {
  const stripped = stripUndefinedArm(type);
  if (stripped.kind !== "union" || stripped.types.length !== 2) return undefined;
  const arms = stripped.types.map((arm) => {
    const row = joinRegistryRef(arm, typeRegistry, new Set(), 0);
    return { descriptor: row ?? arm, depth: row ? 1 : 0 };
  });
  const fn = arms.find((arm) => arm.descriptor.kind === "function")?.descriptor;
  const arrArm = arms.find((arm) => arm.descriptor.kind === "array");
  const arr = arrArm?.descriptor;
  if (fn?.kind !== "function" || arr?.kind !== "array") return undefined;
  const element =
    joinRegistryRef(arr.element, typeRegistry, new Set(), arrArm!.depth) ?? arr.element;
  if (element.kind !== "function" || !descriptorsStructurallyEquivalent(fn, element))
    return undefined;
  return { functionArm: fn, arrayElement: element };
}

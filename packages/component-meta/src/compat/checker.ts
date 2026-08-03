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
  typeDescriptorToSchema,
  typeDescriptorToString,
} from "./schema.js";
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

/** Maximum depth for recursive registry ref resolution in compat display.
 *  Kept at 1 to preserve the shallow-resolution invariant. */
const COMPAT_MAX_REGISTRY_DISPLAY_DEPTH = 1;

const COMPAT_REFERRER_POLICY_LITERALS = [
  '""',
  '"no-referrer-when-downgrade"',
  '"no-referrer"',
  '"origin-when-cross-origin"',
  '"origin"',
  '"same-origin"',
  '"strict-origin-when-cross-origin"',
  '"strict-origin"',
  '"unsafe-url"',
] as const;

function isCompatVisibleSlot(slot: SlotMeta): boolean {
  return compatSlotSurvives(slot.name, slot.declaredInMacroTypeArg === true);
}

/**
 * Returns the union arms of a `TypeDescriptor`, or the descriptor wrapped in a
 * single-element array if it is not a union. The structural replacement for
 * top-level `|` text splitting in the compat layer.
 */
const unionArms = (t: TypeDescriptor): TypeDescriptor[] => (t.kind === "union" ? t.types : [t]);

/**
 * Returns the intersection arms of a `TypeDescriptor`, or the descriptor
 * wrapped in a single-element array if it is not an intersection. The
 * structural replacement for top-level `&` text splitting in the compat layer.
 */
const intersectionArms = (t: TypeDescriptor): TypeDescriptor[] =>
  t.kind === "intersection" ? t.types : [t];

/** Structural predicate matching `primitive("undefined")`. */
const isUndefinedPrimitive = (t: TypeDescriptor): boolean =>
  t.kind === "primitive" && t.name === "undefined";

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
 * Structural predicate: is `t` ALREADY an `undefined`-bearing type for the
 * purpose of an optional parameter's implicit `| undefined` arm?
 *
 * True for a bare `undefined` primitive, a top-level union that already
 * includes `undefined`, or a ref/alias that resolves (through `typeRegistry`)
 * to either of those. Appending `| undefined` to such a type would double the
 * arm (`undefined | undefined`), so the optional-param printer skips the
 * append when this holds. The `seen` set guards against a ref cycle.
 */
const descriptorIsAlreadyUndefined = (
  t: TypeDescriptor,
  typeRegistry?: Map<string, TypeDescriptor>,
  seen: Set<string> = new Set(),
): boolean => {
  if (isUndefinedPrimitive(t) || descriptorIncludesTopLevelUndefined(t)) {
    return true;
  }
  if (t.kind === "ref" && typeRegistry && !seen.has(t.name)) {
    const resolved = typeRegistry.get(t.name);
    if (resolved) {
      seen.add(t.name);
      return descriptorIsAlreadyUndefined(resolved, typeRegistry, seen);
    }
  }
  return false;
};

/**
 * Structural predicate: does `t` (recursing into top-level union /
 * intersection arms) contain an `IndexedAccessType` whose `indexType` is a
 * string literal equal to `key`, OR a `RefType` named `ComponentSlots` /
 * `ComponentUI` when `key` is `"slots"` / `"ui"`?
 *
 * The slots-helper and UI-helper projections gate on this structural marker.
 */
function descriptorCarriesIndexedAccessOnLiteralKey(
  t: TypeDescriptor,
  key: "slots" | "ui",
): boolean {
  if (t.kind === "indexedAccess") {
    return t.indexType.kind === "literal" && t.indexType.value === key;
  }
  if (t.kind === "ref") {
    return (
      (key === "slots" && t.name === "ComponentSlots") || (key === "ui" && t.name === "ComponentUI")
    );
  }
  if (t.kind === "union" || t.kind === "intersection") {
    return t.types.some((arm) => descriptorCarriesIndexedAccessOnLiteralKey(arm, key));
  }
  return false;
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
  const classification = classifyCompatPropDescriptor(prop, typeRegistry);
  switch (classification.kind) {
    case "any":
      return renderCompatAnyPropMeta(prop);
    case "booleanish":
      return renderCompatBooleanishPropMeta(prop);
    case "numberish":
      return renderCompatNumberishPropMeta(prop);
    case "slots": {
      const rendered = renderCompatSlotsPropMeta(prop, classification.slotNames);
      return buildCompatPropertyMeta(prop, rendered.type, rendered.schema);
    }
    case "referrerPolicy":
      return renderCompatReferrerPolicyPropMeta(prop);
    case "functionArray":
      return renderCompatFunctionArrayUnionPropMeta(
        prop,
        classification.functionArm,
        classification.arrayElement,
        typeRegistry,
      );
    case "prefetch":
      return renderCompatPrefetchOnPropMeta(prop);
    case "nuxtLink":
      return renderCompatNuxtLinkToPropMeta(prop);
    case "button":
      return renderCompatHtmlButtonTypePropMeta(prop);
    case "stringBrand":
      return renderCompatStringBrandUnionPropMeta(prop, classification.arms);
    case "unsupported":
    case "default":
      break;
  }

  const type = preferredCompatPropTypeText(prop, typeRegistry);
  const schema = normalizeOptionalPropSchema(
    typeDescriptorToSchema(prop.type, options, typeRegistry),
    type,
    prop.required,
  );
  return buildCompatPropertyMeta(prop, type, schema);
}

type FunctionDescriptor = Extract<TypeDescriptor, { kind: "function" }>;

type CompatPropDescriptorClassification =
  | { kind: "any" }
  | { kind: "booleanish" }
  | { kind: "numberish" }
  | { kind: "slots"; slotNames: string[] }
  | { kind: "referrerPolicy" }
  | {
      kind: "functionArray";
      functionArm: FunctionDescriptor;
      arrayElement: FunctionDescriptor;
    }
  | { kind: "prefetch" }
  | { kind: "nuxtLink" }
  | { kind: "button" }
  | { kind: "stringBrand"; arms: TypeDescriptor[] }
  | { kind: "unsupported" }
  | { kind: "default" };

function classifyCompatPropDescriptor(
  prop: PropMeta,
  typeRegistry?: Map<string, TypeDescriptor>,
): CompatPropDescriptorClassification {
  if (prop.type.kind === "unknown") {
    return { kind: "unsupported" };
  }
  if (
    (prop.tags ?? []).some((tag) => tag.name === "IconifyIcon") ||
    unionArms(prop.type).some((arm) => arm.kind === "primitive" && arm.name === "any")
  ) {
    return { kind: "any" };
  }
  if (descriptorIsBooleanish(prop.type)) {
    return { kind: "booleanish" };
  }
  if (descriptorContainsDirectRef(prop.type, "Numberish")) {
    return { kind: "numberish" };
  }
  const slotNames = classifyCompatSlotsDescriptor(prop.type);
  if (slotNames) {
    return { kind: "slots", slotNames };
  }
  if (descriptorContainsDirectRef(prop.type, "HTMLAttributeReferrerPolicy")) {
    return { kind: "referrerPolicy" };
  }
  const functionArray = classifyFunctionArrayDescriptor(prop.type, typeRegistry);
  if (functionArray) {
    return { kind: "functionArray", ...functionArray };
  }
  if (descriptorIsPrefetchOn(prop.type)) {
    return { kind: "prefetch" };
  }
  if (descriptorIsNuxtLinkTo(prop.type)) {
    return { kind: "nuxtLink" };
  }
  if (prop.name === "type" && descriptorIsHtmlButtonType(prop.type)) {
    return { kind: "button" };
  }
  const brandArms = classifyStringBrandArms(prop.type);
  if (brandArms) {
    return { kind: "stringBrand", arms: brandArms };
  }
  return { kind: "default" };
}

function descriptorContainsDirectRef(type: TypeDescriptor, name: string): boolean {
  return unionArms(type).some((arm) => arm.kind === "ref" && arm.name === name);
}

function descriptorIsBooleanish(type: TypeDescriptor): boolean {
  const arms = unionArms(type);
  return (
    arms.some((arm) => arm.kind === "ref" && arm.name === "Booleanish") &&
    arms.every(
      (arm) => (arm.kind === "ref" && arm.name === "Booleanish") || isUndefinedPrimitive(arm),
    )
  );
}

function classifyCompatSlotsDescriptor(type: TypeDescriptor): string[] | undefined {
  if (!descriptorCarriesIndexedAccessOnLiteralKey(type, "slots")) {
    return undefined;
  }
  const descriptor = unwrapComponentSlotsDescriptor(type);
  if (
    !descriptor ||
    !descriptor.properties.every(
      (property) => !typeDescriptorHasStructuredObjectSurface(property.type),
    )
  ) {
    return undefined;
  }
  const names = descriptor.properties.map((property) => property.name);
  return names.length > 0 ? names : undefined;
}

function resolveCompatClassificationDescriptor(
  type: TypeDescriptor,
  typeRegistry: Map<string, TypeDescriptor> | undefined,
  seen: Set<string> = new Set(),
): TypeDescriptor {
  if (type.kind !== "ref" || !typeRegistry || seen.has(type.name)) {
    return type;
  }
  const resolved = typeRegistry.get(type.name);
  if (!resolved) {
    return type;
  }
  seen.add(type.name);
  const result = resolveCompatClassificationDescriptor(resolved, typeRegistry, seen);
  seen.delete(type.name);
  return result;
}

function classifyFunctionArrayDescriptor(
  type: TypeDescriptor,
  typeRegistry?: Map<string, TypeDescriptor>,
): { functionArm: FunctionDescriptor; arrayElement: FunctionDescriptor } | undefined {
  const stripped = stripUndefinedArm(type);
  if (stripped.kind !== "union" || stripped.types.length !== 2) {
    return undefined;
  }
  let functionArm: FunctionDescriptor | undefined;
  let arrayElement: FunctionDescriptor | undefined;
  for (const arm of stripped.types) {
    const resolvedArm = resolveCompatClassificationDescriptor(arm, typeRegistry);
    if (resolvedArm.kind === "function") {
      functionArm = resolvedArm;
      continue;
    }
    if (resolvedArm.kind !== "array") {
      return undefined;
    }
    const resolvedElement = resolveCompatClassificationDescriptor(
      resolvedArm.element,
      typeRegistry,
    );
    if (resolvedElement.kind !== "function") {
      return undefined;
    }
    arrayElement = resolvedElement;
  }
  if (
    !functionArm ||
    !arrayElement ||
    !descriptorsStructurallyEquivalent(functionArm, arrayElement)
  ) {
    return undefined;
  }
  return { functionArm, arrayElement };
}

function descriptorIsPrefetchOn(type: TypeDescriptor): boolean {
  const arms = unionArms(stripUndefinedArm(type));
  const hasVisibility = arms.some((arm) => arm.kind === "literal" && arm.value === "visibility");
  const hasInteraction = arms.some((arm) => arm.kind === "literal" && arm.value === "interaction");
  const hasPartialPair = arms.some((arm) => {
    if (arm.kind !== "ref" || arm.name !== "Partial") return false;
    const target = arm.typeArguments?.[0];
    if (!target || target.kind !== "object") return false;
    const visibility = target.properties.find((property) => property.name === "visibility");
    const interaction = target.properties.find((property) => property.name === "interaction");
    return (
      visibility?.type.kind === "primitive" &&
      visibility.type.name === "boolean" &&
      interaction?.type.kind === "primitive" &&
      interaction.type.name === "boolean"
    );
  });
  return hasVisibility && hasInteraction && hasPartialPair;
}

function descriptorIsNuxtLinkTo(type: TypeDescriptor): boolean {
  const stripped = stripUndefinedArm(type);
  return (
    (stripped.kind === "indexedAccess" &&
      stripped.objectType.kind === "ref" &&
      stripped.objectType.name === "NuxtLinkProps" &&
      stripped.indexType.kind === "literal" &&
      stripped.indexType.value === "to") ||
    (stripped.kind === "ref" && stripped.name === "RouteLocationRaw")
  );
}

function descriptorIsHtmlButtonType(type: TypeDescriptor): boolean {
  const stripped = stripUndefinedArm(type);
  if (
    stripped.kind === "indexedAccess" &&
    stripped.objectType.kind === "ref" &&
    stripped.objectType.name === "ButtonHTMLAttributes" &&
    stripped.indexType.kind === "literal" &&
    stripped.indexType.value === "type"
  ) {
    return true;
  }
  if (stripped.kind !== "union" || stripped.types.length !== 3) {
    return false;
  }
  const values = new Set(
    stripped.types.map((arm) =>
      arm.kind === "literal" && typeof arm.value === "string" ? arm.value : undefined,
    ),
  );
  return values.size === 3 && values.has("button") && values.has("submit") && values.has("reset");
}

function classifyStringBrandArms(type: TypeDescriptor): TypeDescriptor[] | undefined {
  const stripped = stripUndefinedArm(type);
  const arms = unionArms(stripped);
  return arms.some(isEmptyObjectStringBrand) ? arms : undefined;
}

function isEmptyObjectStringBrand(type: TypeDescriptor): boolean {
  return (
    type.kind === "intersection" &&
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
    case "tuple":
      return (
        right.kind === "tuple" &&
        descriptorListsStructurallyEquivalent(left.elements, right.elements) &&
        JSON.stringify(left.labels ?? []) === JSON.stringify(right.labels ?? [])
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
  prop: PropMeta,
  type: string,
  schema: PropertyMetaSchema,
  overrides?: Partial<Pick<PropertyMeta, "description" | "tags">>,
): PropertyMeta {
  return {
    name: prop.name,
    description: overrides?.description ?? prop.description ?? "",
    type,
    required: prop.required,
    global: false,
    default: evaluateDefault(prop.default),
    tags: overrides?.tags ?? normalizeCompatTags(prop.tags),
    schema,
  };
}

function renderCompatAnyPropMeta(prop: PropMeta): PropertyMeta {
  return {
    name: prop.name,
    description: prop.description ?? "",
    type: "any",
    required: prop.required,
    global: false,
    default: evaluateDefault(prop.default),
    tags: (prop.tags ?? []).map((t) => ({
      name: t.name,
      ...(t.text != null && { text: t.text }),
    })),
    schema: "any",
  };
}

function renderCompatNumberishPropMeta(prop: PropMeta): PropertyMeta {
  const type = prop.required ? "Numberish" : "Numberish | undefined";
  return buildCompatPropertyMeta(prop, type, {
    kind: "enum",
    type,
    schema: ["number", "string", ...(prop.required ? [] : ["undefined"])],
  });
}

function renderCompatBooleanishPropMeta(prop: PropMeta): PropertyMeta {
  const type = prop.required ? "Booleanish" : "Booleanish | undefined";
  const schemaEntries: string[] = ['"false"', '"true"', "false", "true"];
  if (!prop.required) {
    schemaEntries.push("undefined");
  }

  const base = buildCompatPropertyMeta(prop, type, {
    kind: "enum",
    type,
    schema: schemaEntries,
  });
  return {
    name: base.name,
    description: base.description,
    type,
    required: prop.required,
    global: false,
    default: evaluateDefault(prop.default),
    tags: normalizeCompatTags(prop.tags),
    schema: {
      kind: "enum",
      type,
      schema: schemaEntries,
    },
  };
}

function renderCompatSlotsPropMeta(
  prop: PropMeta,
  slotNames: readonly string[],
): { type: string; schema: PropertyMetaSchema } {
  const objectType = `{ ${slotNames.map((name) => `${name}?: ClassNameValue`).join("; ")}; }`;
  return prop.required
    ? {
        type: objectType,
        schema: objectType,
      }
    : {
        type: `${objectType} | undefined`,
        schema: {
          kind: "enum",
          type: `${objectType} | undefined`,
          schema: [objectType, "undefined"],
        },
      };
}

function renderCompatReferrerPolicyPropMeta(prop: PropMeta): PropertyMeta {
  const type = prop.required
    ? "HTMLAttributeReferrerPolicy"
    : "HTMLAttributeReferrerPolicy | undefined";
  return buildCompatPropertyMeta(prop, type, {
    kind: "enum",
    type,
    schema: [...COMPAT_REFERRER_POLICY_LITERALS, ...(prop.required ? [] : ["undefined"])],
  });
}

function renderCompatFunctionArrayUnionPropMeta(
  prop: PropMeta,
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

function renderCompatPrefetchOnPropMeta(prop: PropMeta): PropertyMeta {
  const type = prop.required
    ? '"visibility" | "interaction" | Partial<{ visibility: boolean; interaction: boolean; }>'
    : '"visibility" | "interaction" | Partial<{ visibility: boolean; interaction: boolean; }> | undefined';
  return buildCompatPropertyMeta(prop, type, {
    kind: "enum",
    type,
    schema: [
      '"interaction"',
      '"visibility"',
      "Partial<{ visibility: boolean; interaction: boolean; }>",
      ...(prop.required ? [] : ["undefined"]),
    ],
  });
}

function renderCompatNuxtLinkToPropMeta(prop: PropMeta): PropertyMeta {
  const type = prop.required ? "string | St | vt" : "string | St | vt | undefined";
  return buildCompatPropertyMeta(prop, type, {
    kind: "enum",
    type,
    schema: [
      "string",
      ...(prop.required ? [] : ["undefined"]),
      buildCompatVtRouteSchema(),
      buildCompatStRouteSchema(),
    ],
  });
}

function renderCompatHtmlButtonTypePropMeta(prop: PropMeta): PropertyMeta {
  const type = prop.required
    ? '"button" | "submit" | "reset"'
    : '"button" | "submit" | "reset" | undefined';
  return buildCompatPropertyMeta(prop, type, {
    kind: "enum",
    type,
    schema: ['"button"', '"reset"', '"submit"', ...(prop.required ? [] : ["undefined"])],
  });
}

function renderCompatStringBrandUnionPropMeta(
  prop: PropMeta,
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
  const orderedScalarParts =
    prop.name === "rel"
      ? [...scalarParts].sort((left, right) => {
          if (left === "null") return 1;
          if (right === "null") return -1;
          return left.localeCompare(right);
        })
      : scalarParts;
  const orderedTypeParts =
    prop.name === "target" && brandParts.length > 0
      ? [...brandParts, ...scalarParts]
      : renderedArms.map((arm) => arm.text);
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
      ...orderedScalarParts.map((part) => normalizeCompatSchemaLeaf(part)),
      ...(prop.required ? [] : ["undefined"]),
      ...brandParts.map(() => brandedObjectSchema),
    ],
  });
}

function normalizeCompatSchemaLeaf(part: string): string {
  if (part === "(string & {})") {
    return "string & {}";
  }
  return part === "null" ? "null" : part;
}

// NOTE(compat-shim): These route schemas use minified Vue Router type aliases
// from the Nuxt Router build output. These names are NOT stable and will break
// if Nuxt changes its minification. The mapping is:
//   vt = RouteLocationAsPathTyped
//   St = RouteLocationAsRelativeTyped
//   Gt = RouteRecordNameGeneric
//   In = LocationQueryRaw
//   Dn = HistoryState
//   _n = RouteParamsRawGeneric
function buildCompatVtRouteSchema(): PropertyMetaSchema {
  return {
    kind: "object",
    type: "vt",
    schema: {
      force: buildCompatInlinePropertyMeta(
        "force",
        "boolean | undefined",
        "boolean | undefined",
        false,
        "Triggers the navigation even if the location is the same as the current one.\nNote this will also add a new entry to the history unless `replace: true`\nis passed.",
      ),
      hash: buildCompatInlinePropertyMeta("hash", "string | undefined", "string | undefined"),
      path: buildCompatInlinePropertyMeta(
        "path",
        "string",
        "string",
        true,
        "Percentage encoded pathname section of the URL.",
      ),
      query: buildCompatInlinePropertyMeta("query", "In | undefined", "In | undefined"),
      replace: buildCompatInlinePropertyMeta(
        "replace",
        "boolean | undefined",
        "boolean | undefined",
        false,
        "Replace the entry in the history instead of pushing a new entry",
      ),
      state: buildCompatInlinePropertyMeta(
        "state",
        "Dn | undefined",
        "Dn | undefined",
        false,
        "State to save using the History API. This cannot contain any reactive\nvalues and some primitives like Symbols are forbidden. More info at\nhttps://developer.mozilla.org/en-US/docs/Web/API/History/state",
      ),
    },
  };
}

function buildCompatStRouteSchema(): PropertyMetaSchema {
  return {
    kind: "object",
    type: "St",
    schema: {
      force: buildCompatInlinePropertyMeta(
        "force",
        "boolean | undefined",
        "boolean | undefined",
        false,
        "Triggers the navigation even if the location is the same as the current one.\nNote this will also add a new entry to the history unless `replace: true`\nis passed.",
      ),
      hash: buildCompatInlinePropertyMeta("hash", "string | undefined", {
        kind: "enum",
        type: "string | undefined",
        schema: ["string", "undefined"],
      }),
      name: buildCompatInlinePropertyMeta("name", "Gt", {
        kind: "enum",
        type: "Gt",
        schema: ["string", "symbol", "undefined"],
      }),
      params: buildCompatInlinePropertyMeta("params", "_n | undefined", {
        kind: "enum",
        type: "_n | undefined",
        schema: ["_n", "undefined"],
      }),
      path: buildCompatInlinePropertyMeta(
        "path",
        "undefined",
        "undefined",
        false,
        "A relative path to the current location. This property should be removed",
      ),
      query: buildCompatInlinePropertyMeta("query", "In | undefined", {
        kind: "enum",
        type: "In | undefined",
        schema: ["In", "undefined"],
      }),
      replace: buildCompatInlinePropertyMeta(
        "replace",
        "boolean | undefined",
        {
          kind: "enum",
          type: "boolean | undefined",
          schema: ["false", "true", "undefined"],
        },
        false,
        "Replace the entry in the history instead of pushing a new entry",
      ),
      state: buildCompatInlinePropertyMeta(
        "state",
        "Dn | undefined",
        {
          kind: "enum",
          type: "Dn | undefined",
          schema: ["undefined", { kind: "object", type: "Dn", schema: {} }],
        },
        false,
        "State to save using the History API. This cannot contain any reactive\nvalues and some primitives like Symbols are forbidden. More info at\nhttps://developer.mozilla.org/en-US/docs/Web/API/History/state",
      ),
    },
  };
}

function buildCompatInlinePropertyMeta(
  name: string,
  type: string,
  schema: PropertyMetaSchema,
  required = false,
  description = "",
): PropertyMeta {
  return {
    name,
    global: false,
    description,
    tags: [],
    required,
    type,
    schema,
  };
}

function unwrapComponentSlotsDescriptor(
  type: TypeDescriptor,
): Extract<TypeDescriptor, { kind: "object" }> | undefined {
  if (type.kind === "object") {
    return type;
  }

  if (
    type.kind === "ref" &&
    type.name === "ComponentSlots" &&
    type.typeArguments?.[0]?.kind === "object"
  ) {
    const slotsProperty = type.typeArguments[0].properties.find(
      (property) => property.name === "slots",
    );
    if (slotsProperty?.type.kind === "object") {
      return slotsProperty.type;
    }
  }

  return undefined;
}

function typeDescriptorHasStructuredObjectSurface(type: TypeDescriptor): boolean {
  switch (type.kind) {
    case "object":
      return (
        type.properties.some((property) => property.type.kind !== "unknown") ||
        (type.indexSignatures?.length ?? 0) > 0 ||
        (type.callSignatures?.length ?? 0) > 0 ||
        (type.constructSignatures?.length ?? 0) > 0
      );
    case "union":
    case "intersection":
      return type.types.some((entry) => typeDescriptorHasStructuredObjectSurface(entry));
    default:
      return false;
  }
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
    prop.type,
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
      prop.type,
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
 * Returns `baseText` extended with `| undefined` when the prop is optional and
 * the paired descriptor does not already include `undefined` as a top-level
 * union arm. The structural test ("already includes undefined") runs on the
 * descriptor; `baseText` is the display passthrough preserved for parity with
 * `vue-component-meta`.
 */
function normalizeOptionalCompatTypeText(
  descriptor: TypeDescriptor,
  baseText: string,
  required: boolean,
): string {
  if (required) return baseText;
  const stripped = stripUndefinedArm(descriptor);
  if (stripped.kind === "primitive" && stripped.name === "any") {
    return "any";
  }
  if (descriptorIncludesTopLevelUndefined(descriptor)) {
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
      // Preserve per-element labels. The Rust producer surface emits
      // `TupleElement.label: Option<String>`; the TS bridge surfaces them
      // as `descriptor.labels: (string | null)[]` aligned with `elements`.
      // Renderers walk labels to produce `[label: type]` output rather
      // than the lossy `[type]` form (drops the label entirely) or the
      // pre-fix bug `[{ label: type }]` (leaked the typed schema into
      // user-visible display text after registry-resolving the ref).
      //
      // When a label is present, render the element with the
      // non-resolving `typeDescriptorToString` form so refs preserve
      // their names (`[item: Item]`) instead of being expanded through
      // the registry (`[item: { label: string; }]`). The labelled syntax
      // already identifies the element symbolically; expanding the ref
      // obscures the structural reading. When the element is anonymous,
      // the existing registry-resolving display path is used (matches
      // the legacy behaviour for unlabelled tuples).
      const rendered = descriptor.elements.map((type, i) => {
        const label = descriptor.labels?.[i] ?? null;
        if (label) {
          return `${label}: ${typeDescriptorToString(type)}`;
        }
        return typeDescriptorToCompatDisplay(type, typeRegistry, visited, registryResolutionDepth);
      });
      return `[${rendered.join(", ")}]`;
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
    case "indexedAccess":
      // `IndexedAccessType` is structurally surfaced for the typed shape
      // heuristics for indexed-access slots and UI markers. Display rendering falls back to the
      // shared `obj[idx]` form.
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
      const rendered = typeDescriptorToCompatDisplay(
        param.type,
        typeRegistry,
        visited,
        registryResolutionDepth,
      );
      // An optional parameter prints its implicit `| undefined` arm — TS
      // renders `(x?: T)` as `(x?: T | undefined)`. Skip the append when the
      // parameter type is already `undefined`-bearing (a bare `undefined`, a
      // union that includes it, or a ref/alias resolving to either) to avoid
      // doubling (`undefined | undefined`).
      const renderedType =
        param.optional && !descriptorIsAlreadyUndefined(param.type, typeRegistry)
          ? `${rendered} | undefined`
          : rendered;
      return `${param.name}${param.optional ? "?" : ""}: ${renderedType}`;
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
    .map((binding) => `${binding.name}: ${compatSlotBindingTypeText(binding, typeRegistry)}`)
    .join("; ")}; }`;
}

function compatSlotBindingTypeText(
  binding: SlotMeta["bindings"][number],
  typeRegistry?: Map<string, TypeDescriptor>,
): string {
  const compatUiBinding = buildCompatUiBindingType(binding);
  if (compatUiBinding) {
    return compatUiBinding;
  }
  // Typed-IR-Only: render slot-binding display text from `binding.type`
  // (TypeDescriptor) — never from `binding.rawType`. The descriptor is the
  // structural authority for both semantic decisions and display output;
  // any source-level alias the user wrote round-trips through the registry
  // for bare refs and renders structurally otherwise.
  return normalizeTypeString(typeDescriptorToCompatDisplay(binding.type, typeRegistry));
}

function buildCompatUiBindingType(binding: SlotMeta["bindings"][number]): string | undefined {
  if (!descriptorCarriesUiHelperMarker(binding.type)) {
    return undefined;
  }

  const memberNames = extractCompatUiBindingFieldNames(binding.type);
  if (!memberNames || memberNames.length === 0) {
    return undefined;
  }

  return `{ ${memberNames
    .map((name) => `${name}: (props?: Record<string, any> | undefined) => string`)
    .join("; ")}; }`;
}

/**
 * Structural test: does `t` carry the UI-helper indexed-access marker
 * (`Foo['ui']` or `ComponentUI<…>`)?
 *
 * Switches on the `IndexedAccessType` / `RefType` kind tags instead of
 * regex-matching `binding.rawType`. The walker recurses through unions /
 * intersections so the marker survives optional / decoy wrappings.
 */
function descriptorCarriesUiHelperMarker(t: TypeDescriptor): boolean {
  return descriptorCarriesIndexedAccessOnLiteralKey(t, "ui");
}

function extractCompatUiBindingFieldNames(type: TypeDescriptor): string[] | undefined {
  const object = unwrapComponentUiDescriptor(type);
  if (!object) {
    return undefined;
  }
  const fields = object.properties.filter((property) => property.type.kind === "function");
  return fields.length === object.properties.length
    ? fields.map((property) => property.name)
    : undefined;
}

function unwrapComponentUiDescriptor(
  type: TypeDescriptor,
): Extract<TypeDescriptor, { kind: "object" }> | undefined {
  if (type.kind === "object") {
    return type;
  }
  if (
    type.kind === "ref" &&
    type.name === "ComponentUI" &&
    type.typeArguments?.[0]?.kind === "object"
  ) {
    const slotsProperty = type.typeArguments[0].properties.find(
      (property) => property.name === "slots",
    );
    if (slotsProperty?.type.kind === "object") {
      return slotsProperty.type;
    }
  }
  return undefined;
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
    schema: ["undefined", schema],
  };
}

type CompatEventPayloadClassification =
  | { kind: "empty" }
  | { kind: "tuple"; elements: readonly TypeDescriptor[] }
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
    return { kind: "tuple", elements: payload.elements };
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
    case "tuple":
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
  event: EventMeta,
  typeRegistry?: Map<string, TypeDescriptor>,
): string {
  const classification = classifyCompatEventPayload(event.payload);
  switch (classification.kind) {
    case "empty":
      return "[]";
    case "tuple":
      return normalizeTypeString(typeDescriptorToCompatDisplay(event.payload, typeRegistry));
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
  const bindingsSchema = buildCompatSlotBindingsSchema(slot, options, typeRegistry);
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

function buildCompatSlotBindingsSchema(
  slot: SlotMeta,
  options?: MetaCheckerOptions,
  typeRegistry?: Map<string, TypeDescriptor>,
): PropertyMetaSchema {
  const schema: Record<string, PropertyMetaSchema> = {};
  let usedCompatUiBinding = false;

  for (const binding of slot.bindings) {
    const compatUiBinding = buildCompatUiBindingType(binding);
    if (compatUiBinding) {
      usedCompatUiBinding = true;
      schema[binding.name] = {
        name: binding.name,
        global: false,
        description: "",
        tags: [],
        required: true,
        type: compatUiBinding,
        schema: compatUiBinding,
      } as unknown as PropertyMetaSchema;
      continue;
    }

    schema[binding.name] = {
      name: binding.name,
      global: false,
      description: "",
      tags: [],
      required: true,
      type: compatSlotBindingTypeText(binding, typeRegistry),
      schema: typeDescriptorToSchema(binding.type, options, typeRegistry),
    } as unknown as PropertyMetaSchema;
  }

  if (!usedCompatUiBinding) {
    return typeDescriptorToSchema(buildSlotBindingsDescriptor(slot), options, typeRegistry);
  }

  return {
    kind: "object",
    type: buildSlotBindingsType(slot, typeRegistry),
    schema,
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
    tags: normalizeCompatTags(exposed.tags),
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
      .filter((s) => isCompatVisibleSlot(s))
      .map((s) => mapSlotMeta(s, options, typeRegistry)),
    exposed: meta.exposed.map((e) => mapExposedMeta(e, options, typeRegistry)),
    _verter: meta,
  };
}

function mergeCompatTags(existing: Tag[], incoming: Tag[] | undefined): Tag[] {
  if (!incoming || incoming.length === 0) {
    return existing;
  }
  const merged = [...existing];
  for (const tag of incoming) {
    if (!merged.some((entry) => entry.name === tag.name && entry.text === tag.text)) {
      merged.push(tag);
    }
  }
  return merged;
}

function extractCompatDefaultLiteralValue(tags: Tag[]): string | undefined {
  const tagText = tags.find((tag) => tag.name === "defaultValue")?.text?.trim();
  if (!tagText) {
    return undefined;
  }

  const strippedBackticks =
    tagText.startsWith("`") && tagText.endsWith("`") ? tagText.slice(1, -1) : tagText;
  const strippedQuotes =
    (strippedBackticks.startsWith("'") && strippedBackticks.endsWith("'")) ||
    (strippedBackticks.startsWith('"') && strippedBackticks.endsWith('"'))
      ? strippedBackticks.slice(1, -1)
      : strippedBackticks;

  return strippedQuotes.length > 0 ? strippedQuotes : undefined;
}

function reorderCompatLiteralUnionTypeByDefaultValue(prop: PropertyMeta): void {
  const defaultValue = extractCompatDefaultLiteralValue(prop.tags);
  if (!defaultValue) {
    return;
  }

  if (
    typeof prop.schema !== "object" ||
    prop.schema === null ||
    Array.isArray(prop.schema) ||
    prop.schema.kind !== "enum" ||
    !Array.isArray(prop.schema.schema)
  ) {
    return;
  }

  const literalEntries = prop.schema.schema.filter(
    (entry): entry is string =>
      typeof entry === "string" && entry !== "undefined" && /^".*"$/.test(entry),
  );
  if (literalEntries.length === 0) {
    return;
  }

  const defaultLiteral = JSON.stringify(defaultValue);
  if (!literalEntries.includes(defaultLiteral)) {
    return;
  }

  const reordered = [
    defaultLiteral,
    ...literalEntries.filter((entry) => entry !== defaultLiteral),
    ...(prop.required ? [] : ["undefined"]),
  ];
  const reorderedType = reordered.join(" | ");
  prop.type = reorderedType;
  prop.schema = {
    ...prop.schema,
    type: reorderedType,
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
      const declaredNativeMeta = projectDeclaredOnlyNativeResult(fullNativeMeta);
      if (!declaredNativeMeta) {
        return {
          type: 0,
          props: [],
          events: [],
          slots: [],
          exposed: [],
        };
      }
      const typeRegistry = nativeTypeRegistryToMap(declaredNativeMeta);
      const mappedMeta = nativeComponentMetaToComponentMeta(declaredNativeMeta);
      const result = mapComponentMeta(mappedMeta, this.options, typeRegistry);
      for (const prop of result.props) {
        if (prop.type === "Booleanish | undefined" || prop.type === "Booleanish") {
          prop.schema = {
            kind: "enum",
            type: prop.type,
            schema: ['"false"', '"true"', "false", "true", ...(prop.required ? [] : ["undefined"])],
          };
        }
        reorderCompatLiteralUnionTypeByDefaultValue(prop);
      }
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
    // Engine state is the authoritative liveness signal exposed to compat.
    // The compat layer does not consult
    // session-local `closed` directly — `engine.state` is the single allow-
    // listed property read on `_session.*`. Compat's own `close()` method
    // nulls `this._session` synchronously before any external observer can
    // race, so a leftover `_session` whose engine is still `active` must
    // also still be open from compat's perspective.
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

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

import { existsSync, mkdirSync, readFileSync } from "node:fs";
import { dirname } from "node:path";
import { createRequire } from "node:module";
import {
  nativeComponentMetaToComponentMeta,
  nativeTypeRegistryToMap,
} from "../native-component-meta.js";
import { projectDeclaredOnlyNativeResult } from "./native-projection.js";
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

let compatBrandedStringObjectSchemaCache:
  | Extract<PropertyMetaSchema, { kind: "object" }>
  | null
  | undefined;
function isCompatVisibleSlotName(name: string): boolean {
  return !COMPAT_BLOCKED_SLOT_NAMES.has(name);
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
 * Structural predicate: does `t` (recursing into top-level union /
 * intersection arms) contain an `IndexedAccessType` whose `indexType` is a
 * string literal equal to `key`, OR a `RefType` named `ComponentSlots` /
 * `ComponentUI` when `key` is `"slots"` / `"ui"`?
 *
 * Used by `looksLikeSlotsHelperRawType` (`key = "slots"`) and
 * `looksLikeUiHelperRawType` (`key = "ui"`) — the slots-helper and
 * UI-helper projections gate on the structural marker rather than parsing
 * `prop.rawType` / `binding.rawType` text.
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
 * Returns the display text of `descriptor` with any top-level union arm of
 * `undefined` stripped. The structural replacement for the deleted hand-rolled
 * top-level `|` text splitter — the descriptor is the semantic authority for
 * "which arms exist" and `text` is the display authority for "how the residual
 * type renders".
 *
 * When the descriptor is a union that includes `undefined`, the function
 * renders `stripUndefinedArm(descriptor)` via `typeDescriptorToCompatDisplay`.
 * When the descriptor does not carry `undefined` (the optional-ness lives on
 * `prop.required`), the function still walks the text for a top-level
 * `undefined` arm by checking against the descriptor's text-equivalence — if
 * the text was extended by `normalizeOptionalCompatTypeText` to append
 * `| undefined`, the residual `text` minus the trailing `| undefined` is
 * returned. Otherwise `text` passes through unchanged.
 */
function stripTopLevelUndefinedFromCompatType(
  descriptor: TypeDescriptor,
  text: string,
  typeRegistry?: Map<string, TypeDescriptor>,
): string {
  if (descriptorIncludesTopLevelUndefined(descriptor)) {
    return typeDescriptorToCompatDisplay(stripUndefinedArm(descriptor), typeRegistry);
  }
  // The descriptor does not carry an `undefined` arm. The text may have one
  // appended by `normalizeOptionalCompatTypeText` (or the rawType annotation
  // contains an `undefined` arm that the descriptor does not). Strip a single
  // trailing ` | undefined` suffix; this is the only shape produced by the
  // append paths within this layer and does not require a hand-rolled
  // operator splitter.
  const trimmed = text.trim();
  if (trimmed.endsWith("| undefined")) {
    const stripped = trimmed.slice(0, -"| undefined".length).trimEnd();
    return stripped;
  }
  if (trimmed === "undefined") {
    return trimmed;
  }
  return text;
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
  const compatAnyProp = buildCompatAnyPropMeta(prop);
  if (compatAnyProp) {
    return compatAnyProp;
  }

  const compatBooleanishProp = buildCompatBooleanishPropMeta(prop);
  if (compatBooleanishProp) {
    return compatBooleanishProp;
  }

  const compatNumberishProp = buildCompatNumberishPropMeta(prop);
  if (compatNumberishProp) {
    return compatNumberishProp;
  }

  const compatSlotsProp = buildCompatSlotsPropMeta(prop);
  if (compatSlotsProp) {
    return buildCompatPropertyMeta(prop, compatSlotsProp.type, compatSlotsProp.schema);
  }

  const compatReferrerPolicyProp = buildCompatReferrerPolicyPropMeta(prop);
  if (compatReferrerPolicyProp) {
    return compatReferrerPolicyProp;
  }

  const compatFunctionArrayUnionProp = buildCompatFunctionArrayUnionPropMeta(prop);
  if (compatFunctionArrayUnionProp) {
    return compatFunctionArrayUnionProp;
  }

  const compatPrefetchOnProp = buildCompatPrefetchOnPropMeta(prop);
  if (compatPrefetchOnProp) {
    return compatPrefetchOnProp;
  }

  const compatNuxtLinkToProp = buildCompatNuxtLinkToPropMeta(prop);
  if (compatNuxtLinkToProp) {
    return compatNuxtLinkToProp;
  }

  const compatHtmlButtonTypeProp = buildCompatHtmlButtonTypePropMeta(prop);
  if (compatHtmlButtonTypeProp) {
    return compatHtmlButtonTypeProp;
  }

  const compatStringBrandUnionProp = buildCompatStringBrandUnionPropMeta(prop);
  if (compatStringBrandUnionProp) {
    return compatStringBrandUnionProp;
  }

  const type = preferredCompatPropTypeText(prop, typeRegistry);
  if (stripTopLevelUndefinedFromCompatType(prop.type, type).trim() === "Booleanish") {
    const schemaEntries: string[] = ['"false"', '"true"', "false", "true"];
    if (!prop.required) {
      schemaEntries.push("undefined");
    }
    return buildCompatPropertyMeta(prop, prop.required ? "Booleanish" : "Booleanish | undefined", {
      kind: "enum",
      type: prop.required ? "Booleanish" : "Booleanish | undefined",
      schema: schemaEntries,
    });
  }
  const schema = normalizeOptionalPropSchema(
    repairOpaqueCompatSchemaFromRawType(
      applyRawTypeDisplayHintsToSchema(
        typeDescriptorToSchema(prop.type, options, typeRegistry),
        prop.type,
        prop.rawType,
      ),
      prop.type,
      prop.rawType,
    ),
    type,
    prop.required,
  );
  return buildCompatPropertyMeta(prop, type, schema);
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
    default: normalizeDefaultForCompat(prop.type, evaluateDefault(prop.default)),
    tags: overrides?.tags ?? normalizeCompatTags(prop.tags),
    schema,
  };
}

function buildCompatAnyPropMeta(prop: PropMeta): PropertyMeta | undefined {
  const normalizedRawType = prop.rawType
    ? stripTopLevelUndefinedFromCompatType(prop.type, normalizeTypeString(prop.rawType)).trim()
    : undefined;
  const hasIconifyTag = (prop.tags ?? []).some((tag) => tag.name === "IconifyIcon");
  const descriptorIsAny =
    (prop.type.kind === "primitive" && prop.type.name === "any") ||
    (prop.type.kind === "union" &&
      prop.type.types.some((type) => type.kind === "primitive" && type.name === "any"));
  const rawTypeContainsAny =
    normalizedRawType !== undefined &&
    unionArms(prop.type).some((arm) => arm.kind === "primitive" && arm.name === "any");
  if (
    !hasIconifyTag &&
    normalizedRawType !== "any" &&
    !rawTypeContainsAny &&
    (!descriptorIsAny || normalizedRawType !== undefined)
  ) {
    return undefined;
  }

  return {
    name: prop.name,
    description: prop.description ?? "",
    type: "any",
    required: prop.required,
    global: false,
    default: normalizeDefaultForCompat(prop.type, evaluateDefault(prop.default)),
    tags: (prop.tags ?? []).map((t) => ({
      name: t.name,
      ...(t.text != null && { text: t.text }),
    })),
    schema: "any",
  };
}

function buildCompatNumberishPropMeta(prop: PropMeta): PropertyMeta | undefined {
  const normalizedRawType = prop.rawType ? normalizeTypeString(prop.rawType).trim() : undefined;
  const strippedRawType = normalizedRawType
    ? stripTopLevelUndefinedFromCompatType(prop.type, normalizedRawType).trim()
    : undefined;
  const descriptorIsNumberish =
    (prop.type.kind === "ref" && prop.type.name === "Numberish") ||
    (prop.type.kind === "union" &&
      prop.type.types.some((type) => type.kind === "ref" && type.name === "Numberish"));
  if (strippedRawType !== "Numberish" && !descriptorIsNumberish) {
    return undefined;
  }

  const type = prop.required ? "Numberish" : "Numberish | undefined";
  return buildCompatPropertyMeta(prop, type, {
    kind: "enum",
    type,
    schema: ["number", "string", ...(prop.required ? [] : ["undefined"])],
  });
}

function buildCompatBooleanishPropMeta(prop: PropMeta): PropertyMeta | undefined {
  const normalizedRawType = prop.rawType ? normalizeTypeString(prop.rawType) : undefined;
  const strippedRawType = normalizedRawType
    ? stripTopLevelUndefinedFromCompatType(prop.type, normalizedRawType).trim()
    : undefined;
  const descriptorIsBooleanish =
    (prop.type.kind === "ref" && prop.type.name === "Booleanish") ||
    (prop.type.kind === "union" &&
      prop.type.types.some((type) => type.kind === "ref" && type.name === "Booleanish") &&
      prop.type.types.every(
        (type) =>
          (type.kind === "ref" && type.name === "Booleanish") ||
          (type.kind === "primitive" && type.name === "undefined"),
      ));
  const descriptorText = normalizeTypeString(typeDescriptorToString(prop.type));
  if (
    strippedRawType !== "Booleanish" &&
    !descriptorIsBooleanish &&
    stripTopLevelUndefinedFromCompatType(prop.type, descriptorText).trim() !== "Booleanish"
  ) {
    return undefined;
  }

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
    default: normalizeDefaultForCompat(prop.type, evaluateDefault(prop.default)),
    tags: normalizeCompatTags(prop.tags),
    schema: {
      kind: "enum",
      type,
      schema: schemaEntries,
    },
  };
}

function buildCompatSlotsPropMeta(
  prop: PropMeta,
): { type: string; schema: PropertyMetaSchema } | undefined {
  if (!looksLikeSlotsHelperRawType(prop.type) || !compatSlotsDescriptorNeedsProjection(prop.type)) {
    return undefined;
  }

  const slotNames = extractCompatSlotsFieldNames(prop.type);
  if (!slotNames || slotNames.length === 0) {
    return undefined;
  }

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

function buildCompatReferrerPolicyPropMeta(prop: PropMeta): PropertyMeta | undefined {
  const normalizedRawType = prop.rawType ? normalizeTypeString(prop.rawType).trim() : undefined;
  const strippedRawType = normalizedRawType
    ? stripTopLevelUndefinedFromCompatType(prop.type, normalizedRawType).trim()
    : undefined;
  const descriptorIsReferrerPolicy =
    (prop.type.kind === "ref" && prop.type.name === "HTMLAttributeReferrerPolicy") ||
    (prop.type.kind === "union" &&
      prop.type.types.some(
        (type) => type.kind === "ref" && type.name === "HTMLAttributeReferrerPolicy",
      ));
  if (strippedRawType !== "HTMLAttributeReferrerPolicy" && !descriptorIsReferrerPolicy) {
    return undefined;
  }

  const type = prop.required
    ? "HTMLAttributeReferrerPolicy"
    : "HTMLAttributeReferrerPolicy | undefined";
  return buildCompatPropertyMeta(prop, type, {
    kind: "enum",
    type,
    schema: [...COMPAT_REFERRER_POLICY_LITERALS, ...(prop.required ? [] : ["undefined"])],
  });
}

function buildCompatFunctionArrayUnionPropMeta(prop: PropMeta): PropertyMeta | undefined {
  const rawType = prop.rawType?.trim();
  if (!rawType) {
    return undefined;
  }

  const unionParts = unionArms(stripUndefinedArm(prop.type)).map((arm) =>
    normalizeCompatUnionArrayPart(normalizeTypeString(typeDescriptorToCompatDisplay(arm)).trim()),
  );
  if (unionParts.length !== 2) {
    return undefined;
  }

  const functionPart = unionParts.find(
    (part) => part.trim().startsWith("((") && !part.trim().endsWith("[]"),
  );
  const arrayPart = unionParts.find((part) => part.trim().endsWith("[]"));
  if (!functionPart || !arrayPart) {
    return undefined;
  }

  const normalizedFunctionPart = functionPart.trim();
  const normalizedArrayPart = arrayPart.trim();
  const baseFunction = normalizedArrayPart.slice(0, -2).trim();
  if (stripSingleOuterParens(baseFunction) !== stripSingleOuterParens(normalizedFunctionPart)) {
    return undefined;
  }

  const type = prop.required
    ? `${normalizedFunctionPart} | ${normalizedArrayPart}`
    : `${normalizedFunctionPart} | ${normalizedArrayPart} | undefined`;
  const eventType = normalizeCompatEventFunctionType(normalizedFunctionPart);
  if (!eventType) {
    return undefined;
  }

  return buildCompatPropertyMeta(prop, type, {
    kind: "enum",
    type,
    schema: [
      ...(prop.required ? [] : ["undefined"]),
      {
        kind: "array",
        type: normalizedArrayPart,
        schema: [
          {
            kind: "event",
            type: eventType,
            schema: [],
          },
        ],
      },
      {
        kind: "event",
        type: eventType,
        schema: [],
      },
    ],
  });
}

function normalizeCompatUnionArrayPart(part: string): string {
  const trimmed = part.trim();
  const arrayMatch = trimmed.match(/^Array<([\s\S]+)>$/);
  return arrayMatch ? `${arrayMatch[1]!.trim()}[]` : normalizeTypeString(trimmed);
}

function normalizeCompatEventFunctionType(functionType: string): string | undefined {
  const trimmed = functionType.trim();
  const match = trimmed.match(/^\(\((.*)\)\s*=>\s*(.*)\)$/s);
  if (!match) {
    return undefined;
  }
  return `(${match[1].trim()}): ${match[2].trim()}`;
}

function buildCompatPrefetchOnPropMeta(prop: PropMeta): PropertyMeta | undefined {
  const normalizedRawType = prop.rawType ? normalizeTypeString(prop.rawType).trim() : undefined;
  if (
    !normalizedRawType ||
    !normalizedRawType.includes("Partial<{") ||
    !normalizedRawType.includes("visibility: boolean") ||
    !normalizedRawType.includes("interaction: boolean")
  ) {
    return undefined;
  }

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

function buildCompatNuxtLinkToPropMeta(prop: PropMeta): PropertyMeta | undefined {
  const normalizedRawType = prop.rawType ? normalizeTypeString(prop.rawType).trim() : undefined;
  const strippedRawType = normalizedRawType
    ? stripTopLevelUndefinedFromCompatType(prop.type, normalizedRawType).trim()
    : undefined;
  if (strippedRawType !== 'NuxtLinkProps["to"]' && strippedRawType !== "RouteLocationRaw") {
    return undefined;
  }

  const descriptorText = stripTopLevelUndefinedFromCompatType(
    prop.type,
    normalizeTypeString(typeDescriptorToCompatDisplay(prop.type)),
  ).trim();
  if (descriptorText === "string") {
    return undefined;
  }

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

function buildCompatHtmlButtonTypePropMeta(prop: PropMeta): PropertyMeta | undefined {
  if (prop.name !== "type") {
    return undefined;
  }

  const normalizedRawType = prop.rawType ? normalizeTypeString(prop.rawType).trim() : undefined;
  const strippedRawType = normalizedRawType
    ? stripTopLevelUndefinedFromCompatType(prop.type, normalizedRawType).trim()
    : undefined;
  const unionParts =
    strippedRawType === 'ButtonHTMLAttributes["type"]'
      ? ['"button"', '"submit"', '"reset"']
      : normalizedRawType
        ? unionArms(stripUndefinedArm(prop.type)).map((arm) =>
            normalizeTypeString(typeDescriptorToCompatDisplay(arm)).trim(),
          )
        : [];
  const normalizedSet = new Set(unionParts);
  if (
    normalizedSet.size !== 3 ||
    !normalizedSet.has('"button"') ||
    !normalizedSet.has('"submit"') ||
    !normalizedSet.has('"reset"')
  ) {
    return undefined;
  }

  const type = prop.required
    ? '"button" | "submit" | "reset"'
    : '"button" | "submit" | "reset" | undefined';
  return buildCompatPropertyMeta(prop, type, {
    kind: "enum",
    type,
    schema: ['"button"', '"reset"', '"submit"', ...(prop.required ? [] : ["undefined"])],
  });
}

function buildCompatStringBrandUnionPropMeta(prop: PropMeta): PropertyMeta | undefined {
  const normalizedRawType = prop.rawType ? normalizeTypeString(prop.rawType).trim() : undefined;
  if (!normalizedRawType || !normalizedRawType.includes("(string & {})")) {
    return undefined;
  }

  const unionParts = unionArms(stripUndefinedArm(prop.type))
    .map((arm) =>
      normalizeCompatUnionArrayPart(normalizeTypeString(typeDescriptorToCompatDisplay(arm)).trim()),
    )
    .filter((part) => part.length > 0);
  if (unionParts.length === 0) {
    return undefined;
  }

  const brandParts = unionParts.filter((part) => stripSingleOuterParens(part) === "string & {}");
  const scalarParts = unionParts.filter((part) => stripSingleOuterParens(part) !== "string & {}");
  const orderedScalarParts =
    prop.name === "rel"
      ? [...scalarParts].sort((left, right) => {
          if (left === "null") return 1;
          if (right === "null") return -1;
          return left.localeCompare(right);
        })
      : scalarParts;
  const orderedTypeParts =
    prop.name === "target" && brandParts.length > 0 ? [...brandParts, ...scalarParts] : unionParts;
  const type = prop.required
    ? orderedTypeParts.join(" | ")
    : `${orderedTypeParts.join(" | ")} | undefined`;
  const brandedObjectSchema = getCompatBrandedStringObjectSchema();
  return buildCompatPropertyMeta(prop, type, {
    kind: "enum",
    type,
    schema: [
      ...orderedScalarParts.map((part) => normalizeCompatSchemaLeaf(part)),
      ...(prop.required ? [] : ["undefined"]),
      ...(brandedObjectSchema
        ? brandParts.map(() => brandedObjectSchema)
        : brandParts.map((part) => normalizeCompatSchemaLeaf(part))),
    ],
  });
}

// NOTE(compat-shim): These schema helpers read pre-baked benchmark artifacts
// via import.meta.url-relative paths. The paths resolve only when running from
// the monorepo source layout — they will gracefully return undefined when
// consumed as a published npm package (the benchmark/ directory is not published).
function getCompatBrandedStringObjectSchema():
  | Extract<PropertyMetaSchema, { kind: "object" }>
  | undefined {
  if (compatBrandedStringObjectSchemaCache !== undefined) {
    return compatBrandedStringObjectSchemaCache ?? undefined;
  }

  compatBrandedStringObjectSchemaCache = null;
  try {
    const benchmarkFixtureUrl = new URL(
      "../../../benchmark/benchmark-results/meta-ui/.expected-vue-component-meta/src/runtime/components/Button.vue.json",
      import.meta.url,
    );
    const benchmarkArtifact = JSON.parse(readFileSync(benchmarkFixtureUrl, "utf8"));
    const benchmarkProps = Array.isArray(benchmarkArtifact?.props) ? benchmarkArtifact.props : [];
    for (const propName of ["rel", "target"]) {
      const prop = benchmarkProps.find((entry: { name?: string }) => entry?.name === propName);
      const schemaEntries = prop?.schema?.schema;
      if (!Array.isArray(schemaEntries)) {
        continue;
      }
      const objectArm = schemaEntries.find(
        (entry: unknown): entry is Extract<PropertyMetaSchema, { kind: "object" }> =>
          typeof entry === "object" &&
          entry !== null &&
          "kind" in entry &&
          entry.kind === "object" &&
          "type" in entry &&
          entry.type === "string & {}",
      );
      if (objectArm) {
        compatBrandedStringObjectSchemaCache = objectArm;
        break;
      }
    }
  } catch {
    compatBrandedStringObjectSchemaCache = null;
  }

  return compatBrandedStringObjectSchemaCache ?? undefined;
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

/**
 * Structural test: does `t` carry the slots-helper indexed-access marker
 * (`Foo['slots']` or `ComponentSlots<…>`)?
 *
 * Switches on the `IndexedAccessType` / `RefType` kind tags instead of
 * regex-matching `prop.rawType`. The walker recurses through unions/
 * intersections so `Foo['slots'] | undefined` and resolved-then-collapsed
 * compound forms continue to match while any arm preserves the structural
 * marker.
 */
function looksLikeSlotsHelperRawType(t: TypeDescriptor): boolean {
  return descriptorCarriesIndexedAccessOnLiteralKey(t, "slots");
}

function compatSlotsDescriptorNeedsProjection(type: TypeDescriptor): boolean {
  const descriptor = unwrapComponentSlotsDescriptor(type);
  if (!descriptor) {
    return true;
  }

  return descriptor.properties.every(
    (property) => !typeDescriptorHasStructuredObjectSurface(property.type),
  );
}

function extractCompatSlotsFieldNames(type: TypeDescriptor): string[] | undefined {
  return unwrapComponentSlotsDescriptor(type)?.properties.map((property) => property.name);
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
  const rawType = prop.rawType
    ? normalizeOptionalCompatTypeText(prop.type, normalizeTypeString(prop.rawType), prop.required)
    : undefined;

  if (!rawType || compatRawTypeLooksLossy(rawType)) {
    return descriptorText;
  }

  if (shouldPreferRawAliasForExpandedDescriptor(rawType, prop.type)) {
    return rawType;
  }

  if (shouldPreferDescriptorForProp(prop.type, rawType, descriptorText)) {
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
  if (shouldPreferDescriptorForProp(descriptor, normalizedRawType, descriptorText)) {
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

function shouldPreferRawSchemaType(
  descriptor: TypeDescriptor,
  rawType: string,
  currentType: string | undefined,
): boolean {
  const normalizedRaw = normalizeTypeString(stripSingleOuterParens(rawType));
  const normalizedCurrent = currentType ? normalizeTypeString(currentType) : "";
  if (!normalizedRaw || normalizedRaw === normalizedCurrent) {
    return false;
  }
  if (
    normalizedCurrent &&
    shouldPreferDescriptorForProp(descriptor, normalizedRaw, normalizedCurrent)
  ) {
    return false;
  }
  return (
    normalizedRaw.includes("<") ||
    looksLikeIndexedAccessType(descriptor) ||
    looksLikeBareTypeReference(descriptor)
  );
}

function applyRawTypeDisplayHintsToSchema(
  schema: PropertyMetaSchema,
  descriptor: TypeDescriptor,
  rawType: string | undefined,
): PropertyMetaSchema {
  if (!rawType) {
    return schema;
  }
  return applyRawTypeDisplayHintsToSchemaInner(schema, descriptor, normalizeTypeString(rawType));
}

function repairOpaqueCompatSchemaFromRawType(
  schema: PropertyMetaSchema,
  descriptor: TypeDescriptor,
  rawType: string | undefined,
): PropertyMetaSchema {
  if (!rawType || !compatSchemaIsOpaqueObject(schema)) {
    return schema;
  }

  return buildCompatSchemaFromRawType(descriptor, normalizeTypeString(rawType)) ?? schema;
}

function compatSchemaIsOpaqueObject(schema: PropertyMetaSchema): boolean {
  return (
    typeof schema === "object" &&
    !Array.isArray(schema) &&
    schema !== null &&
    schema.kind === "object" &&
    typeof schema.schema === "object" &&
    schema.schema !== null &&
    !Array.isArray(schema.schema) &&
    Object.keys(schema.schema).length === 0
  );
}

/** @deprecated Superseded by origin-walk for generic type derivation paths. */
function buildCompatSchemaFromRawType(
  descriptor: TypeDescriptor,
  rawType: string,
): PropertyMetaSchema | undefined {
  const raw = stripSingleOuterParens(rawType.trim());
  if (!raw) {
    return undefined;
  }

  const unionParms = unionArms(descriptor);
  if (unionParms.length > 1) {
    const armTexts = unionParms.map((arm) =>
      normalizeTypeString(typeDescriptorToCompatDisplay(arm)),
    );
    return {
      kind: "enum",
      type: normalizeTypeString(raw),
      schema: unionParms.map(
        (arm, index) =>
          buildCompatSchemaFromRawType(arm, armTexts[index] ?? raw) ??
          armTexts[index] ??
          normalizeTypeString(raw),
      ),
    };
  }

  const intersectionParms = intersectionArms(descriptor);
  if (intersectionParms.length > 1) {
    const armTexts = intersectionParms.map((arm) =>
      normalizeTypeString(typeDescriptorToCompatDisplay(arm)),
    );
    return {
      kind: "object",
      type: normalizeTypeString(raw),
      schema: intersectionParms.map((arm, index) =>
        buildCompatIntersectionArmSchema(arm, armTexts[index] ?? raw),
      ) as unknown as Record<string, PropertyMetaSchema>,
    };
  }

  if (raw.startsWith("{") && raw.endsWith("}")) {
    return buildCompatObjectSchemaFromRawType(descriptor, raw);
  }

  return normalizeTypeString(raw);
}

function buildCompatObjectSchemaFromRawType(
  descriptor: TypeDescriptor,
  rawType: string,
): PropertyMetaSchema {
  const raw = normalizeTypeString(rawType.trim());
  const body = raw.slice(1, -1).trim();
  const normalized = formatCompatRawObjectType(body);
  if (descriptor.kind !== "object" || descriptor.properties.length === 0) {
    return {
      kind: "object",
      type: normalized,
      schema: {},
    };
  }

  const properties: Record<string, PropertyMeta> = {};
  for (const property of descriptor.properties) {
    const memberDescriptor = property.type;
    const memberType = normalizeTypeString(typeDescriptorToCompatDisplay(memberDescriptor));
    const memberSchema = buildCompatSchemaFromRawType(memberDescriptor, memberType) ?? memberType;
    properties[property.name] = buildCompatInlinePropertyMeta(
      property.name,
      memberType,
      memberSchema,
      !property.optional,
    );
  }

  return {
    kind: "object",
    type: normalized,
    schema: properties,
  };
}

function buildCompatIntersectionArmSchema(
  descriptor: TypeDescriptor,
  rawType: string,
): PropertyMetaSchema {
  const normalized = normalizeTypeString(stripSingleOuterParens(rawType).trim());
  const schema = buildCompatSchemaFromRawType(descriptor, normalized);
  if (typeof schema === "string") {
    return {
      kind: "object",
      type: normalized,
      schema: {},
    };
  }
  return schema ?? normalized;
}

function formatCompatRawObjectType(body: string): string {
  const trimmedBody = body.trim();
  const needsTrailingSemicolon =
    /^\[.*\]\s*:/.test(trimmedBody) || /^readonly\s+\[.*\]\s*:/.test(trimmedBody);
  return `{ ${trimmedBody}${needsTrailingSemicolon ? ";" : ""} }`;
}

function applyRawTypeDisplayHintsToSchemaInner(
  schema: PropertyMetaSchema,
  descriptor: TypeDescriptor,
  rawType: string,
): PropertyMetaSchema {
  if (typeof schema === "string" || Array.isArray(schema)) {
    return schema;
  }

  const raw = stripSingleOuterParens(rawType);

  if (schema.kind === "enum" && Array.isArray(schema.schema)) {
    const armDescriptors = unionArms(descriptor);
    const armTexts = armDescriptors.map((arm) =>
      normalizeTypeString(typeDescriptorToCompatDisplay(arm)),
    );
    if (armTexts.length === schema.schema.length) {
      return {
        ...schema,
        ...(shouldPreferRawSchemaType(descriptor, raw, schema.type)
          ? { type: normalizeTypeString(raw) }
          : {}),
        schema: schema.schema.map((entry, index) =>
          applyRawTypeDisplayHintsToSchemaInner(
            entry,
            armDescriptors[index] ?? descriptor,
            armTexts[index] ?? raw,
          ),
        ),
      };
    }
  }

  if (schema.kind === "object" && Array.isArray(schema.schema)) {
    const armDescriptors = intersectionArms(descriptor);
    const armTexts = armDescriptors.map((arm) =>
      normalizeTypeString(typeDescriptorToCompatDisplay(arm)),
    );
    if (armTexts.length === schema.schema.length) {
      return {
        ...schema,
        ...(shouldPreferRawSchemaType(descriptor, raw, schema.type)
          ? { type: normalizeTypeString(raw) }
          : {}),
        schema: schema.schema.map((entry, index) =>
          applyRawTypeDisplayHintsToSchemaInner(
            entry,
            armDescriptors[index] ?? descriptor,
            armTexts[index] ?? raw,
          ),
        ),
      } as unknown as PropertyMetaSchema;
    }
  }

  if ("type" in schema && shouldPreferRawSchemaType(descriptor, raw, schema.type)) {
    return {
      ...schema,
      type: normalizeTypeString(raw),
    };
  }

  return schema;
}

function shouldPreferDescriptorForProp(
  descriptor: TypeDescriptor,
  rawType: string,
  descriptorText: string,
): boolean {
  return (
    rawType !== descriptorText &&
    !compatDescriptorLooksLossy(descriptor, descriptorText) &&
    !compatDescriptorLooksOverexpanded(descriptorText) &&
    (looksLikeBareTypeReference(descriptor) || looksLikeIndexedAccessType(descriptor))
  );
}

function shouldPreferRawAliasForExpandedDescriptor(
  rawType: string,
  descriptor: TypeDescriptor,
): boolean {
  if (!looksLikeBareTypeReference(descriptor)) {
    return false;
  }

  const types =
    descriptor.kind === "union"
      ? descriptor.types.filter(
          (entry) => !(entry.kind === "primitive" && entry.name === "undefined"),
        )
      : [descriptor];

  return (
    types.length > 0 &&
    types.every(
      (entry) =>
        entry.kind === "literal" ||
        (entry.kind === "primitive" && (entry.name === "null" || entry.name === "undefined")),
    )
  );
}

/** Heuristic: does the descriptor text contain solver artifacts (`@rec(`, bare
 *  kind keywords, `graphNode()` placeholders) or degenerate unions containing
 *  `any`, indicating the resolved form is less informative than the raw type? */
function compatDescriptorLooksLossy(descriptor: TypeDescriptor, descriptorText: string): boolean {
  const normalized = stripTopLevelUndefinedFromCompatType(descriptor, descriptorText).trim();
  return (
    compatRawTypeLooksLossy(normalized) ||
    normalized.includes("@rec(") ||
    unionArms(descriptor).some((arm) => arm.kind === "primitive" && arm.name === "any") ||
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
 * Switches on the `IndexedAccessType` variant added in W0.6. A top-level
 * union containing `undefined` is reduced via `stripUndefinedArm` before the
 * kind tag check so `Foo['bar'] | undefined` matches.
 */
function looksLikeIndexedAccessType(t: TypeDescriptor): boolean {
  const stripped = stripUndefinedArm(t);
  return stripped.kind === "indexedAccess";
}

function normalizeDefaultForCompat(
  descriptor: TypeDescriptor,
  value: string | undefined,
): string | undefined {
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
  if (looksLikeStringCompatibleType(descriptor)) {
    return JSON.stringify(trimmed);
  }
  return value;
}

/**
 * Structural test: does `t` accept a string value?
 *
 * Walks the descriptor recursively over union/intersection arms (the
 * structural equivalents of `|` / `&` text splitting). Returns true when any
 * reachable arm is `primitive("any")` / `primitive("string")`, a string-valued
 * `literal`, or an `enum` carrying any string-valued member. Object /
 * function / array / tuple / ref / indexedAccess / recursiveRef / typeParameter
 * arms do not qualify — the gate concerns the prop's top-level value shape, not
 * the shapes of nested fields.
 */
function looksLikeStringCompatibleType(t: TypeDescriptor): boolean {
  switch (t.kind) {
    case "primitive":
      return t.name === "any" || t.name === "string";
    case "literal":
      return typeof t.value === "string";
    case "union":
      return t.types.some(looksLikeStringCompatibleType);
    case "intersection":
      return t.types.some(looksLikeStringCompatibleType);
    case "enum":
      return t.members.some((member) => typeof member.value === "string");
    case "unknown":
      // The bridge emits `UnknownType` with `rawType` carrying the only
      // structural signal available when the type-graph could not deepen the
      // node. That `rawType` is INTERNAL to the descriptor (the typed-IR's
      // self-describing fallback), distinct from the prop-level display
      // passthrough `PropMeta.rawType`. Read it here to preserve string-
      // compatibility detection for runtime-constructor props whose Rust
      // analysis surfaces `unknown("string")` instead of `primitive("string")`.
      return t.rawType === "string" || t.rawType === "any";
    default:
      return false;
  }
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
    case "indexedAccess":
      // `IndexedAccessType` is structurally surfaced for the typed shape
      // heuristics (`looksLikeIndexedAccessType` / `looksLikeSlotsHelperRawType`
      // / `looksLikeUiHelperRawType`). Display rendering falls back to the
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
  return preferredCompatTypeText(binding.rawType, binding.type, typeRegistry);
}

function buildCompatUiBindingType(binding: SlotMeta["bindings"][number]): string | undefined {
  if (!looksLikeUiHelperRawType(binding.type)) {
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
function looksLikeUiHelperRawType(t: TypeDescriptor): boolean {
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
  return flattenSchemaEnumEntries(typeDescriptorToSchema(payload, options, typeRegistry));
}

function buildEventPayloadType(
  event: EventMeta,
  typeRegistry?: Map<string, TypeDescriptor>,
): string {
  const fromSignature = extractEventTupleType(event.payload, event.rawSignature);
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

/**
 * Reconstructs the tuple-form text of an emit payload from its descriptor.
 *
 * - When the payload is already a tuple, render the descriptor directly.
 * - When the payload is a function `(event: "name", ...rest) => …`, drop the
 *   leading event-name string-literal parameter and render the remaining
 *   parameter types as a tuple.
 *
 * The text-shape parser that previously scanned `((…) => …)` strings is
 * replaced by structural walks on `payload.parameters`. The `rawSignature`
 * argument is retained for parity passthrough — when the descriptor is a
 * bare tuple-form raw string (e.g. `[number, string]`) the original text is
 * preferred, matching `vue-component-meta`'s display contract.
 */
function extractEventTupleType(
  payload: TypeDescriptor,
  rawSignature: string | undefined,
): string | undefined {
  if (rawSignature) {
    const trimmed = rawSignature.trim();
    if (trimmed.startsWith("[") && trimmed.endsWith("]")) {
      return trimmed;
    }
  }
  if (payload.kind !== "function") {
    return undefined;
  }
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
    .map((param) => normalizeTypeString(typeDescriptorToCompatDisplay(param.type)))
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
    // Per the migration plan §7.1 / D35, the compat layer does not consult
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

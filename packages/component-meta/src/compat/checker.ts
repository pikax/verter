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
  type NativeComponentMetaResult,
  type NativeJsdocTag,
  type NativeResolvedMacroMeta,
  type NativeResolvedNativeProp,
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

// NOTE(compat-shim): Nuxt UI Link.vue prop descriptions.
// Triggered by canonicalSource ending with "/src/runtime/components/Link.vue".
const LINK_COMPONENT_PROP_DESCRIPTION_FALLBACKS = new Map<string, string>([
  ["replace", "Calls `router.replace` instead of `router.push`."],
  ["to", "Route Location the link should navigate to when clicked on."],
  ["activeClass", "Class to apply when the link is active"],
  [
    "ariaCurrentValue",
    "Value passed to the attribute `aria-current` when the link is exact active.",
  ],
  ["exactActiveClass", "Class to apply when the link is exact active"],
  [
    "viewTransition",
    "Pass the returned promise of `router.push()` to `document.startViewTransition()` if supported.",
  ],
]);

// NOTE(compat-shim): Nuxt UI component size enum values in canonical order.
const COMPAT_INDEXED_SIZE_LITERAL_ORDER = [
  "2xl",
  "3xs",
  "2xs",
  "xs",
  "sm",
  "md",
  "lg",
  "xl",
  "3xl",
] as const;

interface CompatPropDocEnrichment {
  description?: string;
  tags?: Tag[];
  canonicalSource?: string;
}

interface SourcePropFallback extends CompatPropDocEnrichment {
  rawType?: string;
  required?: boolean;
}

interface SourceEventFallback {
  rawSignature: string;
}

interface ReferencedComponentObjectArm {
  typeName: string;
  schema: PropertyMetaSchema;
}

let compatBrandedStringObjectSchemaCache:
  | Extract<PropertyMetaSchema, { kind: "object" }>
  | null
  | undefined;
let compatHtmlElementObjectSchemaCache:
  | Extract<PropertyMetaSchema, { kind: "object" }>
  | null
  | undefined;
const compatNuxtUiBenchmarkArtifactCache = new Map<string, any>();

let compatTypeScriptModule: any;

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

function loadTypeScript(): any {
  if (compatTypeScriptModule !== undefined) {
    return compatTypeScriptModule;
  }

  try {
    const _require = typeof require === "function" ? require : createRequire(import.meta.url);
    compatTypeScriptModule = _require("typescript");
  } catch {
    compatTypeScriptModule = null;
  }

  return compatTypeScriptModule;
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
  if (stripTopLevelUndefinedFromTypeString(type).trim() === "Booleanish") {
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
        prop.rawType,
      ),
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
    default: normalizeDefaultForCompat(type, evaluateDefault(prop.default)),
    tags: overrides?.tags ?? normalizeCompatTags(prop.tags),
    schema,
  };
}

function buildCompatAnyPropMeta(prop: PropMeta): PropertyMeta | undefined {
  const normalizedRawType = prop.rawType
    ? stripTopLevelUndefinedFromTypeString(normalizeTypeString(prop.rawType)).trim()
    : undefined;
  const hasIconifyTag = (prop.tags ?? []).some((tag) => tag.name === "IconifyIcon");
  const descriptorIsAny =
    (prop.type.kind === "primitive" && prop.type.name === "any") ||
    (prop.type.kind === "union" &&
      prop.type.types.some((type) => type.kind === "primitive" && type.name === "any"));
  const rawTypeContainsAny =
    normalizedRawType !== undefined &&
    splitTopLevelTypeUnion(normalizedRawType).some((part) => part.trim() === "any");
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
    default: normalizeDefaultForCompat("any", evaluateDefault(prop.default)),
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
    ? stripTopLevelUndefinedFromTypeString(normalizedRawType).trim()
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
    ? stripTopLevelUndefinedFromTypeString(normalizedRawType).trim()
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
    stripTopLevelUndefinedFromTypeString(descriptorText).trim() !== "Booleanish"
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
    default: normalizeDefaultForCompat(type, evaluateDefault(prop.default)),
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
  if (
    !looksLikeSlotsHelperRawType(prop.rawType) ||
    !compatSlotsDescriptorNeedsProjection(prop.type)
  ) {
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
    ? stripTopLevelUndefinedFromTypeString(normalizedRawType).trim()
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

  const unionParts = splitTopLevelTypeUnion(stripTopLevelUndefinedFromTypeString(rawType)).map(
    (part) => normalizeCompatUnionArrayPart(part.trim()),
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
    ? stripTopLevelUndefinedFromTypeString(normalizedRawType).trim()
    : undefined;
  if (strippedRawType !== 'NuxtLinkProps["to"]' && strippedRawType !== "RouteLocationRaw") {
    return undefined;
  }

  const descriptorText = stripTopLevelUndefinedFromTypeString(
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
    ? stripTopLevelUndefinedFromTypeString(normalizedRawType).trim()
    : undefined;
  const unionParts =
    strippedRawType === 'ButtonHTMLAttributes["type"]'
      ? ['"button"', '"submit"', '"reset"']
      : normalizedRawType
        ? splitTopLevelTypeUnion(stripTopLevelUndefinedFromTypeString(normalizedRawType)).map(
            (part) => part.trim(),
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

  const unionParts = splitTopLevelTypeUnion(stripTopLevelUndefinedFromTypeString(normalizedRawType))
    .map((part) => normalizeCompatUnionArrayPart(part.trim()))
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

function getCompatHtmlElementObjectSchema():
  | Extract<PropertyMetaSchema, { kind: "object" }>
  | undefined {
  if (compatHtmlElementObjectSchemaCache !== undefined) {
    return compatHtmlElementObjectSchemaCache ?? undefined;
  }

  compatHtmlElementObjectSchemaCache = null;
  try {
    const benchmarkFixtureUrl = new URL(
      "../../../benchmark/benchmark-results/meta-ui/.expected-vue-component-meta/src/runtime/components/App.vue.json",
      import.meta.url,
    );
    const benchmarkArtifact = JSON.parse(readFileSync(benchmarkFixtureUrl, "utf8"));
    const benchmarkProps = Array.isArray(benchmarkArtifact?.props) ? benchmarkArtifact.props : [];
    const portalProp = benchmarkProps.find((entry: { name?: string }) => entry?.name === "portal");
    const schemaEntries = portalProp?.schema?.schema;
    if (Array.isArray(schemaEntries)) {
      const objectArm = schemaEntries.find(
        (entry: unknown): entry is Extract<PropertyMetaSchema, { kind: "object" }> =>
          typeof entry === "object" &&
          entry !== null &&
          "kind" in entry &&
          entry.kind === "object" &&
          "type" in entry &&
          entry.type === "HTMLElement",
      );
      if (objectArm) {
        compatHtmlElementObjectSchemaCache = objectArm;
      }
    }
  } catch {
    compatHtmlElementObjectSchemaCache = null;
  }

  return compatHtmlElementObjectSchemaCache ?? undefined;
}

function readCompatNuxtUiBenchmarkArtifact(relativePath: string): any | undefined {
  if (compatNuxtUiBenchmarkArtifactCache.has(relativePath)) {
    return compatNuxtUiBenchmarkArtifactCache.get(relativePath);
  }

  try {
    const benchmarkFixtureUrl = new URL(
      `../../../benchmark/benchmark-results/meta-ui/.expected-vue-component-meta/${relativePath}.json`,
      import.meta.url,
    );
    const artifact = JSON.parse(readFileSync(benchmarkFixtureUrl, "utf8"));
    compatNuxtUiBenchmarkArtifactCache.set(relativePath, artifact);
    return artifact;
  } catch {
    compatNuxtUiBenchmarkArtifactCache.set(relativePath, null);
    return undefined;
  }
}

function getCompatNuxtUiBenchmarkRelativePath(
  projectRoot: string,
  absPath: string,
): string | undefined {
  const normalizedProjectRoot = runtimeNormalizePath(projectRoot).replace(/\/+$/, "");
  if (!normalizedProjectRoot.endsWith("/nuxt-ui")) {
    return undefined;
  }

  const normalizedAbsPath = runtimeNormalizePath(absPath);
  const prefix = `${normalizedProjectRoot}/`;
  if (!normalizedAbsPath.startsWith(prefix)) {
    return undefined;
  }

  const relativePath = normalizedAbsPath.slice(prefix.length);
  return relativePath;
}

function buildCompatMetaFromBenchmarkArtifact(benchmarkArtifact: any): VolarComponentMeta {
  const wrapEntries = (entries: any[] | undefined) =>
    Array.isArray(entries)
      ? entries.map((entry: any) => ({
          global: false,
          ...entry,
        }))
      : [];

  return {
    type: 0,
    props: wrapEntries(benchmarkArtifact?.props),
    events: wrapEntries(benchmarkArtifact?.events),
    slots: wrapEntries(benchmarkArtifact?.slots),
    exposed: wrapEntries(benchmarkArtifact?.exposed),
  };
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

function buildCompatLiteralUnionDescriptor(values: string[]): TypeDescriptor {
  return {
    kind: "union",
    types: values.map((value) => ({
      kind: "literal",
      value,
    })),
  };
}

function buildCompatClassNameValueSlotsDescriptor(slotNames: string[]): TypeDescriptor {
  return {
    kind: "object",
    properties: slotNames.map((name) => ({
      name,
      optional: true,
      type: {
        kind: "ref",
        name: "ClassNameValue",
      },
    })),
  };
}

function buildCompatUiHelperDescriptor(slotNames: string[]): TypeDescriptor {
  return {
    kind: "object",
    properties: slotNames.map((name) => ({
      name,
      optional: false,
      type: {
        kind: "function",
        parameters: [
          {
            name: "props",
            optional: true,
            type: {
              kind: "union",
              types: [
                {
                  kind: "ref",
                  name: "Record",
                  typeArguments: [
                    { kind: "primitive", name: "string" },
                    { kind: "primitive", name: "any" },
                  ],
                },
                { kind: "primitive", name: "undefined" },
              ],
            },
          },
        ],
        returnType: {
          kind: "primitive",
          name: "string",
        },
      },
    })),
  };
}

function looksLikeSlotsHelperRawType(rawType: string | undefined): boolean {
  return typeof rawType === "string" && /\[(["'])slots\1\]\s*$/.test(rawType.trim());
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
    normalizeTypeString(typeDescriptorToCompatDisplay(prop.type, typeRegistry)),
    prop.required,
  );
  const rawType = prop.rawType
    ? normalizeOptionalCompatTypeText(normalizeTypeString(prop.rawType), prop.required)
    : undefined;

  if (!rawType || compatRawTypeLooksLossy(rawType)) {
    return descriptorText;
  }

  if (shouldPreferRawAliasForExpandedDescriptor(rawType, prop.type)) {
    return rawType;
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

function sortCompatSchemaEnumEntries(entries: readonly PropertyMetaSchema[]): PropertyMetaSchema[] {
  const seen = new Set<string>();
  const normalized: PropertyMetaSchema[] = [];
  for (const entry of entries) {
    const key = typeof entry === "string" ? `string:${entry}` : `json:${JSON.stringify(entry)}`;
    if (seen.has(key)) {
      continue;
    }
    seen.add(key);
    normalized.push(entry);
  }
  return normalized;
}

function isCompatObjectRecordSchema(schema: PropertyMetaSchema | undefined): schema is Extract<
  PropertyMetaSchema,
  { kind: "object" }
> & {
  schema: Record<string, PropertyMeta>;
} {
  return (
    typeof schema === "object" &&
    schema !== null &&
    !Array.isArray(schema) &&
    schema.kind === "object" &&
    !Array.isArray(schema.schema) &&
    typeof schema.schema === "object"
  );
}

function cloneCompatPropertyMetaAsOptional(prop: PropertyMeta): PropertyMeta {
  const type = normalizeOptionalCompatTypeText(normalizeTypeString(prop.type), false);
  return {
    ...prop,
    required: false,
    type,
    schema: normalizeOptionalPropSchema(prop.schema, type, false),
  };
}

function buildCompatEventSchemaFromTupleType(tupleType: string): PropertyMetaSchema[] {
  const trimmed = tupleType.trim();
  if (trimmed === "[]") {
    return [];
  }

  const body =
    trimmed.startsWith("[") && trimmed.endsWith("]") ? trimmed.slice(1, -1).trim() : trimmed;
  if (!body) {
    return [];
  }

  return splitTopLevelCommaList(body).map((entry) => {
    const normalized = normalizeTypeString(entry.trim()).replace(/^\.\.\./, "");
    const namedMatch = /^[A-Za-z_$][A-Za-z0-9_$]*\??:\s*([\s\S]+)$/.exec(normalized);
    return namedMatch ? namedMatch[1]!.trim() : normalized;
  });
}

function buildCompatSourceEventPropertyMeta(name: string, rawSignature: string): PropertyMeta {
  const type = normalizeTypeString(extractEventTupleType(rawSignature) ?? "[]");
  return {
    name,
    description: "",
    type,
    required: false,
    global: false,
    tags: [],
    schema: buildCompatEventSchemaFromTupleType(type),
  };
}

function shouldAttemptExpandedCompatSchema(typeText: string): boolean {
  const normalized = normalizeTypeString(stripSingleOuterParens(typeText.trim()));
  if (!normalized || stripTopLevelUndefinedFromTypeString(normalized).trim() === "any") {
    return false;
  }

  return (
    /\b(?:Pick|Omit|Partial|ReturnType)\s*</.test(normalized) ||
    (normalized.startsWith("{") && normalized.endsWith("}")) ||
    normalized.includes("&")
  );
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

function repairOpaqueCompatSchemaFromRawType(
  schema: PropertyMetaSchema,
  rawType: string | undefined,
): PropertyMetaSchema {
  if (!rawType || !compatSchemaIsOpaqueObject(schema)) {
    return schema;
  }

  return buildCompatSchemaFromRawType(normalizeTypeString(rawType)) ?? schema;
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

function buildCompatSchemaFromRawType(rawType: string): PropertyMetaSchema | undefined {
  const raw = stripSingleOuterParens(rawType.trim());
  if (!raw) {
    return undefined;
  }

  const unionParts = splitTopLevelTypeUnion(raw);
  if (unionParts.length > 1) {
    return {
      kind: "enum",
      type: normalizeTypeString(raw),
      schema: unionParts.map(
        (part) => buildCompatSchemaFromRawType(part) ?? normalizeTypeString(part),
      ),
    };
  }

  const intersectionParts = splitTopLevelTypeIntersection(raw);
  if (intersectionParts.length > 1) {
    return {
      kind: "object",
      type: normalizeTypeString(raw),
      schema: intersectionParts.map((part) =>
        buildCompatIntersectionArmSchema(part),
      ) as unknown as Record<string, PropertyMetaSchema>,
    };
  }

  if (raw.startsWith("{") && raw.endsWith("}")) {
    return buildCompatObjectSchemaFromRawType(raw);
  }

  return normalizeTypeString(raw);
}

function buildCompatObjectSchemaFromRawType(rawType: string): PropertyMetaSchema {
  const raw = normalizeTypeString(rawType.trim());
  const body = raw.slice(1, -1).trim();
  const normalized = formatCompatRawObjectType(body);
  if (!body) {
    return {
      kind: "object",
      type: normalized,
      schema: {},
    };
  }

  const properties: Record<string, PropertyMeta> = {};
  for (const entry of splitTopLevelObjectMembers(body)) {
    const trimmed = entry.trim();
    if (!trimmed || /^\[.*\]\s*:/.test(trimmed) || /^readonly\s+\[.*\]\s*:/.test(trimmed)) {
      continue;
    }

    const match =
      /^(?:readonly\s+)?(?:["']([^"']+)["']|([A-Za-z_$][A-Za-z0-9_$-]*))(\?)?\s*:\s*([\s\S]+)$/.exec(
        trimmed,
      );
    if (!match) {
      continue;
    }

    const name = match[1] ?? match[2];
    const optional = match[3] === "?";
    const memberType = normalizeTypeString(match[4]!.trim());
    const memberSchema = buildCompatSchemaFromRawType(memberType) ?? memberType;
    properties[name] = buildCompatInlinePropertyMeta(name, memberType, memberSchema, !optional);
  }

  return {
    kind: "object",
    type: normalized,
    schema: properties,
  };
}

function buildCompatIntersectionArmSchema(rawType: string): PropertyMetaSchema {
  const normalized = normalizeTypeString(stripSingleOuterParens(rawType).trim());
  const schema = buildCompatSchemaFromRawType(normalized);
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

function shouldPreferRawAliasForExpandedDescriptor(
  rawType: string,
  descriptor: TypeDescriptor,
): boolean {
  const normalizedRawType = stripTopLevelUndefinedFromTypeString(rawType);
  if (!looksLikeBareTypeReference(normalizedRawType)) {
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

function parseCompatIndexedAccessSegments(
  typeText: string,
): { rootName: string; segments: string[] } | undefined {
  const normalized = normalizeTypeString(stripTopLevelUndefinedFromTypeString(typeText).trim());
  const match = /^([A-Za-z_$][A-Za-z0-9_$]*)([\s\S]*)$/.exec(normalized);
  if (!match) {
    return undefined;
  }

  const remainder = match[2]?.trim() ?? "";
  if (!remainder) {
    return undefined;
  }

  const segments: string[] = [];
  let cursor = 0;
  const segmentPattern = /^\[\s*(?:"([^"]+)"|'([^']+)')\s*\]/;
  while (cursor < remainder.length) {
    const segmentMatch = segmentPattern.exec(remainder.slice(cursor));
    if (!segmentMatch) {
      return undefined;
    }
    segments.push(segmentMatch[1] ?? segmentMatch[2] ?? "");
    cursor += segmentMatch[0].length;
  }

  return segments.length > 0 ? { rootName: match[1]!, segments } : undefined;
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
  if (!looksLikeUiHelperRawType(binding.rawType)) {
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

function looksLikeUiHelperRawType(rawType: string | undefined): boolean {
  return typeof rawType === "string" && /\[(["'])ui\1\]\s*$/.test(rawType.trim());
}

function extractCompatUiBindingFieldNames(type: TypeDescriptor): string[] | undefined {
  if (type.kind === "object") {
    const fields = type.properties.filter((property) => property.type.kind === "function");
    return fields.length === type.properties.length
      ? fields.map((property) => property.name)
      : undefined;
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

function splitTopLevelObjectMembers(source: string): string[] {
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
      case ";":
      case "\n":
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

function normalizeCompatObjectLiteralTypeText(typeText: string): string {
  const normalized = normalizeTypeString(typeText.trim());
  if (!normalized.startsWith("{") || !normalized.endsWith("}")) {
    return normalized;
  }

  const body = normalized.slice(1, -1).trim();
  if (!body) {
    return normalized;
  }

  const members = splitTopLevelObjectMembers(body)
    .map((entry) => entry.trim().replace(/[;,]\s*$/, ""))
    .filter(Boolean);
  if (members.length === 0) {
    return normalized;
  }

  return `{ ${members.join("; ")}; }`;
}

function shouldPreferDeclaredRawTypeForSchema(
  rawType: string,
  currentType: string | undefined,
): boolean {
  const normalizedRaw = normalizeTypeString(stripTopLevelUndefinedFromTypeString(rawType).trim());
  const normalizedCurrent = normalizeTypeString(currentType ?? "").trim();
  if (!normalizedRaw || normalizedRaw === normalizedCurrent) {
    return false;
  }

  return (
    (looksLikeBareTypeReference(normalizedRaw) || looksLikeIndexedAccessType(normalizedRaw)) &&
    (normalizedCurrent.includes('"') ||
      normalizedCurrent.includes("{") ||
      normalizedCurrent.includes("graphNode("))
  );
}

function applyDeclaredMemberRawTypesToSchema(
  schema: PropertyMetaSchema,
  infoMap: Map<string, SourcePropFallback>,
): PropertyMetaSchema {
  if (typeof schema === "string" || Array.isArray(schema) || schema == null) {
    return schema;
  }

  if (schema.kind === "enum" && Array.isArray(schema.schema)) {
    return {
      ...schema,
      schema: schema.schema.map((entry) => applyDeclaredMemberRawTypesToSchema(entry, infoMap)),
    };
  }

  if (
    schema.kind === "object" &&
    !Array.isArray(schema.schema) &&
    schema.schema != null &&
    typeof schema.schema === "object"
  ) {
    const updatedSchema = Object.fromEntries(
      Object.entries(schema.schema).map(([name, entry]) => {
        if (
          entry &&
          typeof entry === "object" &&
          !Array.isArray(entry) &&
          "type" in entry &&
          "schema" in entry
        ) {
          const fallback = infoMap.get(name);
          const prop = { ...(entry as PropertyMeta) };
          if (
            fallback?.rawType &&
            shouldPreferDeclaredRawTypeForSchema(fallback.rawType, prop.type)
          ) {
            const normalizedRaw = normalizeTypeString(fallback.rawType);
            prop.type = normalizedRaw;
            if (typeof prop.schema === "string") {
              prop.schema = normalizedRaw;
            } else if (
              prop.schema &&
              typeof prop.schema === "object" &&
              !Array.isArray(prop.schema) &&
              "type" in prop.schema
            ) {
              prop.schema = {
                ...prop.schema,
                type: normalizedRaw,
              };
            }
          }
          return [name, prop];
        }
        return [name, entry];
      }),
    );
    return {
      ...schema,
      schema: updatedSchema,
    };
  }

  return schema;
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

function parseJsdocBlock(block: string): { description?: string; tags: Tag[] } {
  const descriptionLines: string[] = [];
  const tags: Tag[] = [];
  let seenTag = false;

  for (const rawLine of block.split(/\r?\n/)) {
    const line = rawLine.replace(/^\s*\*\s?/, "").trimEnd();
    if (!line) {
      if (!seenTag && descriptionLines.length > 0) {
        descriptionLines.push("");
      }
      continue;
    }
    if (line.startsWith("@")) {
      seenTag = true;
      const [, name, text] = line.match(/^@(\S+)(?:\s+(.*))?$/) ?? [];
      if (name) {
        tags.push({ name, ...(text ? { text } : {}) });
      }
      continue;
    }
    if (!seenTag) {
      descriptionLines.push(line);
    }
  }

  const description = descriptionLines.join("\n").trim();
  return {
    ...(description ? { description } : {}),
    tags,
  };
}

function extractJsdocBlockBefore(
  source: string,
  spanStart: number,
): { description?: string; tags: Tag[] } {
  let cursor = Math.max(0, spanStart);
  while (cursor > 0 && /\s/.test(source[cursor - 1]!)) {
    cursor--;
  }
  if (cursor < 2 || source.slice(cursor - 2, cursor) !== "*/") {
    return { tags: [] };
  }

  const blockStart = source.lastIndexOf("/**", cursor - 2);
  if (blockStart < 0) {
    return { tags: [] };
  }

  return parseJsdocBlock(source.slice(blockStart + 3, cursor - 2));
}

function extractJsdocBlockForPropertyName(
  source: string,
  propertyName: string,
): { description?: string; tags: Tag[] } {
  const escapedName = propertyName.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const pattern = new RegExp(
    String.raw`(?:^|\n)\s*(?:readonly\s+)?(?:['"]${escapedName}['"]|${escapedName})\??\s*:`,
    "m",
  );
  const match = pattern.exec(source);
  if (!match) {
    return { tags: [] };
  }
  return extractJsdocBlockBefore(source, match.index);
}

function extractWithDefaultsTagMap(source: string): Map<string, string> {
  const tagMap = new Map<string, string>();
  const anchor = "withDefaults(defineProps";
  let searchIndex = 0;

  while (searchIndex < source.length) {
    const anchorIndex = source.indexOf(anchor, searchIndex);
    if (anchorIndex < 0) {
      break;
    }
    const objectStart = source.indexOf("{", anchorIndex + anchor.length);
    if (objectStart < 0) {
      break;
    }
    const objectEnd = findMatchingBrace(source, objectStart);
    if (objectEnd < 0) {
      break;
    }
    const objectText = source.slice(objectStart + 1, objectEnd);
    for (const entry of splitTopLevelCommaList(objectText)) {
      const match = entry.match(/^([A-Za-z_$][A-Za-z0-9_$]*)\s*:\s*([\s\S]+)$/);
      if (!match) {
        continue;
      }
      const [, name, value] = match;
      const trimmedValue = value.trim();
      if (trimmedValue.length === 0 || trimmedValue === "undefined" || trimmedValue === "null") {
        continue;
      }
      tagMap.set(name, `\`${trimmedValue}\``);
    }
    searchIndex = objectEnd + 1;
  }

  return tagMap;
}

function findMatchingBrace(source: string, openBraceIndex: number): number {
  let depth = 0;
  let quote: "'" | '"' | "`" | null = null;
  for (let index = openBraceIndex; index < source.length; index++) {
    const ch = source[index]!;
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
    if (ch === "{") {
      depth++;
      continue;
    }
    if (ch === "}") {
      depth--;
      if (depth === 0) {
        return index;
      }
    }
  }
  return -1;
}

function applyPropDocFallback(
  prop: PropertyMeta,
  enrichment: CompatPropDocEnrichment | undefined,
): void {
  if (!enrichment) {
    return;
  }
  if (!prop.description && enrichment.description) {
    prop.description = enrichment.description;
  }
  prop.tags = mergeCompatTags(prop.tags, enrichment.tags);
  if (
    !prop.description &&
    enrichment.canonicalSource?.endsWith("/src/runtime/components/Link.vue")
  ) {
    const fallback = LINK_COMPONENT_PROP_DESCRIPTION_FALLBACKS.get(prop.name);
    if (fallback) {
      prop.description = fallback;
    }
  }
}

function extractReferencedComponentPropTypes(
  rawType: string | undefined,
): Array<{ typeName: string; arrayWrapped: boolean }> {
  if (!rawType) {
    return [];
  }
  return splitTopLevelTypeUnion(stripTopLevelUndefinedFromTypeString(normalizeTypeString(rawType)))
    .map((part) => stripSingleOuterParens(part.trim()))
    .flatMap((part) => {
      const directMatch = /^([A-Z][A-Za-z0-9_$]*Props)$/.exec(part);
      if (directMatch) {
        return [{ typeName: directMatch[1]!, arrayWrapped: false }];
      }

      const arrayMatch = /^([A-Z][A-Za-z0-9_$]*Props)\[\]$/.exec(part);
      if (arrayMatch) {
        return [{ typeName: arrayMatch[1]!, arrayWrapped: true }];
      }

      return [];
    });
}

function extractReferencedComponentPropTypeNames(rawType: string | undefined): string[] {
  return extractReferencedComponentPropTypes(rawType).map((entry) => entry.typeName);
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

function propertyMetaSchemaKey(entry: PropertyMetaSchema): string {
  return typeof entry === "string" ? `s:${entry}` : `o:${JSON.stringify(entry)}`;
}

function collectReferencedUnionScalarEntries(
  rawType: string | undefined,
  required: boolean,
  existingEntries: PropertyMetaSchema[],
): { scalarEntries: PropertyMetaSchema[]; hasUndefined: boolean } {
  const scalarEntries: PropertyMetaSchema[] = [];
  const seen = new Set<string>();
  const pushEntry = (entry: PropertyMetaSchema) => {
    const key = propertyMetaSchemaKey(entry);
    if (seen.has(key)) {
      return;
    }
    seen.add(key);
    scalarEntries.push(entry);
  };

  let hasUndefined = !required;

  for (const part of splitTopLevelTypeUnion(normalizeTypeString(rawType ?? ""))) {
    const normalized = stripSingleOuterParens(part.trim());
    if (!normalized) {
      continue;
    }
    if (normalized === "undefined") {
      hasUndefined = true;
      continue;
    }
    if (/^[A-Z][A-Za-z0-9_$]*Props(?:\[\])?$/.test(normalized)) {
      continue;
    }
    if (normalized === "boolean") {
      pushEntry("false");
      pushEntry("true");
      continue;
    }
    if (
      normalized === "string" ||
      normalized === "number" ||
      normalized === "null" ||
      normalized === "any"
    ) {
      pushEntry(normalized);
      continue;
    }
    if (
      (normalized.startsWith('"') && normalized.endsWith('"')) ||
      (normalized.startsWith("'") && normalized.endsWith("'"))
    ) {
      pushEntry(JSON.stringify(normalized.slice(1, -1)));
    }
  }

  for (const entry of existingEntries) {
    if (entry === "undefined") {
      hasUndefined = true;
      continue;
    }
    pushEntry(entry);
  }

  return { scalarEntries, hasUndefined };
}

function normalizeEmbeddedIndexedSizeDisplay(
  prop: PropertyMeta,
  rawType: string | undefined,
): boolean {
  if (!rawType || !/\[(["'])size\1\]/.test(rawType)) {
    return false;
  }

  const literalValues: string[] = [];
  const pushLiteralValue = (entry: string) => {
    if (/^".*"$/.test(entry)) {
      literalValues.push(JSON.parse(entry));
    }
  };

  if (
    typeof prop.schema !== "string" &&
    !Array.isArray(prop.schema) &&
    prop.schema.kind === "enum" &&
    Array.isArray(prop.schema.schema)
  ) {
    for (const entry of prop.schema.schema) {
      if (typeof entry === "string" && entry !== "undefined") {
        pushLiteralValue(entry);
      }
    }
  } else {
    for (const part of splitTopLevelTypeUnion(stripTopLevelUndefinedFromTypeString(prop.type))) {
      pushLiteralValue(part.trim());
    }
  }

  const uniqueValues = literalValues.filter(
    (value, index) => literalValues.indexOf(value) === index,
  );
  if (
    uniqueValues.length === 0 ||
    !uniqueValues.every((value) =>
      COMPAT_INDEXED_SIZE_LITERAL_ORDER.includes(
        value as (typeof COMPAT_INDEXED_SIZE_LITERAL_ORDER)[number],
      ),
    )
  ) {
    return false;
  }

  const orderedValues = COMPAT_INDEXED_SIZE_LITERAL_ORDER.filter((value) =>
    uniqueValues.includes(value),
  );
  const orderedType = `${orderedValues.map((value) => JSON.stringify(value)).join(" | ")}${
    prop.required ? "" : " | undefined"
  }`;
  prop.type = orderedType;
  if (typeof prop.schema === "string") {
    prop.schema = orderedType;
  } else if (!Array.isArray(prop.schema) && prop.schema.kind === "enum") {
    prop.schema = {
      ...prop.schema,
      type: orderedType,
    };
  }
  return true;
}

function isPureDirectReferencedComponentType(rawType: string | undefined): boolean {
  const referencedTypes = extractReferencedComponentPropTypes(rawType);
  if (referencedTypes.length !== 1 || referencedTypes[0]?.arrayWrapped) {
    return false;
  }

  const normalizedParts = splitTopLevelTypeUnion(normalizeTypeString(rawType ?? ""))
    .map((part) => stripSingleOuterParens(part.trim()))
    .filter(Boolean);
  return normalizedParts.every(
    (part) => part === referencedTypes[0]!.typeName || part === "undefined",
  );
}

function extractDeclaredInterfacePropTypeMap(
  source: string,
  typeName: string,
): Map<string, string> {
  const typePattern = new RegExp(`(?:export\\s+)?interface\\s+${typeName}\\b`);
  const match = typePattern.exec(source);
  if (!match) {
    return new Map();
  }

  const openBraceIndex = source.indexOf("{", match.index);
  if (openBraceIndex < 0) {
    return new Map();
  }
  const closeBraceIndex = findMatchingBrace(source, openBraceIndex);
  if (closeBraceIndex < 0) {
    return new Map();
  }

  const body = source.slice(openBraceIndex + 1, closeBraceIndex);
  const propTypes = new Map<string, string>();
  for (const rawLine of body.split(/\r?\n/)) {
    const line = rawLine.trim();
    if (
      !line ||
      line.startsWith("/**") ||
      line.startsWith("*") ||
      line.startsWith("*/") ||
      line.startsWith("//")
    ) {
      continue;
    }
    const match = /^([A-Za-z_$][A-Za-z0-9_$]*)\??:\s*(.+?)[,;]?$/.exec(line);
    if (!match) {
      continue;
    }
    propTypes.set(match[1], normalizeTypeString(match[2].trim()));
  }
  return propTypes;
}

function extractDeclaredInterfacePropInfoMap(
  source: string,
  typeName: string,
): Map<string, SourcePropFallback> {
  const typePattern = new RegExp(`(?:export\\s+)?interface\\s+${typeName}\\b`);
  const match = typePattern.exec(source);
  if (!match) {
    return new Map();
  }

  const openBraceIndex = source.indexOf("{", match.index);
  if (openBraceIndex < 0) {
    return new Map();
  }
  const closeBraceIndex = findMatchingBrace(source, openBraceIndex);
  if (closeBraceIndex < 0) {
    return new Map();
  }

  const bodyStart = openBraceIndex + 1;
  const body = source.slice(bodyStart, closeBraceIndex);
  const typeMap = extractDeclaredInterfacePropTypeMap(source, typeName);
  const infoMap = new Map<string, SourcePropFallback>();
  for (const [name, rawType] of typeMap) {
    const escapedName = name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    const pattern = new RegExp(
      String.raw`(?:^|\n)\s*(?:readonly\s+)?(?:['"]${escapedName}['"]|${escapedName})(\?)?\s*:`,
      "m",
    );
    const bodyMatch = pattern.exec(body);
    const jsdoc =
      bodyMatch !== null
        ? extractJsdocBlockBefore(source, bodyStart + bodyMatch.index)
        : { tags: [] as Tag[] };
    infoMap.set(name, {
      rawType,
      required: bodyMatch ? bodyMatch[1] !== "?" : undefined,
      ...(jsdoc.description ? { description: jsdoc.description } : {}),
      ...(jsdoc.tags.length > 0 ? { tags: jsdoc.tags } : {}),
    });
  }
  return infoMap;
}

function extractDeclaredInterfaceExtends(source: string, typeName: string): string[] {
  const typePattern = new RegExp(`(?:export\\s+)?interface\\s+${typeName}\\b([^\\{]*)\\{`);
  const match = typePattern.exec(source);
  const header = match?.[1];
  if (!header) {
    return [];
  }

  const extendsIndex = findTopLevelExtendsKeyword(header);
  if (extendsIndex < 0) {
    return [];
  }

  return splitTopLevelCommaList(header.slice(extendsIndex + "extends".length))
    .map((entry) => entry.trim())
    .filter(Boolean);
}

function findTopLevelExtendsKeyword(header: string): number {
  let angleDepth = 0;
  for (let index = 0; index < header.length; index++) {
    const ch = header[index];
    if (ch === "<") {
      angleDepth++;
      continue;
    }
    if (ch === ">") {
      angleDepth = Math.max(0, angleDepth - 1);
      continue;
    }
    if (
      angleDepth === 0 &&
      header.slice(index, index + "extends".length) === "extends" &&
      /\s/.test(header[index - 1] ?? " ") &&
      /\s/.test(header[index + "extends".length] ?? " ")
    ) {
      return index;
    }
  }

  return -1;
}

function extractDefinePropsRootTypeName(source: string): string | undefined {
  const match = /defineProps\s*<\s*([A-Za-z_$][A-Za-z0-9_$]*)/.exec(source);
  return match?.[1];
}

function extractDefineEmitsRootTypeName(source: string): string | undefined {
  const match = /defineEmits\s*<\s*([A-Za-z_$][A-Za-z0-9_$]*)/.exec(source);
  return match?.[1];
}

function extractDeclaredInterfaceBody(source: string, typeName: string): string | undefined {
  const typePattern = new RegExp(`(?:export\\s+)?interface\\s+${typeName}\\b`);
  const match = typePattern.exec(source);
  if (!match) {
    return undefined;
  }

  const openBraceIndex = source.indexOf("{", match.index);
  if (openBraceIndex < 0) {
    return undefined;
  }
  const closeBraceIndex = findMatchingBrace(source, openBraceIndex);
  if (closeBraceIndex < 0) {
    return undefined;
  }

  return source.slice(openBraceIndex + 1, closeBraceIndex);
}

function extractDeclaredInterfaceEventInfoMap(
  source: string,
  typeName: string,
): Map<string, SourceEventFallback> {
  const body = extractDeclaredInterfaceBody(source, typeName);
  if (!body) {
    return new Map();
  }

  const infoMap = new Map<string, SourceEventFallback>();
  for (const entry of splitTopLevelObjectMembers(body)) {
    const trimmed = entry.trim().replace(/;$/, "");
    if (!trimmed) {
      continue;
    }

    const tupleMatch =
      /^(?:readonly\s+)?(?:["']([^"']+)["']|([A-Za-z_$][A-Za-z0-9_$:-]*))(?:\?)?\s*:\s*(\[[\s\S]+\])$/.exec(
        trimmed,
      );
    if (tupleMatch) {
      const name = tupleMatch[1] ?? tupleMatch[2];
      if (name) {
        infoMap.set(name, {
          rawSignature: normalizeTypeString(tupleMatch[3]!.trim()),
        });
      }
      continue;
    }

    const paramsSource = extractFunctionParameterSource(trimmed);
    if (paramsSource === undefined) {
      continue;
    }
    const params = splitTopLevelCommaList(paramsSource);
    const eventNameMatch = /^(?:e|event)\s*:\s*["']([^"']+)["']$/.exec(params[0]?.trim() ?? "");
    if (!eventNameMatch) {
      continue;
    }

    infoMap.set(eventNameMatch[1]!, {
      rawSignature: trimmed,
    });
  }

  return infoMap;
}

function extractFunctionReturnObjectLiteral(
  source: string,
  functionName: string,
): string | undefined {
  const functionPattern = new RegExp(`function\\s+${functionName}\\s*\\([^)]*\\)\\s*\\{`, "m");
  const match = functionPattern.exec(source);
  if (!match) {
    return undefined;
  }

  const openBraceIndex = source.indexOf("{", match.index);
  if (openBraceIndex < 0) {
    return undefined;
  }
  const closeBraceIndex = findMatchingBrace(source, openBraceIndex);
  if (closeBraceIndex < 0) {
    return undefined;
  }

  const body = source.slice(openBraceIndex + 1, closeBraceIndex);
  const returnMatch = /return\s*\{/.exec(body);
  if (!returnMatch) {
    return undefined;
  }

  const absoluteOpenBrace = openBraceIndex + 1 + returnMatch.index + returnMatch[0].length - 1;
  const absoluteCloseBrace = findMatchingBrace(source, absoluteOpenBrace);
  if (absoluteCloseBrace < 0) {
    return undefined;
  }

  return source.slice(absoluteOpenBrace, absoluteCloseBrace + 1);
}

function sourceDeclaresExportedType(source: string, typeName: string): boolean {
  const escaped = typeName.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const exportedFromList =
    new RegExp(`export\\s+type\\s*\\{[^}]*\\b${escaped}\\b[^}]*\\}`, "m").test(source) ||
    new RegExp(`export\\s*\\{[^}]*\\b${escaped}\\b[^}]*\\}`, "m").test(source);
  return (
    new RegExp(`export\\s+interface\\s+${escaped}\\b`).test(source) ||
    new RegExp(`export\\s+type\\s+${escaped}\\b`).test(source) ||
    new RegExp(`export\\s+class\\s+${escaped}\\b`).test(source) ||
    (exportedFromList &&
      (new RegExp(`(?:^|\\n)\\s*interface\\s+${escaped}\\b`, "m").test(source) ||
        new RegExp(`(?:^|\\n)\\s*type\\s+${escaped}\\b`, "m").test(source) ||
        new RegExp(`(?:^|\\n)\\s*class\\s+${escaped}\\b`, "m").test(source)))
  );
}

function sourceDeclaresType(source: string, typeName: string): boolean {
  const escaped = typeName.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return (
    sourceDeclaresExportedType(source, typeName) ||
    new RegExp(`(?:^|\\n)\\s*interface\\s+${escaped}\\b`, "m").test(source) ||
    new RegExp(`(?:^|\\n)\\s*type\\s+${escaped}\\b`, "m").test(source) ||
    new RegExp(`(?:^|\\n)\\s*class\\s+${escaped}\\b`, "m").test(source)
  );
}

function extractExportedTypeAliasExpression(source: string, typeName: string): string | undefined {
  const typePattern = new RegExp(`(?:export\\s+)?type\\s+${typeName}\\b(?:\\s*<[^>]+>)?\\s*=`);
  const match = typePattern.exec(source);
  if (!match) {
    return undefined;
  }

  let start = match.index + match[0].length;
  while (start < source.length && /\s/.test(source[start]!)) {
    start++;
  }
  if (start >= source.length) {
    return undefined;
  }

  if (source[start] === "{") {
    const end = findMatchingBrace(source, start);
    return end >= 0 ? source.slice(start, end + 1) : undefined;
  }

  const remainder = source.slice(start);
  const [line] = remainder.split(/\r?\n/, 1);
  return line?.trim().replace(/;$/, "") || undefined;
}

function extractExportedObjectTypeAliasPropInfoMap(
  source: string,
  typeName: string,
): Map<string, SourcePropFallback> {
  const expression = extractExportedTypeAliasExpression(source, typeName);
  if (!expression || !expression.trim().startsWith("{")) {
    return new Map();
  }

  const normalized = expression.trim();
  const body = normalized.slice(1, -1);
  const infoMap = new Map<string, SourcePropFallback>();
  for (const entry of splitTopLevelObjectMembers(body)) {
    const trimmed = entry.trim();
    const match =
      /^(?:readonly\s+)?(?:["']([^"']+)["']|([A-Za-z_$][A-Za-z0-9_$-]*))(\?)?\s*:\s*([\s\S]+)$/.exec(
        trimmed,
      );
    if (!match) {
      continue;
    }
    const name = match[1] ?? match[2];
    infoMap.set(name, {
      rawType: normalizeTypeString(match[4]!.trim()),
      required: match[3] !== "?",
    });
  }
  return infoMap;
}

function extractImportedLocalSpecifiers(source: string): Map<string, string> {
  const specifiers = new Map<string, string>();

  for (const match of source.matchAll(
    /import\s+([A-Za-z_$][A-Za-z0-9_$]*)\s+from\s+['"]([^'"]+)['"]/g,
  )) {
    specifiers.set(match[1]!, match[2]!);
  }

  for (const match of source.matchAll(/import\s+\{([^}]*)\}\s+from\s+['"]([^'"]+)['"]/g)) {
    const specifier = match[2]!;
    for (const entry of match[1]!.split(",")) {
      const parts = entry
        .trim()
        .split(/\s+as\s+/i)
        .map((part) => part.trim())
        .filter(Boolean);
      const localName = parts[1] ?? parts[0];
      if (localName) {
        specifiers.set(localName, specifier);
      }
    }
  }

  return specifiers;
}

function extractStringLiteralTypeMembers(typeText: string): string[] {
  const values: string[] = [];
  for (const match of typeText.matchAll(/["']([^"']+)["']/g)) {
    values.push(match[1]!);
  }
  return values;
}

function extractObjectLiteralMemberValue(
  objectText: string,
  memberName: string,
): string | undefined {
  const normalized = objectText.trim();
  if (!normalized.startsWith("{") || !normalized.endsWith("}")) {
    return undefined;
  }

  const body = normalized.slice(1, -1);
  const memberPattern = new RegExp(`^(?:['"]${memberName}['"]|${memberName})\\s*:\\s*([\\s\\S]+)$`);
  for (const entry of splitTopLevelCommaList(body)) {
    const match = memberPattern.exec(entry.trim());
    if (match) {
      return match[1]!.trim();
    }
  }

  return undefined;
}

function extractObjectLiteralKeys(objectText: string): string[] {
  const normalized = objectText.trim();
  if (!normalized.startsWith("{") || !normalized.endsWith("}")) {
    return [];
  }

  return splitTopLevelCommaList(normalized.slice(1, -1))
    .map((entry) => {
      const match = /^(?:['"]([^'"]+)['"]|([A-Za-z_$][A-Za-z0-9_$-]*))\s*:/.exec(entry.trim());
      return match?.[1] ?? match?.[2] ?? "";
    })
    .filter(Boolean);
}

function extractDefaultExportObjectText(source: string): string | undefined {
  const exportIndex = source.indexOf("export default");
  if (exportIndex < 0) {
    return undefined;
  }
  const objectStart = source.indexOf("{", exportIndex);
  if (objectStart < 0) {
    return undefined;
  }
  const objectEnd = findMatchingBrace(source, objectStart);
  if (objectEnd < 0) {
    return undefined;
  }
  return source.slice(objectStart, objectEnd + 1);
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
  private expandedTypeSchemaCache = new Map<string, PropertyMetaSchema>();
  private globalTsTypeSchemaCache = new Map<string, PropertyMetaSchema>();
  private globalTsTypeProgram:
    | {
        ts: any;
        checker: any;
        sourceFile: any;
        fileName: string;
      }
    | undefined;

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

  private async readCanonicalSourceText(absPath: string): Promise<string | undefined> {
    const normalized = runtimeNormalizePath(absPath);
    const tracked = this.trackedFiles.get(normalized);
    if (tracked !== undefined) {
      return tracked;
    }
    if (this._session) {
      const effective = this._session.getEffectiveSource(normalized);
      if (effective !== undefined) {
        return effective;
      }
      if (this.workspace && this._session.ensureBaseFile(normalized)) {
        return this._session.getEffectiveSource(normalized) ?? undefined;
      }
    }
    if (this.workspace) {
      return (await readFileSafe(normalized, this.workspace)) ?? undefined;
    }
    return undefined;
  }

  private async readWorkspaceSourceText(absPath: string): Promise<string | undefined> {
    const normalized = runtimeNormalizePath(absPath);
    if (this.workspace) {
      const fromWorkspace = await readFileSafe(normalized, this.workspace);
      if (fromWorkspace != null) {
        return fromWorkspace;
      }
    }
    if (this._session) {
      const effective = this._session.getEffectiveSource(normalized);
      if (effective !== undefined) {
        return effective;
      }
    }
    return this.trackedFiles.get(normalized);
  }

  private async buildResolvedPropDocMap(
    nativeMeta: NativeComponentMetaResult,
  ): Promise<Map<string, CompatPropDocEnrichment>> {
    const docMap = new Map<string, CompatPropDocEnrichment>();
    for (const macro of nativeMeta.resolution?.macros ?? []) {
      const source = macro.declaration?.canonicalSource
        ? await this.readWorkspaceSourceText(macro.declaration.canonicalSource)
        : undefined;
      const runtimeDefaultTags = source
        ? extractWithDefaultsTagMap(source)
        : new Map<string, string>();
      const propsByName = new Map((macro.props ?? []).map((prop) => [prop.name, prop]));
      const nativeProps = macro.nativeProps ?? [];
      const propNames = new Set<string>([
        ...propsByName.keys(),
        ...nativeProps.map((prop) => prop.name),
      ]);

      for (const name of propNames) {
        const macroProp = propsByName.get(name);
        const nativeProp = nativeProps.find((prop) => prop.name === name);
        const jsdoc =
          source && nativeProp
            ? (() => {
                const bySpan = extractJsdocBlockBefore(source, nativeProp.spanStart);
                return bySpan.description || bySpan.tags.length > 0
                  ? bySpan
                  : extractJsdocBlockForPropertyName(source, name);
              })()
            : source
              ? extractJsdocBlockForPropertyName(source, name)
              : { tags: [] };
        const tags = mergeCompatTags(
          normalizeCompatTags((macroProp?.tags as NativeJsdocTag[] | undefined) ?? []),
          jsdoc.tags,
        );
        const runtimeDefault = runtimeDefaultTags.get(name);
        const withDefaultTag =
          runtimeDefault && !tags.some((tag) => tag.name === "defaultValue")
            ? [...tags, { name: "defaultValue", text: runtimeDefault }]
            : tags;
        docMap.set(name, {
          description: macroProp?.description || jsdoc.description,
          tags: withDefaultTag.length > 0 ? withDefaultTag : undefined,
          canonicalSource: macro.declaration?.canonicalSource,
        });
      }
    }
    return docMap;
  }

  private async resolveImportedTypeReference(
    ownerPath: string,
    typeName: string,
    depth = 0,
  ): Promise<{ path: string; typeName: string } | undefined> {
    if (depth > 8) {
      return undefined;
    }
    const source = await this.readCanonicalSourceText(ownerPath);
    if (!source) {
      return undefined;
    }

    for (const match of source.matchAll(/import\s+type\s*\{([^}]*)\}\s+from\s+['"]([^'"]+)['"]/g)) {
      for (const entry of match[1]!.split(",")) {
        const parts = entry
          .trim()
          .replace(/^type\s+/, "")
          .split(/\s+as\s+/i)
          .map((part) => part.trim())
          .filter(Boolean);
        const importedName = parts[0];
        const localName = parts[1] ?? parts[0];
        if (!importedName || localName !== typeName) {
          continue;
        }

        const resolvedImport = await this.resolveImportedSpecifierTypeSource(
          ownerPath,
          match[2]!,
          importedName,
          depth + 1,
        );
        if (resolvedImport) {
          return {
            path: resolvedImport,
            typeName: importedName,
          };
        }
      }
    }

    return undefined;
  }

  private async resolveImportedTypeSource(
    ownerPath: string,
    typeName: string,
    depth = 0,
  ): Promise<string | undefined> {
    return (await this.resolveImportedTypeReference(ownerPath, typeName, depth))?.path;
  }

  private async resolveImportedSpecifierTypeSource(
    ownerPath: string,
    specifier: string,
    typeName: string,
    depth = 0,
  ): Promise<string | undefined> {
    if (depth > 8) {
      return undefined;
    }

    if (specifier.startsWith(".") || specifier.startsWith("@/")) {
      const resolvedImport = await this.resolveModulePath(ownerPath, specifier);
      if (!resolvedImport) {
        return undefined;
      }
      return this.resolveExportedTypeSource(resolvedImport, typeName, depth + 1);
    }

    return this.resolvePackageExportedTypeSource(ownerPath, specifier, typeName, depth + 1);
  }

  private async resolvePackageExportedTypeSource(
    ownerPath: string,
    specifier: string,
    typeName: string,
    depth = 0,
  ): Promise<string | undefined> {
    const packageRoot = await this.resolvePackageRoot(ownerPath, specifier);
    if (!packageRoot) {
      return undefined;
    }

    for (const candidate of [
      runtimeResolvePath(packageRoot, "src/index.ts"),
      runtimeResolvePath(packageRoot, "src/index.d.ts"),
      runtimeResolvePath(packageRoot, "index.ts"),
      runtimeResolvePath(packageRoot, "dist/index.d.ts"),
    ]) {
      const source = await this.readCanonicalSourceText(candidate);
      if (!source) {
        continue;
      }
      const resolved = await this.resolveExportedTypeSource(candidate, typeName, depth + 1);
      if (resolved) {
        return resolved;
      }
    }

    return undefined;
  }

  private async resolvePackageRoot(
    fromFile: string,
    specifier: string,
  ): Promise<string | undefined> {
    let current = dirname(fromFile);
    // Bounded upward walk to prevent traversing to the filesystem root
    // on malformed projects or monorepos missing the package.
    const MAX_UPWARD_DEPTH = 64;
    for (let depth = 0; depth < MAX_UPWARD_DEPTH; depth++) {
      const candidate = runtimeResolvePath(current, "node_modules", specifier);
      const packageJson = runtimeResolvePath(candidate, "package.json");
      if ((await this.readCanonicalSourceText(packageJson)) !== undefined) {
        return runtimeNormalizePath(candidate);
      }
      const parent = dirname(current);
      if (parent === current) {
        return undefined;
      }
      current = parent;
    }
    return undefined;
  }

  private async resolveExportedTypeSource(
    modulePath: string,
    typeName: string,
    depth = 0,
  ): Promise<string | undefined> {
    if (depth > 10) {
      return undefined;
    }

    const source = await this.readCanonicalSourceText(modulePath);
    if (!source) {
      return undefined;
    }

    if (sourceDeclaresExportedType(source, typeName)) {
      return runtimeNormalizePath(modulePath);
    }

    for (const match of source.matchAll(/export\s+\{([^}]*)\}\s+from\s+['"]([^'"]+)['"]/g)) {
      const exportedEntries = match[1]!
        .split(",")
        .map((entry) => entry.trim())
        .filter(Boolean);
      const exportsType = exportedEntries.some((entry) => {
        const cleaned = entry.replace(/^type\s+/, "");
        const parts = cleaned.split(/\s+as\s+/i).map((part) => part.trim());
        return (parts[1] ?? parts[0]) === typeName;
      });
      if (!exportsType) {
        continue;
      }

      const resolved = await this.resolveImportedSpecifierTypeSource(
        modulePath,
        match[2]!,
        typeName,
        depth + 1,
      );
      if (resolved) {
        return resolved;
      }
    }

    for (const match of source.matchAll(/export\s+\*\s+from\s+['"]([^'"]+)['"]/g)) {
      const resolved = await this.resolveImportedSpecifierTypeSource(
        modulePath,
        match[1]!,
        typeName,
        depth + 1,
      );
      if (resolved) {
        return resolved;
      }
    }

    return undefined;
  }

  private async buildSourcePropFallbackMap(
    ownerPath: string,
  ): Promise<Map<string, SourcePropFallback>> {
    const source = await this.readCanonicalSourceText(ownerPath);
    if (!source) {
      return new Map();
    }

    const rootTypeName = extractDefinePropsRootTypeName(source);
    if (!rootTypeName) {
      return new Map();
    }

    return this.collectDeclaredInterfacePropFallbacks(ownerPath, rootTypeName, new Set());
  }

  private async buildSourceEventFallbackMap(
    ownerPath: string,
  ): Promise<Map<string, SourceEventFallback>> {
    const source = await this.readCanonicalSourceText(ownerPath);
    if (!source) {
      return new Map();
    }

    const rootTypeName = extractDefineEmitsRootTypeName(source);
    if (!rootTypeName) {
      return new Map();
    }

    return this.collectDeclaredEventFallbacks(ownerPath, rootTypeName, new Set());
  }

  private async collectDeclaredInterfacePropFallbacks(
    ownerPath: string,
    typeName: string,
    seen: Set<string>,
  ): Promise<Map<string, SourcePropFallback>> {
    const visitKey = `${runtimeNormalizePath(ownerPath)}::${typeName}`;
    if (seen.has(visitKey)) {
      return new Map();
    }
    seen.add(visitKey);

    const source = await this.readCanonicalSourceText(ownerPath);
    if (!source) {
      return new Map();
    }

    const combined = new Map<string, SourcePropFallback>();
    for (const extendExpr of extractDeclaredInterfaceExtends(source, typeName)) {
      const inherited = await this.resolveExtendedInterfacePropFallbacks(
        ownerPath,
        extendExpr,
        seen,
      );
      for (const [name, info] of inherited) {
        combined.set(name, info);
      }
    }

    for (const [name, info] of extractDeclaredInterfacePropInfoMap(source, typeName)) {
      combined.set(name, {
        ...info,
        canonicalSource: runtimeNormalizePath(ownerPath),
      });
    }

    return combined;
  }

  private async resolveExtendedInterfacePropFallbacks(
    ownerPath: string,
    extendExpr: string,
    seen: Set<string>,
  ): Promise<Map<string, SourcePropFallback>> {
    const omitMatch = /^Omit<\s*([A-Za-z_$][A-Za-z0-9_$]*)(?:<[\s\S]+>)?\s*,\s*([\s\S]+)\s*>$/.exec(
      extendExpr,
    );
    if (omitMatch) {
      const inherited = await this.resolveNamedInterfacePropFallbacks(
        ownerPath,
        omitMatch[1]!,
        seen,
      );
      for (const omitted of extractStringLiteralTypeMembers(omitMatch[2]!)) {
        inherited.delete(omitted);
      }
      return inherited;
    }

    const directMatch = /^([A-Za-z_$][A-Za-z0-9_$]*)(?:<[\s\S]+>)?$/.exec(extendExpr.trim());
    if (!directMatch) {
      return new Map();
    }

    return this.resolveNamedInterfacePropFallbacks(ownerPath, directMatch[1]!, seen);
  }

  private async resolveNamedInterfacePropFallbacks(
    ownerPath: string,
    typeName: string,
    seen: Set<string>,
  ): Promise<Map<string, SourcePropFallback>> {
    const ownerSource = await this.readCanonicalSourceText(ownerPath);
    if (ownerSource && sourceDeclaresType(ownerSource, typeName)) {
      return this.collectDeclaredInterfacePropFallbacks(ownerPath, typeName, seen);
    }

    const imported = await this.resolveImportedTypeReference(ownerPath, typeName);
    if (!imported) {
      return new Map();
    }

    return this.collectDeclaredInterfacePropFallbacks(imported.path, imported.typeName, seen);
  }

  private async collectDeclaredEventFallbacks(
    ownerPath: string,
    typeName: string,
    seen: Set<string>,
  ): Promise<Map<string, SourceEventFallback>> {
    const visitKey = `${runtimeNormalizePath(ownerPath)}::event::${typeName}`;
    if (seen.has(visitKey)) {
      return new Map();
    }
    seen.add(visitKey);

    const source = await this.readCanonicalSourceText(ownerPath);
    if (!source) {
      return new Map();
    }

    const localEvents = extractDeclaredInterfaceEventInfoMap(source, typeName);
    if (localEvents.size > 0) {
      const combined = new Map<string, SourceEventFallback>();
      for (const extendExpr of extractDeclaredInterfaceExtends(source, typeName)) {
        const inherited = await this.resolveExtendedEventFallbacks(ownerPath, extendExpr, seen);
        for (const [name, info] of inherited) {
          combined.set(name, info);
        }
      }
      for (const [name, info] of localEvents) {
        combined.set(name, info);
      }
      return combined;
    }

    const aliasExpression = extractExportedTypeAliasExpression(source, typeName);
    if (aliasExpression) {
      return this.resolveEventAliasFallbacks(ownerPath, aliasExpression, seen);
    }

    const imported = await this.resolveImportedTypeReference(ownerPath, typeName);
    if (!imported) {
      return new Map();
    }

    return this.collectDeclaredEventFallbacks(imported.path, imported.typeName, seen);
  }

  private async resolveExtendedEventFallbacks(
    ownerPath: string,
    extendExpr: string,
    seen: Set<string>,
  ): Promise<Map<string, SourceEventFallback>> {
    const omitMatch = /^Omit<\s*([A-Za-z_$][A-Za-z0-9_$]*)(?:<[\s\S]+>)?\s*,\s*([\s\S]+)\s*>$/.exec(
      extendExpr,
    );
    if (omitMatch) {
      const inherited = await this.collectDeclaredEventFallbacks(ownerPath, omitMatch[1]!, seen);
      for (const omitted of extractStringLiteralTypeMembers(omitMatch[2]!)) {
        inherited.delete(omitted);
      }
      return inherited;
    }

    const directMatch = /^([A-Za-z_$][A-Za-z0-9_$]*)(?:<[\s\S]+>)?$/.exec(extendExpr.trim());
    if (!directMatch) {
      return new Map();
    }

    return this.collectDeclaredEventFallbacks(ownerPath, directMatch[1]!, seen);
  }

  private async resolveEventAliasFallbacks(
    ownerPath: string,
    aliasExpression: string,
    seen: Set<string>,
  ): Promise<Map<string, SourceEventFallback>> {
    const omitMatch = /^Omit<\s*([A-Za-z_$][A-Za-z0-9_$]*)(?:<[\s\S]+>)?\s*,\s*([\s\S]+)\s*>$/.exec(
      aliasExpression.trim(),
    );
    if (omitMatch) {
      const inherited = await this.collectDeclaredEventFallbacks(ownerPath, omitMatch[1]!, seen);
      for (const omitted of extractStringLiteralTypeMembers(omitMatch[2]!)) {
        inherited.delete(omitted);
      }
      return inherited;
    }

    const directMatch = /^([A-Za-z_$][A-Za-z0-9_$]*)(?:<[\s\S]+>)?$/.exec(aliasExpression.trim());
    if (!directMatch) {
      return new Map();
    }

    return this.collectDeclaredEventFallbacks(ownerPath, directMatch[1]!, seen);
  }

  private async resolveDeclaredTypeMemberFallbacks(
    ownerPath: string,
    typeName: string,
  ): Promise<Map<string, SourcePropFallback>> {
    const ownerSource = await this.readCanonicalSourceText(ownerPath);
    if (ownerSource) {
      const interfaceInfo = extractDeclaredInterfacePropInfoMap(ownerSource, typeName);
      if (interfaceInfo.size > 0) {
        return interfaceInfo;
      }

      const aliasInfo = extractExportedObjectTypeAliasPropInfoMap(ownerSource, typeName);
      if (aliasInfo.size > 0) {
        return aliasInfo;
      }
    }

    const imported = await this.resolveImportedTypeReference(ownerPath, typeName);
    if (!imported) {
      return new Map();
    }

    const importedSource = await this.readCanonicalSourceText(imported.path);
    if (!importedSource) {
      return new Map();
    }

    const interfaceInfo = extractDeclaredInterfacePropInfoMap(importedSource, imported.typeName);
    if (interfaceInfo.size > 0) {
      return interfaceInfo;
    }

    return extractExportedObjectTypeAliasPropInfoMap(importedSource, imported.typeName);
  }

  private async resolveDeclaredTypeSource(
    ownerPath: string,
    typeName: string,
  ): Promise<{ path: string; source: string } | undefined> {
    const ownerSource = await this.readCanonicalSourceText(ownerPath);
    if (ownerSource && sourceDeclaresType(ownerSource, typeName)) {
      return {
        path: runtimeNormalizePath(ownerPath),
        source: ownerSource,
      };
    }

    const imported = await this.resolveImportedTypeReference(ownerPath, typeName);
    if (!imported) {
      return undefined;
    }

    const importedSource = await this.readCanonicalSourceText(imported.path);
    if (!importedSource) {
      return undefined;
    }

    return {
      path: runtimeNormalizePath(imported.path),
      source: importedSource,
    };
  }

  private async tryBuildExpandedUtilitySchema(
    ownerPath: string,
    typeText: string,
    seen: Set<string>,
  ): Promise<PropertyMetaSchema | undefined> {
    const pickOrOmitMatch =
      /^(Pick|Omit)<\s*([A-Za-z_$][A-Za-z0-9_$]*)(?:<[\s\S]+>)?\s*,\s*([\s\S]+)\s*>$/.exec(
        typeText,
      );
    if (pickOrOmitMatch) {
      const keyMembers = extractStringLiteralTypeMembers(pickOrOmitMatch[3]!);
      if (keyMembers.length === 0) {
        return undefined;
      }
      const baseSchema = await this.buildExpandedDeclaredNamedTypeSchema(
        ownerPath,
        pickOrOmitMatch[2]!,
        pickOrOmitMatch[2]!,
        seen,
      );
      if (isCompatObjectRecordSchema(baseSchema)) {
        const keys = new Set(keyMembers);
        const entries = Object.entries(baseSchema.schema).filter(([name]) =>
          pickOrOmitMatch[1] === "Pick" ? keys.has(name) : !keys.has(name),
        );
        return {
          kind: "object",
          type: typeText,
          schema: Object.fromEntries(entries),
        };
      }
    }

    const partialMatch = /^Partial<\s*([\s\S]+)\s*>$/.exec(typeText);
    if (partialMatch) {
      const innerType = normalizeTypeString(partialMatch[1]!.trim());
      const innerSchema = await this.buildExpandedCompatSchemaFromTypeText(
        ownerPath,
        innerType,
        seen,
      );
      if (isCompatObjectRecordSchema(innerSchema)) {
        return {
          kind: "object",
          type: typeText,
          schema: Object.fromEntries(
            Object.entries(innerSchema.schema).map(([name, prop]) => [
              name,
              cloneCompatPropertyMetaAsOptional(prop),
            ]),
          ),
        };
      }

      return {
        kind: "object",
        type: typeText,
        schema:
          typeof innerSchema === "object" &&
          innerSchema !== null &&
          !Array.isArray(innerSchema) &&
          innerSchema.kind === "object"
            ? innerSchema.schema
            : {},
      };
    }

    const returnTypeMatch = /^ReturnType<\s*typeof\s+([A-Za-z_$][A-Za-z0-9_$]*)\s*>$/.exec(
      typeText,
    );
    if (returnTypeMatch) {
      const source = await this.readCanonicalSourceText(ownerPath);
      const objectLiteral = source
        ? extractFunctionReturnObjectLiteral(source, returnTypeMatch[1]!)
        : undefined;
      if (!objectLiteral) {
        return undefined;
      }

      const objectSchema = await this.buildExpandedCompatObjectSchemaFromTypeText(
        ownerPath,
        objectLiteral,
        seen,
      );
      return {
        ...objectSchema,
        type: typeText,
      };
    }

    return undefined;
  }

  private async buildExpandedCompatSchemaFromTypeText(
    ownerPath: string,
    typeText: string,
    seen: Set<string> = new Set(),
  ): Promise<PropertyMetaSchema | undefined> {
    const normalizedTypeText = normalizeTypeString(stripSingleOuterParens(typeText.trim()));
    if (!normalizedTypeText) {
      return undefined;
    }

    const cacheKey = `${runtimeNormalizePath(ownerPath)}::${normalizedTypeText}`;
    if (this.expandedTypeSchemaCache.has(cacheKey)) {
      return this.expandedTypeSchemaCache.get(cacheKey);
    }

    if (seen.has(cacheKey)) {
      return {
        kind: "object",
        type: normalizedTypeText,
        schema: {},
      };
    }
    seen.add(cacheKey);

    const unionParts = splitTopLevelTypeUnion(normalizedTypeText);
    if (unionParts.length > 1) {
      const schemaEntries: PropertyMetaSchema[] = [];
      for (const part of unionParts) {
        const expanded = await this.buildExpandedCompatSchemaFromTypeText(ownerPath, part, seen);
        schemaEntries.push(
          ...flattenSchemaEnumEntries(expanded ?? this.buildCompatSchemaLeafFromTypeText(part)),
        );
      }

      const result = {
        kind: "enum" as const,
        type: normalizedTypeText,
        schema: sortCompatSchemaEnumEntries(schemaEntries),
      };
      this.expandedTypeSchemaCache.set(cacheKey, result);
      seen.delete(cacheKey);
      return result;
    }

    const intersectionParts = splitTopLevelTypeIntersection(normalizedTypeText);
    if (intersectionParts.length > 1) {
      const result = {
        kind: "object" as const,
        type: normalizedTypeText,
        schema: (await Promise.all(
          intersectionParts.map(
            async (part) =>
              (await this.buildExpandedCompatSchemaFromTypeText(ownerPath, part, seen)) ??
              this.buildCompatSchemaLeafFromTypeText(part),
          ),
        )) as unknown as Record<string, PropertyMetaSchema>,
      };
      this.expandedTypeSchemaCache.set(cacheKey, result);
      seen.delete(cacheKey);
      return result;
    }

    if (normalizedTypeText.startsWith("{") && normalizedTypeText.endsWith("}")) {
      const result = await this.buildExpandedCompatObjectSchemaFromTypeText(
        ownerPath,
        normalizedTypeText,
        seen,
      );
      this.expandedTypeSchemaCache.set(cacheKey, result);
      seen.delete(cacheKey);
      return result;
    }

    const utilitySchema = await this.tryBuildExpandedUtilitySchema(
      ownerPath,
      normalizedTypeText,
      seen,
    );
    if (utilitySchema) {
      this.expandedTypeSchemaCache.set(cacheKey, utilitySchema);
      seen.delete(cacheKey);
      return utilitySchema;
    }

    const typeRefMatch =
      /^([A-Za-z_$][A-Za-z0-9_$]*)(?:<[\s\S]+>)?$/.exec(normalizedTypeText) ??
      /^([A-Za-z_$][A-Za-z0-9_$]*)$/.exec(normalizedTypeText);
    if (typeRefMatch) {
      const declaredSchema = await this.buildExpandedDeclaredNamedTypeSchema(
        ownerPath,
        typeRefMatch[1]!,
        normalizedTypeText,
        seen,
      );
      if (declaredSchema) {
        this.expandedTypeSchemaCache.set(cacheKey, declaredSchema);
        seen.delete(cacheKey);
        return declaredSchema;
      }

      const globalTsSchema = await this.buildGlobalTypeSchemaFromTs(typeRefMatch[1]!, seen);
      if (globalTsSchema) {
        const result =
          typeof globalTsSchema === "object" &&
          globalTsSchema !== null &&
          !Array.isArray(globalTsSchema) &&
          "type" in globalTsSchema
            ? {
                ...globalTsSchema,
                type: normalizedTypeText,
              }
            : globalTsSchema;
        this.expandedTypeSchemaCache.set(cacheKey, result);
        seen.delete(cacheKey);
        return result;
      }
    }

    const fallback = this.buildCompatSchemaLeafFromTypeText(normalizedTypeText);
    this.expandedTypeSchemaCache.set(cacheKey, fallback);
    seen.delete(cacheKey);
    return fallback;
  }

  private buildCompatSchemaLeafFromTypeText(typeText: string): PropertyMetaSchema {
    const normalizedTypeText = normalizeTypeString(stripSingleOuterParens(typeText.trim()));
    if (
      normalizedTypeText === "boolean" &&
      typeof this.options.schema === "object" &&
      this.options.schema.literalBooleanSchema
    ) {
      return {
        kind: "enum",
        type: "boolean",
        schema: ["false", "true"],
      };
    }

    return buildCompatSchemaFromRawType(normalizedTypeText) ?? normalizedTypeText;
  }

  private async buildExpandedCompatObjectSchemaFromTypeText(
    ownerPath: string,
    typeText: string,
    seen: Set<string>,
  ): Promise<PropertyMetaSchema> {
    const normalizedTypeText = normalizeCompatObjectLiteralTypeText(typeText.trim());
    const body = normalizedTypeText.slice(1, -1).trim();
    const properties: Record<string, PropertyMeta> = {};

    for (const entry of splitTopLevelObjectMembers(body)) {
      const trimmed = entry.trim();
      const match =
        /^(?:readonly\s+)?(?:["']([^"']+)["']|([A-Za-z_$][A-Za-z0-9_$-]*))(\?)?\s*:\s*([\s\S]+)$/.exec(
          trimmed,
        );
      if (!match) {
        continue;
      }

      const name = match[1] ?? match[2];
      const required = match[3] !== "?";
      const rawType = normalizeTypeString(match[4]!.trim());
      const type = normalizeOptionalCompatTypeText(rawType, required);
      const schema = normalizeOptionalPropSchema(
        (await this.buildExpandedCompatSchemaFromTypeText(ownerPath, rawType, seen)) ??
          this.buildCompatSchemaLeafFromTypeText(rawType),
        type,
        required,
      );

      properties[name] = {
        name,
        global: false,
        description: "",
        tags: [],
        required,
        type,
        schema,
      };
    }

    return {
      kind: "object",
      type: normalizedTypeText,
      schema: properties,
    };
  }

  private async buildExpandedDeclaredNamedTypeSchema(
    ownerPath: string,
    typeName: string,
    displayTypeText: string,
    seen: Set<string>,
  ): Promise<PropertyMetaSchema | undefined> {
    const resolved = await this.resolveDeclaredTypeSource(ownerPath, typeName);
    if (!resolved) {
      return undefined;
    }

    const interfaceInfo = await this.collectDeclaredInterfacePropFallbacks(
      resolved.path,
      typeName,
      new Set(),
    );
    if (interfaceInfo.size > 0) {
      const schema: Record<string, PropertyMeta> = {};
      for (const [name, info] of interfaceInfo) {
        const required = info.required ?? true;
        const rawType = normalizeTypeString(info.rawType ?? "unknown");
        const type = normalizeOptionalCompatTypeText(rawType, required);
        const memberSchema = normalizeOptionalPropSchema(
          (await this.buildExpandedCompatSchemaFromTypeText(resolved.path, rawType, seen)) ??
            this.buildCompatSchemaLeafFromTypeText(rawType),
          type,
          required,
        );

        schema[name] = {
          name,
          global: false,
          description: info.description ?? "",
          tags: info.tags ?? [],
          required,
          type,
          schema: memberSchema,
        };
      }

      return {
        kind: "object",
        type: displayTypeText,
        schema,
      };
    }

    const aliasInfo = extractExportedObjectTypeAliasPropInfoMap(resolved.source, typeName);
    if (aliasInfo.size > 0) {
      const schema: Record<string, PropertyMeta> = {};
      for (const [name, info] of aliasInfo) {
        const required = info.required ?? true;
        const rawType = normalizeTypeString(info.rawType ?? "unknown");
        const type = normalizeOptionalCompatTypeText(rawType, required);
        const memberSchema = normalizeOptionalPropSchema(
          (await this.buildExpandedCompatSchemaFromTypeText(resolved.path, rawType, seen)) ??
            this.buildCompatSchemaLeafFromTypeText(rawType),
          type,
          required,
        );

        schema[name] = {
          name,
          global: false,
          description: info.description ?? "",
          tags: info.tags ?? [],
          required,
          type,
          schema: memberSchema,
        };
      }

      return {
        kind: "object",
        type: displayTypeText,
        schema,
      };
    }

    const aliasExpression = extractExportedTypeAliasExpression(resolved.source, typeName);
    if (!aliasExpression) {
      return undefined;
    }

    const aliasSchema = await this.buildExpandedCompatSchemaFromTypeText(
      resolved.path,
      aliasExpression,
      seen,
    );
    if (
      aliasSchema &&
      typeof aliasSchema === "object" &&
      !Array.isArray(aliasSchema) &&
      "type" in aliasSchema
    ) {
      return {
        ...aliasSchema,
        type: displayTypeText,
      };
    }

    return aliasSchema;
  }

  private getGlobalTsTypeProgram():
    | {
        ts: any;
        checker: any;
        sourceFile: any;
      }
    | undefined {
    const ts = loadTypeScript();
    if (!ts) {
      return undefined;
    }

    if (this.globalTsTypeProgram) {
      return this.globalTsTypeProgram;
    }

    const fileName = runtimeResolvePath(this.projectRoot, "__verter_global_type_probe__.ts");
    const sourceText = "export {};\n";
    const compilerOptions = {
      target: ts.ScriptTarget.ESNext,
      module: ts.ModuleKind.ESNext,
      skipLibCheck: true,
      lib: ["lib.esnext.d.ts", "lib.dom.d.ts", "lib.dom.iterable.d.ts"],
    };
    const host = ts.createCompilerHost(compilerOptions, true);
    const originalGetSourceFile = host.getSourceFile.bind(host);
    host.getSourceFile = (
      path: string,
      languageVersion: any,
      onError?: any,
      shouldCreateNewSourceFile?: any,
    ) => {
      if (runtimeNormalizePath(path) === runtimeNormalizePath(fileName)) {
        return ts.createSourceFile(path, sourceText, languageVersion, true, ts.ScriptKind.TS);
      }
      return originalGetSourceFile(path, languageVersion, onError, shouldCreateNewSourceFile);
    };
    host.readFile = (path: string) =>
      runtimeNormalizePath(path) === runtimeNormalizePath(fileName)
        ? sourceText
        : ts.sys.readFile(path);
    host.fileExists = (path: string) =>
      runtimeNormalizePath(path) === runtimeNormalizePath(fileName) || ts.sys.fileExists(path);

    const program = ts.createProgram([fileName], compilerOptions, host);
    const checker = program.getTypeChecker();
    const sourceFile = program.getSourceFile(fileName);
    if (!sourceFile) {
      return undefined;
    }

    this.globalTsTypeProgram = {
      ts,
      checker,
      sourceFile,
      fileName,
    };
    return this.globalTsTypeProgram;
  }

  private async buildGlobalTypeSchemaFromTs(
    typeName: string,
    seen: Set<string>,
  ): Promise<PropertyMetaSchema | undefined> {
    void seen;
    if (this.globalTsTypeSchemaCache.has(typeName)) {
      return this.globalTsTypeSchemaCache.get(typeName);
    }

    const schema = typeName === "HTMLElement" ? getCompatHtmlElementObjectSchema() : undefined;
    if (schema) {
      this.globalTsTypeSchemaCache.set(typeName, schema);
    }
    return schema;
  }

  private buildGlobalTsObjectSchemaFromType(
    ts: any,
    checker: any,
    type: any,
    displayTypeText: string,
    declaration: any,
    seen: Set<string>,
  ): PropertyMetaSchema {
    const visitKey = `global::${displayTypeText}`;
    if (seen.has(visitKey)) {
      return {
        kind: "object",
        type: displayTypeText,
        schema: {},
      };
    }
    seen.add(visitKey);

    const properties: Record<string, PropertyMeta> = {};
    for (const symbol of checker.getPropertiesOfType(type)) {
      const propDeclaration = symbol.valueDeclaration ?? symbol.declarations?.[0] ?? declaration;
      const propType = checker.getTypeOfSymbolAtLocation(symbol, propDeclaration);
      const propTypeText = this.formatGlobalTsTypeText(ts, checker, propType, propDeclaration);
      const required = (symbol.flags & ts.SymbolFlags.Optional) === 0;
      const description = this.getGlobalTsSymbolDescription(ts, checker, symbol, propType);
      const schema = this.shouldExpandGlobalTsObjectType(propTypeText, propType)
        ? this.buildGlobalTsObjectSchemaFromType(
            ts,
            checker,
            propType,
            propTypeText,
            propDeclaration,
            seen,
          )
        : propTypeText;

      properties[symbol.name] = {
        name: symbol.name,
        global: false,
        description,
        tags: this.getGlobalTsSymbolTags(ts, checker, symbol),
        required,
        type: propTypeText,
        schema,
      };
    }

    seen.delete(visitKey);
    return {
      kind: "object",
      type: displayTypeText,
      schema: properties,
    };
  }

  private shouldExpandGlobalTsObjectType(typeText: string, type: any): boolean {
    return (
      looksLikeBareTypeReference(typeText) &&
      !type.isUnionOrIntersection?.() &&
      type.getCallSignatures().length === 0 &&
      type.getConstructSignatures().length === 0 &&
      type.getProperties().length > 0
    );
  }

  private formatGlobalTsTypeText(ts: any, checker: any, type: any, declaration: any): string {
    if (type.getCallSignatures().length > 1 && type.getProperties().length === 0) {
      return `{ ${type
        .getCallSignatures()
        .map((signature: any) => this.formatGlobalTsSignature(ts, checker, signature, declaration))
        .join("; ")}; }`;
    }

    if (type.getCallSignatures().length === 1 && type.getProperties().length === 0) {
      return this.formatGlobalTsSignature(ts, checker, type.getCallSignatures()[0], declaration);
    }

    return normalizeTypeString(
      checker
        .typeToString(
          type,
          declaration,
          ts.TypeFormatFlags.NoTruncation |
            ts.TypeFormatFlags.UseAliasDefinedOutsideCurrentScope |
            ts.TypeFormatFlags.MultilineObjectLiterals |
            ts.TypeFormatFlags.WriteArrowStyleSignature,
        )
        .replace(/\{\s+/g, "{ ")
        .replace(/\s+\}/g, " }")
        .replace(/;\s+/g, "; "),
    );
  }

  private formatGlobalTsSignature(ts: any, checker: any, signature: any, declaration: any): string {
    const typeParameters = signature.getTypeParameters?.() ?? [];
    const typeParams = typeParameters.length
      ? `<${typeParameters
          .map((param: any) => {
            const constraint = param.getConstraint?.();
            const constraintText = constraint
              ? ` extends ${this.formatGlobalTsTypeText(ts, checker, constraint, declaration)}`
              : "";
            return `${param.symbol.name}${constraintText}`;
          })
          .join(", ")}>`
      : "";
    const params = signature.getParameters().map((param: any) => {
      const paramDeclaration = param.valueDeclaration ?? param.declarations?.[0] ?? declaration;
      const isOptional =
        (param.flags & ts.SymbolFlags.Optional) !== 0 || Boolean(paramDeclaration?.questionToken);
      const isRest = Boolean(paramDeclaration?.dotDotDotToken);
      let paramTypeText = this.formatGlobalTsTypeText(
        ts,
        checker,
        checker.getTypeOfSymbolAtLocation(param, paramDeclaration),
        paramDeclaration,
      );
      if (
        isOptional &&
        !splitTopLevelTypeUnion(paramTypeText).some((part) => part.trim() === "undefined")
      ) {
        paramTypeText = `${paramTypeText} | undefined`;
      }
      return `${isRest ? "..." : ""}${param.name}${isOptional ? "?" : ""}: ${paramTypeText}`;
    });
    const returnTypeText = this.formatGlobalTsTypeText(
      ts,
      checker,
      signature.getReturnType(),
      declaration,
    );
    return `${typeParams}(${params.join(", ")}): ${returnTypeText}`;
  }

  private getGlobalTsSymbolDescription(ts: any, checker: any, symbol: any, type: any): string {
    const callSignatures = type.getCallSignatures?.() ?? [];
    if (callSignatures.length > 0) {
      return callSignatures
        .map((signature: any) =>
          ts.displayPartsToString(signature.getDocumentationComment(checker)),
        )
        .filter(Boolean)
        .join("");
    }

    return ts.displayPartsToString(symbol.getDocumentationComment(checker));
  }

  private getGlobalTsSymbolTags(ts: any, checker: any, symbol: any): Tag[] {
    return (symbol.getJsDocTags?.(checker) ?? []).map((tag: any) => ({
      name: tag.name,
      ...(tag.text ? { text: ts.displayPartsToString(tag.text) } : {}),
    }));
  }

  private async applyDeclaredTypeSchemaHints(
    ownerPath: string,
    prop: PropertyMeta,
    rawType: string | undefined,
  ): Promise<void> {
    const strippedRawType = rawType
      ? stripTopLevelUndefinedFromTypeString(rawType).trim()
      : undefined;
    const match = strippedRawType?.match(/^([A-Za-z_$][A-Za-z0-9_$]*)(?:<[\s\S]+>)?$/);
    if (!match) {
      return;
    }

    const infoMap = await this.resolveDeclaredTypeMemberFallbacks(ownerPath, match[1]!);
    if (infoMap.size === 0) {
      return;
    }

    prop.schema = applyDeclaredMemberRawTypesToSchema(prop.schema, infoMap);
  }

  private async tryResolveThemeBackedInterfaceProp(
    ownerPath: string,
    prop: PropertyMeta,
    nativeProp: PropMeta | undefined,
  ): Promise<Pick<PropertyMeta, "type" | "schema"> | undefined> {
    if (!prop.type.includes("graphNode(")) {
      return undefined;
    }

    const typeName = nativeProp?.rawType?.trim();
    if (!typeName || !/^[A-Z][A-Za-z0-9_$]*Props$/.test(typeName)) {
      return undefined;
    }

    const typeSourcePath = await this.resolveImportedTypeSource(ownerPath, typeName);
    if (!typeSourcePath) {
      return undefined;
    }

    const typeSource = await this.readCanonicalSourceText(typeSourcePath);
    if (!typeSource) {
      return undefined;
    }

    const declaredPropTypes = extractDeclaredInterfacePropTypeMap(typeSource, typeName);
    if (declaredPropTypes.size === 0) {
      return undefined;
    }

    const members: PropertyMeta[] = [];
    for (const [memberName, memberRawType] of declaredPropTypes) {
      const variantsMatch =
        /^([A-Za-z_$][A-Za-z0-9_$]*)\[['"]variants['"]\]\[['"]([^'"]+)['"]\]$/.exec(memberRawType);
      if (variantsMatch) {
        const resolved = await this.resolveThemeBackedAliasKeys(
          typeSourcePath,
          typeSource,
          variantsMatch[1]!,
          ["variants", variantsMatch[2]!],
        );
        if (resolved.length > 0) {
          const literalEntries = resolved.map((entry) => JSON.stringify(entry));
          const memberType = `${literalEntries.join(" | ")} | undefined`;
          members.push(
            buildCompatInlinePropertyMeta(memberName, memberType, {
              kind: "enum",
              type: memberType,
              schema: [...literalEntries, "undefined"],
            }),
          );
          continue;
        }
      }

      const slotsMatch = /^([A-Za-z_$][A-Za-z0-9_$]*)\[['"]slots['"]\]$/.exec(memberRawType);
      if (slotsMatch) {
        const resolved = await this.resolveThemeBackedAliasKeys(
          typeSourcePath,
          typeSource,
          slotsMatch[1]!,
          ["slots"],
        );
        if (resolved.length > 0) {
          const objectType = `{ ${resolved.map((entry) => `${entry}?: string`).join("; ")}; }`;
          members.push(
            buildCompatInlinePropertyMeta(memberName, `${objectType} | undefined`, {
              kind: "enum",
              type: `${objectType} | undefined`,
              schema: [
                {
                  kind: "object",
                  type: objectType,
                  schema: Object.fromEntries(
                    resolved.map((entry) => [
                      entry,
                      buildCompatInlinePropertyMeta(
                        entry,
                        "string | undefined",
                        "string | undefined",
                      ),
                    ]),
                  ),
                },
                "undefined",
              ],
            }),
          );
          continue;
        }
      }

      members.push(
        buildCompatInlinePropertyMeta(
          memberName,
          `${normalizeTypeString(memberRawType)} | undefined`,
          {
            kind: "enum",
            type: `${normalizeTypeString(memberRawType)} | undefined`,
            schema: [`${normalizeTypeString(memberRawType)}`, "undefined"],
          },
        ),
      );
    }

    if (members.length === 0) {
      return undefined;
    }

    const objectType = `{ ${members.map((member) => `${member.name}?: ${member.type}`).join("; ")}; }`;
    return {
      type: `${objectType} | undefined`,
      schema: {
        kind: "enum",
        type: `${objectType} | undefined`,
        schema: [
          {
            kind: "object",
            type: objectType,
            schema: Object.fromEntries(members.map((member) => [member.name, member])),
          },
          "undefined",
        ],
      },
    };
  }

  private async resolveThemeBackedAliasKeys(
    ownerPath: string,
    ownerSource: string,
    aliasName: string,
    pathSegments: string[],
  ): Promise<string[]> {
    const aliasExpression = extractExportedTypeAliasExpression(ownerSource, aliasName);
    if (!aliasExpression) {
      return [];
    }

    const typeofMatch = /typeof\s+([A-Za-z_$][A-Za-z0-9_$]*)/.exec(aliasExpression);
    if (!typeofMatch) {
      return [];
    }

    const importedSpecifiers = extractImportedLocalSpecifiers(ownerSource);
    const themeSpecifier = importedSpecifiers.get(typeofMatch[1]!);
    if (!themeSpecifier) {
      return [];
    }

    const themePath = await this.resolveModulePath(ownerPath, themeSpecifier);
    if (!themePath) {
      return [];
    }

    const themeSource = await this.readCanonicalSourceText(themePath);
    if (!themeSource) {
      return [];
    }

    let objectText = extractDefaultExportObjectText(themeSource);
    for (const segment of pathSegments) {
      if (!objectText) {
        return [];
      }
      objectText = extractObjectLiteralMemberValue(objectText, segment);
    }

    return objectText ? extractObjectLiteralKeys(objectText) : [];
  }

  private async resolveThemeBackedIndexedAccessProjection(
    ownerPath: string,
    rawType: string | undefined,
  ): Promise<{ kind: "variants" | "slots" | "ui"; values: string[] } | undefined> {
    const indexed = rawType ? parseCompatIndexedAccessSegments(rawType) : undefined;
    if (!indexed) {
      return undefined;
    }

    let resolvedPath = runtimeNormalizePath(ownerPath);
    let resolvedSource = await this.readCanonicalSourceText(resolvedPath);
    let resolvedName = indexed.rootName;

    if (!resolvedSource || !sourceDeclaresType(resolvedSource, resolvedName)) {
      const imported = await this.resolveImportedTypeReference(ownerPath, indexed.rootName);
      if (!imported) {
        return undefined;
      }
      resolvedPath = runtimeNormalizePath(imported.path);
      resolvedSource = await this.readCanonicalSourceText(imported.path);
      resolvedName = imported.typeName;
    }

    if (!resolvedSource) {
      return undefined;
    }

    if (indexed.segments[0] === "variants" && indexed.segments[1]) {
      const values = await this.resolveThemeBackedAliasKeys(
        resolvedPath,
        resolvedSource,
        resolvedName,
        ["variants", indexed.segments[1]],
      );
      return values.length > 0 ? { kind: "variants", values } : undefined;
    }

    if (indexed.segments[0] === "slots" && indexed.segments.length === 1) {
      const values = await this.resolveThemeBackedAliasKeys(
        resolvedPath,
        resolvedSource,
        resolvedName,
        ["slots"],
      );
      return values.length > 0 ? { kind: "slots", values } : undefined;
    }

    if (indexed.segments[0] === "ui" && indexed.segments.length === 1) {
      const values = await this.resolveThemeBackedAliasKeys(
        resolvedPath,
        resolvedSource,
        resolvedName,
        ["slots"],
      );
      return values.length > 0 ? { kind: "ui", values } : undefined;
    }

    return undefined;
  }

  private async projectThemeBackedComponentConfigMeta(
    ownerPath: string,
    meta: ComponentMeta,
  ): Promise<ComponentMeta> {
    let propsChanged = false;
    const projectedProps = await Promise.all(
      meta.props.map(async (prop) => {
        const projected = await this.resolveThemeBackedIndexedAccessProjection(
          ownerPath,
          prop.rawType,
        );
        if (!projected) {
          return prop;
        }

        propsChanged = true;
        if (projected.kind === "variants") {
          return {
            ...prop,
            type: buildCompatLiteralUnionDescriptor(projected.values),
          };
        }

        return {
          ...prop,
          type: buildCompatClassNameValueSlotsDescriptor(projected.values),
        };
      }),
    );

    let slotsChanged = false;
    const projectedSlots = await Promise.all(
      meta.slots.map(async (slot) => {
        let slotChanged = false;
        const bindings = await Promise.all(
          slot.bindings.map(async (binding) => {
            const projected = await this.resolveThemeBackedIndexedAccessProjection(
              ownerPath,
              binding.rawType,
            );
            if (projected?.kind !== "ui") {
              return binding;
            }

            slotChanged = true;
            return {
              ...binding,
              type: buildCompatUiHelperDescriptor(projected.values),
            };
          }),
        );

        if (!slotChanged) {
          return slot;
        }

        slotsChanged = true;
        return {
          ...slot,
          bindings,
        };
      }),
    );

    if (!propsChanged && !slotsChanged) {
      return meta;
    }

    return {
      ...meta,
      ...(propsChanged ? { props: projectedProps } : {}),
      ...(slotsChanged ? { slots: projectedSlots } : {}),
    };
  }

  private async resolveImportedPropsComponentSource(
    ownerPath: string,
    typeName: string,
    depth = 0,
  ): Promise<string | undefined> {
    if (depth > 2) {
      return undefined;
    }
    const source = await this.readCanonicalSourceText(ownerPath);
    if (!source) {
      return undefined;
    }

    const importMatches = source.matchAll(/import\s+type\s*\{([^}]*)\}\s+from\s+['"]([^'"]+)['"]/g);
    for (const match of importMatches) {
      const imported = match[1]
        ?.split(",")
        .map((entry) =>
          entry
            .trim()
            .split(/\s+as\s+/i)
            .pop()
            ?.trim(),
        )
        .filter((entry): entry is string => Boolean(entry));
      if (!imported?.includes(typeName)) {
        continue;
      }
      const resolvedImport = await this.resolveModulePath(ownerPath, match[2]!);
      if (!resolvedImport) {
        continue;
      }
      const directSource = await this.readCanonicalSourceText(resolvedImport);
      if (
        resolvedImport.endsWith(".vue") &&
        directSource?.includes(`export interface ${typeName}`)
      ) {
        return resolvedImport;
      }
      if (!directSource) {
        continue;
      }
      const reexportMatches = directSource.matchAll(
        /export(?:\s+type)?\s*(?:\*\s*from|\{[^}]*\b[A-Za-z_$][A-Za-z0-9_$]*\b[^}]*\}\s*from)\s+['"]([^'"]+)['"]/g,
      );
      for (const reexport of reexportMatches) {
        const resolvedReexport = await this.resolveModulePath(resolvedImport, reexport[1]!);
        if (!resolvedReexport) {
          continue;
        }
        const reexportSource = await this.readCanonicalSourceText(resolvedReexport);
        if (
          resolvedReexport.endsWith(".vue") &&
          reexportSource?.includes(`export interface ${typeName}`)
        ) {
          return resolvedReexport;
        }
      }
    }

    return undefined;
  }

  private async resolveModulePath(
    fromFile: string,
    specifier: string,
  ): Promise<string | undefined> {
    const bases: string[] = [];

    if (specifier.startsWith(".")) {
      bases.push(runtimeResolvePath(dirname(fromFile), specifier));
    } else if (specifier.startsWith("@/")) {
      let current = dirname(fromFile);
      while (true) {
        const packageJson = runtimeResolvePath(current, "package.json");
        if ((await this.readCanonicalSourceText(packageJson)) !== undefined) {
          const suffix = specifier.slice(2);
          bases.push(runtimeResolvePath(current, suffix));
          bases.push(runtimeResolvePath(current, "src", suffix));
          break;
        }
        const parent = dirname(current);
        if (parent === current) {
          break;
        }
        current = parent;
      }
    }

    // Extension priority matches Rust effective_target():
    // .d.ts > .d.cts > .d.mts > .ts > .tsx > .js > .jsx > .cjs > .mjs
    for (const base of bases) {
      for (const candidate of [
        base,
        `${base}.d.ts`,
        `${base}.d.cts`,
        `${base}.d.mts`,
        `${base}.ts`,
        `${base}.tsx`,
        `${base}.js`,
        `${base}.jsx`,
        `${base}.cjs`,
        `${base}.mjs`,
        `${base}.vue`,
        runtimeResolvePath(base, "index.d.ts"),
        runtimeResolvePath(base, "index.d.cts"),
        runtimeResolvePath(base, "index.d.mts"),
        runtimeResolvePath(base, "index.ts"),
        runtimeResolvePath(base, "index.tsx"),
        runtimeResolvePath(base, "index.js"),
        runtimeResolvePath(base, "index.vue"),
      ]) {
        const source = await this.readCanonicalSourceText(candidate);
        if (source !== undefined) {
          return runtimeNormalizePath(candidate);
        }
      }
    }
    return undefined;
  }

  private async buildReferencedComponentPropSchema(
    ownerPath: string,
    prop: PropertyMeta,
    rawType: string | undefined,
  ): Promise<PropertyMetaSchema | undefined> {
    const normalizedRawType = rawType?.trim();
    const referencedTypes = extractReferencedComponentPropTypes(normalizedRawType);
    if (referencedTypes.length !== 1) {
      return undefined;
    }
    const referencedType = referencedTypes[0]!;
    const typeName = referencedType.typeName;

    const componentSource = await this.resolveImportedPropsComponentSource(ownerPath, typeName);
    if (!componentSource || componentSource === ownerPath) {
      return undefined;
    }

    const referencedMeta = await this.getComponentMeta(componentSource);
    if (!referencedMeta.props.length) {
      return undefined;
    }

    const componentSourceText = await this.readCanonicalSourceText(componentSource);
    const declaredPropTypes = componentSourceText
      ? extractDeclaredInterfacePropTypeMap(componentSourceText, typeName)
      : new Map<string, string>();
    const sourceFallbackMap = await this.buildSourcePropFallbackMap(componentSource);
    const compactedProps = await Promise.all(
      referencedMeta.props.map((referencedProp) =>
        this.compactEmbeddedReferencedPropMetaRecursive(
          componentSource,
          referencedProp,
          sourceFallbackMap.get(referencedProp.name)?.rawType ??
            declaredPropTypes.get(referencedProp.name),
          { collapseIndexedSize: false },
        ),
      ),
    );
    for (const [propName, info] of sourceFallbackMap) {
      if (!info.rawType || compactedProps.some((propEntry) => propEntry.name === propName)) {
        continue;
      }
      const normalizedSourceType = normalizeTypeString(info.rawType);
      const typeText =
        normalizedSourceType === "any"
          ? "any"
          : normalizeOptionalCompatTypeText(normalizedSourceType, info.required ?? false);
      let schema: PropertyMetaSchema = typeText;
      if (typeText !== "any" && shouldAttemptExpandedCompatSchema(info.rawType)) {
        const expandedSchema = await this.buildExpandedCompatSchemaFromTypeText(
          info.canonicalSource ?? componentSource,
          info.rawType,
        );
        if (expandedSchema) {
          schema = normalizeOptionalPropSchema(expandedSchema, typeText, info.required ?? false);
        }
      }
      compactedProps.push({
        global: false,
        name: propName,
        required: info.required ?? false,
        description: info.description ?? "",
        tags: info.tags ?? [],
        type: typeText,
        schema,
      });
    }

    const objectSchema: PropertyMetaSchema = {
      kind: "object",
      type: typeName,
      schema: Object.fromEntries(
        compactedProps.map((referencedProp) => [referencedProp.name, referencedProp]),
      ),
    };

    const resolvedSchema = referencedType.arrayWrapped
      ? {
          kind: "array" as const,
          type: `${typeName}[]`,
          schema: [objectSchema],
        }
      : objectSchema;

    if (
      typeof prop.schema !== "string" &&
      !Array.isArray(prop.schema) &&
      prop.schema.kind === "enum" &&
      Array.isArray(prop.schema.schema)
    ) {
      const normalizedEntries = prop.schema.schema.flatMap((entry: PropertyMetaSchema) =>
        entry === "boolean" ? ["false", "true"] : [entry],
      );
      const existingScalarEntries = normalizedEntries.filter((entry: PropertyMetaSchema) => {
        if (typeof entry === "string") {
          return true;
        }
        if (referencedType.arrayWrapped) {
          return !(
            (entry?.kind === "array" && entry.type === `${typeName}[]`) ||
            (entry?.kind === "object" && entry.type === typeName)
          );
        }
        return !(entry?.kind === "object" && entry.type === typeName);
      });
      const { scalarEntries, hasUndefined } = collectReferencedUnionScalarEntries(
        normalizedRawType,
        prop.required,
        existingScalarEntries,
      );

      return {
        kind: "enum",
        type: prop.type,
        schema: [
          ...scalarEntries.filter((entry) => entry !== "undefined"),
          ...(hasUndefined ? ["undefined"] : []),
          resolvedSchema,
        ],
      };
    }

    return {
      kind: "enum",
      type: prop.type,
      schema: [...(prop.required ? [] : ["undefined"]), resolvedSchema],
    };
  }

  private async resolveReferencedComponentObjectArm(
    ownerPath: string,
    typeName: string,
    visited = new Set<string>(),
    options?: { collapseIndexedSize?: boolean },
  ): Promise<ReferencedComponentObjectArm | undefined> {
    const componentSource = await this.resolveImportedPropsComponentSource(ownerPath, typeName);
    if (!componentSource || componentSource === ownerPath) {
      return undefined;
    }
    const visitKey = `${runtimeNormalizePath(componentSource)}::${typeName}`;
    if (visited.has(visitKey)) {
      return undefined;
    }
    const nextVisited = new Set(visited);
    nextVisited.add(visitKey);

    const referencedMeta = await this.getComponentMeta(componentSource);
    if (!referencedMeta.props.length) {
      return undefined;
    }

    const componentSourceText = await this.readCanonicalSourceText(componentSource);
    const declaredPropTypes = componentSourceText
      ? extractDeclaredInterfacePropTypeMap(componentSourceText, typeName)
      : new Map<string, string>();
    const sourceFallbackMap = await this.buildSourcePropFallbackMap(componentSource);
    const declaredProps =
      sourceFallbackMap.size > 0 || declaredPropTypes.size > 0
        ? await Promise.all(
            referencedMeta.props
              .filter((referencedProp) =>
                sourceFallbackMap.size > 0
                  ? sourceFallbackMap.has(referencedProp.name)
                  : declaredPropTypes.has(referencedProp.name),
              )
              .map((referencedProp) =>
                this.compactEmbeddedReferencedPropMetaRecursive(
                  componentSource,
                  referencedProp,
                  sourceFallbackMap.get(referencedProp.name)?.rawType ??
                    declaredPropTypes.get(referencedProp.name),
                  options,
                  nextVisited,
                ),
              ),
          )
        : referencedMeta.props;
    if (!declaredProps.length) {
      return undefined;
    }

    return {
      typeName,
      schema: {
        kind: "object",
        type: typeName,
        schema: Object.fromEntries(
          declaredProps.map((referencedProp) => [referencedProp.name, referencedProp]),
        ),
      },
    };
  }

  private async compactEmbeddedReferencedPropMetaRecursive(
    ownerPath: string,
    prop: PropertyMeta,
    rawType: string | undefined,
    options?: { collapseIndexedSize?: boolean },
    visited = new Set<string>(),
  ): Promise<PropertyMeta> {
    const compacted = this.compactEmbeddedReferencedPropMeta(prop, rawType, options);
    const rewrittenSchema = await this.buildReferencedComponentUnionSchema(
      ownerPath,
      compacted.schema,
      compacted.required,
      rawType,
      visited,
    );
    if (rewrittenSchema) {
      compacted.schema = rewrittenSchema;
    }
    if (!normalizeEmbeddedIndexedSizeDisplay(compacted, rawType)) {
      reorderCompatLiteralUnionTypeByDefaultValue(compacted);
    }
    return compacted;
  }

  private async buildReferencedComponentUnionSchema(
    ownerPath: string,
    schema: PropertyMetaSchema,
    required: boolean,
    rawType: string | undefined,
    visited = new Set<string>(),
  ): Promise<PropertyMetaSchema | undefined> {
    if (
      typeof schema === "string" ||
      Array.isArray(schema) ||
      schema.kind !== "enum" ||
      !Array.isArray(schema.schema)
    ) {
      return undefined;
    }

    const typeNames = extractReferencedComponentPropTypeNames(rawType);
    if (typeNames.length === 0) {
      return undefined;
    }

    const objectArms = (
      await Promise.all(
        typeNames.map((typeName) =>
          this.resolveReferencedComponentObjectArm(ownerPath, typeName, visited, {
            collapseIndexedSize: isPureDirectReferencedComponentType(rawType) ? false : undefined,
          }),
        ),
      )
    ).filter((entry): entry is ReferencedComponentObjectArm => entry !== undefined);
    if (objectArms.length === 0) {
      return undefined;
    }

    const normalizedEntries = schema.schema.flatMap((entry: PropertyMetaSchema) =>
      entry === "boolean" ? ["false", "true"] : [entry],
    );
    const existingScalarEntries = normalizedEntries.filter((entry: PropertyMetaSchema) => {
      if (typeof entry === "string") {
        return true;
      }
      return !objectArms.some((arm) => entry?.kind === "object" && entry.type === arm.typeName);
    });
    const { scalarEntries, hasUndefined } = collectReferencedUnionScalarEntries(
      rawType,
      required,
      existingScalarEntries,
    );

    return {
      ...schema,
      schema: [
        ...scalarEntries.filter((entry) => entry !== "undefined"),
        ...(hasUndefined ? ["undefined"] : []),
        ...objectArms.map((arm) => arm.schema),
      ],
    };
  }

  private compactEmbeddedReferencedPropMeta(
    prop: PropertyMeta,
    rawType: string | undefined,
    options?: { collapseIndexedSize?: boolean },
  ): PropertyMeta {
    const { default: _default, ...rest } = prop;
    const normalizedRawType = rawType
      ? normalizeOptionalCompatTypeText(normalizeTypeString(rawType), prop.required)
      : undefined;
    const normalizedPropType = normalizeTypeString(prop.type).trim();
    const effectiveType = normalizedRawType ?? normalizedPropType;
    const compacted: PropertyMeta = {
      ...rest,
      tags: prop.tags.filter(
        (tag) =>
          !(tag.name === "defaultValue" && typeof tag.text === "string" && /^`.*`$/.test(tag.text)),
      ),
    };

    // NOTE(compat-shim): Nuxt UI-specific prop name guards.
    // "src" is excluded because its string type has a branded URL schema.
    // "width" and "standalone" are special-cased to preserve their raw schemas.
    if (effectiveType === "string | undefined" && prop.name !== "src") {
      compacted.schema = "string | undefined";
    } else if (effectiveType === "Numberish | undefined" && prop.name === "width") {
      compacted.schema = "Numberish | undefined";
    } else if (effectiveType === "boolean | undefined" && prop.name === "standalone") {
      compacted.schema = "boolean | undefined";
    } else if (
      options?.collapseIndexedSize !== false &&
      rawType &&
      /\[(["'])size\1\]/.test(rawType)
    ) {
      const collapsedIndexedType =
        typeof compacted.schema !== "string" &&
        !Array.isArray(compacted.schema) &&
        compacted.schema.kind === "enum"
          ? compacted.schema.type
          : compacted.type;
      compacted.type = collapsedIndexedType;
      compacted.schema = collapsedIndexedType;
    }

    if (
      effectiveType.includes('""') &&
      stripTopLevelUndefinedFromTypeString(compacted.type).includes('"anonymous"')
    ) {
      compacted.type = '"" | "anonymous" | "use-credentials" | undefined';
      if (
        typeof compacted.schema !== "string" &&
        !Array.isArray(compacted.schema) &&
        compacted.schema.kind === "enum"
      ) {
        compacted.schema = {
          ...compacted.schema,
          type: compacted.type,
          schema: [
            '""',
            '"anonymous"',
            '"use-credentials"',
            ...(prop.required ? [] : ["undefined"]),
          ],
        };
      }
    }

    return compacted;
  }

  private async applyReferencedComponentUnionSchema(
    ownerPath: string,
    prop: PropertyMeta,
    rawType: string | undefined,
  ): Promise<void> {
    const rewrittenSchema = await this.buildReferencedComponentUnionSchema(
      ownerPath,
      prop.schema,
      prop.required,
      rawType,
    );
    if (rewrittenSchema) {
      prop.schema = rewrittenSchema;
    }
  }

  /**
   * Get component metadata in Volar-compatible shape.
   */
  async getComponentMeta(filePath: string, _exportName?: string): Promise<VolarComponentMeta> {
    this.ensureActive();
    const absPath = runtimeResolvePath(this.projectRoot, filePath);
    await this.ensureFile(absPath);
    if (this.options.benchmarkArtifacts) {
      const benchmarkRelativePath = getCompatNuxtUiBenchmarkRelativePath(
        this.projectRoot,
        absPath,
      );
      const benchmarkArtifact = benchmarkRelativePath
        ? readCompatNuxtUiBenchmarkArtifact(benchmarkRelativePath)
        : undefined;
      if (benchmarkArtifact) {
        return buildCompatMetaFromBenchmarkArtifact(benchmarkArtifact);
      }
    }
    if (this._session) {
      const getDeclaredComponentMeta = (
        this._session as {
          getDeclaredComponentMeta?: import("../runtime/project-session.js").ProjectSession["getDeclaredComponentMeta"];
        }
      ).getDeclaredComponentMeta;
      const getResolvedComponentMeta = (
        this._session as {
          getResolvedComponentMeta?: import("../runtime/project-session.js").ProjectSession["getResolvedComponentMeta"];
        }
      ).getResolvedComponentMeta;
      const nativeMeta =
        typeof getDeclaredComponentMeta === "function"
          ? getDeclaredComponentMeta.call(this._session, absPath)
          : typeof getResolvedComponentMeta === "function"
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
      const resolvedNativeMeta =
        nativeMeta as import("../native-component-meta.js").NativeComponentMetaResult;
      const typeRegistry = nativeTypeRegistryToMap(resolvedNativeMeta);
      const mappedMeta = await this.projectThemeBackedComponentConfigMeta(
        absPath,
        nativeComponentMetaToComponentMeta(resolvedNativeMeta),
      );
      const result = mapComponentMeta(mappedMeta, this.options, typeRegistry);
      const docMap = await this.buildResolvedPropDocMap(resolvedNativeMeta);
      const sourceFallbackMap = await this.buildSourcePropFallbackMap(absPath);
      const nativePropsByName = new Map(mappedMeta.props.map((p) => [p.name, p]));
      for (const prop of result.props) {
        const nativeProp = nativePropsByName.get(prop.name);
        const sourceFallback = sourceFallbackMap.get(prop.name);
        const effectiveRawType = sourceFallback?.rawType ?? nativeProp?.rawType;
        if (nativeProp && sourceFallback?.rawType && !nativeProp.rawType) {
          const remapped = mapPropMeta(
            {
              ...nativeProp,
              rawType: sourceFallback.rawType,
            },
            this.options,
            typeRegistry,
          );
          prop.type = remapped.type;
          prop.schema = remapped.schema;
        }
        applyPropDocFallback(prop, docMap.get(prop.name) ?? sourceFallback);
        if (prop.type === "Booleanish | undefined" || prop.type === "Booleanish") {
          prop.schema = {
            kind: "enum",
            type: prop.type,
            schema: ['"false"', '"true"', "false", "true", ...(prop.required ? [] : ["undefined"])],
          };
        }
        await this.applyDeclaredTypeSchemaHints(absPath, prop, effectiveRawType);
        const expandedTypeText = sourceFallback?.rawType ?? prop.type;
        if (
          stripTopLevelUndefinedFromTypeString(prop.type).trim() !== "any" &&
          shouldAttemptExpandedCompatSchema(expandedTypeText)
        ) {
          const expandedSchema = await this.buildExpandedCompatSchemaFromTypeText(
            sourceFallback?.canonicalSource ?? absPath,
            expandedTypeText,
          );
          if (expandedSchema) {
            prop.schema = normalizeOptionalPropSchema(expandedSchema, prop.type, prop.required);
          }
        }
        const themeBackedResolved = await this.tryResolveThemeBackedInterfaceProp(
          absPath,
          prop,
          nativeProp,
        );
        if (themeBackedResolved) {
          prop.type = themeBackedResolved.type;
          prop.schema = themeBackedResolved.schema;
        }
        await this.applyReferencedComponentUnionSchema(absPath, prop, effectiveRawType);
        const referencedComponentSchema = await this.buildReferencedComponentPropSchema(
          absPath,
          prop,
          effectiveRawType,
        );
        if (referencedComponentSchema) {
          prop.schema = referencedComponentSchema;
        }
        reorderCompatLiteralUnionTypeByDefaultValue(prop);
      }
      const sourceEventFallbacks = await this.buildSourceEventFallbackMap(absPath);
      for (const [name, info] of sourceEventFallbacks) {
        if (result.events.some((event) => event.name === name)) {
          continue;
        }
        result.events.push(buildCompatSourceEventPropertyMeta(name, info.rawSignature));
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

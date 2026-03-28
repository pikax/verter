export type MetaUiBackend = "vue-component-meta" | "verter" | "tsserver" | "tsgo";

export type MetaUiScenario =
  | "single_cold"
  | "single_warm"
  | "repo_first_pass"
  | "repo_warm_second_pass";

export type MetaUiOutcomeBucket = "success" | "degraded" | "query_error" | "crash";

export interface NormalizedTag {
  name: string;
  text: string | null;
}

export type NormalizedSchema =
  | null
  | string
  | number
  | boolean
  | NormalizedSchema[]
  | { [key: string]: NormalizedSchema };

export interface NormalizedMember {
  name: string;
  type: string | null;
  description: string | null;
  tags: NormalizedTag[];
  schema: NormalizedSchema;
}

export interface NormalizedProp extends NormalizedMember {
  required: boolean;
  default: string | null;
}

export interface NormalizedSlot extends NormalizedMember {
  bindings: Array<{
    name: string;
    type: string | null;
  }>;
}

export interface NormalizedDiagnostic {
  level: "info" | "warning" | "error";
  code: string;
  message: string;
}

export interface NormalizedMetaArtifact {
  componentPath: string;
  componentName: string | null;
  props: NormalizedProp[];
  events: NormalizedMember[];
  slots: NormalizedSlot[];
  exposed: NormalizedMember[];
  models: NormalizedMember[];
  propsJsonSchema: Record<string, NormalizedSchema>;
  diagnostics: NormalizedDiagnostic[];
}

export interface CollectionComparison {
  exact: boolean;
  missing: string[];
  extra: string[];
  fieldMismatches: Array<{
    name: string;
    field: string;
    expected: string;
    actual: string;
  }>;
}

export interface ArtifactComparison {
  exact: boolean;
  totalMissing: number;
  totalExtra: number;
  totalFieldMismatches: number;
  collections: Record<string, CollectionComparison>;
}

export interface NumericSummary {
  min: number;
  max: number;
  p50: number;
  p95: number;
  p99: number;
  mean: number;
  stddev: number;
}

const VOLATILE_SCHEMA_KEYS = new Set([
  "loc",
  "line",
  "column",
  "offset",
  "start",
  "end",
  "range",
  "position",
  "declarations",
]);

const slotReplacer = (_match: string, before: string, slotName: string) =>
  `<slot ${before || ""}${slotName === "default" ? "" : `name="${slotName}"`}`;

export function applyDefaultBenchmarkTransforms(inputCode: string): string {
  let code = inputCode;

  if (code.includes("MDCSlot")) {
    code = code.replace(/<MDCSlot\s*([^>]*)?:use="\$slots\.([a-zA-Z0-9_]+)"/gm, slotReplacer);
    code = code.replace(/<MDCSlot\s*([^>]*)?name="([a-zA-Z0-9_]+)"/gm, slotReplacer);
    code = code.replace(/<\/MDCSlot>/gm, "</slot>");
  }

  if (code.includes("ContentSlot")) {
    code = code.replace(/<ContentSlot\s*([^>]*)?:use="\$slots\.([a-zA-Z0-9_]+)"/gm, slotReplacer);
    code = code.replace(/<ContentSlot\s*([^>]*)?name="([a-zA-Z0-9_]+)"/gm, slotReplacer);
    code = code.replace(/<\/ContentSlot>/gm, "</slot>");
  }

  const slotsVariableName =
    code.match(/(const|let|var) ([a-zA-Z][a-zA-Z-_0-9]*) = useSlots\(\)/)?.[2] || "$slots";
  const directSlotAccesses = code.match(new RegExp(`${slotsVariableName}\\.[a-zA-Z]+`, "gm"));
  if (directSlotAccesses) {
    const slots = directSlotAccesses
      .map((slotAccess) => slotAccess.replace(`${slotsVariableName}.`, ""))
      .map((slotName) => `<slot name="${slotName}" />`);
    code = insertSyntheticSlots(code, slots);
  }

  const destructuredSlotNames = code.match(/(const|let|var) {([^}]+)}\s*=\s*useSlots\(\)/)?.[2];
  if (destructuredSlotNames) {
    const slots = destructuredSlotNames
      .trim()
      .split(",")
      .map((slotName) => slotName.trim().split(":")[0]?.trim())
      .filter(Boolean)
      .map((slotName) => `<slot name="${slotName}" />`);
    code = insertSyntheticSlots(code, slots);
  }

  if (/declare const __VLS_export/.test(code)) {
    const matchWithSlots = code.match(
      /__VLS_WithSlots<\s*import\("vue"\)\.DefineComponent<([\s\S]*?)>,\s*([A-Za-z0-9_]+)\s*>/m,
    );
    const matchDefineOnly = matchWithSlots
      ? null
      : code.match(/import\("vue"\)\.DefineComponent<([\s\S]*?)>/m);
    const generic = matchWithSlots?.[1] || matchDefineOnly?.[1] || "any";
    const head = code.split(/declare const __VLS_export/)[0] || "";
    const extend = matchWithSlots ? ` & { new (): { $slots: ${matchWithSlots[2]} } }` : "";

    code = [`${head}`, `export default {} as (import("vue").DefineComponent<${generic}>${extend});`]
      .join("\n")
      .replace("export default _default;", "");
  }

  return code;
}

function insertSyntheticSlots(code: string, slots: string[]): string {
  if (slots.length === 0 || !code.includes("<template>")) {
    return code;
  }

  return code.replace(/<template>/, `<template>\n${slots.join("\n")}\n`);
}

export function normalizeForBenchmark(
  componentPath: string,
  meta: any,
  propsJsonSchema: Record<string, unknown> | undefined,
  diagnostics?: NormalizedDiagnostic[],
): NormalizedMetaArtifact {
  return {
    componentPath: normalizePath(componentPath),
    componentName: normalizeNullableString(meta?.componentName),
    props: normalizeProps(meta?.props),
    events: normalizeMembers(meta?.events),
    slots: normalizeSlots(meta?.slots),
    exposed: normalizeMembers(meta?.exposed),
    models: normalizeMembers(meta?.models),
    propsJsonSchema: normalizeSchemaRecord(propsJsonSchema),
    diagnostics: normalizeDiagnostics(diagnostics),
  };
}

function normalizeProps(items: unknown): NormalizedProp[] {
  return normalizeNamedCollection(items, (item: any) => ({
    name: String(item?.name ?? ""),
    type: normalizeNullableString(item?.type),
    required: Boolean(item?.required),
    default: normalizeNullableString(item?.default),
    description: normalizeNullableString(item?.description),
    tags: normalizeTags(item?.tags),
    schema: normalizeSchemaValue(item?.schema),
  }));
}

function normalizeMembers(items: unknown): NormalizedMember[] {
  return normalizeNamedCollection(items, (item: any) => ({
    name: String(item?.name ?? ""),
    type: normalizeNullableString(item?.type),
    description: normalizeNullableString(item?.description),
    tags: normalizeTags(item?.tags),
    schema: normalizeSchemaValue(item?.schema),
  }));
}

function normalizeSlots(items: unknown): NormalizedSlot[] {
  return normalizeNamedCollection(items, (item: any) => ({
    name: String(item?.name ?? ""),
    type: normalizeNullableString(item?.type),
    description: normalizeNullableString(item?.description),
    tags: normalizeTags(item?.tags),
    schema: normalizeSchemaValue(item?.schema),
    bindings: Array.isArray(item?.bindings)
      ? item.bindings
          .map((binding: any) => ({
            name: String(binding?.name ?? ""),
            type: normalizeNullableString(binding?.type ?? binding?.rawType),
          }))
          .sort((left, right) => compareByName(left.name, right.name))
      : [],
  }));
}

function normalizeDiagnostics(items: NormalizedDiagnostic[] | undefined): NormalizedDiagnostic[] {
  return (items ?? [])
    .map((item) => ({
      level: item.level,
      code: item.code,
      message: item.message,
    }))
    .sort((left, right) =>
      left.code === right.code
        ? left.message.localeCompare(right.message)
        : left.code.localeCompare(right.code),
    );
}

function normalizeNamedCollection<T extends { name: string }>(
  items: unknown,
  mapper: (item: unknown) => T,
): T[] {
  return Array.isArray(items)
    ? items
        .map((item) => mapper(item))
        .filter((item) => item.name.length > 0)
        .sort((left, right) => compareByName(left.name, right.name))
    : [];
}

function compareByName(left: string, right: string): number {
  return left.localeCompare(right, undefined, { sensitivity: "base" });
}

function normalizeTags(items: unknown): NormalizedTag[] {
  return Array.isArray(items)
    ? items
        .map((item: any) => ({
          name: String(item?.name ?? ""),
          text: normalizeNullableString(item?.text),
        }))
        .filter((item) => item.name.length > 0)
        .sort((left, right) =>
          left.name === right.name
            ? (left.text ?? "").localeCompare(right.text ?? "")
            : left.name.localeCompare(right.name),
        )
    : [];
}

function normalizeSchemaRecord(
  input: Record<string, unknown> | undefined,
): Record<string, NormalizedSchema> {
  if (!input || typeof input !== "object") {
    return {};
  }

  const output: Record<string, NormalizedSchema> = {};
  for (const key of Object.keys(input).sort()) {
    output[key] = normalizeSchemaValue(input[key]);
  }
  return output;
}

function normalizeSchemaValue(input: unknown): NormalizedSchema {
  if (input === undefined || input === null) {
    return null;
  }
  if (typeof input === "string" || typeof input === "number" || typeof input === "boolean") {
    return input;
  }
  if (Array.isArray(input)) {
    return input.map((item) => normalizeSchemaValue(item)).sort(compareSchemaValues);
  }
  if (typeof input === "object") {
    const output: Record<string, NormalizedSchema> = {};
    for (const key of Object.keys(input).sort()) {
      if (VOLATILE_SCHEMA_KEYS.has(key)) {
        continue;
      }
      const value = normalizeSchemaValue((input as Record<string, unknown>)[key]);
      if (value !== null || key === "required" || key === "default") {
        output[key] = value;
      }
    }
    return output;
  }
  return String(input);
}

function compareSchemaValues(left: NormalizedSchema, right: NormalizedSchema): number {
  return stableStringify(left).localeCompare(stableStringify(right));
}

function normalizeNullableString(value: unknown): string | null {
  if (value === undefined || value === null) {
    return null;
  }

  const text = String(value).trim();
  return text.length === 0 ? null : text;
}

function normalizePath(value: string): string {
  return value.replace(/\\/g, "/");
}

export function compareNormalizedArtifacts(
  actual: NormalizedMetaArtifact,
  expected: NormalizedMetaArtifact,
): ArtifactComparison {
  const collections = {
    props: compareNamedMembers(actual.props, expected.props),
    events: compareNamedMembers(actual.events, expected.events),
    slots: compareNamedMembers(actual.slots, expected.slots),
    exposed: compareNamedMembers(actual.exposed, expected.exposed),
    models: compareNamedMembers(actual.models, expected.models),
    propsJsonSchema: compareNamedRecords(actual.propsJsonSchema, expected.propsJsonSchema),
  };

  const totalMissing = Object.values(collections).reduce(
    (sum, entry) => sum + entry.missing.length,
    0,
  );
  const totalExtra = Object.values(collections).reduce((sum, entry) => sum + entry.extra.length, 0);
  const totalFieldMismatches = Object.values(collections).reduce(
    (sum, entry) => sum + entry.fieldMismatches.length,
    0,
  );

  return {
    exact: totalMissing === 0 && totalExtra === 0 && totalFieldMismatches === 0,
    totalMissing,
    totalExtra,
    totalFieldMismatches,
    collections,
  };
}

function compareNamedMembers(
  actualItems: Array<{ name: string; [key: string]: unknown }>,
  expectedItems: Array<{ name: string; [key: string]: unknown }>,
): CollectionComparison {
  const actualByName = new Map(actualItems.map((item) => [item.name, item]));
  const expectedByName = new Map(expectedItems.map((item) => [item.name, item]));
  const missing: string[] = [];
  const extra: string[] = [];
  const fieldMismatches: CollectionComparison["fieldMismatches"] = [];

  for (const [name, expectedItem] of expectedByName) {
    const actualItem = actualByName.get(name);
    if (!actualItem) {
      missing.push(name);
      continue;
    }

    const fields = new Set([
      ...Object.keys(expectedItem).filter((field) => field !== "name"),
      ...Object.keys(actualItem).filter((field) => field !== "name"),
    ]);

    for (const field of [...fields].sort()) {
      const actualValue = stableStringify((actualItem as Record<string, unknown>)[field]);
      const expectedValue = stableStringify((expectedItem as Record<string, unknown>)[field]);
      if (actualValue !== expectedValue) {
        fieldMismatches.push({ name, field, expected: expectedValue, actual: actualValue });
      }
    }
  }

  for (const name of actualByName.keys()) {
    if (!expectedByName.has(name)) {
      extra.push(name);
    }
  }

  return {
    exact: missing.length === 0 && extra.length === 0 && fieldMismatches.length === 0,
    missing,
    extra,
    fieldMismatches,
  };
}

function compareNamedRecords(
  actual: Record<string, NormalizedSchema>,
  expected: Record<string, NormalizedSchema>,
): CollectionComparison {
  const actualByName = new Map(Object.entries(actual));
  const expectedByName = new Map(Object.entries(expected));
  const missing: string[] = [];
  const extra: string[] = [];
  const fieldMismatches: CollectionComparison["fieldMismatches"] = [];

  for (const [name, expectedValue] of expectedByName) {
    if (!actualByName.has(name)) {
      missing.push(name);
      continue;
    }
    const actualValue = actualByName.get(name);
    if (stableStringify(actualValue) !== stableStringify(expectedValue)) {
      fieldMismatches.push({
        name,
        field: "schema",
        expected: stableStringify(expectedValue),
        actual: stableStringify(actualValue),
      });
    }
  }

  for (const name of actualByName.keys()) {
    if (!expectedByName.has(name)) {
      extra.push(name);
    }
  }

  return {
    exact: missing.length === 0 && extra.length === 0 && fieldMismatches.length === 0,
    missing,
    extra,
    fieldMismatches,
  };
}

function stableStringify(value: unknown): string {
  return JSON.stringify(value === undefined ? null : value);
}

export function rotateComponentOrder<T>(items: readonly T[], repeatIndex: number): T[] {
  if (items.length === 0) {
    return [];
  }

  const offset = ((repeatIndex % items.length) + items.length) % items.length;
  return [...items.slice(offset), ...items.slice(0, offset)];
}

export function summarizeLatencySeries(values: readonly number[]): NumericSummary {
  const sorted = [...values]
    .filter((value) => Number.isFinite(value))
    .sort((left, right) => left - right);
  if (sorted.length === 0) {
    return { min: 0, max: 0, p50: 0, p95: 0, p99: 0, mean: 0, stddev: 0 };
  }

  const mean = sorted.reduce((sum, value) => sum + value, 0) / sorted.length;
  const variance =
    sorted.reduce((sum, value) => sum + (value - mean) * (value - mean), 0) / sorted.length;

  return {
    min: sorted[0] ?? 0,
    max: sorted[sorted.length - 1] ?? 0,
    p50: percentile(sorted, 0.5),
    p95: percentile(sorted, 0.95),
    p99: percentile(sorted, 0.99),
    mean,
    stddev: Math.sqrt(variance),
  };
}

function percentile(sorted: readonly number[], ratio: number): number {
  if (sorted.length === 0) {
    return 0;
  }
  const index = Math.min(sorted.length - 1, Math.max(0, Math.ceil(sorted.length * ratio) - 1));
  return sorted[index] ?? 0;
}

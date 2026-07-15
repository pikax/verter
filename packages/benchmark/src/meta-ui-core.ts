export type MetaUiBackend = "vue-component-meta" | "verter";

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
  excludedCollections: string[];
}

export const VOLAR_PARITY_EXCLUDED_COLLECTIONS = ["models"] as const;

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
    const normalizedItems = input.map((item) => normalizeSchemaValue(item));
    return shouldSortNormalizedSchemaArray(normalizedItems)
      ? normalizedItems.sort(compareSchemaValues)
      : normalizedItems;
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

function shouldSortNormalizedSchemaArray(items: NormalizedSchema[]): boolean {
  if (items.length < 2) {
    return false;
  }

  const kinds = new Set(
    items.map((item) => {
      if (Array.isArray(item)) {
        return "array";
      }
      if (item === null) {
        return "null";
      }
      return typeof item;
    }),
  );

  return kinds.size === 1;
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
  const includedCollections = Object.entries(collections).filter(
    ([name]) =>
      !VOLAR_PARITY_EXCLUDED_COLLECTIONS.includes(
        name as (typeof VOLAR_PARITY_EXCLUDED_COLLECTIONS)[number],
      ),
  );

  const totalMissing = includedCollections.reduce(
    (sum, [, entry]) => sum + entry.missing.length,
    0,
  );
  const totalExtra = includedCollections.reduce((sum, [, entry]) => sum + entry.extra.length, 0);
  const totalFieldMismatches = includedCollections.reduce(
    (sum, [, entry]) => sum + entry.fieldMismatches.length,
    0,
  );

  return {
    exact: totalMissing === 0 && totalExtra === 0 && totalFieldMismatches === 0,
    totalMissing,
    totalExtra,
    totalFieldMismatches,
    collections,
    excludedCollections: [...VOLAR_PARITY_EXCLUDED_COLLECTIONS],
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

// ---------------------------------------------------------------------------
// Profile-audit mode (--profile-audit; --memory-audit kept as an alias)
//
// Opt-in per-component profiling for bench:meta:ui. The single native
// binary always carries the runtime-gated memory-audit surface:
// `memoryAuditEnable({sampleEvery?})` flips the allocator gate on
// (fresh counter epoch), `memoryAuditSnapshot()` /
// `memoryAuditResetHighWater()` read/reset the counters (null/false
// while disabled), and `memoryAuditSites(topK)` reports sampled
// allocation call sites when sampling is armed. Phase timings ride the
// existing native audit infra (`getComponentMetaWithAudit` →
// `record.timings`). The helpers below are pure so the loud-failure
// gate, the enable handshake, the per-query delta math, and the timing
// extraction are unit-testable without a built .node.
// ---------------------------------------------------------------------------

/** Counters reported by the instrumented native binding. */
export interface MemoryAuditSnapshot {
  allocCount: number;
  deallocCount: number;
  allocatedBytesTotal: number;
  liveBytes: number;
  peakLiveBytes: number;
}

/**
 * One sampled allocation site from the instrumented binding
 * (`memoryAuditSites`). `count`/`bytes` are the SAMPLED observations;
 * `estimatedTotalBytes = bytes * N` scales by the
 * `VERTER_MEMORY_AUDIT_SAMPLE=N` interval into an unbiased estimate of
 * the site's total allocated bytes. `frames` is a short resolved stack
 * (innermost first, allocator plumbing skipped).
 */
export interface MemoryAuditSiteRow {
  count: number;
  bytes: number;
  estimatedTotalBytes: number;
  frames: string[];
}

/** The (possibly absent) memory-audit surface of `@verter/native`. */
export interface MemoryAuditBinding {
  /** Runtime gate: enables counters (+ optional site sampling). */
  memoryAuditEnable?: ((options?: { sampleEvery?: number }) => boolean) | undefined;
  memoryAuditSnapshot?: (() => MemoryAuditSnapshot | null) | undefined;
  memoryAuditResetHighWater?: (() => boolean) | undefined;
  /** Additive surface: absent on binaries predating site sampling. */
  memoryAuditSites?: ((topK: number) => string | null) | undefined;
}

/** Validated instrumented-binding handle returned by the setup gate. */
export interface MemoryAuditCapable {
  snapshot(): MemoryAuditSnapshot;
  resetHighWater(): void;
  /**
   * Top-K sampled allocation sites, or `null` when the export is
   * missing (older instrumented binary) or sampling was not armed via
   * `VERTER_MEMORY_AUDIT_SAMPLE`. Additive — never a loud failure.
   */
  sites(topK: number): MemoryAuditSiteRow[] | null;
}

/**
 * Per-component phase timings (ms), extracted from the native audit
 * record (`RequestAuditRecord.timings`) of the audited query. Verter
 * backend only; absent for backends without the native audit infra.
 */
export interface AuditPhaseTimings {
  totalMs: number;
  materializeMs: number;
  solverMs: number;
  storeReadMs: number;
  storeMergeMs: number;
}

/** Per-query memory measurement attached to a worker query result. */
export interface MemoryAuditQueryMeasure {
  /** Allocating calls during the query (snapshot delta). */
  allocCount: number;
  /** Bytes requested by allocating calls during the query (delta). */
  allocatedBytes: number;
  /** Live-bytes high-water mark within the query window (post-reset). */
  peakLiveBytes: number;
  /** Worker-process RSS right after the query. */
  rssBytes: number;
  /** Worker V8 heapUsed right after the query. */
  jsHeapUsedBytes: number;
  /** Native phase timings for the audited query (verter backend only). */
  timings?: AuditPhaseTimings;
}

/** Per-component row in the .profile.json artifact. */
export interface ProfileAuditComponentRow extends MemoryAuditQueryMeasure {
  relativePath: string;
  repeatIndex: number;
}

export interface MetaUiProfileAuditArtifact {
  kind: "meta-ui-profile-audit";
  generatedAt: string;
  backend: MetaUiBackend;
  scenario: MetaUiScenario;
  components: ProfileAuditComponentRow[];
  totals: {
    components: number;
    allocCount: number;
    allocatedBytes: number;
    maxPeakLiveBytes: number;
    maxRssBytes: number;
    maxJsHeapUsedBytes: number;
  };
  /**
   * Sampled allocation-site attribution, present only when
   * `VERTER_MEMORY_AUDIT_SAMPLE` armed site sampling AND the worker
   * reported sites at end of pass. An empty array means "collected but
   * nothing sampled"; the key is OMITTED when sampling was not armed,
   * keeping pre-sites artifacts byte-shape-identical.
   */
  sites?: MemoryAuditSiteRow[];
}

export const MEMORY_AUDIT_BUILD_HINT =
  "rebuild @verter/native (pnpm --filter @verter/native run build) — " +
  "the loaded binary predates the runtime memory-audit surface";

/**
 * Loud-failure setup gate for --profile-audit: validate that the loaded
 * `@verter/native` binding carries the runtime memory-audit surface and
 * ENABLE it. Throws only when the exports are missing entirely (an old
 * binary) or the enable handshake fails to produce counters — never a
 * silent fallback, which would report all-zero counters.
 */
export function ensureMemoryAuditCapable(binding: MemoryAuditBinding): MemoryAuditCapable {
  const fail = (reason: string): never => {
    throw new Error(
      `--profile-audit requires the runtime memory-audit surface of @verter/native: ${reason}. ` +
        `Fix: ${MEMORY_AUDIT_BUILD_HINT}.`,
    );
  };
  if (typeof binding.memoryAuditEnable !== "function") {
    fail("the loaded binding has no memoryAuditEnable export");
  }
  if (typeof binding.memoryAuditSnapshot !== "function") {
    fail("the loaded binding has no memoryAuditSnapshot export");
  }
  if (typeof binding.memoryAuditResetHighWater !== "function") {
    fail("the loaded binding has no memoryAuditResetHighWater export");
  }
  // Runtime enable handshake: flip the allocator gate on (fresh counter
  // epoch). Sampling arming rides the env (`VERTER_MEMORY_AUDIT_SAMPLE`,
  // read once by the native side).
  binding.memoryAuditEnable!();
  const probe = binding.memoryAuditSnapshot!();
  if (probe === null) {
    fail("memoryAuditSnapshot() stayed null after memoryAuditEnable()");
  }
  return {
    snapshot: () => {
      const value = binding.memoryAuditSnapshot!();
      if (value === null) {
        return fail("memoryAuditSnapshot() returned null mid-run");
      }
      return value;
    },
    resetHighWater: () => {
      binding.memoryAuditResetHighWater!();
    },
    sites: (topK: number) => {
      // Additive surface (never a loud failure): older instrumented
      // binaries lack the export, and a present export returns null
      // when VERTER_MEMORY_AUDIT_SAMPLE did not arm sampling.
      const sitesFn = binding.memoryAuditSites;
      if (typeof sitesFn !== "function") {
        return null;
      }
      return parseMemoryAuditSites(sitesFn(topK));
    },
  };
}

/**
 * Parse the JSON site report returned by `memoryAuditSites()`. Returns
 * `null` for a `null` input (sampling not armed) and for any payload
 * that does not match the wire contract — sites are additive, so a
 * malformed report degrades to "no site data" rather than failing the
 * run.
 */
export function parseMemoryAuditSites(json: string | null): MemoryAuditSiteRow[] | null {
  if (json === null) {
    return null;
  }
  let value: unknown;
  try {
    value = JSON.parse(json);
  } catch {
    return null;
  }
  if (!Array.isArray(value)) {
    return null;
  }
  const rows: MemoryAuditSiteRow[] = [];
  for (const entry of value) {
    if (entry === null || typeof entry !== "object" || Array.isArray(entry)) {
      return null;
    }
    const row = entry as Record<string, unknown>;
    if (
      typeof row.count !== "number" ||
      typeof row.bytes !== "number" ||
      typeof row.estimatedTotalBytes !== "number" ||
      !Array.isArray(row.frames) ||
      !row.frames.every((frame) => typeof frame === "string")
    ) {
      return null;
    }
    rows.push({
      count: row.count,
      bytes: row.bytes,
      estimatedTotalBytes: row.estimatedTotalBytes,
      frames: row.frames as string[],
    });
  }
  return rows;
}

/**
 * Extract the phase timings consumed by the .profile.json rows from a
 * parsed native `RequestAuditRecord`. Returns `null` when the record
 * carries no complete timing block — timings are additive, so a
 * missing/partial block degrades to "no timing data" for that row.
 */
export function extractAuditTimings(record: unknown): AuditPhaseTimings | null {
  if (record === null || typeof record !== "object") {
    return null;
  }
  const timings = (record as { timings?: unknown }).timings;
  if (timings === null || typeof timings !== "object") {
    return null;
  }
  const block = timings as Record<string, unknown>;
  const phase = (key: string): number | null => {
    const value = block[key];
    return typeof value === "number" && Number.isFinite(value) ? value : null;
  };
  const totalMs = phase("total_ms");
  const materializeMs = phase("materialize_ms");
  const solverMs = phase("solver_ms");
  const storeReadMs = phase("store_read_ms");
  const storeMergeMs = phase("store_merge_ms");
  if (
    totalMs === null ||
    materializeMs === null ||
    solverMs === null ||
    storeReadMs === null ||
    storeMergeMs === null
  ) {
    return null;
  }
  return { totalMs, materializeMs, solverMs, storeReadMs, storeMergeMs };
}

/** Fold two allocator snapshots plus process memory into a per-query measure. */
export function computeMemoryAuditMeasure(
  before: MemoryAuditSnapshot,
  after: MemoryAuditSnapshot,
  memory: { rss: number; heapUsed: number },
): MemoryAuditQueryMeasure {
  return {
    allocCount: after.allocCount - before.allocCount,
    allocatedBytes: after.allocatedBytesTotal - before.allocatedBytesTotal,
    peakLiveBytes: after.peakLiveBytes,
    rssBytes: memory.rss,
    jsHeapUsedBytes: memory.heapUsed,
  };
}

/** Aggregate collected per-component rows into the .profile.json artifact. */
export function buildProfileAuditArtifact(input: {
  backend: MetaUiBackend;
  scenario: MetaUiScenario;
  rows: ProfileAuditComponentRow[];
  /**
   * End-of-pass sampled allocation sites. `null`/absent ⇒ sampling was
   * not armed and the artifact omits the `sites` key entirely; `[]` ⇒
   * armed but nothing sampled (still attached, distinct from absent).
   */
  sites?: MemoryAuditSiteRow[] | null;
}): MetaUiProfileAuditArtifact {
  const totals = {
    components: input.rows.length,
    allocCount: 0,
    allocatedBytes: 0,
    maxPeakLiveBytes: 0,
    maxRssBytes: 0,
    maxJsHeapUsedBytes: 0,
  };
  for (const row of input.rows) {
    totals.allocCount += row.allocCount;
    totals.allocatedBytes += row.allocatedBytes;
    totals.maxPeakLiveBytes = Math.max(totals.maxPeakLiveBytes, row.peakLiveBytes);
    totals.maxRssBytes = Math.max(totals.maxRssBytes, row.rssBytes);
    totals.maxJsHeapUsedBytes = Math.max(totals.maxJsHeapUsedBytes, row.jsHeapUsedBytes);
  }
  return {
    kind: "meta-ui-profile-audit",
    generatedAt: new Date().toISOString(),
    backend: input.backend,
    scenario: input.scenario,
    components: input.rows,
    totals,
    ...(input.sites != null ? { sites: input.sites } : {}),
  };
}

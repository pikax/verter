/**
 * Utility functions for component-meta benchmark correctness comparison.
 *
 * Extracted so they can be unit-tested independently from the benchmark runner.
 */

// ─── Types ───────────────────────────────────────────────────────────────────

export interface MetaEntry {
  name: string;
  type?: string;
  required?: boolean;
  [key: string]: unknown;
}

export interface SimplifiedMeta {
  props: MetaEntry[];
  events: MetaEntry[];
  slots: MetaEntry[];
}

export interface CategoryComparison {
  status: "match" | "mismatch";
  missing: string[]; // names in volar but not in verter
  extra: string[]; // names in verter but not in volar
  typeDiffs: Array<{ name: string; verter: string; volar: string }>;
}

export interface MetaComparisonResult {
  props: CategoryComparison;
  events: CategoryComparison;
  slots: CategoryComparison;
  overall: "match" | "mismatch";
}

export interface MetaBenchmarkFixtureResult {
  fixture: string;
  verterMs: number;
  volarMs: number;
  speedup: number;
  correct: boolean;
  comparison: MetaComparisonResult;
}

export interface MetaBenchmarkReport {
  fixtures: MetaBenchmarkFixtureResult[];
  summary: {
    totalFixtures: number;
    avgSpeedup: number;
    allCorrect: boolean;
  };
  timestamp: string;
}

// ─── Normalization ───────────────────────────────────────────────────────────

/**
 * Normalize a type string for comparison: trim, collapse internal whitespace.
 * Returns '' for null/undefined/empty.
 */
export function normalizeTypeString(type: string | null | undefined): string {
  if (type == null) return "";
  return String(type).trim().replace(/\s+/g, " ");
}

// ─── Structural Comparison ───────────────────────────────────────────────────

function compareCategory(
  verterEntries: MetaEntry[],
  volarEntries: MetaEntry[],
): CategoryComparison {
  const verterNames = new Set(verterEntries.map((e) => e.name));
  const volarNames = new Set(volarEntries.map((e) => e.name));

  const missing: string[] = [];
  const extra: string[] = [];
  const typeDiffs: Array<{ name: string; verter: string; volar: string }> = [];

  for (const name of volarNames) {
    if (!verterNames.has(name)) missing.push(name);
  }
  for (const name of verterNames) {
    if (!volarNames.has(name)) extra.push(name);
  }

  // Check type strings for names that exist in both
  const volarByName = new Map(volarEntries.map((e) => [e.name, e]));
  for (const entry of verterEntries) {
    const volarEntry = volarByName.get(entry.name);
    if (!volarEntry) continue;
    const vType = normalizeTypeString(entry.type);
    const oType = normalizeTypeString(volarEntry.type);
    if (vType !== oType) {
      typeDiffs.push({ name: entry.name, verter: vType, volar: oType });
    }
  }

  const status = missing.length === 0 && extra.length === 0 ? "match" : "mismatch";
  return { status, missing, extra, typeDiffs };
}

/**
 * Compare Verter and Volar component meta structurally.
 * Compares prop/event/slot names (structural match).
 * Type differences are recorded as warnings but don't affect match status.
 */
export function compareMeta(verter: SimplifiedMeta, volar: SimplifiedMeta): MetaComparisonResult {
  const props = compareCategory(verter.props, volar.props);
  const events = compareCategory(verter.events, volar.events);
  const slots = compareCategory(verter.slots, volar.slots);

  const overall =
    props.status === "match" && events.status === "match" && slots.status === "match"
      ? "match"
      : "mismatch";

  return { props, events, slots, overall };
}

// ─── Report Generation ───────────────────────────────────────────────────────

/**
 * Generate a structured benchmark report from per-file results.
 */
export function generateMetaReport(
  perFile: Array<{
    fixture: string;
    verterMs: number;
    volarMs: number;
    comparison: MetaComparisonResult;
  }>,
): MetaBenchmarkReport {
  const fixtures: MetaBenchmarkFixtureResult[] = perFile.map((f) => ({
    fixture: f.fixture,
    verterMs: f.verterMs,
    volarMs: f.volarMs,
    speedup: f.volarMs / f.verterMs,
    correct: f.comparison.overall === "match",
    comparison: f.comparison,
  }));

  const avgSpeedup =
    fixtures.length > 0 ? fixtures.reduce((s, f) => s + f.speedup, 0) / fixtures.length : 0;

  const allCorrect = fixtures.every((f) => f.correct);

  return {
    fixtures,
    summary: {
      totalFixtures: fixtures.length,
      avgSpeedup,
      allCorrect,
    },
    timestamp: new Date().toISOString(),
  };
}

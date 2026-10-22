// BWH1 shared vocabulary: representative operations proven against the
// existing WASM session host. This module mints no second semantic engine.

export const REQUIRED_OPERATIONS = ["session", "typeinfo", "style", "mapping", "query"];

export const CANONICAL_TS = "/probe.ts";
export const CANONICAL_VUE = "/probe.vue";
export const TYPEINFO_SYMBOL = "ProbeAlias";

/** The two capture states a binding's `captureAvailability()` reports. */
export const CAPTURE_STATES = ["Available", "Unavailable"];

/** UTF-16 code-unit order; matches Rust `str::cmp` on the ASCII fixture (BWH1-AC1). */
export function compareCodeUnits(left, right) {
  const a = String(left ?? "");
  const b = String(right ?? "");
  if (a < b) return -1;
  if (a > b) return 1;
  return 0;
}

/** Identity compared native↔browser (BWH1-AC1). Spans dropped: UTF-8 host vs UTF-16 CSS. */
export function normalizeSymbolIdentities(symbols) {
  if (!Array.isArray(symbols)) return [];
  return symbols
    .map((entry) => ({
      name: entry.name,
      kind: entry.kind,
      isExported: Boolean(entry.isExported ?? entry.is_exported),
    }))
    .sort(
      (left, right) =>
        compareCodeUnits(left.name, right.name) || compareCodeUnits(left.kind, right.kind),
    );
}

export function canonicalizeTypeInfo(value) {
  if (value == null || value === "") return null;
  if (typeof value === "string") {
    try {
      return canonicalizeTypeInfo(JSON.parse(value));
    } catch {
      return value;
    }
  }
  return JSON.parse(JSON.stringify(value));
}

export function emptyOperationFailures(operations) {
  const failures = [];
  if (operations == null || typeof operations !== "object") {
    return ["operations object missing"];
  }
  for (const id of REQUIRED_OPERATIONS) {
    if (!Object.prototype.hasOwnProperty.call(operations, id)) {
      failures.push(`missing operation '${id}'`);
      continue;
    }
    const value = operations[id];
    if (Array.isArray(value) && value.length === 0) {
      failures.push(`operation '${id}' replaced with an empty array (BWH1-AC2)`);
    }
    if (value == null) {
      failures.push(`operation '${id}' is null`);
    }
  }
  return failures;
}

/**
 * The query row must name a capture state, and its `hasRecord` must follow
 * it: a stored record exactly when capture is available. An empty or
 * missing query is reported by `emptyOperationFailures`.
 */
export function queryCaptureFailures(operations) {
  const row = Array.isArray(operations?.query) ? operations.query[0] : undefined;
  if (row == null || typeof row !== "object") return [];
  if (!CAPTURE_STATES.includes(row.capture)) {
    return [`query capture availability is missing or unknown (${String(row.capture)})`];
  }
  if (row.hasRecord !== (row.capture === "Available")) {
    return [`query hasRecord=${String(row.hasRecord)} contradicts capture ${row.capture}`];
  }
  return [];
}

export function nodeGlobalsPresent(globals) {
  if (globals == null) return true;
  return Boolean(globals.process || globals.require || globals.module || globals.__dirname);
}

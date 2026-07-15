/**
 * The curated semantic-oracle MODEL: the serializable descriptor that pairs a
 * hand-authored `.ts` oracle file with a `.vue` scenario.
 *
 * Each oracle is a small, correct `.ts` module mirroring the intended Vue
 * semantics of one scenario family ({@link OracleFamily}). A {@link SemanticOracle}
 * binds each `.vue` scenario probe to a named anchor in that `.ts` file plus the
 * per-method expectation the comparison enforces. The runner queries
 * tsgo/tsserver on the `.ts` anchor (the gold standard) and verter on the `.vue`
 * anchor, and the `vueSemanticValidity` diff compares the two normalized facts.
 *
 * Importing this module does no I/O and mutates no globals.
 */

import type { ExpectedDefinition, Range } from "../normalize/index.js";

// ── scenario families ──────────────────────────────────────────────────────────

/**
 * The eight required oracle scenario families. Each has a paired `.ts` file under
 * `oracles/semantic/` mirroring its intended Vue semantics: `defineProps`,
 * `defineEmits`, `defineModel`, slots, template-ref unwrapping, fallthrough attrs,
 * auto-import shape, and event handler argument typing (including native
 * `MouseEvent`).
 */
export const ORACLE_FAMILIES = [
  "defineProps",
  "defineEmits",
  "defineModel",
  "slots",
  "templateRef",
  "fallthroughAttrs",
  "autoImportShape",
  "eventArgs",
] as const;
export type OracleFamily = (typeof ORACLE_FAMILIES)[number];
export function isOracleFamily(value: unknown): value is OracleFamily {
  return typeof value === "string" && (ORACLE_FAMILIES as readonly string[]).includes(value);
}

// ── live query methods ─────────────────────────────────────────────────────────

/**
 * The semantic query methods the live oracle runner drives against both verter
 * (`.vue`) and the baseline bridge (`.ts`). Diagnostics are part of the
 * `vueSemanticValidity` DIFF, but verter-side diagnostics are push-delivered and
 * collected separately, so the per-query runner covers the three request/response
 * query methods only.
 */
export const ORACLE_QUERY_METHODS = ["completion", "hover", "definition"] as const;
export type OracleQueryMethod = (typeof ORACLE_QUERY_METHODS)[number];
export function isOracleQueryMethod(value: unknown): value is OracleQueryMethod {
  return typeof value === "string" && (ORACLE_QUERY_METHODS as readonly string[]).includes(value);
}

// ── descriptor ─────────────────────────────────────────────────────────────────

/**
 * One `.vue` scenario probe's binding to its `.ts` oracle anchor plus the
 * per-method expectation the comparison enforces. The `.vue` anchor is the
 * scenario probe's own `anchor`; this adds the corresponding `.ts` anchor and the
 * intended-semantics assertions.
 */
export interface OracleBinding {
  /** The id of the `.vue` scenario probe this binds to (its method drives the query). */
  readonly probeId: string;
  /** The named anchor in the `.ts` oracle file the baseline provider is queried at. */
  readonly oracleAnchor: string;
  /** Completion: labels the intended Vue semantics require verter to surface. */
  readonly requiredLabels?: readonly string[];
  /** Hover: type tokens the intended Vue semantics require in verter's stripped label. */
  readonly requiredSnippets?: readonly string[];
  /** Definition: the expected authored Vue identity verter must resolve to. */
  readonly expected?: ExpectedDefinition;
  /** Diagnostics: code → the diagnostic's known true `.vue` source span. */
  readonly knownSourceSpans?: Readonly<Record<string, Range>>;
  /** Completion: a trigger character to drive the query with. */
  readonly triggerCharacter?: string;
}

/**
 * A curated semantic oracle: a `.ts` file mirroring the intended Vue semantics of
 * a paired `.vue` scenario, plus the per-probe bindings the runner drives.
 */
export interface SemanticOracle {
  readonly family: OracleFamily;
  /** The oracle `.ts` path (relative to `oracles/semantic/`). */
  readonly oracleFile: string;
  /** The id of the paired `.vue` scenario. */
  readonly scenarioId: string;
  readonly bindings: readonly OracleBinding[];
}

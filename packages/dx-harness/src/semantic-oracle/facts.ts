/**
 * Semantic-fact extraction for the oracle runner.
 *
 * Two sides, each folded into the SHARED comparable shapes the `vueSemanticValidity`
 * diff consumes:
 *  - the verter side — a raw `.vue` LSP response folded through the shared
 *    LSP-response normalizers into the `Canonical*` forms (these helpers name the
 *    oracle role and reuse the normalizers verbatim, never re-deriving normalization);
 *  - the oracle side — a `verter_dx_baseline` bridge response folded into the
 *    `ProviderResult<B>` the {@link classifyProbe} orchestrator expects, with a typed
 *    bridge refusal (`error` frame) passing through as `ok: false` rather than
 *    throwing — a refusal is an expected differential outcome, not a transport fault.
 *
 * Every function is pure and deterministic.
 */

import type {
  DiagnosticsResponse,
  ErrorResponse,
  NormalizedDiagnostic,
  NormalizedHover,
  NormalizedLocation,
  QueryResponse,
} from "../baseline/bridgeClient.js";
import type { BaselineCompletion, ProviderResult } from "../differential/index.js";
import {
  normalizeCompletion,
  normalizeDefinition,
  normalizeHover,
  type CanonicalCompletionList,
  type CanonicalDefinitionTarget,
  type CanonicalHover,
} from "../normalize/index.js";
import type {
  CompletionResponse,
  DefinitionResponse,
  HoverResponse,
} from "../normalize/lspTypes.js";

// ── verter side: raw `.vue` LSP response → `Canonical*` (the shared normalize/ layer) ─

/** Fold verter's raw `.vue` completion response into the canonical set. */
export function verterCompletionFact(raw: CompletionResponse): CanonicalCompletionList {
  return normalizeCompletion(raw);
}

/** Fold verter's raw `.vue` hover response into the canonical hover (`null` = no hover). */
export function verterHoverFact(raw: HoverResponse): CanonicalHover | null {
  return normalizeHover(raw);
}

/** Fold verter's raw `.vue` definition response into canonical targets. */
export function verterDefinitionFact(
  raw: DefinitionResponse,
): readonly CanonicalDefinitionTarget[] {
  return normalizeDefinition(raw);
}

// ── oracle side: bridge response → `ProviderResult<B>` ─────────────────────────

/** Fold a bridge completion query response into a `BaselineCompletion` ProviderResult. */
export function bridgeCompletionFact(
  response: QueryResponse | ErrorResponse,
): ProviderResult<BaselineCompletion> {
  if (response.type === "error") return { ok: false, error: response };
  const result = response.result;
  if (result.kind !== "completion") {
    throw new Error(`expected a completion query result, got "${result.kind}"`);
  }
  return { ok: true, output: { items: result.items, isIncomplete: result.isIncomplete } };
}

/** Fold a bridge hover query response into a `NormalizedHover | null` ProviderResult. */
export function bridgeHoverFact(
  response: QueryResponse | ErrorResponse,
): ProviderResult<NormalizedHover | null> {
  if (response.type === "error") return { ok: false, error: response };
  const result = response.result;
  if (result.kind !== "hover") {
    throw new Error(`expected a hover query result, got "${result.kind}"`);
  }
  return { ok: true, output: result.hover };
}

/** Fold a bridge definition query response into a location ProviderResult. */
export function bridgeDefinitionFact(
  response: QueryResponse | ErrorResponse,
): ProviderResult<readonly NormalizedLocation[]> {
  if (response.type === "error") return { ok: false, error: response };
  const result = response.result;
  if (result.kind !== "definition") {
    throw new Error(`expected a definition query result, got "${result.kind}"`);
  }
  return { ok: true, output: result.locations };
}

/** Fold a bridge diagnostics response into a diagnostic ProviderResult. */
export function bridgeDiagnosticsFact(
  response: DiagnosticsResponse | ErrorResponse,
): ProviderResult<readonly NormalizedDiagnostic[]> {
  if (response.type === "error") return { ok: false, error: response };
  return { ok: true, output: response.diagnostics };
}

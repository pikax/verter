/**
 * Definition response normalizer.
 *
 * Folds any LSP definition shape (`Location | Location[] | LocationLink |
 * LocationLink[] | null`) into a canonical `{ uri, range, fromGenerated? }[]`.
 *
 * Failure invariant: a definition target is NEVER invalid merely because its range
 * starts on line 0 — a precise line-0 range is a valid declaration position. The
 * provided predicate helpers fail by SYMBOL IDENTITY (source file + source
 * range, {@link definitionMatchesExpected}) or by a GENERATED-ONLY-UNMAPPED target
 * ({@link isUnmappedGeneratedOnly} / {@link isDefinitionGeneratedOnly}) — never by
 * `line === 0`.
 */

import type { DefinitionResponse, Range } from "./lspTypes.js";
import { coerceRange, isGeneratedUri, rangesEqual } from "./shared.js";

/** A canonical definition target. `fromGenerated` is set only for generated artifacts. */
export interface CanonicalDefinitionTarget {
  readonly uri: string;
  readonly range: Range;
  /** `true` when `uri` is a verter-generated artifact (`.vue.tsx`/`.vue.jsx`/`.vue.ts`). */
  readonly fromGenerated?: boolean;
}

/** The expected SOURCE-side identity of a definition: authored file and/or range. */
export interface ExpectedDefinition {
  readonly uri?: string;
  readonly range?: Range;
}

function makeTarget(uri: string, range: Range): CanonicalDefinitionTarget {
  return isGeneratedUri(uri) ? { uri, range, fromGenerated: true } : { uri, range };
}

/**
 * Project one `Location` / `LocationLink` entry to a canonical target. An entry
 * with no recognizable string uri (`null`, a non-object, a bare `{}`) is skipped;
 * an entry WITH a valid uri keeps it and folds any missing/malformed nested range
 * to the `(0,0)` default via {@link coerceRange}, so a broken range never leaks
 * into the canonical output (where a downstream `rangesEqual` would throw).
 */
function toTarget(entry: unknown): CanonicalDefinitionTarget | undefined {
  if (!entry || typeof entry !== "object") return undefined;
  const obj = entry as Record<string, unknown>;
  // LocationLink: the precise symbol range is `targetSelectionRange`; fall back to
  // `targetRange` when the selection range is absent or not an object.
  if (typeof obj.targetUri === "string") {
    const selection = obj.targetSelectionRange;
    const rangeSource =
      selection !== null && typeof selection === "object" ? selection : obj.targetRange;
    return makeTarget(obj.targetUri, coerceRange(rangeSource));
  }
  // Location.
  if (typeof obj.uri === "string") {
    return makeTarget(obj.uri, coerceRange(obj.range));
  }
  return undefined;
}

/**
 * Normalize a raw definition response into canonical targets. Total over
 * `null`/`undefined` (→ `[]`); the line-0 position of any target is preserved
 * as-is and is NOT treated as invalid.
 */
export function normalizeDefinition(raw: DefinitionResponse): readonly CanonicalDefinitionTarget[] {
  if (raw === null || raw === undefined) return [];
  const entries = Array.isArray(raw) ? raw : [raw];
  const targets: CanonicalDefinitionTarget[] = [];
  for (const entry of entries) {
    const target = toTarget(entry);
    if (target) targets.push(target);
  }
  return targets;
}

/**
 * Whether a target resolved ONLY into a generated artifact (never mapped back to
 * authored source) — the generated-only-unmapped failure. This is the
 * generated-only predicate, NOT a line-number check.
 */
export function isUnmappedGeneratedOnly(target: CanonicalDefinitionTarget): boolean {
  return target.fromGenerated === true;
}

/**
 * Whether a NON-EMPTY definition result is entirely generated-only — every target
 * is unmapped-generated and not one authored source target was produced. An empty
 * result is not "generated-only" (there is no generated target at all).
 */
export function isDefinitionGeneratedOnly(targets: readonly CanonicalDefinitionTarget[]): boolean {
  return targets.length > 0 && targets.every(isUnmappedGeneratedOnly);
}

/**
 * Whether some target matches the expected SOURCE identity — by file (`uri`) and
 * range, the declared fields only. Matching is by symbol identity, NEVER by line
 * number: a precise line-0 target whose file+range match passes.
 */
export function definitionMatchesExpected(
  targets: readonly CanonicalDefinitionTarget[],
  expected: ExpectedDefinition,
): boolean {
  // An empty expectation is not a wildcard: with no declared identity there is
  // nothing to match against, so it never matches — a vacuous `true` would mask
  // a real miss.
  if (expected.uri === undefined && expected.range === undefined) return false;
  return targets.some((target) => {
    if (expected.uri !== undefined && target.uri !== expected.uri) return false;
    if (expected.range !== undefined && !rangesEqual(target.range, expected.range)) return false;
    return true;
  });
}

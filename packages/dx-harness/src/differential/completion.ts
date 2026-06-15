/**
 * Completion comparator.
 *
 * Compares verter's normalized completion set against a baseline provider's on
 * the same emitted TSX: required-label presence, the normalized label SET
 * (compared in BOTH directions, order-insensitive), kind, and insert/edit shape.
 * The DX-critical signal is No-Suggestions collapse: verter empty where the
 * baseline is non-empty.
 *
 * The field equivalence (label set + kind + insert/edit shape) is factored into
 * one place — {@link completionFieldDivergences} — so the forward verter-vs-baseline
 * comparison and the baseline-vs-baseline disagreement check cannot drift apart.
 *
 * Two surfaces are deliberately NOT verter-vs-baseline parity here:
 *  - Auto-import `additionalTextEdits`: the baseline normalized item carries none,
 *    so it cannot be compared against verter. The auto-import edit DIFFERENTIAL is
 *    owned by the later raw-LSP auto-import collector, which resolves the completion
 *    item and inspects its applied `textEdit`/`additionalTextEdits`.
 *  - Ranking: the verter `Canonical*` list is order-normalized upstream, so verter's
 *    own order is unavailable here. {@link compareBaselineRanking} therefore asserts a
 *    scenario's expected ranking against the BASELINE provider's order only — a
 *    baseline-side signal, never a verter divergence. Comparing verter's own ranking
 *    would require the normalizer to preserve verter's original emission order in a
 *    side channel.
 */

import type { NormalizedCompletionItem } from "../baseline/bridgeClient.js";
import { normalizeEol } from "../normalize/index.js";
import type { CanonicalCompletionItem, CanonicalCompletionList } from "../normalize/index.js";
import type { Divergence } from "./outcome.js";

/** The baseline provider's normalized completion result. */
export interface BaselineCompletion {
  readonly items: readonly NormalizedCompletionItem[];
  readonly isIncomplete: boolean;
}

/** Options driving the completion comparison. */
export interface CompletionCompareOptions {
  /** Labels that MUST appear in verter's set regardless of the baseline. */
  readonly requiredLabels?: readonly string[];
}

/** The provider-agnostic fields the completion equivalence compares. */
export interface ComparableCompletionItem {
  readonly label: string;
  readonly kind?: string;
  /** The effective insert/edit text (an edit's text, else `insertText`, else `label`). */
  readonly insert: string;
}

/** The effective insert text of a verter item: its edit's text, else `insertText`, else `label`. */
function verterInsert(item: CanonicalCompletionItem): string {
  if (item.textEdit && typeof item.textEdit.newText === "string") return item.textEdit.newText;
  return item.insertText ?? item.label;
}

/** The effective insert text of a baseline item: its `insertText`, else `label`. */
function baselineInsert(item: NormalizedCompletionItem): string {
  return item.insertText ?? item.label;
}

/** Reduce a verter item to the comparable field view. */
export function verterComparable(item: CanonicalCompletionItem): ComparableCompletionItem {
  return {
    label: item.label,
    ...(item.kind !== undefined ? { kind: item.kind } : {}),
    insert: verterInsert(item),
  };
}

/** Reduce a baseline item to the comparable field view. */
export function baselineComparable(item: NormalizedCompletionItem): ComparableCompletionItem {
  return {
    label: item.label,
    ...(item.kind !== undefined ? { kind: item.kind } : {}),
    insert: baselineInsert(item),
  };
}

/** First-wins label → item map (a label rarely repeats; the first is canonical). */
function indexByLabel<T extends { label: string }>(items: readonly T[]): Map<string, T> {
  const map = new Map<string, T>();
  for (const item of items) if (!map.has(item.label)) map.set(item.label, item);
  return map;
}

/** Whether two label sequences are identical (element-wise, order-sensitive). */
function sameOrder(a: readonly string[], b: readonly string[]): boolean {
  return a.length === b.length && a.every((label, i) => b[i] === label);
}

/**
 * The shared completion field equivalence: the symmetric label-set parity (a `right`-only
 * label is `missingLabel`, a `left`-only label is `extraLabel`) plus, per shared label,
 * a case-insensitive `kind` comparison (a kind on exactly one side counts) and an
 * EOL-normalized insert/edit-shape comparison. In the forward path `left` is verter and
 * `right` is the baseline; the baseline-vs-baseline disagreement check passes the two
 * providers. Returns a flat divergence list (empty = the field views agree).
 */
export function completionFieldDivergences(
  left: readonly ComparableCompletionItem[],
  right: readonly ComparableCompletionItem[],
): Divergence[] {
  const divergences: Divergence[] = [];
  const leftByLabel = indexByLabel(left);
  const rightByLabel = indexByLabel(right);

  // Label-set parity, both directions.
  for (const label of rightByLabel.keys()) {
    if (!leftByLabel.has(label)) {
      divergences.push({
        class: "missingLabel",
        detail: `completion label "${label}" is present in the baseline but absent from verter`,
        baselineValue: label,
      });
    }
  }
  for (const label of leftByLabel.keys()) {
    if (!rightByLabel.has(label)) {
      divergences.push({
        class: "extraLabel",
        detail: `completion label "${label}" is present in verter but absent from the baseline`,
        verterValue: label,
      });
    }
  }

  // Per shared label: kind (one-sided counts) + insert/edit shape.
  for (const [label, l] of leftByLabel) {
    const r = rightByLabel.get(label);
    if (r === undefined) continue;
    const lk = l.kind?.toLowerCase();
    const rk = r.kind?.toLowerCase();
    if (lk !== rk) {
      divergences.push({
        class: "wrongKind",
        detail: `completion "${label}" kind differs`,
        verterValue: l.kind,
        baselineValue: r.kind,
      });
    }
    const li = normalizeEol(l.insert);
    const ri = normalizeEol(r.insert);
    if (li !== ri) {
      divergences.push({
        class: "insertEditShape",
        detail: `completion "${label}" insert/edit text differs`,
        verterValue: li,
        baselineValue: ri,
      });
    }
  }

  return divergences;
}

/**
 * Compare a verter completion list against a baseline completion result. Returns
 * a flat divergence list (empty = agreement). Total over empty inputs.
 */
export function compareCompletion(
  verter: CanonicalCompletionList,
  baseline: BaselineCompletion,
  options: CompletionCompareOptions = {},
): Divergence[] {
  const divergences: Divergence[] = [];
  const verterByLabel = indexByLabel(verter.items);
  const verterEmpty = verter.items.length === 0;

  // No-Suggestions collapse: verter offers nothing where the baseline does.
  if (verterEmpty && baseline.items.length > 0) {
    divergences.push({
      class: "noSuggestionsCollapse",
      detail: "verter returned no completions where the baseline returned a non-empty set",
      verterValue: [],
      baselineValue: baseline.items.map((item) => item.label),
    });
  }

  // The label-set / field comparison is the redundant noise of a total collapse —
  // skip it and let `noSuggestionsCollapse` be the signal. Field divergences carry
  // the baseline∖verter missing-label gap; required labels add the rest.
  const fieldDivergences = verterEmpty
    ? []
    : completionFieldDivergences(
        verter.items.map(verterComparable),
        baseline.items.map(baselineComparable),
      );
  const alreadyMissing = new Set<string>(
    fieldDivergences
      .filter((d) => d.class === "missingLabel")
      .map((d) => d.baselineValue as string),
  );
  divergences.push(...fieldDivergences);

  // Required labels: a scenario-required label absent from verter, not already
  // reported as a baseline∖verter missing-label gap.
  for (const label of options.requiredLabels ?? []) {
    if (!verterByLabel.has(label) && !alreadyMissing.has(label)) {
      divergences.push({
        class: "missingLabel",
        detail: `required completion label "${label}" is absent from verter's set`,
        baselineValue: label,
      });
    }
  }

  return divergences;
}

/**
 * Assert a scenario's expected label ranking against the BASELINE provider's order
 * (restricted to the asserted labels). This is a baseline-side signal — verter's own
 * order is normalized away upstream and is not compared here — so a mismatch is routed
 * to a non-verter outcome by the caller, never folded into a verter divergence.
 */
export function compareBaselineRanking(
  baseline: BaselineCompletion,
  expectedRanking: readonly string[],
): Divergence[] {
  const assertedSet = new Set(expectedRanking);
  const baselineOrder = baseline.items
    .map((item) => item.label)
    .filter((label) => assertedSet.has(label));
  if (sameOrder(baselineOrder, expectedRanking)) return [];
  return [
    {
      class: "rankingMismatch",
      detail: "baseline completion ranking does not match the asserted order",
      verterValue: expectedRanking,
      baselineValue: baselineOrder,
    },
  ];
}

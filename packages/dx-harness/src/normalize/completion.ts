/**
 * Completion response normalizer.
 *
 * Folds a raw `CompletionList | CompletionItem[] | null` into a canonical,
 * stably-sorted, de-duplicated set so the differential compares SETS, not server
 * emission order. The auto-import `textEdit` / `additionalTextEdits` are preserved
 * VERBATIM (the auto-import collector applies them to the harness buffer); no item
 * is discarded. An empty set raises `noSuggestionsCollapse`, the DX No-Suggestions
 * signal.
 */

import type { CompletionEdit, CompletionItem, CompletionResponse, TextEdit } from "./lspTypes.js";
import { completionItemKindName, stableStringify } from "./shared.js";

/** A canonical completion item; `textEdit`/`additionalTextEdits` are preserved verbatim. */
export interface CanonicalCompletionItem {
  readonly label: string;
  /** The `CompletionItemKind` name (see {@link completionItemKindName}). */
  readonly kind?: string;
  readonly detail?: string;
  readonly insertText?: string;
  readonly textEdit?: CompletionEdit;
  readonly additionalTextEdits?: readonly TextEdit[];
}

/** A canonical completion result: a sorted/deduped item set plus the DX-collapse flag. */
export interface CanonicalCompletionList {
  readonly items: readonly CanonicalCompletionItem[];
  readonly isIncomplete: boolean;
  /** `true` when the set is empty — the DX No-Suggestions collapse signal. */
  readonly noSuggestionsCollapse: boolean;
}

function toCanonical(item: unknown): CanonicalCompletionItem {
  // Every list entry is an `any` from the raw client: a `null`, a non-object, or a
  // malformed item must fold to a safe canonical item rather than dereferencing
  // `item.label`.
  const it = (item !== null && typeof item === "object" ? item : {}) as Record<string, unknown>;
  const out: {
    label: string;
    kind?: string;
    detail?: string;
    insertText?: string;
    textEdit?: CompletionEdit;
    additionalTextEdits?: readonly TextEdit[];
  } = { label: typeof it.label === "string" ? it.label : "" };
  const kind = completionItemKindName(typeof it.kind === "number" ? it.kind : undefined);
  if (kind !== undefined) out.kind = kind;
  if (typeof it.detail === "string") out.detail = it.detail;
  if (typeof it.insertText === "string") out.insertText = it.insertText;
  // Preserve the edits VERBATIM — the auto-import collector applies them; only the
  // container kind is guarded (object edit / array of edits), never the contents.
  if (it.textEdit !== null && typeof it.textEdit === "object") {
    out.textEdit = it.textEdit as CompletionEdit;
  }
  if (Array.isArray(it.additionalTextEdits)) {
    out.additionalTextEdits = it.additionalTextEdits as readonly TextEdit[];
  }
  return out;
}

/** A stable content key — label-led, so the sort is label-primary and deterministic. */
function contentKey(item: CanonicalCompletionItem): string {
  return stableStringify([
    item.label,
    item.kind ?? null,
    item.detail ?? null,
    item.insertText ?? null,
    item.textEdit ?? null,
    item.additionalTextEdits ?? null,
  ]);
}

/**
 * Normalize a raw completion response. Total over `null`/`undefined`/empty; the
 * result is order-insensitive (a permuted response normalizes equal) and free of
 * structurally-identical duplicates, while same-label different-detail items are
 * both kept.
 */
export function normalizeCompletion(raw: CompletionResponse): CanonicalCompletionList {
  let items: readonly CompletionItem[] = [];
  let isIncomplete = false;
  if (Array.isArray(raw)) {
    items = raw;
  } else if (raw && typeof raw === "object" && Array.isArray((raw as { items?: unknown }).items)) {
    const list = raw as { items: readonly CompletionItem[]; isIncomplete?: boolean };
    items = list.items;
    isIncomplete = list.isIncomplete === true;
  }

  const byKey = new Map<string, CanonicalCompletionItem>();
  for (const item of items) {
    const canonical = toCanonical(item);
    const key = contentKey(canonical);
    if (!byKey.has(key)) byKey.set(key, canonical);
  }

  const sorted = [...byKey.entries()]
    .sort((a, b) => (a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0))
    .map((entry) => entry[1]);

  return { items: sorted, isIncomplete, noSuggestionsCollapse: sorted.length === 0 };
}

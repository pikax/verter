import { describe, expect, it } from "vitest";

import type { CompletionItem, CompletionList, TextEdit } from "../src/normalize/index.js";
import { normalizeCompletion } from "../src/normalize/index.js";

const range = (sl: number, sc: number, el: number, ec: number) => ({
  start: { line: sl, character: sc },
  end: { line: el, character: ec },
});

describe("normalizeCompletion — empty / collapse handling", () => {
  it("collapses a null / undefined response to an empty, collapse-flagged set", () => {
    for (const empty of [null, undefined]) {
      const out = normalizeCompletion(empty);
      expect(out.items).toEqual([]);
      expect(out.isIncomplete).toBe(false);
      expect(out.noSuggestionsCollapse).toBe(true);
    }
  });

  it("flags `noSuggestionsCollapse` for an empty list and NOT for a populated one", () => {
    const emptyList: CompletionList = { isIncomplete: false, items: [] };
    expect(normalizeCompletion(emptyList).noSuggestionsCollapse).toBe(true);
    expect(normalizeCompletion([]).noSuggestionsCollapse).toBe(true);

    const populated: CompletionItem[] = [{ label: "foo" }];
    expect(normalizeCompletion(populated).noSuggestionsCollapse).toBe(false);
  });

  it("carries `isIncomplete` from a CompletionList through verbatim", () => {
    const list: CompletionList = { isIncomplete: true, items: [{ label: "foo" }] };
    expect(normalizeCompletion(list).isIncomplete).toBe(true);
    // A bare item array is, by LSP, always complete.
    expect(normalizeCompletion([{ label: "foo" }]).isIncomplete).toBe(false);
  });
});

describe("normalizeCompletion — order-insensitivity and dedup", () => {
  it("normalizes two different server orderings to an EQUAL set", () => {
    const a: CompletionItem[] = [
      { label: "bar", kind: 5 },
      { label: "foo", kind: 3 },
      { label: "baz", kind: 6 },
    ];
    const b: CompletionItem[] = [
      { label: "foo", kind: 3 },
      { label: "baz", kind: 6 },
      { label: "bar", kind: 5 },
    ];
    expect(normalizeCompletion(a)).toEqual(normalizeCompletion(b));
    // …and the sorted order is stable / label-led (NOT the emission order).
    expect(normalizeCompletion(a).items.map((i) => i.label)).toEqual(["bar", "baz", "foo"]);
  });

  it("de-duplicates structurally-identical items but keeps same-label different-detail", () => {
    const items: CompletionItem[] = [
      { label: "foo", kind: 3, detail: "() => void" },
      { label: "foo", kind: 3, detail: "() => void" }, // exact duplicate → collapses
      { label: "foo", kind: 3, detail: "(x: number) => void" }, // different detail → kept
    ];
    const out = normalizeCompletion(items);
    expect(out.items).toHaveLength(2);
    const details = out.items.map((i) => i.detail);
    expect(details).toContain("() => void");
    expect(details).toContain("(x: number) => void");
  });

  it("maps the numeric CompletionItemKind to a stable name", () => {
    const out = normalizeCompletion([{ label: "f", kind: 3 }]); // 3 = Function
    expect(out.items[0].kind).toBe("Function");
  });
});

describe("normalizeCompletion — auto-import edit preservation", () => {
  it("PRESERVES textEdit and additionalTextEdits verbatim (the auto-import collector applies them)", () => {
    const textEdit: TextEdit = { range: range(5, 2, 5, 6), newText: "Drawer" };
    const additionalTextEdits: TextEdit[] = [
      { range: range(0, 0, 0, 0), newText: "import { Drawer } from './drawer'\n" },
    ];
    const item: CompletionItem = {
      label: "Drawer",
      kind: 7,
      detail: "class Drawer",
      insertText: "Drawer",
      textEdit,
      additionalTextEdits,
    };
    const out = normalizeCompletion([item]);
    expect(out.items).toHaveLength(1);
    const got = out.items[0];
    // The edits survive — they are NOT discarded by normalization.
    expect(got.textEdit).toBeDefined();
    expect(got.textEdit).toEqual(textEdit);
    expect(got.additionalTextEdits).toBeDefined();
    expect(got.additionalTextEdits).toEqual(additionalTextEdits);
    // Negative: the import edit text is intact for the auto-import collector.
    expect(got.additionalTextEdits?.[0]?.newText).toContain("import { Drawer }");
  });

  it("does NOT discard items lacking a kind or detail", () => {
    const out = normalizeCompletion([{ label: "plain" }]);
    expect(out.items).toHaveLength(1);
    expect(out.items[0].label).toBe("plain");
    expect(out.items[0].kind).toBeUndefined();
  });
});

describe("normalizeCompletion — totality over malformed entries and edits", () => {
  // Each list entry is an `any` from the raw client; a null / non-object entry
  // must not throw on `item.label`, and a malformed field must fold safely.
  it("folds null / undefined / non-object entries to a safe item without throwing", () => {
    expect(() =>
      normalizeCompletion([null, undefined, 42, "x"] as unknown as CompletionItem[]),
    ).not.toThrow();
    const out = normalizeCompletion([null] as unknown as CompletionItem[]);
    expect(out.items).toHaveLength(1);
    expect(out.items[0].label).toBe("");
  });

  it("coerces a non-string label/detail/insertText and a non-number kind", () => {
    const out = normalizeCompletion([
      { label: 5, detail: 1, insertText: {}, kind: "x" },
    ] as unknown as CompletionItem[]);
    expect(out.items[0].label).toBe("");
    expect(out.items[0].detail).toBeUndefined();
    expect(out.items[0].insertText).toBeUndefined();
    expect(out.items[0].kind).toBeUndefined();
  });

  it("omits a non-object `textEdit` and a non-array `additionalTextEdits` (malformed containers)", () => {
    const out = normalizeCompletion([{ label: "x", textEdit: 42 }] as unknown as CompletionItem[]);
    expect(out.items[0].textEdit).toBeUndefined();
    const out2 = normalizeCompletion([
      { label: "y", additionalTextEdits: 7 },
    ] as unknown as CompletionItem[]);
    expect(out2.items[0].additionalTextEdits).toBeUndefined();
  });

  it("preserves an object `textEdit` VERBATIM even when its inner shape is unusual (no throw)", () => {
    // An object edit is preserved verbatim — the auto-import collector applies it;
    // normalization must neither rewrite its contents nor throw on a deep oddity.
    const weird = { range: { start: null, end: null }, newText: "z" } as unknown;
    expect(() =>
      normalizeCompletion([{ label: "w", textEdit: weird }] as unknown as CompletionItem[]),
    ).not.toThrow();
    const out = normalizeCompletion([
      { label: "w", textEdit: weird },
    ] as unknown as CompletionItem[]);
    expect(out.items[0].textEdit).toEqual(weird);
  });

  it("does not change a well-formed item's canonical output", () => {
    const item: CompletionItem = { label: "foo", kind: 3, detail: "d", insertText: "i" };
    const out = normalizeCompletion([item]);
    expect(out.items[0]).toEqual({ label: "foo", kind: "Function", detail: "d", insertText: "i" });
  });
});

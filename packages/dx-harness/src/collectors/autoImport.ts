/**
 * Auto-import collector (raw-LSP).
 *
 * The raw-LSP gate: request completion, RESOLVE the chosen item (the
 * `completionItem/resolve` request — verter's `TypeProvider.resolve_completion`
 * already backs it, so no product change), inspect its `textEdit` +
 * `additionalTextEdits`, APPLY them to the harness buffer, and VERIFY the resulting
 * import text is correct. A resolve that produces no edit, or one whose applied text
 * lacks the expected import, is a user-visible failure (the import never lands).
 *
 * The edit application is a pure {@link CodeTransform}-free splice over LSP
 * `TextEdit`s — completion edits are non-overlapping, so they are converted to UTF-16
 * offsets and applied right-to-left, leaving lower offsets valid.
 */

import { DocumentPositions, type PositionEncoding } from "@verter/lsp-test-client";

import {
  normalizeCompletion,
  type CanonicalCompletionItem,
  type CompletionEdit,
  type TextEdit,
} from "../normalize/index.js";
import type { CompletionResponse } from "../normalize/lspTypes.js";
import type { EditStep } from "../scenario/index.js";
import { openDocument, sendTickChange, type CollectorLspClient } from "./client.js";
import { EditBuffer, runEditScript } from "./editLoop.js";
import {
  collectorEvent,
  type CollectorEvent,
  type CollectorEventKey,
  type EventSink,
  type SignalProvenance,
} from "./event.js";

const RAW_LSP: SignalProvenance = { detectedBy: "rawLsp" };

/** A flat `{ range, newText }` edit (the comparable form of either completion-edit shape). */
interface FlatEdit {
  readonly range: TextEdit["range"];
  readonly newText: string;
}

/** Unwrap a completion `textEdit` to a flat edit: a `TextEdit` keeps its range; an `InsertReplaceEdit` uses `replace`. */
function unwrapCompletionEdit(edit: CompletionEdit): FlatEdit {
  if ("range" in edit) return { range: edit.range, newText: edit.newText };
  // InsertReplaceEdit: accept replaces the existing token (the conventional accept behavior).
  return { range: edit.replace, newText: edit.newText };
}

/**
 * The flat edits a resolved completion item applies: its main `textEdit` (unwrapped)
 * followed by its `additionalTextEdits` (the auto-import statements), in that order.
 */
export function completionItemEdits(item: CanonicalCompletionItem): FlatEdit[] {
  const edits: FlatEdit[] = [];
  if (item.textEdit !== undefined) edits.push(unwrapCompletionEdit(item.textEdit));
  for (const edit of item.additionalTextEdits ?? [])
    edits.push({ range: edit.range, newText: edit.newText });
  return edits;
}

/**
 * Apply non-overlapping LSP text edits to `text`. Each edit's range is converted to
 * UTF-16 offsets in `encoding`, and the edits are spliced from the highest start
 * offset down, so applying one never invalidates a not-yet-applied lower offset.
 */
export function applyTextEdits(
  text: string,
  edits: readonly FlatEdit[],
  encoding: PositionEncoding = "utf-16",
): string {
  const doc = new DocumentPositions(text);
  const resolved = edits
    .map((edit) => ({
      start: doc.positionToUtf16(edit.range.start, encoding),
      end: doc.positionToUtf16(edit.range.end, encoding),
      newText: edit.newText,
    }))
    .sort((a, b) => b.start - a.start || b.end - a.end);
  // The right-to-left splice presumes the ranges do not overlap (completion edits never
  // do). Sorted by descending start, a lower-start edit whose end runs past the next
  // higher-start edit's start is an overlap — fail loudly rather than splice garbage.
  // Abutting ranges (one ends exactly where the next begins) are half-open and fine.
  for (let i = 1; i < resolved.length; i++) {
    const higher = resolved[i - 1];
    const lower = resolved[i];
    if (lower.end > higher.start) {
      throw new Error(
        `applyTextEdits: overlapping edits are not supported ` +
          `([${lower.start}, ${lower.end}) overlaps [${higher.start}, ${higher.end}))`,
      );
    }
  }
  let out = text;
  for (const edit of resolved) {
    out = out.slice(0, edit.start) + edit.newText + out.slice(edit.end);
  }
  return out;
}

/** Apply a resolved completion item's edits (main + additional) to `text`. */
export function applyResolvedCompletion(
  text: string,
  item: CanonicalCompletionItem,
  encoding: PositionEncoding = "utf-16",
): string {
  return applyTextEdits(text, completionItemEdits(item), encoding);
}

/** Find the raw completion item with `label` (for resolve, which needs the item's `data`). */
export function findCompletionItem(raw: CompletionResponse, label: string): unknown {
  // Treat the response as `unknown` so the list-vs-`{items}` shape narrowing needs no
  // cast through the typed union (whose array arm does not overlap a record shape).
  const value: unknown = raw;
  const items: readonly unknown[] = Array.isArray(value)
    ? value
    : value !== null &&
        typeof value === "object" &&
        Array.isArray((value as { items?: unknown }).items)
      ? (value as { items: readonly unknown[] }).items
      : [];
  return items.find((item) => (item as { label?: unknown } | null)?.label === label);
}

/** The exact binding a resolved auto-import edit must produce: a local symbol from a module. */
export interface ExpectedImport {
  /** The local name the import must bind (a named binding, an alias, or a default). */
  readonly symbol: string;
  /** The EXACT module specifier the import must reference (no substring match). */
  readonly module: string;
}

/** A parsed ES `import … from "module"` declaration: its module plus the local names it binds. */
export interface ParsedImportDeclaration {
  readonly module: string;
  /** The local names the declaration binds (default, namespace alias, and named/aliased bindings). */
  readonly bindings: readonly string[];
}

/** Drop a leading `type` modifier token (`import type …` / a `{ type Foo }` named spec). */
function stripTypeModifier(token: string): string {
  return token.replace(/^type\s+/, "").trim();
}

/** The local names a single import clause (the text between `import` and `from`) binds. */
function parseImportClauseBindings(clause: string): string[] {
  const bindings: string[] = [];
  let rest = stripTypeModifier(clause.trim());

  // Named bindings: `{ a, b as c, type D }`. Each spec binds its ALIAS when present,
  // else its first token — the exact name, never a prefix of a longer identifier.
  const braceStart = rest.indexOf("{");
  if (braceStart !== -1) {
    const braceEnd = rest.indexOf("}", braceStart);
    const inner =
      braceEnd === -1 ? rest.slice(braceStart + 1) : rest.slice(braceStart + 1, braceEnd);
    for (const raw of inner.split(",")) {
      const spec = stripTypeModifier(raw.trim());
      if (spec === "") continue;
      const aliased = /\bas\b\s+(\w+)/.exec(spec);
      bindings.push(aliased !== null ? aliased[1] : spec.split(/\s+/)[0]);
    }
    rest = rest.slice(0, braceStart);
  }

  // Namespace: `* as ns`.
  const namespace = /\*\s*as\s+(\w+)/.exec(rest);
  if (namespace !== null) bindings.push(namespace[1]);

  // Default binding: a bare leading identifier (before any `,`/`{`/`*`).
  for (const part of rest.replace(/\*\s*as\s+\w+/, "").split(",")) {
    const name = part.trim();
    if (/^\w+$/.test(name)) bindings.push(name);
  }

  return bindings;
}

/**
 * Parse the CONTIGUOUS ES `import … from "module"` declarations in `text` into their
 * module specifier and bound local names. The clause may not cross a `;`, so a
 * preceding side-effect import (`import "x";`) never lets a later statement's `from`
 * stitch into one declaration; whitespace/newlines inside the clause are tolerated,
 * so a multi-line named-import block stays one declaration. A side-effect import
 * (no `from`) binds nothing and is skipped.
 */
export function parseImportDeclarations(text: string): ParsedImportDeclaration[] {
  const declarations: ParsedImportDeclaration[] = [];
  const re = /\bimport\b\s+([^;]*?)\bfrom\b\s*(['"])([^'"]+)\2/g;
  let match: RegExpExecArray | null;
  while ((match = re.exec(text)) !== null) {
    declarations.push({ module: match[3], bindings: parseImportClauseBindings(match[1]) });
  }
  return declarations;
}

/** The pure inputs to one auto-import verification. */
export interface AutoImportInput {
  readonly key: CollectorEventKey;
  /** The buffer text BEFORE the resolved edits apply. */
  readonly before: string;
  /** The RESOLVED completion item (its edits are applied + verified). */
  readonly item: CanonicalCompletionItem;
  /** The EXACT `{ symbol, module }` binding the applied import declaration must produce. */
  readonly expectedImport: ExpectedImport;
  readonly encoding?: PositionEncoding;
}

/**
 * Apply a resolved completion item's edits and verify the resulting import STRUCTURALLY.
 * A resolve that produced no effective edit is an `auto_import_empty_edit`; an applied
 * buffer with no contiguous import declaration binding the EXACT symbol from the EXACT
 * module is an `auto_import_wrong_text`; a binding the resolved item did NOT introduce
 * (it already existed before the edit) is an `auto_import_not_introduced`; only a binding
 * the resolved item newly introduces is an `auto_import_applied`.
 *
 * Verification inspects the import declarations as parsed `{ module, bindings }` units —
 * never an independent whole-buffer substring scan — and gates on the DELTA the resolved
 * item produced: the `{ symbol, module }` binding must be satisfiable in the parsed
 * declarations of `after` but NOT already satisfiable in those of `before`. A whole-buffer
 * match alone is unsound — a pre-existing `import { helperValue } from "./helper"` would
 * let a resolved item that applied only a usage edit (no import) pass, a false green that
 * masks a real auto-import failure. A wrong-symbol same-module import
 * (`import { other } from "./helper"`) fails the symbol check; a wrong module whose
 * specifier merely CONTAINS the expected one (`import { helperValue } from "./helper-extra"`)
 * fails the exact-module check that a substring scan would wrongly pass; a merge that adds
 * the symbol to an existing same-module import is a genuine application (new in `after`).
 */
export function verifyAutoImport(input: AutoImportInput): CollectorEvent[] {
  const { key, before, item, expectedImport } = input;
  const events: CollectorEvent[] = [];
  const edits = completionItemEdits(item);
  const after = applyTextEdits(before, edits, input.encoding);

  if (edits.length === 0 || after === before) {
    events.push(
      collectorEvent({
        collector: "autoImport",
        signal: "auto_import_empty_edit",
        ok: false,
        severity: "userVisible",
        provenance: RAW_LSP,
        key,
        detail: `resolving "${item.label}" produced no import edit`,
        data: { label: item.label, editCount: edits.length },
      }),
    );
    return events;
  }

  // Whether the parsed declarations satisfy the exact `{ symbol, module }` binding.
  const bindsExpected = (decls: readonly ParsedImportDeclaration[]): boolean =>
    decls.some(
      (decl) =>
        decl.module === expectedImport.module && decl.bindings.includes(expectedImport.symbol),
    );
  const declarations = parseImportDeclarations(after);
  const boundAfter = bindsExpected(declarations);
  // The binding must be INTRODUCED by the resolved item — present in `after` but not
  // already in `before`. A whole-buffer match of `after` alone is unsound: a pre-existing
  // same import would let an item that applied only a usage edit pass as `applied`.
  const boundBefore = bindsExpected(parseImportDeclarations(before));

  if (!boundAfter) {
    events.push(
      collectorEvent({
        collector: "autoImport",
        signal: "auto_import_wrong_text",
        ok: false,
        severity: "userVisible",
        provenance: RAW_LSP,
        key,
        detail: `the resolved import for "${item.label}" does not bind "${expectedImport.symbol}" from "${expectedImport.module}"`,
        data: { label: item.label, expectedImport, declarations, after },
      }),
    );
    return events;
  }

  if (boundBefore) {
    events.push(
      collectorEvent({
        collector: "autoImport",
        signal: "auto_import_not_introduced",
        ok: false,
        severity: "userVisible",
        provenance: RAW_LSP,
        key,
        detail: `resolving "${item.label}" introduced no import — "${expectedImport.symbol}" from "${expectedImport.module}" already bound before the edit`,
        data: { label: item.label, expectedImport, declarations, after },
      }),
    );
    return events;
  }

  events.push(
    collectorEvent({
      collector: "autoImport",
      signal: "auto_import_applied",
      ok: true,
      severity: "userVisible",
      provenance: RAW_LSP,
      key,
      detail: `resolving "${item.label}" bound "${expectedImport.symbol}" from "${expectedImport.module}"`,
      data: { label: item.label, expectedImport },
    }),
  );
  return events;
}

// ── live raw-LSP driver ──────────────────────────────────────────────────────

/** Options for the live {@link collectAutoImport} run. */
export interface CollectAutoImportOptions {
  readonly client: CollectorLspClient;
  readonly sink: EventSink;
  readonly uri: string;
  readonly languageId?: string;
  readonly buffer: EditBuffer;
  /** Edits typed before completion is requested (e.g. typing the component name). */
  readonly script?: readonly EditStep[];
  readonly scenario: string;
  readonly probe: string;
  readonly anchor: string;
  readonly provider: string;
  /** The completion label to resolve (the auto-import candidate). */
  readonly targetLabel: string;
  /** The EXACT `{ symbol, module }` binding the resolved import must produce. */
  readonly expectedImport: ExpectedImport;
  readonly requestTimeoutMs?: number;
}

/**
 * Drive verter through the edit script, request completion at the cursor, resolve the
 * target item via `completionItem/resolve`, apply its edits to the buffer text, and
 * verify the resulting import. Emits an `auto_import_no_candidate` finding when the
 * target label is not offered at all.
 */
export async function collectAutoImport(options: CollectAutoImportOptions): Promise<void> {
  const { client, sink, uri, buffer } = options;
  const script = options.script ?? [];
  openDocument(client, uri, options.languageId ?? "vue", buffer.text, buffer.version);
  let lastCursor = buffer.anchorOffset(options.anchor);
  await runEditScript(buffer, script, (tick) => {
    sendTickChange(client, uri, tick);
    lastCursor = tick.cursor;
  });

  const key: CollectorEventKey = {
    scenario: options.scenario,
    editStepIndex: script.length - 1,
    driver: "rawLsp",
    provider: options.provider,
    probe: options.probe,
    version: buffer.version,
    anchor: options.anchor,
  };

  const position = new DocumentPositions(buffer.text).utf16ToPosition(
    lastCursor,
    client.positionEncoding,
  );
  const raw = await client.sendRequest<CompletionResponse>(
    "textDocument/completion",
    { textDocument: { uri }, position: { line: position.line, character: position.character } },
    options.requestTimeoutMs,
  );
  const rawItem = findCompletionItem(raw, options.targetLabel);
  if (rawItem === undefined) {
    sink.emit(
      collectorEvent({
        collector: "autoImport",
        signal: "auto_import_no_candidate",
        ok: false,
        severity: "userVisible",
        provenance: RAW_LSP,
        key,
        detail: `no completion candidate labelled "${options.targetLabel}" was offered`,
        data: { targetLabel: options.targetLabel },
      }),
    );
    return;
  }

  // Resolve the RAW item (it carries the `data` the server needs to compute the import).
  const resolvedRaw = await client.sendRequest<unknown>(
    "completionItem/resolve",
    rawItem,
    options.requestTimeoutMs,
  );
  const [resolved] = normalizeCompletion([resolvedRaw] as CompletionResponse).items;
  for (const event of verifyAutoImport({
    key,
    before: buffer.text,
    item: resolved ?? { label: options.targetLabel },
    expectedImport: options.expectedImport,
    encoding: client.positionEncoding,
  })) {
    sink.emit(event);
  }
}

/**
 * SSR/client dead-code elimination transforms.
 *
 * Replaces `import.meta.server`, `import.meta.client`, and `import.meta.env.SSR`
 * with boolean literals so bundlers can tree-shake dead branches, and strips
 * client/server-only component render calls. Every rewrite here changes the
 * generated text's byte length, so a caller carrying a source map for the
 * pre-rewrite code must have its mappings' generated columns shifted to match
 * — otherwise the published map points at stale positions once the code has
 * moved. `applyTextEdits` is the single map-aware primitive both rewrites
 * route through.
 */

import { decode, encode, type SourceMapSegment } from "@jridgewell/sourcemap-codec";

/** The result of a map-aware rewrite: the new code, and its map (if one travelled in). */
export interface SsrRewriteResult {
  code: string;
  map?: string;
}

/** A single literal-text replacement, expressed as an offset range in the ORIGINAL code. */
interface TextEdit {
  start: number;
  end: number;
  replacement: string;
}

/** Per-line column edit, used to shift a decoded map's generated columns. */
interface LineColumnEdit {
  start: number;
  end: number;
  delta: number;
}

/**
 * Apply a set of non-overlapping literal-text edits to `code`, and — when a
 * source map for the pre-edit `code` is supplied — shift the map's generated
 * columns so it still describes the rewritten text.
 *
 * Edits must not span a newline: every rewrite site here (`import.meta.*`
 * literals, `_resolveComponent(...)` calls) is a single-line token, so each
 * edit only ever needs to shift columns later on the SAME generated line —
 * no line renumbering is required.
 */
function applyTextEdits(
  code: string,
  map: string | undefined,
  edits: TextEdit[],
): SsrRewriteResult {
  if (edits.length === 0) return { code, map };

  const sorted = [...edits].sort((a, b) => a.start - b.start);

  let newCode = "";
  let cursor = 0;
  let lineIndex = 0;
  let lineStart = 0;
  let searchFrom = 0;
  const lineEditsByLine = new Map<number, LineColumnEdit[]>();

  for (const edit of sorted) {
    // Edits must be non-overlapping and non-duplicate. A duplicate/overlapping
    // edit (e.g. the same component name listed twice) would otherwise splice
    // its replacement in twice, producing invalid output — skip it instead.
    if (edit.start < cursor) continue;

    newCode += code.slice(cursor, edit.start);

    // Advance the line cursor to the line containing this edit.
    for (;;) {
      const newline = code.indexOf("\n", searchFrom);
      if (newline !== -1 && newline < edit.start) {
        lineIndex++;
        lineStart = newline + 1;
        searchFrom = lineStart;
      } else {
        break;
      }
    }

    const startCol = edit.start - lineStart;
    const endCol = edit.end - lineStart;
    const delta = edit.replacement.length - (edit.end - edit.start);
    const lineEdits = lineEditsByLine.get(lineIndex) ?? [];
    lineEdits.push({ start: startCol, end: endCol, delta });
    lineEditsByLine.set(lineIndex, lineEdits);

    newCode += edit.replacement;
    cursor = edit.end;
  }
  newCode += code.slice(cursor);

  return {
    code: newCode,
    map: map === undefined ? undefined : shiftMapColumns(map, lineEditsByLine),
  };
}

/**
 * Shift the generated-column field of every mapping segment past an edit by
 * that edit's length delta. A segment whose original column falls strictly
 * inside a replaced span is clamped to the replacement's new start column —
 * the token it named no longer exists, and the closest still-meaningful
 * position is where its replacement begins.
 *
 * An unparseable map is dropped (`undefined`) rather than passed through
 * unshifted: a stale map describing pre-rewrite positions is worse than no
 * map at all, since it silently mispoints instead of visibly disappearing.
 */
function shiftMapColumns(
  mapJson: string,
  lineEditsByLine: Map<number, LineColumnEdit[]>,
): string | undefined {
  let parsed: { mappings?: unknown; [key: string]: unknown };
  try {
    parsed = JSON.parse(mapJson);
  } catch {
    return undefined;
  }
  if (typeof parsed.mappings !== "string") return undefined;

  const decoded = decode(parsed.mappings);
  for (const [lineIndex, rawEdits] of lineEditsByLine) {
    const segments = decoded[lineIndex];
    if (!segments) continue;
    const edits = [...rawEdits].sort((a, b) => a.start - b.start);
    for (const segment of segments as SourceMapSegment[]) {
      const col = segment[0];
      let shift = 0;
      let clamped = false;
      for (const edit of edits) {
        if (col >= edit.end) {
          shift += edit.delta;
        } else if (col > edit.start) {
          segment[0] = edit.start + shift;
          clamped = true;
          break;
        } else {
          break;
        }
      }
      if (!clamped) segment[0] = col + shift;
    }
  }

  parsed.mappings = encode(decoded);
  return JSON.stringify(parsed);
}

/** Find every non-overlapping occurrence of a literal string, as `[start, end)` ranges. */
function findAllOccurrences(code: string, literal: string): Array<[number, number]> {
  const ranges: Array<[number, number]> = [];
  if (literal.length === 0) return ranges;
  let from = 0;
  for (;;) {
    const index = code.indexOf(literal, from);
    if (index === -1) break;
    ranges.push([index, index + literal.length]);
    from = index + literal.length;
  }
  return ranges;
}

/**
 * Replace SSR-related `import.meta` expressions with boolean literals.
 *
 * - SSR build: `import.meta.server` → `true`, `import.meta.client` → `false`, `import.meta.env.SSR` → `true`
 * - Client build: `import.meta.server` → `false`, `import.meta.client` → `true`, `import.meta.env.SSR` → `false`
 */
export function replaceImportMetaSsr(code: string, isSSR: boolean, map?: string): SsrRewriteResult {
  // Only process if the code contains import.meta references
  if (!code.includes("import.meta.")) return { code, map };

  const edits: TextEdit[] = [];

  // import.meta.env.SSR is matched first (longer literal, avoids partial
  // replacement) by excluding it from the import.meta.server/client scans
  // below: those two literals are not substrings of it, so scan order does
  // not matter for correctness, only for readability.
  for (const [start, end] of findAllOccurrences(code, "import.meta.env.SSR")) {
    edits.push({ start, end, replacement: isSSR ? "true" : "false" });
  }
  for (const [start, end] of findAllOccurrences(code, "import.meta.server")) {
    edits.push({ start, end, replacement: isSSR ? "true" : "false" });
  }
  for (const [start, end] of findAllOccurrences(code, "import.meta.client")) {
    edits.push({ start, end, replacement: isSSR ? "false" : "true" });
  }

  return applyTextEdits(code, map, edits);
}

/**
 * Strip component tags from compiled output by replacing their render calls
 * with comment placeholders. Works on the compiled JS output (not SFC source).
 *
 * Replaces `_resolveComponent("ComponentName")` calls with a no-op that renders
 * an empty comment node.
 */
export function stripComponents(
  code: string,
  componentNames: string[],
  map?: string,
): SsrRewriteResult {
  if (componentNames.length === 0) return { code, map };

  // Deduplicate requested names before scanning: a name listed twice would
  // otherwise run findAllOccurrences (and thus this whole edit-construction
  // loop) twice over the same pattern. `applyTextEdits`'s cursor guard is
  // what actually keeps duplicate/overlapping edits from ever being spliced
  // in — this Set is a redundant-scan avoidance, not a second correctness
  // gate; removing either one alone still leaves the other holding the line.
  const edits: TextEdit[] = [];
  for (const name of new Set(componentNames)) {
    const pattern = `_resolveComponent("${name}")`;
    const replacement = `(() => ({ __name: "${name}", render: () => null }))()`;
    for (const [start, end] of findAllOccurrences(code, pattern)) {
      edits.push({ start, end, replacement });
    }
  }

  return applyTextEdits(code, map, edits);
}

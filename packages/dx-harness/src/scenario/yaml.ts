/**
 * A small, dependency-free YAML SUBSET parser, scoped to the authored scenario
 * schema ({@link ./model}).
 *
 * The scenario corpus is authored as `scenarios/*.yaml`. Pulling a third-party
 * `yaml` dependency in just to read our own controlled files would add lockfile
 * surface (and risk unrelated drift) for no benefit, so the loader parses a
 * deliberately small subset itself. The parser produces a plain JS value; ALL
 * semantic checking is the scenario validator's job ({@link ./validate}) — this
 * layer only turns well-formed text into the right shape and rejects everything
 * outside the subset with a typed {@link YamlParseError} (never a silent accept).
 *
 * Supported subset:
 *  - UTF-8 text, `\n` / `\r\n` / `\r` line breaks (folded identically).
 *  - `#` comments to end-of-line, full-line or trailing, honoured outside quotes.
 *  - Block mappings `key: value`. A key with no inline value takes its value from
 *    a block on the following lines (a mapping or a sequence) indented EXACTLY one
 *    step ({@link INDENT_STEP}, two spaces) deeper; a child indented by any other
 *    amount is rejected, never silently re-homed. Duplicate keys are rejected.
 *  - Block sequences `- item`; an item is a scalar, a flow value, or a mapping
 *    whose first `key: value` is inline after the dash (the scenario-file shape).
 *    A bare `-` whose value is a nested block requires that block one step deeper.
 *  - Flow sequences `[a, b, c]` and the empty `[]` (scalars only, one line).
 *  - The empty flow mapping `{}` only; a non-empty `{...}` is out of subset.
 *  - Scalars: double-quoted (`\n \t \r \" \\ \/ \uXXXX` escapes), single-quoted
 *    (`''` escapes one quote), and plain. A plain `true`/`false` is boolean,
 *    `null`/`~` is null, an integer/float literal is a number, and everything
 *    else (e.g. `App.vue`, `minimal-member-access`) stays a string.
 *
 * Indentation grammar: indentation MUST use spaces (a tab in the indentation is
 * rejected); each nested block steps in by exactly {@link INDENT_STEP} spaces; all
 * members of one block share its single indentation; and a dedent must return to
 * an open ancestor block's indentation — a line whose indentation matches no open
 * level is rejected as stray content rather than re-homed onto the wrong block.
 */

/** A syntactic fault outside the supported YAML subset; carries the 1-based line. */
export class YamlParseError extends Error {
  /** 1-based source line the fault was detected on (0 when not line-specific). */
  readonly line: number;
  constructor(message: string, line: number) {
    super(line > 0 ? `${message} (line ${line})` : message);
    this.name = "YamlParseError";
    this.line = line;
  }
}

/**
 * The fixed indentation step, in spaces, between a block and its nested child
 * block. Each nesting level indents exactly this much deeper than its parent; a
 * child indented by any other amount is a {@link YamlParseError}, not a silent
 * accept. The committed scenario corpus is authored to this step.
 */
const INDENT_STEP = 2;

/** One significant source line: its indentation depth, comment-stripped content, and 1-based number. */
interface PreLine {
  readonly indent: number;
  readonly text: string;
  readonly lineNo: number;
}

/** Whether `ch` is an ASCII space or tab. */
function isSpace(ch: string): boolean {
  return ch === " " || ch === "\t";
}

/**
 * Remove a trailing `# comment` from one line's content, honouring quotes (a `#`
 * inside `'…'`/`"…"` is literal). A comment `#` must be at the content start or
 * be preceded by whitespace, matching YAML's plain-scalar comment rule. An
 * unbalanced quote is left for {@link parseScalar} to report precisely.
 */
function stripTrailingComment(content: string): string {
  let inSingle = false;
  let inDouble = false;
  for (let i = 0; i < content.length; i++) {
    const ch = content[i];
    if (inSingle) {
      if (ch === "'") {
        if (content[i + 1] === "'")
          i++; // doubled '' is an escaped quote, stay in string
        else inSingle = false;
      }
    } else if (inDouble) {
      if (ch === "\\")
        i++; // skip the escaped char so a `\"` does not close the string
      else if (ch === '"') inDouble = false;
    } else if (ch === "'") {
      inSingle = true;
    } else if (ch === '"') {
      inDouble = true;
    } else if (ch === "#" && (i === 0 || isSpace(content[i - 1]))) {
      return content.slice(0, i).replace(/[ \t]+$/, "");
    }
  }
  return content;
}

/**
 * Split raw `source` into significant {@link PreLine}s: drop blank/comment-only
 * lines, strip trailing comments, and record each line's space-indentation.
 *
 * @throws {YamlParseError} if a line's indentation contains a tab.
 */
function preprocess(source: string): PreLine[] {
  const rawLines = source.split(/\r\n|\r|\n/);
  const out: PreLine[] = [];
  for (let i = 0; i < rawLines.length; i++) {
    const raw = rawLines[i];
    const lineNo = i + 1;
    const leadingMatch = /^[ \t]*/.exec(raw);
    const leading = leadingMatch ? leadingMatch[0] : "";
    if (leading.includes("\t")) {
      throw new YamlParseError("indentation must use spaces, not tabs", lineNo);
    }
    const content = stripTrailingComment(raw.slice(leading.length)).replace(/[ \t]+$/, "");
    if (content === "") continue; // blank or comment-only line
    out.push({ indent: leading.length, text: content, lineNo });
  }
  return out;
}

/** Whether `text` introduces a block-sequence item (`-` alone or `- …`). */
function isDashLine(text: string): boolean {
  return text === "-" || text.startsWith("- ");
}

/**
 * The index of the first key-terminating `:` (followed by space or end-of-text)
 * outside quotes/brackets, or `-1` if the text is not a mapping entry.
 */
function mappingColonIndex(text: string): number {
  let inSingle = false;
  let inDouble = false;
  let depth = 0;
  for (let i = 0; i < text.length; i++) {
    const ch = text[i];
    if (inSingle) {
      if (ch === "'") {
        if (text[i + 1] === "'") i++;
        else inSingle = false;
      }
    } else if (inDouble) {
      if (ch === "\\") i++;
      else if (ch === '"') inDouble = false;
    } else if (ch === "'") {
      inSingle = true;
    } else if (ch === '"') {
      inDouble = true;
    } else if (ch === "[" || ch === "{") {
      depth++;
    } else if (ch === "]" || ch === "}") {
      if (depth > 0) depth--;
    } else if (ch === ":" && depth === 0 && (i + 1 === text.length || text[i + 1] === " ")) {
      return i;
    }
  }
  return -1;
}

/** Parse a double-quoted scalar body (between the quotes), applying escapes. */
function parseDoubleQuoted(text: string, lineNo: number): string {
  // text starts with `"`; find the matching close, processing escapes.
  let out = "";
  let i = 1;
  for (; i < text.length; i++) {
    const ch = text[i];
    if (ch === "\\") {
      const esc = text[i + 1];
      i++;
      switch (esc) {
        case "n":
          out += "\n";
          break;
        case "t":
          out += "\t";
          break;
        case "r":
          out += "\r";
          break;
        case '"':
          out += '"';
          break;
        case "\\":
          out += "\\";
          break;
        case "/":
          out += "/";
          break;
        case "u": {
          const hex = text.slice(i + 1, i + 5);
          if (!/^[0-9a-fA-F]{4}$/.test(hex)) {
            throw new YamlParseError("invalid \\u escape in double-quoted scalar", lineNo);
          }
          out += String.fromCharCode(parseInt(hex, 16));
          i += 4;
          break;
        }
        default:
          throw new YamlParseError(
            `unknown escape "\\${esc ?? ""}" in double-quoted scalar`,
            lineNo,
          );
      }
    } else if (ch === '"') {
      if (i !== text.length - 1) {
        throw new YamlParseError("trailing characters after double-quoted scalar", lineNo);
      }
      return out;
    } else {
      out += ch;
    }
  }
  throw new YamlParseError("unterminated double-quoted scalar", lineNo);
}

/** Parse a single-quoted scalar body, where a doubled `''` is one literal quote. */
function parseSingleQuoted(text: string, lineNo: number): string {
  let out = "";
  let i = 1;
  for (; i < text.length; i++) {
    const ch = text[i];
    if (ch === "'") {
      if (text[i + 1] === "'") {
        out += "'";
        i++;
      } else {
        if (i !== text.length - 1) {
          throw new YamlParseError("trailing characters after single-quoted scalar", lineNo);
        }
        return out;
      }
    } else {
      out += ch;
    }
  }
  throw new YamlParseError("unterminated single-quoted scalar", lineNo);
}

/** A plain (unquoted) scalar: typed `true`/`false`/`null`/`~`/number, else string. */
function parsePlainScalar(text: string): unknown {
  if (text === "true") return true;
  if (text === "false") return false;
  if (text === "null" || text === "~") return null;
  if (/^-?\d+$/.test(text)) return parseInt(text, 10);
  if (/^-?\d+\.\d+$/.test(text) || /^-?\d+(?:\.\d+)?[eE][+-]?\d+$/.test(text)) {
    return Number(text);
  }
  return text;
}

/** Parse one comma-separated flow-sequence item (scalars only — no nested flow). */
function parseScalar(text: string, lineNo: number): unknown {
  if (text.startsWith('"')) return parseDoubleQuoted(text, lineNo);
  if (text.startsWith("'")) return parseSingleQuoted(text, lineNo);
  return parsePlainScalar(text);
}

/** Split a flow `[...]` body into top-level comma items, honouring quotes. */
function splitFlowItems(body: string, lineNo: number): string[] {
  const items: string[] = [];
  let current = "";
  let inSingle = false;
  let inDouble = false;
  for (let i = 0; i < body.length; i++) {
    const ch = body[i];
    if (inSingle) {
      current += ch;
      if (ch === "'") {
        if (body[i + 1] === "'") {
          current += "'";
          i++;
        } else inSingle = false;
      }
    } else if (inDouble) {
      current += ch;
      if (ch === "\\") {
        current += body[i + 1] ?? "";
        i++;
      } else if (ch === '"') inDouble = false;
    } else if (ch === "'") {
      inSingle = true;
      current += ch;
    } else if (ch === '"') {
      inDouble = true;
      current += ch;
    } else if (ch === ",") {
      items.push(current.trim());
      current = "";
    } else {
      current += ch;
    }
  }
  if (inSingle || inDouble) {
    throw new YamlParseError("unterminated quote in flow sequence", lineNo);
  }
  const last = current.trim();
  if (last !== "" || items.length > 0) items.push(last);
  return items;
}

/** Parse a value that is either a flow collection (`[…]` / `{}`) or a scalar. */
function parseFlowOrScalar(text: string, lineNo: number): unknown {
  if (text.startsWith("[")) {
    if (!text.endsWith("]")) {
      throw new YamlParseError("unterminated flow sequence", lineNo);
    }
    const body = text.slice(1, -1).trim();
    if (body === "") return [];
    return splitFlowItems(body, lineNo).map((item) => {
      if (item === "") throw new YamlParseError("empty item in flow sequence", lineNo);
      return parseScalar(item, lineNo);
    });
  }
  if (text.startsWith("{")) {
    if (text.replace(/\s+/g, "") === "{}") return {};
    throw new YamlParseError("non-empty flow mappings are not supported", lineNo);
  }
  return parseScalar(text, lineNo);
}

/** A parse cursor over the preprocessed lines. */
interface Cursor {
  readonly lines: readonly PreLine[];
  index: number;
}

function parseNode(cur: Cursor, indent: number): unknown {
  return isDashLine(cur.lines[cur.index].text)
    ? parseSequence(cur, indent)
    : parseMapping(cur, indent);
}

/** Apply one `key: value` (or `key:` + deeper block) entry into `map`. */
function applyEntry(
  cur: Cursor,
  map: Record<string, unknown>,
  entryText: string,
  indent: number,
): void {
  const line = cur.lines[cur.index];
  const colon = mappingColonIndex(entryText);
  if (colon === -1) {
    throw new YamlParseError(
      `expected a "key: value" mapping entry, got "${entryText}"`,
      line.lineNo,
    );
  }
  const key = entryText.slice(0, colon).trim();
  if (key === "") throw new YamlParseError("empty mapping key", line.lineNo);
  if (Object.prototype.hasOwnProperty.call(map, key)) {
    throw new YamlParseError(`duplicate key "${key}"`, line.lineNo);
  }
  const inlineValue = entryText.slice(colon + 1).trim();
  if (inlineValue !== "") {
    map[key] = parseFlowOrScalar(inlineValue, line.lineNo);
    cur.index++;
    return;
  }
  // No inline value: the value is the block on the following lines, which must
  // indent exactly one step deeper. A deeper-but-misaligned child is a fault,
  // never silently re-homed (so `a:\n    b: 1` is rejected, not parsed).
  cur.index++;
  const next = cur.lines[cur.index];
  if (next === undefined || next.indent <= indent) {
    map[key] = null;
    return;
  }
  if (next.indent !== indent + INDENT_STEP) {
    throw new YamlParseError(
      `child of "${key}" must indent exactly ${INDENT_STEP} spaces deeper ` +
        `(parent column ${indent}, child column ${next.indent})`,
      next.lineNo,
    );
  }
  map[key] = parseNode(cur, next.indent);
}

function parseMapping(cur: Cursor, indent: number): Record<string, unknown> {
  const map: Record<string, unknown> = {};
  while (cur.index < cur.lines.length) {
    const line = cur.lines[cur.index];
    if (line.indent !== indent || isDashLine(line.text)) break;
    applyEntry(cur, map, line.text, indent);
  }
  return map;
}

function parseSequence(cur: Cursor, indent: number): unknown[] {
  const items: unknown[] = [];
  while (cur.index < cur.lines.length) {
    const line = cur.lines[cur.index];
    if (line.indent !== indent || !isDashLine(line.text)) break;
    const rest = line.text.slice(1).trimStart();
    if (rest === "") {
      // Empty dash: the item is the block on the following lines, one step deeper.
      cur.index++;
      const next = cur.lines[cur.index];
      if (next === undefined || next.indent <= indent) {
        throw new YamlParseError("sequence item has no value", line.lineNo);
      }
      if (next.indent !== indent + INDENT_STEP) {
        throw new YamlParseError(
          `nested sequence-item block must indent exactly ${INDENT_STEP} spaces deeper ` +
            `(dash column ${indent}, block column ${next.indent})`,
          next.lineNo,
        );
      }
      items.push(parseNode(cur, next.indent));
    } else if (mappingColonIndex(rest) !== -1) {
      // `- key: value` — a mapping whose first entry is inline after the dash;
      // continuation keys align at the column where `rest` begins.
      const mapIndent = indent + (line.text.length - line.text.slice(1).trimStart().length);
      const map: Record<string, unknown> = {};
      applyEntry(cur, map, rest, mapIndent);
      while (cur.index < cur.lines.length) {
        const k = cur.lines[cur.index];
        if (k.indent !== mapIndent || isDashLine(k.text)) break;
        applyEntry(cur, map, k.text, mapIndent);
      }
      items.push(map);
    } else {
      items.push(parseFlowOrScalar(rest, line.lineNo));
      cur.index++;
    }
  }
  return items;
}

/**
 * Parse a YAML-subset document into a plain JS value. Returns `null` for an empty
 * or comment-only document. The result is handed verbatim to the scenario
 * validator; this function makes NO semantic judgement about scenario shape.
 *
 * @throws {YamlParseError} on any construct outside the supported subset.
 */
export function parseScenarioYaml(source: string): unknown {
  const lines = preprocess(source);
  if (lines.length === 0) return null;
  const cur: Cursor = { lines, index: 0 };
  const baseIndent = lines[0].indent;
  const value = parseNode(cur, baseIndent);
  if (cur.index !== lines.length) {
    const stray = lines[cur.index];
    throw new YamlParseError(
      `unexpected content at indentation ${stray.indent}: "${stray.text}"`,
      stray.lineNo,
    );
  }
  return value;
}

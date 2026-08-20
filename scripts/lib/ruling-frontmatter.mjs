// Minimal, strict parser for the typed YAML frontmatter prepended to every
// docs/arch/refactor/rev11/rulings/*.md document (see rulings/INDEX.md for
// the field list: ruling_id, type, date, date_source, binds, source_file,
// summary, supersedes, superseded_by, contradicts, notes).
//
// This is NOT a general YAML parser. It supports exactly the subset the
// rulings corpus actually uses:
//   - top-level `key: value` lines (no nesting beyond one level)
//   - a double-quoted scalar value, with backslash-escaped characters
//     (the corpus contains literal `\"` inside notes/summary text)
//   - a flow array of double-quoted scalars: `[]` or `["a", "b"]`
//   - a block sequence of small mappings, used only by
//     supersedes/superseded_by/contradicts:
//       key:
//         - ruling: "ID"
//           claim: "text"
//         - document: "text"
//           claim: "text"
// Anything outside this shape fails loudly (FrontmatterError), matching the
// rest of this program's tooling: unrecognised structure is a defect to
// surface, never a silent partial read.

export class FrontmatterError extends Error {}

// Parse one double-quoted scalar starting at s[0] === '"'. Returns
// { value, rest } where `rest` is everything after the closing quote.
function parseQuotedScalar(s, label, lineNo) {
  if (s[0] !== '"') {
    throw new FrontmatterError(`${label}:${lineNo}: expected a double-quoted string, got ${JSON.stringify(s)}`);
  }
  let out = "";
  let i = 1;
  while (i < s.length) {
    const ch = s[i];
    if (ch === "\\") {
      if (i + 1 >= s.length) {
        throw new FrontmatterError(`${label}:${lineNo}: unterminated escape sequence`);
      }
      out += s[i + 1];
      i += 2;
      continue;
    }
    if (ch === '"') {
      return { value: out, rest: s.slice(i + 1) };
    }
    out += ch;
    i++;
  }
  throw new FrontmatterError(`${label}:${lineNo}: unterminated string`);
}

function parseFlowArray(s, label, lineNo) {
  const trimmed = s.trim();
  if (!trimmed.startsWith("[") || !trimmed.endsWith("]")) {
    throw new FrontmatterError(`${label}:${lineNo}: expected a flow array, got ${JSON.stringify(s)}`);
  }
  const inner = trimmed.slice(1, -1).trim();
  if (inner === "") return [];
  const out = [];
  let rest = inner;
  while (rest.trim() !== "") {
    rest = rest.trim();
    const { value, rest: after } = parseQuotedScalar(rest, label, lineNo);
    out.push(value);
    rest = after.trim();
    if (rest.startsWith(",")) {
      rest = rest.slice(1);
    } else if (rest !== "") {
      throw new FrontmatterError(`${label}:${lineNo}: unexpected content in array: ${JSON.stringify(rest)}`);
    }
  }
  return out;
}

function parseScalarValue(raw, label, lineNo) {
  const s = raw.trim();
  if (s.startsWith('"')) return parseQuotedScalar(s, label, lineNo).value;
  return s; // unquoted bare token (not used by the current corpus, tolerated)
}

// Parse the indented lines following `key:` with an empty value into either
// a block sequence (every line is `- field: "..."` or a continuation
// `  field: "..."`) — used by supersedes/superseded_by/contradicts.
function parseBlockSequence(lines, label) {
  const items = [];
  let current = null;
  for (const { text, lineNo } of lines) {
    const dash = /^\s*-\s*([A-Za-z_][A-Za-z0-9_]*)\s*:\s*(.*)$/.exec(text);
    if (dash) {
      current = {};
      items.push(current);
      current[dash[1]] = parseScalarValue(dash[2], label, lineNo);
      continue;
    }
    const cont = /^\s+([A-Za-z_][A-Za-z0-9_]*)\s*:\s*(.*)$/.exec(text);
    if (cont && current) {
      current[cont[1]] = parseScalarValue(cont[2], label, lineNo);
      continue;
    }
    throw new FrontmatterError(
      `${label}:${lineNo}: unrecognised block-sequence line: ${JSON.stringify(text)}`,
    );
  }
  return items;
}

// Extract the frontmatter block (between the first two `---` lines) and
// parse it into a plain object. `text` is the full file content.
export function parseRulingFrontmatter(text, label) {
  const lines = text.split(/\r?\n/);
  if (lines[0]?.trim() !== "---") {
    throw new FrontmatterError(`${label}:1: file does not start with a --- frontmatter fence`);
  }
  let end = -1;
  for (let i = 1; i < lines.length; i++) {
    if (lines[i].trim() === "---") {
      end = i;
      break;
    }
  }
  if (end === -1) {
    throw new FrontmatterError(`${label}: frontmatter fence opened at line 1 but never closed`);
  }

  const body = lines.slice(1, end);
  const result = Object.create(null);
  let i = 0;
  while (i < body.length) {
    const lineNo = i + 2; // +1 for the opening fence, +1 for 1-based numbering
    const raw = body[i];
    if (raw.trim() === "") {
      i++;
      continue;
    }
    const m = /^([A-Za-z_][A-Za-z0-9_]*)\s*:\s*(.*)$/.exec(raw);
    if (!m) {
      throw new FrontmatterError(`${label}:${lineNo}: unrecognised frontmatter line: ${JSON.stringify(raw)}`);
    }
    const key = m[1];
    const rest = m[2];
    if (key in result) {
      throw new FrontmatterError(`${label}:${lineNo}: duplicate frontmatter key ${JSON.stringify(key)}`);
    }
    if (rest === "") {
      // Block sequence: gather following indented, non-empty lines.
      const seq = [];
      i++;
      while (i < body.length && /^\s/.test(body[i]) && body[i].trim() !== "") {
        seq.push({ text: body[i], lineNo: i + 2 });
        i++;
      }
      result[key] = parseBlockSequence(seq, label);
      continue;
    }
    if (rest.startsWith("[")) {
      result[key] = parseFlowArray(rest, label, lineNo);
      i++;
      continue;
    }
    if (rest.startsWith('"')) {
      const { value, rest: trailing } = parseQuotedScalar(rest, label, lineNo);
      if (trailing.trim() !== "") {
        throw new FrontmatterError(`${label}:${lineNo}: trailing content after string: ${JSON.stringify(trailing)}`);
      }
      result[key] = value;
      i++;
      continue;
    }
    // Unquoted bare scalar (not used by the current corpus for any field
    // this parser is asked to read, but tolerated rather than rejected).
    result[key] = rest.trim();
    i++;
  }
  return result;
}

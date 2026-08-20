// Minimal strict TOML reader shared by the rev11 program-state and
// stack-window validators. Extracted from scripts/validate-program-state.mjs
// so both validators read the exact same TOML dialect — a second, divergent
// parser would risk the two validators disagreeing on what a file says, which
// is precisely the failure mode the composite cross-validation exists to
// prevent.
//
// Supported shapes (the full set used by program-dag.toml,
// templates/program-state.template.toml, and
// templates/stack-window.template.toml): full-line comments, `[table]`,
// `[[array-of-tables]]`, and `key = value` where value is a basic
// double-quoted string (no escapes), a single-line array of basic strings,
// an integer, or a boolean. A trailing `# comment` after a value is allowed.
// Everything else fails loudly with the file/line.

export class TomlError extends Error {}

export function parseToml(text, label) {
  const root = Object.create(null);
  let current = root; // table currently receiving keys
  const lines = text.split(/\r?\n/);
  const fail = (lineNo, msg) => {
    throw new TomlError(`${label}:${lineNo}: unparseable TOML — ${msg}`);
  };

  const parseValue = (raw, lineNo) => {
    const s = raw.trim();
    if (s.startsWith('"')) {
      // Basic string, no escape support: fail loudly if a backslash appears
      // before the closing quote rather than mis-reading it.
      const end = s.indexOf('"', 1);
      if (end === -1) fail(lineNo, "unterminated string");
      const body = s.slice(1, end);
      if (body.includes("\\")) fail(lineNo, "escape sequences are not supported");
      const rest = s.slice(end + 1).trim();
      if (rest !== "") {
        if (!rest.startsWith("#")) {
          fail(lineNo, `trailing content after string: ${JSON.stringify(rest)}`);
        }
        // A double-quote inside the trailing "comment" is indistinguishable from
        // an unbalanced/ambiguous string (`"ACT"#IVE"`, `""#REQUIRED_X"`): the
        // reader closed at the FIRST inner quote and would otherwise silently
        // mis-read the value (and bypass the live-mode REQUIRED_ scan). Loud
        // failure, per this file's header promise.
        if (rest.includes('"')) {
          fail(
            lineNo,
            `trailing comment after string contains a double-quote — ambiguous/unbalanced quoting: ${JSON.stringify(rest)}`,
          );
        }
      }
      return body;
    }
    if (s.startsWith("[")) {
      // Single-line array of basic strings (e.g. predecessors = ["A0", "A1"]).
      const end = s.lastIndexOf("]");
      if (end === -1) fail(lineNo, "unterminated array (multi-line arrays unsupported)");
      const rest = s.slice(end + 1).trim();
      if (rest !== "") {
        if (!rest.startsWith("#")) {
          fail(lineNo, `trailing content after array: ${JSON.stringify(rest)}`);
        }
        if (rest.includes('"')) {
          fail(
            lineNo,
            `trailing comment after array contains a double-quote — ambiguous/unbalanced quoting: ${JSON.stringify(rest)}`,
          );
        }
      }
      const inner = s.slice(1, end).trim();
      if (inner === "") return [];
      return inner.split(",").map((piece) => {
        const p = piece.trim();
        if (p === "") fail(lineNo, "empty array element");
        if (!(p.startsWith('"') && p.endsWith('"') && p.length >= 2)) {
          fail(lineNo, `non-string array element: ${JSON.stringify(p)}`);
        }
        const body = p.slice(1, -1);
        if (body.includes('"') || body.includes("\\")) {
          fail(lineNo, `unsupported array element: ${JSON.stringify(p)}`);
        }
        return body;
      });
    }
    // Bare scalar: strip a trailing comment, then integer or boolean only.
    const bare = s.split("#")[0].trim();
    if (bare === "true") return true;
    if (bare === "false") return false;
    if (/^[+-]?\d+$/.test(bare)) {
      // TOML forbids leading zeros on integers (`007` is invalid TOML, not 7).
      // Silently reading it as 7 would contradict this file's loud-failure
      // promise, so reject it here.
      if (/^[+-]?0\d/.test(bare)) {
        fail(lineNo, `integer with leading zero(s) is not valid TOML: ${JSON.stringify(bare)}`);
      }
      return Number.parseInt(bare, 10);
    }
    fail(lineNo, `unsupported value: ${JSON.stringify(s)}`);
  };

  for (let i = 0; i < lines.length; i++) {
    const lineNo = i + 1;
    const line = lines[i].trim();
    if (line === "" || line.startsWith("#")) continue;

    let m;
    if ((m = /^\[\[([A-Za-z0-9_-]+)\]\]$/.exec(line))) {
      const name = m[1];
      if (!Array.isArray(root[name])) {
        if (name in root) fail(lineNo, `[[${name}]] conflicts with existing key`);
        root[name] = [];
      }
      current = Object.create(null);
      root[name].push(current);
      continue;
    }
    if ((m = /^\[([A-Za-z0-9_-]+)\]$/.exec(line))) {
      const name = m[1];
      if (name in root) fail(lineNo, `duplicate table [${name}]`);
      current = Object.create(null);
      root[name] = current;
      continue;
    }
    if ((m = /^([A-Za-z0-9_-]+)\s*=\s*(.+)$/.exec(line))) {
      const key = m[1];
      if (key in current) fail(lineNo, `duplicate key ${JSON.stringify(key)}`);
      current[key] = parseValue(m[2], lineNo);
      continue;
    }
    fail(lineNo, `unrecognized line: ${JSON.stringify(line)}`);
  }
  return root;
}

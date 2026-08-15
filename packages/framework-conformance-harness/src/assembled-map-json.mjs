// A strict RFC 8259 reader for input source maps.
//
// Implements LAYER 1 §4.3 step 1.1 (the three ordered clauses of the
// interoperable JSON domain), step 1.2 (duplicate object members), and §4.5
// (`DECISION` D-7 — binary64 under round-ties-to-even) of
// `spec/assembled-map-composition-layer1.md`.
//
// WHY A HAND-WRITTEN PARSER. Layer 1 imposes three obligations `JSON.parse`
// cannot discharge:
//
//   - §4.4 `U1.8` / `DECISION` D-2: a duplicate object member is REJECTED.
//     `JSON.parse` collapses duplicates silently, so "an implementation that
//     relies on its parser's object model alone does not satisfy `U1.8`".
//   - §4.5 `U1.9`: a number outside the finite binary64 range is REJECTED,
//     "regardless of whether the implementation's native parser would have
//     accepted it". `JSON.parse("1e400")` yields `Infinity` silently.
//   - §4.5 `U1.10`: a string containing an unpaired surrogate after unescaping
//     is REJECTED. `JSON.parse('"\\uD800"')` yields a lone surrogate silently.
//
// Layer 1 also fixes the ORDER of those three: syntax (a), then every number in
// source order (b), then every string in source order (c) — "before any member
// of the document is read". So the reader collects numbers and strings in
// source order during one pass and the caller checks them in that order.

/** Thrown for clause (a) — the bytes are not well-formed JSON (`U1.1`). */
export class JsonSyntaxError extends Error {}

const CHAR_TAB = 0x09;
const CHAR_LF = 0x0a;
const CHAR_CR = 0x0d;
const CHAR_SPACE = 0x20;

function isWhitespace(code) {
  return code === CHAR_SPACE || code === CHAR_TAB || code === CHAR_LF || code === CHAR_CR;
}

function isDigit(code) {
  return code >= 0x30 && code <= 0x39;
}

function isHexDigit(code) {
  return (
    (code >= 0x30 && code <= 0x39) ||
    (code >= 0x41 && code <= 0x46) ||
    (code >= 0x61 && code <= 0x66)
  );
}

/**
 * Reads one JSON document.
 *
 * The value model distinguishes every JSON type without collapsing any of
 * them: objects are `Map`s (so a member present with value `null` is
 * distinguishable from an absent member, and insertion order is retained),
 * arrays are arrays, numbers are the CONVERTED binary64 values (§4.5), strings
 * are JavaScript strings, `true`/`false` are booleans, and `null` is `null`.
 *
 * @param {string} text
 * @returns {{
 *   value: unknown,
 *   numbers: Array<{ lexeme: string, value: number }>,
 *   strings: string[],
 *   hasDuplicateMember: boolean,
 * }}
 * @throws {JsonSyntaxError} clause (a)
 */
export function readJsonDocument(text) {
  let at = 0;
  /** Every JSON number in the document, in SOURCE order (§4.3 step 1.1(b)). */
  const numbers = [];
  /** Every JSON string in the document, unescaped, in SOURCE order (clause (c)). */
  const strings = [];
  let hasDuplicateMember = false;

  function fail(message) {
    throw new JsonSyntaxError(`${message} at offset ${at}`);
  }

  function skipWhitespace() {
    while (at < text.length && isWhitespace(text.charCodeAt(at))) at += 1;
  }

  function expect(character) {
    if (at >= text.length || text[at] !== character) fail(`expected ${JSON.stringify(character)}`);
    at += 1;
  }

  function readLiteral(word, value) {
    if (text.slice(at, at + word.length) !== word) fail(`expected ${word}`);
    at += word.length;
    return value;
  }

  function readString() {
    expect('"');
    let out = "";
    for (;;) {
      if (at >= text.length) fail("unterminated string");
      const code = text.charCodeAt(at);
      if (code === 0x22) {
        at += 1;
        break;
      }
      if (code === 0x5c) {
        at += 1;
        if (at >= text.length) fail("unterminated escape");
        const escape = text[at];
        at += 1;
        switch (escape) {
          case '"':
            out += '"';
            break;
          case "\\":
            out += "\\";
            break;
          case "/":
            out += "/";
            break;
          case "b":
            out += "\b";
            break;
          case "f":
            out += "\f";
            break;
          case "n":
            out += "\n";
            break;
          case "r":
            out += "\r";
            break;
          case "t":
            out += "\t";
            break;
          case "u": {
            if (at + 4 > text.length) fail("truncated \\u escape");
            for (let k = 0; k < 4; k += 1) {
              if (!isHexDigit(text.charCodeAt(at + k))) fail("malformed \\u escape");
            }
            // The escape produces a UTF-16 code unit, which may be a lone
            // surrogate. That is NOT a syntax error: clause (c) owns it, and
            // clause (a) must pass first so the reported outcome is `U1.10`
            // rather than `U1.1` (§4.5: "an implementation whose parser is
            // stricter must report `U1.9` / `U1.10` rather than let a
            // parse-time rejection surface as `U1.1`").
            out += String.fromCharCode(Number.parseInt(text.slice(at, at + 4), 16));
            at += 4;
            break;
          }
          default:
            fail(`invalid escape \\${escape}`);
        }
        continue;
      }
      if (code <= 0x1f) fail("unescaped control character in string");
      out += text[at];
      at += 1;
    }
    strings.push(out);
    return out;
  }

  function readNumber() {
    const start = at;
    if (text[at] === "-") at += 1;
    if (at >= text.length) fail("truncated number");
    if (text[at] === "0") {
      at += 1;
    } else if (isDigit(text.charCodeAt(at))) {
      while (at < text.length && isDigit(text.charCodeAt(at))) at += 1;
    } else {
      fail("expected a digit");
    }
    if (text[at] === ".") {
      at += 1;
      if (at >= text.length || !isDigit(text.charCodeAt(at))) fail("expected a fraction digit");
      while (at < text.length && isDigit(text.charCodeAt(at))) at += 1;
    }
    if (text[at] === "e" || text[at] === "E") {
      at += 1;
      if (text[at] === "+" || text[at] === "-") at += 1;
      if (at >= text.length || !isDigit(text.charCodeAt(at))) fail("expected an exponent digit");
      while (at < text.length && isDigit(text.charCodeAt(at))) at += 1;
    }
    const lexeme = text.slice(start, at);
    // §4.5 / `DECISION` D-7: "The conversion is IEEE-754 binary64 using
    // round-ties-to-even, applied to the number's exact decimal lexeme".
    // `Number(lexeme)` is exactly that conversion; `1e400` yields `Infinity`,
    // which clause (b) then rejects as `U1.9`.
    const value = Number(lexeme);
    numbers.push({ lexeme, value });
    return value;
  }

  function readArray() {
    expect("[");
    const out = [];
    skipWhitespace();
    if (text[at] === "]") {
      at += 1;
      return out;
    }
    for (;;) {
      skipWhitespace();
      out.push(readValue());
      skipWhitespace();
      if (text[at] === ",") {
        at += 1;
        continue;
      }
      if (text[at] === "]") {
        at += 1;
        return out;
      }
      fail("expected ',' or ']'");
    }
  }

  function readObject() {
    expect("{");
    const out = new Map();
    skipWhitespace();
    if (text[at] === "}") {
      at += 1;
      return out;
    }
    for (;;) {
      skipWhitespace();
      if (text[at] !== '"') fail("expected a member name");
      const name = readString();
      // §4.3 step 1.2 / `U1.8`: detected DURING parsing, before any member is
      // read, rather than inherited from a last-wins object model.
      if (out.has(name)) hasDuplicateMember = true;
      skipWhitespace();
      expect(":");
      skipWhitespace();
      out.set(name, readValue());
      skipWhitespace();
      if (text[at] === ",") {
        at += 1;
        continue;
      }
      if (text[at] === "}") {
        at += 1;
        return out;
      }
      fail("expected ',' or '}'");
    }
  }

  function readValue() {
    if (at >= text.length) fail("unexpected end of document");
    const character = text[at];
    if (character === "{") return readObject();
    if (character === "[") return readArray();
    if (character === '"') return readString();
    if (character === "t") return readLiteral("true", true);
    if (character === "f") return readLiteral("false", false);
    if (character === "n") return readLiteral("null", null);
    if (character === "-" || isDigit(text.charCodeAt(at))) return readNumber();
    return fail("unexpected character");
  }

  skipWhitespace();
  const value = readValue();
  skipWhitespace();
  if (at !== text.length) fail("trailing content after the document");

  return { value, numbers, strings, hasDuplicateMember };
}

/**
 * §4.3 step 1.1(c) / `U1.10` — is the unescaped string a sequence of
 * well-formed Unicode scalar values? An unpaired surrogate is not, "whether
 * written literally or as a `\uD800`-style escape".
 */
export function hasUnpairedSurrogate(value) {
  for (let i = 0; i < value.length; i += 1) {
    const code = value.charCodeAt(i);
    if (code >= 0xd800 && code <= 0xdbff) {
      const next = i + 1 < value.length ? value.charCodeAt(i + 1) : -1;
      if (next >= 0xdc00 && next <= 0xdfff) {
        i += 1;
        continue;
      }
      return true;
    }
    if (code >= 0xdc00 && code <= 0xdfff) return true;
  }
  return false;
}

export function isJsonObject(value) {
  return value instanceof Map;
}

export function isJsonArray(value) {
  return Array.isArray(value);
}

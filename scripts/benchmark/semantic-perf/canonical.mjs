// Canonical form of a printed TypeScript type, so two tools' prints of the
// same type compare equal however each spells it.
//
// What is normalised (and nothing else):
//   - whitespace;
//   - string-literal quoting ('a' and "a" are the same literal);
//   - union member order (a union is a set);
//   - object-type member order and separators ({ a: 1, b: 2 } and
//     { b: 2; a: 1; } are the same type);
//   - redundant parentheses around a whole union member or tuple element.
// Tuple element order, generic argument order, function parameter order and
// everything else are significant and kept.
//
// The canonical form is a string; `canonicalDigest` hashes it so that very
// large answers (a 100,000-member union) are compared and stored compactly.

import { createHash } from "node:crypto";

const PUNCT = ["=>", "...", "?:", "-?", "+?", "(", ")", "[", "]", "{", "}", "<", ">", "|", "&", ",", ";", ":", "?", "=", "."];

function tokenize(text) {
  const tokens = [];
  let i = 0;
  while (i < text.length) {
    const c = text[i];
    if (/\s/.test(c)) {
      i++;
      continue;
    }
    if (c === '"' || c === "'") {
      let j = i + 1;
      let value = "";
      while (j < text.length && text[j] !== c) {
        if (text[j] === "\\") {
          value += text[j] + text[j + 1];
          j += 2;
        } else {
          value += text[j++];
        }
      }
      if (j >= text.length) throw new Error(`unterminated string literal in ${JSON.stringify(text.slice(0, 80))}`);
      // Re-quote with double quotes: an escaped single quote needs no escape.
      const unescapedSingle = value.replace(/\\'/g, "'");
      const requoted = unescapedSingle.replace(/(^|[^\\])"/g, '$1\\"');
      tokens.push({ kind: "str", text: `"${requoted}"` });
      i = j + 1;
      continue;
    }
    if (c === "`") {
      // A template literal type: keep it as one token, normalising only the
      // whitespace inside its `${…}` holes.
      let j = i + 1;
      let out = "`";
      while (j < text.length && text[j] !== "`") {
        if (text[j] === "\\") {
          out += text[j] + text[j + 1];
          j += 2;
        } else if (text[j] === "$" && text[j + 1] === "{") {
          let depth = 1;
          let k = j + 2;
          while (k < text.length && depth) {
            if (text[k] === "{") depth++;
            else if (text[k] === "}") depth--;
            k++;
          }
          out += "${" + canonicalType(text.slice(j + 2, k - 1)) + "}";
          j = k;
        } else {
          out += text[j++];
        }
      }
      tokens.push({ kind: "tpl", text: out + "`" });
      i = j + 1;
      continue;
    }
    const punct = PUNCT.find((p) => text.startsWith(p, i));
    if (punct) {
      tokens.push({ kind: "p", text: punct });
      i += punct.length;
      continue;
    }
    let j = i;
    while (j < text.length && !/\s/.test(text[j]) && !PUNCT.some((p) => text.startsWith(p, j)) && !"\"'`".includes(text[j])) j++;
    if (j === i) throw new Error(`cannot tokenize at ${JSON.stringify(text.slice(i, i + 20))}`);
    tokens.push({ kind: "w", text: text.slice(i, j) });
    i = j;
  }
  return tokens;
}

const OPEN = { "(": ")", "[": "]", "{": "}", "<": ">" };

// A node is { kind: "seq", items: (token | group)[] }; a group is
// { kind: "group", open, items: node[] separated by commas/semicolons }.
function parseSeq(tokens, pos, close) {
  const items = [];
  while (pos < tokens.length) {
    const t = tokens[pos];
    if (t.kind === "p" && t.text === close) return { items, pos };
    if (t.kind === "p" && OPEN[t.text]) {
      const inner = parseSeq(tokens, pos + 1, OPEN[t.text]);
      if (inner.pos >= tokens.length) throw new Error(`unbalanced ${t.text}`);
      items.push({ kind: "group", open: t.text, items: inner.items });
      pos = inner.pos + 1;
      continue;
    }
    if (t.kind === "p" && [")", "]", "}", ">"].includes(t.text)) throw new Error(`unexpected ${t.text}`);
    items.push(t);
    pos++;
  }
  if (close) return { items, pos };
  return { items, pos };
}

function splitTop(items, separators) {
  const parts = [[]];
  for (const item of items) {
    if (item.kind === "p" && separators.includes(item.text)) parts.push([]);
    else parts.at(-1).push(item);
  }
  return parts.filter((part, index) => part.length || index < parts.length - 1).filter((part) => part.length);
}

// `T[]` and `Array<T>` (`readonly T[]` and `ReadonlyArray<T>`) are one type:
// rewrite the suffix form into the generic form.
function normalizeArrays(items) {
  const out = [];
  for (const item of items) {
    if (item.kind === "group" && item.open === "[" && item.items.length === 0 && out.length) {
      const element = out.pop();
      const elementText = stripParens(element.kind === "group" ? renderGroup(element) : element.text);
      const readonly = out.length && out.at(-1).kind === "w" && out.at(-1).text === "readonly";
      if (readonly) out.pop();
      out.push({ kind: "w", text: `${readonly ? "ReadonlyArray" : "Array"}<${elementText}>` });
      continue;
    }
    out.push(item);
  }
  return out;
}

function render(items) {
  items = normalizeArrays(items);
  let out = "";
  for (const item of items) {
    const piece = item.kind === "group" ? renderGroup(item) : item.text;
    if (!out) out = piece;
    else if (item.kind === "p" && [",", ";", ":", "?:", "?", ".", ">"].includes(item.text)) out += piece;
    else if (out.endsWith(".") || out.endsWith("...")) out += piece;
    else if (item.kind === "group" && item.open === "<") out += piece;
    else out += " " + piece;
  }
  return out;
}

function unionCanonical(items) {
  // A leading `|` (multi-line union prints) carries no meaning.
  while (items.length && items[0].kind === "p" && items[0].text === "|") items = items.slice(1);
  const members = splitTop(items, ["|"]);
  if (members.length <= 1) return intersectionCanonical(items);
  let texts = [...new Set(members.map((m) => stripParens(intersectionCanonical(m))))];
  // `boolean` is `false | true`.
  if (texts.includes("false") && texts.includes("true")) texts = [...texts.filter((t) => t !== "false" && t !== "true"), "boolean"];
  texts.sort();
  return texts.join(" | ");
}

function intersectionCanonical(items) {
  const members = splitTop(items, ["&"]);
  if (members.length <= 1) return render(items);
  return members.map((m) => render(m)).join(" & ");
}

function stripParens(text) {
  // `(A)` as a whole member is `A` when the inside is a single balanced group.
  while (text.startsWith("(") && text.endsWith(")")) {
    let depth = 0;
    let wraps = true;
    for (let i = 0; i < text.length; i++) {
      if (text[i] === "(") depth++;
      else if (text[i] === ")") depth--;
      if (depth === 0 && i < text.length - 1) {
        wraps = false;
        break;
      }
    }
    if (!wraps) break;
    text = text.slice(1, -1).trim();
  }
  return text;
}

function renderGroup(group) {
  if (group.open === "{") {
    const members = splitTop(group.items, [";", ","]).map((m) => unionCanonical(m));
    members.sort();
    return members.length ? `{ ${members.join("; ")}; }` : "{}";
  }
  if (group.open === "[") {
    const elements = splitTop(group.items, [","]).map((m) => stripParens(unionCanonical(m)));
    return `[${elements.join(", ")}]`;
  }
  if (group.open === "<") {
    return `<${splitTop(group.items, [","]).map((m) => unionCanonical(m)).join(", ")}>`;
  }
  // Parenthesised: a parameter list or a grouping.
  const parts = splitTop(group.items, [","]).map((m) => unionCanonical(m));
  return `(${parts.join(", ")})`;
}

/** The canonical text of a printed type. */
export function canonicalType(text) {
  const tokens = tokenize(String(text).trim());
  const { items } = parseSeq(tokens, 0, null);
  return stripParens(unionCanonical(items));
}

/** A compact identity of a printed type: canonical length, digest, members. */
export function canonicalDigest(text) {
  const canonical = canonicalType(text);
  return {
    sha256: createHash("sha256").update(canonical).digest("hex"),
    length: canonical.length,
    preview: canonical.length > 160 ? canonical.slice(0, 160) + "…" : canonical,
  };
}

/** The canonical members of a printed type's top-level union (one member when it is not a union). */
export function canonicalUnionMembers(text) {
  let { items } = parseSeq(tokenize(String(text).trim()), 0, null);
  while (items.length && items[0].kind === "p" && items[0].text === "|") items = items.slice(1);
  return [...new Set(splitTop(items, ["|"]).map((m) => stripParens(intersectionCanonical(m))))].sort();
}

/** The element of a one-element tuple type `[X]`, canonical; throws when the text is not one. */
export function singleTupleElement(text) {
  const { items } = parseSeq(tokenize(String(text).trim()), 0, null);
  if (items.length !== 1 || items[0].kind !== "group" || items[0].open !== "[") {
    throw new Error(`not a one-element tuple: ${String(text).slice(0, 120)}`);
  }
  const elements = splitTop(items[0].items, [","]);
  if (elements.length !== 1) throw new Error(`not a one-element tuple: ${String(text).slice(0, 120)}`);
  return stripParens(unionCanonical(elements[0]));
}

// Canonical form of a printed TypeScript type, so two tools' prints of the
// same type compare equal however each spells it — and prints of different
// types never do.
//
// The text is parsed with a recursive-descent parser for TypeScript's type
// syntax (the part either tool prints), then rendered from the tree. Syntax
// the parser does not know throws: the comparison fails closed, it never
// guesses.
//
// Normalisation rules, each an identity of the type system (and nothing
// else is normalised):
//   - whitespace, separators (`,` / `;` in object types), redundant
//     parentheses and a leading `|` / `&`;
//   - string literals by value ('a' and "a" are one literal); numeric
//     literals by value (1.0 and 1);
//   - union members are a set: nested unions flatten, duplicates go, order
//     does not matter, and `true | false` is `boolean`;
//   - `T[]` is `Array<T>`, `readonly T[]` is `ReadonlyArray<T>`;
//   - object-type properties and methods are keyed by name, so their order
//     does not matter — but overloads (same-named methods, call signatures,
//     construct signatures) keep their order, which overload resolution
//     observes;
//   - parameter names and tuple labels are dropped: they are names, not part
//     of the type's identity (optional and rest markers are kept).
// Intersection order, tuple element order, argument order, modifiers and
// everything else are significant and kept.

import { createHash } from "node:crypto";

// ---------------------------------------------------------------- tokens

const PUNCT = ["=>", "...", "(", ")", "[", "]", "{", "}", "<", ">", "|", "&", ",", ";", ":", "?", "=", ".", "+", "-"];

function decodeString(raw, quote) {
  const simple = { n: "\n", r: "\r", t: "\t", b: "\b", f: "\f", v: "\v", 0: "\0", "\\": "\\", "'": "'", '"': '"', "`": "`", $: "$" };
  let out = "";
  for (let i = 0; i < raw.length; i++) {
    const c = raw[i];
    if (c !== "\\") {
      out += c;
      continue;
    }
    const n = raw[++i];
    if (n in simple) out += simple[n];
    else if (n === "u" && raw[i + 1] === "{") {
      const end = raw.indexOf("}", i);
      out += String.fromCodePoint(parseInt(raw.slice(i + 2, end), 16));
      i = end;
    } else if (n === "u") {
      out += String.fromCharCode(parseInt(raw.slice(i + 1, i + 5), 16));
      i += 4;
    } else if (n === "x") {
      out += String.fromCharCode(parseInt(raw.slice(i + 1, i + 3), 16));
      i += 2;
    } else throw new Error(`unsupported escape \\${n} in a ${quote}-quoted literal`);
  }
  return out;
}

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
      while (j < text.length && text[j] !== c) j += text[j] === "\\" ? 2 : 1;
      if (j >= text.length) throw new Error(`unterminated string literal in ${JSON.stringify(text.slice(i, i + 60))}`);
      tokens.push({ kind: "str", value: decodeString(text.slice(i + 1, j), c) });
      i = j + 1;
      continue;
    }
    if (c === "`") {
      // A template literal type: literal chunks and `${type}` holes.
      const chunks = [];
      const holes = [];
      let chunk = "";
      let j = i + 1;
      while (j < text.length && text[j] !== "`") {
        if (text[j] === "\\") {
          chunk += text.slice(j, j + 2);
          j += 2;
        } else if (text[j] === "$" && text[j + 1] === "{") {
          let depth = 1;
          let k = j + 2;
          while (k < text.length && depth) {
            if (text[k] === "{") depth++;
            else if (text[k] === "}") depth--;
            k++;
          }
          if (depth) throw new Error("unterminated template hole");
          chunks.push(decodeString(chunk, "`"));
          chunk = "";
          holes.push(parseType(text.slice(j + 2, k - 1)));
          j = k;
        } else chunk += text[j++];
      }
      if (j >= text.length) throw new Error("unterminated template literal type");
      chunks.push(decodeString(chunk, "`"));
      tokens.push({ kind: "tpl", chunks, holes });
      i = j + 1;
      continue;
    }
    if (/[0-9]/.test(c) || (c === "." && /[0-9]/.test(text[i + 1] ?? ""))) {
      const m = /^(0[xX][0-9a-fA-F_]+|0[bB][01_]+|0[oO][0-7_]+|(?:\d[\d_]*)?\.?\d[\d_]*(?:[eE][+-]?\d+)?)(n?)/.exec(text.slice(i));
      tokens.push({ kind: m[2] ? "bigint" : "num", text: m[1].replace(/_/g, "") });
      i += m[0].length;
      continue;
    }
    if (/[A-Za-z_$#\u0080-￿]/.test(c)) {
      let j = i + 1;
      while (j < text.length && /[A-Za-z0-9_$\u0080-￿]/.test(text[j])) j++;
      tokens.push({ kind: "id", text: text.slice(i, j) });
      i = j;
      continue;
    }
    const punct = PUNCT.find((p) => text.startsWith(p, i));
    if (!punct) throw new Error(`unsupported character ${JSON.stringify(c)} in a printed type`);
    tokens.push({ kind: "p", text: punct });
    i += punct.length;
  }
  return tokens;
}

// ---------------------------------------------------------------- parser

class Parser {
  constructor(tokens) {
    this.t = tokens;
    this.i = 0;
    // Inside a conditional's `extends` clause a nested `extends` belongs to
    // an `infer … extends` constraint, not a new conditional.
    this.noConditional = false;
  }
  peek(o = 0) {
    return this.t[this.i + o];
  }
  isP(text, o = 0) {
    const t = this.peek(o);
    return t?.kind === "p" && t.text === text;
  }
  isId(text, o = 0) {
    const t = this.peek(o);
    return t?.kind === "id" && (text === undefined || t.text === text);
  }
  eatP(text) {
    if (this.isP(text)) {
      this.i++;
      return true;
    }
    return false;
  }
  eatId(text) {
    if (this.isId(text)) {
      this.i++;
      return true;
    }
    return false;
  }
  expectP(text) {
    if (!this.eatP(text)) throw new Error(`expected ${text}, found ${this.describe()}`);
  }
  ident() {
    const t = this.peek();
    if (t?.kind !== "id") throw new Error(`expected a name, found ${this.describe()}`);
    this.i++;
    return t.text;
  }
  describe() {
    const t = this.peek();
    return t ? JSON.stringify(t.text ?? t.value ?? t.kind) : "the end";
  }
  /** Run `fn` with conditional types allowed (inside brackets). */
  nested(fn) {
    const saved = this.noConditional;
    this.noConditional = false;
    try {
      return fn();
    } finally {
      this.noConditional = saved;
    }
  }

  type() {
    const check = this.union();
    // A conditional type: `C extends E ? T : F`.
    if (this.isId("extends") && !this.noConditional) {
      this.i++;
      const saved = this.noConditional;
      this.noConditional = true;
      const ext = this.union();
      this.noConditional = saved;
      this.expectP("?");
      const whenTrue = this.type();
      this.expectP(":");
      const whenFalse = this.type();
      return { k: "conditional", check, ext, whenTrue, whenFalse };
    }
    return check;
  }

  union() {
    this.eatP("|");
    const members = [this.intersection()];
    while (this.eatP("|")) members.push(this.intersection());
    return members.length === 1 ? members[0] : { k: "union", members };
  }

  intersection() {
    this.eatP("&");
    const members = [this.operator()];
    while (this.eatP("&")) members.push(this.operator());
    return members.length === 1 ? members[0] : { k: "intersection", members };
  }

  operator() {
    if (this.isId("keyof") && !this.isP("[", 1) && !this.isP(".", 1) && !this.isP("<", 1)) {
      this.i++;
      return { k: "keyof", type: this.operator() };
    }
    if (this.isId("unique") && this.isId("symbol", 1)) {
      this.i += 2;
      return { k: "kw", name: "unique symbol" };
    }
    if (this.isId("readonly") && (this.isP("[", 1) || this.isId(undefined, 1) || this.isP("(", 1) || this.isP("{", 1))) {
      this.i++;
      const inner = this.operator();
      if (inner.k === "array" || inner.k === "tuple") return { ...inner, readonly: true };
      throw new Error("`readonly` applies only to array and tuple types");
    }
    if (this.isId("infer") && this.isId(undefined, 1)) {
      this.i++;
      const name = this.ident();
      let constraint = null;
      if (this.isId("extends")) {
        // `infer X extends C` is a constraint unless a `?` follows it, in
        // which case the `extends` opens a conditional (TypeScript's rule).
        const start = this.i;
        const saved = this.noConditional;
        this.i++;
        this.noConditional = true;
        try {
          constraint = this.union();
        } catch {
          constraint = null;
        }
        this.noConditional = saved;
        if (constraint === null || this.isP("?")) {
          this.i = start;
          constraint = null;
        }
      }
      return { k: "infer", name, constraint };
    }
    return this.postfix();
  }

  postfix() {
    let type = this.primary();
    for (;;) {
      if (this.isP("[") && this.isP("]", 1)) {
        this.i += 2;
        type = { k: "array", element: type, readonly: false };
      } else if (this.isP("[")) {
        this.i++;
        const index = this.nested(() => this.type());
        this.expectP("]");
        type = { k: "indexed", object: type, index };
      } else return type;
    }
  }

  typeArgs() {
    this.expectP("<");
    const args = this.nested(() => {
      const list = [this.type()];
      while (this.eatP(",")) list.push(this.type());
      return list;
    });
    this.expectP(">");
    return args;
  }

  typeParams() {
    this.expectP("<");
    const params = this.nested(() => {
      const list = [];
      do {
        const modifiers = [];
        while ((this.isId("in") || this.isId("out") || this.isId("const")) && this.isId(undefined, 1)) modifiers.push(this.ident());
        const name = this.ident();
        const constraint = this.eatId("extends") ? this.type() : null;
        const dflt = this.eatP("=") ? this.type() : null;
        list.push({ name, modifiers, constraint, dflt });
      } while (this.eatP(","));
      return list;
    });
    this.expectP(">");
    return params;
  }

  /** `(a: T, b?: U, ...c: V)` — names dropped, markers kept. */
  params() {
    this.expectP("(");
    const params = this.nested(() => {
      const list = [];
      while (!this.isP(")")) {
        const rest = this.eatP("...");
        let name;
        if (this.isId() && (this.isP(":", 1) || this.isP("?", 1) || this.isP(",", 1) || this.isP(")", 1))) name = this.ident();
        else throw new Error(`expected a parameter, found ${this.describe()}`);
        const optional = this.eatP("?");
        const type = this.eatP(":") ? this.returnType() : { k: "kw", name: "any" };
        list.push({ isThis: name === "this", rest, optional, type });
        if (!this.eatP(",")) break;
      }
      return list;
    });
    this.expectP(")");
    return params;
  }

  /** At `(`: a function type `(…) => R` or a parenthesised type. */
  parenOrFunction() {
    const start = this.i;
    try {
      const params = this.params();
      if (this.eatP("=>")) return { k: "function", typeParams: null, params, ret: this.returnType() };
    } catch {
      // not a parameter list
    }
    this.i = start;
    this.expectP("(");
    const inner = this.nested(() => this.type());
    this.expectP(")");
    return inner;
  }

  returnType() {
    // `x is T` / `asserts x is T` / `asserts x` type predicates.
    if (this.isId("asserts") && this.isId(undefined, 1) && !this.isP(".", 1)) {
      this.i++;
      const name = this.ident();
      const type = this.eatId("is") ? this.type() : null;
      return { k: "predicate", asserts: true, name, type };
    }
    if (this.isId() && this.isId("is", 1)) {
      const name = this.ident();
      this.i++;
      return { k: "predicate", asserts: false, name, type: this.type() };
    }
    return this.type();
  }

  signatureTail() {
    const typeParams = this.isP("<") ? this.typeParams() : null;
    const params = this.params();
    return { typeParams, params };
  }

  primary() {
    const t = this.peek();
    if (!t) throw new Error("unexpected end of a printed type");
    if (t.kind === "str") {
      this.i++;
      return { k: "lit", repr: JSON.stringify(t.value) };
    }
    const negative = t.kind === "p" && t.text === "-" && (this.peek(1)?.kind === "num" || this.peek(1)?.kind === "bigint");
    if (t.kind === "num" || (negative && this.peek(1).kind === "num")) {
      if (negative) this.i++;
      const value = Number(this.peek().text);
      this.i++;
      if (!Number.isFinite(value)) throw new Error("unsupported numeric literal");
      return { k: "lit", repr: String(negative ? -value : value) };
    }
    if (t.kind === "bigint" || negative) {
      if (negative) this.i++;
      const value = BigInt(this.peek().text);
      this.i++;
      return { k: "lit", repr: `${negative ? -value : value}n` };
    }
    if (t.kind === "tpl") {
      this.i++;
      if (!t.holes.length) return { k: "lit", repr: JSON.stringify(t.chunks[0]) };
      return { k: "template", chunks: t.chunks, holes: t.holes };
    }
    if (t.kind === "p") {
      if (t.text === "(") return this.parenOrFunction();
      if (t.text === "<") {
        const { typeParams, params } = this.signatureTail();
        this.expectP("=>");
        return { k: "function", typeParams, params, ret: this.returnType() };
      }
      if (t.text === "[") return this.tuple();
      if (t.text === "{") return this.objectOrMapped();
      throw new Error(`unexpected ${t.text} in a printed type`);
    }
    if (t.text === "new" && (this.isP("(", 1) || this.isP("<", 1))) {
      this.i++;
      const { typeParams, params } = this.signatureTail();
      this.expectP("=>");
      return { k: "constructor", abstract: false, typeParams, params, ret: this.type() };
    }
    if (t.text === "abstract" && this.isId("new", 1)) {
      this.i += 2;
      const { typeParams, params } = this.signatureTail();
      this.expectP("=>");
      return { k: "constructor", abstract: true, typeParams, params, ret: this.type() };
    }
    if (t.text === "typeof" && this.isId(undefined, 1)) {
      this.i++;
      const name = this.entityName();
      const args = this.isP("<") ? this.typeArgs() : [];
      return { k: "typeof", name, args };
    }
    if (t.text === "import" && this.isP("(", 1)) {
      this.i += 2;
      const spec = this.peek();
      if (spec?.kind !== "str") throw new Error("expected a module specifier");
      this.i++;
      this.expectP(")");
      let name = "";
      while (this.eatP(".")) name += (name ? "." : "") + this.ident();
      const args = this.isP("<") ? this.typeArgs() : [];
      return { k: "import", spec: spec.value, name, args };
    }
    const keywords = ["any", "unknown", "never", "string", "number", "boolean", "symbol", "bigint", "object", "void", "undefined", "null", "this"];
    if ((keywords.includes(t.text) || t.text === "true" || t.text === "false") && !this.isP(".", 1) && !this.isP("<", 1)) {
      this.i++;
      if (t.text === "true" || t.text === "false") return { k: "lit", repr: t.text };
      return { k: "kw", name: t.text };
    }
    const name = this.entityName();
    const args = this.isP("<") ? this.typeArgs() : [];
    if (name === "Array" && args.length === 1) return { k: "array", element: args[0], readonly: false };
    if (name === "ReadonlyArray" && args.length === 1) return { k: "array", element: args[0], readonly: true };
    return { k: "ref", name, args };
  }

  entityName() {
    let name = this.ident();
    while (this.isP(".") && this.isId(undefined, 1)) {
      this.i++;
      name += "." + this.ident();
    }
    return name;
  }

  tuple() {
    this.expectP("[");
    const elements = this.nested(() => {
      const list = [];
      while (!this.isP("]")) {
        const rest = this.eatP("...");
        // A labelled element `name: T` / `name?: T`: the label is dropped.
        let optional = false;
        if (this.isId() && (this.isP(":", 1) || (this.isP("?", 1) && this.isP(":", 2)))) {
          this.i++;
          optional = this.eatP("?");
          this.expectP(":");
        }
        const type = this.type();
        if (this.eatP("?")) optional = true;
        list.push({ rest, optional, type });
        if (!this.eatP(",")) break;
      }
      return list;
    });
    this.expectP("]");
    return { k: "tuple", elements, readonly: false };
  }

  memberName() {
    const t = this.peek();
    if (t?.kind === "id") {
      this.i++;
      return t.text;
    }
    if (t?.kind === "str") {
      this.i++;
      return t.value;
    }
    if (t?.kind === "num") {
      this.i++;
      return String(Number(t.text));
    }
    if (this.isP("[")) {
      // A computed name such as `[Symbol.iterator]`.
      this.i++;
      const inner = this.entityName();
      this.expectP("]");
      return `[${inner}]`;
    }
    throw new Error(`expected a member name, found ${this.describe()}`);
  }

  objectOrMapped() {
    this.expectP("{");
    const start = this.i;
    // A mapped type: `{ readonly [K in T as N]?: X }`.
    let readonly = null;
    if ((this.isP("+") || this.isP("-")) && this.isId("readonly", 1)) {
      readonly = this.peek().text;
      this.i += 2;
    } else if (this.isId("readonly") && this.isP("[", 1)) {
      this.i++;
      readonly = "+";
    }
    if (this.isP("[") && this.isId(undefined, 1) && this.isId("in", 2)) {
      this.i++;
      const param = this.ident();
      this.i++;
      const { constraint, as } = this.nested(() => ({ constraint: this.type(), as: this.eatId("as") ? this.type() : null }));
      this.expectP("]");
      let optional = null;
      if ((this.isP("+") || this.isP("-")) && this.isP("?", 1)) {
        optional = this.peek().text;
        this.i += 2;
      } else if (this.eatP("?")) optional = "+";
      this.expectP(":");
      const template = this.nested(() => this.type());
      this.eatP(";");
      this.expectP("}");
      return { k: "mapped", readonly, param, constraint, as, optional, template };
    }
    this.i = start;
    const members = this.nested(() => {
      const list = [];
      while (!this.isP("}")) {
        list.push(this.member());
        if (!this.eatP(";") && !this.eatP(",") && !this.isP("}")) throw new Error(`expected ; or } in an object type, found ${this.describe()}`);
      }
      return list;
    });
    this.expectP("}");
    return { k: "object", members };
  }

  member() {
    // Call signature `(…): R` / `<T>(…): R`.
    if (this.isP("(") || this.isP("<")) {
      const { typeParams, params } = this.signatureTail();
      this.expectP(":");
      return { m: "call", typeParams, params, ret: this.returnType() };
    }
    // Construct signature `new (…): R`.
    if (this.isId("new") && (this.isP("(", 1) || this.isP("<", 1))) {
      this.i++;
      const { typeParams, params } = this.signatureTail();
      this.expectP(":");
      return { m: "construct", typeParams, params, ret: this.type() };
    }
    let readonly = false;
    if (this.isId("readonly") && !this.isP(":", 1) && !this.isP("?", 1) && !this.isP("(", 1) && !this.isP("<", 1)) {
      this.i++;
      readonly = true;
    }
    // Index signature `[key: K]: V`.
    if (this.isP("[") && this.isId(undefined, 1) && this.isP(":", 2)) {
      this.i += 3;
      const key = this.type();
      this.expectP("]");
      this.expectP(":");
      return { m: "index", readonly, key, type: this.type() };
    }
    // `get x(): T` / `set x(v: T)` accessors.
    if ((this.isId("get") || this.isId("set")) && (this.isId(undefined, 1) || this.peek(1)?.kind === "str") && this.isP("(", 2)) {
      const accessor = this.ident();
      const name = this.memberName();
      const params = this.params();
      const type = this.eatP(":") ? this.type() : null;
      return { m: "accessor", accessor, name, params, type };
    }
    const name = this.memberName();
    const optional = this.eatP("?");
    if (this.isP("(") || this.isP("<")) {
      const { typeParams, params } = this.signatureTail();
      this.expectP(":");
      return { m: "method", name, optional, typeParams, params, ret: this.returnType() };
    }
    this.expectP(":");
    return { m: "property", name, optional, readonly, type: this.type() };
  }
}

/** Parse a printed type; throws on syntax it does not support. */
export function parseType(text) {
  const parser = new Parser(tokenize(String(text)));
  if (!parser.t.length) throw new Error("an empty printed type");
  const type = parser.type();
  if (parser.i !== parser.t.length) throw new Error(`unexpected ${parser.describe()} after a printed type`);
  return type;
}

// ---------------------------------------------------------------- render

// A union or intersection member that would change meaning unparenthesised.
const loosePrecedence = (n) => ["function", "constructor", "conditional", "union", "intersection"].includes(n.k);
// An operand of `keyof`, `[]` or `T[K]`.
const operandPrecedence = (n) => loosePrecedence(n) || ["keyof", "infer"].includes(n.k);

function wrap(node, test) {
  const text = render(node);
  return test(node) ? `(${text})` : text;
}

function renderTypeParams(params) {
  if (!params?.length) return "";
  return `<${params
    .map(
      (p) =>
        `${p.modifiers.length ? p.modifiers.join(" ") + " " : ""}${p.name}${p.constraint ? ` extends ${render(p.constraint)}` : ""}${p.dflt ? ` = ${render(p.dflt)}` : ""}`,
    )
    .join(", ")}>`;
}

function renderParams(params) {
  return `(${params.map((p) => `${p.isThis ? "this: " : ""}${p.rest ? "..." : ""}${render(p.type)}${p.optional ? "?" : ""}`).join(", ")})`;
}

function flatten(kind, members) {
  return members.flatMap((m) => (m.k === kind ? flatten(kind, m.members) : [m]));
}

function renderUnion(node) {
  let texts = [...new Set(flatten("union", node.members).map((m) => wrap(m, loosePrecedence)))];
  if (texts.includes("false") && texts.includes("true")) texts = [...texts.filter((t) => t !== "false" && t !== "true"), "boolean"];
  return [...new Set(texts)].sort().join(" | ");
}

function renderMember(m) {
  switch (m.m) {
    case "call":
      return `${renderTypeParams(m.typeParams)}${renderParams(m.params)}: ${render(m.ret)}`;
    case "construct":
      return `new ${renderTypeParams(m.typeParams)}${renderParams(m.params)}: ${render(m.ret)}`;
    case "index":
      return `${m.readonly ? "readonly " : ""}[key: ${render(m.key)}]: ${render(m.type)}`;
    case "accessor":
      return `${m.accessor} ${JSON.stringify(m.name)}${renderParams(m.params)}${m.type ? `: ${render(m.type)}` : ""}`;
    case "method":
      return `${JSON.stringify(m.name)}${m.optional ? "?" : ""}${renderTypeParams(m.typeParams)}${renderParams(m.params)}: ${render(m.ret)}`;
    case "property":
      return `${m.readonly ? "readonly " : ""}${JSON.stringify(m.name)}${m.optional ? "?" : ""}: ${render(m.type)}`;
    default:
      throw new Error(`unknown member kind ${m.m}`);
  }
}

function renderObject(node) {
  // Index signatures (by key), then properties, methods and accessors by
  // name — keeping the order of same-named overloads — then call and
  // construct signatures in their order.
  const index = node.members.filter((m) => m.m === "index").map(renderMember).sort();
  const named = new Map();
  for (const m of node.members.filter((m) => ["property", "method", "accessor"].includes(m.m))) {
    if (!named.has(m.name)) named.set(m.name, []);
    named.get(m.name).push(renderMember(m));
  }
  const names = [...named.keys()].sort();
  const calls = node.members.filter((m) => m.m === "call").map(renderMember);
  const constructs = node.members.filter((m) => m.m === "construct").map(renderMember);
  const parts = [...index, ...names.flatMap((n) => named.get(n)), ...calls, ...constructs];
  return parts.length ? `{ ${parts.join("; ")}; }` : "{}";
}

function renderTemplate(node) {
  const escapeChunk = (chunk) => JSON.stringify(chunk).slice(1, -1).replace(/`/g, "\\`").replace(/\$\{/g, "\\${");
  let out = "`";
  node.chunks.forEach((chunk, i) => {
    out += escapeChunk(chunk);
    if (i < node.holes.length) out += "${" + render(node.holes[i]) + "}";
  });
  return out + "`";
}

function render(node) {
  switch (node.k) {
    case "kw":
      return node.name;
    case "lit":
      return node.repr;
    case "template":
      return renderTemplate(node);
    case "union":
      return renderUnion(node);
    case "intersection":
      return flatten("intersection", node.members)
        .map((m) => wrap(m, loosePrecedence))
        .join(" & ");
    case "array":
      return `${node.readonly ? "ReadonlyArray" : "Array"}<${render(node.element)}>`;
    case "tuple":
      return `${node.readonly ? "readonly " : ""}[${node.elements.map((e) => `${e.rest ? "..." : ""}${render(e.type)}${e.optional ? "?" : ""}`).join(", ")}]`;
    case "object":
      return renderObject(node);
    case "mapped":
      return `{ ${node.readonly ? `${node.readonly}readonly ` : ""}[${node.param} in ${render(node.constraint)}${node.as ? ` as ${render(node.as)}` : ""}]${node.optional ? `${node.optional}?` : ""}: ${render(node.template)}; }`;
    case "function":
      return `${renderTypeParams(node.typeParams)}${renderParams(node.params)} => ${render(node.ret)}`;
    case "constructor":
      return `${node.abstract ? "abstract " : ""}new ${renderTypeParams(node.typeParams)}${renderParams(node.params)} => ${render(node.ret)}`;
    case "predicate":
      return `${node.asserts ? "asserts " : ""}${node.name}${node.type ? ` is ${render(node.type)}` : ""}`;
    case "conditional":
      return `${wrap(node.check, operandPrecedence)} extends ${wrap(node.ext, (n) => n.k === "conditional" || n.k === "function")} ? ${render(node.whenTrue)} : ${render(node.whenFalse)}`;
    case "infer":
      return `infer ${node.name}${node.constraint ? ` extends ${render(node.constraint)}` : ""}`;
    case "keyof":
      return `keyof ${wrap(node.type, operandPrecedence)}`;
    case "indexed":
      return `${wrap(node.object, operandPrecedence)}[${render(node.index)}]`;
    case "typeof":
      return `typeof ${node.name}${node.args.length ? `<${node.args.map(render).join(", ")}>` : ""}`;
    case "import":
      return `import(${JSON.stringify(node.spec)})${node.name ? "." + node.name : ""}${node.args.length ? `<${node.args.map(render).join(", ")}>` : ""}`;
    case "ref":
      return `${node.name}${node.args.length ? `<${node.args.map(render).join(", ")}>` : ""}`;
    default:
      throw new Error(`unknown type node ${node.k}`);
  }
}

/** The canonical text of a printed type; throws on syntax it does not support. */
export function canonicalType(text) {
  return render(parseType(text));
}

/** A compact identity of a printed type: canonical length, digest, preview. */
export function canonicalDigest(text) {
  const canonical = canonicalType(text);
  return {
    sha256: createHash("sha256").update(canonical).digest("hex"),
    length: canonical.length,
    preview: canonical.length > 160 ? canonical.slice(0, 160) + "…" : canonical,
  };
}

/** The top-level union members of a printed type, each parsed. */
export function unionMemberNodes(text) {
  const node = parseType(text);
  return node.k === "union" ? flatten("union", node.members) : [node];
}

/** Render one parsed node canonically. */
export function renderNode(node) {
  return render(node);
}

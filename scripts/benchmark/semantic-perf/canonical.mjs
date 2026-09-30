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
//   - binders are positions: parameter names, type-parameter names, `infer`
//     names and mapped-type keys are replaced by their position in the
//     lexical environment (so consistent renaming is one type, swapped names
//     are two); tuple labels are dropped (optional and rest markers are
//     kept).
// Intersection order, tuple element order, argument order, modifiers and
// everything else are significant and kept.

import { createHash } from "node:crypto";

// ---------------------------------------------------------------- tokens

const PUNCT = [
  "=>",
  "...",
  "(",
  ")",
  "[",
  "]",
  "{",
  "}",
  "<",
  ">",
  "|",
  "&",
  ",",
  ";",
  ":",
  "?",
  "=",
  ".",
  "+",
  "-",
];

function decodeString(raw, quote) {
  const simple = {
    n: "\n",
    r: "\r",
    t: "\t",
    b: "\b",
    f: "\f",
    v: "\v",
    0: "\0",
    "\\": "\\",
    "'": "'",
    '"': '"',
    "`": "`",
    $: "$",
  };
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
      if (j >= text.length)
        throw new Error(`unterminated string literal in ${JSON.stringify(text.slice(i, i + 60))}`);
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
      const m =
        /^(0[xX][0-9a-fA-F_]+|0[bB][01_]+|0[oO][0-7_]+|(?:\d[\d_]*)?\.?\d[\d_]*(?:[eE][+-]?\d+)?)(n?)/.exec(
          text.slice(i),
        );
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
    if (
      this.isId("readonly") &&
      (this.isP("[", 1) || this.isId(undefined, 1) || this.isP("(", 1) || this.isP("{", 1))
    ) {
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
        while (
          (this.isId("in") || this.isId("out") || this.isId("const")) &&
          this.isId(undefined, 1)
        )
          modifiers.push(this.ident());
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
        if (
          this.isId() &&
          (this.isP(":", 1) || this.isP("?", 1) || this.isP(",", 1) || this.isP(")", 1))
        )
          name = this.ident();
        else throw new Error(`expected a parameter, found ${this.describe()}`);
        const optional = this.eatP("?");
        const type = this.eatP(":") ? this.returnType() : { k: "kw", name: "any" };
        list.push({ name, isThis: name === "this", rest, optional, type });
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
      if (this.eatP("=>"))
        return { k: "function", typeParams: null, params, ret: this.returnType() };
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
    const negative =
      t.kind === "p" &&
      t.text === "-" &&
      (this.peek(1)?.kind === "num" || this.peek(1)?.kind === "bigint");
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
    const keywords = [
      "any",
      "unknown",
      "never",
      "string",
      "number",
      "boolean",
      "symbol",
      "bigint",
      "object",
      "void",
      "undefined",
      "null",
      "this",
    ];
    if (
      (keywords.includes(t.text) || t.text === "true" || t.text === "false") &&
      !this.isP(".", 1) &&
      !this.isP("<", 1)
    ) {
      this.i++;
      if (t.text === "true" || t.text === "false") return { k: "lit", repr: t.text };
      return { k: "kw", name: t.text };
    }
    const name = this.entityName();
    const args = this.isP("<") ? this.typeArgs() : [];
    if (name === "Array" && args.length === 1)
      return { k: "array", element: args[0], readonly: false };
    if (name === "ReadonlyArray" && args.length === 1)
      return { k: "array", element: args[0], readonly: true };
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

  /**
   * A member's key, tagged: a written name (identifier, string or number —
   * `0` and `"0"` are one property) or a computed key (`[Symbol.iterator]`,
   * never equal to the string `"[Symbol.iterator]"`).
   */
  memberName() {
    const t = this.peek();
    if (t?.kind === "id") {
      this.i++;
      return { kind: "name", text: t.text };
    }
    if (t?.kind === "str") {
      this.i++;
      return { kind: "name", text: t.value };
    }
    if (t?.kind === "num") {
      this.i++;
      return { kind: "name", text: String(Number(t.text)) };
    }
    if (this.isP("[")) {
      this.i++;
      const inner = this.entityName();
      this.expectP("]");
      return { kind: "computed", text: inner };
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
      const { constraint, as } = this.nested(() => ({
        constraint: this.type(),
        as: this.eatId("as") ? this.type() : null,
      }));
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
        if (!this.eatP(";") && !this.eatP(",") && !this.isP("}"))
          throw new Error(`expected ; or } in an object type, found ${this.describe()}`);
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
    if (
      this.isId("readonly") &&
      !this.isP(":", 1) &&
      !this.isP("?", 1) &&
      !this.isP("(", 1) &&
      !this.isP("<", 1)
    ) {
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
    if (
      (this.isId("get") || this.isId("set")) &&
      (this.isId(undefined, 1) || this.peek(1)?.kind === "str") &&
      this.isP("(", 2)
    ) {
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
  if (parser.i !== parser.t.length)
    throw new Error(`unexpected ${parser.describe()} after a printed type`);
  return type;
}

// ---------------------------------------------------------------- normalise

// The canonical form is a normalised tree, compared by its serialisation:
// rendering is for display only and never re-parsed.
//
// Names a type binds are replaced by positions in a lexical environment, so
// consistent renaming is one type and swapped names are two: a signature
// binds its type parameters and its parameters (a `typeof` of a parameter
// and a type predicate's target refer to it by position), a conditional binds
// the `infer` names of its extends clause (in that clause and its true
// branch), a mapped type binds its key. A reference resolves to the innermost
// binding of its name (shadowing), as `{ up, index }`: how many scopes out,
// which binder there.

function flatten(kind, members) {
  return members.flatMap((m) => (m.k === kind ? flatten(kind, m.members) : [m]));
}

const keyOf = (node) => JSON.stringify(node);

/** A scope: names of one binding kind each map to their position. */
const scope = (entries) => ({
  types: new Map(entries.types ?? []),
  values: new Map(entries.values ?? []),
});

function lookup(env, space, name) {
  for (let i = env.length - 1; i >= 0; i--) {
    const index = env[i][space].get(name);
    if (index !== undefined) return { up: env.length - 1 - i, index };
  }
  return null;
}

/** The `infer` names a conditional's extends clause declares, in order (not those of nested conditionals). */
function inferNames(node, out = []) {
  if (!node || typeof node !== "object") return out;
  if (node.k === "infer") {
    if (!out.includes(node.name)) out.push(node.name);
    if (node.constraint) inferNames(node.constraint, out);
    return out;
  }
  if (node.k === "conditional") return out;
  for (const value of Object.values(node)) {
    if (Array.isArray(value)) value.forEach((v) => inferNames(v, out));
    else if (value && typeof value === "object") inferNames(value, out);
  }
  return out;
}

function normalizeSignature(typeParams, params, ret, env) {
  // One scope for the signature: its type parameters (visible in their own
  // constraints and defaults) and its parameters (visible to `typeof` in
  // later parameter types and in the return type).
  const frame = scope({
    types: (typeParams ?? []).map((p, i) => [p.name, i]),
    values: params
      .map((p, i) => [p.isThis ? "this" : p.name, i])
      .filter(([n]) => n && n !== "this"),
  });
  const inner = [...env, frame];
  const typeParamsOut = typeParams?.length
    ? typeParams.map((p) => ({
        modifiers: p.modifiers,
        constraint: p.constraint && normalize(p.constraint, inner),
        dflt: p.dflt && normalize(p.dflt, inner),
      }))
    : null;
  const paramsOut = params.map((p) => ({
    this: p.isThis,
    rest: p.rest,
    optional: p.optional,
    type: normalize(p.type, inner),
  }));
  let retOut;
  if (ret.k === "predicate") {
    const target =
      ret.name === "this"
        ? { this: true }
        : frame.values.has(ret.name)
          ? { param: frame.values.get(ret.name) }
          : { free: ret.name };
    retOut = {
      k: "predicate",
      asserts: ret.asserts,
      target,
      type: ret.type && normalize(ret.type, inner),
    };
  } else retOut = normalize(ret, inner);
  return { typeParams: typeParamsOut, params: paramsOut, ret: retOut };
}

function normalizeMember(m, env) {
  switch (m.m) {
    case "call":
    case "construct":
      return { m: m.m, ...normalizeSignature(m.typeParams, m.params, m.ret, env) };
    case "method":
      return {
        m: "method",
        name: m.name,
        optional: m.optional,
        ...normalizeSignature(m.typeParams, m.params, m.ret, env),
      };
    case "index":
      return {
        m: "index",
        readonly: m.readonly,
        key: normalize(m.key, env),
        type: normalize(m.type, env),
      };
    case "accessor": {
      const sig = normalizeSignature(null, m.params, m.type ?? { k: "kw", name: "void" }, env);
      return {
        m: "accessor",
        accessor: m.accessor,
        name: m.name,
        params: sig.params,
        type: m.type ? sig.ret : null,
      };
    }
    case "property":
      return {
        m: "property",
        name: m.name,
        optional: m.optional,
        readonly: m.readonly,
        type: normalize(m.type, env),
      };
    default:
      throw new Error(`unknown member kind ${m.m}`);
  }
}

function normalizeObject(node, env) {
  // Index signatures (by key), then properties, methods and accessors by
  // tagged name — keeping the order of same-named overloads — then call and
  // construct signatures in their order.
  const members = node.members.map((m) => normalizeMember(m, env));
  const index = members
    .filter((m) => m.m === "index")
    .sort((a, b) => keyOf(a).localeCompare(keyOf(b)));
  const named = new Map();
  for (const m of members.filter((x) => ["property", "method", "accessor"].includes(x.m))) {
    const k = keyOf(m.name);
    if (!named.has(k)) named.set(k, []);
    named.get(k).push(m);
  }
  const names = [...named.keys()].sort();
  return {
    k: "object",
    members: [
      ...index,
      ...names.flatMap((n) => named.get(n)),
      ...members.filter((m) => m.m === "call"),
      ...members.filter((m) => m.m === "construct"),
    ],
  };
}

/**
 * The normalised form of a parsed type (see the module comment for the
 * rules). `env` is the lexical environment of the enclosing binders.
 */
export function normalize(node, env = []) {
  const n = (child) => normalize(child, env);
  switch (node.k) {
    case "kw":
    case "lit":
      return node;
    case "template":
      return { k: "template", chunks: node.chunks, holes: node.holes.map(n) };
    case "union": {
      let members = flatten("union", flatten("union", node.members).map(n));
      const hasTrue = members.some((m) => m.k === "lit" && m.repr === "true");
      const hasFalse = members.some((m) => m.k === "lit" && m.repr === "false");
      if (hasTrue && hasFalse) {
        members = members.filter(
          (m) => !(m.k === "lit" && (m.repr === "true" || m.repr === "false")),
        );
        members.push({ k: "kw", name: "boolean" });
      }
      const unique = new Map(members.map((m) => [keyOf(m), m]));
      const sorted = [...unique.keys()].sort().map((k) => unique.get(k));
      return sorted.length === 1 ? sorted[0] : { k: "union", members: sorted };
    }
    case "intersection":
      return {
        k: "intersection",
        members: flatten("intersection", flatten("intersection", node.members).map(n)),
      };
    case "array":
      return { k: "array", readonly: node.readonly, element: n(node.element) };
    case "tuple":
      return {
        k: "tuple",
        readonly: node.readonly,
        elements: node.elements.map((e) => ({
          rest: e.rest,
          optional: e.optional,
          type: n(e.type),
        })),
      };
    case "object":
      return normalizeObject(node, env);
    case "mapped": {
      const inner = [...env, scope({ types: [[node.param, 0]] })];
      return {
        k: "mapped",
        readonly: node.readonly,
        constraint: n(node.constraint),
        as: node.as && normalize(node.as, inner),
        optional: node.optional,
        template: normalize(node.template, inner),
      };
    }
    case "function":
      return { k: "function", ...normalizeSignature(node.typeParams, node.params, node.ret, env) };
    case "constructor":
      return {
        k: "constructor",
        abstract: node.abstract,
        ...normalizeSignature(node.typeParams, node.params, node.ret, env),
      };
    case "predicate": {
      // A predicate outside a signature names whatever binds its target.
      const bound = node.name === "this" ? null : lookup(env, "values", node.name);
      const target =
        node.name === "this" ? { this: true } : bound ? { bound } : { free: node.name };
      return { k: "predicate", asserts: node.asserts, target, type: node.type && n(node.type) };
    }
    case "conditional": {
      const inner = [...env, scope({ types: inferNames(node.ext).map((name, i) => [name, i]) })];
      return {
        k: "conditional",
        check: n(node.check),
        ext: normalize(node.ext, inner),
        whenTrue: normalize(node.whenTrue, inner),
        whenFalse: n(node.whenFalse),
      };
    }
    case "infer": {
      // Declared by the innermost conditional's scope (the name is a binder).
      const bound = lookup(env, "types", node.name);
      return { k: "infer", bound, constraint: node.constraint && n(node.constraint) };
    }
    case "keyof":
      return { k: "keyof", type: n(node.type) };
    case "indexed":
      return { k: "indexed", object: n(node.object), index: n(node.index) };
    case "typeof": {
      const [head, ...path] = node.name.split(".");
      const bound = lookup(env, "values", head);
      return bound
        ? { k: "typeof", bound, path, args: node.args.map(n) }
        : { k: "typeof", name: node.name, args: node.args.map(n) };
    }
    case "import":
      return { k: "import", spec: node.spec, name: node.name, args: node.args.map(n) };
    case "ref": {
      if (!node.args.length && !node.name.includes(".")) {
        const bound = lookup(env, "types", node.name);
        if (bound) return { k: "bound", bound };
      }
      return { k: "ref", name: node.name, args: node.args.map(n) };
    }
    default:
      throw new Error(`unknown type node ${node.k}`);
  }
}

// ---------------------------------------------------------------- display

// A preview of a normalised tree, for reports only (never parsed again).
const loose = (n) =>
  ["function", "constructor", "conditional", "union", "intersection"].includes(n.k);
const operand = (n) => loose(n) || ["keyof", "infer"].includes(n.k);
const wrap = (n, test) => (test(n) ? `(${display(n)})` : display(n));
const typeParamsText = (ps) =>
  ps?.length
    ? `<${ps.map((p, i) => `${p.modifiers.length ? p.modifiers.join(" ") + " " : ""}T${i}${p.constraint ? ` extends ${display(p.constraint)}` : ""}${p.dflt ? ` = ${display(p.dflt)}` : ""}`).join(", ")}>`
    : "";
const boundText = (b) => (b ? "$" + b.up + "." + b.index : "?");
const paramsText = (ps) =>
  `(${ps.map((p, i) => `${p.this ? "this" : `$${i}`}${p.optional ? "?" : ""}: ${p.rest ? "..." : ""}${display(p.type)}`).join(", ")})`;
const keyText = (name) =>
  name.kind === "computed"
    ? `[${name.text}]`
    : /^[A-Za-z_$][\w$]*$/.test(name.text)
      ? name.text
      : JSON.stringify(name.text);

function memberText(m) {
  switch (m.m) {
    case "call":
      return `${typeParamsText(m.typeParams)}${paramsText(m.params)}: ${display(m.ret)}`;
    case "construct":
      return `new ${typeParamsText(m.typeParams)}${paramsText(m.params)}: ${display(m.ret)}`;
    case "index":
      return `${m.readonly ? "readonly " : ""}[key: ${display(m.key)}]: ${display(m.type)}`;
    case "accessor":
      return `${m.accessor} ${keyText(m.name)}${paramsText(m.params)}${m.type ? `: ${display(m.type)}` : ""}`;
    case "method":
      return `${keyText(m.name)}${m.optional ? "?" : ""}${typeParamsText(m.typeParams)}${paramsText(m.params)}: ${display(m.ret)}`;
    default:
      return `${m.readonly ? "readonly " : ""}${keyText(m.name)}${m.optional ? "?" : ""}: ${display(m.type)}`;
  }
}

function display(n) {
  switch (n.k) {
    case "kw":
      return n.name;
    case "lit":
      return n.repr;
    case "template":
      return (
        "`" +
        n.chunks
          .map(
            (c, i) =>
              JSON.stringify(c).slice(1, -1).replace(/`/g, "\\`") +
              (i < n.holes.length ? "${" + display(n.holes[i]) + "}" : ""),
          )
          .join("") +
        "`"
      );
    case "union":
      return n.members.map((m) => wrap(m, loose)).join(" | ");
    case "intersection":
      return n.members.map((m) => wrap(m, loose)).join(" & ");
    case "array":
      return `${n.readonly ? "ReadonlyArray" : "Array"}<${display(n.element)}>`;
    case "tuple":
      return `${n.readonly ? "readonly " : ""}[${n.elements.map((e) => `${e.rest ? "..." : ""}${display(e.type)}${e.optional ? "?" : ""}`).join(", ")}]`;
    case "object":
      return n.members.length ? `{ ${n.members.map(memberText).join("; ")}; }` : "{}";
    case "mapped":
      return `{ ${n.readonly ? `${n.readonly}readonly ` : ""}[K in ${display(n.constraint)}${n.as ? ` as ${display(n.as)}` : ""}]${n.optional ? `${n.optional}?` : ""}: ${display(n.template)}; }`;
    case "function":
      return `${typeParamsText(n.typeParams)}${paramsText(n.params)} => ${display(n.ret)}`;
    case "constructor":
      return `${n.abstract ? "abstract " : ""}new ${typeParamsText(n.typeParams)}${paramsText(n.params)} => ${display(n.ret)}`;
    case "predicate": {
      const t = n.target.this
        ? "this"
        : n.target.param !== undefined
          ? "$" + n.target.param
          : n.target.bound
            ? boundText(n.target.bound)
            : n.target.free;
      return `${n.asserts ? "asserts " : ""}${t}${n.type ? ` is ${display(n.type)}` : ""}`;
    }
    case "conditional":
      return `${wrap(n.check, operand)} extends ${wrap(n.ext, (x) => x.k === "conditional" || x.k === "function")} ? ${display(n.whenTrue)} : ${display(n.whenFalse)}`;
    case "infer":
      return `infer ${boundText(n.bound)}${n.constraint ? ` extends ${display(n.constraint)}` : ""}`;
    case "bound":
      return boundText(n.bound);
    case "keyof":
      return `keyof ${wrap(n.type, operand)}`;
    case "indexed":
      return `${wrap(n.object, operand)}[${display(n.index)}]`;
    case "typeof":
      return `typeof ${n.bound ? boundText(n.bound) + n.path.map((p) => "." + p).join("") : n.name}${n.args.length ? `<${n.args.map(display).join(", ")}>` : ""}`;
    case "import":
      return `import(${JSON.stringify(n.spec)})${n.name ? "." + n.name : ""}${n.args.length ? `<${n.args.map(display).join(", ")}>` : ""}`;
    case "ref":
      return `${n.name}${n.args.length ? `<${n.args.map(display).join(", ")}>` : ""}`;
    default:
      return `<${n.k}>`;
  }
}

// ---------------------------------------------------------------- API

/** The canonical key of a printed type: equal keys, equal types. Throws on unsupported syntax. */
export function canonicalType(text) {
  return keyOf(normalize(parseType(text)));
}

/** A compact identity of a normalised tree: digest of its canonical key, key length, display preview. */
export function digestNode(normalized) {
  const key = keyOf(normalized);
  const preview = display(normalized);
  return {
    sha256: createHash("sha256").update(key).digest("hex"),
    length: key.length,
    preview: preview.length > 160 ? preview.slice(0, 160) + "…" : preview,
  };
}

/** The digest of a printed type. */
export function canonicalDigest(text) {
  return digestNode(normalize(parseType(text)));
}

/** The top-level union members of a printed type, each parsed (not normalised). */
export function unionMemberNodes(text) {
  const node = parseType(text);
  return node.k === "union" ? flatten("union", node.members) : [node];
}

/** Whether a normalised tree is the keyword `name`. */
export const isKeyword = (normalized, name) => normalized.k === "kw" && normalized.name === name;

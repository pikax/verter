/**
 * Lightweight recursive-descent parser for TypeScript type annotation strings.
 *
 * Converts type annotation strings (from `AnalyzedPropField.typeAnnotation`)
 * into TypeDescriptor trees. Handles the common subset appearing in Vue
 * component APIs:
 *
 * - Primitives, string/number/boolean literals
 * - Unions (`|`), intersections (`&`)
 * - Arrays (`T[]`, `Array<T>`), tuples (`[A, B]`)
 * - Object literals (`{ prop: Type; prop?: Type }`)
 * - Function types (`(x: T) => R`)
 * - Generic references (`Map<string, number>`)
 * - Parenthesized groups
 *
 * Falls back to `{ kind: 'unknown', rawType }` for types too complex
 * to parse statically (conditional types, mapped types, template literals).
 */

import type { TypeDescriptor, FunctionParameter, ObjectProperty } from "./type-ir.js";
import {
  primitive,
  literal,
  union,
  intersection,
  array,
  tuple,
  object,
  func,
  ref,
  unknown,
} from "./type-ir.js";
import type { PrimitiveName } from "./type-ir.js";

const PRIMITIVES = new Set<string>([
  "string",
  "number",
  "boolean",
  "symbol",
  "bigint",
  "any",
  "unknown",
  "void",
  "never",
  "null",
  "undefined",
  "object",
]);

// ── Tokenizer ────────────────────────────────────────────────────

const enum TokenKind {
  Identifier,
  StringLiteral,
  NumberLiteral,
  Pipe, // |
  Ampersand, // &
  LParen, // (
  RParen, // )
  LBracket, // [
  RBracket, // ]
  LBrace, // {
  RBrace, // }
  LAngle, // <
  RAngle, // >
  Comma, // ,
  Colon, // :
  Semicolon, // ;
  Question, // ?
  Arrow, // =>
  DotDotDot, // ...
  EOF,
}

interface Token {
  kind: TokenKind;
  value: string;
}

function tokenize(input: string): Token[] {
  const tokens: Token[] = [];
  let i = 0;
  const len = input.length;

  while (i < len) {
    const ch = input[i];

    // Whitespace
    if (ch === " " || ch === "\t" || ch === "\n" || ch === "\r") {
      i++;
      continue;
    }

    // Arrow =>
    if (ch === "=" && input[i + 1] === ">") {
      tokens.push({ kind: TokenKind.Arrow, value: "=>" });
      i += 2;
      continue;
    }

    // Spread ...
    if (ch === "." && input[i + 1] === "." && input[i + 2] === ".") {
      tokens.push({ kind: TokenKind.DotDotDot, value: "..." });
      i += 3;
      continue;
    }

    // Single-char tokens
    const single: Record<string, TokenKind> = {
      "|": TokenKind.Pipe,
      "&": TokenKind.Ampersand,
      "(": TokenKind.LParen,
      ")": TokenKind.RParen,
      "[": TokenKind.LBracket,
      "]": TokenKind.RBracket,
      "{": TokenKind.LBrace,
      "}": TokenKind.RBrace,
      "<": TokenKind.LAngle,
      ">": TokenKind.RAngle,
      ",": TokenKind.Comma,
      ":": TokenKind.Colon,
      ";": TokenKind.Semicolon,
      "?": TokenKind.Question,
    };

    if (ch in single) {
      tokens.push({ kind: single[ch], value: ch });
      i++;
      continue;
    }

    // String literal (single or double quoted)
    if (ch === "'" || ch === '"') {
      const quote = ch;
      let str = "";
      i++; // skip opening quote
      while (i < len && input[i] !== quote) {
        if (input[i] === "\\") {
          str += input[i + 1] ?? "";
          i += 2;
        } else {
          str += input[i];
          i++;
        }
      }
      i++; // skip closing quote
      tokens.push({ kind: TokenKind.StringLiteral, value: str });
      continue;
    }

    // Number literal (including negative)
    if (ch === "-" || (ch >= "0" && ch <= "9")) {
      let num = ch;
      i++;
      while (i < len && ((input[i] >= "0" && input[i] <= "9") || input[i] === ".")) {
        num += input[i];
        i++;
      }
      // Only treat as number if it's actually a number (not just "-")
      if (num !== "-") {
        tokens.push({ kind: TokenKind.NumberLiteral, value: num });
        continue;
      }
      // Fallback: treat "-" as unknown
      i--;
    }

    // Identifier (also handles `true`, `false`, `readonly`, `keyof`, `typeof`, `infer`)
    if ((ch >= "a" && ch <= "z") || (ch >= "A" && ch <= "Z") || ch === "_" || ch === "$") {
      let ident = ch;
      i++;
      while (i < len) {
        const c = input[i];
        if (
          (c >= "a" && c <= "z") ||
          (c >= "A" && c <= "Z") ||
          (c >= "0" && c <= "9") ||
          c === "_" ||
          c === "$"
        ) {
          ident += c;
          i++;
        } else {
          break;
        }
      }
      tokens.push({ kind: TokenKind.Identifier, value: ident });
      continue;
    }

    // Unknown character — skip
    i++;
  }

  tokens.push({ kind: TokenKind.EOF, value: "" });
  return tokens;
}

// ── Parser ───────────────────────────────────────────────────────

class Parser {
  private tokens: Token[];
  private pos: number;
  private input: string;

  constructor(tokens: Token[], input: string) {
    this.tokens = tokens;
    this.pos = 0;
    this.input = input;
  }

  private peek(): Token {
    return this.tokens[this.pos];
  }

  private advance(): Token {
    return this.tokens[this.pos++];
  }

  private expect(kind: TokenKind): Token {
    const tok = this.advance();
    if (tok.kind !== kind) {
      throw new Error(`Expected ${kind}, got ${tok.kind} (${tok.value})`);
    }
    return tok;
  }

  private match(kind: TokenKind): boolean {
    if (this.peek().kind === kind) {
      this.advance();
      return true;
    }
    return false;
  }

  /** Entry point: parse a complete type. */
  parse(): TypeDescriptor {
    const result = this.parseUnion();
    return result;
  }

  /** Union: `A | B | C` */
  private parseUnion(): TypeDescriptor {
    // Allow leading `|`
    this.match(TokenKind.Pipe);
    const types: TypeDescriptor[] = [this.parseIntersection()];
    while (this.peek().kind === TokenKind.Pipe) {
      this.advance();
      types.push(this.parseIntersection());
    }
    return union(types);
  }

  /** Intersection: `A & B & C` */
  private parseIntersection(): TypeDescriptor {
    const types: TypeDescriptor[] = [this.parsePostfix()];
    while (this.peek().kind === TokenKind.Ampersand) {
      this.advance();
      types.push(this.parsePostfix());
    }
    return intersection(types);
  }

  /** Postfix: `T[]`, `T[][]`, etc. */
  private parsePostfix(): TypeDescriptor {
    let type = this.parsePrimary();
    while (
      this.peek().kind === TokenKind.LBracket &&
      this.tokens[this.pos + 1]?.kind === TokenKind.RBracket
    ) {
      this.advance(); // [
      this.advance(); // ]
      type = array(type);
    }
    return type;
  }

  /** Primary: identifier, literal, parenthesized, tuple, object, function */
  private parsePrimary(): TypeDescriptor {
    const tok = this.peek();

    // Parenthesized group or function type
    if (tok.kind === TokenKind.LParen) {
      return this.parseParenOrFunction();
    }

    // Tuple: `[A, B, C]`
    if (tok.kind === TokenKind.LBracket) {
      return this.parseTuple();
    }

    // Object literal: `{ key: Type; ... }`
    if (tok.kind === TokenKind.LBrace) {
      return this.parseObjectType();
    }

    // String literal: `'primary'`
    if (tok.kind === TokenKind.StringLiteral) {
      this.advance();
      return literal(tok.value);
    }

    // Number literal: `42`
    if (tok.kind === TokenKind.NumberLiteral) {
      this.advance();
      return literal(Number(tok.value));
    }

    // Identifier: primitive, `true`, `false`, `readonly`, or named type
    if (tok.kind === TokenKind.Identifier) {
      return this.parseIdentifierType();
    }

    // Fallback
    throw new Error(`Unexpected token: ${tok.kind} (${tok.value})`);
  }

  private parseIdentifierType(): TypeDescriptor {
    const tok = this.advance();
    const name = tok.value;

    // Boolean literals
    if (name === "true") return literal(true);
    if (name === "false") return literal(false);

    // Skip `readonly` prefix
    if (name === "readonly") {
      return this.parsePrimary();
    }

    // `keyof`, `typeof`, `infer` — fallback to unknown
    if (name === "keyof" || name === "typeof" || name === "infer") {
      return unknown(this.input);
    }

    // Primitives
    if (PRIMITIVES.has(name)) {
      return primitive(name as PrimitiveName);
    }

    // Named type — may have generic arguments `<...>`
    if (this.peek().kind === TokenKind.LAngle) {
      const typeArgs = this.parseTypeArguments();

      // Special-case: `Array<T>` → array(T)
      if (name === "Array" && typeArgs.length === 1) {
        return array(typeArgs[0]);
      }
      // Special-case: `Record<K, V>` → object([])
      // We keep it as ref since the structure is opaque without resolution

      return ref(name, typeArgs);
    }

    return ref(name);
  }

  private parseTypeArguments(): TypeDescriptor[] {
    this.expect(TokenKind.LAngle); // <
    const args: TypeDescriptor[] = [];
    let depth = 1;

    // We need to handle nested < > correctly
    if (this.peek().kind !== TokenKind.RAngle) {
      args.push(this.parseUnion());
      while (this.peek().kind === TokenKind.Comma) {
        this.advance();
        if (this.peek().kind === TokenKind.RAngle) break;
        args.push(this.parseUnion());
      }
    }
    this.expect(TokenKind.RAngle); // >
    return args;
  }

  private parseParenOrFunction(): TypeDescriptor {
    // Look ahead to determine if this is a function type or parenthesized group.
    // Function type: `(x: T, y: U) => R`
    // Parenthesized: `(A | B)`
    //
    // Heuristic: if after `(` we see `identifier :` or `)` followed by `=>`,
    // treat as function type.

    const saved = this.pos;

    if (this.isFunctionType()) {
      this.pos = saved;
      return this.parseFunctionType();
    }

    this.pos = saved;
    // Parenthesized group
    this.advance(); // (
    const inner = this.parseUnion();
    this.expect(TokenKind.RParen);
    return inner;
  }

  /** Peek ahead to determine if `(...)` is a function type signature. */
  private isFunctionType(): boolean {
    this.advance(); // skip (

    // `() => R` — empty params
    if (this.peek().kind === TokenKind.RParen) {
      this.advance(); // skip )
      return this.peek().kind === TokenKind.Arrow;
    }

    // `(... ` — rest param
    if (this.peek().kind === TokenKind.DotDotDot) {
      return true;
    }

    // `(ident: ...` or `(ident? : ...`
    if (this.peek().kind === TokenKind.Identifier) {
      this.advance();
      if (this.peek().kind === TokenKind.Colon) return true;
      if (this.peek().kind === TokenKind.Question) {
        this.advance();
        if (this.peek().kind === TokenKind.Colon) return true;
      }
    }

    return false;
  }

  private parseFunctionType(): TypeDescriptor {
    this.expect(TokenKind.LParen);
    const params: FunctionParameter[] = [];

    while (this.peek().kind !== TokenKind.RParen && this.peek().kind !== TokenKind.EOF) {
      // Handle rest param `...args: T`
      this.match(TokenKind.DotDotDot);

      const paramName = this.expect(TokenKind.Identifier).value;
      const optional = this.match(TokenKind.Question);
      this.expect(TokenKind.Colon);
      const paramType = this.parseUnion();
      params.push({ name: paramName, type: paramType, optional });

      if (!this.match(TokenKind.Comma)) break;
    }

    this.expect(TokenKind.RParen);
    this.expect(TokenKind.Arrow);
    const returnType = this.parseUnion();

    return func(params, returnType);
  }

  private parseTuple(): TypeDescriptor {
    this.expect(TokenKind.LBracket);
    const elements: TypeDescriptor[] = [];

    while (this.peek().kind !== TokenKind.RBracket && this.peek().kind !== TokenKind.EOF) {
      // Handle labeled tuple elements: `name: Type` or `name?: Type`
      if (this.peek().kind === TokenKind.Identifier && this.pos + 1 < this.tokens.length) {
        const next = this.tokens[this.pos + 1];
        // Check for `label:` or `label?:` pattern
        if (next.kind === TokenKind.Colon) {
          // Skip label and colon
          this.advance(); // label
          this.advance(); // :
        } else if (
          next.kind === TokenKind.Question &&
          this.pos + 2 < this.tokens.length &&
          this.tokens[this.pos + 2].kind === TokenKind.Colon
        ) {
          // Skip label, ?, and colon
          this.advance(); // label
          this.advance(); // ?
          this.advance(); // :
        }
      }
      elements.push(this.parseUnion());
      if (!this.match(TokenKind.Comma)) break;
    }

    this.expect(TokenKind.RBracket);
    return tuple(elements);
  }

  private parseObjectType(): TypeDescriptor {
    this.expect(TokenKind.LBrace);
    const properties: ObjectProperty[] = [];

    while (this.peek().kind !== TokenKind.RBrace && this.peek().kind !== TokenKind.EOF) {
      // Skip `readonly` modifier
      if (this.peek().kind === TokenKind.Identifier && this.peek().value === "readonly") {
        this.advance();
      }

      if (this.peek().kind === TokenKind.RBrace) break;

      // Index signature `[key: string]: Type` — fallback to unknown
      if (this.peek().kind === TokenKind.LBracket) {
        return unknown(this.input);
      }

      const propName = this.parsePropertyName();
      if (propName === null) break;

      const optional = this.match(TokenKind.Question);
      this.expect(TokenKind.Colon);
      const propType = this.parseUnion();
      properties.push({ name: propName, type: propType, optional });

      // Accept both `;` and `,` as separators
      if (!this.match(TokenKind.Semicolon)) {
        this.match(TokenKind.Comma);
      }
    }

    this.expect(TokenKind.RBrace);
    return object(properties);
  }

  private parsePropertyName(): string | null {
    const tok = this.peek();
    if (tok.kind === TokenKind.Identifier) {
      this.advance();
      return tok.value;
    }
    if (tok.kind === TokenKind.StringLiteral) {
      this.advance();
      return tok.value;
    }
    if (tok.kind === TokenKind.NumberLiteral) {
      this.advance();
      return tok.value;
    }
    return null;
  }
}

// ── Public API ───────────────────────────────────────────────────

/**
 * Parse a TypeScript type annotation string into a TypeDescriptor tree.
 *
 * Falls back to `{ kind: 'unknown', rawType }` when the input is too
 * complex or malformed.
 */
export function parseType(input: string): TypeDescriptor {
  const trimmed = input.trim();
  if (!trimmed) return unknown("");

  try {
    const tokens = tokenize(trimmed);
    const parser = new Parser(tokens, trimmed);
    return parser.parse();
  } catch {
    return unknown(trimmed);
  }
}

/**
 * Convert a Vue runtime type constructor name to a TypeDescriptor.
 *
 * Used for Options API props where only `type: String` (etc.) is available.
 */
export function runtimeTypeToDescriptor(constructorName: string): TypeDescriptor {
  switch (constructorName) {
    case "String":
      return primitive("string");
    case "Number":
      return primitive("number");
    case "Boolean":
      return primitive("boolean");
    case "Symbol":
      return primitive("symbol");
    case "BigInt":
      return primitive("bigint");
    case "Array":
      return array(unknown("unknown"));
    case "Object":
      return object([]);
    case "Function":
      return func([], unknown("unknown"));
    case "Date":
      return ref("Date");
    case "RegExp":
      return ref("RegExp");
    case "Promise":
      return ref("Promise", [unknown("unknown")]);
    case "Map":
      return ref("Map", [unknown("unknown"), unknown("unknown")]);
    case "Set":
      return ref("Set", [unknown("unknown")]);
    default:
      return ref(constructorName);
  }
}

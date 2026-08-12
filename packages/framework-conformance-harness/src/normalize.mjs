// Parser-backed cosmetic normalizer (conformance-normalizer.md).
//
// Produces a CANONICAL FORM of a generated module by re-parsing it to an
// ESTree AST and returning a position-free, alpha-normalized structural
// tree. Two programs whose canonical forms are deep-equal are cosmetically
// identical under the allowed normalization rules:
//
//   - whitespace/line-layout: FREE — canonical form never carries
//     start/end/loc/range, so re-indentation or reflow never affects it.
//   - quote-delimiter spelling: FREE — acorn decodes string literals to
//     their `.value`; the canonical form compares decoded values, never raw
//     source text, so `'x'` and `"x"` canonicalize identically.
//   - harmless redundant parentheses: FREE — parentheses are not ESTree
//     nodes; the parser already resolves precedence, so re-parenthesizing
//     an equivalent expression produces the identical AST shape.
//   - private generated identifier spelling: performed EXPLICITLY below by
//     `canonicalize()`'s scope-aware local-binding renamer.
//
// Everything the contract forbids stays live BY CONSTRUCTION, not by a
// separate check bolted on after the fact:
//   - import/export sources and specifiers: never renamed (see
//     `isRenamableBinding` — Import*Specifier locals and every exported
//     name are excluded from the rename map) and their `source.value` /
//     property positions are ordinary struct fields the deep-equal walk
//     compares verbatim.
//   - helper/declaration/statement ORDER: the canonical form is a
//     positional array walk (`Program.body`, `BlockStatement.body`, …) —
//     reordering two statements changes the canonical array order, which
//     `deepEqualCanonical` treats as inequality.
//   - literal values, property keys, JSX-free string content (element tags,
//     class names, prop names, diagnostic text carried as string literals):
//     never touched — only `Identifier` BINDING/REFERENCE nodes are
//     candidates for renaming, and then only when they resolve to a
//     tracked local binding (see `resolve()`); an ObjectExpression
//     property KEY identifier is not a binding or a reference (it is a
//     property name) and is walked as an ordinary literal-shaped leaf, so
//     `{ class: "x" }` survives untouched even after `class` collides with
//     an unrelated local variable name elsewhere in the same module.

import * as acorn from "acorn";

export function parseModule(code, sourceFileForDiagnostics) {
  return acorn.parse(code, {
    ecmaVersion: "latest",
    sourceType: "module",
    locations: true,
    allowAwaitOutsideFunction: true,
    sourceFile: sourceFileForDiagnostics,
  });
}

const FUNCTION_TYPES = new Set([
  "FunctionDeclaration",
  "FunctionExpression",
  "ArrowFunctionExpression",
]);

/** Scope frame: name -> canonical id. `kind` is "function" or "block". */
class Scope {
  constructor(parent, kind) {
    this.parent = parent;
    this.kind = kind;
    this.bindings = new Map();
  }
  /** Function-scoped (`var`) declarations attach to the nearest function/program frame. */
  functionScope() {
    let scope = this;
    while (scope.kind === "block") scope = scope.parent;
    return scope;
  }
  declare(name, canonicalId) {
    this.bindings.set(name, canonicalId);
  }
  resolve(name) {
    let scope = this;
    while (scope) {
      if (scope.bindings.has(name)) return scope.bindings.get(name);
      scope = scope.parent;
    }
    return null;
  }
}

/**
 * @returns {{ tree: object, renameCount: number }}
 */
export function canonicalize(ast) {
  let counter = 0;
  let renameCount = 0;
  const exportedNames = collectExportedNames(ast);

  function freshId() {
    const id = `$local${counter}`;
    counter += 1;
    return id;
  }

  function declareBinding(scope, name, isRenamable) {
    if (!isRenamable || exportedNames.has(name)) {
      scope.declare(name, null); // shadows outer without renaming
      return;
    }
    const id = freshId();
    scope.declare(name, id);
    renameCount += 1;
  }

  function declarePattern(scope, pattern, targetScope, isRenamable) {
    if (!pattern) return;
    switch (pattern.type) {
      case "Identifier":
        declareBinding(targetScope, pattern.name, isRenamable);
        return;
      case "AssignmentPattern":
        declarePattern(scope, pattern.left, targetScope, isRenamable);
        return;
      case "ArrayPattern":
        for (const el of pattern.elements) declarePattern(scope, el, targetScope, isRenamable);
        return;
      case "ObjectPattern":
        for (const prop of pattern.properties) {
          if (prop.type === "RestElement")
            declarePattern(scope, prop.argument, targetScope, isRenamable);
          else declarePattern(scope, prop.value, targetScope, isRenamable);
        }
        return;
      case "RestElement":
        declarePattern(scope, pattern.argument, targetScope, isRenamable);
        return;
      default:
        return;
    }
  }

  function visitPattern(pattern, scope) {
    if (!pattern) return null;
    switch (pattern.type) {
      case "Identifier": {
        const canonical = scope.resolve(pattern.name);
        return { type: "Identifier", name: canonical ?? pattern.name };
      }
      case "AssignmentPattern":
        return {
          type: "AssignmentPattern",
          left: visitPattern(pattern.left, scope),
          right: visit(pattern.right, scope),
        };
      case "ArrayPattern":
        return {
          type: "ArrayPattern",
          elements: pattern.elements.map((el) => visitPattern(el, scope)),
        };
      case "ObjectPattern":
        return {
          type: "ObjectPattern",
          properties: pattern.properties.map((prop) =>
            prop.type === "RestElement"
              ? { type: "RestElement", argument: visitPattern(prop.argument, scope) }
              : {
                  type: "Property",
                  computed: prop.computed,
                  shorthand: prop.shorthand,
                  key: prop.computed ? visit(prop.key, scope) : leafKey(prop.key),
                  value: visitPattern(prop.value, scope),
                },
          ),
        };
      case "RestElement":
        return { type: "RestElement", argument: visitPattern(pattern.argument, scope) };
      default:
        return visit(pattern, scope);
    }
  }

  /** Property keys are names, not references — never renamed. */
  function leafKey(key) {
    if (key.type === "Identifier") return { type: "Identifier", name: key.name };
    return stripLeaf(key);
  }

  function stripLeaf(node) {
    if (node === null || typeof node !== "object") return node;
    if (Array.isArray(node)) return node.map(stripLeaf);
    const out = {};
    for (const [k, v] of Object.entries(node)) {
      if (k === "start" || k === "end" || k === "loc" || k === "range") continue;
      out[k] = stripLeaf(v);
    }
    return out;
  }

  function hoistVarsAndFunctions(body, scope) {
    for (const stmt of body) hoistStatement(stmt, scope);
  }

  function hoistStatement(stmt, scope) {
    if (!stmt) return;
    switch (stmt.type) {
      case "VariableDeclaration":
        if (stmt.kind === "var") {
          for (const decl of stmt.declarations)
            declarePattern(scope, decl.id, scope.functionScope(), true);
        }
        return;
      case "FunctionDeclaration":
        if (stmt.id) declareBinding(scope, stmt.id.name, true);
        return;
      case "IfStatement":
        hoistStatement(stmt.consequent, scope);
        hoistStatement(stmt.alternate, scope);
        return;
      case "ForStatement":
      case "ForOfStatement":
      case "ForInStatement":
        if (stmt.init?.type === "VariableDeclaration") hoistStatement(stmt.init, scope);
        if (stmt.left?.type === "VariableDeclaration") hoistStatement(stmt.left, scope);
        hoistStatement(stmt.body, scope);
        return;
      case "WhileStatement":
      case "DoWhileStatement":
        hoistStatement(stmt.body, scope);
        return;
      case "BlockStatement":
        hoistVarsAndFunctions(stmt.body, scope);
        return;
      case "TryStatement":
        hoistStatement(stmt.block, scope);
        if (stmt.handler) hoistStatement(stmt.handler.body, scope);
        if (stmt.finalizer) hoistStatement(stmt.finalizer, scope);
        return;
      case "SwitchStatement":
        for (const c of stmt.cases) hoistVarsAndFunctions(c.consequent, scope);
        return;
      default:
        return;
    }
  }

  function visitBlockScoped(body, parentScope) {
    const scope = new Scope(parentScope, "block");
    for (const stmt of body) {
      if (stmt.type === "VariableDeclaration" && stmt.kind !== "var") {
        for (const decl of stmt.declarations) declarePattern(scope, decl.id, scope, true);
      }
      if (stmt.type === "ClassDeclaration" && stmt.id) declareBinding(scope, stmt.id.name, true);
    }
    return { scope, tree: body.map((stmt) => visit(stmt, scope)) };
  }

  function visitFunctionBody(fn, scope) {
    const fnScope = new Scope(scope, "function");
    for (const param of fn.params) declarePattern(fnScope, param, fnScope, true);
    if (fn.body.type === "BlockStatement") {
      hoistVarsAndFunctions(fn.body.body, fnScope);
      const { tree } = visitBlockScoped(fn.body.body, fnScope);
      return {
        params: fn.params.map((p) => visitPattern(p, fnScope)),
        body: { type: "BlockStatement", body: tree },
      };
    }
    return {
      params: fn.params.map((p) => visitPattern(p, fnScope)),
      body: visit(fn.body, fnScope),
    };
  }

  function visit(node, scope) {
    if (node === null || node === undefined) return node;
    if (Array.isArray(node)) return node.map((n) => visit(n, scope));
    if (typeof node !== "object") return node;

    switch (node.type) {
      case "Identifier": {
        const canonical = scope.resolve(node.name);
        return { type: "Identifier", name: canonical ?? node.name };
      }
      case "VariableDeclarator":
        return {
          type: "VariableDeclarator",
          id: visitPattern(node.id, scope),
          init: visit(node.init, scope),
        };
      case "VariableDeclaration":
        return {
          type: "VariableDeclaration",
          kind: node.kind,
          declarations: node.declarations.map((d) => visit(d, scope)),
        };
      case "FunctionDeclaration":
      case "FunctionExpression": {
        const { params, body } = visitFunctionBody(node, scope);
        return {
          type: node.type,
          id: node.id ? { type: "Identifier", name: node.id.name } : null,
          async: node.async,
          generator: node.generator,
          params,
          body,
        };
      }
      case "ArrowFunctionExpression": {
        const { params, body } = visitFunctionBody(node, scope);
        return { type: node.type, async: node.async, expression: node.expression, params, body };
      }
      case "BlockStatement":
        return { type: "BlockStatement", body: visitBlockScoped(node.body, scope).tree };
      case "CatchClause": {
        const catchScope = new Scope(scope, "block");
        if (node.param) declarePattern(catchScope, node.param, catchScope, true);
        return {
          type: "CatchClause",
          param: node.param ? visitPattern(node.param, catchScope) : null,
          body: { type: "BlockStatement", body: visitBlockScoped(node.body.body, catchScope).tree },
        };
      }
      case "ImportSpecifier":
      case "ImportDefaultSpecifier":
      case "ImportNamespaceSpecifier":
        // Import bindings are helper identities — never renamed, including
        // the local alias, which must stay observable for helper-source
        // substitution detection.
        return stripLeaf(node);
      case "ImportDeclaration":
        return stripLeaf(node);
      case "ExportNamedDeclaration":
      case "ExportDefaultDeclaration":
      case "ExportAllDeclaration":
        return {
          type: node.type,
          declaration: visit(node.declaration, scope),
          specifiers: node.specifiers ? node.specifiers.map(stripLeaf) : undefined,
          source: node.source ? stripLeaf(node.source) : undefined,
          exported: node.exported ? stripLeaf(node.exported) : undefined,
        };
      case "Property":
        return {
          type: "Property",
          computed: node.computed,
          shorthand: node.shorthand,
          method: node.method,
          kind: node.kind,
          key: node.computed ? visit(node.key, scope) : leafKey(node.key),
          value: visit(node.value, scope),
        };
      case "Literal":
        // Quote-delimiter spelling is cosmetic: compare the DECODED value
        // only, never `raw` (which carries the original quote characters).
        return { type: "Literal", value: node.value, regex: node.regex, bigint: node.bigint };
      case "TemplateElement":
        // Same rationale as Literal.raw: `cooked` is the decoded value;
        // `raw` carries the original escape/quote spelling.
        return { type: "TemplateElement", tail: node.tail, cooked: node.value?.cooked };
      case "MemberExpression":
        return {
          type: "MemberExpression",
          computed: node.computed,
          optional: node.optional,
          object: visit(node.object, scope),
          property: node.computed ? visit(node.property, scope) : leafKey(node.property),
        };
      default: {
        const out = { type: node.type };
        for (const [key, value] of Object.entries(node)) {
          if (
            key === "type" ||
            key === "start" ||
            key === "end" ||
            key === "loc" ||
            key === "range"
          )
            continue;
          out[key] = visit(value, scope);
        }
        return out;
      }
    }
  }

  const rootScope = new Scope(null, "function");
  hoistVarsAndFunctions(ast.body, rootScope);
  const canonicalBody = ast.body.map((stmt) => {
    if (stmt.type === "VariableDeclaration" && stmt.kind !== "var") {
      for (const decl of stmt.declarations) declarePattern(rootScope, decl.id, rootScope, true);
    }
    if (stmt.type === "ClassDeclaration" && stmt.id) declareBinding(rootScope, stmt.id.name, true);
    return visit(stmt, rootScope);
  });

  return { tree: { type: "Program", body: canonicalBody }, renameCount };
}

/** Names that must never be alpha-renamed because they are publicly observable. */
function collectExportedNames(ast) {
  const names = new Set();
  for (const stmt of ast.body) {
    if (stmt.type === "ExportNamedDeclaration") {
      if (stmt.declaration?.type === "VariableDeclaration") {
        for (const decl of stmt.declaration.declarations) collectPatternNames(decl.id, names);
      } else if (stmt.declaration?.type === "FunctionDeclaration" && stmt.declaration.id) {
        names.add(stmt.declaration.id.name);
      }
      for (const spec of stmt.specifiers ?? []) names.add(spec.local.name);
    }
  }
  return names;
}

function collectPatternNames(pattern, out) {
  if (!pattern) return;
  if (pattern.type === "Identifier") out.add(pattern.name);
  else if (pattern.type === "ArrayPattern")
    for (const el of pattern.elements) collectPatternNames(el, out);
  else if (pattern.type === "ObjectPattern")
    for (const prop of pattern.properties)
      collectPatternNames(prop.type === "RestElement" ? prop.argument : prop.value, out);
  else if (pattern.type === "AssignmentPattern") collectPatternNames(pattern.left, out);
}

export function deepEqualCanonical(a, b) {
  if (a === b) return true;
  if (typeof a !== typeof b) return false;
  if (a === null || b === null) return a === b;
  if (Array.isArray(a) !== Array.isArray(b)) return false;
  if (Array.isArray(a)) {
    if (a.length !== b.length) return false;
    return a.every((item, i) => deepEqualCanonical(item, b[i]));
  }
  if (typeof a === "object") {
    const keysA = Object.keys(a).sort();
    const keysB = Object.keys(b).sort();
    if (keysA.length !== keysB.length || keysA.some((k, i) => k !== keysB[i])) return false;
    return keysA.every((k) => deepEqualCanonical(a[k], b[k]));
  }
  return a === b;
}

/** Digest of the canonical tree — used as the normalized-golden digest. */
export function canonicalDigest(canonicalTree) {
  return stableStringify(canonicalTree);
}

function stableStringify(value) {
  if (value === null || typeof value !== "object") return JSON.stringify(value);
  if (Array.isArray(value)) return `[${value.map(stableStringify).join(",")}]`;
  const keys = Object.keys(value).sort();
  return `{${keys.map((k) => `${JSON.stringify(k)}:${stableStringify(value[k])}`).join(",")}}`;
}

/** Versioned identity for this normalizer's behavior — bump on any rule change. */
export const NORMALIZER_VERSION = 1;

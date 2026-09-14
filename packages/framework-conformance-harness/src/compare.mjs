// Independent oracles the normalizer sits alongside, not inside
// (conformance-normalizer.md: raw parse, import/export/link, execution,
// diagnostic, and mapping checks run outside the normalizer. A normalizer
// pass cannot override failure of any independent oracle). Runtime behaviour
// is reported apart from output similarity; the structural comparison
// additionally matches module-local bindings by scope (see
// `matchLocalBindings`).

import { createHash, randomUUID } from "node:crypto";
import { mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import path from "node:path";
import { pathToFileURL } from "node:url";

import { parseModule, canonicalize, canonicalDigest, deepEqualCanonical } from "./normalize.mjs";
import { VUE_DOMAIN, SVELTE_DOMAIN } from "./domain-pin.mjs";
import { validateAuthoredMapping } from "./mapping-oracle.mjs";

/** @returns {{ ok: true, ast: object } | { ok: false, error: string }} */
export function checkParseValidity(code, label) {
  try {
    const ast = parseModule(code, label);
    return { ok: true, ast };
  } catch (error) {
    return { ok: false, error: String(error.message ?? error) };
  }
}

/**
 * Exact package/version identities generated conformance artifacts may
 * link against (pinned official closures from domain-pin.mjs). Any bare
 * import outside this set is a link violation.
 */
const PINNED_PACKAGE_VERSIONS = new Map([
  ...Object.keys(VUE_DOMAIN.directPackages).map((name) => [name, VUE_DOMAIN.packageVersion]),
  ...Object.keys(SVELTE_DOMAIN.directPackages).map((name) => [name, SVELTE_DOMAIN.packageVersion]),
]);

/** Top-level package name of a bare specifier, or null for relative/URL. */
function barePackageName(specifier) {
  if (specifier.startsWith(".") || specifier.startsWith("/") || specifier.includes(":"))
    return null;
  const segments = specifier.split("/");
  return specifier.startsWith("@") ? segments.slice(0, 2).join("/") : segments[0];
}

const namespaceCache = new Map();

// `baseDir` (the shared per-framework oracle install, e.g.
// `.oracle-installs/vue`) is the SAME directory across every concurrent
// process that links against that oracle — one `node
// bin/check-candidate.mjs` invocation per test, all sharing the install.
// The scratch importer must therefore be unique PER PROCESS, not merely
// per imported specifier: two processes importing the same specifier
// concurrently must never write the same file, and one process's cleanup
// must never delete a sibling process's still-in-use files. Minted once
// per process at module load, never reused across a process boundary.
const PROCESS_SCRATCH_ID = `${process.pid}-${randomUUID()}`;

/**
 * Import `specifier` as an ES module located in `baseDir` would: a scratch
 * importer under `baseDir` so Node's real ESM resolution applies. Returns
 * the live namespace, never a mock.
 *
 * @returns {Promise<{ ns: object } | { error: string, kind: "unresolved"|"load-failed" }>}
 */
async function importNamespace(baseDir, specifier) {
  const key = `${baseDir}\0${specifier}`;
  if (namespaceCache.has(key)) return namespaceCache.get(key);
  const scratchDir = path.join(baseDir, ".link-scratch", PROCESS_SCRATCH_ID);
  mkdirSync(scratchDir, { recursive: true });
  const digest = createHash("sha256").update(specifier).digest("hex").slice(0, 16);
  const importerPath = path.join(scratchDir, `ns-${digest}.mjs`);
  writeFileSync(importerPath, `export * as ns from ${JSON.stringify(specifier)};\n`, "utf8");
  let result;
  try {
    const mod = await import(pathToFileURL(importerPath).href);
    result = { ns: mod.ns };
  } catch (error) {
    const message = String(error?.message ?? error);
    const notFound =
      (error?.code === "ERR_MODULE_NOT_FOUND" ||
        error?.code === "ERR_PACKAGE_PATH_NOT_EXPORTED" ||
        error?.code === "MODULE_NOT_FOUND") &&
      message.includes(specifier);
    result = { error: message, kind: notFound ? "unresolved" : "load-failed" };
  }
  namespaceCache.set(key, result);
  return result;
}

export function cleanupLinkScratch(baseDir) {
  namespaceCache.clear();
  // Remove only THIS process's own scratch subdirectory — never the shared
  // `.link-scratch` parent, which sibling processes linking against the
  // same oracle install may still be writing under concurrently.
  rmSync(path.join(baseDir, ".link-scratch", PROCESS_SCRATCH_ID), {
    recursive: true,
    force: true,
  });
}

/** Realized (name, version) of the package a bare specifier resolves to. */
function resolvedPackageIdentity(baseDir, packageName) {
  const require = createRequire(baseDir.endsWith("/") ? baseDir : `${baseDir}/`);
  let manifestPath = null;
  try {
    manifestPath = require.resolve(`${packageName}/package.json`);
  } catch {
    try {
      let dir = path.dirname(require.resolve(packageName));
      while (dir !== path.dirname(dir)) {
        const candidate = path.join(dir, "package.json");
        try {
          const parsed = JSON.parse(readFileSync(candidate, "utf8"));
          if (parsed.name === packageName) {
            manifestPath = candidate;
            break;
          }
        } catch {
          /* keep walking up */
        }
        dir = path.dirname(dir);
      }
    } catch {
      return null;
    }
  }
  if (manifestPath === null) return null;
  const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
  return { name: manifest.name, version: manifest.version };
}

/**
 * Linking-surface validation against the real installed pinned packages —
 * never a mock. Independently reported:
 *
 *  - static import resolution (bare specifiers; relatives are unsupported
 *    here and reported unresolved)
 *  - module-load failure (resolves but throws while evaluating)
 *  - named imports — name must exist on the live ESM export surface
 *  - default imports — module must expose a default binding
 *  - namespace / side-effect imports — module must load
 *  - re-export sources — source must resolve/load; named re-exports must
 *    exist on its surface
 *  - local named exports (`export { x }`) — undeclared `x` is a parse-oracle
 *    early error (acorn checks export references), caught upstream
 *  - exact-package identity — every bare import must resolve to the pinned
 *    version from domain-pin.mjs
 *
 * `specifierOverrides` redirects the export-surface load of a bare
 * specifier to a different entry of the same pinned install (vapor-backend
 * `vue` → with-vapor runtime build; Vue publishes vapor exports only in
 * ESM browser/bundler builds). Exact-package-identity still runs against
 * the original bare specifier.
 *
 * @returns {Promise<{
 *   ok: boolean, resolved: string[], unresolved: string[],
 *   loadFailures: string[], missingExports: string[],
 *   missingDefaults: string[],
 *   packageIdentityViolations: string[], unpinnedPackages: string[],
 * }>}
 */
export async function checkLinkValidity(ast, baseDir, { specifierOverrides } = {}) {
  const resolved = [];
  const unresolved = [];
  const loadFailures = [];
  const missingExports = [];
  const missingDefaults = [];
  const packageIdentityViolations = [];
  const unpinnedPackages = [];
  const checkedPackages = new Set();

  function checkPackageIdentity(specifier) {
    const packageName = barePackageName(specifier);
    if (packageName === null || checkedPackages.has(packageName)) return;
    checkedPackages.add(packageName);
    const pinnedVersion = PINNED_PACKAGE_VERSIONS.get(packageName);
    const identity = resolvedPackageIdentity(baseDir, packageName);
    if (pinnedVersion === undefined) {
      unpinnedPackages.push(packageName);
      return;
    }
    if (identity === null) {
      packageIdentityViolations.push(`${packageName}: resolved package identity unreadable`);
      return;
    }
    if (identity.name !== packageName || identity.version !== pinnedVersion) {
      packageIdentityViolations.push(
        `${packageName}: resolved ${identity.name}@${identity.version}, pinned ${packageName}@${pinnedVersion}`,
      );
    }
  }

  async function loadSurface(specifier) {
    const target = specifierOverrides?.get(specifier) ?? specifier;
    const result = await importNamespace(baseDir, target);
    if (result.ns) {
      resolved.push(specifier);
      checkPackageIdentity(specifier);
      return result.ns;
    }
    if (result.kind === "unresolved") unresolved.push(specifier);
    else loadFailures.push(`${specifier}: ${result.error}`);
    return null;
  }

  for (const stmt of ast.body) {
    if (stmt.type === "ImportDeclaration") {
      const specifier = stmt.source.value;
      const ns = await loadSurface(specifier);
      if (ns === null) continue;
      for (const spec of stmt.specifiers) {
        if (spec.type === "ImportSpecifier") {
          const name = spec.imported.name ?? spec.imported.value;
          if (!Object.prototype.hasOwnProperty.call(ns, name))
            missingExports.push(`${specifier}#${name}`);
        } else if (spec.type === "ImportDefaultSpecifier") {
          if (!Object.prototype.hasOwnProperty.call(ns, "default")) missingDefaults.push(specifier);
        }
        // ImportNamespaceSpecifier: load success above is the whole check.
      }
    } else if (stmt.type === "ExportNamedDeclaration" && stmt.source) {
      const specifier = stmt.source.value;
      const ns = await loadSurface(specifier);
      if (ns === null) continue;
      for (const spec of stmt.specifiers ?? []) {
        const name = spec.local.name ?? spec.local.value;
        if (!Object.prototype.hasOwnProperty.call(ns, name))
          missingExports.push(`${specifier}#${name}`);
      }
    } else if (stmt.type === "ExportAllDeclaration") {
      await loadSurface(stmt.source.value);
    }
  }

  return {
    ok:
      unresolved.length === 0 &&
      loadFailures.length === 0 &&
      missingExports.length === 0 &&
      missingDefaults.length === 0 &&
      packageIdentityViolations.length === 0 &&
      unpinnedPackages.length === 0,
    resolved,
    unresolved,
    loadFailures,
    missingExports,
    missingDefaults,
    packageIdentityViolations,
    unpinnedPackages,
  };
}

// ── Local-binding matching (comparison-time) ──────────────────────────
//
// A module-local binding's spelling is not program behaviour: renaming it
// consistently — the declaration and every use resolved to it — yields the
// same program when the old name was unobservable and the new one captures
// nothing. `matchLocalBindings` resolves every identifier of ONE module on
// its own ESTree scope chain (ECMAScript module scoping: module, function
// parameter/body, block, loop head, switch, catch, class, static block,
// named-function-expression scopes; `var` hoisting; parameter merging) and
// returns a copy of the AST in which every occurrence of a matchable binding
// is spelled `#localN`, N numbering bindings in traversal order. `#` cannot
// start an identifier, so a canonical name never collides with a verbatim
// one. The normalizer then compares the two copies: equal trees mean the
// modules are the same program up to a capture-free renaming. Each side is
// resolved independently, so a rename that re-binds a use to a different
// declaration (shadow capture) changes the `#localN` that use carries and
// stays a structural difference. Nothing is ever renamed by text.
//
// Verbatim (observable or not local):
//   - free identifiers (globals, `arguments`), property keys, member names,
//     labels, imported/exported names and module paths;
//   - bindings declared by an `export` declaration (the export name);
//   - function/class declaration ids and named function/class expression
//     ids (`Function.prototype.name`);
//   - a binding that receives an anonymous function or class through named
//     evaluation (`const f = () => {}`, `f = function () {}`,
//     `({ f = class {} } = o)`) — the binding name becomes the value's `.name`;
//   - every binding of a module with a direct `eval(...)` call (dynamic
//     evaluation can reach any name in scope).
// Import aliases are local bindings: `import { a as x }` matches
// `import { a as y }` when `x` and `y` are used alike; the imported name and
// source stay structural. Named import specifiers are numbered in
// imported-name order so specifier permutation stays cosmetic. `{ a }` and
// `{ a: a }` are the same property, so the shorthand flag is dropped. An
// unrecognised node type, or an Annex B `var` re-declaring a catch parameter,
// disables matching for that module — every name then compares verbatim
// (fail closed). Engine error-message text (a TDZ `ReferenceError` naming the
// binding) and `Function.prototype.toString` source text are not treated as
// name observations, as the normalizer already ignores layout and prose
// comments that `toString` exposes too.

const LOCAL_NAME_PREFIX = "#local";

/** Node types whose Identifier children are all references. */
const REFERENCE_CHILD_TYPES = new Set([
  "ExpressionStatement",
  "ReturnStatement",
  "IfStatement",
  "WhileStatement",
  "DoWhileStatement",
  "ThrowStatement",
  "TryStatement",
  "ArrayExpression",
  "ObjectExpression",
  "SpreadElement",
  "UnaryExpression",
  "UpdateExpression",
  "BinaryExpression",
  "LogicalExpression",
  "ConditionalExpression",
  "NewExpression",
  "SequenceExpression",
  "TemplateLiteral",
  "TaggedTemplateExpression",
  "YieldExpression",
  "AwaitExpression",
  "ChainExpression",
  "ImportExpression",
  "ParenthesizedExpression",
]);

/** Node types with no identifier that could name a local binding. */
const NAMELESS_TYPES = new Set([
  "Literal",
  "ThisExpression",
  "Super",
  "TemplateElement",
  "PrivateIdentifier",
  "MetaProperty",
  "EmptyStatement",
  "DebuggerStatement",
  "BreakStatement",
  "ContinueStatement",
  "ExportAllDeclaration",
]);

/** Assignment operators that apply named evaluation to an anonymous function. */
const NAMED_EVALUATION_OPERATORS = new Set(["=", "&&=", "||=", "??="]);

function isAnonymousFunctionDefinition(node) {
  return (
    node.type === "ArrowFunctionExpression" ||
    ((node.type === "FunctionExpression" || node.type === "ClassExpression") && !node.id)
  );
}

function patternIdentifiers(pattern, out = []) {
  switch (pattern.type) {
    case "Identifier":
      out.push(pattern);
      break;
    case "ObjectPattern":
      for (const property of pattern.properties)
        patternIdentifiers(
          property.type === "RestElement" ? property.argument : property.value,
          out,
        );
      break;
    case "ArrayPattern":
      for (const element of pattern.elements)
        if (element !== null) patternIdentifiers(element, out);
      break;
    case "AssignmentPattern":
      patternIdentifiers(pattern.left, out);
      break;
    case "RestElement":
      patternIdentifiers(pattern.argument, out);
      break;
  }
  return out;
}

/**
 * Copies an AST with every node object unshared (acorn reuses one node for
 * `import { a }`'s imported/local and `export { a }`'s local/exported, which
 * play different roles here). Non-node values are kept by reference.
 */
function cloneTree(value) {
  if (Array.isArray(value)) return value.map(cloneTree);
  if (value === null || typeof value !== "object" || typeof value.type !== "string") return value;
  const out = {};
  for (const [key, child] of Object.entries(value))
    out[key] = key === "loc" ? child : cloneTree(child);
  if (out.type === "Property") out.shorthand = false;
  return out;
}

/**
 * Resolves every identifier of `program` to the module-local binding it
 * names. Returns `Map<Identifier, binding>` where a binding is
 * `{ observable, occurrences }`, or null when matching must not apply.
 */
function resolveLocalBindings(program) {
  const bindingOf = new Map();
  const references = [];
  const nameExposures = [];
  const evalCallees = [];
  let supported = true;

  const newScope = (parent, { varScope = false, params = null } = {}) => ({
    parent,
    bindings: new Map(),
    varScope,
    params,
  });
  const bind = (binding, id) => {
    binding.occurrences.push(id);
    bindingOf.set(id, binding);
  };
  const declareLexical = (scope, id) => {
    let binding = scope.bindings.get(id.name);
    if (binding === undefined) {
      binding = { observable: false, occurrences: [] };
      scope.bindings.set(id.name, binding);
    }
    bind(binding, id);
  };
  // `var` (and a function declaration at function/module top level) binds
  // in the nearest var scope, sharing the binding of a same-named parameter.
  const declareVar = (scope, id) => {
    let target = scope;
    while (!target.varScope) {
      if (target.bindings.has(id.name)) supported = false; // Annex B catch-parameter `var`
      target = target.parent;
    }
    const binding = target.bindings.get(id.name) ??
      target.params?.bindings.get(id.name) ?? { observable: false, occurrences: [] };
    target.bindings.set(id.name, binding);
    bind(binding, id);
  };

  function walkPattern(node, scope, declare) {
    switch (node.type) {
      case "Identifier":
        if (declare === null) references.push({ id: node, scope });
        else declare(scope, node);
        return;
      case "ObjectPattern":
        for (const property of node.properties) {
          if (property.type === "RestElement") {
            walkPattern(property.argument, scope, declare);
          } else {
            if (property.computed) walk(property.key, scope);
            walkPattern(property.value, scope, declare);
          }
        }
        return;
      case "ArrayPattern":
        for (const element of node.elements)
          if (element !== null) walkPattern(element, scope, declare);
        return;
      case "AssignmentPattern":
        walkPattern(node.left, scope, declare);
        walk(node.right, scope);
        if (node.left.type === "Identifier" && isAnonymousFunctionDefinition(node.right))
          nameExposures.push(node.left);
        return;
      case "RestElement":
        walkPattern(node.argument, scope, declare);
        return;
      case "MemberExpression":
        walk(node, scope);
        return;
      default:
        supported = false;
    }
  }

  function walkFunction(node, scope) {
    const params = newScope(scope);
    for (const param of node.params) walkPattern(param, params, declareLexical);
    // Parameter defaults resolve in `params` and never see body declarations.
    const body = newScope(params, { varScope: true, params });
    if (node.body.type === "BlockStatement")
      for (const statement of node.body.body) walk(statement, body);
    else walk(node.body, body);
  }

  function walkClass(node, scope, idBindsInner) {
    const inner = newScope(scope);
    if (node.id) {
      const binding = { observable: true, occurrences: [] };
      inner.bindings.set(node.id.name, binding);
      if (idBindsInner) bind(binding, node.id);
    }
    if (node.superClass) walk(node.superClass, inner);
    for (const element of node.body.body) {
      if (element.type === "MethodDefinition" || element.type === "PropertyDefinition") {
        if (element.computed) walk(element.key, inner);
        if (element.value) walk(element.value, inner);
      } else if (element.type === "StaticBlock") {
        const block = newScope(inner, { varScope: true });
        for (const statement of element.body) walk(statement, block);
      } else {
        supported = false;
      }
    }
  }

  function walk(node, scope) {
    switch (node.type) {
      case "Identifier":
        references.push({ id: node, scope });
        return;
      case "VariableDeclaration":
        for (const declarator of node.declarations) {
          walkPattern(declarator.id, scope, node.kind === "var" ? declareVar : declareLexical);
          if (declarator.init) {
            walk(declarator.init, scope);
            if (
              declarator.id.type === "Identifier" &&
              isAnonymousFunctionDefinition(declarator.init)
            )
              nameExposures.push(declarator.id);
          }
        }
        return;
      case "FunctionDeclaration":
        if (node.id) {
          (scope.varScope ? declareVar : declareLexical)(scope, node.id);
          nameExposures.push(node.id);
        }
        walkFunction(node, scope);
        return;
      case "FunctionExpression": {
        let outer = scope;
        if (node.id) {
          outer = newScope(scope);
          declareLexical(outer, node.id);
          nameExposures.push(node.id);
        }
        walkFunction(node, outer);
        return;
      }
      case "ArrowFunctionExpression":
        walkFunction(node, scope);
        return;
      case "ClassDeclaration":
        if (node.id) {
          declareLexical(scope, node.id);
          nameExposures.push(node.id);
        }
        walkClass(node, scope, false);
        return;
      case "ClassExpression":
        walkClass(node, scope, true);
        return;
      case "BlockStatement": {
        const block = newScope(scope);
        for (const statement of node.body) walk(statement, block);
        return;
      }
      case "ForStatement": {
        const loop = newScope(scope);
        for (const part of [node.init, node.test, node.update]) if (part) walk(part, loop);
        walk(node.body, loop);
        return;
      }
      case "ForInStatement":
      case "ForOfStatement": {
        const loop = newScope(scope);
        if (node.left.type === "VariableDeclaration") walk(node.left, loop);
        else walkPattern(node.left, loop, null);
        walk(node.right, loop);
        walk(node.body, loop);
        return;
      }
      case "SwitchStatement": {
        walk(node.discriminant, scope);
        const cases = newScope(scope);
        for (const switchCase of node.cases) {
          if (switchCase.test) walk(switchCase.test, cases);
          for (const statement of switchCase.consequent) walk(statement, cases);
        }
        return;
      }
      case "CatchClause": {
        const handler = newScope(scope);
        if (node.param) walkPattern(node.param, handler, declareLexical);
        walk(node.body, handler);
        return;
      }
      case "LabeledStatement":
        walk(node.body, scope);
        return;
      case "ImportDeclaration":
        for (const specifier of node.specifiers) declareLexical(scope, specifier.local);
        return;
      case "ExportNamedDeclaration": {
        const declaration = node.declaration;
        if (declaration) {
          walk(declaration, scope);
          if (declaration.type === "VariableDeclaration") {
            for (const declarator of declaration.declarations)
              nameExposures.push(...patternIdentifiers(declarator.id));
          } else if (declaration.id) {
            nameExposures.push(declaration.id);
          }
        } else if (node.source === null) {
          for (const specifier of node.specifiers)
            if (specifier.local.type === "Identifier")
              references.push({ id: specifier.local, scope });
        }
        return;
      }
      case "ExportDefaultDeclaration":
        walk(node.declaration, scope);
        return;
      case "Property":
        if (node.computed) walk(node.key, scope);
        walk(node.value, scope);
        return;
      case "MemberExpression":
        walk(node.object, scope);
        if (node.computed) walk(node.property, scope);
        return;
      case "AssignmentExpression":
        walkPattern(node.left, scope, null);
        walk(node.right, scope);
        if (
          node.left.type === "Identifier" &&
          NAMED_EVALUATION_OPERATORS.has(node.operator) &&
          isAnonymousFunctionDefinition(node.right)
        )
          nameExposures.push(node.left);
        return;
      case "CallExpression":
        if (node.callee.type === "Identifier" && node.callee.name === "eval")
          evalCallees.push(node.callee);
        walk(node.callee, scope);
        for (const argument of node.arguments) walk(argument, scope);
        return;
      default:
        if (NAMELESS_TYPES.has(node.type)) return;
        if (!REFERENCE_CHILD_TYPES.has(node.type)) {
          supported = false;
          return;
        }
        for (const [key, child] of Object.entries(node)) {
          if (key === "loc") continue;
          for (const item of Array.isArray(child) ? child : [child])
            if (item !== null && typeof item === "object" && typeof item.type === "string")
              walk(item, scope);
        }
    }
  }

  const moduleScope = newScope(null, { varScope: true });
  for (const statement of program.body) walk(statement, moduleScope);
  for (const { id, scope } of references) {
    for (let current = scope; current !== null; current = current.parent) {
      const binding = current.bindings.get(id.name);
      if (binding !== undefined) {
        bind(binding, id);
        break;
      }
    }
  }
  for (const id of nameExposures) {
    const binding = bindingOf.get(id);
    if (binding !== undefined) binding.observable = true;
  }
  // A direct eval is an `eval` call resolving to no local binding.
  if (evalCallees.some((callee) => !bindingOf.has(callee))) supported = false;
  return supported ? bindingOf : null;
}

/** Named import specifiers in imported-name order (stable), after default/namespace. */
function importNumberingOrder(specifiers) {
  const importedName = (specifier) => specifier.imported.name ?? specifier.imported.value;
  const named = specifiers
    .filter((specifier) => specifier.type === "ImportSpecifier")
    .sort((a, b) => {
      const nameA = importedName(a);
      const nameB = importedName(b);
      return nameA < nameB ? -1 : nameA > nameB ? 1 : 0;
    });
  return [...specifiers.filter((specifier) => specifier.type !== "ImportSpecifier"), ...named];
}

/**
 * The comparison form of one module: a copy of `ast` whose matchable local
 * bindings are spelled `#localN` (see the section comment).
 *
 * @returns {{ ast: object, applied: boolean }} `applied` is false when
 *   matching was disabled and every name stays verbatim
 */
export function matchLocalBindings(ast) {
  const copy = cloneTree(ast);
  Object.defineProperty(copy, "sourceComments", { value: ast.sourceComments, enumerable: false });
  const bindingOf = resolveLocalBindings(copy);
  if (bindingOf === null) return { ast: copy, applied: false };
  const numbers = new Map();
  const visit = (value) => {
    if (Array.isArray(value)) {
      for (const item of value) visit(item);
      return;
    }
    if (value === null || typeof value !== "object" || typeof value.type !== "string") return;
    if (value.type === "Identifier") {
      const binding = bindingOf.get(value);
      if (binding !== undefined && !binding.observable && !numbers.has(binding))
        numbers.set(binding, numbers.size);
      return;
    }
    for (const [key, child] of Object.entries(value)) {
      if (key === "loc") continue;
      visit(
        key === "specifiers" && value.type === "ImportDeclaration"
          ? importNumberingOrder(child)
          : child,
      );
    }
  };
  visit(copy);
  for (const [binding, number] of numbers)
    for (const id of binding.occurrences) id.name = `${LOCAL_NAME_PREFIX}${number}`;
  return { ast: copy, applied: true };
}

/**
 * Output-fidelity comparison: normalizer canonical forms of both modules
 * after local-binding matching.
 *
 * @returns {{
 *   equal: boolean, goldenDigest: string, candidateDigest: string,
 *   firstDivergence: string|null,
 *   localBindingMatching: { golden: boolean, candidate: boolean },
 * }}
 */
export function compareStructural(goldenAst, candidateAst) {
  const goldenMatched = matchLocalBindings(goldenAst);
  const candidateMatched = matchLocalBindings(candidateAst);
  const golden = canonicalize(goldenMatched.ast);
  const candidate = canonicalize(candidateMatched.ast);
  const equal = deepEqualCanonical(golden.tree, candidate.tree);
  return {
    equal,
    goldenDigest: canonicalDigest(golden.tree),
    candidateDigest: canonicalDigest(candidate.tree),
    firstDivergence: equal ? null : firstDivergencePath(golden.tree, candidate.tree),
    localBindingMatching: { golden: goldenMatched.applied, candidate: candidateMatched.applied },
  };
}

/** Best-effort structural-diff pointer for failure reports — not itself an oracle. */
function firstDivergencePath(a, b, path = "$") {
  if (a === b) return null;
  if (typeof a !== typeof b || a === null || b === null) return `${path}: type/nullness differs`;
  if (Array.isArray(a) !== Array.isArray(b)) return `${path}: array-shape differs`;
  if (Array.isArray(a)) {
    if (a.length !== b.length) return `${path}: length ${a.length} vs ${b.length}`;
    for (let i = 0; i < a.length; i += 1) {
      const sub = firstDivergencePath(a[i], b[i], `${path}[${i}]`);
      if (sub) return sub;
    }
    return null;
  }
  if (typeof a === "object") {
    const keys = new Set([...Object.keys(a), ...Object.keys(b)]);
    for (const key of keys) {
      const sub = firstDivergencePath(a[key], b[key], `${path}.${key}`);
      if (sub) return sub;
    }
    return null;
  }
  return `${path}: ${JSON.stringify(a)} vs ${JSON.stringify(b)}`;
}

/** Canonical position: {line, column} with absent members normalized to null. */
function canonicalPosition(position) {
  if (position === null || position === undefined) return null;
  return { line: position.line ?? null, column: position.column ?? null };
}

/**
 * Full message chain, flattened in order. Accepts a plain string, an array
 * chain, or a nested `{ message, next }` chain (the TypeScript
 * DiagnosticMessageChain shape) — every link enters the comparison.
 */
function canonicalMessageChain(message) {
  if (message === null || message === undefined) return [];
  if (typeof message === "string") return [message];
  if (Array.isArray(message)) return message.flatMap(canonicalMessageChain);
  if (typeof message === "object") {
    const head = typeof message.messageText === "string" ? message.messageText : message.message;
    return [
      ...(head === undefined ? [] : [String(head)]),
      ...canonicalMessageChain(message.next ?? []),
    ];
  }
  return [String(message)];
}

function canonicalRelated(related) {
  return {
    message: canonicalMessageChain(related.message ?? related.messageText),
    source: related.source ?? related.file ?? null,
    start: canonicalPosition(related.start),
    end: canonicalPosition(related.end),
  };
}

/**
 * The canonical, fully-discriminating diagnostic record. EVERY
 * contract-observable field participates: category/kind, code, the FULL
 * message chain, source/file identity, start AND end spans, related
 * information, and (by positional array comparison) order and count.
 */
export function canonicalDiagnostic(diagnostic) {
  return {
    kind: diagnostic.kind ?? null,
    code: diagnostic.code ?? null,
    message: canonicalMessageChain(diagnostic.message),
    source: diagnostic.source ?? diagnostic.file ?? null,
    start: canonicalPosition(diagnostic.start),
    end: canonicalPosition(diagnostic.end),
    related: (diagnostic.related ?? diagnostic.relatedInformation ?? []).map(canonicalRelated),
  };
}

const DIAGNOSTIC_FIELDS = ["kind", "code", "message", "source", "start", "end", "related"];

/**
 * Ordered, full-field diagnostic comparison. Two sequences are equal only
 * when they have the same length and every diagnostic matches on every
 * canonical field at the same position. `firstMismatch` names the index and
 * the exact fields that differ (or `count` when the lengths differ), so a
 * diagnostic matching on every field but one is always caught and
 * attributable.
 */
export function compareDiagnostics(goldenDiagnostics, candidateDiagnostics) {
  const golden = goldenDiagnostics.map(canonicalDiagnostic);
  const candidate = candidateDiagnostics.map(canonicalDiagnostic);
  let firstMismatch = null;
  if (golden.length !== candidate.length) {
    firstMismatch = { index: Math.min(golden.length, candidate.length), fields: ["count"] };
  } else {
    for (let i = 0; i < golden.length; i += 1) {
      const fields = DIAGNOSTIC_FIELDS.filter(
        (field) => !deepEqualCanonical(golden[i][field], candidate[i][field]),
      );
      if (fields.length > 0) {
        firstMismatch = { index: i, fields };
        break;
      }
    }
  }
  return {
    equal: firstMismatch === null,
    firstMismatch,
    goldenCount: golden.length,
    candidateCount: candidate.length,
    golden,
    candidate,
  };
}

/** The observable part of one runtime executor result. */
function runtimeObservation(run) {
  return { html: run.html ?? null, steps: run.steps ?? null };
}

/**
 * Executes both arms through `execute` and compares what the runtime
 * observed (rendered HTML and/or per-step update observations), plus
 * candidate-only runtime warnings. Independent of structural similarity.
 */
async function compareRuntime(execute, goldenCode, candidateCode) {
  if (candidateCode === null || candidateCode === undefined) {
    return {
      status: "fail",
      reasons: ["runtime divergence: candidate produced no executable code"],
    };
  }
  const reasons = [];
  const goldenRun = await execute(goldenCode);
  const candidateRun = await execute(candidateCode);
  if (!goldenRun.ok)
    reasons.push(`runtime divergence: golden failed to execute: ${goldenRun.error}`);
  if (!candidateRun.ok)
    reasons.push(`runtime divergence: candidate failed to execute: ${candidateRun.error}`);
  if (goldenRun.ok && candidateRun.ok) {
    const goldenObserved = runtimeObservation(goldenRun);
    const candidateObserved = runtimeObservation(candidateRun);
    if (!deepEqualCanonical(goldenObserved, candidateObserved)) {
      reasons.push(
        `runtime divergence: observed output differs at ${firstDivergencePath(goldenObserved, candidateObserved)}`,
      );
    }
    const candidateWarnings = candidateRun.warnings ?? [];
    if (candidateWarnings.length > 0 && (goldenRun.warnings ?? []).length === 0) {
      reasons.push(
        `runtime divergence: candidate produced runtime warnings the golden does not: ${JSON.stringify(candidateWarnings)}`,
      );
    }
  }
  return { status: reasons.length === 0 ? "pass" : "fail", reasons };
}

/**
 * Full comparison report combining every independent oracle. `structural`
 * is only computed when both arms parse validly — a normalizer pass never
 * runs over, and can never mask, a parse failure.
 *
 * `axes` records, per independent oracle, whether it genuinely RAN or was
 * SKIPPED (with the reason). Default behavior is unchanged: a skipped axis
 * is informational. Under `authoritative: true` — the fail-closed mode a
 * consumer opts into to prove every axis genuinely executed — any skipped
 * axis becomes a hard failure reason instead of a silent narrowing.
 *
 * Behaviour and official-output similarity are reported separately:
 *
 *  - `behavior` — runtime correctness. With an `execute` executor (one of the
 *    pinned-runtime executors, e.g. `executeVueSsr`, `executeSvelteSsr`, or
 *    an update/cleanup driver over `executeVueClientInteractions`; it must
 *    resolve to `{ ok, error?, warnings?, html?, steps? }`), both arms run and
 *    `status` is `pass` or `fail`, with `reasons` naming each runtime
 *    divergence. Without one, `status` is `unrun` — never inferred from a
 *    structural match.
 *  - `fidelity` — output similarity: `equivalent` when the modules are the
 *    same after cosmetic normalization and local-binding matching,
 *    `divergent` (with `firstDivergence`) otherwise, `unrun` when an arm
 *    failed to parse. A divergence is an output-fidelity finding; it never
 *    alters `behavior`.
 *
 * `verdict`/`reasons` remain the all-axis aggregate: any runtime,
 * structural, parse, link, diagnostic or mapping failure fails it.
 */
export async function compareArtifacts(
  golden,
  candidate,
  { linkBaseDir, authoritative, linkSpecifierOverrides, mappingContext, execute } = {},
) {
  const reasons = [];
  const axes = {
    parse: { status: "ran", reason: null },
    link: { status: "ran", reason: null },
    structural: { status: "ran", reason: null },
    diagnostics: { status: "ran", reason: null },
    mapping: { status: "ran", reason: null },
  };
  const goldenParse = checkParseValidity(golden.code, "golden");
  const candidateParse = checkParseValidity(candidate.code, "candidate");
  if (!goldenParse.ok) reasons.push(`golden failed to parse: ${goldenParse.error}`);
  if (!candidateParse.ok) reasons.push(`candidate failed to parse: ${candidateParse.error}`);

  let link = null;
  if (!linkBaseDir) {
    axes.link = { status: "skipped", reason: "no linkBaseDir supplied" };
  } else if (!candidateParse.ok) {
    axes.link = { status: "skipped", reason: "candidate failed to parse" };
  }
  if (candidateParse.ok && linkBaseDir) {
    link = await checkLinkValidity(candidateParse.ast, linkBaseDir, {
      specifierOverrides: linkSpecifierOverrides,
    });
    if (link.unresolved.length > 0)
      reasons.push(`candidate has unresolved imports: ${link.unresolved.join(", ")}`);
    if (link.loadFailures.length > 0)
      reasons.push(`candidate imports fail to load: ${link.loadFailures.join("; ")}`);
    if (link.missingExports.length > 0)
      reasons.push(`candidate imports missing named exports: ${link.missingExports.join(", ")}`);
    if (link.missingDefaults.length > 0)
      reasons.push(
        `candidate default-imports modules without a default export: ${link.missingDefaults.join(", ")}`,
      );
    if (link.packageIdentityViolations.length > 0)
      reasons.push(
        `candidate resolves wrong package identities: ${link.packageIdentityViolations.join("; ")}`,
      );
    if (link.unpinnedPackages.length > 0)
      reasons.push(
        `candidate imports packages outside the pinned closures: ${link.unpinnedPackages.join(", ")}`,
      );
  }

  let structural = null;
  if (goldenParse.ok && candidateParse.ok) {
    structural = compareStructural(goldenParse.ast, candidateParse.ast);
    if (!structural.equal) reasons.push(`structural divergence at ${structural.firstDivergence}`);
  } else {
    axes.structural = { status: "skipped", reason: "an arm failed to parse" };
  }

  const diagnostics = compareDiagnostics(golden.diagnostics ?? [], candidate.diagnostics ?? []);
  if (!diagnostics.equal) {
    reasons.push(
      `diagnostics diverge (index ${diagnostics.firstMismatch.index}: ${diagnostics.firstMismatch.fields.join(", ")})`,
    );
  }

  // The mapping axis is SELF-REFERENTIAL: the candidate's map is validated
  // against the candidate's OWN generated code and the authored fixture, and
  // the golden's map is not an input. See mapping-oracle.mjs for why a
  // candidate-vs-official map comparison cannot be the oracle here.
  let mapping = null;
  if (!mappingContext) {
    axes.mapping = {
      status: "skipped",
      reason: "no authored-source mapping context supplied",
    };
  } else {
    mapping = validateAuthoredMapping({
      ...mappingContext,
      code: candidate.code ?? null,
      map: candidate.map ?? null,
    });
    if (!mapping.ok) {
      reasons.push(
        `candidate source map is not truthful about its own output: ${mapping.violations
          .map((violation) => `${violation.rule} — ${violation.detail}`)
          .join("; ")}`,
      );
    }
  }

  const behavior = execute
    ? await compareRuntime(execute, golden.code, candidate.code)
    : { status: "unrun", reasons: [], reason: "no runtime executor supplied" };
  reasons.push(...behavior.reasons);

  if (authoritative) {
    for (const [axis, state] of Object.entries(axes)) {
      if (state.status === "skipped") {
        reasons.push(`authoritative mode: ${axis} axis skipped (${state.reason})`);
      }
    }
  }

  return {
    verdict: reasons.length === 0 ? "pass" : "fail",
    reasons,
    axes,
    goldenParse: { ok: goldenParse.ok, error: goldenParse.ok ? null : goldenParse.error },
    candidateParse: {
      ok: candidateParse.ok,
      error: candidateParse.ok ? null : candidateParse.error,
    },
    link,
    structural,
    diagnostics,
    mapping,
    behavior,
    fidelity:
      structural === null
        ? { status: "unrun", firstDivergence: null }
        : {
            status: structural.equal ? "equivalent" : "divergent",
            firstDivergence: structural.firstDivergence,
          },
  };
}

/**
 * Architectural walker enforcing the compat layer's native session call surface.
 *
 * This walker uses the TypeScript compiler API to inspect TS sources and report
 * violations of three rules:
 *
 *   1. Member calls of the form `<...>._session.<method>(...)` must use a
 *      method name in {@link ALLOWED_NATIVE_SESSION_METHODS}.
 *   2. Member accesses of the form `<...>._session.<prop>` that are read-only
 *      (i.e. not the LHS of an assignment) must use a property name in
 *      {@link ALLOWED_NATIVE_SESSION_PROPERTY_READS}.
 *   3. Member accesses of the form `<...>._session.<prop>` that are the LHS of
 *      an assignment (`=`, `+=`, `++`, etc.) are forbidden — the compat layer
 *      may not mutate session-side properties.
 *
 * It also enforces:
 *   4. Imports of modules matching {@link NATIVE_MODULE_GLOBS} must be
 *      named-symbol imports — namespace imports (`import * as foo from ...`)
 *      are forbidden.
 *
 * The walker is intentionally conservative: it gates strictly on the literal
 * identifier `_session`. This matches the call-site convention used in
 * `packages/component-meta/src/compat/`, where `this._session` is the only
 * gateway to the underlying `ProjectSession`. Aliasing the field through a
 * local variable is an explicit escape hatch and is therefore not gated.
 *
 * Authority:
 *   - D24 — Compat allow-list = TS compiler API member-call walker.
 *   - D35 — Allow-list contents.
 *   - D58 — `NATIVE_MODULE_GLOBS`.
 */

import * as fs from "node:fs";
import * as path from "node:path";
import * as ts from "typescript";

/**
 * Methods callable on `_session.*` — every other method is forbidden.
 * @internal
 */
export const ALLOWED_NATIVE_SESSION_METHODS: readonly string[] = [
  "getComponentMeta",
  "getEffectiveSource",
  "delete",
  "restoreBaseFile",
  "refreshBaseFile",
  "ensureBaseFile",
];

/**
 * Properties readable on `_session.*` — every other property read is forbidden.
 * @internal
 */
export const ALLOWED_NATIVE_SESSION_PROPERTY_READS: readonly string[] = ["engine"];

/**
 * Module specifiers that, when imported, must use named-symbol form only.
 * The first two entries are exact (`@verter/native`) and prefix-glob
 * (`@verter/native-*`). The third (`../native`) is exact-match only —
 * sibling files like `../native-component-meta.js` do NOT match because the
 * specifier carries additional path segments.
 * @internal
 */
export const NATIVE_MODULE_GLOBS: readonly string[] = [
  "@verter/native",
  "@verter/native-*",
  "../native",
];

const ALLOWED_METHOD_SET = new Set(ALLOWED_NATIVE_SESSION_METHODS);
const ALLOWED_PROPERTY_READ_SET = new Set(ALLOWED_NATIVE_SESSION_PROPERTY_READS);

export type WalkerRule =
  | "native-session-method-allowlist"
  | "native-session-property-read-allowlist"
  | "native-session-no-property-write"
  | "native-module-no-namespace-import";

export interface WalkerViolation {
  rule: WalkerRule;
  file: string;
  line: number;
  column: number;
  detail: string;
}

export interface WalkSourceInput {
  fileName: string;
  source: string;
}

/**
 * Walk a single TS source string and return its violations.
 *
 * The source is parsed with `ScriptKind.TS` regardless of the supplied file
 * name extension — this keeps fixture invocations independent of disk state
 * and makes the walker's behavior depend only on the source text and walker
 * rules.
 */
export function walkSourceForViolations(input: WalkSourceInput): WalkerViolation[] {
  const sourceFile = ts.createSourceFile(
    input.fileName,
    input.source,
    ts.ScriptTarget.Latest,
    /*setParentNodes*/ true,
    ts.ScriptKind.TS,
  );
  return walkSourceFile(sourceFile);
}

/**
 * Walk every `.ts` file under {@link compatDir}, returning all violations.
 * `.spec.ts`, `.test.ts`, and files inside an `__arch__/` subtree are skipped:
 * spec/test files exist purely to characterize behavior (they may
 * intentionally contain near-violations), and the walker itself lives under
 * `__arch__/` so we don't gate the gate on itself.
 */
export function walkCompatLayerForViolations(compatDir: string): WalkerViolation[] {
  const violations: WalkerViolation[] = [];
  for (const file of collectCompatTsFiles(compatDir)) {
    const text = fs.readFileSync(file, "utf8");
    const sourceFile = ts.createSourceFile(
      file,
      text,
      ts.ScriptTarget.Latest,
      /*setParentNodes*/ true,
      ts.ScriptKind.TS,
    );
    violations.push(...walkSourceFile(sourceFile));
  }
  return violations;
}

// ---------------------------------------------------------------------------
// Implementation
// ---------------------------------------------------------------------------

function collectCompatTsFiles(root: string): string[] {
  const out: string[] = [];
  const stack: string[] = [root];
  while (stack.length > 0) {
    const dir = stack.pop()!;
    if (!fs.existsSync(dir)) continue;
    for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
      const full = path.join(dir, entry.name);
      if (entry.isDirectory()) {
        if (entry.name === "__arch__") continue;
        stack.push(full);
        continue;
      }
      if (!entry.isFile()) continue;
      if (!entry.name.endsWith(".ts")) continue;
      if (entry.name.endsWith(".spec.ts")) continue;
      if (entry.name.endsWith(".test.ts")) continue;
      out.push(full);
    }
  }
  return out;
}

function walkSourceFile(sourceFile: ts.SourceFile): WalkerViolation[] {
  const violations: WalkerViolation[] = [];

  const visit = (node: ts.Node): void => {
    // Rule 4 — imports of native modules must be named-symbol form.
    if (ts.isImportDeclaration(node)) {
      checkImportDeclaration(node, sourceFile, violations);
    }

    // Rules 1-3 are driven by `_session` accesses.
    if (ts.isPropertyAccessExpression(node) && isUnderscoreSessionAccess(node)) {
      classifySessionAccess(node, sourceFile, violations);
    }

    ts.forEachChild(node, visit);
  };

  visit(sourceFile);
  return violations;
}

/**
 * True iff `node` accesses a member off an expression named `_session`.
 *
 * We accept any expression `<lhs>._session.<member>` (e.g. `this._session.foo`,
 * `that._session.foo`). The classifier later determines whether the access is
 * a method call, property read, or property write.
 */
function isUnderscoreSessionAccess(node: ts.PropertyAccessExpression): boolean {
  // node.expression is the part LEFT of the dot whose name is `node.name`.
  // For `this._session.foo`, node.expression is `this._session` (another
  // PropertyAccessExpression whose .name.escapedText === "_session").
  const lhs = node.expression;
  if (!ts.isPropertyAccessExpression(lhs)) return false;
  return lhs.name.text === "_session";
}

function classifySessionAccess(
  node: ts.PropertyAccessExpression,
  sourceFile: ts.SourceFile,
  violations: WalkerViolation[],
): void {
  const member = node.name.text;
  const parent = node.parent;

  // Distinguish call sites: `<...>._session.foo(...)` (method call).
  // The CallExpression's `expression` is the PropertyAccessExpression.
  if (parent && ts.isCallExpression(parent) && parent.expression === node) {
    if (!ALLOWED_METHOD_SET.has(member)) {
      violations.push(
        makeViolation(
          "native-session-method-allowlist",
          sourceFile,
          node,
          `_session.${member}() is not in the allow-list (${ALLOWED_NATIVE_SESSION_METHODS.join(", ")})`,
        ),
      );
    }
    return;
  }

  // Distinguish writes: assignment with the access on the LHS, OR an update
  // operator like `++`/`--`, OR a delete expression. All are property writes
  // (or write-shaped operations) on `_session.<member>`.
  if (isPropertyWriteTarget(node)) {
    violations.push(
      makeViolation(
        "native-session-no-property-write",
        sourceFile,
        node,
        `_session.${member} = <expr> (or compound write) is forbidden — _session is read-only from compat`,
      ),
    );
    return;
  }

  // Otherwise it's a property read (or a `typeof`-shaped check, indexing, etc).
  if (!ALLOWED_PROPERTY_READ_SET.has(member)) {
    violations.push(
      makeViolation(
        "native-session-property-read-allowlist",
        sourceFile,
        node,
        `_session.${member} is not in the property-read allow-list (${ALLOWED_NATIVE_SESSION_PROPERTY_READS.join(", ")})`,
      ),
    );
  }
}

function isPropertyWriteTarget(node: ts.PropertyAccessExpression): boolean {
  const parent = node.parent;
  if (!parent) return false;

  // `this._session.foo = X`, `+=`, `-=`, etc.
  if (
    ts.isBinaryExpression(parent) &&
    parent.left === node &&
    isAssignmentOperator(parent.operatorToken.kind)
  ) {
    return true;
  }

  // `++this._session.foo`, `this._session.foo++`, etc.
  if (
    (ts.isPrefixUnaryExpression(parent) || ts.isPostfixUnaryExpression(parent)) &&
    parent.operand === node &&
    (parent.operator === ts.SyntaxKind.PlusPlusToken ||
      parent.operator === ts.SyntaxKind.MinusMinusToken)
  ) {
    return true;
  }

  // `delete this._session.foo`
  if (ts.isDeleteExpression(parent) && parent.expression === node) {
    return true;
  }

  return false;
}

function isAssignmentOperator(kind: ts.SyntaxKind): boolean {
  return (
    kind === ts.SyntaxKind.EqualsToken ||
    kind === ts.SyntaxKind.PlusEqualsToken ||
    kind === ts.SyntaxKind.MinusEqualsToken ||
    kind === ts.SyntaxKind.AsteriskEqualsToken ||
    kind === ts.SyntaxKind.AsteriskAsteriskEqualsToken ||
    kind === ts.SyntaxKind.SlashEqualsToken ||
    kind === ts.SyntaxKind.PercentEqualsToken ||
    kind === ts.SyntaxKind.LessThanLessThanEqualsToken ||
    kind === ts.SyntaxKind.GreaterThanGreaterThanEqualsToken ||
    kind === ts.SyntaxKind.GreaterThanGreaterThanGreaterThanEqualsToken ||
    kind === ts.SyntaxKind.AmpersandEqualsToken ||
    kind === ts.SyntaxKind.BarEqualsToken ||
    kind === ts.SyntaxKind.CaretEqualsToken ||
    kind === ts.SyntaxKind.AmpersandAmpersandEqualsToken ||
    kind === ts.SyntaxKind.BarBarEqualsToken ||
    kind === ts.SyntaxKind.QuestionQuestionEqualsToken
  );
}

function checkImportDeclaration(
  node: ts.ImportDeclaration,
  sourceFile: ts.SourceFile,
  violations: WalkerViolation[],
): void {
  // Only string literal specifiers — guard against arbitrary expressions
  // (which the parser shouldn't produce for `import` declarations anyway).
  const specifierNode = node.moduleSpecifier;
  if (!ts.isStringLiteralLike(specifierNode)) return;
  const specifier = specifierNode.text;
  if (!matchesNativeModuleGlob(specifier)) return;

  // `import "./foo"` (side-effect import, no clause) — no namespace, ok.
  const clause = node.importClause;
  if (!clause) return;

  // namedBindings can be NamespaceImport (`* as ns`) or NamedImports (`{a, b}`).
  const named = clause.namedBindings;
  if (named && ts.isNamespaceImport(named)) {
    violations.push(
      makeViolation(
        "native-module-no-namespace-import",
        sourceFile,
        node,
        `namespace import "import * as ${named.name.text} from \"${specifier}\"" is forbidden — use named imports`,
      ),
    );
  }
}

function matchesNativeModuleGlob(specifier: string): boolean {
  for (const glob of NATIVE_MODULE_GLOBS) {
    if (glob.endsWith("*")) {
      const prefix = glob.slice(0, -1);
      if (specifier.startsWith(prefix) && specifier.length > prefix.length) {
        return true;
      }
    } else if (specifier === glob) {
      return true;
    }
  }
  return false;
}

function makeViolation(
  rule: WalkerRule,
  sourceFile: ts.SourceFile,
  node: ts.Node,
  detail: string,
): WalkerViolation {
  const { line, character } = ts.getLineAndCharacterOfPosition(
    sourceFile,
    node.getStart(sourceFile),
  );
  return {
    rule,
    file: sourceFile.fileName,
    line: line + 1,
    column: character + 1,
    detail,
  };
}

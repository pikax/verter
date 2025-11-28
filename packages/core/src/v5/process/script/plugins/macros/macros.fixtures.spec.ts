import * as ts from "typescript";
import { createFixtures } from "./macros.fixtures";
import type {
  Fixture,
  WithPrefixString,
  TypeTest,
} from "../../../../../fixtures/types";
import { resolveWithPrefix } from "../../../../../fixtures/types";
import {
  runTypeTest,
  assertNoTypeErrors as assertNoTypeErrorsShared,
} from "../../../../../fixtures/ts-program";

/**
 * Fixture validation tests for the macros plugin.
 *
 * These tests validate that all fixture transformations:
 * - Generate syntactically valid TypeScript
 * - Generate semantically valid TypeScript (no type errors)
 * - Match expected patterns and type structures
 * - Pass type tests (verify hover types and catch `any`/`unknown`)
 *
 * For visual type inspection, run: pnpm generate:fixtures
 * Then open __generated__/*.ts in VS Code
 *
 * DEBUGGING TIPS:
 * - To run a single fixture: pnpm vitest --run -t "defineModel basic"
 * - To run syntax tests only: pnpm vitest --run -t "syntax validation"
 * - To run semantic tests only: pnpm vitest --run -t "semantic validation"
 * - Use .only on specific tests below for debugging
 *
 * KNOWN ISSUES (tests expected to fail):
 * - destructured defineProps: Creates duplicate type aliases and malformed type references
 *   (e.g., `const { foo, bar } = defineProps<{ foo: string }>()`)
 */

const PREFIX = "___VERTER___";

// Get the process function from fixtures
const fixtureConfig = createFixtures(PREFIX);
const processMacros = fixtureConfig.process;

/**
 * Resolve a WithPrefixString value to its actual string value.
 * @deprecated Use resolveWithPrefix from fixtures/types instead
 */
function resolveExpectation(value: WithPrefixString): string {
  return resolveWithPrefix(value, PREFIX);
}

// Parse Vue SFC and get transformed result
function parseVueSFC(
  content: string,
  lang = "ts",
  generic?: string
): { result: string; context: any } {
  const result = processMacros(content, lang, generic);
  return {
    result: result.result,
    context: result.context,
  };
}

// Parse TypeScript source code into an AST
function parseTypeScript(code: string): ts.SourceFile {
  return ts.createSourceFile(
    "test.ts",
    code,
    ts.ScriptTarget.Latest,
    true,
    ts.ScriptKind.TS
  );
}

// Find all type alias declarations in the AST
function findTypeAliases(
  sourceFile: ts.SourceFile
): Map<string, ts.TypeAliasDeclaration> {
  const aliases = new Map<string, ts.TypeAliasDeclaration>();

  function visit(node: ts.Node) {
    if (ts.isTypeAliasDeclaration(node)) {
      aliases.set(node.name.text, node);
    }
    ts.forEachChild(node, visit);
  }

  ts.forEachChild(sourceFile, visit);
  return aliases;
}

// Find all variable declarations in the AST
function findVariableDeclarations(
  sourceFile: ts.SourceFile
): Map<string, ts.VariableDeclaration> {
  const declarations = new Map<string, ts.VariableDeclaration>();

  function visit(node: ts.Node) {
    if (ts.isVariableDeclaration(node) && ts.isIdentifier(node.name)) {
      declarations.set(node.name.text, node);
    }
    ts.forEachChild(node, visit);
  }

  ts.forEachChild(sourceFile, visit);
  return declarations;
}

// Check if code has syntax errors
function hasSyntaxErrors(sourceFile: ts.SourceFile): ts.Diagnostic[] {
  // Use the built-in syntax diagnostic check (parseDiagnostics is internal)
  return (sourceFile as any).parseDiagnostics || [];
}

// ============================================================================
// Local Wrapper for assertNoTypeErrors
// ============================================================================

/**
 * Assert that the generated code has no TypeScript errors.
 * This wraps the shared utility.
 */
function assertNoTypeErrors(code: string, testName?: string): void {
  assertNoTypeErrorsShared(code, testName);
}

// ============================================================================
// Get fixtures and prepare test data
// ============================================================================

const { fixtures } = createFixtures(PREFIX);

// Create test data arrays for it.each
// const fixtureNames = fixtures.map((f) => f.name);
// const fixturesByName = new Map(fixtures.map((f) => [f.name, f]));

// ============================================================================
// Individual test functions (can be called directly for debugging)
// ============================================================================

/**
 * Test a single fixture for syntax validity.
 * Can be called directly in a debugger or from a .only test.
 */
export function testSyntaxValidity(
  fixtureName: string,
  sourceFile: ts.SourceFile
): void {
  const errors = hasSyntaxErrors(sourceFile);

  if (errors.length > 0) {
    console.log(`Fixture: ${fixtureName}`);
    console.log(
      "Syntax errors:",
      errors.map((e) => e.messageText)
    );
    console.log("Generated code:", sourceFile.getFullText());
  }

  expect(errors).toHaveLength(0);
}

/**
 * Test a single fixture for semantic validity.
 * Can be called directly in a debugger or from a .only test.
 */
export function testSemanticValidity(
  fixtureName: string,
  content: string
): void {
  assertNoTypeErrors(content, fixtureName);
}

/**
 * Test a single type test for a fixture.
 * Can be called directly in a debugger or from a .only test.
 */
export function testTypeTest(
  fixture: Fixture,
  typeTestDescription: string,
  content: string
): void {
  const typeTest = fixture.expectations?.typeTests?.find(
    (t) => t.description === typeTestDescription
  );
  if (!typeTest) {
    throw new Error(
      `Type test not found: ${typeTestDescription} in fixture ${fixture.name}`
    );
  }

  const testResult = runTypeTest(content, typeTest, PREFIX);
  if (!testResult.success) {
    throw new Error(testResult.error);
  }
}

// ============================================================================
// Test Suites
// ============================================================================

describe("macros fixtures validation", () => {
  const fixturesMap = new Map<
    string,
    { fixture: Fixture; code: string; sourceFile: ts.SourceFile }
  >();
  function getFixture(name: string) {
    const entry = fixturesMap.get(name);
    if (entry) return entry;

    const fixture = fixtures.find((f) => f.name === name);
    if (!fixture) throw new Error(`Fixture not found: ${name}`);

    const { result } = parseVueSFC(fixture.code, fixture.lang, fixture.generic);
    const sourceFile = parseTypeScript(result);
    const record = { fixture, code: result, sourceFile };
    fixturesMap.set(name, record);
    return record;
  }
  const fixtureNames = fixtures.map((f) => f.name);

  // Test all fixtures for syntax validity using it.each
  describe("syntax validation", () => {
    it.each(fixtureNames)(
      "should generate syntactically valid TypeScript for: %s",
      (fixtureName) => {
        const { sourceFile } = getFixture(fixtureName);
        testSyntaxValidity(fixtureName, sourceFile);
      }
    );
  });

  // Test all fixtures for TypeScript semantic validity using it.each
  describe("semantic validation", () => {
    it.each(fixtureNames)(
      "should generate semantically valid TypeScript for: %s",
      (fixtureName) => {
        const { code } = getFixture(fixtureName);
        testSemanticValidity(fixtureName, code);
      }
    );
  });

  // Test fixtures with expectations
  describe("pattern expectations", () => {
    for (const fixture of fixtures) {
      if (!fixture.expectations) continue;

      describe(fixture.name, () => {
        const { code, sourceFile } = getFixture(fixture.name);
        if (fixture.expectations?.typeAliases) {
          const typeAliases = fixture.expectations.typeAliases.map((t) =>
            resolveExpectation(t)
          );
          it.each(typeAliases)("should create type alias: %s", (typeAlias) => {
            const aliases = findTypeAliases(sourceFile);
            expect(aliases.has(typeAlias)).toBe(true);
          });
        }

        if (fixture.expectations?.boxedVariables) {
          const boxedVars = fixture.expectations.boxedVariables.map((b) =>
            resolveExpectation(b)
          );
          it.each(boxedVars)("should create boxed variable: %s", (boxedVar) => {
            const declarations = findVariableDeclarations(sourceFile);
            expect(declarations.has(boxedVar)).toBe(true);
          });
        }

        if (fixture.expectations?.patterns) {
          const patterns = fixture.expectations.patterns.map((p) =>
            resolveExpectation(p)
          );
          it.each(patterns)("should contain pattern: %s", (pattern) => {
            expect(code).toContain(pattern);
          });
        }

        if (fixture.expectations?.antiPatterns) {
          const antiPatterns = fixture.expectations.antiPatterns.map((a) =>
            resolveExpectation(a)
          );
          it.each(antiPatterns)("should NOT contain: %s", (antiPattern) => {
            expect(code).not.toContain(antiPattern);
          });
        }

        // Type tests - verify hover types and catch any/unknown
        if (fixture.expectations?.typeTests) {
          const typeTests = fixture.expectations.typeTests;
          it.each(typeTests.map((t) => [t.description, t] as const))(
            "type test: %s",
            (_description, typeTest) => {
              const testResult = runTypeTest(code, typeTest, PREFIX);
              if (!testResult.success) {
                throw new Error(testResult.error);
              }
            }
          );
        }
      });
    }
  });
});

// ============================================================================
// Debug helpers - uncomment to debug a specific test
// ============================================================================

// Example: Debug a specific fixture's syntax
// describe.only("debug", () => {
//   it("debug syntax", () => {
//     testSyntaxValidity("defineModel basic");
//   });
// });

// Example: Debug a specific fixture's semantics
// describe.only("debug", () => {
//   it("debug semantics", () => {
//     testSemanticValidity("defineModel basic");
//   });
// });

// Example: Debug a specific type test
// describe.only("debug", () => {
//   it("debug type test", () => {
//     testTypeTest("defineModel basic", "model variable should not be any");
//   });
// });

// Example: Just print the generated code
// describe.only("debug", () => {
//   it("show generated code", () => {
//     console.log(getGeneratedCode("defineModel basic"));
//   });
// });

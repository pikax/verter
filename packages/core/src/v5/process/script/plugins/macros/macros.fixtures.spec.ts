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
 * This wraps the shared utility to pass the PREFIX.
 */
function assertNoTypeErrors(code: string, testName?: string): void {
  assertNoTypeErrorsShared(code, testName, PREFIX);
}

describe("macros fixtures validation", () => {
  const { fixtures } = createFixtures(PREFIX);

  // Test all fixtures for syntax validity
  describe("syntax validation", () => {
    for (const fixture of fixtures) {
      it(`should generate syntactically valid TypeScript for: ${fixture.name}`, () => {
        const { result } = parseVueSFC(
          fixture.code,
          fixture.lang,
          fixture.generic
        );
        const sourceFile = parseTypeScript(result);
        const errors = hasSyntaxErrors(sourceFile);

        if (errors.length > 0) {
          console.log(`Fixture: ${fixture.name}`);
          console.log(
            "Syntax errors:",
            errors.map((e) => e.messageText)
          );
          console.log("Generated code:", result);
        }

        expect(errors).toHaveLength(0);
      });
    }
  });

  // Test all fixtures for TypeScript semantic validity
  describe("semantic validation", () => {
    for (const fixture of fixtures) {
      //   if (fixture.skipSemanticValidation) {
      //     it.skip(`should generate semantically valid TypeScript for: ${fixture.name} (${fixture.skipSemanticValidation})`, () => {
      //       const { result } = parseVueSFC(fixture.code, fixture.lang, fixture.generic);
      //       assertNoTypeErrors(result, fixture.name);
      //     });
      //   } else {
      it(`should generate semantically valid TypeScript for: ${fixture.name}`, () => {
        const { result } = parseVueSFC(
          fixture.code,
          fixture.lang,
          fixture.generic
        );
        assertNoTypeErrors(result, fixture.name);
      });
      //   }
    }
  });

  // Test fixtures with expectations
  describe("pattern expectations", () => {
    for (const fixture of fixtures) {
      if (!fixture.expectations) continue;

      describe(fixture.name, () => {
        const { result } = parseVueSFC(
          fixture.code,
          fixture.lang,
          fixture.generic
        );

        if (fixture.expectations?.typeAliases) {
          for (const typeAliasItem of fixture.expectations.typeAliases) {
            const typeAlias = resolveExpectation(typeAliasItem);
            it(`should create type alias: ${typeAlias}`, () => {
              const sourceFile = parseTypeScript(result);
              const typeAliases = findTypeAliases(sourceFile);
              expect(typeAliases.has(typeAlias)).toBe(true);
            });
          }
        }

        if (fixture.expectations?.boxedVariables) {
          for (const boxedVarItem of fixture.expectations.boxedVariables) {
            const boxedVar = resolveExpectation(boxedVarItem);
            it(`should create boxed variable: ${boxedVar}`, () => {
              const sourceFile = parseTypeScript(result);
              const declarations = findVariableDeclarations(sourceFile);
              expect(declarations.has(boxedVar)).toBe(true);
            });
          }
        }

        if (fixture.expectations?.patterns) {
          for (const patternItem of fixture.expectations.patterns) {
            const pattern = resolveExpectation(patternItem);
            it(`should contain pattern: ${pattern.slice(0, 50)}...`, () => {
              expect(result).toContain(pattern);
            });
          }
        }

        if (fixture.expectations?.antiPatterns) {
          for (const antiPatternItem of fixture.expectations.antiPatterns) {
            const antiPattern = resolveExpectation(antiPatternItem);
            it(`should NOT contain: ${antiPattern.slice(0, 50)}...`, () => {
              expect(result).not.toContain(antiPattern);
            });
          }
        }

        // Type tests - verify hover types and catch any/unknown
        if (fixture.expectations?.typeTests) {
          for (const typeTest of fixture.expectations.typeTests) {
            const targetName = resolveWithPrefix(typeTest.target, PREFIX);
            it(`type test: ${typeTest.description}`, () => {
              const testResult = runTypeTest(result, typeTest, PREFIX);
              if (!testResult.success) {
                throw new Error(testResult.error);
              }
            });
          }
        }
      });
    }
  });
});

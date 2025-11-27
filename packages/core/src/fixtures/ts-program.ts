/**
 * TypeScript Program Utilities for Fixture Testing
 *
 * This module provides utilities for creating TypeScript programs
 * for testing fixture transformations. It provides type checking
 * capabilities to verify that generated code is semantically valid
 * and has correct types.
 *
 * ## Usage
 *
 * ```typescript
 * import { getTypeString, isAnyType, isUnknownType, runTypeTest } from "@/fixtures/ts-program";
 *
 * // Get the type of a symbol
 * const type = getTypeString(code, "myVariable", "variable");
 *
 * // Run type tests
 * const result = runTypeTest(code, typeTest, "___VERTER___");
 * ```
 */

import * as ts from "typescript";
import type { TypeTest } from "./types";
import { resolveWithPrefix } from "./types";

// ============================================================================
// Type Definitions for Semantic Validation
// ============================================================================

/**
 * Generate inline type definitions for semantic validation.
 *
 * These definitions simulate Vue macros and @verter/types helpers
 * to enable type checking without needing actual module resolution.
 *
 * @param prefix - The prefix used for helper types (e.g., "___VERTER___")
 * @returns Type definition string to prepend to code
 */
function generateTypeDefinitions(prefix: string): string {
  return `
// Standard library types (if not already defined)
type PropertyKey = string | number | symbol;
type Record<K extends keyof any, T> = { [P in K]: T };
type ReturnType<T extends (...args: any) => any> = T extends (...args: any) => infer R ? R : any;
type Readonly<T> = { readonly [K in keyof T]: T[K] };
type Partial<T> = { [K in keyof T]?: T[K] };
type Required<T> = { [K in keyof T]-?: T[K] };
type Pick<T, K extends keyof T> = { [P in K]: T[P] };
type Omit<T, K extends keyof any> = Pick<T, Exclude<keyof T, K>>;
type Exclude<T, U> = T extends U ? never : T;
type Extract<T, U> = T extends U ? T : never;

// DOM types (stubs for testing)
interface Event {}
interface MouseEvent extends Event {}
interface KeyboardEvent extends Event {}
interface FocusEvent extends Event {}
interface InputEvent extends Event {}

// Standard library stubs
interface Date {}
interface Map<K, V> { get(key: K): V | undefined; set(key: K, value: V): this; }
interface Set<T> { has(value: T): boolean; add(value: T): this; }
interface Array<T> { length: number; [n: number]: T; }

// Vue macros - these are global in Vue SFC context
declare function defineProps<T>(): T;
declare function defineProps<T>(props: T): T;
declare function defineEmits<T>(): T;
declare function defineEmits<T>(emits: T): T;
declare function defineModel<T>(options?: any): ModelRef<T | undefined, string>;
declare function defineModel<T>(name: string, options?: any): ModelRef<T | undefined, string>;
declare function defineSlots<T>(): T;
declare function defineExpose<T>(exposed?: T): void;
declare function withDefaults<T, D>(props: T, defaults: D): T;
declare function defineOptions<T>(options: T): void;

// Vue types
interface ModelRef<T, M extends PropertyKey = string, G = T, S = T> {
  value: T;
}
interface Ref<T> { value: T; }
interface ComputedRef<T> { readonly value: T; }
declare function ref<T>(value: T): Ref<T>;
declare function computed<T>(getter: () => T): ComputedRef<T>;

// @verter/types helpers - Prettify flattens intersection types for better display
type Prettify<T> = T extends { (...args: any[]): any }
  ? T & {}
  : { [K in keyof T]: T[K] } & {};

// Prefixed version for code that uses it
type ${prefix}Prettify<T> = Prettify<T>;

// Helper function stubs for runtime
declare function ${prefix}createMacroReturn<T>(value: T): T;

// Box functions for capturing macro values
declare function ${prefix}defineProps_Box<T extends Record<string, any>>(props: T): T;
declare function ${prefix}defineEmits_Box<T extends readonly string[]>(emits: T): T;
declare function ${prefix}defineEmits_Box<T extends Record<string, any>>(emits: T): T;
declare function ${prefix}defineModel_Box<T>(options?: any): any;
declare function ${prefix}defineModel_Box<T>(name: string, options?: any): [string, any];
declare function ${prefix}defineExpose_Box<T>(exposed?: T): T;
declare function ${prefix}defineSlots_Box<T>(slots?: T): T;
declare function ${prefix}withDefaults_Box<T, D>(props: T, defaults: D): [T, D];

// PropType for object syntax props
interface PropType<T> {}
declare const String: PropType<string>;
declare const Number: PropType<number>;
declare const Boolean: PropType<boolean>;
declare const Array: PropType<any[]>;
declare const Object: PropType<object>;
declare const Function: PropType<(...args: any[]) => any>;
`;
}

// ============================================================================
// Type String Extraction
// ============================================================================

/**
 * Options for creating a TypeScript program for type testing.
 */
export interface TypeProgramOptions {
  /**
   * Whether to strip import statements from the code.
   * When true, imports are removed and type definitions are injected.
   * @default true
   */
  stripImports?: boolean;

  /**
   * Additional type definitions to prepend to the code.
   * Use this for any additional declarations needed.
   */
  additionalDefinitions?: string;

  /**
   * The prefix used for generated type names.
   * Used for creating helper type mappings.
   */
  prefix?: string;
}

/**
 * Create a TypeScript program from code with injected type definitions.
 *
 * This creates a full TypeScript program with inline type definitions
 * for Vue macros and @verter/types helpers.
 *
 * @param code - The TypeScript code to compile
 * @param options - Configuration options
 * @returns The TypeScript program, source file, and type checker
 */
export function createTypeProgram(
  code: string,
  options: TypeProgramOptions = {}
): {
  program: ts.Program;
  sourceFile: ts.SourceFile;
  checker: ts.TypeChecker;
} {
  const { stripImports = true, additionalDefinitions = "", prefix = "___VERTER___" } = options;

  // Process the code
  let processedCode = code;

  if (stripImports) {
    // Strip import statements - we'll inject type definitions instead
    processedCode = code
      .replace(/import\s+[\s\S]*?\s+from\s+["'][^"']+["'];?\s*/g, "")
      .replace(/import\s*["'][^"']+["'];?\s*/g, "");
  }

  // Generate and prepend type definitions
  const typeDefinitions = generateTypeDefinitions(prefix);
  const fullCode = typeDefinitions + additionalDefinitions + "\n" + processedCode;

  const compilerOptions: ts.CompilerOptions = {
    target: ts.ScriptTarget.ESNext,
    module: ts.ModuleKind.ESNext,
    moduleResolution: ts.ModuleResolutionKind.Bundler,
    strict: true,
    noEmit: true,
    skipLibCheck: true,
    lib: ["lib.esnext.d.ts"],
    types: [],
  };

  const fileName = "test.ts";
  const sourceFile = ts.createSourceFile(
    fileName,
    fullCode,
    ts.ScriptTarget.ESNext,
    true
  );

  // Create a minimal compiler host
  const host: ts.CompilerHost = {
    getSourceFile: (name) => (name === fileName ? sourceFile : undefined),
    getDefaultLibFileName: () => "lib.d.ts",
    writeFile: () => {},
    getCurrentDirectory: () => "/",
    getCanonicalFileName: (f) => f,
    useCaseSensitiveFileNames: () => true,
    getNewLine: () => "\n",
    fileExists: (name) => name === fileName,
    readFile: () => undefined,
  };

  const program = ts.createProgram([fileName], compilerOptions, host);
  const checker = program.getTypeChecker();

  return { program, sourceFile, checker };
}

/**
 * Get the type string for a symbol using real TypeScript type resolution.
 *
 * This uses a full TypeScript program with real module resolution
 * to get accurate type information.
 *
 * @param code - The TypeScript code to analyze
 * @param symbolName - The name of the symbol to get the type for
 * @param kind - Whether to look for a "type" alias or "variable" declaration
 * @param prefix - The prefix used for helper types
 * @returns The type as a string, or null if not found
 *
 * @example
 * ```typescript
 * const type = getTypeString(
 *   'type MyType = { foo: string }',
 *   'MyType',
 *   'type'
 * );
 * // => "{ foo: string }"
 * ```
 */
export function getTypeString(
  code: string,
  symbolName: string,
  kind: "type" | "variable" = "type",
  prefix: string = ""
): string | null {
  const { sourceFile, checker } = createTypeProgram(code, { prefix });

  let result: string | null = null;

  function visit(node: ts.Node) {
    if (kind === "type" && ts.isTypeAliasDeclaration(node)) {
      if (node.name.text === symbolName) {
        const type = checker.getTypeAtLocation(node.name);
        result = checker.typeToString(
          type,
          node,
          ts.TypeFormatFlags.NoTruncation |
            ts.TypeFormatFlags.MultilineObjectLiterals
        );
      }
    } else if (kind === "variable" && ts.isVariableDeclaration(node)) {
      if (ts.isIdentifier(node.name) && node.name.text === symbolName) {
        const type = checker.getTypeAtLocation(node.name);
        result = checker.typeToString(
          type,
          node,
          ts.TypeFormatFlags.NoTruncation |
            ts.TypeFormatFlags.MultilineObjectLiterals
        );
      }
    }
    ts.forEachChild(node, visit);
  }

  ts.forEachChild(sourceFile, visit);
  return result;
}

// ============================================================================
// Type Validation Utilities
// ============================================================================

/**
 * Check if a type string represents `any`.
 *
 * This is critical for catching silent type inference failures.
 * When TypeScript can't infer a type, it often falls back to `any`,
 * which disables type checking for that value.
 *
 * @param typeStr - The type string to check
 * @returns true if the type is `any`
 *
 * @example
 * ```typescript
 * isAnyType("any")      // => true
 * isAnyType("any[]")    // => false (this is an array of any)
 * isAnyType("string")   // => false
 * ```
 */
export function isAnyType(typeStr: string): boolean {
  // Check for exact "any" - not "any[]" or other compound types
  return typeStr === "any";
}

/**
 * Check if a type string represents `unknown`.
 *
 * While `unknown` is safer than `any`, it still indicates
 * a type inference failure that should be investigated.
 *
 * @param typeStr - The type string to check
 * @returns true if the type is `unknown`
 */
export function isUnknownType(typeStr: string): boolean {
  return typeStr === "unknown";
}

/**
 * Normalize a type string for comparison.
 *
 * Removes whitespace variations and normalizes formatting
 * to enable reliable type comparison.
 *
 * @param typeStr - The type string to normalize
 * @returns Normalized type string
 */
export function normalizeTypeString(typeStr: string): string {
  return typeStr
    // Normalize whitespace
    .replace(/\s+/g, " ")
    // Remove spaces around punctuation
    .replace(/\s*([<>(),|&:;{}[\]])\s*/g, "$1")
    // Add space after colons in object types
    .replace(/:(?=[^:\s])/g, ": ")
    .trim();
}

// ============================================================================
// Type Test Runner
// ============================================================================

/**
 * Result from running a type test.
 */
export interface TypeTestResult {
  /** Whether the test passed */
  success: boolean;
  /** Error message if the test failed */
  error?: string;
  /** The actual type string that was found */
  actualType?: string;
}

/**
 * Run a single type test against generated code.
 *
 * This function performs all the type test validations:
 * - Checks that the target symbol exists
 * - Validates `notAny` and `notUnknown` constraints
 * - Checks `shouldContain` patterns
 * - Checks `shouldNotContain` patterns
 * - Checks `shouldEqual` exact match (if provided)
 *
 * @param code - The generated code to test
 * @param test - The type test configuration
 * @param prefix - The prefix used for type names
 * @returns Test result with success status and error details
 *
 * @example
 * ```typescript
 * const result = runTypeTest(
 *   'type MyProps = { msg: string }',
 *   {
 *     target: 'MyProps',
 *     description: 'Props should have msg property',
 *     shouldContain: ['msg: string'],
 *     notAny: true,
 *   },
 *   '___VERTER___'
 * );
 * ```
 */
export function runTypeTest(
  code: string,
  test: TypeTest,
  prefix: string
): TypeTestResult {
  const targetName = resolveWithPrefix(test.target, prefix);
  const kind = test.kind || "type";

  const typeStr = getTypeString(code, targetName, kind, prefix);

  if (typeStr === null) {
    return {
      success: false,
      error: `${kind} "${targetName}" not found in generated code`,
    };
  }

  // Check notAny
  if (test.notAny && isAnyType(typeStr)) {
    return {
      success: false,
      error: `${kind} "${targetName}" resolved to "any" - this breaks type safety!\nFull type: ${typeStr}`,
      actualType: typeStr,
    };
  }

  // Check notUnknown
  if (test.notUnknown && isUnknownType(typeStr)) {
    return {
      success: false,
      error: `${kind} "${targetName}" resolved to "unknown" - type inference may have failed.\nFull type: ${typeStr}`,
      actualType: typeStr,
    };
  }

  // Check shouldContain patterns
  if (test.shouldContain) {
    for (const pattern of test.shouldContain) {
      if (!typeStr.includes(pattern)) {
        return {
          success: false,
          error: `${kind} "${targetName}" should contain "${pattern}" but got: ${typeStr}`,
          actualType: typeStr,
        };
      }
    }
  }

  // Check shouldNotContain patterns
  if (test.shouldNotContain) {
    for (const pattern of test.shouldNotContain) {
      if (typeStr.includes(pattern)) {
        return {
          success: false,
          error: `${kind} "${targetName}" should NOT contain "${pattern}" but got: ${typeStr}`,
          actualType: typeStr,
        };
      }
    }
  }

  // Check shouldEqual exact match
  if (test.shouldEqual) {
    const normalizedActual = normalizeTypeString(typeStr);
    const normalizedExpected = normalizeTypeString(test.shouldEqual);

    if (normalizedActual !== normalizedExpected) {
      return {
        success: false,
        error: `${kind} "${targetName}" type mismatch:\n  Expected: ${test.shouldEqual}\n  Actual:   ${typeStr}`,
        actualType: typeStr,
      };
    }
  }

  return { success: true, actualType: typeStr };
}

// ============================================================================
// Semantic Error Checking
// ============================================================================

/**
 * Get semantic errors from TypeScript code using real module resolution.
 *
 * This creates a full TypeScript program and checks for both
 * syntax and semantic errors.
 *
 * @param code - The TypeScript code to check
 * @param prefix - The prefix used for helper types
 * @returns Array of TypeScript diagnostics
 */
export function getSemanticErrors(
  code: string,
  prefix: string = ""
): ts.Diagnostic[] {
  const { program, sourceFile } = createTypeProgram(code, { prefix });

  // Get all diagnostics (semantic + syntactic)
  const diagnostics = [
    ...program.getSyntacticDiagnostics(sourceFile),
    ...program.getSemanticDiagnostics(sourceFile),
  ];

  return diagnostics;
}

/**
 * Format TypeScript diagnostics into readable error messages.
 *
 * @param diagnostics - Array of TypeScript diagnostics
 * @returns Formatted error message string
 */
export function formatDiagnostics(diagnostics: ts.Diagnostic[]): string {
  return diagnostics
    .map((d) => {
      const message = ts.flattenDiagnosticMessageText(d.messageText, "\n");
      if (d.file && d.start !== undefined) {
        const { line, character } = d.file.getLineAndCharacterOfPosition(
          d.start
        );
        return `  Line ${line + 1}, Col ${character + 1}: ${message}`;
      }
      return `  ${message}`;
    })
    .join("\n");
}

/**
 * Assert that code has no TypeScript errors.
 *
 * Throws an error with detailed diagnostics if any errors are found.
 *
 * @param code - The TypeScript code to check
 * @param testName - Optional test name for error context
 * @param prefix - The prefix used for helper types
 * @throws Error if TypeScript errors are found
 */
export function assertNoTypeErrors(
  code: string,
  testName?: string,
  prefix: string = ""
): void {
  const errors = getSemanticErrors(code, prefix);

  if (errors.length > 0) {
    const errorMessages = formatDiagnostics(errors);
    const context = testName ? ` in test "${testName}"` : "";
    throw new Error(
      `TypeScript errors found${context}:\n${errorMessages}\n\nGenerated code:\n${code}`
    );
  }
}

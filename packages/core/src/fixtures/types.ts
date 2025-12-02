/**
 * Shared types and utilities for fixture-based type testing.
 *
 * This module provides the foundation for creating type fixtures that can be:
 * 1. Generated as standalone .ts files for IDE inspection (hover to see types)
 * 2. Used in vitest spec files for automated testing
 * 3. Type-checked to ensure no accidental `any` or `unknown` types
 *
 * ## Quick Start
 *
 * 1. Create a `*.fixtures.ts` file in your plugin folder
 * 2. Export a `createFixtures` function that returns `FixtureConfig`
 * 3. Run `pnpm generate:fixtures` to generate `.ts` files for IDE inspection
 * 4. Import fixtures in your `.spec.ts` for automated testing
 *
 * ## Example Fixture Definition
 *
 * ```typescript
 * export const fixtures: Fixture[] = [
 *   {
 *     name: "defineProps with type argument",
 *     code: `const props = defineProps<{ msg: string }>()`,
 *     expectations: {
 *       typeAliases: [(p) => p + "defineProps_Type"],
 *       patterns: ["msg: string"],
 *       typeTests: [
 *         {
 *           target: (p) => `${p}defineProps_Type`,
 *           description: "Props type should be the defined interface",
 *           shouldContain: ["msg: string"],
 *           notAny: true,
 *         },
 *       ],
 *     },
 *   },
 * ];
 * ```
 */

import { ScriptContext } from "../v5/process/script/types";

// ============================================================================
// Core Types
// ============================================================================

/**
 * A single test fixture definition.
 *
 * Each fixture represents one test case with:
 * - Input code (Vue SFC script setup content)
 * - Expected output patterns and type structures
 * - Optional type tests for hover verification
 *
 * @example
 * ```typescript
 * const fixture: Fixture = {
 *   name: "basic defineProps",
 *   code: `const props = defineProps<{ msg: string }>()`,
 *   lang: "ts",
 *   expectations: {
 *     typeAliases: [(p) => p + "defineProps_Type"],
 *     patterns: ["msg: string"],
 *   },
 * };
 * ```
 */
export interface Fixture {
  /**
   * Descriptive name for the fixture.
   * Used in test names and generated file names.
   *
   * @example "defineProps with type argument"
   * @example "defineModel with options"
   */
  name: string;

  /**
   * Vue SFC script setup content (without `<script>` tags).
   * This is the raw code that will be transformed by the plugin.
   *
   * @example
   * ```typescript
   * code: `
   * const props = defineProps<{ msg: string }>()
   * const emit = defineEmits<{ change: [value: string] }>()
   * `
   * ```
   */
  code: string;

  /**
   * Language for the script block.
   * Determines how the code is parsed and transformed.
   *
   * @default "ts"
   */
  lang?: "ts" | "js" | "tsx" | "jsx";

  /**
   * Generic type parameters for the script setup block.
   * Used for generic components: `<script setup lang="ts" generic="T">`.
   *
   * @example "T"
   * @example "T extends object"
   * @example "T, K extends keyof T"
   */
  generic?: string;

  /**
   * Vue SFC template content (without `<template>` tags).
   * Used for testing template transformations.
   *
   * @example
   * ```typescript
   * template: `
   * <div>
   *   <span>{{ msg }}</span>
   * </div>
   * `
   * ```
   */
  template?: string;

  /**
   * Expected patterns and type structures to verify in the output.
   * See {@link FixtureExpectations} for available validation options.
   */
  expectations?: FixtureExpectations;
}

// ============================================================================
// Prefix String Types
// ============================================================================

/**
 * A string value that may depend on the configured prefix.
 *
 * Can be either:
 * - A static string: `"defineProps_Type"`
 * - A function that receives the prefix: `(p) => p + "defineProps_Type"`
 *
 * Use functions when the expected value includes the configured prefix
 * (e.g., `___VERTER___defineProps_Type`).
 *
 * @example Static string (no prefix needed)
 * ```typescript
 * const pattern: WithPrefixString = "msg: string";
 * ```
 *
 * @example Dynamic string with prefix
 * ```typescript
 * const pattern: WithPrefixString = (p) => `${p}defineProps_Type`;
 * // With prefix "___VERTER___" resolves to "___VERTER___defineProps_Type"
 * ```
 */
export type WithPrefixString = string | ((prefix?: string) => string);

// ============================================================================
// Type Test Types
// ============================================================================

/**
 * Configuration for testing a specific type's hover information.
 *
 * Type tests verify that:
 * 1. A type/variable exists with the expected name
 * 2. Its hover information contains specific content
 * 3. It is NOT resolved to `any` or `unknown` (which would break type safety)
 *
 * These tests catch issues where TypeScript silently falls back to `any`,
 * which can mask type errors throughout the codebase.
 *
 * @example Basic type test
 * ```typescript
 * const test: TypeTest = {
 *   target: (p) => `${p}defineProps_Type`,
 *   description: "Props type should contain msg property",
 *   shouldContain: ["msg: string"],
 *   notAny: true,
 * };
 * ```
 *
 * @example Testing for specific NOT patterns
 * ```typescript
 * const test: TypeTest = {
 *   target: "props",
 *   description: "Props variable should not be any",
 *   notAny: true,
 *   notUnknown: true,
 *   shouldNotContain: ["error", "never"],
 * };
 * ```
 */
export interface TypeTest {
  /**
   * The target type alias or variable name to test.
   * Can be a static string or a function that receives the prefix.
   *
   * @example Static: `"props"`
   * @example Dynamic: `(p) => \`${p}defineProps_Type\``
   */
  target: WithPrefixString;

  /**
   * Human-readable description of what this type test verifies.
   * Used in test output and error messages.
   *
   * @example "Props type should be the defined interface"
   * @example "Emit function should have correct call signatures"
   */
  description: string;

  /**
   * Strings that MUST appear in the type's hover information.
   * Use this to verify the type contains expected properties/signatures.
   *
   * @example `["msg: string", "count?: number"]`
   */
  shouldContain?: string[];

  /**
   * Strings that must NOT appear in the type's hover information.
   * Use this to verify unwanted types are not present.
   *
   * @example `["never", "error"]`
   */
  shouldNotContain?: string[];

  /**
   * If true, asserts the type is NOT `any`.
   *
   * This is critical for catching silent type inference failures.
   * When TypeScript can't infer a type, it often falls back to `any`,
   * which disables type checking for that value.
   *
   * @default false
   * @example
   * ```typescript
   * // BAD: props is `any` - no type safety!
   * const props = defineProps<any>()
   *
   * // GOOD: props has proper type
   * const props = defineProps<{ msg: string }>()
   * ```
   */
  notAny?: boolean;

  /**
   * If true, asserts the type is NOT `unknown`.
   *
   * While `unknown` is safer than `any`, it still indicates
   * a type inference failure that should be investigated.
   *
   * @default false
   */
  notUnknown?: boolean;

  /**
   * The kind of symbol to test.
   * - `"type"`: Test a type alias (e.g., `type Foo = ...`)
   * - `"variable"`: Test a variable declaration (e.g., `const foo = ...`)
   *
   * @default "type"
   */
  kind?: "type" | "variable";

  /**
   * Exact type string that the type MUST equal.
   *
   * Use this for precise type matching when you need to verify
   * the exact structure of a type, including function signatures.
   *
   * The comparison is whitespace-normalized, so minor formatting
   * differences won't cause failures.
   *
   * @example Exact emit function signature
   * ```typescript
   * shouldEqual: "((e: \"change\", id: number) => void) | ((e: \"update\", value: string) => void)"
   * ```
   *
   * @example Object type
   * ```typescript
   * shouldEqual: "{ msg: string; count?: number }"
   * ```
   */
  shouldEqual?: string;
}

// ============================================================================
// Expectations Types
// ============================================================================

/**
 * Expected patterns and structures for validating fixture output.
 *
 * Expectations define what the transformed code should (and shouldn't) contain.
 * They are used both for automated testing and documentation.
 *
 * @example Complete expectations
 * ```typescript
 * const expectations: FixtureExpectations = {
 *   // Type aliases that should be created
 *   typeAliases: [(p) => p + "defineProps_Type"],
 *
 *   // Boxed variables for runtime values
 *   boxedVariables: [(p) => p + "defineProps_Boxed"],
 *
 *   // Patterns that must appear in output
 *   patterns: ["msg: string", (p) => p + "Prettify"],
 *
 *   // Patterns that must NOT appear
 *   antiPatterns: ["any", "unknown"],
 *
 *   // Type tests for hover verification
 *   typeTests: [
 *     {
 *       target: (p) => `${p}defineProps_Type`,
 *       description: "Props type should be correct",
 *       shouldContain: ["msg: string"],
 *       notAny: true,
 *     },
 *   ],
 * };
 * ```
 */
export interface FixtureExpectations {
  /**
   * Type aliases that should be created in the output.
   *
   * These are `type X = ...` declarations generated by the transformation.
   * The test verifies these type aliases exist in the generated code.
   *
   * @example
   * ```typescript
   * typeAliases: [
   *   (p) => p + "defineProps_Type",    // ___VERTER___defineProps_Type
   *   (p) => p + "defineEmits_Type",    // ___VERTER___defineEmits_Type
   * ]
   * ```
   */
  typeAliases?: WithPrefixString[];

  /**
   * Boxed variables that should be created in the output.
   *
   * These are `const X = Box(...)` declarations that wrap runtime values
   * to preserve their type information for extraction.
   *
   * @example
   * ```typescript
   * boxedVariables: [
   *   (p) => p + "defineProps_Boxed",   // For object syntax props
   *   (p) => p + "defineEmits_Boxed",   // For array syntax emits
   * ]
   * ```
   */
  boxedVariables?: WithPrefixString[];

  /**
   * Patterns that MUST appear somewhere in the output.
   *
   * Use for verifying specific code patterns, type structures,
   * or identifiers are present in the transformed code.
   *
   * @example
   * ```typescript
   * patterns: [
   *   "msg: string",                         // Property in type
   *   (p) => `defineProps<${p}defineProps_Type>()`,  // Macro call
   *   (p) => p + "Prettify",                 // Helper type usage
   * ]
   * ```
   */
  patterns?: WithPrefixString[];

  /**
   * Patterns that must NOT appear in the output.
   *
   * Use for verifying certain patterns are absent, such as:
   * - Type aliases that shouldn't be created
   * - Incorrect transformations
   * - Unwanted fallback types
   *
   * @example
   * ```typescript
   * antiPatterns: [
   *   (p) => p + "defineProps_Type",  // Not created for object syntax
   *   "as any",                        // No unsafe type assertions
   * ]
   * ```
   */
  antiPatterns?: WithPrefixString[];

  /**
   * Type tests for verifying hover information and type safety.
   *
   * These tests go beyond pattern matching to verify:
   * - Specific types resolve to expected structures
   * - No accidental `any` or `unknown` types
   * - Hover information shows correct content
   *
   * @example
   * ```typescript
   * typeTests: [
   *   {
   *     target: (p) => `${p}defineProps_Type`,
   *     description: "Props type should match interface",
   *     shouldContain: ["msg: string", "count?: number"],
   *     notAny: true,
   *   },
   *   {
   *     target: "props",
   *     kind: "variable",
   *     description: "Props variable should have correct type",
   *     notAny: true,
   *     notUnknown: true,
   *   },
   * ]
   * ```
   */
  typeTests?: TypeTest[];
}

// ============================================================================
// Configuration Types
// ============================================================================

/**
 * Configuration for a fixture file.
 *
 * This is returned by the `createFixtures()` function in each `*.fixtures.ts` file.
 *
 * @example
 * ```typescript
 * export function createFixtures(prefix = "___VERTER___"): FixtureConfig {
 *   return {
 *     fixtures: myFixtures,
 *     process: (code, lang) => processMyPlugin(code, prefix, lang),
 *     prefix,
 *   };
 * }
 * ```
 */
export interface FixtureConfig {
  /**
   * Array of fixtures to generate and test.
   */
  fixtures: Fixture[];

  /**
   * Process function to transform Vue SFC content.
   *
   * This function receives the full fixture object, giving implementations
   * full control over how to handle all fixture properties (code, template,
   * lang, generic, etc.).
   *
   * @param fixture - The fixture object containing all test data
   * @returns The processed result with transformed code
   *
   * @example
   * ```typescript
   * process: (fixture) => {
   *   const result = myPlugin.transform(fixture.code, {
   *     lang: fixture.lang,
   *     generic: fixture.generic,
   *     template: fixture.template,
   *   });
   *   return {
   *     result: result.code,
   *     context: result.context,
   *     sourcemap: result.map?.toUrl(),
   *   };
   * }
   * ```
   */
  process: (fixture: Fixture) => ProcessResult;

  /**
   * Prefix for helper types and internal variables.
   *
   * This prefix is added to generated type aliases and variables
   * to avoid naming conflicts with user code.
   *
   * Set to `""` (empty string) to disable prefixing for cleaner output
   * in generated fixture files.
   *
   * @default "___VERTER___"
   *
   * @example
   * ```typescript
   * // With prefix "___VERTER___":
   * type ___VERTER___defineProps_Type = Prettify<{ msg: string }>;
   *
   * // With empty prefix:
   * type defineProps_Type = Prettify<{ msg: string }>;
   * ```
   */
  prefix?: string;
}

// ============================================================================
// Result Types
// ============================================================================

/**
 * Result from processing a fixture through the transformation pipeline.
 *
 * Contains the transformed code and optional metadata like
 * context information and sourcemaps.
 */
export interface ProcessResult {
  /**
   * The transformed code result.
   * This is the final output after plugin processing.
   */
  result: string;

  /**
   * Optional context with additional processing info.
   * Contains metadata about the transformation like:
   * - Created type aliases
   * - Macro bindings
   * - Import statements
   */
  context?: ScriptContext;

  /**
   * Optional inline sourcemap URL (data URL format).
   * Enables source mapping in generated fixture files.
   */
  sourcemap?: string;
}

/**
 * Result from running a fixture through the pipeline.
 *
 * Used by the fixture runner to track success/failure status.
 */
export interface FixtureResult {
  /** The fixture definition that was processed */
  fixture: Fixture;

  /** The processed output code */
  output: string;

  /** Whether processing completed successfully */
  success: boolean;

  /** Error message if processing failed */
  error?: string;
}

// ============================================================================
// Utility Functions
// ============================================================================

/**
 * Resolve a {@link WithPrefixString} value to its actual string.
 *
 * Handles both static strings and prefix functions.
 *
 * @param value - The value to resolve (string or function)
 * @param prefix - The prefix to pass to functions (defaults to empty string)
 * @returns The resolved string value
 *
 * @example Static string
 * ```typescript
 * resolveWithPrefix("msg: string", "___VERTER___")
 * // => "msg: string"
 * ```
 *
 * @example Function with prefix
 * ```typescript
 * resolveWithPrefix((p) => `${p}defineProps_Type`, "___VERTER___")
 * // => "___VERTER___defineProps_Type"
 * ```
 */
export function resolveWithPrefix(
  value: WithPrefixString,
  prefix: string = ""
): string {
  return typeof value === "function" ? value(prefix) : value;
}

/**
 * Generate header content for fixture files.
 *
 * Creates the standard imports and type aliases needed for
 * generated fixture files to be valid TypeScript.
 *
 * @param prefix - The prefix used for type aliases (default: empty)
 * @returns The header string to prepend to generated files
 *
 * @example
 * ```typescript
 * const header = createFixtureHeader("___VERTER___");
 * // Includes Vue imports and type alias re-exports
 * ```
 */
export function createFixtureHeader(prefix: string = ""): string {
  const importStatement = prefix
    ? `import type * as VerterTypes from "@verter/types";\n\n// Re-export with prefix for type resolution\ntype ${prefix}Prettify<T> = VerterTypes.Prettify<T>;\n// ... other type aliases would go here`
    : `import type * as VerterTypes from "@verter/types";`;

  return `/**
 * AUTO-GENERATED FILE - DO NOT EDIT
 * 
 * This file is generated by the fixtures generator.
 * Open this file in VS Code to inspect types with hover.
 * 
 * Install the "Twoslash Query Comments" extension for inline type hints:
 * https://marketplace.visualstudio.com/items?itemName=Orta.vscode-twoslash-queries
 * 
 * Run: pnpm generate:fixtures
 */

/* eslint-disable */

import type { Ref, ComputedRef, ModelRef, WritableComputedRef } from 'vue';
import { ref, computed, defineProps, defineEmits, defineModel, defineSlots, defineExpose, withDefaults } from 'vue';
${importStatement}

`;
}

/**
 * Format code for better readability in generated fixtures.
 *
 * Applies formatting rules to make generated code easier to read:
 * - Adds line breaks between statements
 * - Cleans up excessive whitespace
 *
 * @param code - The code to format
 * @returns Formatted code string
 *
 * @example
 * ```typescript
 * const formatted = formatForReadability(
 *   "type A = string;;const b = 1"
 * );
 * // => "type A = string;\nconst b = 1"
 * ```
 */
export function formatForReadability(code: string): string {
  return (
    code
      // Split after closing brace/parenthesis + semicolon before statement start
      .replace(/(}|>|\)|;)\s*;(const|let|type)/g, "$1;\n$2")
      // Clean up double newlines
      .replace(/\n\n+/g, "\n")
  );
}

/**
 * Add twoslash query annotations for IDE type inspection.
 *
 * Inserts `//^?` comments after variable and type declarations
 * to enable type hover inspection with the Twoslash Query Comments
 * VS Code extension.
 *
 * @param code - The code to annotate
 * @returns Code with twoslash annotations
 *
 * @example
 * ```typescript
 * const annotated = addTwoslashAnnotations("const props = defineProps()");
 * // => "const props = defineProps()\n     //^^^^^?"
 * ```
 */
export function addTwoslashAnnotations(code: string): string {
  const formatted = formatForReadability(code);
  const lines = formatted.split("\n");
  const result: string[] = [];

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    result.push(line);

    // Add ^? annotation after variable declarations
    const inlineVarMatch = line.match(/(const|let)\s+(\w+)\s*=/);
    if (inlineVarMatch) {
      const [fullMatch, , varName] = inlineVarMatch;
      // Only annotate user-facing variables
      if (!varName.startsWith("___VERTER___")) {
        const varStart = line.indexOf(fullMatch) + fullMatch.indexOf(varName);
        const spaces = " ".repeat(varStart);
        result.push(`${spaces}//${"^".repeat(varName.length)}?`);
      }
    }

    // Add ^? annotation after standalone type alias declarations
    const standaloneTypeMatch = line.match(
      /^(\s*)type\s+(___VERTER___\w+)\s*=/
    );
    if (standaloneTypeMatch) {
      const [, , typeName] = standaloneTypeMatch;
      const typeStart = line.indexOf(typeName);
      const spaces = " ".repeat(typeStart);
      result.push(`${spaces}//${"^".repeat(Math.min(typeName.length, 30))}?`);
    }
  }

  return result.join("\n");
}

/**
 * Generate a single fixture block for the output file.
 *
 * Creates a formatted code block containing:
 * - Section header with fixture name
 * - Original source code (commented)
 * - Transformed output with optional annotations
 *
 * @param fixture - The fixture definition
 * @param output - The transformed output code
 * @param includeAnnotations - Whether to add twoslash annotations (default: true)
 * @returns Formatted fixture block string
 *
 * @example
 * ```typescript
 * const block = generateFixtureBlock(
 *   { name: "basic props", code: "const p = defineProps()" },
 *   "const p = defineProps<Type>()",
 *   true
 * );
 * ```
 */
export function generateFixtureBlock(
  fixture: Fixture,
  output: string,
  includeAnnotations: boolean = true
): string {
  const processedOutput = includeAnnotations
    ? addTwoslashAnnotations(output)
    : formatForReadability(output);

  return `
// ============================================================================
// ${fixture.name}
// ============================================================================
/*
Original:
\`\`\`vue
<script setup lang="${fixture.lang || "ts"}">${fixture.code}</script>
\`\`\`
*/

// Transformed output:
{
${processedOutput
  .split("\n")
  .map((l: string) => `  ${l}`)
  .join("\n")}
}
`;
}

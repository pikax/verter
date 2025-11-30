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
import * as path from "path";
import type { TypeTest } from "./types";
import { resolveWithPrefix } from "./types";

// ============================================================================
// Path Constants
// ============================================================================

/** Path to the packages/core directory */
const CORE_PACKAGE_DIR = path.resolve(__dirname, "../..");

/** Path to tsconfig.test.json */
const TSCONFIG_TEST_PATH = path.join(CORE_PACKAGE_DIR, "tsconfig.test.json");

/** Path to global.d.ts for fixture testing */
const GLOBAL_DTS_PATH = path.join(__dirname, "global.d.ts");

/** Virtual test file path */
const TEST_FILE_PATH = path.join(CORE_PACKAGE_DIR, "src", "__test__.ts");

// Pre-normalize paths for fast comparison
const NORMALIZED_TEST_FILE_PATH = path.normalize(TEST_FILE_PATH);
const NORMALIZED_GLOBAL_DTS_PATH = path.normalize(GLOBAL_DTS_PATH);

// ============================================================================
// Cached TypeScript Configuration
// ============================================================================

let cachedCompilerOptions: ts.CompilerOptions | null = null;

/**
 * Load and parse the tsconfig.test.json configuration.
 * Results are cached for performance.
 */
function loadTsConfig(): ts.CompilerOptions {
  if (cachedCompilerOptions) {
    return cachedCompilerOptions;
  }

  const configFile = ts.readConfigFile(TSCONFIG_TEST_PATH, ts.sys.readFile);
  if (configFile.error) {
    throw new Error(
      `Failed to read tsconfig.test.json: ${ts.flattenDiagnosticMessageText(configFile.error.messageText, "\n")}`
    );
  }

  const parsedConfig = ts.parseJsonConfigFileContent(
    configFile.config,
    ts.sys,
    CORE_PACKAGE_DIR
  );

  if (parsedConfig.errors.length > 0) {
    const errors = parsedConfig.errors
      .map((e) => ts.flattenDiagnosticMessageText(e.messageText, "\n"))
      .join("\n");
    throw new Error(`Failed to parse tsconfig.test.json: ${errors}`);
  }

  // Override some options for fixture testing
  cachedCompilerOptions = {
    ...parsedConfig.options,
    noEmit: true,
    skipLibCheck: true,
  };

  return cachedCompilerOptions;
}

// ============================================================================
// Cached Language Service for Performance
// ============================================================================

/**
 * Cached language service instance for efficient repeated type checking.
 * The language service caches parsed files and type information,
 * so only the test file needs to be re-parsed on each check.
 */
let cachedLanguageService: ts.LanguageService | null = null;
let cachedServiceHost: {
  host: ts.LanguageServiceHost;
  setTestFileContent: (content: string) => void;
  getTestFileVersion: () => number;
} | null = null;

/** Cache for file contents read from disk (node_modules files don't change) */
const fileContentCache = new Map<string, string | undefined>();

/** Cache for script snapshots (avoid recreating for unchanged files) */
const scriptSnapshotCache = new Map<string, ts.IScriptSnapshot>();

/** Cache for module resolution results */
const moduleResolutionCache = new Map<string, ts.ResolvedModule | undefined>();

/** Cache for fileExists calls */
const fileExistsCache = new Map<string, boolean>();

/** Cache for directoryExists calls */
const directoryExistsCache = new Map<string, boolean>();

/** Cached default lib file path */
let cachedDefaultLibPath: string | null = null;

/**
 * Create or get the cached language service host.
 * The host manages file content and versions for the language service.
 */
function getLanguageServiceHost(): typeof cachedServiceHost {
  if (cachedServiceHost) {
    return cachedServiceHost;
  }

  const compilerOptions = loadTsConfig();
  const globalDtsContent = ts.sys.readFile(GLOBAL_DTS_PATH) ?? "";
  
  // Pre-create snapshot for global.d.ts (never changes)
  const globalDtsSnapshot = ts.ScriptSnapshot.fromString(globalDtsContent);
  
  // Mutable state for the test file
  let testFileContent = "";
  let testFileVersion = 0;
  let testFileSnapshot: ts.IScriptSnapshot = ts.ScriptSnapshot.fromString("");

  const scriptFileNames = [TEST_FILE_PATH, GLOBAL_DTS_PATH];

  // Fast path check function (avoid normalize on every call)
  const isTestFile = (fileName: string) => 
    fileName === TEST_FILE_PATH || path.normalize(fileName) === NORMALIZED_TEST_FILE_PATH;
  const isGlobalDts = (fileName: string) => 
    fileName === GLOBAL_DTS_PATH || path.normalize(fileName) === NORMALIZED_GLOBAL_DTS_PATH;

  const host: ts.LanguageServiceHost = {
    getScriptFileNames: () => scriptFileNames,
    getScriptVersion: (fileName) => {
      if (isTestFile(fileName)) {
        return testFileVersion.toString();
      }
      // Other files don't change
      return "1";
    },
    getScriptSnapshot: (fileName) => {
      if (isTestFile(fileName)) {
        return testFileSnapshot;
      }
      if (isGlobalDts(fileName)) {
        return globalDtsSnapshot;
      }
      
      // Check cache first for disk files
      if (scriptSnapshotCache.has(fileName)) {
        return scriptSnapshotCache.get(fileName);
      }
      
      // Read from disk for other files (node_modules, etc.)
      let content = fileContentCache.get(fileName);
      if (content === undefined && !fileContentCache.has(fileName)) {
        content = ts.sys.readFile(fileName);
        fileContentCache.set(fileName, content);
      }
      
      if (content !== undefined) {
        const snapshot = ts.ScriptSnapshot.fromString(content);
        scriptSnapshotCache.set(fileName, snapshot);
        return snapshot;
      }
      return undefined;
    },
    getCurrentDirectory: () => CORE_PACKAGE_DIR,
    getCompilationSettings: () => compilerOptions,
    getDefaultLibFileName: () => {
      if (!cachedDefaultLibPath) {
        cachedDefaultLibPath = ts.getDefaultLibFilePath(compilerOptions);
      }
      return cachedDefaultLibPath;
    },
    fileExists: (fileName) => {
      if (isTestFile(fileName) || isGlobalDts(fileName)) {
        return true;
      }
      
      // Check cache
      if (fileExistsCache.has(fileName)) {
        return fileExistsCache.get(fileName)!;
      }
      
      const exists = ts.sys.fileExists(fileName);
      fileExistsCache.set(fileName, exists);
      return exists;
    },
    readFile: (fileName) => {
      if (isTestFile(fileName)) {
        return testFileContent;
      }
      if (isGlobalDts(fileName)) {
        return globalDtsContent;
      }
      
      // Check cache first
      if (fileContentCache.has(fileName)) {
        return fileContentCache.get(fileName);
      }
      
      const content = ts.sys.readFile(fileName);
      fileContentCache.set(fileName, content);
      return content;
    },
    readDirectory: ts.sys.readDirectory,
    directoryExists: (directoryName) => {
      // Check cache
      if (directoryExistsCache.has(directoryName)) {
        return directoryExistsCache.get(directoryName)!;
      }
      
      const exists = ts.sys.directoryExists(directoryName);
      directoryExistsCache.set(directoryName, exists);
      return exists;
    },
    getDirectories: ts.sys.getDirectories,
    realpath: ts.sys.realpath,
    resolveModuleNames: (moduleNames, containingFile, _reusedNames, _redirectedReference, options) => {
      return moduleNames.map((moduleName) => {
        // Create cache key
        const cacheKey = `${containingFile}|${moduleName}`;
        
        if (moduleResolutionCache.has(cacheKey)) {
          return moduleResolutionCache.get(cacheKey);
        }
        
        // Redirect $verter/types$ to @verter/types
        const actualModuleName = moduleName === "$verter/types$" ? "@verter/types" : moduleName;
        
        const result = ts.resolveModuleName(
          actualModuleName,
          containingFile,
          options,
          {
            fileExists: host.fileExists!,
            readFile: host.readFile!,
            directoryExists: host.directoryExists!,
            getDirectories: host.getDirectories!,
            realpath: ts.sys.realpath,
          }
        );
        
        moduleResolutionCache.set(cacheKey, result.resolvedModule);
        return result.resolvedModule;
      });
    },
  };

  cachedServiceHost = {
    host,
    setTestFileContent: (content: string) => {
      testFileContent = content;
      testFileVersion++;
      // Create new snapshot only when content changes
      testFileSnapshot = ts.ScriptSnapshot.fromString(content);
    },
    getTestFileVersion: () => testFileVersion,
  };

  return cachedServiceHost;
}

/**
 * Get or create the cached language service.
 */
function getLanguageService(): ts.LanguageService {
  if (cachedLanguageService) {
    return cachedLanguageService;
  }

  const serviceHost = getLanguageServiceHost()!;
  cachedLanguageService = ts.createLanguageService(
    serviceHost.host,
    ts.createDocumentRegistry()
  );

  return cachedLanguageService;
}

// ============================================================================
// Type String Extraction
// ============================================================================

/**
 * Options for creating a TypeScript program for type testing.
 */
export interface TypeProgramOptions {
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
 * Create a TypeScript program from code with proper module resolution.
 *
 * This uses a cached language service for efficient repeated type checking.
 * The language service caches all resolved modules and type information,
 * so only the test file content needs to be updated.
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
  const { additionalDefinitions = "" } = options;

  // Prepend additional definitions if provided
  const fullCode = additionalDefinitions ? additionalDefinitions + "\n" + code : code;

  // Update the test file content in the cached service host
  const serviceHost = getLanguageServiceHost()!;
  serviceHost.setTestFileContent(fullCode);

  // Get the language service and program
  const languageService = getLanguageService();
  const program = languageService.getProgram();
  
  if (!program) {
    throw new Error("Failed to get program from language service");
  }

  const checker = program.getTypeChecker();
  const sourceFile = program.getSourceFile(TEST_FILE_PATH);
  
  if (!sourceFile) {
    throw new Error(`Failed to get test source file from program. Test file: ${TEST_FILE_PATH}`);
  }

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
  kind: "type" | "variable" = "type"
): string | null {
  const { sourceFile, checker } = createTypeProgram(code);

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
 * @param prefix - The prefix used for type names (for resolving prefixed targets)
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

  const typeStr = getTypeString(code, targetName, kind);

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
 * @returns Array of TypeScript diagnostics
 */
export function getSemanticErrors(
  code: string
): ts.Diagnostic[] {
  const { program, sourceFile } = createTypeProgram(code);

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
 * @throws Error if TypeScript errors are found
 */
export function assertNoTypeErrors(
  code: string,
  testName?: string
): void {
  const errors = getSemanticErrors(code);

  if (errors.length > 0) {
    const errorMessages = formatDiagnostics(errors);
    const context = testName ? ` in test "${testName}"` : "";
    throw new Error(
      `TypeScript errors found${context}:\n${errorMessages}\n\nGenerated code:\n${code}`
    );
  }
}

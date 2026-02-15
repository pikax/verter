export type InputBuffer = string | Uint8Array;

/**
 * Severity level for a compilation diagnostic
 */
export type DiagnosticSeverity = "error" | "warning" | "info";

/**
 * A structured diagnostic message from the compiler
 */
export interface Diagnostic {
  /**
   * Severity level
   */
  severity: DiagnosticSeverity;
  /**
   * Vue-compatible error code (e.g., "XMissingEndTag", "XInvalidEndTag")
   */
  code: string;
  /**
   * Human-readable error message
   */
  message: string;
  /**
   * Optional source span start (byte offset into original input)
   */
  span_start?: number;
  /**
   * Optional source span end (byte offset into original input)
   */
  span_end?: number;
}

/**
 * Options for compiling Vue SFC to JavaScript
 */
export interface CodegenOptions {
  /**
   * The filename for source map generation
   */
  filename?: string;
  /**
   * Production mode - affects component ID generation and optimizations
   */
  is_production?: boolean;
  /**
   * Custom component ID (overrides auto-generation from filename)
   */
  component_id?: string;
  /**
   * Skip source map generation for faster compilation
   */
  skip_source_map?: boolean;
  /**
   * Custom interpolation delimiters [open, close]. Default: ["{{", "}}"]
   */
  delimiters?: [string, string];
  /**
   * Tag name prefixes treated as custom elements (skip component resolution).
   * E.g. ["ion-", "my-"] matches <ion-button>, <my-card>
   */
  custom_elements?: string[];
  /**
   * Whether to preserve HTML comments in output. Default: !isProduction
   */
  comments?: boolean;
  /**
   * Runtime module name to import helpers from. Default: "vue"
   */
  runtime_module_name?: string;
  /**
   * Hoist static VNodes/props to constants. Default: true
   */
  hoist_static?: boolean;
  /**
   * Whitespace handling: "condense" or "preserve". Default: "condense"
   */
  whitespace?: "condense" | "preserve";
  /**
   * Cache event handler expressions. Default: false
   */
  cache_handlers?: boolean;
  /**
   * Inline render function in setup(). Default: isProduction
   */
  inline?: boolean;
  /**
   * Indicates SFC uses :slotted() in styles. Default: true
   */
  slotted?: boolean;
}

/**
 * A compiled CSS style block from an SFC `<style>` tag
 */
export interface JsCompiledStyleBlock {
  /**
   * Compiled CSS code (scoped selectors, v-bind replacements, module hashing applied)
   */
  code: string;
  /**
   * Whether this style block is scoped
   */
  scoped: boolean;
  /**
   * Style language (css, scss, less, stylus)
   */
  lang?: string;
  /**
   * Whether this is a CSS module block
   */
  is_module: boolean;
  /**
   * CSS module class mappings (each entry is [original, hashed])
   */
  module_classes: [string, string][];
  /**
   * CSS processing diagnostics
   */
  errors: Diagnostic[];
}

/**
 * Result of compiling a Vue SFC
 */
export interface CodegenResult {
  /**
   * The transformed code
   */
  code: string;
  /**
   * The source map as JSON string
   */
  source_map: string;
  /**
   * The transformed code with inline source map appended
   */
  code_with_source_map: string;
  /**
   * Compiled CSS blocks from `<style>` tags
   */
  styles: JsCompiledStyleBlock[];
  /**
   * Scope ID for scoped styles (e.g., "data-v-a4f2eed6"). Empty if no scoped styles.
   */
  scope_id: string;
  /**
   * Compilation diagnostics (errors, warnings, info)
   */
  errors: Diagnostic[];
  /**
   * Time taken for the Rust pipeline in milliseconds
   */
  duration_ms: number;
}

/**
 * Options for compiling Vue SFC for Vite plugin usage
 */
export interface ViteCodegenOptions {
  /**
   * The filename for source map generation
   */
  filename?: string;
  /**
   * SSR mode
   */
  ssr?: boolean;
  /**
   * Production mode
   */
  is_production?: boolean;
  /**
   * Custom component ID
   */
  component_id?: string;
  /**
   * Whether to generate source maps
   */
  sourcemap?: boolean;
}

/**
 * An import statement in a block's output (with UTF-16 offsets for JS)
 */
export interface JsBlockImport {
  /**
   * Import source (e.g., "vue")
   */
  source: string;
  /**
   * Specifier strings (e.g., ["openBlock as _openBlock", ...])
   */
  specifiers: string[];
  /**
   * UTF-16 code unit offset of import start in block's code
   */
  start_utf16: number;
  /**
   * UTF-16 code unit offset of import end in block's code
   */
  end_utf16: number;
}

/**
 * Output block with code, source map, and import metadata (UTF-16 offsets for JS)
 */
export interface JsBlockOutput {
  /**
   * Generated code for this block
   */
  code: string;
  /**
   * Source map as JSON string
   */
  source_map?: string;
  /**
   * Import statements with UTF-16 offsets
   */
  imports: JsBlockImport[];
  /**
   * UTF-16 code unit offset where non-import code begins
   */
  body_start_utf16: number;
}

/**
 * Style block in a Vue SFC
 */
export interface JsStyleBlock {
  /**
   * Processed CSS content
   */
  code: string;
  /**
   * Source map for CSS transformations
   */
  source_map?: string;
  /**
   * Is scoped style
   */
  scoped: boolean;
  /**
   * Is CSS module
   */
  is_module: boolean;
  /**
   * Language (css, scss, less)
   */
  lang?: string;
  /**
   * Module name (e.g., "$style")
   */
  module_name?: string;
  /**
   * CSS module class mappings (original -> hashed)
   */
  module_classes: string[][];
}

/**
 * Result of compiling a Vue SFC for Vite
 */
export interface ViteCodegenResult {
  /**
   * Script block (component definition)
   */
  script?: JsBlockOutput;
  /**
   * Template block (render function)
   */
  template?: JsBlockOutput;
  /**
   * Style blocks
   */
  styles: JsStyleBlock[];
  /**
   * Whether the SFC has a default export (script setup or script with export default)
   */
  has_default_export: boolean;
  /**
   * Build time in milliseconds
   */
  duration_ms: number;
}

/**
 * Compile a Vue SFC to JavaScript.
 *
 * @param input - The Vue SFC source code (string or Buffer)
 * @param options - Optional compilation options
 * @returns The compiled result with code, source map, and code with inline source map
 */
export declare function compile(input: InputBuffer, options?: CodegenOptions): CodegenResult;

/**
 * Compile a Vue SFC to JavaScript (synchronous).
 *
 * @param input - The Vue SFC source code (string or Buffer)
 * @param options - Optional compilation options
 * @returns The compiled result with code, source map, and code with inline source map
 */
export declare function compileSync(input: InputBuffer, options?: CodegenOptions): CodegenResult;

/**
 * Compile a Vue SFC for Vite plugin usage.
 *
 * Returns split blocks (script, template, styles) for virtual module serving.
 * Each block has its own code, source map, and import metadata with UTF-16 offsets.
 *
 * @param input - The Vue SFC source code (string or Buffer)
 * @param options - Optional compilation options
 * @returns Compiled result with split blocks for virtual modules
 */
export declare function compileForVite(
  input: InputBuffer,
  options?: ViteCodegenOptions,
): ViteCodegenResult;

/**
 * Compile a Vue SFC for Vite plugin usage (synchronous).
 *
 * @param input - The Vue SFC source code (string or Buffer)
 * @param options - Optional compilation options
 * @returns Compiled result with split blocks for virtual modules
 */
export declare function compileForViteSync(
  input: InputBuffer,
  options?: ViteCodegenOptions,
): ViteCodegenResult;

// =============================================================================
// Standalone CSS Style Processing (for preprocessed CSS from Vite plugin)
// =============================================================================

/**
 * Options for processing a CSS style block
 */
export interface ProcessStyleOptions {
  /**
   * Scope ID string (e.g., "a4f2eed6")
   */
  scope_id: string;
  /**
   * Whether this style block is scoped
   */
  scoped?: boolean;
  /**
   * Whether this is a CSS module block
   */
  is_module?: boolean;
  /**
   * Custom module name (None = "$style")
   */
  module_name?: string;
  /**
   * Source filename for source map generation
   */
  filename?: string;
  /**
   * Whether to generate source maps
   */
  sourcemap?: boolean;
}

/**
 * A v-bind() expression that was replaced with a CSS variable
 */
export interface ProcessStyleVBind {
  /**
   * The original expression text (e.g., "color" or "theme.color")
   */
  expression: string;
  /**
   * The generated CSS variable name (e.g., "--a4f2eed6-color")
   */
  var_name: string;
}

/**
 * Result of processing a CSS style block
 */
export interface ProcessStyleResult {
  /**
   * Transformed CSS code
   */
  code: string;
  /**
   * Source map as JSON string (if sourcemap was requested)
   */
  source_map?: string;
  /**
   * CSS module class mappings (original -> hashed)
   */
  module_classes: string[][];
  /**
   * v-bind() expressions found and replaced
   */
  v_bind_vars: ProcessStyleVBind[];
}

/**
 * Process a CSS style block: apply scoping, CSS modules, and v-bind replacement.
 *
 * Called by the Vite plugin after preprocessing SCSS/Less/Stylus to valid CSS.
 * For plain CSS blocks, the Rust compiler handles this inline during compileForVite().
 *
 * @param css - Valid CSS string (already preprocessed if originally SCSS/Less/etc.)
 * @param options - Processing options (scope ID, scoped, modules, etc.)
 * @returns Processed CSS with scoping/modules applied, plus v-bind metadata
 */
export declare function processStyle(
  css: string,
  options: ProcessStyleOptions,
): ProcessStyleResult;

// =============================================================================
// Standalone TypeScript Stripping
// =============================================================================

/**
 * Result of stripping TypeScript syntax
 */
export interface StripTypesResult {
  /**
   * The JavaScript output with TypeScript syntax removed
   */
  code: string;
  /**
   * Any parse errors encountered
   */
  errors: string[];
}

/**
 * Strip TypeScript syntax from a standalone .ts/.tsx file.
 *
 * Removes type annotations, interfaces, type aliases, and converts enums to JavaScript.
 *
 * @param source - The TypeScript source code (string or Buffer)
 * @returns The stripped JavaScript code and any parse errors
 */
export declare function stripTypes(source: InputBuffer): StripTypesResult;

export {};

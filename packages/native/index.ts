export type InputBuffer = string | Uint8Array;

/**
 * Feature flags for Vue compilation
 */
export interface FeatureFlags {
  /**
   * Enable Options API support (default: true)
   */
  options_api?: boolean;
  /**
   * Enable reactive destructure for defineProps (default: true)
   */
  props_destructure?: boolean;
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
   * Whether to include source content in the source map
   */
  include_source_content?: boolean;
  /**
   * SSR mode
   */
  ssr?: boolean;
  /**
   * Production mode - affects component ID generation and optimizations
   */
  is_production?: boolean;
  /**
   * Custom component ID (overrides auto-generation from filename)
   */
  component_id?: string;
  /**
   * Feature flags for codegen
   */
  features?: FeatureFlags;
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
export declare function compile(
  input: InputBuffer,
  options?: CodegenOptions
): CodegenResult;

/**
 * Synchronous version of compile (same as compile, kept for API compatibility)
 *
 * @param input - The Vue SFC source code (string or Buffer)
 * @param options - Optional compilation options
 * @returns The compiled result with code, source map, and code with inline source map
 */
export declare function compileSync(
  input: InputBuffer,
  options?: CodegenOptions
): CodegenResult;

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
  options?: ViteCodegenOptions
): ViteCodegenResult;

export {};

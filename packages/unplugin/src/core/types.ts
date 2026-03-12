/**
 * A function that preprocesses a custom block's content.
 *
 * @param content - Raw content of the block.
 * @param lang - The `lang` attribute value (e.g., "yaml"), or undefined.
 * @param filename - The SFC filename (canonical ID).
 * @returns Preprocessed `{ code, sourceMap? }`, or `null` to skip.
 */
export type BlockPreprocessor = (
  content: string,
  lang: string | undefined,
  filename: string,
) =>
  | { code: string; sourceMap?: string }
  | Promise<{ code: string; sourceMap?: string }>
  | null;

export interface VerterPluginOptions {
  /** Custom component ID generator */
  componentId?: (filename: string, source: string, isProd: boolean) => string;
  /** File patterns to include (default: [/\.vue$/]). Matches @vitejs/plugin-vue `include` option. */
  include?: string | RegExp | (string | RegExp)[];
  /**
   * Pre-compile all `.vue` files during `buildStart` by scanning the project root.
   * Populates the host cache so that `transform()` calls get instant cache hits
   * when the source hasn't been modified by other plugins.
   * Third-party `.vue` files in `node_modules` are excluded from scanning.
   * @default false
   */
  preCompile?: boolean;
  /**
   * Cross-file analysis for prop constness optimization.
   * Requires `preCompile: true`. Only effective in production builds.
   * After all files are pre-compiled, analyzes the render tree to determine
   * which props are always passed constant values by every parent component.
   * Those props skip dynamic tracking in the compiled output.
   * @default false
   */
  crossFileOptimize?: boolean;
  /**
   * Template compiler options. Accepted for compatibility with `@vitejs/plugin-vue`
   * but currently only `isCustomElement` is forwarded.
   */
  template?: {
    compilerOptions?: {
      isCustomElement?: (tag: string) => boolean;
      [key: string]: unknown;
    };
    [key: string]: unknown;
  };
  /**
   * Custom block preprocessors keyed by block type.
   * Called when a custom block (e.g., `<i18n>`, `<docs>`) needs preprocessing.
   * @example { i18n: (content, lang) => ({ code: JSON.stringify(yaml.load(content)) }) }
   */
  customBlocks?: Record<string, BlockPreprocessor>;
  /**
   * SSR configuration for server/client build splitting.
   */
  ssr?: {
    /**
     * Override SSR auto-detection. When not set, the plugin auto-detects
     * SSR mode from `viteConfig.build.ssr`.
     */
    enabled?: boolean;
    /**
     * Component tag names to strip from SSR output.
     * These are replaced with empty comment nodes in server builds.
     * @example ["GoogleMap", "VideoPlayer"]
     */
    clientOnlyComponents?: string[];
    /**
     * Component tag names to strip from client output.
     * These are replaced with empty comment nodes in client builds.
     * @example ["ServerMetrics", "CrawlerHint"]
     */
    serverOnlyComponents?: string[];
    /**
     * Replace `import.meta.server` / `import.meta.client` / `import.meta.env.SSR`
     * with boolean literals for dead-code elimination.
     * @default true
     */
    deadCodeElimination?: boolean;
  };
}

/**
 * Alias for `VerterPluginOptions`.
 * Provides compatibility with code importing `Options` from `@vitejs/plugin-vue`.
 */
export type Options = VerterPluginOptions;

export type HmrStrategy = "vite" | "webpack" | "none";

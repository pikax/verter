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
}

/**
 * Alias for `VerterPluginOptions`.
 * Provides compatibility with code importing `Options` from `@vitejs/plugin-vue`.
 */
export type Options = VerterPluginOptions;

export type HmrStrategy = "vite" | "webpack" | "none";

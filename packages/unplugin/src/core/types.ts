export interface VerterPluginOptions {
  /** Custom component ID generator */
  componentId?: (filename: string, source: string, isProd: boolean) => string;
  /** File patterns to include (default: [/\.vue$/]). Matches @vitejs/plugin-vue `include` option. */
  include?: string | RegExp | (string | RegExp)[];
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

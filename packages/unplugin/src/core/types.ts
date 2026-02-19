import type {
  ViteCodegenOptions,
  ViteCodegenResult,
  ProcessStyleOptions,
  ProcessStyleResult,
  StripTypesResult,
} from "@verter/native";

export type { ViteCodegenOptions, ViteCodegenResult, ProcessStyleOptions, ProcessStyleResult, StripTypesResult };

export interface VerterPluginOptions {
  /** Custom component ID generator */
  componentId?: (filename: string, source: string, isProd: boolean) => string;
  /** File patterns to include (default: [/\.vue$/]). Matches @vitejs/plugin-vue `include` option. */
  include?: string | RegExp | (string | RegExp)[];
}

export interface Compiler {
  compileForVite(input: string, options?: ViteCodegenOptions): ViteCodegenResult;
  processStyle(css: string, options: ProcessStyleOptions): ProcessStyleResult;
  stripTypes(source: string): StripTypesResult;
}

export type HmrStrategy = "vite" | "webpack" | "none";

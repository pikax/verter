import type {
  ViteCodegenOptions,
  ViteCodegenResult,
  ProcessStyleOptions,
  ProcessStyleResult,
} from "@verter/native";

export type { ViteCodegenOptions, ViteCodegenResult, ProcessStyleOptions, ProcessStyleResult };

export interface VerterPluginOptions {
  /** Custom component ID generator */
  componentId?: (filename: string, source: string, isProd: boolean) => string;
}

export interface Compiler {
  compileForVite(input: string, options?: ViteCodegenOptions): ViteCodegenResult;
  processStyle(css: string, options: ProcessStyleOptions): ProcessStyleResult;
}

export type HmrStrategy = "vite" | "webpack" | "none";

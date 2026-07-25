// Types for the shipped-bundle esbuild configuration, so the artifact guard
// consumes the real build options with full type checking.
import type { BuildOptions } from "esbuild";

export declare const PRODUCTION_ENTRY_POINT: string;

export declare function productionBundleConfig(options?: {
  production?: boolean;
  sourcemap?: boolean;
}): BuildOptions;

// @ai-generated - Synthetic patch that augments the string-literal
// ambient module `"external-spec"` via `declare module` interface
// merging. Combined with `module_features_external.d.ts`, the merged
// `Config` interface includes both `base: string` and `extra: number`.

import "external-spec";

declare module "external-spec" {
  export interface Config {
    extra: number;
  }
}

// Force this file to be a module.
export {};

// @ai-generated - Synthetic patch module that augments
// `module_features_base.ts` via `declare module "./..."` interface
// merging.

import type { Plugin } from "./module_features_base";
import "./module_features_base";

declare module "./module_features_base" {
  interface Plugin {
    extra: number;
    label?: string;
  }
}

export function describePlugin(plugin: Plugin): string {
  return `${plugin.id}:${plugin.extra}:${plugin.label ?? "(no label)"}`;
}

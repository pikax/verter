// @ai-generated - V2 patch: contributes a `declare module` interface
// augmentation. After editing the patch from V1 to V2, the owner's
// published `Surface` must surface the merged shape
// `{ id: string; extra: number }`.

import type { Plugin } from "./cache_invalidation_aug_base";
import "./cache_invalidation_aug_base";

declare module "./cache_invalidation_aug_base" {
  interface Plugin {
    extra: number;
  }
}

export function noop(plugin: Plugin): Plugin {
  return plugin;
}

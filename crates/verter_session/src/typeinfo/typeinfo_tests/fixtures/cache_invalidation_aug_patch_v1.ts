// @ai-generated - V1 patch: a placeholder side-effect module that does
// NOT yet contribute an augmentation. The owner's V1 Surface should be
// the bare base shape `{ id: string }`.

import type { Plugin } from "./cache_invalidation_aug_base";

export function noop(plugin: Plugin): Plugin {
  return plugin;
}

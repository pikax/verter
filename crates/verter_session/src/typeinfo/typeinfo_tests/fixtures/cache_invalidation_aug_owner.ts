// @ai-generated - Owner imports `Plugin` from the base module and pulls
// in the patch file via a side-effect import. The patch is initially a
// no-op; the V2 patch contributes a `declare module` augmentation that
// must surface in the owner's published `Surface`.

import type { Plugin } from "./cache_invalidation_aug_base";
import "./cache_invalidation_aug_patch";

export type Surface = Plugin;

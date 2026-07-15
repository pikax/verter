/**
 * @verter/component-meta/compat — Volar-compatible API surface.
 *
 * Drop-in replacement for `vue-component-meta`. Consumers can swap imports:
 *
 * ```diff
 * - import { createChecker } from 'vue-component-meta'
 * + import { createChecker } from '@verter/component-meta/compat'
 * ```
 */

export {
  createChecker,
  createCheckerByJson,
  ComponentMetaChecker,
  mapPropMeta,
  mapEventMeta,
  mapSlotMeta,
  mapExposedMeta,
  mapComponentMeta,
} from "./checker.js";

export type { CheckerWorkspace } from "./checker.js";

export type {
  PropertyMeta,
  PropertyMetaSchema,
  MetaCheckerOptions,
  VolarComponentMeta as ComponentMeta,
  Tag,
} from "./types.js";

export { typeDescriptorToSchema, typeDescriptorToString } from "./schema.js";

export {
  projectDeclaredOnlyFromNativePayload,
  projectDeclaredOnlyNativeResult,
} from "./native-projection.js";

export { getMetaOrigin, walkOriginChain, findOriginNodesByKind } from "../origin-walk.js";
export type { OriginWalkResult, OriginChainEntry } from "../origin-walk.js";

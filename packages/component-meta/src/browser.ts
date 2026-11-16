/**
 * Browser-safe entry point for @verter/component-meta.
 *
 * Excludes Node.js runtime/session APIs so this can be used in browser
 * contexts like the playground without pulling in native bindings.
 */

// Core types
export type {
  ComponentMeta,
  PropMeta,
  EventMeta,
  SlotMeta,
  SlotBinding,
  ModelMeta,
  ExposedMeta,
  JsdocTag,
  ComponentPropUsage,
  ComponentUsage,
  TemplateRefMeta,
  ImportMeta,
  BindingMeta,
  VueApiCallMeta,
  StyleMeta,
  SelectorMeta,
  ComponentFlags,
} from "./types.js";

export * from "@verter/type-ir";

// Native type evaluation bridge
export { typeExprToDescriptor, buildEvaluatedTypeMap } from "./type-expr-bridge.js";
export type {
  NativeEvaluatedTypes,
  NativeEvaluatedField,
  NativeTypeExpr,
} from "./type-expr-bridge.js";

export { decodeComponentMetaPayload } from "./type-graph-decode.js";
export type { FactResult, FactCause, FactCauses } from "./facts.js";
export type { NativeComponentMetaResult, NativeComponentFacts } from "./native-component-meta.js";

export { nativeComponentMetaToComponentMeta } from "./native-component-meta.js";

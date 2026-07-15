/**
 * @verter/component-meta — session-backed Vue component metadata powered by native Verter.
 *
 * @example
 * ```ts
 * import { openComponentMetaSession } from '@verter/component-meta'
 *
 * const session = await openComponentMetaSession({ root: '.', tsconfig: './tsconfig.json' })
 * const meta = await session.getComponentMeta('./src/MyComponent.vue')
 * session.close()
 * ```
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
  PublicInstanceMeta,
  PublicInstanceMemberMeta,
  JsdocTag,
  ComponentPropUsage,
  ComponentUsage,
  ComponentBindingUsage,
  ComponentEventUsage,
  TemplateRefMeta,
  ImportMeta,
  BindingMeta,
  VueApiCallMeta,
  StyleMeta,
  SelectorMeta,
  ComponentFlags,
  // Fallthrough surface types
  AcceptedPropMeta,
  AcceptedEventMeta,
  AcceptedSurfaceCompleteness,
  MemberAvailability,
  MemberProvenance,
  InheritedSource,
  AcceptedPropKind,
  AcceptedEventKind,
  RootReachability,
  NoFallthroughReason,
  RootBranch,
  RootTargetRef,
  UnresolvedRootTargetReason,
  ConsumedRootBindings,
  FallthroughSurface,
  FallthroughBranch,
  FallthroughPropEntry,
  FallthroughEventEntry,
  GenericResolutionFailure,
  PartialBranchReason,
  BranchStatus,
  UnresolvedBranchReason,
  ResolvedRootStep,
} from "./types.js";

// Type IR — re-exported from @verter/type-ir for public-API stability.
export type {
  TypeDescriptor,
  PrimitiveName,
  PrimitiveType,
  LiteralType,
  UnionType,
  IntersectionType,
  ArrayType,
  TupleType,
  ObjectType,
  ObjectProperty,
  ObjectIndexSignature,
  FunctionType,
  FunctionParameter,
  TypeParameterType,
  EnumType,
  EnumMember,
  RefType,
  RecursiveRefType,
  RecursiveRefConditionalFrame,
  UnknownType,
} from "@verter/type-ir";

export {
  primitive,
  literal,
  union,
  intersection,
  array,
  tuple,
  object,
  func,
  typeParameter,
  ref,
  recursiveRef,
  unknown,
} from "@verter/type-ir";

// Native type evaluation bridge
export { typeExprToDescriptor, buildEvaluatedTypeMap } from "./type-expr-bridge.js";
export type {
  NativeEvaluatedTypes,
  NativeEvaluatedField,
  NativeTypeExpr,
} from "./type-expr-bridge.js";

// Native component-meta payload + projection helpers
export {
  nativeComponentMetaToComponentMeta,
  nativeTypeRegistryToMap,
} from "./native-component-meta.js";
export type {
  NativeComponentMetaResult,
  NativeComponentMetaResolution,
  NativeResolvedTypeMeta,
  NativePublicInstanceMeta,
  NativePublicInstanceMemberMeta,
  NativeResolvedMacroMeta,
  NativeResolvedTypeDeclaration,
  NativeResolvedNativeProp,
  NativeResolvedJsdocBlock,
  NativeResolvedJsdocTag,
  // Native leaf types for consumers that destructure NativeComponentMetaResult
  NativeJsdocTag,
  NativePropMeta,
  NativeEventMeta,
  NativeSlotMeta,
  NativeSlotBindingMeta,
  NativeModelMeta,
  NativeExposedMeta,
  NativeComponentFlags,
  NativeComponentUsage,
  NativeComponentPropUsage,
  NativeComponentVModelEntry,
  NativeTemplateRefMeta,
  NativeImportMeta,
  NativeImportBindingMeta,
  NativeBindingMeta,
  NativeVueApiCallMeta,
  NativeSelectorMeta,
  NativeStyleMeta,
  // Native fallthrough surface types
  NativeAcceptedPropMeta,
  NativeAcceptedEventMeta,
  NativeAcceptedSurfaceCompleteness,
  NativeMemberProvenance,
  NativeMemberAvailability,
  NativeInheritedSource,
  NativeRootReachability,
  NativeNoFallthroughReason,
  NativeRootBranch,
  NativeRootTargetRef,
  NativeUnresolvedRootTargetReason,
  NativeConsumedRootBindings,
  NativeFallthroughSurface,
  NativeFallthroughBranch,
  NativeFallthroughPropEntry,
  NativeFallthroughEventEntry,
  NativeGenericResolutionFailure,
  NativePartialBranchReason,
  NativeBranchStatus,
  NativeUnresolvedBranchReason,
  NativeResolvedRootStep,
  // Native origin graph types
  NativeOriginNode,
  NativeOriginEdge,
  NativeOriginGraph,
} from "./native-component-meta.js";

// Session-first project API
export {
  ComponentMetaSession,
  openComponentMetaSession,
  evictComponentMetaSession,
  shutdownMetaRuntime,
} from "./project.js";
export type { ComponentMetaSessionConfig } from "./project.js";

// Origin graph walk API
export { getMetaOrigin, walkOriginChain, findOriginNodesByKind } from "./origin-walk.js";
export type { OriginWalkResult, OriginChainEntry } from "./origin-walk.js";

// Semantic pipeline types (from verter_protocol via @verter/language-shared)
export type {
  ComponentRuntimeSchema,
  RuntimePropSchema,
  RuntimeModelSchema,
  RuntimeEventSchema,
  RuntimeSlotSchema,
  ComponentSurfaceDto,
  QueryResultDto,
} from "./semantic-bridge.js";

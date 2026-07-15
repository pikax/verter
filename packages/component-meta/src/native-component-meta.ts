import { basename } from "node:path";

import { typeExprToDescriptor } from "./type-expr-bridge.js";
import type {
  ComponentMeta,
  AcceptedPropMeta,
  AcceptedEventMeta,
  AcceptedSurfaceCompleteness,
  SfcBlocksMeta,
  SfcAttributeMeta,
  TemplateBlockMeta,
  ScriptBlockMeta,
  StyleBlockMeta,
  CustomBlockMeta,
  RootInfo,
  RootReachability,
  FallthroughSurface,
  FallthroughBranch,
  TypeExpansionMeta,
} from "./types.js";
import type { TypeDescriptor } from "@verter/type-ir";
import type { NativeTypeExprLike } from "./type-expr-bridge.js";

export interface NativeJsdocTag {
  name: string;
  text?: string;
}

export interface NativeExpansionDiagnostic {
  reason:
    | "budgetExceeded"
    | "projectionWorkLimit"
    | "connectedQueryDepthLimit"
    | "mappedDepthExceeded"
    | "unresolvedReference"
    | "indeterminateConditional"
    | "infiniteKeySpace"
    | "unsupportedOperator"
    | "conditionalContextTruncated"
    | "idempotentArm"
    | "cyclicReference"
    | "cyclicInstantiation"
    | "instantiationError"
    | "emptyUnionArm";
  context: string;
  propertyName?: string;
}

export interface NativeExpansionMetadata extends TypeExpansionMeta {
  diagnostics: NativeExpansionDiagnostic[];
}

export interface NativePropMeta {
  name: string;
  type: NativeTypeExprLike;
  typeExpansion?: NativeExpansionMetadata;
  rawType?: string;
  required: boolean;
  hasDefault: boolean;
  defaultValue?: string;
  description?: string;
  tags?: NativeJsdocTag[];
  /**
   * Producer fact: did the SFC author write this prop name explicitly as a
   * member of the `defineProps<T>()` type argument's own body (or its
   * directly-referenced interface's own body)? Distinguishes
   * author-declared names from names that arrived via heritage / utility-
   * type expansion. Consumed by
   * `@verter/component-meta/published-surface`'s `Refined` policy.
   *
   * Default `false` so a missing payload field (e.g. from an older
   * native build) does NOT silently mark every prop as declared.
   */
  declaredInMacroTypeArg?: boolean;
}

export interface NativeEventMeta {
  name: string;
  payload: NativeTypeExprLike;
  payloadExpansion?: NativeExpansionMetadata;
  rawSignature?: string;
  description?: string;
  tags?: NativeJsdocTag[];
}

export interface NativeSlotBindingMeta {
  name: string;
  type: NativeTypeExprLike;
  typeExpansion?: NativeExpansionMetadata;
  rawType?: string;
}

export interface NativeSlotMeta {
  name: string;
  isScoped: boolean;
  bindings: NativeSlotBindingMeta[];
  isRequired: boolean;
  returnType?: string;
  description?: string;
  tags?: NativeJsdocTag[];
}

export interface NativeModelMeta {
  name: string;
  type: NativeTypeExprLike;
}

export interface NativeExposedMeta {
  name: string;
  type: NativeTypeExprLike;
  typeExpansion?: NativeExpansionMetadata;
  description?: string;
  tags?: NativeJsdocTag[];
}

export interface NativePublicInstanceMeta {
  completeness: "exact" | "partial";
  members: NativePublicInstanceMemberMeta[];
}

export interface NativePublicInstanceMemberMeta {
  name: string;
  kind: "prop" | "slotContainer" | "exposed";
  type: NativeTypeExprLike;
  typeExpansion?: NativeExpansionMetadata;
  rawType?: string;
  description?: string;
  tags?: NativeJsdocTag[];
}

export interface NativeSfcBlocksMeta {
  template?: NativeTemplateBlockMeta;
  script?: NativeScriptBlockMeta;
  scriptSetup?: NativeScriptBlockMeta;
  styles: NativeStyleBlockMeta[];
  custom: NativeCustomBlockMeta[];
}

export interface NativeSfcAttributeMeta {
  name: string;
  value?: string;
}

export interface NativeTemplateBlockMeta {
  lang?: string;
  src?: string;
  attributes: NativeSfcAttributeMeta[];
}

export interface NativeScriptBlockMeta {
  lang?: string;
  src?: string;
  generic?: string;
  attrsType?: string;
  attributes: NativeSfcAttributeMeta[];
}

export interface NativeStyleBlockMeta {
  index: number;
  lang?: string;
  src?: string;
  scoped: boolean;
  isModule: boolean;
  moduleName?: string;
  attributes: NativeSfcAttributeMeta[];
}

export interface NativeCustomBlockMeta {
  index: number;
  blockType: string;
  lang?: string;
  src?: string;
  attributes: NativeSfcAttributeMeta[];
}

export interface NativeResolvedTypeMeta {
  name: string;
  /**
   * Expanded lightweight native type.
   * This is what compat/public mapping consumes.
   */
  type: NativeTypeExprLike;
  typeExpansion?: NativeExpansionMetadata;
  /**
   * Pre-expansion source form retained for native callers that need to inspect
   * what the expanded type came from.
   */
  rawType?: string;
  declaration?: NativeResolvedTypeDeclaration;
}

export interface NativeResolvedTypeDeclaration {
  requestedName: string;
  resolvedName: string;
  canonicalSource: string;
  spanStart: number;
  spanEnd: number;
  kind: "interface" | "typeAlias" | "class" | "unknown";
  text?: string;
}

export interface NativeResolvedNativeProp {
  name: string;
  isOptional: boolean;
  typeAnnotation?: string;
  visibility: "public" | "protected" | "private";
  spanStart: number;
  spanEnd: number;
}

export interface NativeResolvedJsdocTag extends NativeJsdocTag {
  rawType?: string;
  subjectName?: string;
  resolvedType?: NativeTypeExprLike;
}

export interface NativeResolvedJsdocBlock {
  description?: string;
  tags?: NativeResolvedJsdocTag[];
}

export interface NativeResolvedMacroMeta {
  macroIndex: number;
  macroKind: string;
  typeName: string;
  importSource: string;
  declaration: NativeResolvedTypeDeclaration;
  nativeProps?: NativeResolvedNativeProp[];
  jsdoc?: NativeResolvedJsdocBlock;
}

export interface NativeComponentMetaResolution {
  mode: "type" | "expanded";
  macros: NativeResolvedMacroMeta[];
}

export interface NativeComponentPropUsage {
  name: string;
  isBound: boolean;
  constness: "const" | "dynamic" | "unknown";
  expression?: string;
  referencedBindings?: string[];
  fromSpread?: boolean;
  isShorthand?: boolean;
}

export interface NativeComponentVModelEntry {
  bindingName: string;
}

export interface NativeComponentBindingUsage {
  name: string;
  modifiers: string[];
}

export interface NativeComponentEventUsage {
  name: string;
  handlerExpression?: string;
  isInline: boolean;
  modifiers: string[];
}

export interface NativeComponentUsage {
  name: string;
  importSource?: string;
  isDynamic: boolean;
  props: NativeComponentPropUsage[];
  hasSpread?: boolean;
  slotsUsed: string[];
  staticClasses: string[];
  hasDynamicClass: boolean;
  vModels: string[];
  vModelEntries?: NativeComponentVModelEntry[];
  /** Framework-neutral two-way bindings (the Svelte `bind:` family). Empty for Vue. */
  bindings?: NativeComponentBindingUsage[];
  /** Framework-neutral events (the legacy Svelte `on:` directive only — a plain `on*` attr is a prop). Empty for Vue. */
  events?: NativeComponentEventUsage[];
}

export interface NativeTemplateRefMeta {
  name: string;
  isDynamic: boolean;
  targetTag: string;
}

export interface NativeImportBindingMeta {
  name: string;
  kind: "named" | "default" | "namespace";
  importedName?: string | null;
  isTypeOnly: boolean;
}

export interface NativeImportMeta {
  source: string;
  isTypeOnly: boolean;
  bindings: NativeImportBindingMeta[];
}

export interface NativeBindingMeta {
  name: string;
  kind: "const" | "let" | "var" | "function" | "asyncFunction" | "class";
  reactivityKind: "none" | "ref" | "reactive" | "computed" | "maybeRef" | "mutable";
  typeAnnotation?: string;
  usedInTemplate: boolean;
  usedInStyle: boolean;
}

export interface NativeVueApiCallMeta {
  api: string;
  argValue?: string;
}

export interface NativeSelectorMeta {
  text: string;
  specificity: [number, number, number];
}

export interface NativeStyleMeta {
  lang: string;
  scoped: boolean;
  isModule: boolean;
  moduleName?: string;
  classes: string[];
  ids: string[];
  customProperties: string[];
  vBinds: string[];
  selectors: NativeSelectorMeta[];
}

export interface NativeComponentFlags {
  asyncSetup: boolean;
  hasReactiveState: boolean;
  hasComputed: boolean;
  hasWatchers: boolean;
  hasLifecycleHooks: boolean;
  hasProvide: boolean;
  hasInject: boolean;
  hasInheritAttrsFalse: boolean;
  hasStoreUsage: boolean;
}

// ── Fallthrough surface native types ─────────────────────────────

export interface NativeAcceptedPropMeta {
  name: string;
  type: NativeTypeExprLike;
  rawType?: string;
  required: boolean;
  provenance: NativeMemberProvenance;
  availability: NativeMemberAvailability;
  kind: "declaredProp" | "attr";
}

export interface NativeAcceptedEventMeta {
  name: string;
  payload: NativeTypeExprLike;
  rawSignature?: string;
  provenance: NativeMemberProvenance;
  availability: NativeMemberAvailability;
  kind: "declaredEmit" | "listener";
}

export type NativeMemberProvenance =
  | { kind: "declared" }
  | { kind: "inherited"; sources: NativeInheritedSource[] };

export type NativeInheritedSource =
  | { kind: "nativeTag"; tag: string }
  | { kind: "component"; canonicalId: string };

export type NativeMemberAvailability =
  | { kind: "always" }
  | { kind: "conditional"; branchKeys: string[] };

export type NativeAcceptedSurfaceCompleteness = "exact" | "lowerBound";

export type NativeRootReachability =
  | { kind: "noFallthrough"; reason: NativeNoFallthroughReason }
  | { kind: "branches"; branches: NativeRootBranch[] };

export interface NativeRootInfo {
  kind: "none" | "single" | "conditional" | "multiple";
  reason?: NativeNoFallthroughReason;
  targets: NativeRootTargetRef[];
}

export type NativeNoFallthroughReason =
  | "inheritAttrsFalse"
  | "multiRoot"
  | "branchNotSingleRoot"
  | "rootVFor"
  | "noTemplate"
  | "emptyTemplate"
  | "textOrInterpolationRoot";

export interface NativeRootBranch {
  branchIndex: number;
  conditionText?: string;
  target: NativeRootTargetRef;
  consumed: NativeConsumedRootBindings;
  hasUnknownSpread: boolean;
}

export type NativeRootTargetRef =
  | { kind: "nativeElement"; elementIndex: number; tag: string }
  | { kind: "dynamicComponentUsage"; elementIndex: number; usageIndex: number }
  | {
      kind: "componentUsage";
      elementIndex: number;
      usageIndex: number;
      name: string;
      importSource?: string;
    }
  | {
      kind: "unresolvedTarget";
      elementIndex: number;
      tag: string;
      reason: NativeUnresolvedRootTargetReason;
    };

export type NativeUnresolvedRootTargetReason =
  | { kind: "dynamicComponentIs" }
  | { kind: "slotOutlet" }
  | { kind: "unsupportedBuiltin"; tag: string }
  | { kind: "missingUsageLink" }
  | { kind: "unresolvedImport" }
  | { kind: "unknownRootTarget" };

export interface NativeConsumedRootBindings {
  attrs: string[];
  listeners: string[];
  hasDynamicAttrName: boolean;
  hasDynamicListenerName: boolean;
}

export type NativeFallthroughSurface =
  | { kind: "none"; reason: NativeNoFallthroughReason }
  | { kind: "branches"; branches: NativeFallthroughBranch[] };

export type NativeGenericResolutionFailure =
  | "spreadInput"
  | "dynamicKey"
  | "missingType"
  | "unsupportedExpression"
  | "missingUsageLink"
  | "unresolvedChildGenericSurface";

export type NativePartialBranchReason =
  | { kind: "dynamicAttrName" }
  | { kind: "dynamicListenerName" }
  | { kind: "unknownSpread" }
  | { kind: "genericResolution"; failure: NativeGenericResolutionFailure };

export type NativeUnresolvedBranchReason =
  | { kind: "cycle"; canonicalId: string }
  | { kind: "dynamicComponentIs" }
  | { kind: "childResolutionFailed" }
  | { kind: "unresolvedChildImport"; importSource?: string }
  | { kind: "rootTarget"; reason: NativeUnresolvedRootTargetReason }
  | { kind: "genericResolution"; failure: NativeGenericResolutionFailure };

export interface NativeFallthroughPropEntry {
  name: string;
  type: NativeTypeExprLike;
  rawType?: string;
  sources: NativeInheritedSource[];
}

export interface NativeFallthroughEventEntry {
  name: string;
  payload: NativeTypeExprLike;
  rawSignature?: string;
  sources: NativeInheritedSource[];
}

export type NativeBranchStatus =
  | { kind: "resolved" }
  | { kind: "partiallyUnresolved"; reasons: NativePartialBranchReason[] }
  | { kind: "unresolved"; reason: NativeUnresolvedBranchReason };

export type NativeResolvedRootStep =
  | { kind: "nativeTag"; tag: string }
  | { kind: "component"; canonicalId: string; componentName: string }
  | { kind: "unresolved"; tag: string; reason: NativeUnresolvedBranchReason };

export interface NativeFallthroughBranch {
  branchKey: string;
  conditionText?: string;
  props: NativeFallthroughPropEntry[];
  events: NativeFallthroughEventEntry[];
  rootChain: NativeResolvedRootStep[];
  status: NativeBranchStatus;
}

// ── Top-level native result ─────────────────────────────────────

export interface NativeOriginNode {
  id: number;
  kind: string;
  label?: string;
}

export interface NativeOriginEdge {
  source: number;
  target: number;
  kind: string;
  metaIndex?: number;
}

export interface NativeOriginGraph {
  nodes: NativeOriginNode[];
  edges: NativeOriginEdge[];
  metaStrings: string[];
}

export interface NativeComponentMetaResult {
  props: NativePropMeta[];
  events: NativeEventMeta[];
  slots: NativeSlotMeta[];
  models: NativeModelMeta[];
  exposed: NativeExposedMeta[];
  publicInstance?: NativePublicInstanceMeta;
  sfcBlocks?: NativeSfcBlocksMeta;
  typeRegistry?: NativeResolvedTypeMeta[];
  components: NativeComponentUsage[];
  templateRefs: NativeTemplateRefMeta[];
  imports: NativeImportMeta[];
  bindings: NativeBindingMeta[];
  vueApiCalls: NativeVueApiCallMeta[];
  styles: NativeStyleMeta[];
  flags: NativeComponentFlags;
  acceptedProps: NativeAcceptedPropMeta[];
  acceptedEvents: NativeAcceptedEventMeta[];
  acceptedSurfaceCompleteness: NativeAcceptedSurfaceCompleteness;
  rootInfo?: NativeRootInfo;
  rootReachability: NativeRootReachability;
  fallthroughSurface: NativeFallthroughSurface;
  macroExpansionDiagnostics?: NativeMacroExpansionDiagnostics[];
  optionsApi: boolean;
  filePath: string;
  resolution?: NativeComponentMetaResolution;
  origin?: NativeOriginGraph;
}

export interface NativeMacroExpansionDiagnostics {
  macroKind: "defineProps" | "defineEmits" | "defineSlots";
  macroIndex: number;
  exactness: NativeExpansionMetadata["exactness"];
  executionStatus: NativeExpansionMetadata["executionStatus"];
  diagnostics: NativeExpansionDiagnostic[];
}

function deriveComponentName(filePath: string): string {
  return basename(filePath).replace(/\.[^.]+$/, "") || "AnonymousComponent";
}

export function nativeComponentMetaToComponentMeta(meta: NativeComponentMetaResult): ComponentMeta {
  const nativeRegistry = buildNativeTypeRegistry(meta);
  return {
    filePath: meta.filePath,
    componentName: deriveComponentName(meta.filePath),
    optionsApi: meta.optionsApi,
    props: meta.props.map((prop) => ({
      name: prop.name,
      type: typeExprToDescriptor(prop.type, nativeRegistry),
      ...(prop.typeExpansion !== undefined ? { typeExpansion: prop.typeExpansion } : {}),
      required: prop.required,
      hasDefault: prop.hasDefault,
      ...(prop.rawType !== undefined ? { rawType: prop.rawType } : {}),
      ...(prop.defaultValue !== undefined ? { default: prop.defaultValue } : {}),
      ...(prop.description !== undefined ? { description: prop.description } : {}),
      ...(prop.tags?.length ? { tags: prop.tags } : {}),
      // Forward-compat coerce — see `NativePropMeta.declaredInMacroTypeArg`
      // JSDoc above. The field is optional on the native sidecar type
      // because older native builds (predating the producer fact on
      // `PropMeta` proto field 10) emit payloads without it; missing is
      // correctly `false` (matching the "drop unless explicitly
      // declared" semantics that the Refined policy enforces). This is
      // legitimate forward-compat.
      declaredInMacroTypeArg: Boolean(prop.declaredInMacroTypeArg),
    })),
    events: meta.events.map((event) => ({
      name: event.name,
      payload: typeExprToDescriptor(event.payload, nativeRegistry),
      ...(event.payloadExpansion !== undefined ? { payloadExpansion: event.payloadExpansion } : {}),
      hasValidator: false,
      isDeclared: true,
      ...(event.rawSignature !== undefined ? { rawSignature: event.rawSignature } : {}),
      ...(event.description !== undefined ? { description: event.description } : {}),
      ...(event.tags?.length ? { tags: event.tags } : {}),
    })),
    slots: meta.slots.map((slot) => ({
      name: slot.name,
      isScoped: slot.isScoped,
      bindings: slot.bindings.map((binding) => ({
        name: binding.name,
        type: typeExprToDescriptor(binding.type, nativeRegistry),
        ...(binding.typeExpansion !== undefined ? { typeExpansion: binding.typeExpansion } : {}),
        ...(binding.rawType !== undefined ? { rawType: binding.rawType } : {}),
      })),
      isRequired: slot.isRequired,
      ...(slot.returnType !== undefined ? { returnType: slot.returnType } : {}),
      ...(slot.description !== undefined ? { description: slot.description } : {}),
      ...(slot.tags?.length ? { tags: slot.tags } : {}),
    })),
    models: meta.models.map((model) => ({
      name: model.name,
      type: typeExprToDescriptor(model.type, nativeRegistry),
    })),
    exposed: meta.exposed.map((exposed) => ({
      name: exposed.name,
      type: typeExprToDescriptor(exposed.type, nativeRegistry),
      ...(exposed.typeExpansion !== undefined ? { typeExpansion: exposed.typeExpansion } : {}),
      ...(exposed.description !== undefined ? { description: exposed.description } : {}),
      ...(exposed.tags?.length ? { tags: exposed.tags } : {}),
    })),
    ...(meta.publicInstance !== undefined
      ? {
          publicInstance: {
            completeness: meta.publicInstance.completeness,
            members: meta.publicInstance.members.map((member) => ({
              name: member.name,
              kind: member.kind,
              type: typeExprToDescriptor(member.type, nativeRegistry),
              ...(member.typeExpansion !== undefined
                ? { typeExpansion: member.typeExpansion }
                : {}),
              ...(member.rawType !== undefined ? { rawType: member.rawType } : {}),
              ...(member.description !== undefined ? { description: member.description } : {}),
              ...(member.tags?.length ? { tags: member.tags } : {}),
            })),
          },
        }
      : {}),
    ...(meta.sfcBlocks !== undefined
      ? {
          sfcBlocks: mapNativeSfcBlocks(meta.sfcBlocks),
        }
      : {}),
    components: meta.components.map((component) => ({
      name: component.name,
      ...(component.importSource !== undefined ? { importSource: component.importSource } : {}),
      isDynamic: component.isDynamic,
      props: component.props.map((prop) => ({
        name: prop.name,
        isBound: prop.isBound,
        constness: prop.constness,
        ...(prop.expression !== undefined ? { expression: prop.expression } : {}),
        referencedBindings: [...(prop.referencedBindings ?? [])],
        fromSpread: prop.fromSpread ?? false,
        isShorthand: prop.isShorthand ?? false,
      })),
      hasSpread: component.hasSpread ?? false,
      slotsUsed: [...component.slotsUsed],
      staticClasses: [...component.staticClasses],
      hasDynamicClass: component.hasDynamicClass,
      vModels: [...component.vModels],
      vModelEntries: (
        component.vModelEntries ?? component.vModels.map((bindingName) => ({ bindingName }))
      ).map((entry) => ({
        bindingName: entry.bindingName,
      })),
      bindings: (component.bindings ?? []).map((binding) => ({
        name: binding.name,
        modifiers: [...binding.modifiers],
      })),
      events: (component.events ?? []).map((event) => ({
        name: event.name,
        ...(event.handlerExpression !== undefined
          ? { handlerExpression: event.handlerExpression }
          : {}),
        isInline: event.isInline,
        modifiers: [...event.modifiers],
      })),
    })),
    templateRefs: meta.templateRefs.map((templateRef) => ({
      name: templateRef.name,
      isDynamic: templateRef.isDynamic,
      targetTag: templateRef.targetTag,
    })),
    imports: meta.imports.map((imp) => ({
      source: imp.source,
      isTypeOnly: imp.isTypeOnly,
      bindings: imp.bindings.map((binding) => ({
        name: binding.name,
        kind: binding.kind,
        importedName: binding.importedName ?? null,
        isTypeOnly: binding.isTypeOnly,
      })),
    })),
    bindings: meta.bindings.map((binding) => ({
      name: binding.name,
      kind: binding.kind,
      reactivityKind: binding.reactivityKind,
      ...(binding.typeAnnotation !== undefined ? { typeAnnotation: binding.typeAnnotation } : {}),
      usedInTemplate: binding.usedInTemplate,
      usedInStyle: binding.usedInStyle,
    })),
    vueApiCalls: meta.vueApiCalls.map((call) => ({
      api: call.api,
      ...(call.argValue !== undefined ? { argValue: call.argValue } : {}),
    })),
    styles: meta.styles.map((style) => ({
      lang: style.lang,
      scoped: style.scoped,
      isModule: style.isModule,
      ...(style.moduleName !== undefined ? { moduleName: style.moduleName } : {}),
      classes: [...style.classes],
      ids: [...style.ids],
      customProperties: [...style.customProperties],
      vBinds: [...style.vBinds],
      selectors: style.selectors.map((selector) => ({
        text: selector.text,
        specificity: selector.specificity,
      })),
    })),
    acceptedProps: mapNativeAcceptedProps(meta.acceptedProps, nativeRegistry),
    acceptedEvents: mapNativeAcceptedEvents(meta.acceptedEvents, nativeRegistry),
    acceptedSurfaceCompleteness: meta.acceptedSurfaceCompleteness as AcceptedSurfaceCompleteness,
    rootInfo: mapNativeRootInfo(meta.rootInfo ?? deriveRootInfo(meta.rootReachability)),
    rootReachability: meta.rootReachability as RootReachability,
    fallthroughSurface: mapNativeFallthroughSurface(meta.fallthroughSurface, nativeRegistry),
    flags: meta.flags,
    ...(meta.origin !== undefined ? { origin: meta.origin } : {}),
  };
}

function mapNativeRootInfo(info: NativeRootInfo): RootInfo {
  return {
    kind: info.kind,
    ...(info.reason !== undefined ? { reason: info.reason } : {}),
    targets: info.targets.map((target) => ({ ...target })),
  };
}

function mapNativeSfcBlocks(blocks: NativeSfcBlocksMeta): SfcBlocksMeta {
  return {
    ...(blocks.template !== undefined ? { template: mapNativeTemplateBlock(blocks.template) } : {}),
    ...(blocks.script !== undefined ? { script: mapNativeScriptBlock(blocks.script) } : {}),
    ...(blocks.scriptSetup !== undefined
      ? { scriptSetup: mapNativeScriptBlock(blocks.scriptSetup) }
      : {}),
    styles: blocks.styles.map((style) => mapNativeStyleBlock(style)),
    custom: blocks.custom.map((block) => mapNativeCustomBlock(block)),
  };
}

function mapNativeSfcAttributes(attributes: NativeSfcAttributeMeta[]): SfcAttributeMeta[] {
  return attributes.map((attribute) => ({
    name: attribute.name,
    ...(attribute.value !== undefined ? { value: attribute.value } : {}),
  }));
}

function mapNativeTemplateBlock(block: NativeTemplateBlockMeta): TemplateBlockMeta {
  return {
    ...(block.lang !== undefined ? { lang: block.lang } : {}),
    ...(block.src !== undefined ? { src: block.src } : {}),
    attributes: mapNativeSfcAttributes(block.attributes),
  };
}

function mapNativeScriptBlock(block: NativeScriptBlockMeta): ScriptBlockMeta {
  return {
    ...(block.lang !== undefined ? { lang: block.lang } : {}),
    ...(block.src !== undefined ? { src: block.src } : {}),
    ...(block.generic !== undefined ? { generic: block.generic } : {}),
    ...(block.attrsType !== undefined ? { attrsType: block.attrsType } : {}),
    attributes: mapNativeSfcAttributes(block.attributes),
  };
}

function mapNativeStyleBlock(block: NativeStyleBlockMeta): StyleBlockMeta {
  return {
    index: block.index,
    ...(block.lang !== undefined ? { lang: block.lang } : {}),
    ...(block.src !== undefined ? { src: block.src } : {}),
    scoped: block.scoped,
    isModule: block.isModule,
    ...(block.moduleName !== undefined ? { moduleName: block.moduleName } : {}),
    attributes: mapNativeSfcAttributes(block.attributes),
  };
}

function mapNativeCustomBlock(block: NativeCustomBlockMeta): CustomBlockMeta {
  return {
    index: block.index,
    blockType: block.blockType,
    ...(block.lang !== undefined ? { lang: block.lang } : {}),
    ...(block.src !== undefined ? { src: block.src } : {}),
    attributes: mapNativeSfcAttributes(block.attributes),
  };
}

function deriveRootInfo(reachability: NativeRootReachability): NativeRootInfo {
  if (reachability.kind === "branches") {
    return {
      kind: reachability.branches.length <= 1 ? "single" : "conditional",
      targets: reachability.branches.map((branch) => ({ ...branch.target })),
    };
  }

  const kind =
    reachability.reason === "multiRoot" || reachability.reason === "rootVFor"
      ? "multiple"
      : reachability.reason === "branchNotSingleRoot"
        ? "conditional"
        : "none";
  return {
    kind,
    reason: reachability.reason,
    targets: [],
  };
}

function mapNativeAcceptedProps(
  props: NativeAcceptedPropMeta[],
  nativeRegistry?: Map<string, NativeTypeExprLike>,
): AcceptedPropMeta[] {
  return props.map((p) => ({
    name: p.name,
    type: typeExprToDescriptor(p.type, nativeRegistry),
    ...(p.rawType !== undefined ? { rawType: p.rawType } : {}),
    required: p.required,
    provenance: p.provenance,
    availability: p.availability,
    kind: p.kind,
  }));
}

function mapNativeAcceptedEvents(
  events: NativeAcceptedEventMeta[],
  nativeRegistry?: Map<string, NativeTypeExprLike>,
): AcceptedEventMeta[] {
  return events.map((e) => ({
    name: e.name,
    payload: typeExprToDescriptor(e.payload, nativeRegistry),
    ...(e.rawSignature !== undefined ? { rawSignature: e.rawSignature } : {}),
    provenance: e.provenance,
    availability: e.availability,
    kind: e.kind,
  }));
}

function mapNativeFallthroughSurface(
  surface: NativeFallthroughSurface,
  nativeRegistry?: Map<string, NativeTypeExprLike>,
): FallthroughSurface {
  if (surface.kind === "none") {
    return surface;
  }
  return {
    kind: "branches",
    branches: surface.branches.map((branch) => mapNativeFallthroughBranch(branch, nativeRegistry)),
  };
}

function mapNativeFallthroughBranch(
  branch: NativeFallthroughBranch,
  nativeRegistry?: Map<string, NativeTypeExprLike>,
): FallthroughBranch {
  return {
    branchKey: branch.branchKey,
    ...(branch.conditionText !== undefined ? { conditionText: branch.conditionText } : {}),
    props: branch.props.map((p) => ({
      name: p.name,
      type: typeExprToDescriptor(p.type, nativeRegistry),
      ...(p.rawType !== undefined ? { rawType: p.rawType } : {}),
      sources: p.sources,
    })),
    events: branch.events.map((e) => ({
      name: e.name,
      payload: typeExprToDescriptor(e.payload, nativeRegistry),
      ...(e.rawSignature !== undefined ? { rawSignature: e.rawSignature } : {}),
      sources: e.sources,
    })),
    rootChain: branch.rootChain,
    status: branch.status,
  };
}

export function nativeTypeRegistryToMap(
  meta: NativeComponentMetaResult,
): Map<string, TypeDescriptor> | undefined {
  const nativeRegistry = buildNativeTypeRegistry(meta);
  if (!nativeRegistry) {
    return undefined;
  }
  const registry = new Map<string, TypeDescriptor>();
  for (const entry of meta.typeRegistry ?? []) {
    registry.set(entry.name, typeExprToDescriptor(entry.type, nativeRegistry));
  }
  return registry;
}

function buildNativeTypeRegistry(
  meta: NativeComponentMetaResult,
): Map<string, NativeTypeExprLike> | undefined {
  if (!meta.typeRegistry?.length) {
    return undefined;
  }

  const registry = new Map<string, NativeTypeExprLike>();
  for (const entry of meta.typeRegistry) {
    registry.set(entry.name, entry.type);
  }
  return registry;
}

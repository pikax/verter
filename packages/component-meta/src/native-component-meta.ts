import { basename } from "node:path";

import { typeExprToDescriptor } from "./type-expr-bridge.js";
import type {
  ComponentMeta,
  AcceptedPropMeta,
  AcceptedEventMeta,
  AcceptedSurfaceCompleteness,
  RootReachability,
  FallthroughSurface,
  FallthroughBranch,
  TypeExpansionMeta,
} from "./types.js";
import type { TypeDescriptor } from "./type-ir.js";
import type { NativeTypeExpr } from "./type-expr-bridge.js";

export interface NativeJsdocTag {
  name: string;
  text?: string;
}

export interface NativeExpansionDiagnostic {
  reason:
    | "budgetExceeded"
    | "mappedDepthExceeded"
    | "unresolvedReference"
    | "indeterminateConditional"
    | "infiniteKeySpace"
    | "unsupportedOperator";
  context: string;
  propertyName?: string;
}

export interface NativeExpansionMetadata extends TypeExpansionMeta {
  diagnostics: NativeExpansionDiagnostic[];
}

export interface NativePropMeta {
  name: string;
  type: NativeTypeExpr;
  typeExpansion?: NativeExpansionMetadata;
  rawType?: string;
  required: boolean;
  hasDefault: boolean;
  defaultValue?: string;
  description?: string;
  tags?: NativeJsdocTag[];
}

export interface NativeEventMeta {
  name: string;
  payload: NativeTypeExpr;
  payloadExpansion?: NativeExpansionMetadata;
  rawSignature?: string;
  description?: string;
  tags?: NativeJsdocTag[];
}

export interface NativeSlotBindingMeta {
  name: string;
  type: NativeTypeExpr;
  typeExpansion?: NativeExpansionMetadata;
  rawType?: string;
}

export interface NativeSlotMeta {
  name: string;
  isScoped: boolean;
  bindings: NativeSlotBindingMeta[];
  isRequired: boolean;
  description?: string;
  tags?: NativeJsdocTag[];
}

export interface NativeModelMeta {
  name: string;
  type: NativeTypeExpr;
}

export interface NativeExposedMeta {
  name: string;
  type: NativeTypeExpr;
  typeExpansion?: NativeExpansionMetadata;
  description?: string;
}

export interface NativeResolvedTypeMeta {
  name: string;
  /**
   * Expanded lightweight native type.
   * This is what compat/public mapping consumes.
   */
  type: NativeTypeExpr;
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

export interface NativeResolvedPropField {
  name: string;
  isOptional: boolean;
  typeAnnotation?: string;
  description?: string;
  tags?: NativeJsdocTag[];
}

export interface NativeResolvedEmitField {
  name: string;
  payloadType?: string;
  description?: string;
  tags?: NativeJsdocTag[];
}

export interface NativeResolvedSlotBinding {
  name: string;
  typeAnnotation?: string;
}

export interface NativeResolvedSlotField {
  name: string;
  isRequired: boolean;
  bindings: NativeResolvedSlotBinding[];
  returnType?: string;
  description?: string;
  tags?: NativeJsdocTag[];
}

export interface NativeResolvedJsdocTag extends NativeJsdocTag {
  rawType?: string;
  subjectName?: string;
  resolvedType?: NativeTypeExpr;
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
  props?: NativeResolvedPropField[];
  emits?: NativeResolvedEmitField[];
  slots?: NativeResolvedSlotField[];
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
}

export interface NativeComponentUsage {
  name: string;
  importSource?: string;
  isDynamic: boolean;
  props: NativeComponentPropUsage[];
  slotsUsed: string[];
  staticClasses: string[];
  hasDynamicClass: boolean;
  vModels: string[];
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
  type: NativeTypeExpr;
  rawType?: string;
  required: boolean;
  provenance: NativeMemberProvenance;
  availability: NativeMemberAvailability;
  kind: "declaredProp" | "attr";
}

export interface NativeAcceptedEventMeta {
  name: string;
  payload: NativeTypeExpr;
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
  type: NativeTypeExpr;
  rawType?: string;
  sources: NativeInheritedSource[];
}

export interface NativeFallthroughEventEntry {
  name: string;
  payload: NativeTypeExpr;
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

export interface NativeComponentMetaResult {
  props: NativePropMeta[];
  events: NativeEventMeta[];
  slots: NativeSlotMeta[];
  models: NativeModelMeta[];
  exposed: NativeExposedMeta[];
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
  rootReachability: NativeRootReachability;
  fallthroughSurface: NativeFallthroughSurface;
  optionsApi: boolean;
  filePath: string;
  resolution?: NativeComponentMetaResolution;
}

function deriveComponentName(filePath: string): string {
  return basename(filePath).replace(/\.[^.]+$/, "") || "AnonymousComponent";
}

export function nativeComponentMetaToComponentMeta(meta: NativeComponentMetaResult): ComponentMeta {
  return {
    filePath: meta.filePath,
    componentName: deriveComponentName(meta.filePath),
    optionsApi: meta.optionsApi,
    props: meta.props.map((prop) => ({
      name: prop.name,
      type: typeExprToDescriptor(prop.type),
      ...(prop.typeExpansion !== undefined ? { typeExpansion: prop.typeExpansion } : {}),
      required: prop.required,
      hasDefault: prop.hasDefault,
      ...(prop.rawType !== undefined ? { rawType: prop.rawType } : {}),
      ...(prop.defaultValue !== undefined ? { default: prop.defaultValue } : {}),
      ...(prop.description !== undefined ? { description: prop.description } : {}),
      ...(prop.tags?.length ? { tags: prop.tags } : {}),
    })),
    events: meta.events.map((event) => ({
      name: event.name,
      payload: typeExprToDescriptor(event.payload),
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
        type: typeExprToDescriptor(binding.type),
        ...(binding.typeExpansion !== undefined ? { typeExpansion: binding.typeExpansion } : {}),
        ...(binding.rawType !== undefined ? { rawType: binding.rawType } : {}),
      })),
      isRequired: slot.isRequired,
      ...(slot.description !== undefined ? { description: slot.description } : {}),
      ...(slot.tags?.length ? { tags: slot.tags } : {}),
    })),
    models: meta.models.map((model) => ({
      name: model.name,
      type: typeExprToDescriptor(model.type),
    })),
    exposed: meta.exposed.map((exposed) => ({
      name: exposed.name,
      type: typeExprToDescriptor(exposed.type),
      ...(exposed.typeExpansion !== undefined ? { typeExpansion: exposed.typeExpansion } : {}),
      ...(exposed.description !== undefined ? { description: exposed.description } : {}),
    })),
    components: meta.components.map((component) => ({
      name: component.name,
      ...(component.importSource !== undefined ? { importSource: component.importSource } : {}),
      isDynamic: component.isDynamic,
      props: component.props.map((prop) => ({
        name: prop.name,
        isBound: prop.isBound,
        constness: prop.constness,
      })),
      slotsUsed: [...component.slotsUsed],
      staticClasses: [...component.staticClasses],
      hasDynamicClass: component.hasDynamicClass,
      vModels: [...component.vModels],
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
    acceptedProps: mapNativeAcceptedProps(meta.acceptedProps),
    acceptedEvents: mapNativeAcceptedEvents(meta.acceptedEvents),
    acceptedSurfaceCompleteness: meta.acceptedSurfaceCompleteness as AcceptedSurfaceCompleteness,
    rootReachability: meta.rootReachability as RootReachability,
    fallthroughSurface: mapNativeFallthroughSurface(meta.fallthroughSurface),
    flags: meta.flags,
  };
}

function mapNativeAcceptedProps(props: NativeAcceptedPropMeta[]): AcceptedPropMeta[] {
  return props.map((p) => ({
    name: p.name,
    type: typeExprToDescriptor(p.type),
    ...(p.rawType !== undefined ? { rawType: p.rawType } : {}),
    required: p.required,
    provenance: p.provenance,
    availability: p.availability,
    kind: p.kind,
  }));
}

function mapNativeAcceptedEvents(events: NativeAcceptedEventMeta[]): AcceptedEventMeta[] {
  return events.map((e) => ({
    name: e.name,
    payload: typeExprToDescriptor(e.payload),
    ...(e.rawSignature !== undefined ? { rawSignature: e.rawSignature } : {}),
    provenance: e.provenance,
    availability: e.availability,
    kind: e.kind,
  }));
}

function mapNativeFallthroughSurface(surface: NativeFallthroughSurface): FallthroughSurface {
  if (surface.kind === "none") {
    return surface;
  }
  return {
    kind: "branches",
    branches: surface.branches.map(mapNativeFallthroughBranch),
  };
}

function mapNativeFallthroughBranch(branch: NativeFallthroughBranch): FallthroughBranch {
  return {
    branchKey: branch.branchKey,
    ...(branch.conditionText !== undefined ? { conditionText: branch.conditionText } : {}),
    props: branch.props.map((p) => ({
      name: p.name,
      type: typeExprToDescriptor(p.type),
      ...(p.rawType !== undefined ? { rawType: p.rawType } : {}),
      sources: p.sources,
    })),
    events: branch.events.map((e) => ({
      name: e.name,
      payload: typeExprToDescriptor(e.payload),
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
  if (!meta.typeRegistry?.length) {
    return undefined;
  }
  const registry = new Map<string, TypeDescriptor>();
  for (const entry of meta.typeRegistry) {
    registry.set(entry.name, typeExprToDescriptor(entry.type));
  }
  return registry;
}

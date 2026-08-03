import { basename } from "node:path";

import { typeExprToDescriptor } from "./type-expr-bridge.js";
import type {
  ComponentMeta,
  AcceptedPropMeta,
  AcceptedEventMeta,
  AcceptedSurfaceCompleteness,
  OrderedSfcStructureMeta,
  RootInfo,
  RootReachability,
  FallthroughSurface,
  FallthroughBranch,
  TypeExpansionMeta,
  ReturnWrapperRole,
  ReturnWrapperUnresolvedReason,
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

export interface NativeTerminalTypeDisplay {
  text?: string | null;
}

export type NativeResolutionProvenance =
  | "semanticEvaluator"
  | "sessionProjector"
  | "frameworkSurface"
  | "fallthroughInheritance"
  | "schema";

export type NativePublicationProvenance =
  | { kind: "resolved"; value: NativeResolutionProvenance }
  | {
      kind: "authored";
      value: "macroPayload" | "declarationBody" | "augmentationBody" | "jsdocTypedefBody";
    };

export type NativePublicationReason =
  | { kind: "resolvedExactConcrete" }
  | { kind: "resolvedExactSymbolic" }
  | { kind: "resolvedIncomplete" }
  | {
      kind: "authoredForIncomplete";
      policy: "importedMacroCompound" | "importedIndexedAccess";
    }
  | {
      kind: "authoredSymbolicRepresentation";
      proof: "importedMacroCompound" | "importedIndexedAccess";
    };

export type NativeTypePublicationFailure =
  | "unrepresentableRequiredMemberValue"
  | "unrepresentableRequiredPayload";

export type NativeTypePublication =
  | {
      kind: "failed";
      failure: NativeTypePublicationFailure;
      provenance: NativeResolutionProvenance;
    }
  | {
      kind: "absent";
      absence: "unannotated" | "branchDivergent";
      provenance: NativeResolutionProvenance;
    }
  | {
      kind: "published";
      semanticAuthority: "resolved" | "authoredFallback";
      exactness: "exactConcrete" | "exactSymbolic" | "incomplete";
      reason: NativePublicationReason;
      provenance: NativePublicationProvenance;
    };

export type NativeContractExactness = "exact" | "degraded";
export type NativeContractProvenance = "componentMetaOutput";

export type NativeContractSurface =
  | { kind: "prop"; name: string }
  | { kind: "event"; name: string; overloadIndex: number }
  | { kind: "slotBinding"; slot: string; binding: string }
  | { kind: "slotReturn"; slot: string };

export interface NativeResolutionDiagnostic {
  kind: NativeExpansionDiagnostic["reason"];
  context: string;
  propertyName?: string;
}

export interface NativeContractDegradation {
  surface: NativeContractSurface;
  reason: "absent" | "incomplete";
  diagnostics: NativeResolutionDiagnostic[];
}

export interface NativePublicTypeReference {
  type?: NativeTypeExprLike;
  publication: NativeTypePublication;
  terminalDisplay: NativeTerminalTypeDisplay;
}

export interface NativePublicParameter {
  name?: string;
  optional: boolean;
  rest: boolean;
  type: NativeTypeExprLike;
}

export interface NativePublicCallSignature {
  source: NativePublicTypeReference;
  parameters: NativePublicParameter[];
  returnType: NativeTypeExprLike;
}

export interface NativePublicHandlerSignature {
  parameters: NativePublicParameter[];
  returnType: NativeTypeExprLike;
}

export interface NativePublicProp {
  name: string;
  optional: boolean;
  hasDefault: boolean;
  type: NativePublicTypeReference;
  exactness: NativeContractExactness;
  degradation: NativeContractDegradation[];
  provenance: NativeContractProvenance;
}

export interface NativePublicEvent {
  name: string;
  overloads: NativePublicCallSignature[];
  derivedHandler: { overloads: NativePublicHandlerSignature[] };
  exactness: NativeContractExactness;
  degradation: NativeContractDegradation[];
  provenance: NativeContractProvenance;
}

export interface NativePublicSlot {
  name: string;
  optional: boolean;
  input: { bindings: Array<{ name: string; type: NativePublicTypeReference }> };
  returnType?: NativePublicTypeReference;
  exactness: NativeContractExactness;
  degradation: NativeContractDegradation[];
  provenance: NativeContractProvenance;
}

export interface NativeComponentPublicContract {
  adapterId: string;
  exactness: NativeContractExactness;
  degradation: NativeContractDegradation[];
  provenance: NativeContractProvenance;
  props: NativePublicProp[];
  events: NativePublicEvent[];
  slots: NativePublicSlot[];
}

export type NativeComponentMetaOutputFailure =
  | { kind: "unraisableSource" }
  | {
      kind: "requiredSourceUnavailable";
      publicationFailure: NativeTypePublicationFailure;
    }
  | { kind: "interiorSourceMiss" }
  | { kind: "shellMaterializationMiss" }
  | { kind: "unknownMaterializingSourceInterior" };

export type NativeComponentContractAvailability =
  | { kind: "supported"; contract: NativeComponentPublicContract }
  | {
      kind: "unsupported";
      adapterId: string;
      reason:
        | { kind: "adapterUnavailable" }
        | { kind: "componentMetaUnavailable" }
        | {
            kind: "outputMaterializationFailed";
            lane: string;
            index: number;
            innerIndex?: number;
            failure: NativeComponentMetaOutputFailure;
          }
        | {
            kind: "publicationFailed";
            surface: NativeContractSurface;
            failure: NativeTypePublicationFailure;
            provenance: NativeResolutionProvenance;
          };
      diagnostics: NativeResolutionDiagnostic[];
    };

export interface NativePropMeta {
  name: string;
  type?: NativeTypeExprLike;
  publication: NativeTypePublication;
  terminalDisplay: NativeTerminalTypeDisplay;
  typeExpansion?: NativeExpansionMetadata;
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
  publication: NativeTypePublication;
  terminalDisplay: NativeTerminalTypeDisplay;
  payloadExpansion?: NativeExpansionMetadata;
  rawSignature?: string;
  description?: string;
  tags?: NativeJsdocTag[];
}

export interface NativeSlotBindingMeta {
  name: string;
  type?: NativeTypeExprLike;
  publication: NativeTypePublication;
  terminalDisplay: NativeTerminalTypeDisplay;
  typeExpansion?: NativeExpansionMetadata;
}

export interface NativeSlotMeta {
  name: string;
  isScoped: boolean;
  bindings: NativeSlotBindingMeta[];
  isRequired: boolean;
  returnType?: string;
  returnValue?: NativeTypeExprLike;
  returnPublication?: NativeTypePublication;
  returnTerminalDisplay?: NativeTerminalTypeDisplay;
  description?: string;
  tags?: NativeJsdocTag[];
  /**
   * Producer fact: does this slot come from the component's own AUTHORED
   * slots surface (the resolved `defineSlots<T>()` macro surface or a
   * template `<slot>` element)? Consumed by the compat slot blocklist —
   * an author-declared slot is never blocked, whatever its name.
   * Forward-compat: coerced with `Boolean(...)` so older payloads
   * without the field read `false` (the name block applies).
   */
  declaredInMacroTypeArg?: boolean;
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

export interface NativeOrderedSfcStructure {
  schemaVersion: 1;
  artifactToken: string;
  blocks: Array<Record<string, unknown>>;
  markupNodes: Array<Record<string, unknown>>;
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
  returnWrapperRole?: ReturnWrapperRole;
  returnWrapperUnresolvedReason?: ReturnWrapperUnresolvedReason;
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
  /**
   * Opaque sealed block token binding this style analysis to its ordered
   * structure block (same vocabulary as the structure block tokens). Absent
   * when the sealed identity could not be revalidated — treat absence as
   * typed unavailable, never an ordinal fallback.
   */
  blockToken?: string;
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
  type?: NativeTypeExprLike;
  publication: NativeTypePublication;
  terminalDisplay: NativeTerminalTypeDisplay;
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
  type?: NativeTypeExprLike;
  publication: NativeTypePublication;
  terminalDisplay: NativeTerminalTypeDisplay;
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
  componentPublicContract: NativeComponentContractAvailability;
  props: NativePropMeta[];
  events: NativeEventMeta[];
  slots: NativeSlotMeta[];
  models: NativeModelMeta[];
  exposed: NativeExposedMeta[];
  publicInstance?: NativePublicInstanceMeta;
  orderedSfcStructure: NativeOrderedSfcStructure;
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

function requirePublishedType(
  row: {
    type?: NativeTypeExprLike;
    publication: NativeTypePublication;
  },
  label: string,
): NativeTypeExprLike {
  if (row.publication.kind === "failed") {
    throw new Error(`component-meta ${label} has failed type publication`);
  }
  if (row.type === undefined) {
    throw new Error(`component-meta ${label} has no materialized type`);
  }
  return row.type;
}

function compatRawType(display: NativeTerminalTypeDisplay): { rawType?: string } {
  return typeof display.text === "string" ? { rawType: display.text } : {};
}

export function nativeComponentMetaToComponentMeta(meta: NativeComponentMetaResult): ComponentMeta {
  const nativeRegistry = buildNativeTypeRegistry(meta);
  return {
    filePath: meta.filePath,
    componentName: deriveComponentName(meta.filePath),
    optionsApi: meta.optionsApi,
    componentPublicContract: meta.componentPublicContract,
    props: meta.props.map((prop) => ({
      name: prop.name,
      type: typeExprToDescriptor(requirePublishedType(prop, `prop ${prop.name}`), nativeRegistry),
      ...(prop.typeExpansion !== undefined ? { typeExpansion: prop.typeExpansion } : {}),
      required: prop.required,
      hasDefault: prop.hasDefault,
      ...compatRawType(prop.terminalDisplay),
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
      payload: typeExprToDescriptor(
        requirePublishedType(
          { type: event.payload, publication: event.publication },
          `event ${event.name}`,
        ),
        nativeRegistry,
      ),
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
        type: typeExprToDescriptor(
          requirePublishedType(binding, `slot binding ${slot.name}.${binding.name}`),
          nativeRegistry,
        ),
        ...(binding.typeExpansion !== undefined ? { typeExpansion: binding.typeExpansion } : {}),
        ...compatRawType(binding.terminalDisplay),
      })),
      isRequired: slot.isRequired,
      ...(slot.returnType !== undefined ? { returnType: slot.returnType } : {}),
      ...(slot.description !== undefined ? { description: slot.description } : {}),
      ...(slot.tags?.length ? { tags: slot.tags } : {}),
      // Forward-compat coerce — see `NativeSlotMeta.declaredInMacroTypeArg`.
      declaredInMacroTypeArg: Boolean(slot.declaredInMacroTypeArg),
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
    orderedSfcStructure: mapNativeOrderedStructure(meta.orderedSfcStructure),
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
      ...(binding.returnWrapperRole !== undefined
        ? { returnWrapperRole: binding.returnWrapperRole }
        : {}),
      ...(binding.returnWrapperUnresolvedReason !== undefined
        ? { returnWrapperUnresolvedReason: binding.returnWrapperUnresolvedReason }
        : {}),
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
      ...(style.blockToken !== undefined ? { blockToken: style.blockToken } : {}),
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

function mapNativeOrderedStructure(structure: NativeOrderedSfcStructure): OrderedSfcStructureMeta {
  if (structure.schemaVersion !== 1) {
    throw new Error(`unsupported ordered structure schema ${structure.schemaVersion}`);
  }
  return structure;
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
    type: typeExprToDescriptor(requirePublishedType(p, `accepted prop ${p.name}`), nativeRegistry),
    ...compatRawType(p.terminalDisplay),
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
      type: typeExprToDescriptor(
        requirePublishedType(p, `fallthrough prop ${p.name}`),
        nativeRegistry,
      ),
      ...compatRawType(p.terminalDisplay),
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

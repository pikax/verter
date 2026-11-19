import type { FactResult } from "./facts.js";

import { typeExprToDescriptor } from "./type-expr-bridge.js";
import type {
  ComponentMeta,
  AcceptedPropMeta,
  AcceptedEventMeta,
  AcceptedSurfaceCompleteness,
  OrderedSfcStructureMeta,
  RootInfo,
  FallthroughSurface,
  FallthroughBranch,
  TypeExpansionMeta,
  ReturnWrapperRole,
  ReturnWrapperUnresolvedReason,
} from "./types.js";
import type { TypeDescriptor } from "@verter/type-ir";
import type { NativeTypeExprLike } from "./type-expr-bridge.js";

export type { NativeTypeExprLike };

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
    | "emptyUnionArm"
    | "connectedMemoryLimit";
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
  requiredFact?: FactResult<boolean>;
  hasDefaultFact?: FactResult<boolean>;
  typeFact: FactResult<NativeTypeExprLike | undefined>;
  name: string;
  type?: NativeTypeExprLike;
  publication: NativeTypePublication;
  terminalDisplay: NativeTerminalTypeDisplay;
  typeExpansion?: NativeExpansionMetadata;
  /** Absent when the producer published no boolean. Absence is not false. */
  required: boolean | undefined;
  /** Absent when the producer published no boolean. Absence is not false. */
  hasDefault: boolean | undefined;
  defaultValue?: string;
  description?: string;
  tags?: NativeJsdocTag[];
  /** Producer-owned authored member flag. */
  declaredInMacroTypeArg: boolean;
}

export interface NativeEventMeta {
  typeFact: FactResult<NativeTypeExprLike | undefined>;
  name: string;
  /** Absent when the payload fact is unavailable. */
  payload?: NativeTypeExprLike;
  publication: NativeTypePublication;
  terminalDisplay: NativeTerminalTypeDisplay;
  payloadExpansion?: NativeExpansionMetadata;
  rawSignature?: string;
  description?: string;
  tags?: NativeJsdocTag[];
}

export interface NativeSlotBindingMeta {
  typeFact: FactResult<NativeTypeExprLike | undefined>;
  name: string;
  type?: NativeTypeExprLike;
  publication: NativeTypePublication;
  terminalDisplay: NativeTerminalTypeDisplay;
  typeExpansion?: NativeExpansionMetadata;
}

export interface NativeSlotMeta {
  returnFact?: FactResult<NativeTypeExprLike | undefined>;
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
  /** Producer-owned authored member flag. */
  declaredInMacroTypeArg: boolean;
}

export interface NativeModelMeta {
  name: string;
  type?: NativeTypeExprLike;
  typeFact: FactResult<NativeTypeExprLike | undefined>;
}

export interface NativeExposedMeta {
  name: string;
  type?: NativeTypeExprLike;
  typeFact: FactResult<NativeTypeExprLike | undefined>;
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
  /** Absent when this member's value fact failed and siblings stayed. */
  type?: NativeTypeExprLike;
  typeFact: FactResult<NativeTypeExprLike | undefined>;
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
   * Absent when the registry value fact is unavailable.
   */
  type?: NativeTypeExprLike;
  typeFact: FactResult<NativeTypeExprLike | undefined>;
  typeExpansion?: NativeExpansionMetadata;
  /**
   * Pre-expansion source form retained for native callers that need to inspect
   * what the expanded type came from.
   */
  rawType?: string;
  declaration?: NativeResolvedTypeDeclaration;
}

/**
 * A range that names its own encoding: 0-based, half-open, counted from the
 * start of the file the owning record names. A consumer MUST reject a tag it
 * does not know.
 */
export interface NativeTaggedSpan {
  start: number;
  end: number;
  encoding: "utf8.v1" | "utf16.v1";
}

export interface NativeResolvedTypeDeclaration {
  requestedName: string;
  resolvedName: string;
  canonicalSource: string;
  /** The declaration's range in `canonicalSource`; absent when not exact. */
  span?: NativeTaggedSpan;
  kind: "interface" | "typeAlias" | "class" | "unknown";
  text?: string;
}

export interface NativeResolvedNativeProp {
  name: string;
  isOptional: boolean;
  typeAnnotation?: string;
  visibility: "public" | "protected" | "private";
  /** The property's range in the owning component's source; absent when not exact. */
  span?: NativeTaggedSpan;
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
  typeFact: FactResult<NativeTypeExprLike | undefined>;
  publication: NativeTypePublication;
  terminalDisplay: NativeTerminalTypeDisplay;
  required: boolean;
  provenance: NativeMemberProvenance;
  availability: NativeMemberAvailability;
  kind: "declaredProp" | "attr";
}

export interface NativeAcceptedEventMeta {
  name: string;
  payload?: NativeTypeExprLike;
  typeFact: FactResult<NativeTypeExprLike | undefined>;
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

/**
 * Why a published component surface is partial. Closed taxonomy — one entry
 * per reason class the producer records.
 */
export type NativeSurfacePartialReason =
  | "budgetExceeded"
  | "cancelled"
  | "supersededGeneration"
  | "unstableState"
  | "samePathRecursion"
  | "walkerFatal"
  | "propagated"
  | "deferredEvaluationLimit"
  | "structuralFactDemandLimit"
  | "semanticQueryFault"
  | "missingSemanticNodeData"
  | "projectionWorkLimit"
  | "connectedQueryDepthLimit"
  | "missingDependency"
  | "flowReturnUninferred"
  | "flowReturnUnverified"
  | "flowReturnNoSurface"
  | "undecidedConditional"
  | "connectedMemoryLimit"
  | "operationBudget"
  | "fanoutLimit";

/**
 * Typed completeness of a published component surface. Without it an
 * empty-because-degraded payload is indistinguishable from an
 * empty-because-nothing-is-declared payload, and a consumer cannot tell
 * "nothing is declared" from "we failed to find what is declared".
 *
 * A `partial` result never pairs with `componentPublicContract.exactness ===
 * "exact"`, and the producer refuses it warm admission, so an identical
 * follow-up request recomputes rather than replaying it as complete.
 */
export type NativeResultCompleteness =
  | { kind: "complete" }
  | { kind: "partial"; reasons: NativeSurfacePartialReason[] };

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
  typeFact: FactResult<NativeTypeExprLike | undefined>;
  publication: NativeTypePublication;
  terminalDisplay: NativeTerminalTypeDisplay;
  sources: NativeInheritedSource[];
}

export interface NativeFallthroughEventEntry {
  name: string;
  payload?: NativeTypeExprLike;
  typeFact: FactResult<NativeTypeExprLike | undefined>;
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

export type NativeSemanticLane =
  | "props"
  | "events"
  | "slots"
  | "models"
  | "exposed"
  | "acceptedProps"
  | "acceptedEvents"
  | "rootReachability"
  | "fallthroughSurface"
  | "publicInstance"
  | "componentPublicContract"
  | "typeRegistry";
export type NativeComponentFacts = {
  readonly [K in Exclude<NativeSemanticLane, "componentPublicContract">]: FactResult<
    K extends "publicInstance"
      ? NativeComponentMetaResult[K]
      : NonNullable<NativeComponentMetaResult[K]>
  >;
} & {
  readonly delivery: FactResult<undefined>;
  readonly componentPublicContract: FactResult<NativeComponentPublicContract>;
};

export interface NativeComponentMetaResult {
  facts?: NativeComponentFacts;
  componentPublicContract?: NativeComponentContractAvailability;
  resultCompleteness: NativeResultCompleteness;
  props?: NativePropMeta[];
  events?: NativeEventMeta[];
  slots?: NativeSlotMeta[];
  models?: NativeModelMeta[];
  exposed?: NativeExposedMeta[];
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
  acceptedProps?: NativeAcceptedPropMeta[];
  acceptedEvents?: NativeAcceptedEventMeta[];
  acceptedSurfaceCompleteness: NativeAcceptedSurfaceCompleteness;
  rootInfo?: NativeRootInfo;
  rootReachability?: NativeRootReachability;
  fallthroughSurface?: NativeFallthroughSurface;
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
  return (filePath.split(/[\\/]/).pop() ?? "").replace(/\.[^.]+$/, "") || "AnonymousComponent";
}

function compatRawType(display: NativeTerminalTypeDisplay): { rawType?: string } {
  return typeof display.text === "string" ? { rawType: display.text } : {};
}

function usableMemberType<
  T extends {
    type?: NativeTypeExprLike | null;
    typeFact?: { kind: string };
    publication?: { kind: string };
  },
>(row: T): row is T & { type: NativeTypeExprLike } {
  return (
    row.type != null && row.typeFact?.kind !== "unavailable" && row.publication?.kind !== "failed"
  );
}

/** Transcribe the member fact once; unavailable values never gain a descriptor. */
function descriptorFact(
  fact: FactResult<NativeTypeExprLike | undefined>,
  key: "type" | "payload" = "type",
): {
  typeFact: FactResult<TypeDescriptor | undefined>;
  type?: TypeDescriptor;
  payload?: TypeDescriptor;
} {
  if (fact.kind === "unavailable") return { typeFact: fact };
  const value = fact.value === undefined ? undefined : typeExprToDescriptor(fact.value);
  return { typeFact: { ...fact, value }, ...(value === undefined ? {} : { [key]: value }) };
}

export function nativeComponentMetaToComponentMeta(meta: NativeComponentMetaResult): ComponentMeta {
  const result: ComponentMeta = {
    filePath: meta.filePath,
    componentName: deriveComponentName(meta.filePath),
    optionsApi: meta.optionsApi,
    componentPublicContract: meta.componentPublicContract,
    resultCompleteness: meta.resultCompleteness,
    facts: meta.facts,
    props: meta.props?.map((prop) => ({
      name: prop.name,
      ...descriptorFact(prop.typeFact),
      ...(prop.typeExpansion !== undefined ? { typeExpansion: prop.typeExpansion } : {}),
      required: prop.required,
      hasDefault: prop.hasDefault,
      ...compatRawType(prop.terminalDisplay),
      ...(prop.defaultValue !== undefined ? { default: prop.defaultValue } : {}),
      ...(prop.description !== undefined ? { description: prop.description } : {}),
      ...(prop.tags?.length ? { tags: prop.tags } : {}),
      declaredInMacroTypeArg: prop.declaredInMacroTypeArg,
    })),
    events: meta.events?.map((event) => ({
      name: event.name,
      ...descriptorFact(event.typeFact, "payload"),
      ...(event.payloadExpansion !== undefined ? { payloadExpansion: event.payloadExpansion } : {}),
      ...(event.rawSignature !== undefined ? { rawSignature: event.rawSignature } : {}),
      ...(event.description !== undefined ? { description: event.description } : {}),
      ...(event.tags?.length ? { tags: event.tags } : {}),
    })),
    slots: meta.slots?.map((slot) => ({
      name: slot.name,
      isScoped: slot.isScoped,
      bindings: slot.bindings.map((binding) => ({
        name: binding.name,
        ...descriptorFact(binding.typeFact),
        ...(binding.typeExpansion !== undefined ? { typeExpansion: binding.typeExpansion } : {}),
        ...compatRawType(binding.terminalDisplay),
      })),
      isRequired: slot.isRequired,
      ...(slot.returnType !== undefined ? { returnType: slot.returnType } : {}),
      ...(slot.description !== undefined ? { description: slot.description } : {}),
      ...(slot.tags?.length ? { tags: slot.tags } : {}),
      declaredInMacroTypeArg: slot.declaredInMacroTypeArg,
    })),
    models: meta.models?.map((model) => ({
      name: model.name,
      ...descriptorFact(model.typeFact),
    })),
    exposed: meta.exposed?.map((exposed) => ({
      name: exposed.name,
      ...descriptorFact(exposed.typeFact),
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
              ...descriptorFact(member.typeFact),
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
      // Structured entries transcribe the vModels name list.
      vModelEntries: component.vModels.map((bindingName) => ({ bindingName })),
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
    acceptedProps:
      meta.acceptedProps === undefined ? undefined : mapNativeAcceptedProps(meta.acceptedProps),
    acceptedEvents:
      meta.acceptedEvents === undefined ? undefined : mapNativeAcceptedEvents(meta.acceptedEvents),
    acceptedSurfaceCompleteness: meta.acceptedSurfaceCompleteness as AcceptedSurfaceCompleteness,
    rootInfo: meta.rootInfo === undefined ? undefined : mapNativeRootInfo(meta.rootInfo),
    rootReachability: meta.rootReachability,
    fallthroughSurface:
      meta.fallthroughSurface === undefined
        ? undefined
        : mapNativeFallthroughSurface(meta.fallthroughSurface),
    flags: meta.flags,
    ...(meta.origin !== undefined ? { origin: meta.origin } : {}),
  };
  for (const key of Object.keys(result) as Array<keyof ComponentMeta>) {
    if (result[key] === undefined) Reflect.deleteProperty(result, key);
  }
  return result;
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

function mapNativeAcceptedProps(props: NativeAcceptedPropMeta[]): AcceptedPropMeta[] {
  return props.map((p) => ({
    name: p.name,
    ...descriptorFact(p.typeFact),
    ...compatRawType(p.terminalDisplay),
    required: p.required,
    provenance: p.provenance,
    availability: p.availability,
    kind: p.kind,
  }));
}

function mapNativeAcceptedEvents(events: NativeAcceptedEventMeta[]): AcceptedEventMeta[] {
  return events.map((e) => ({
    name: e.name,
    ...descriptorFact(e.typeFact, "payload"),
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
    branches: surface.branches.map((branch) => mapNativeFallthroughBranch(branch)),
  };
}

function mapNativeFallthroughBranch(branch: NativeFallthroughBranch): FallthroughBranch {
  return {
    branchKey: branch.branchKey,
    ...(branch.conditionText !== undefined ? { conditionText: branch.conditionText } : {}),
    props: branch.props.map((p) => ({
      name: p.name,
      ...descriptorFact(p.typeFact),
      ...compatRawType(p.terminalDisplay),
      sources: p.sources,
    })),
    events: branch.events.map((e) => ({
      name: e.name,
      ...descriptorFact(e.typeFact, "payload"),
      ...(e.rawSignature !== undefined ? { rawSignature: e.rawSignature } : {}),
      sources: e.sources,
    })),
    rootChain: branch.rootChain,
    status: branch.status,
  };
}

/** Transcribe usable producer rows, withholding ambiguous declaration names. */
export function nativeTypeRegistryToMap(
  meta: NativeComponentMetaResult,
): Map<string, TypeDescriptor> | undefined {
  const registry = new Map<string, TypeDescriptor>();
  const declarations = new Map<string, NativeResolvedTypeDeclaration | undefined>();
  const ambiguous = new Set<string>();
  for (const entry of meta.typeRegistry ?? []) {
    if (declarations.has(entry.name)) {
      const previous = declarations.get(entry.name);
      const current = entry.declaration;
      if (
        !previous ||
        !current ||
        !previous.span ||
        !current.span ||
        previous.canonicalSource.replaceAll("\\", "/") !==
          current.canonicalSource.replaceAll("\\", "/") ||
        previous.resolvedName !== current.resolvedName ||
        previous.kind !== current.kind ||
        previous.span?.start !== current.span?.start ||
        previous.span?.end !== current.span?.end ||
        previous.span?.encoding !== current.span?.encoding
      ) {
        ambiguous.add(entry.name);
        registry.delete(entry.name);
      }
    } else {
      declarations.set(entry.name, entry.declaration);
    }
    if (ambiguous.has(entry.name) || !usableMemberType(entry) || registry.has(entry.name)) continue;
    registry.set(entry.name, typeExprToDescriptor(entry.type));
  }
  return registry.size === 0 ? undefined : registry;
}

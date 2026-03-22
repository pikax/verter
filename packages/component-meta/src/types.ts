/**
 * Component metadata types extracted from Vue SFCs.
 */

import type { TypeDescriptor } from "./type-ir.js";

/** Structured metadata extracted from a Vue Single File Component. */
export interface ComponentMeta {
  /** File path or canonical ID of the source SFC. */
  filePath: string;
  /** Component name derived from the file name (e.g. `"MyButton"`). */
  componentName: string;
  /** Whether the component uses the Options API (`export default { ... }`). */
  optionsApi: boolean;
  /** Props declared via `defineProps` or Options API `props`. */
  props: PropMeta[];
  /** Events declared via `defineEmits` or Options API `emits`. */
  events: EventMeta[];
  /** Slots discovered in the template. */
  slots: SlotMeta[];
  /** Models declared via `defineModel`. */
  models: ModelMeta[];
  /** Members exposed via `defineExpose` or Options API `expose`. */
  exposed: ExposedMeta[];

  // ── Template usage ─────────────────────────────────────────────

  /** Child components used in the template. */
  components: ComponentUsage[];
  /** `ref="foo"` usages in the template. */
  templateRefs: TemplateRefMeta[];

  // ── Script analysis ────────────────────────────────────────────

  /** All imports in the script block. */
  imports: ImportMeta[];
  /** Script bindings (variables, functions, etc.). */
  bindings: BindingMeta[];
  /** Vue API call sites (lifecycle hooks, watchers, provide/inject, etc.). */
  vueApiCalls: VueApiCallMeta[];

  // ── Style analysis ─────────────────────────────────────────────

  /** Per-style-block analysis. */
  styles: StyleMeta[];

  // ── Fallthrough surface ────────────────────────────────────────

  /** Accepted props: declared props + inherited attrs (computed by the host). */
  acceptedProps: AcceptedPropMeta[];
  /** Accepted events: declared emits + inherited listeners (computed by the host). */
  acceptedEvents: AcceptedEventMeta[];
  /** Whether `acceptedProps`/`acceptedEvents` are exact or only a sound lower bound. */
  acceptedSurfaceCompleteness: AcceptedSurfaceCompleteness;
  /** Root reachability classification for fallthrough inheritance. */
  rootReachability: RootReachability;
  /** Branch-structured inherited surface (declared members do NOT appear here). */
  fallthroughSurface: FallthroughSurface;

  // ── Flags ──────────────────────────────────────────────────────

  /** Quick O(1) boolean checks for component characteristics. */
  flags: ComponentFlags;
}

/** A single JSDoc tag. */
export interface JsdocTag {
  /** Tag name without the `@` prefix (e.g. `"param"`, `"deprecated"`, `"default"`). */
  name: string;
  /** Tag text after the tag name, if any. */
  text?: string;
}

/** Diagnostic explaining why a type expansion is partial. */
export interface TypeExpansionDiagnostic {
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

/** Completeness and diagnostics from native type expansion. */
export interface TypeExpansionMeta {
  completeness: "exact" | "partial";
  diagnostics: TypeExpansionDiagnostic[];
}

/** Metadata for a single component prop. */
export interface PropMeta {
  /** Prop name as declared in `defineProps` or Options API. */
  name: string;
  /** Parsed type descriptor. */
  type: TypeDescriptor;
  /** Native expansion completeness for the prop type, when available. */
  typeExpansion?: TypeExpansionMeta;
  /** Whether the prop is required (no default, no `?`). */
  required: boolean;
  /** Whether the prop has a default value (via `withDefaults` or Options API). */
  hasDefault: boolean;
  /** Original TS type annotation string (e.g. `"string | number"`). */
  rawType?: string;
  /** Vue runtime constructor names (e.g. `["String", "Number"]`). */
  runtimeTypes?: string[];
  /** JSDoc description from the leading `/** ... *​/` comment. */
  description?: string;
  /** JSDoc tags (e.g. `@default`, `@deprecated`). */
  tags?: JsdocTag[];
  /** Default value text (from `withDefaults` or Options API `default`). */
  default?: string;
}

/** Metadata for a single component event. */
export interface EventMeta {
  /** Event name (e.g. `"click"`, `"update:modelValue"`). */
  name: string;
  /** Payload type descriptor. */
  payload: TypeDescriptor;
  /** Native expansion completeness for the payload type, when available. */
  payloadExpansion?: TypeExpansionMeta;
  /** Whether the event has a runtime validator function. */
  hasValidator: boolean;
  /** Whether the event is explicitly declared (vs. inferred from template usage). */
  isDeclared: boolean;
  /** Original emit signature string. */
  rawSignature?: string;
  /** JSDoc description from the leading `/** ... *​/` comment. */
  description?: string;
  /** JSDoc tags (e.g. `@deprecated`). */
  tags?: JsdocTag[];
}

/** Metadata for a single template slot. */
export interface SlotMeta {
  /** Slot name (`"default"` for the unnamed slot). */
  name: string;
  /** Whether the slot exposes scoped bindings. */
  isScoped: boolean;
  /** Scoped slot bindings (empty for non-scoped slots). */
  bindings: SlotBinding[];
  /** Whether the slot is required (no `?` in `defineSlots` type param). */
  isRequired?: boolean;
  /** Whether the `<slot>` element has fallback content. */
  hasFallbackContent?: boolean;
  /** Return type of the slot function (e.g., `"VNode[]"`, `"any"`). Used for strict slots. */
  returnType?: string;
  /** JSDoc description from the leading `/** ... *​/` comment. */
  description?: string;
  /** JSDoc tags (e.g. `@deprecated`). */
  tags?: JsdocTag[];
}

/** A single binding exposed by a scoped slot. */
export interface SlotBinding {
  /** Binding name available in the slot scope. */
  name: string;
  /** Type descriptor for the binding value. */
  type: TypeDescriptor;
  /** Native expansion completeness for the binding type, when available. */
  typeExpansion?: TypeExpansionMeta;
  /** The expression text (e.g. `"row"`, `"i"`) — may differ from `name`. */
  expression?: string;
  /** Original TS type annotation string (e.g. `"string"`, `"MyItem"`). */
  rawType?: string;
}

/** Metadata for a `defineModel` declaration. */
export interface ModelMeta {
  /** Model name (`"modelValue"` for the default model). */
  name: string;
  /** Type descriptor for the model value. */
  type: TypeDescriptor;
}

/** Metadata for a member exposed via `defineExpose`. */
export interface ExposedMeta {
  /** Exposed member name. */
  name: string;
  /** Type descriptor for the exposed value. */
  type: TypeDescriptor;
  /** Native expansion completeness for the exposed type, when available. */
  typeExpansion?: TypeExpansionMeta;
  /** JSDoc description from the leading `/** ... *​/` comment. */
  description?: string;
}

// ── Template usage types ───────────────────────────────────────────

/** A prop usage on a child component in the template. */
export interface ComponentPropUsage {
  /** Prop name. */
  name: string;
  /** Whether this prop is bound (`:prop` vs `prop="static"`). */
  isBound: boolean;
  /** Constness classification. */
  constness: "const" | "dynamic" | "unknown";
}

/** A child component used in the template. */
export interface ComponentUsage {
  /** PascalCase component name. */
  name: string;
  /** Resolved import path (undefined for globals/unresolved). */
  importSource?: string;
  /** Whether this is a dynamic component (`<component :is>`). */
  isDynamic: boolean;
  /** Props passed to this component. */
  props: ComponentPropUsage[];
  /** Slot names used on this component. */
  slotsUsed: string[];
  /** Static class names from `class="foo bar"`. */
  staticClasses: string[];
  /** Whether `:class="..."` is present. */
  hasDynamicClass: boolean;
  /** v-model binding names. */
  vModels: string[];
}

/** A template ref usage (`ref="foo"` or `:ref="expr"`). */
export interface TemplateRefMeta {
  /** Ref name. */
  name: string;
  /** Whether this is a dynamic ref (`:ref="expr"`). */
  isDynamic: boolean;
  /** The element or component tag this ref points to (e.g. `"input"`, `"Modal"`). */
  targetTag: string;
}

// ── Script analysis types ──────────────────────────────────────────

/** An import statement from the script block. */
export interface ImportMeta {
  /** Import source path (e.g. `"vue"`, `"./utils"`). */
  source: string;
  /** Whether the entire import is type-only (`import type ...`). */
  isTypeOnly: boolean;
  /** Individual imported bindings. */
  bindings: { name: string; isTypeOnly: boolean }[];
}

/** A script-level binding (variable, function, class, etc.). */
export interface BindingMeta {
  /** Binding name. */
  name: string;
  /** Declaration kind. */
  kind: "const" | "let" | "var" | "function" | "asyncFunction" | "class";
  /** Reactivity classification. */
  reactivityKind: "none" | "ref" | "reactive" | "computed" | "maybeRef" | "mutable";
  /** TS type annotation if present (e.g. `"number"`, `"Ref<string>"`). */
  typeAnnotation?: string;
  /** Whether this binding is used in the template. */
  usedInTemplate: boolean;
  /** Whether this binding is used in a style block (via `v-bind()`). */
  usedInStyle: boolean;
}

/** A Vue API function call site. */
export interface VueApiCallMeta {
  /** API name (e.g. `"OnMounted"`, `"Watch"`, `"Provide"`). */
  api: string;
  /** First string argument value, if available. */
  argValue?: string;
}

// ── Style analysis types ───────────────────────────────────────────

/** Analysis of a single `<style>` block. */
export interface StyleMeta {
  /** Preprocessor language (`"Css"`, `"Scss"`, `"Less"`, etc.). */
  lang: string;
  /** Whether the style block is scoped. */
  scoped: boolean;
  /** Whether this is a CSS module (`<style module>`). */
  isModule: boolean;
  /** Module name if named module (`<style module="foo">`). */
  moduleName?: string;
  /** All class names found in this style block. */
  classes: string[];
  /** All ID selectors found. */
  ids: string[];
  /** CSS custom property names (`--foo`). */
  customProperties: string[];
  /** `v-bind()` expression names used in styles. */
  vBinds: string[];
  /** All selectors with specificity. */
  selectors: SelectorMeta[];
}

/** A CSS selector with its computed specificity. */
export interface SelectorMeta {
  /** Selector text. */
  text: string;
  /** Specificity as `[id, class, type]`. */
  specificity: [number, number, number];
}

// ── Component flags ────────────────────────────────────────────────

// ── Fallthrough surface types ───────────────────────────────────────

/** How a member arrived on the accepted surface. */
export type MemberProvenance =
  | { kind: "declared" }
  | { kind: "inherited"; sources: InheritedSource[] };

/** A single inheritance source. */
export type InheritedSource =
  | { kind: "nativeTag"; tag: string }
  | { kind: "component"; canonicalId: string };

/** Whether a member is always available or only in certain branches. */
export type MemberAvailability = { kind: "always" } | { kind: "conditional"; branchKeys: string[] };

/** Kind of accepted prop. */
export type AcceptedPropKind = "declaredProp" | "attr";

/** Kind of accepted event. */
export type AcceptedEventKind = "declaredEmit" | "listener";

/** Whether the accepted surface is exact or only a lower bound. */
export type AcceptedSurfaceCompleteness = "exact" | "lowerBound";

/** An accepted prop on the computed call-site surface. */
export interface AcceptedPropMeta {
  /** Prop/attr name. */
  name: string;
  /** Parsed type descriptor. */
  type: TypeDescriptor;
  /** Original TS type annotation string. */
  rawType?: string;
  /** Whether the prop is required. */
  required: boolean;
  /** How this member arrived on the surface. */
  provenance: MemberProvenance;
  /** In which branches this member is available. */
  availability: MemberAvailability;
  /** Whether this is a declared prop or an inherited attr. */
  kind: AcceptedPropKind;
}

/** An accepted event on the computed call-site surface. */
export interface AcceptedEventMeta {
  /** Event/listener name. */
  name: string;
  /** Payload type descriptor. */
  payload: TypeDescriptor;
  /** Original emit signature string. */
  rawSignature?: string;
  /** How this member arrived on the surface. */
  provenance: MemberProvenance;
  /** In which branches this member is available. */
  availability: MemberAvailability;
  /** Whether this is a declared emit or an inherited listener. */
  kind: AcceptedEventKind;
}

/** Root reachability classification for fallthrough inheritance. */
export type RootReachability =
  | { kind: "noFallthrough"; reason: NoFallthroughReason }
  | { kind: "branches"; branches: RootBranch[] };

/** Why a component has no fallthrough surface. */
export type NoFallthroughReason =
  | "inheritAttrsFalse"
  | "multiRoot"
  | "branchNotSingleRoot"
  | "rootVFor"
  | "noTemplate"
  | "emptyTemplate"
  | "textOrInterpolationRoot";

/** A single root render branch. */
export interface RootBranch {
  /** Branch index in normalized source order. */
  branchIndex: number;
  /** Condition text for diagnostics only. */
  conditionText?: string;
  /** Root target reference. */
  target: RootTargetRef;
  /** Consumed root bindings. */
  consumed: ConsumedRootBindings;
  /** Whether `v-bind="obj"` spread is used on the root. */
  hasUnknownSpread: boolean;
}

/** The kind of root render target. */
export type RootTargetRef =
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
      reason: UnresolvedRootTargetReason;
    };

/** Why a root target cannot be resolved. */
export type UnresolvedRootTargetReason =
  | { kind: "dynamicComponentIs" }
  | { kind: "slotOutlet" }
  | { kind: "unsupportedBuiltin"; tag: string }
  | { kind: "missingUsageLink" }
  | { kind: "unresolvedImport" }
  | { kind: "unknownRootTarget" };

/** Attrs/listeners explicitly bound on the root element. */
export interface ConsumedRootBindings {
  /** Static attr names consumed on the root. */
  attrs: string[];
  /** Canonical listener names consumed on the root. */
  listeners: string[];
  /** Whether a dynamic attr name is bound. */
  hasDynamicAttrName: boolean;
  /** Whether a dynamic listener name is bound. */
  hasDynamicListenerName: boolean;
}

/** The branch-structured inherited surface. */
export type FallthroughSurface =
  | { kind: "none"; reason: NoFallthroughReason }
  | { kind: "branches"; branches: FallthroughBranch[] };

/** Why generic-root specialization could not resolve a concrete instantiation. */
export type GenericResolutionFailure =
  | "spreadInput"
  | "dynamicKey"
  | "missingType"
  | "unsupportedExpression"
  | "missingUsageLink"
  | "unresolvedChildGenericSurface";

/** Known lower-bound causes for a partially resolved fallthrough branch. */
export type PartialBranchReason =
  | { kind: "dynamicAttrName" }
  | { kind: "dynamicListenerName" }
  | { kind: "unknownSpread" }
  | { kind: "genericResolution"; failure: GenericResolutionFailure };

/** Why a fallthrough branch could not be resolved at all. */
export type UnresolvedBranchReason =
  | { kind: "cycle"; canonicalId: string }
  | { kind: "dynamicComponentIs" }
  | { kind: "childResolutionFailed" }
  | { kind: "unresolvedChildImport"; importSource?: string }
  | { kind: "rootTarget"; reason: UnresolvedRootTargetReason }
  | { kind: "genericResolution"; failure: GenericResolutionFailure };

/** An inherited prop entry in a fallthrough branch. */
export interface FallthroughPropEntry {
  /** Prop/attr name. */
  name: string;
  /** Parsed type descriptor. */
  type: TypeDescriptor;
  /** Original TS type annotation string. */
  rawType?: string;
  /** Where this member was inherited from. */
  sources: InheritedSource[];
}

/** An inherited event entry in a fallthrough branch. */
export interface FallthroughEventEntry {
  /** Event/listener name. */
  name: string;
  /** Payload type descriptor. */
  payload: TypeDescriptor;
  /** Original emit signature string. */
  rawSignature?: string;
  /** Where this member was inherited from. */
  sources: InheritedSource[];
}

/** Status of a fallthrough branch. */
export type BranchStatus =
  | { kind: "resolved" }
  | { kind: "partiallyUnresolved"; reasons: PartialBranchReason[] }
  | { kind: "unresolved"; reason: UnresolvedBranchReason };

/** A single step in the root resolution chain. */
export type ResolvedRootStep =
  | { kind: "nativeTag"; tag: string }
  | { kind: "component"; canonicalId: string; componentName: string }
  | { kind: "unresolved"; tag: string; reason: UnresolvedBranchReason };

/** A single branch in the fallthrough surface. */
export interface FallthroughBranch {
  /** Deterministic branch key. */
  branchKey: string;
  /** Condition text for diagnostics only. */
  conditionText?: string;
  /** Inherited props in this branch (after subtraction). */
  props: FallthroughPropEntry[];
  /** Inherited events in this branch (after subtraction). */
  events: FallthroughEventEntry[];
  /** Chain of root steps traversed to produce this branch. */
  rootChain: ResolvedRootStep[];
  /** Resolution status of this branch. */
  status: BranchStatus;
}

// ── Component flags ────────────────────────────────────────────────

/** Quick boolean flags derived from script analysis flags. */
export interface ComponentFlags {
  /** Whether the setup function is async. */
  asyncSetup: boolean;
  /** Whether the component has reactive state (`ref`, `reactive`, etc.). */
  hasReactiveState: boolean;
  /** Whether the component uses `computed()`. */
  hasComputed: boolean;
  /** Whether the component uses watchers (`watch`, `watchEffect`, etc.). */
  hasWatchers: boolean;
  /** Whether the component has lifecycle hooks. */
  hasLifecycleHooks: boolean;
  /** Whether the component uses `provide()`. */
  hasProvide: boolean;
  /** Whether the component uses `inject()`. */
  hasInject: boolean;
  /** Whether `inheritAttrs: false` is set. */
  hasInheritAttrsFalse: boolean;
  /** Whether the component uses Pinia/Vuex stores. */
  hasStoreUsage: boolean;
}

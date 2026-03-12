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

  // ── Flags ──────────────────────────────────────────────────────

  /** Quick O(1) boolean checks for component characteristics. */
  flags: ComponentFlags;
}

/** Metadata for a single component prop. */
export interface PropMeta {
  /** Prop name as declared in `defineProps` or Options API. */
  name: string;
  /** Parsed type descriptor. */
  type: TypeDescriptor;
  /** Whether the prop is required (no default, no `?`). */
  required: boolean;
  /** Whether the prop has a default value (via `withDefaults` or Options API). */
  hasDefault: boolean;
  /** Original TS type annotation string (e.g. `"string | number"`). */
  rawType?: string;
  /** Vue runtime constructor names (e.g. `["String", "Number"]`). */
  runtimeTypes?: string[];
}

/** Metadata for a single component event. */
export interface EventMeta {
  /** Event name (e.g. `"click"`, `"update:modelValue"`). */
  name: string;
  /** Payload type descriptor. */
  payload: TypeDescriptor;
  /** Whether the event has a runtime validator function. */
  hasValidator: boolean;
  /** Whether the event is explicitly declared (vs. inferred from template usage). */
  isDeclared: boolean;
  /** Original emit signature string. */
  rawSignature?: string;
}

/** Metadata for a single template slot. */
export interface SlotMeta {
  /** Slot name (`"default"` for the unnamed slot). */
  name: string;
  /** Whether the slot exposes scoped bindings. */
  isScoped: boolean;
  /** Scoped slot bindings (empty for non-scoped slots). */
  bindings: SlotBinding[];
}

/** A single binding exposed by a scoped slot. */
export interface SlotBinding {
  /** Binding name available in the slot scope. */
  name: string;
  /** Type descriptor for the binding value. */
  type: TypeDescriptor;
  /** The expression text (e.g. `"row"`, `"i"`) — may differ from `name`. */
  expression?: string;
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

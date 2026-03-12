/**
 * TypeScript interfaces mirroring the Rust analysis types from verter_analysis.
 * All field names use camelCase to match serde(rename_all = "camelCase").
 * `spanStart`/`spanEnd` are always absolute source offsets in the boundary encoding
 * chosen by the transport (negotiated LSP encoding for the language server).
 */

// ── Script Analysis Types ──────────────────────────────────────

export interface AnalyzedImportBinding {
  name: string;
  isTypeOnly: boolean;
  vueApi: string | null;
  spanStart: number;
  spanEnd: number;
}

export interface AnalyzedImport {
  source: string;
  isTypeOnly: boolean;
  bindings: AnalyzedImportBinding[];
  spanStart: number;
  spanEnd: number;
  resolvedCanonicalId: string | null;
}

export type ReactivityKind = "None" | "Ref" | "Computed" | "Reactive" | "MaybeRef" | "Mutable";

export type AnalyzedBindingKind = "Const" | "Let" | "Var" | "Function" | "AsyncFunction" | "Class";

export interface BindingInitializer {
  FunctionCall?: {
    callee: string;
    calleeImportSource: string | null;
    vueApi: string | null;
  };
  Literal?: { kind: string };
  Reference?: { name: string };
  Other?: Record<string, never>;
}

export interface AnalyzedBinding {
  name: string;
  kind: AnalyzedBindingKind;
  isReactive: boolean;
  reactivityKind: ReactivityKind;
  typeAnnotation: string | null;
  initializer: BindingInitializer | null;
  spanStart: number;
  spanEnd: number;
}

export type AnalyzedMacroKind =
  | "DefineProps"
  | "DefineEmits"
  | "DefineModel"
  | "DefineExpose"
  | "DefineOptions"
  | "DefineSlots"
  | "WithDefaults";

export interface AnalyzedMacro {
  kind: AnalyzedMacroKind;
  isTypeBased: boolean;
  typeReferences: string[];
  bindingName: string | null;
  spanStart: number;
  spanEnd: number;
}

export interface MacroTypeDep {
  typeName: string;
  importSource: string;
  macroKind: AnalyzedMacroKind;
}

// ── Template Analysis Types ────────────────────────────────────

export type PropValueConstness = "Const" | "Dynamic" | "Unknown";

export interface TemplatePropUsage {
  name: string;
  isBound: boolean;
  constness: PropValueConstness;
  /** Skipped from JSON when empty (`skip_serializing_if = "Vec::is_empty"`). */
  referencedBindings?: string[];
  fromSpread: boolean;
  spanStart: number;
  spanEnd: number;
}

export interface TemplateComponentVModel {
  bindingName: string;
  spanStart: number;
  spanEnd: number;
}

export interface TemplateComponentUsage {
  name: string;
  /** Skipped from JSON when None (`skip_serializing_if = "Option::is_none"`). */
  importSource?: string | null;
  isDynamic: boolean;
  props: TemplatePropUsage[];
  hasSpread: boolean;
  /** Skipped from JSON when empty (`skip_serializing_if = "Vec::is_empty"`). */
  slotsUsed?: string[];
  /** Skipped from JSON when empty. */
  staticClasses?: string[];
  hasDynamicClass: boolean;
  /** Skipped from JSON when empty. */
  dynamicClasses?: string[];
  /** Skipped from JSON when empty. */
  vModels?: TemplateComponentVModel[];
  spanStart: number;
  spanEnd: number;
}

export type BindingUsageKind =
  | "Interpolation"
  | "DirectiveValue"
  | "EventHandler"
  | "ComponentTag"
  | "TemplateRef"
  | "IteratorSource";

export interface TemplateBindingOccurrence {
  name: string;
  spanStart: number;
  spanEnd: number;
  usageKind: BindingUsageKind;
}

export interface DefinedSlot {
  name: string;
  hasBindings: boolean;
  /** Skipped from JSON when empty. */
  bindingNames?: string[];
  /** Skipped from JSON when empty. */
  bindingExpressions?: string[];
  spanStart: number;
  spanEnd: number;
}

export interface TemplateRef {
  name: string;
  isDynamic: boolean;
  targetTag: string;
}

export interface TemplateEventHandler {
  eventName: string;
  handlerBinding?: string | null;
  isInline: boolean;
  targetTag: string;
  spanStart: number;
  spanEnd: number;
}

export interface TemplateDirective {
  name: string;
  rawName: string;
  argument?: string | null;
  /** Skipped from JSON when empty. */
  modifiers?: string[];
  expression?: string | null;
  spanStart: number;
  spanEnd: number;
  nameEnd?: number;
  argSpanStart?: number;
  argSpanEnd?: number;
  expressionSpanStart?: number;
  expressionSpanEnd?: number;
  /** Skipped from JSON when empty. */
  modifierSpans?: Array<{ start: number; end: number }>;
}

export interface VForDirective {
  variable: string;
  index?: string | null;
  iterable: string;
  hasKey: boolean;
  keyExpression?: string | null;
  keyUsesIndex: boolean;
  spanStart: number;
  spanEnd: number;
}

export interface VModelDirective {
  bindingName: string;
  /** Skipped from JSON when empty. */
  modifiers?: string[];
  targetIsComponent: boolean;
  targetTag: string;
  spanStart: number;
  spanEnd: number;
}

export type ElementNamespace = "html" | "svg" | "mathML";

export interface TemplateAttribute {
  name: string;
  value?: string | null;
  isDynamic: boolean;
  spanStart: number;
  spanEnd: number;
  nameEnd?: number;
  valueSpanStart?: number;
  valueSpanEnd?: number;
}

export interface DynamicStyleVar {
  name: string;
  exprOffset: number;
  valueExpr: string;
  isDynamicKey: boolean;
  isConditional: boolean;
}

export interface StaticStyleVar {
  name: string;
  value: string;
  nameOffset: number;
}

export interface TemplateElement {
  tag: string;
  isComponent: boolean;
  isSelfClosing: boolean;
  namespace: ElementNamespace;
  /** Skipped from JSON when empty. */
  attributes?: TemplateAttribute[];
  /** Skipped from JSON when empty. */
  directives?: TemplateDirective[];
  vFor?: VForDirective | null;
  vModel?: VModelDirective | null;
  hasVIf: boolean;
  hasVElse: boolean;
  hasVElseIf: boolean;
  hasVShow: boolean;
  hasVHtml: boolean;
  hasVText: boolean;
  hasTextContent: boolean;
  hasBareText?: boolean;
  hasElementChildren?: boolean;
  nestingDepth: number;
  parentTag?: string | null;
  parentIndex?: number | null;
  /** Skipped from JSON when empty. */
  dynamicClasses?: string[];
  spanStart: number;
  spanEnd: number;
  tagSpanEnd?: number;
  contentEnd?: number;
  /** Skipped from JSON when empty. */
  dynamicStyleVars?: DynamicStyleVar[];
  /** Skipped from JSON when empty. */
  staticStyleVars?: StaticStyleVar[];
}

export interface IfChain {
  /** Condition expressions with their spans: [expression, spanStart, spanEnd]. */
  conditions: [string, number, number][];
}

export interface AnalyzedPropDefinition {
  name: string;
  typeAnnotation?: string | null;
  hasDefault: boolean;
  isRequired: boolean;
  isBoolean: boolean;
  usedInTemplate: boolean;
  usedInScript: boolean;
  spanStart: number;
  spanEnd: number;
}

export interface AnalyzedEmitDefinition {
  eventName: string;
  hasValidator: boolean;
  isDeclared: boolean;
  /** Skipped from JSON when empty. */
  emitLocations?: [number, number][];
  spanStart: number;
  spanEnd: number;
}

export type CommentDirectiveKind =
  | "Disable"
  | "DisableNextLine"
  | "Enable"
  | "Todo"
  | "Fixme"
  | "Deprecated"
  | "IgnoreStart"
  | "IgnoreEnd";

export interface CommentDirective {
  kind: CommentDirectiveKind;
  message?: string | null;
  spanStart: number;
  spanEnd: number;
  affectsNextLine: boolean;
}

export interface TemplateAnalysisSnapshot {
  components: TemplateComponentUsage[];
  bindingOccurrences: TemplateBindingOccurrence[];
  /** Skipped from JSON when empty. */
  unresolvedBindings?: Array<{ name: string; spanStart: number; spanEnd: number }>;
  /** Skipped from JSON when empty. */
  definedSlots?: DefinedSlot[];
  /** Skipped from JSON when empty. */
  templateRefs?: TemplateRef[];
  /** Skipped from JSON when empty. */
  eventHandlers?: TemplateEventHandler[];
  /** Skipped from JSON when empty. */
  elements?: TemplateElement[];
  /** Skipped from JSON when empty. */
  ifChains?: IfChain[];
  maxNestingDepth: number;
  /** Skipped from JSON when empty. */
  vIfVForConflicts?: [number, number][];
  /** Skipped from JSON when empty. */
  propDefinitions?: AnalyzedPropDefinition[];
  /** Skipped from JSON when empty. */
  emitDefinitions?: AnalyzedEmitDefinition[];
  /** Skipped from JSON when empty. */
  commentDirectives?: CommentDirective[];
  /** Skipped from JSON when empty. */
  cssVarNames?: string[];
}

// ── Style Analysis Types ───────────────────────────────────────

export interface AnalyzedVBind {
  expression: string;
  quoted: boolean;
  start: number;
  end: number;
}

export interface CssVarFallback {
  text: string;
  span: { start: number; end: number };
  nestedVarReferences?: CssVarReference[];
}

export interface CssVarReference {
  name: string;
  span: { start: number; end: number };
  nameSpan: { start: number; end: number };
  fallback?: CssVarFallback | null;
}

export interface CssVarUsage {
  propertyName: string;
  reference: CssVarReference;
  selectorIndex?: number | null;
}

export interface CssAnalysis {
  selectors: Array<{ text: string; specificity: [number, number, number] }>;
  classes: Array<{ name: string }>;
  ids: Array<{ name: string }>;
  customProperties: Array<{ name: string }>;
  atRules: Array<{ kind: string; name: string }>;
  /** var() usages in non-custom-property declarations. */
  varUsages?: CssVarUsage[];
  ruleCount: number;
}

export interface StyleBlockAnalysis {
  lang: string;
  scoped: boolean;
  isModule: boolean;
  moduleName: string | null;
  vBinds: AnalyzedVBind[];
  specialPseudos: Array<{ kind: string; start: number; end: number; inner: string | null }>;
  css: CssAnalysis | null;
  flags: number;
}

// ── Vue API Call Sites ─────────────────────────────────────────

export interface VueApiCallSite {
  api: string;
  spanStart: number;
  spanEnd: number;
  /** First string argument value (e.g., provide key, useTemplateRef name). */
  argValue?: string | null;
  /** Whether the call has type parameters (e.g., `ref<string>()`). */
  hasTypeParams?: boolean;
  /** Whether the first function argument is async. */
  isAsyncCallback?: boolean;
}

// ── DOM Query Call Sites ──────────────────────────────────────

export type DomQueryKind =
  | "QuerySelector"
  | "QuerySelectorAll"
  | "GetElementById"
  | "GetElementsByClassName";

export interface DomQueryCallSite {
  kind: DomQueryKind;
  selectorText: string;
  parsed?: unknown | null;
  spanStart: number;
  spanEnd: number;
  argSpanStart: number;
  argSpanEnd: number;
}

// ── CSS Variable Manipulations ────────────────────────────────

export type CssVarManipulationKind = "SetProperty" | "GetPropertyValue" | "RemoveProperty";

export interface CssVarManipulation {
  kind: CssVarManipulationKind;
  varName: string;
  valueExpr?: string | null;
  spanStart: number;
  spanEnd: number;
}

// ── File Analysis Snapshot (top-level response) ─────────────────

export interface FileAnalysisSnapshot {
  imports: AnalyzedImport[];
  bindings: AnalyzedBinding[];
  macros: AnalyzedMacro[];
  macroTypeDeps: MacroTypeDep[];
  scriptFlags: number;
  styles: StyleBlockAnalysis[];
  template: TemplateAnalysisSnapshot | null;
  /** Vue API call sites (lifecycle hooks, watchers, provide/inject, etc.). */
  vueApiCalls?: VueApiCallSite[];
  /** DOM query call sites (querySelector, getElementById, etc.). */
  domQueryCalls?: DomQueryCallSite[];
  /** CSS variable manipulations via DOM style APIs. */
  cssVarManipulations?: CssVarManipulation[];
}

// ── Project Overview ────────────────────────────────────────────

export interface ProjectOverviewFile {
  path: string;
  kind: "vue" | "ts" | "js";
}

export interface ProjectOverviewComponentEdge {
  file: string;
  usesComponents: string[];
}

export interface ProjectOverviewStats {
  totalVueFiles: number;
  totalComponents: number;
  totalProvideKeys: number;
  totalInjectKeys: number;
  filesWithScopedStyles: number;
}

export interface ProjectOverview {
  files: ProjectOverviewFile[];
  componentGraph: ProjectOverviewComponentEdge[];
  stats: ProjectOverviewStats;
}

// ── Component Parents Response ──────────────────────────────────

export interface ComponentParentInfo {
  /** File path of the parent file */
  filePath: string;
  /** Component name as used in the parent's template */
  componentName: string;
  /** Props passed by this parent */
  props: TemplatePropUsage[];
  /** Slots used by this parent */
  slotsUsed: string[];
}

export interface ComponentParentsResponse {
  /** The component file being queried */
  componentPath: string;
  /** Files that use this component */
  parents: ComponentParentInfo[];
}

// ── Virtual Files Response ──────────────────────────────────────

export interface VirtualFileEntry {
  kind: string;
  code: string;
  lang: string;
  sourceMap: string | null;
  stale: boolean;
}

export interface CodeBlock {
  code: string;
  sourceMap: string | null;
  /** `true` when the SFC script is JavaScript rather than TypeScript. */
  isJs: boolean;
}

export interface VirtualFilesResponse {
  ide: CodeBlock | null;
  api: CodeBlock | null;
  virtualFiles: VirtualFileEntry[];
}

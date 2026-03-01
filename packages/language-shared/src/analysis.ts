/**
 * TypeScript interfaces mirroring the Rust analysis types from verter_analysis.
 * All field names use camelCase to match serde(rename_all = "camelCase").
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

export interface TemplateComponentUsage {
  name: string;
  /** Skipped from JSON when None (`skip_serializing_if = "Option::is_none"`). */
  importSource?: string | null;
  isDynamic: boolean;
  props: TemplatePropUsage[];
  hasSpread: boolean;
  /** Skipped from JSON when empty (`skip_serializing_if = "Vec::is_empty"`). */
  slotsUsed?: string[];
  spanStart: number;
  spanEnd: number;
}

export interface TemplateBindingOccurrence {
  name: string;
  spanStart: number;
  spanEnd: number;
  usageKind: string;
}

export interface DefinedSlot {
  name: string;
  hasBindings: boolean;
}

export interface TemplateRef {
  name: string;
  isDynamic: boolean;
  targetTag: string;
}

export interface TemplateEventHandler {
  eventName: string;
  handlerBinding: string | null;
  isInline: boolean;
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
  maxNestingDepth: number;
  /** Skipped from JSON when empty. */
  vIfVForConflicts?: [number, number][];
}

// ── Style Analysis Types ───────────────────────────────────────

export interface AnalyzedVBind {
  expression: string;
  quoted: boolean;
  start: number;
  end: number;
}

export interface CssAnalysis {
  selectors: Array<{ text: string; specificity: [number, number, number] }>;
  classes: Array<{ name: string }>;
  ids: Array<{ name: string }>;
  customProperties: Array<{ name: string }>;
  atRules: Array<{ kind: string; name: string }>;
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

export interface TsxBlock {
  code: string;
  sourceMap: string | null;
}

export interface VirtualFilesResponse {
  tsx: TsxBlock | null;
  virtualFiles: VirtualFileEntry[];
}

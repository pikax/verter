export interface HostDiagnostic {
  severity: "error" | "warning" | "info";
  code: string;
  message: string;
  spanStart?: number;
  spanEnd?: number;
}

export interface DestructuredBlockMeta {
  bindings: Array<{ name: string; sourceStart: number; sourceEnd: number }>;
  blockStart: number;
  blockEnd: number;
}

export interface CompiledFile {
  js: string;
  css: string;
  types: string;
  typesSourceMap: string;
  destructuredBlock: DestructuredBlockMeta | null;
  /** Raw template render function code (before merging into assembled JS). */
  templateCode: string;
  verterSourceMap: string;
  /** TSC declaration output (minimal .d.ts). */
  tscCode: string;
  /** SSR-compiled JS output (when SSR mode is enabled). */
  ssrCode: string;
  errors: string[];
  compilerDiagnostics: HostDiagnostic[];
  analysis: FileAnalysis | null;
  lintDiagnostics: LintDiagnostic[];
}

export class File {
  filename: string;
  code: string;
  compiled: CompiledFile = {
    js: "",
    css: "",
    types: "",
    typesSourceMap: "",
    destructuredBlock: null,
    templateCode: "",
    verterSourceMap: "",
    tscCode: "",
    ssrCode: "",
    errors: [],
    compilerDiagnostics: [],
    analysis: null,
    lintDiagnostics: [],
  };

  constructor(filename: string, code = "") {
    this.filename = filename;
    this.code = code;
  }

  get language(): "vue" | "typescript" | "javascript" | "css" | "json" {
    if (this.filename.endsWith(".vue")) return "vue";
    if (this.filename.endsWith(".ts")) return "typescript";
    if (this.filename.endsWith(".js")) return "javascript";
    if (this.filename.endsWith(".css")) return "css";
    if (this.filename.endsWith(".json")) return "json";
    return "typescript";
  }

  /** Whether this file contains TypeScript */
  get isTS(): boolean {
    if (this.filename.endsWith(".ts") || this.filename.endsWith(".tsx")) return true;
    if (this.filename.endsWith(".vue")) {
      return /<script[^>]*\blang\s*=\s*["'](ts|tsx)["']/.test(this.code);
    }
    return false;
  }
}

export type OutputMode = "preview" | "js" | "ssr" | "css" | "types" | "tsc" | "analysis" | "lint" | "outline" | "files" | "cssMatch" | "map" | "diagnostics";

export interface TsDiagnosticEntry {
  message: string;
  start: number;
  end: number;
  severity: "error" | "warning" | "info";
  code: number;
}

export interface CompilerOptions {
  isProduction: boolean;
  ssr: boolean;
}

export interface CompileTiming {
  verterNewJs: number | null; // ms for total compilation (JS-measured)
  parseDurationMs: number | null; // ms for Rust parse phase
  scriptMs: number | null; // ms for script codegen
  templateMs: number | null; // ms for template codegen
  styleMs: number | null; // ms for style codegen (total)
  tsxMs: number | null; // ms for IDE/TSX codegen
  tscMs: number | null; // ms for public API codegen
  lintMs: number | null; // ms for lint execution
}

export type TypeCheckerMode = "tsc" | "tsgo";
export type TypeCheckerStatus = "active" | "unavailable" | "initializing";

export interface StoreState {
  files: Record<string, File>;
  activeFilename: string;
  mainFile: string;
  errors: string[];
  outputMode: OutputMode;
  loading: boolean;
  darkMode: boolean;
  autoSave: boolean;
  compilerOptions: CompilerOptions;
  compileTiming: CompileTiming;
  typeChecker: TypeCheckerMode;
  typeCheckerStatus: TypeCheckerStatus;
  currentProjectName: string | null;
  editableOutput: boolean;
  tsxUserEdited: boolean;
  tsxOverrideCode: string | null;
}

// ── Analysis types (mirror Rust FileAnalysisSnapshot) ──

export interface FileAnalysis {
  imports: AnalysisImport[];
  bindings: AnalysisBinding[];
  macros: AnalysisMacro[];
  macroTypeDeps: AnalysisMacroTypeDep[];
  scriptFlags: number;
  styles: AnalysisStyleBlock[];
}

export interface AnalysisImport {
  source: string;
  isTypeOnly: boolean;
  bindings: AnalysisImportBinding[];
}

export interface AnalysisImportBinding {
  name: string;
  isTypeOnly: boolean;
  vueApi: string | null;
}

export interface AnalysisBinding {
  name: string;
  kind: string;
  isReactive: boolean;
  reactivityKind?: string;
  typeAnnotation?: string | null;
  initializer: AnalysisBindingInitializer | null;
}

export type AnalysisBindingInitializer =
  | { FunctionCall: { callee: string; calleeImportSource: string | null; vueApi: string | null } }
  | { Literal: { kind: string } }
  | { Reference: { name: string } }
  | "Other";

export interface AnalysisMacro {
  kind: string;
  isTypeBased: boolean;
  typeReferences: string[];
  bindingName: string | null;
}

export interface AnalysisMacroTypeDep {
  typeName: string;
  importSource: string;
  macroKind: string;
}

export interface AnalysisStyleBlock {
  lang: string;
  scoped: boolean;
  isModule: boolean;
  moduleName: string | null;
  vBinds: AnalysisVBind[];
  specialPseudos: AnalysisSpecialPseudo[];
  css: AnalysisCss | null;
  flags: number;
}

export interface AnalysisVBind {
  expression: string;
  quoted: boolean;
  start: number;
  end: number;
}

export interface AnalysisSpecialPseudo {
  kind: string;
  start: number;
  end: number;
  inner: string | null;
}

export interface AnalysisCss {
  selectors: AnalysisCssSelector[];
  classes: AnalysisCssClass[];
  ids: AnalysisCssId[];
  customProperties: AnalysisCssCustomProperty[];
  atRules: AnalysisCssAtRule[];
  ruleCount: number;
}

export interface AnalysisCssSelector {
  text: string;
  specificity: [number, number, number];
  start: number;
  end: number;
}

export interface AnalysisCssClass {
  name: string;
  start: number;
  end: number;
}

export interface AnalysisCssId {
  name: string;
  start: number;
  end: number;
}

export interface AnalysisCssCustomProperty {
  name: string;
  start: number;
  end: number;
}

export interface AnalysisCssAtRule {
  kind: string;
  name: string;
  start: number;
  end: number;
}

/** Bitwise flags for quick analysis queries (mirrors Rust AnalysisFlags) */
export const AnalysisFlags = {
  ASYNC_SETUP: 1 << 0,
  HAS_DEFINE_PROPS: 1 << 1,
  HAS_DEFINE_EMITS: 1 << 2,
  HAS_DEFINE_MODEL: 1 << 3,
  HAS_DEFINE_EXPOSE: 1 << 4,
  HAS_DEFINE_OPTIONS: 1 << 5,
  HAS_DEFINE_SLOTS: 1 << 6,
  HAS_WITH_DEFAULTS: 1 << 7,
  HAS_TYPE_BASED_PROPS: 1 << 8,
  HAS_TYPE_BASED_EMITS: 1 << 9,
  HAS_TYPE_BASED_MODEL: 1 << 10,
  HAS_REACTIVE_STATE: 1 << 11,
  HAS_COMPUTED: 1 << 12,
  HAS_WATCHERS: 1 << 13,
  HAS_LIFECYCLE_HOOKS: 1 << 14,
  HAS_PROVIDE: 1 << 15,
  HAS_INJECT: 1 << 16,
  HAS_EXTERNAL_TYPE_DEPS: 1 << 17,
} as const;

/** Human-readable labels for AnalysisFlags bits */
export const AnalysisFlagLabels: Record<number, string> = {
  [AnalysisFlags.ASYNC_SETUP]: "Async Setup",
  [AnalysisFlags.HAS_DEFINE_PROPS]: "defineProps",
  [AnalysisFlags.HAS_DEFINE_EMITS]: "defineEmits",
  [AnalysisFlags.HAS_DEFINE_MODEL]: "defineModel",
  [AnalysisFlags.HAS_DEFINE_EXPOSE]: "defineExpose",
  [AnalysisFlags.HAS_DEFINE_OPTIONS]: "defineOptions",
  [AnalysisFlags.HAS_DEFINE_SLOTS]: "defineSlots",
  [AnalysisFlags.HAS_WITH_DEFAULTS]: "withDefaults",
  [AnalysisFlags.HAS_TYPE_BASED_PROPS]: "Type-based Props",
  [AnalysisFlags.HAS_TYPE_BASED_EMITS]: "Type-based Emits",
  [AnalysisFlags.HAS_TYPE_BASED_MODEL]: "Type-based Model",
  [AnalysisFlags.HAS_REACTIVE_STATE]: "Reactive State",
  [AnalysisFlags.HAS_COMPUTED]: "Computed",
  [AnalysisFlags.HAS_WATCHERS]: "Watchers",
  [AnalysisFlags.HAS_LIFECYCLE_HOOKS]: "Lifecycle Hooks",
  [AnalysisFlags.HAS_PROVIDE]: "Provide",
  [AnalysisFlags.HAS_INJECT]: "Inject",
  [AnalysisFlags.HAS_EXTERNAL_TYPE_DEPS]: "External Type Deps",
};

// ── Lint diagnostic types (mirror Rust verter_linter) ──

export type LintSeverity = "error" | "warning" | "info";

export interface LintDiagnostic {
  rule: string;
  category: string;
  severity: LintSeverity;
  message: string;
  spanStart: number;
  spanEnd: number;
  fix?: LintFix;
}

export interface LintFix {
  description: string;
  replacement: string;
  spanStart: number;
  spanEnd: number;
}

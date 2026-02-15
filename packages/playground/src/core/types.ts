export interface CompiledFile {
  ts: string;
  js: string;
  css: string;
  tsx: string;
  kai: string;
  errors: string[];
  sourceMap: string;
}

export class File {
  filename: string;
  code: string;
  compiled: CompiledFile = {
    ts: "",
    js: "",
    css: "",
    tsx: "",
    kai: "",
    errors: [],
    sourceMap: "",
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

export type OutputMode = "preview" | "ts" | "js" | "css" | "tsx" | "kai";

export interface CompilerOptions {
  isProduction: boolean;
  ssr: boolean;
}

export interface CompileTiming {
  verter: number | null; // ms for Vue SFC compilation (JS-measured)
  verterNative: number | null; // ms for Rust pipeline (reported by Rust)
  stripTypes: number | null; // ms for stripTypes call (when showTS is enabled)
  tsx: number | null; // ms for TSX generation (reported by Rust, when showTSX is enabled)
  kai: number | null; // ms for kai codegen pipeline (reported by Rust)
  kaiJs: number | null; // ms for kai codegen (JS-measured)
}

export interface StoreState {
  files: Record<string, File>;
  activeFilename: string;
  mainFile: string;
  errors: string[];
  outputMode: OutputMode;
  loading: boolean;
  darkMode: boolean;
  autoSave: boolean;
  showTS: boolean;
  showTSX: boolean;
  compilerOptions: CompilerOptions;
  compileTiming: CompileTiming;
}

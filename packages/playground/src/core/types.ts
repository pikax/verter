export interface CompiledFile {
  ts: string;
  js: string;
  css: string;
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

  /** Whether this file contains TypeScript that needs OXC transpilation */
  get isTS(): boolean {
    if (this.filename.endsWith(".ts") || this.filename.endsWith(".tsx")) return true;
    if (this.filename.endsWith(".vue")) {
      return /<script[^>]*\blang\s*=\s*["'](ts|tsx)["']/.test(this.code);
    }
    return false;
  }
}

export type OutputMode = "preview" | "ts" | "js" | "css";

export interface CompilerOptions {
  isProduction: boolean;
  ssr: boolean;
}

export interface CompileTiming {
  verter: number | null; // ms for Vue SFC → TS (JS-measured)
  verterNative: number | null; // ms for Rust pipeline (reported by Rust)
  oxc: number | null; // ms for TS → JS
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
  compilerOptions: CompilerOptions;
  compileTiming: CompileTiming;
}

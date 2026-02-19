export interface CompiledFile {
  js: string;
  css: string;
  verterSourceMap: string;
  errors: string[];
}

export class File {
  filename: string;
  code: string;
  compiled: CompiledFile = {
    js: "",
    css: "",
    verterSourceMap: "",
    errors: [],
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

export type OutputMode = "preview" | "js" | "css";

export interface CompilerOptions {
  isProduction: boolean;
  ssr: boolean;
}

export interface CompileTiming {
  verterNew: number | null; // ms for new_impl codegen pipeline (reported by Rust)
  verterNewJs: number | null; // ms for new_impl codegen (JS-measured)
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

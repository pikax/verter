import type { WasmDiagnostic } from "@verter/wasm";
import type { File, CompilerOptions, CompileTiming } from "./types";
import { loadLocalWasm, loadCommitWasm, loadReleaseWasm, type WasmModule } from "./wasmLoader";
import type { VersionEntry } from "./versions";

/** Result shape returned by compileVerter WASM binding. */
export interface VerterCompileResult {
  script: {
    code: string;
    durationMs: number;
    sourceMap: string;
    setup: boolean;
    attrs: [string, string][];
  } | null;
  template: {
    code: string;
    sourceMap: string;
    imports: string[];
    durationMs: number;
    attrs: [string, string][];
  } | null;
  styles: Array<{
    code: string;
    scoped: boolean;
    lang: string | null;
    durationMs: number;
    attrs: [string, string][];
  }>;
  customBlocks: Array<{
    type: string;
    content: string;
    attrs: [string, string][];
  }>;
  scopeId: string;
  errors: WasmDiagnostic[];
  parseDurationMs: number;
  totalDurationMs: number;
}

/** Convert structured WASM diagnostics to display strings. */
export function formatDiagnostics(diagnostics: WasmDiagnostic[] | undefined): string[] {
  if (!diagnostics || diagnostics.length === 0) return [];
  return diagnostics.map((d) => {
    const loc = d.spanStart != null ? ` (${d.spanStart}:${d.spanEnd ?? d.spanStart})` : "";
    return `[${d.severity}] ${d.message}${loc}`;
  });
}

let wasmCompileVerter: ((input: string, options?: unknown) => VerterCompileResult) | null = null;
let initialized = false;
let initPromise: Promise<void> | null = null;

export async function initCompilers(): Promise<void> {
  if (initialized) return;
  if (initPromise) return initPromise;

  initPromise = (async () => {
    const wasmModule = await loadLocalWasm();
    wasmCompileVerter = (wasmModule.compileVerter as typeof wasmCompileVerter) ?? null;
    initialized = true;
  })();

  return initPromise;
}

/**
 * Switch the WASM compiler to a different version.
 * Loads the appropriate WASM module based on the version entry type.
 */
export async function switchWasmVersion(entry: VersionEntry): Promise<void> {
  let wasmModule: WasmModule;

  if (entry.type === "local") {
    wasmModule = await loadLocalWasm();
  } else if (entry.type === "commit" && entry.sha) {
    wasmModule = await loadCommitWasm(entry.sha);
  } else if (entry.type === "release" && entry.version) {
    wasmModule = await loadReleaseWasm(entry.version);
  } else {
    throw new Error(`Unknown version type: ${entry.type}`);
  }

  wasmCompileVerter = (wasmModule.compileVerter as typeof wasmCompileVerter) ?? null;
}

/**
 * Post-process the Verter WASM output to match the Vue SFC playground format.
 *
 * Transforms:
 *   export default _defineComponent({...}); function render(...) {...}
 * Into:
 *   const __sfc__ = _defineComponent({...}); function render(...) {...}
 *   __sfc__.render = render; export default __sfc__;
 *
 * This ensures the render function is properly attached to the component object.
 */
export function mergeRenderIntoComponent(code: string): string {
  let merged = code;

  // Detect if compiler already used "const __sfc__" (scoped styles emit this)
  const hasSfcVariable = /^const __sfc__ = /m.test(merged);

  if (!hasSfcVariable) {
    // Non-scoped: transform "export default" → "const __sfc__ ="
    merged = merged.replace(/^export default /m, "const __sfc__ = ");
  }

  // Only attach render if the output contains a render function declaration
  const hasRender = /^function render\s*\(/m.test(merged);

  // Find insertion point: before existing "export default __sfc__" or at end
  const exportMatch = merged.indexOf("\nexport default __sfc__");
  const insertPoint = exportMatch !== -1 ? exportMatch : merged.length;

  let attachment = "";
  if (hasRender) {
    attachment += "\n__sfc__.render = render;";
  }
  if (exportMatch === -1) {
    // No "export default __sfc__" yet — add it
    attachment += "\nexport default __sfc__;\n";
  }

  merged = merged.slice(0, insertPoint) + attachment + merged.slice(insertPoint);
  return merged;
}

/** Format an internal helper name as an import specifier.
 *  e.g. "_createElementVNode" → "createElementVNode as _createElementVNode" */
export function formatImportSpecifier(name: string): string {
  if (name.startsWith("_") && name.length > 1) {
    return `${name.slice(1)} as ${name}`;
  }
  return name;
}

/** Assemble new_impl VerterCompileResult blocks into a single JS string. */
export function assembleVerterResult(result: VerterCompileResult): string {
  const parts: string[] = [];
  if (result.template?.imports?.length) {
    const specifiers = result.template.imports.map(formatImportSpecifier);
    parts.push(`import { ${specifiers.join(", ")} } from "vue"\n`);
  }
  if (result.script) parts.push(result.script.code);
  if (result.template) parts.push(result.template.code);
  return parts.join("\n");
}

export async function compileFile(
  file: File,
  options?: CompilerOptions,
): Promise<CompileTiming> {
  await initCompilers();
  const timing: CompileTiming = { verterNew: null, verterNewJs: null };

  if (file.filename.endsWith(".vue")) {
    try {
      if (!wasmCompileVerter) throw new Error("compileVerter WASM binding not available");

      const start = performance.now();
      const result = wasmCompileVerter(file.code, {
        filename: file.filename,
        isProduction: options?.isProduction ?? false,
        stripTs: true,
        sourceMap: true,
      });
      timing.verterNewJs = performance.now() - start;
      timing.verterNew = result.totalDurationMs ?? null;

      const assembled = assembleVerterResult(result);
      file.compiled.js = mergeRenderIntoComponent(assembled);
      file.compiled.css = result.styles.map((s) => s.code).join("\n");
      file.compiled.verterSourceMap = result.template?.sourceMap ?? "";
      file.compiled.errors = formatDiagnostics(result.errors);
    } catch (e) {
      file.compiled.errors = [e instanceof Error ? e.message : String(e)];
    }
  } else if (file.filename.endsWith(".ts")) {
    try {
      if (!wasmCompileVerter) throw new Error("compileVerter WASM binding not available");

      const sfc = `<script setup lang="ts">\n${file.code}\n</script>`;
      const start = performance.now();
      const result = wasmCompileVerter(sfc, {
        filename: file.filename.replace(".ts", ".vue"),
        isProduction: options?.isProduction ?? false,
        stripTs: true,
        sourceMap: true,
      });
      timing.verterNewJs = performance.now() - start;
      timing.verterNew = result.totalDurationMs ?? null;

      file.compiled.js = result.script?.code ?? "";
      file.compiled.errors = formatDiagnostics(result.errors);
    } catch (e) {
      file.compiled.js = "";
      file.compiled.errors = [e instanceof Error ? e.message : String(e)];
    }
  } else if (file.filename.endsWith(".js")) {
    file.compiled.js = file.code;
    file.compiled.errors = [];
  } else if (file.filename.endsWith(".css")) {
    file.compiled.css = file.code;
    file.compiled.errors = [];
  }

  return timing;
}

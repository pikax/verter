import type { CodegenResult, StripTypesResult, WasmDiagnostic } from "@verter/wasm";
import type { File, CompilerOptions, CompileTiming } from "./types";
import { loadLocalWasm, loadCommitWasm, loadReleaseWasm, type WasmModule } from "./wasmLoader";
import type { VersionEntry } from "./versions";

/** Convert structured WASM diagnostics to display strings. */
function formatDiagnostics(diagnostics: WasmDiagnostic[] | undefined): string[] {
  if (!diagnostics || diagnostics.length === 0) return [];
  return diagnostics.map((d) => {
    const loc = d.spanStart != null ? ` (${d.spanStart}:${d.spanEnd ?? d.spanStart})` : "";
    return `[${d.severity}] ${d.message}${loc}`;
  });
}

let wasmCompile: ((input: string, options?: unknown) => CodegenResult) | null = null;
let wasmStripTypes: ((source: string) => StripTypesResult) | null = null;
let initialized = false;
let initPromise: Promise<void> | null = null;

export async function initCompilers(): Promise<void> {
  if (initialized) return;
  if (initPromise) return initPromise;

  initPromise = (async () => {
    const wasmModule = await loadLocalWasm();
    wasmCompile = wasmModule.compile as typeof wasmCompile;
    wasmStripTypes = (wasmModule.stripTypes as typeof wasmStripTypes) ?? null;
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

  wasmCompile = wasmModule.compile as typeof wasmCompile;
  wasmStripTypes = (wasmModule.stripTypes as typeof wasmStripTypes) ?? null;
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

function compileInner(
  source: string,
  filename: string,
  options?: CompilerOptions,
  includeTsx?: boolean,
): CodegenResult {
  if (!wasmCompile) {
    throw new Error("WASM compiler not initialized");
  }
  const result = wasmCompile(source, {
    filename,
    isProduction: options?.isProduction ?? false,
    includeTsx: includeTsx ?? false,
  });
  result.code = mergeRenderIntoComponent(result.code);
  return result;
}

export async function compileVueSFC(
  source: string,
  filename: string,
  options?: CompilerOptions,
): Promise<CodegenResult> {
  await initCompilers();
  return compileInner(source, filename, options);
}

/** Extract raw CSS from <style> blocks in a Vue SFC source */
function extractStyles(source: string): string {
  const styleRegex = /<style[^>]*>([\s\S]*?)<\/style>/gi;
  const styles: string[] = [];
  let match;
  while ((match = styleRegex.exec(source)) !== null) {
    styles.push(match[1].trim());
  }
  return styles.join("\n");
}

export async function compileFile(
  file: File,
  options?: CompilerOptions,
  showTS?: boolean,
  showTSX?: boolean,
): Promise<CompileTiming> {
  await initCompilers();
  const timing: CompileTiming = { verter: null, verterNative: null, stripTypes: null, tsx: null, kai: null, kaiJs: null };

  if (file.filename.endsWith(".vue")) {
    try {
      if (showTS && file.isTS) {
        // Show TS mode: compile, then stripTypes for JS
        const verterStart = performance.now();
        const verterResult = compileInner(file.code, file.filename, options, showTSX);
        timing.verter = performance.now() - verterStart;
        timing.verterNative = (verterResult as any).durationMs ?? null;
        timing.tsx = (verterResult as any).tsxDurationMs ?? null;

        file.compiled.sourceMap = verterResult.sourceMap ?? "";
        file.compiled.css = verterResult.styles?.length
          ? verterResult.styles.map((s) => s.code).join("\n")
          : verterResult.css || extractStyles(file.code);
        file.compiled.tsx = verterResult.tsx ?? "";
        file.compiled.ts = verterResult.code;
        file.compiled.kai = verterResult.code;

        // Collect compiler diagnostics (missing end tags, invalid end tags, etc.)
        const compilerErrors = formatDiagnostics(verterResult.errors);

        // Strip types for JS tab
        if (wasmStripTypes) {
          const stripStart = performance.now();
          const jsResult = wasmStripTypes(verterResult.code);
          timing.stripTypes = performance.now() - stripStart;
          file.compiled.js = jsResult.code;
          file.compiled.errors = [...compilerErrors, ...(jsResult.errors ?? [])];
        } else {
          file.compiled.js = verterResult.code;
          file.compiled.errors = compilerErrors;
        }
      } else {
        // Default: compile → JS directly
        const verterStart = performance.now();
        const verterResult = compileInner(file.code, file.filename, options, showTSX);
        timing.verter = performance.now() - verterStart;
        timing.verterNative = (verterResult as any).durationMs ?? null;
        timing.tsx = (verterResult as any).tsxDurationMs ?? null;

        file.compiled.sourceMap = verterResult.sourceMap ?? "";
        file.compiled.css = verterResult.styles?.length
          ? verterResult.styles.map((s) => s.code).join("\n")
          : verterResult.css || extractStyles(file.code);
        file.compiled.tsx = verterResult.tsx ?? "";
        file.compiled.js = verterResult.code;
        file.compiled.ts = "";
        file.compiled.kai = verterResult.code;
        file.compiled.errors = formatDiagnostics(verterResult.errors);
      }

      console.log(
        `Compiled ${file.filename} in ${timing.verter}ms (WASM:${timing.verterNative ?? "N/A"}ms)`,
      );
    } catch (e) {
      file.compiled.errors = [e instanceof Error ? e.message : String(e)];
    }
  } else if (file.filename.endsWith(".ts")) {
    // Standalone .ts files: wrap in SFC and compile
    try {
      const sfc = `<script setup lang="ts">\n${file.code}\n</script>`;
      if (showTS) {
        // Show TS mode: compile, then stripTypes
        file.compiled.ts = file.code;
        const verterStart = performance.now();
        const result = compileInner(
          sfc,
          file.filename.replace(".ts", ".vue"),
          undefined,
          showTSX,
        );
        timing.verter = performance.now() - verterStart;
        timing.tsx = (result as any).tsxDurationMs ?? null;
        file.compiled.tsx = result.tsx ?? "";

        const tsCompilerErrors = formatDiagnostics(result.errors);
        if (wasmStripTypes) {
          const stripStart = performance.now();
          const jsResult = wasmStripTypes(result.code);
          timing.stripTypes = performance.now() - stripStart;
          file.compiled.js = jsResult.code;
          file.compiled.errors = [...tsCompilerErrors, ...(jsResult.errors ?? [])];
        } else {
          file.compiled.js = result.code;
          file.compiled.errors = tsCompilerErrors;
        }
      } else {
        // Default: compile → JS directly
        file.compiled.ts = "";
        const verterStart = performance.now();
        const result = compileInner(
          sfc,
          file.filename.replace(".ts", ".vue"),
          undefined,
          showTSX,
        );
        timing.verter = performance.now() - verterStart;
        timing.tsx = (result as any).tsxDurationMs ?? null;
        file.compiled.tsx = result.tsx ?? "";
        file.compiled.js = result.code;
        file.compiled.errors = formatDiagnostics(result.errors);
      }
    } catch (e) {
      file.compiled.js = "";
      file.compiled.tsx = "";
      file.compiled.errors = [e instanceof Error ? e.message : String(e)];
    }
  } else if (file.filename.endsWith(".js")) {
    file.compiled.js = file.code;
    file.compiled.ts = "";
    file.compiled.tsx = "";
    file.compiled.errors = [];
  } else if (file.filename.endsWith(".css")) {
    file.compiled.css = file.code;
    file.compiled.tsx = "";
    file.compiled.errors = [];
  }

  return timing;
}

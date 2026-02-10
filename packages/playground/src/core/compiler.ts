import type { CodegenResult, StripTypesResult } from "@verter/wasm";
import type { File, CompilerOptions, CompileTiming } from "./types";
import { loadLocalWasm, loadCommitWasm, loadReleaseWasm, type WasmModule } from "./wasmLoader";
import type { VersionEntry } from "./versions";

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
function mergeRenderIntoComponent(code: string): string {
  // Replace `export default` (the component export) with `const __sfc__ =`
  let merged = code.replace(/^export default /m, "const __sfc__ = ");

  // Only attach render if the output contains a render function declaration
  const hasRender = /^function render\s*\(/m.test(merged);

  // Insert merge + export before CSS exports or at end
  const cssExportIndex = merged.indexOf("\nexport const __css__");
  const insertPoint = cssExportIndex !== -1 ? cssExportIndex : merged.length;
  const attachment = hasRender
    ? "\n__sfc__.render = render;\nexport default __sfc__;\n"
    : "\nexport default __sfc__;\n";
  merged = merged.slice(0, insertPoint) + attachment + merged.slice(insertPoint);

  return merged;
}

function compileInner(
  source: string,
  filename: string,
  options?: CompilerOptions,
  keepTs?: boolean,
  includeTsx?: boolean,
): CodegenResult {
  if (!wasmCompile) {
    throw new Error("WASM compiler not initialized");
  }
  const result = wasmCompile(source, {
    filename,
    includeSourceContent: true,
    isProduction: options?.isProduction ?? false,
    ssr: options?.ssr ?? false,
    keepTs: keepTs ?? false,
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
  return compileInner(source, filename, options, false);
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
  const timing: CompileTiming = { verter: null, verterNative: null, stripTypes: null, tsx: null };

  if (file.filename.endsWith(".vue")) {
    try {
      if (showTS && file.isTS) {
        // Show TS mode: compile with keepTs: true, then stripTypes for JS
        const verterStart = performance.now();
        const verterResult = compileInner(file.code, file.filename, options, true, showTSX);
        timing.verter = performance.now() - verterStart;
        timing.verterNative = (verterResult as any).durationMs ?? null;
        timing.tsx = (verterResult as any).tsxDurationMs ?? null;

        file.compiled.sourceMap = verterResult.sourceMap ?? "";
        file.compiled.css = verterResult.css || extractStyles(file.code);
        file.compiled.tsx = verterResult.tsx ?? "";
        file.compiled.ts = verterResult.code;

        // Strip types for JS tab
        if (wasmStripTypes) {
          const stripStart = performance.now();
          const jsResult = wasmStripTypes(verterResult.code);
          timing.stripTypes = performance.now() - stripStart;
          file.compiled.js = jsResult.code;
          file.compiled.errors = jsResult.errors ?? [];
        } else {
          file.compiled.js = verterResult.code;
          file.compiled.errors = [];
        }
      } else {
        // Default: compile with keepTs: false → JS directly
        const verterStart = performance.now();
        const verterResult = compileInner(file.code, file.filename, options, false, showTSX);
        timing.verter = performance.now() - verterStart;
        timing.verterNative = (verterResult as any).durationMs ?? null;
        timing.tsx = (verterResult as any).tsxDurationMs ?? null;

        file.compiled.sourceMap = verterResult.sourceMap ?? "";
        file.compiled.css = verterResult.css || extractStyles(file.code);
        file.compiled.tsx = verterResult.tsx ?? "";
        file.compiled.js = verterResult.code;
        file.compiled.ts = "";
        file.compiled.errors = [];
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
        // Show TS mode: compile with keepTs: true, then stripTypes
        file.compiled.ts = file.code;
        const verterStart = performance.now();
        const result = compileInner(
          sfc,
          file.filename.replace(".ts", ".vue"),
          undefined,
          true,
          showTSX,
        );
        timing.verter = performance.now() - verterStart;
        timing.tsx = (result as any).tsxDurationMs ?? null;
        file.compiled.tsx = result.tsx ?? "";

        if (wasmStripTypes) {
          const stripStart = performance.now();
          const jsResult = wasmStripTypes(result.code);
          timing.stripTypes = performance.now() - stripStart;
          file.compiled.js = jsResult.code;
          file.compiled.errors = jsResult.errors ?? [];
        } else {
          file.compiled.js = result.code;
          file.compiled.errors = [];
        }
      } else {
        // Default: compile with keepTs: false → JS directly
        file.compiled.ts = "";
        const verterStart = performance.now();
        const result = compileInner(
          sfc,
          file.filename.replace(".ts", ".vue"),
          undefined,
          false,
          showTSX,
        );
        timing.verter = performance.now() - verterStart;
        timing.tsx = (result as any).tsxDurationMs ?? null;
        file.compiled.tsx = result.tsx ?? "";
        file.compiled.js = result.code;
        file.compiled.errors = [];
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

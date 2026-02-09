import type { CodegenResult } from "@verter/wasm";
import type { File, CompilerOptions, CompileTiming } from "./types";
import { loadLocalWasm, loadCommitWasm, loadReleaseWasm, type WasmModule } from "./wasmLoader";
import type { VersionEntry } from "./versions";

interface TransformResult {
  code: string;
  errors?: Array<{ message: string }>;
}

interface TransformOptions {
  lang?: "ts" | "js" | "tsx" | "jsx";
  sourceType?: "module" | "script";
}

let wasmCompile: ((input: string, options?: unknown) => CodegenResult) | null = null;
let oxcTransform:
  | ((filename: string, code: string, options?: TransformOptions) => Promise<TransformResult>)
  | null = null;
let initialized = false;
let initPromise: Promise<void> | null = null;

export async function initCompilers(): Promise<void> {
  if (initialized) return;
  if (initPromise) return initPromise;

  initPromise = (async () => {
    // Initialize @verter/wasm (local build)
    const wasmModule = await loadLocalWasm();
    wasmCompile = wasmModule.compile as typeof wasmCompile;

    // Initialize oxc-transform (dynamic import to handle top-level await)
    const oxc = await import("oxc-transform");
    oxcTransform = oxc.transform as typeof oxcTransform;

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

export async function compileVueSFC(
  source: string,
  filename: string,
  options?: CompilerOptions,
): Promise<CodegenResult> {
  await initCompilers();
  if (!wasmCompile) {
    throw new Error("WASM compiler not initialized");
  }
  const result = wasmCompile(source, {
    filename,
    includeSourceContent: true,
    isProduction: options?.isProduction ?? false,
    ssr: options?.ssr ?? false,
  });
  result.code = mergeRenderIntoComponent(result.code);
  return result;
}

export async function transpileTS(
  code: string,
  filename: string,
): Promise<{ code: string; errors: string[] }> {
  if (!oxcTransform) {
    return { code: "", errors: ["oxc-transform not initialized"] };
  }

  try {
    const result = await oxcTransform(filename, code, {
      lang: "tsx",
      sourceType: "module",
    });
    return {
      code: result.code,
      errors: result.errors?.map((e) => e.message) ?? [],
    };
  } catch (e) {
    return {
      code: "",
      errors: [e instanceof Error ? e.message : String(e)],
    };
  }
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

export async function compileFile(file: File, options?: CompilerOptions): Promise<CompileTiming> {
  const timing: CompileTiming = { verter: null, verterNative: null, oxc: null };

  if (file.filename.endsWith(".vue")) {
    try {
      const verterStart = performance.now();
      const verterResult = await compileVueSFC(file.code, file.filename, options);
      timing.verter = performance.now() - verterStart;
      timing.verterNative = (verterResult as any).durationMs ?? null;
      console.log(
        `Compiled ${file.filename} in ${timing.verter}ms (WASM: ${verterResult.durationMs ?? "N/A"}ms)`,
      );

      // Store source map from Verter
      file.compiled.sourceMap = verterResult.sourceMap ?? "";

      // Extract CSS from source for the preview
      file.compiled.css = extractStyles(file.code);

      if (file.isTS) {
        // TypeScript: TS tab gets Verter output (with types), JS tab gets OXC output (types stripped)
        file.compiled.ts = verterResult.code;

        const oxcStart = performance.now();
        const jsResult = await transpileTS(verterResult.code, file.filename.replace(".vue", ".ts"));
        timing.oxc = performance.now() - oxcStart;
        file.compiled.js = jsResult.code;
        file.compiled.errors = jsResult.errors;
      } else {
        // JavaScript: no TS tab needed, JS tab gets Verter output directly
        file.compiled.ts = "";
        file.compiled.js = verterResult.code;
        file.compiled.errors = [];
      }
    } catch (e) {
      file.compiled.errors = [e instanceof Error ? e.message : String(e)];
    }
  } else if (file.filename.endsWith(".ts")) {
    file.compiled.ts = file.code;
    const oxcStart = performance.now();
    const jsResult = await transpileTS(file.code, file.filename);
    timing.oxc = performance.now() - oxcStart;
    file.compiled.js = jsResult.code;
    file.compiled.errors = jsResult.errors;
  } else if (file.filename.endsWith(".js")) {
    file.compiled.js = file.code;
    file.compiled.ts = "";
    file.compiled.errors = [];
  } else if (file.filename.endsWith(".css")) {
    file.compiled.css = file.code;
    file.compiled.errors = [];
  }

  return timing;
}

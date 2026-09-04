import { VerterHost } from "@verter/native";
import type { HostCompileRequest, HostRuntimeCompiledProduct } from "@verter/native";

export interface VerterCompileResult {
  code: string;
  errors: string[];
}

/**
 * Create a VerterHost instance for benchmarking.
 * @param analysisLevel - "full" (default), "essential", or "none"
 * @param hostCpuThreads - Optional worker count for the host-owned CPU
 *   pool used by `compileMany`. `undefined` resolves to the platform's
 *   available parallelism at host-construction time.
 */
export function createVerterHost(
  analysisLevel: "full" | "essential" | "none" = "full",
  hostCpuThreads?: number,
): VerterHost {
  return new VerterHost({
    devMode: false,
    analysisLevel,
    hostCpuThreads,
  } as any);
}

function vueRuntimeClientRequest(filename: string): HostCompileRequest {
  return {
    framework: "vue",
    identity: { filename, isProduction: false, forceJs: false },
    products: [{ kind: "runtimeClient", runtimeSourceMap: false }],
    options: {
      backend: "inferred",
      ssr: false,
      isCustomElement: [],
      babelParserPlugins: [],
    },
  };
}

function runtimeNode(product: HostRuntimeCompiledProduct | undefined, kind: "script" | "template") {
  return product?.nodes.find((node) => node.node.kind === kind);
}

/**
 * Compile a Vue SFC using VerterHost (new AST-based pipeline, stateful).
 * Forces recompilation by calling remove() before upsert() to defeat caching.
 * Uses camelCase field names as required by the NAPI runtime.
 */
export function compileVerterHost(
  host: VerterHost,
  source: string,
  filename: string = "anonymous.vue",
): VerterCompileResult {
  try {
    // Remove any cached version to force recompilation
    host.remove(filename);

    const result = host.upsert({
      inputId: filename,
      source,
    });

    const compiled = host.compileRequest(
      result.canonicalId,
      vueRuntimeClientRequest(result.canonicalId),
    );
    const runtime = compiled.products.find(
      (product): product is HostRuntimeCompiledProduct => product.kind === "runtimeClient",
    );

    let code = "";
    const scriptFile = runtimeNode(runtime, "script");
    if (scriptFile) code += scriptFile.code;

    const templateFile = runtimeNode(runtime, "template");
    if (templateFile) code += "\n\n" + templateFile.code;

    return {
      code,
      errors: compiled.diagnostics.diagnostics
        .filter((d) => d.severity === "error")
        .map((d) => d.message),
    };
  } catch (error) {
    return {
      code: "",
      errors: [error instanceof Error ? error.message : String(error)],
    };
  }
}

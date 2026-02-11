import type { Plugin, ResolvedConfig } from "vite";
import { preprocessCSS, transformWithEsbuild } from "vite";
import type {
  ViteCodegenOptions,
  ViteCodegenResult,
  ProcessStyleOptions,
  ProcessStyleResult,
} from "@verter/native";
import { createHash } from "crypto";
import { createRequire } from "module";
import { generateMainModule } from "./main";
import { parseVueRequest, setDescriptor, getDescriptor, deleteDescriptor } from "./utils";

const EXPORT_HELPER_ID = "\0plugin-vue:export-helper";

/**
 * The export helper function matches @vitejs/plugin-vue's _export_sfc.
 * It applies metadata (like __scopeId) to Vue components, handling both
 * script setup (direct assignment) and Options API (__vccOpts fallback).
 */
const EXPORT_HELPER_CODE = `
export default (sfc, props) => {
  const target = sfc.__vccOpts || sfc;
  for (const [key, val] of props) {
    target[key] = val;
  }
  return target;
}
`;

const require = createRequire(import.meta.url);

export interface VerterPluginOptions {
  /** Custom component ID generator */
  componentId?: (filename: string, source: string, isProd: boolean) => string;
}

interface Compiler {
  compileForVite(input: string, options?: ViteCodegenOptions): ViteCodegenResult;
  processStyle(css: string, options: ProcessStyleOptions): ProcessStyleResult;
}

/**
 * Generate a short hash for component ID
 */
function getHash(text: string): string {
  return createHash("sha256").update(text).digest("hex").substring(0, 8);
}

/**
 * Generate component ID from filename and source
 */
function generateComponentId(filename: string, source: string, isProd: boolean): string {
  const normalized = filename.replace(/\\/g, "/");
  return isProd ? getHash(normalized) : getHash(normalized + source);
}

/**
 * Vite plugin for compiling Vue SFCs with Verter.
 *
 * @example
 * ```ts
 * // vite.config.ts
 * import { defineConfig } from 'vite';
 * import verter from '@verter/vite-plugin';
 *
 * export default defineConfig({
 *   plugins: [verter()],
 * });
 * ```
 */
export function verter(options: VerterPluginOptions = {}): Plugin {
  let config: ResolvedConfig;
  let compiler: Compiler | null = null;

  const loadCompiler = (): Compiler => {
    if (compiler) return compiler;

    // Load native bindings (Node.js only for now)
    const native = require("@verter/native") as typeof import("@verter/native");
    compiler = {
      compileForVite: (input, opts) => native.compileForVite(input, opts),
      processStyle: (css, opts) => native.processStyle(css, opts),
    };
    return compiler;
  };

  return {
    name: "vite-plugin-verter",

    configResolved(resolvedConfig) {
      config = resolvedConfig;
    },

    // Resolve virtual module IDs for style blocks and export helper
    resolveId(id) {
      if (id === EXPORT_HELPER_ID) {
        return id;
      }
      const { query } = parseVueRequest(id);
      if (query.vue) {
        return id;
      }
    },

    // Load virtual modules (style blocks, export helper)
    load(id) {
      if (id === EXPORT_HELPER_ID) {
        return EXPORT_HELPER_CODE;
      }

      const { filename, query } = parseVueRequest(id);
      if (!query.vue) return;

      const descriptor = getDescriptor(filename);
      if (!descriptor) return;

      if (query.type === "style" && query.index != null) {
        const style = descriptor.styles[query.index];
        if (!style) return;
        return {
          code: style.code,
          map: undefined,
        };
      }
    },

    async transform(code, id) {
      const { filename, query } = parseVueRequest(id);

      // Skip virtual module requests (style blocks)
      if (query.vue) return;

      // Only process .vue files
      if (!filename.endsWith(".vue")) return null;

      const compiler = loadCompiler();
      const isProd = config.command === "build" && !config.build?.ssr;
      const ssr = Boolean(config.build?.ssr);

      const componentIdFn = options.componentId || generateComponentId;
      const componentId = componentIdFn(filename, code, isProd);

      const compileOptions: ViteCodegenOptions = {
        filename,
        ssr,
        isProduction: isProd,
        componentId,
        sourcemap: true,
      };

      try {
        const result = compiler.compileForVite(code, compileOptions);

        // Process preprocessor styles (SCSS/Less/Stylus) that need Vite's preprocessCSS
        // Plain CSS is already processed inline by Rust's lightningcss pipeline.
        const processedStyles = await Promise.all(
          result.styles.map(async (style) => {
            const hasPreprocessor = style.lang && style.lang !== "css";

            if (hasPreprocessor) {
              // 1. Preprocess SCSS/Less/Stylus → CSS via Vite's API
              const preprocessed = await preprocessCSS(
                style.code,
                `${filename}.${style.lang}`,
                config,
              );

              // 2. Apply scoping/modules via NAPI if needed
              if (style.scoped || style.is_module) {
                const processed = compiler.processStyle(preprocessed.code, {
                  scope_id: componentId,
                  scoped: style.scoped,
                  is_module: style.is_module,
                  module_name: style.module_name ?? undefined,
                });
                return {
                  ...style,
                  code: processed.code,
                  lang: "css",
                  module_classes: processed.module_classes,
                };
              }

              // Preprocessor but no scoping/modules — just preprocessed
              return { ...style, code: preprocessed.code, lang: "css" };
            }

            // Plain CSS — already processed by Rust inline
            return style;
          }),
        );

        // Cache with processed styles for virtual module loading
        setDescriptor(filename, { ...result, styles: processedStyles });

        // Assemble the main module from split blocks
        const output = generateMainModule({ ...result, styles: processedStyles }, {
          filename,
          scopeId: componentId,
          ssr,
          isProd,
        });

        // Transform TypeScript imports/syntax through esbuild
        const stripped = await transformWithEsbuild(output, filename + ".ts", {
          loader: "ts",
          sourcemap: true,
          sourcefile: filename,
        });

        return {
          code: stripped.code,
          map: stripped.map ?? null,
        };
      } catch (error) {
        this.error(`Verter compilation error in ${filename}: ${error}`);
      }
    },

    handleHotUpdate({ file, server, modules }) {
      if (!file.endsWith(".vue")) return;

      // Clear cached descriptor
      deleteDescriptor(file);

      // Invalidate the module and trigger HMR
      const affectedModules = modules.filter((m) => m.file === file);
      if (affectedModules.length > 0) {
        server.ws.send({
          type: "full-reload",
          path: file,
        });
      }
    },
  };
}

export default verter;

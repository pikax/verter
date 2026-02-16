import type { UnpluginFactory } from "unplugin";
import { createUnplugin } from "unplugin";
import type { ResolvedConfig } from "vite";
import type { ViteCodegenOptions } from "@verter/native";
import type { VerterPluginOptions, HmrStrategy } from "./core/types";
import { EXPORT_HELPER_ID, EXPORT_HELPER_CODE } from "./core/constants";
import { loadCompiler, generateComponentId } from "./core/compiler";
import { parseVueRequest, getDescriptor, setDescriptor, deleteDescriptor } from "./core/utils";
import { generateMainModule } from "./core/main";

export type { VerterPluginOptions, HmrStrategy } from "./core/types";

function getHmrStrategy(framework: string): HmrStrategy {
  switch (framework) {
    case "vite":
    case "rolldown":
      return "vite";
    case "webpack":
    case "rspack":
      return "webpack";
    default:
      return "none";
  }
}

export const unpluginFactory: UnpluginFactory<VerterPluginOptions | undefined> = (
  options,
  meta,
) => {
  const opts = options ?? {};
  let viteConfig: ResolvedConfig | null = null;
  const hmrStrategy = getHmrStrategy(meta.framework);

  return {
    name: "unplugin-verter",

    resolveId(id) {
      if (id === EXPORT_HELPER_ID) {
        return id;
      }
      const { query } = parseVueRequest(id);
      if (query.vue) {
        return id;
      }
    },

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

      // Custom blocks (e.g., <i18n>, <docs>)
      if (
        query.type &&
        query.type !== "script" &&
        query.type !== "template" &&
        query.type !== "style" &&
        query.index != null
      ) {
        const block = (descriptor as any).customBlocks?.[query.index];
        if (!block) return;
        return { code: block.content };
      }
    },

    transformInclude(id) {
      const { filename, query } = parseVueRequest(id);
      return filename.endsWith(".vue") && !query.vue;
    },

    async transform(code, id) {
      const { filename } = parseVueRequest(id);

      const compiler = loadCompiler();
      const isProd = viteConfig
        ? viteConfig.command === "build" && !viteConfig.build?.ssr
        : process.env.NODE_ENV === "production";
      const ssr = viteConfig ? Boolean(viteConfig.build?.ssr) : false;

      const componentIdFn = opts.componentId || generateComponentId;
      const componentId = componentIdFn(filename, code, isProd);

      // napi-rs converts snake_case to camelCase at runtime
      const compileOptions = {
        filename,
        ssr,
        isProduction: isProd,
        componentId,
        sourcemap: true,
      };

      const result = compiler.compileForVite(code, compileOptions as any);

      // Process preprocessor styles via Vite's CSS pipeline when available
      let processedStyles = result.styles;
      if (viteConfig) {
        const { preprocessCSS } = await import("vite");
        processedStyles = await Promise.all(
          result.styles.map(async (style) => {
            const hasPreprocessor = style.lang && style.lang !== "css";
            const isModule = (style as any).isModule ?? (style as any).is_module ?? false;
            const moduleName = (style as any).moduleName ?? (style as any).module_name;

            if (hasPreprocessor) {
              const preprocessed = await preprocessCSS(
                style.code,
                `${filename}.${style.lang}`,
                viteConfig!,
              );

              if (style.scoped || isModule) {
                const processed = compiler.processStyle(preprocessed.code, {
                  scopeId: componentId,
                  scoped: style.scoped,
                  isModule,
                  moduleName: moduleName ?? undefined,
                } as any);
                return {
                  ...style,
                  code: processed.code,
                  lang: "css",
                  moduleClasses: (processed as any).moduleClasses ?? (processed as any).module_classes ?? [],
                };
              }

              return { ...style, code: preprocessed.code, lang: "css" };
            }

            return style;
          }),
        );
      }

      setDescriptor(filename, { ...result, styles: processedStyles });

      const output = generateMainModule({ ...result, styles: processedStyles }, {
        filename,
        scopeId: componentId,
        ssr,
        isProd,
        hmr: isProd ? "none" : hmrStrategy,
      });

      // Strip TypeScript via Vite's esbuild transform when available
      if (viteConfig) {
        const { transformWithEsbuild } = await import("vite");
        const stripped = await transformWithEsbuild(output, filename + ".ts", {
          loader: "ts",
          sourcemap: true,
          sourcefile: filename,
        });

        return {
          code: stripped.code,
          map: stripped.map ?? null,
        };
      }

      // Fallback: strip TypeScript using native bindings for non-Vite bundlers
      const stripped = compiler.stripTypes(output);
      return { code: stripped.code, map: null };
    },

    watchChange(id) {
      if (id.endsWith(".vue")) {
        deleteDescriptor(id);
      }
    },

    // Vite-specific hooks
    vite: {
      configResolved(resolvedConfig) {
        viteConfig = resolvedConfig;
      },

      handleHotUpdate({ file, server, modules }) {
        if (!file.endsWith(".vue")) return;

        deleteDescriptor(file);

        const affectedModules = modules.filter((m) => m.file === file);
        if (affectedModules.length > 0) {
          server.ws.send({
            type: "full-reload",
            path: file,
          });
        }
      },
    },
  };
};

const unplugin = createUnplugin(unpluginFactory);

export default unplugin;

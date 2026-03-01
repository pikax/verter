import type { UnpluginFactory } from "unplugin";
import { createUnplugin } from "unplugin";
import type { ResolvedConfig } from "vite";
import type { VerterPluginOptions, HmrStrategy, BlockPreprocessor } from "./core/types";
import { EXPORT_HELPER_ID, EXPORT_HELPER_CODE } from "./core/constants";
import type { HostCompileProfile, HostUpdateResult, NativeBlockOverrideEntry } from "@verter/native";
import type { VerterHost } from "@verter/native";
import { loadHost, generateComponentId, processStyle } from "./core/compiler";
import { parseVueRequest } from "./core/utils";
import { preprocessBlock } from "./core/preprocessor";

export type { VerterPluginOptions, HmrStrategy, Options } from "./core/types";

/**
 * Resolves external sources and type-dependency imports from an upsert result.
 * Shared between `transform()` and `buildStart()` (preCompile).
 */
async function resolveUpsertDependencies(
  host: VerterHost,
  filename: string,
  upsertResult: HostUpdateResult,
): Promise<void> {
  // Resolve external sources (e.g., <style src="./foo.less">, <template src="./t.html">)
  if (upsertResult.externalSourceRequests.length > 0) {
    const fs = await import("fs");
    const path = await import("path");
    for (const req of upsertResult.externalSourceRequests) {
      const resolvedId: string = req.resolvedCanonicalId;
      const specifier: string = req.specifier;
      // Resolve relative to the owner file's directory
      const absPath = path.resolve(path.dirname(filename), specifier);
      try {
        const extSource = fs.readFileSync(absPath);
        host.upsert({
          inputId: resolvedId,
          source: extSource,
          fileKind: "non_sfc",
        });
      } catch {
        // External source not found — host will report the error
      }
    }
  }

  // Upsert type-dependency .ts files so compile_entry() can resolve external types
  // (e.g., `import type { Props } from './types'` in a .vue script setup).
  if (upsertResult.importSpecifiers.length > 0) {
    const fs = await import("fs");
    const path = await import("path");
    const exts = ["", ".ts", ".tsx", ".js", ".jsx", ".mts", ".mjs"];
    for (const imp of upsertResult.importSpecifiers) {
      if (!imp.source.startsWith(".")) continue; // skip bare specifiers (node_modules)

      const absBase = path.resolve(path.dirname(filename), imp.source);
      for (const ext of exts) {
        const fullPath = absBase + ext;
        // Skip .vue files — they'll be properly upserted as VueSfc when
        // Vite's module graph processes them via transform(). Upserting
        // them as non_sfc here would clobber their SFC-specific metadata
        // (script_lang, style_langs, etc.).
        if (fullPath.endsWith(".vue")) continue;
        try {
          const depSource = fs.readFileSync(fullPath);
          host.upsert({
            inputId: fullPath,
            source: depSource,
            fileKind: "non_sfc",
          });
          break;
        } catch {
          continue;
        }
      }
    }
  }
}

/**
 * Processes preprocessor requests from an upsert result.
 * For each non-native block (e.g., Pug template, SCSS style), invokes the
 * appropriate preprocessor and sends the results back to the host via
 * `applyBlockOverrides`.
 */
async function applyPreprocessorRequests(
  host: VerterHost,
  filename: string,
  upsertResult: HostUpdateResult,
  profile: HostCompileProfile | undefined,
  viteConfig: unknown | null,
  customBlocks?: Record<string, BlockPreprocessor>,
): Promise<void> {
  if (!upsertResult.preprocessorRequests?.length) return;

  const overrides: NativeBlockOverrideEntry[] = [];
  for (const req of upsertResult.preprocessorRequests) {
    const result = await preprocessBlock(req, filename, viteConfig, customBlocks);
    if (result) {
      overrides.push({
        blockType: req.blockType,
        index: req.index,
        code: result.code,
        sourceMap: result.sourceMap,
      });
    }
  }
  if (overrides.length > 0) {
    host.applyBlockOverrides({
      canonicalId: filename,
      compileProfile: profile,
      overrides,
    });
  }
}

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

function createFilter(include?: string | RegExp | (string | RegExp)[]): (filename: string) => boolean {
  if (!include) {
    return (f) => f.endsWith(".vue");
  }
  const patterns = Array.isArray(include) ? include : [include];
  return (f) => patterns.some((p) => (typeof p === "string" ? f.endsWith(p) : p.test(f)));
}

export const unpluginFactory: UnpluginFactory<VerterPluginOptions | undefined> = (
  options,
  meta,
) => {
  const opts = options ?? {};
  let viteConfig: ResolvedConfig | null = null;
  const hmrStrategy = getHmrStrategy(meta.framework);
  const filter = createFilter(opts.include);

  // Store compile profiles from transform() so load() can reuse the same profile.
  // This ensures virtual file requests (style, template) use the same componentId
  // and other profile fields as the initial compilation.
  const profileCache = new Map<string, HostCompileProfile>();

  // Cache compiled scripts for script sub-requests.
  // In Vite mode, the main .vue transform returns a thin module that imports from
  // a script sub-request (?vue&type=script&lang.ts). This lets vite:esbuild and
  // @vitejs/plugin-vue-jsx handle TS stripping and JSX transformation natively,
  // matching @vitejs/plugin-vue's behavior.
  const scriptCache = new Map<string, { code: string; map: any }>();

  // Build timing instrumentation — accumulates per-phase timings across all .vue transforms.
  // Enabled when VERTER_TIMING=1 env var is set.
  const timing = process.env.VERTER_TIMING === "1";
  let tFileCount = 0;
  let tUpsertMs = 0;
  let tDepsMs = 0;
  let tCompileMs = 0;
  let tLoadFileCount = 0;
  let tLoadMs = 0;

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

      // Script sub-requests: return cached compiled output from transform()
      if (query.type === "script") {
        const cached = scriptCache.get(filename);
        if (cached) {
          return {
            code: cached.code,
            map: cached.map ?? undefined,
          };
        }
      }

      const host = loadHost();

      // Reuse the compile profile from transform() to ensure the same componentId
      // and other fields are used. Fall back to a basic profile if not cached.
      const cachedProfile = profileCache.get(filename);
      const compileProfile: HostCompileProfile = cachedProfile ?? (() => {
        const isProd = viteConfig
          ? viteConfig.command === "build" && !viteConfig.build?.ssr
          : process.env.NODE_ENV === "production";
        const ssr = viteConfig ? Boolean(viteConfig.build?.ssr) : false;
        return {
          isProduction: isProd,
          ssr,
          hmrStrategy: (isProd ? "none" : hmrStrategy) as HostCompileProfile["hmrStrategy"],
        };
      })();

      try {
        const lt0 = timing ? performance.now() : 0;
        const file = host.getVirtualFile({
          rawId: id,
          compileProfile,
        });
        if (timing) {
          tLoadMs += performance.now() - lt0;
          tLoadFileCount++;
        }

        return {
          code: file.code,
          map: file.sourceMap ?? undefined,
        };
      } catch {
        // File not yet in host (shouldn't happen in normal flow)
        return undefined;
      }
    },

    transformInclude(id) {
      const { filename, query } = parseVueRequest(id);
      // Main .vue files for compilation
      if (filter(filename) && !query.vue) return true;
      // Style virtual files that need CSS preprocessing (LESS/SCSS/Stylus → CSS)
      if (query.vue && query.type === "style" && query.lang && query.lang !== "css" && filter(filename)) return true;
      return false;
    },

    async buildStart() {
      if (!opts.preCompile) return;

      const { scanVueFiles } = await import("./core/scanner");
      const root = viteConfig?.root ?? process.cwd();
      const files = await scanVueFiles(root, filter);

      if (files.size === 0) return;

      const host = loadHost();
      const isProd = viteConfig
        ? viteConfig.command === "build" && !viteConfig.build?.ssr
        : process.env.NODE_ENV === "production";
      const ssr = viteConfig ? Boolean(viteConfig.build?.ssr) : false;
      const componentIdFn = opts.componentId || generateComponentId;

      for (const [filename, source] of files) {
        const componentId = componentIdFn(filename, source, isProd, viteConfig?.root);

        const profile: HostCompileProfile = {
          filename,
          ssr,
          isProduction: isProd,
          componentId,
          hmrStrategy: (isProd ? "none" : hmrStrategy) as HostCompileProfile["hmrStrategy"],
          sourceMap: true,
          forceJs: !viteConfig || meta.framework !== "vite",
        };

        profileCache.set(filename, profile);

        const upsertResult = host.upsert({
          inputId: filename,
          source,
        });

        await resolveUpsertDependencies(host, filename, upsertResult);

        // Preprocess non-native blocks (Pug, CoffeeScript, SCSS, custom)
        await applyPreprocessorRequests(
          host, filename, upsertResult, profile, viteConfig, opts.customBlocks,
        );

        const main = host.getVirtualFile({
          rawId: filename,
          compileProfile: profile,
        });

        if (viteConfig) {
          scriptCache.set(filename, {
            code: main.code,
            map: main.sourceMap ?? null,
          });
        }
      }

      // Cross-file optimization: analyze render tree for prop constness
      if (opts.crossFileOptimize && isProd) {
        const resultJson = host.computeCrossFileOptimizations();
        const result = JSON.parse(resultJson);
        // Recompile files whose constness hints changed
        for (const file of result.changedFiles) {
          const cachedProfile = profileCache.get(file);
          if (cachedProfile) {
            const recompiled = host.getVirtualFile({
              rawId: file,
              compileProfile: cachedProfile,
            });
            if (viteConfig) {
              scriptCache.set(file, {
                code: recompiled.code,
                map: recompiled.sourceMap ?? null,
              });
            }
          }
        }
        // Emit diagnostics as warnings
        for (const diag of result.diagnostics) {
          console.warn(`[verter] ${diag.code}: ${diag.message} (${diag.fileId})`);
        }
      }
    },

    async transform(code, id) {
      const { filename, query } = parseVueRequest(id);

      // Handle style virtual files: preprocess LESS/SCSS/Stylus → CSS, then scope
      if (query.vue && query.type === "style") {
        const lang = query.lang;
        if (lang && lang !== "css" && viteConfig) {
          try {
            const { preprocessCSS } = await import("vite");
            const preprocessed = await preprocessCSS(
              code,
              `${filename}.${lang}`,
              viteConfig!,
            );
            let css = preprocessed.code;

            // Apply scoped CSS transformation
            const profile = profileCache.get(filename);
            if (profile) {
              const processed = processStyle(css, {
                scopeId: profile.componentId ?? "",
                scoped: true, // TODO: detect from block attributes
              });
              css = processed.code;
            }

            return { code: css, map: null };
          } catch {
            // Preprocessor unavailable — return raw content
          }
        }
        return;
      }

      const host = loadHost();
      const isProd = viteConfig
        ? viteConfig.command === "build" && !viteConfig.build?.ssr
        : process.env.NODE_ENV === "production";
      const ssr = viteConfig ? Boolean(viteConfig.build?.ssr) : false;

      const componentIdFn = opts.componentId || generateComponentId;
      const componentId = componentIdFn(filename, code, isProd, viteConfig?.root);

      const profile: HostCompileProfile = {
        filename,
        ssr,
        isProduction: isProd,
        componentId,
        hmrStrategy: (isProd ? "none" : hmrStrategy) as HostCompileProfile["hmrStrategy"],
        sourceMap: true,
        // Only Vite itself strips TS via vite:esbuild on script sub-requests.
        // Rolldown/tsdown sets viteConfig (via Vite's API) but lacks vite:esbuild,
        // so Verter must strip TS. Other bundlers (webpack, rspack) also need stripping.
        forceJs: !viteConfig || meta.framework !== "vite",
      };

      // Cache the profile so load() can reuse it for virtual file requests
      profileCache.set(filename, profile);

      // Register file in host (handles parsing, caching, change detection)
      const t0 = timing ? performance.now() : 0;
      const upsertResult = host.upsert({
        inputId: filename,
        source: code,
      });
      const t1 = timing ? performance.now() : 0;

      await resolveUpsertDependencies(host, filename, upsertResult);

      // Preprocess non-native blocks (Pug, CoffeeScript, SCSS, custom)
      await applyPreprocessorRequests(
        host, filename, upsertResult, profile, viteConfig, opts.customBlocks,
      );
      const t2 = timing ? performance.now() : 0;

      // Get the main module from the host (assembled in Rust)
      const main = host.getVirtualFile({
        rawId: filename,
        compileProfile: profile,
      });
      const t3 = timing ? performance.now() : 0;

      if (timing) {
        tUpsertMs += t1 - t0;
        tDepsMs += t2 - t1;
        tCompileMs += t3 - t2;
        tFileCount++;
      }

      // Determine the effective language of the compiled output.
      const mainLang: string = main.lang ?? "ts";

      if (viteConfig) {
        // In Vite mode, emit the compiled output as a script sub-request.
        // This matches @vitejs/plugin-vue's architecture where the main module
        // is a thin wrapper that imports from sub-requests:
        //   - Script: ?vue&type=script&lang.ts  → processed by vite:esbuild (TS)
        //   - Script: ?vue&type=script&lang.jsx → processed by @vitejs/plugin-vue-jsx
        //   - Style:  ?vue&type=style&lang.less → processed by Vite's CSS pipeline
        //
        // This ensures downstream plugins (vue-jsx, external-globals, etc.) receive
        // properly processed JavaScript, not raw TS/JSX.
        scriptCache.set(filename, {
          code: main.code,
          map: main.sourceMap ?? null,
        });

        const scriptRequest = `${filename}?vue&type=script&lang.${mainLang}`;
        const mainModule = [
          `import _sfc_main from "${scriptRequest}"`,
          `export * from "${scriptRequest}"`,
          `export default _sfc_main`,
        ].join("\n");

        return {
          code: mainModule,
          map: null,
          meta: { vite: { lang: mainLang } },
        } as any;
      }

      // Non-Vite mode: inline everything (no sub-request support).
      // TS stripping is handled by the host via forceJs: true in the profile.
      return { code: main.code, map: null };
    },

    buildEnd() {
      if (!timing) return;
      const transformTotal = tUpsertMs + tDepsMs + tCompileMs;
      const lines = [
        `[verter] Build timing (${tFileCount} .vue files transformed):`,
        `  transform: upsert=${tUpsertMs.toFixed(0)}ms  deps=${tDepsMs.toFixed(0)}ms  compile=${tCompileMs.toFixed(0)}ms  total=${transformTotal.toFixed(0)}ms`,
        `  load: ${tLoadFileCount} virtual files, ${tLoadMs.toFixed(0)}ms`,
      ];
      const host = loadHost();
      const metrics = (host as any).getMetrics?.();
      if (metrics) {
        lines.push(
          `  host metrics: upserts=${metrics.upserts}  compileRequests=${metrics.compileRequests}  cacheHits=${metrics.compileCacheHits}  hitRate=${(metrics.compileCacheHitRate * 100).toFixed(1)}%`,
          `    rustParseTotal=${(metrics.sliceHashTimeUsTotal / 1000).toFixed(0)}ms  rustCompileTotal=${(metrics.compileTimeUsTotal / 1000).toFixed(0)}ms`,
        );
      }
      console.log(lines.join("\n"));
    },

    watchChange(id) {
      if (filter(id)) {
        const host = loadHost();
        host.remove(id);
        profileCache.delete(id);
        scriptCache.delete(id);
      }
    },

    // Vite-specific hooks
    vite: {
      configResolved(resolvedConfig) {
        viteConfig = resolvedConfig;
      },

      handleHotUpdate({ file, server, modules }) {
        if (!file.endsWith(".vue")) return;

        const host = loadHost();
        host.remove(file);
        profileCache.delete(file);
        scriptCache.delete(file);

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

import type { UnpluginFactory } from "unplugin";
import { createUnplugin } from "unplugin";
import { existsSync, statSync } from "node:fs";
import type { ResolvedConfig } from "vite";
import type {
  VerterPluginOptions,
  VerterFrameworkLanguage,
  HmrStrategy,
  BlockPreprocessor,
} from "./core/types";
import { EXPORT_HELPER_ID, EXPORT_HELPER_CODE } from "./core/constants";
import type {
  HostCompileProfile,
  HostUpdateResult,
  NativeBlockOverrideEntry,
  HostDependencyResolution,
} from "@verter/native";
import type { VerterHost } from "@verter/native";
import {
  loadHost,
  getWorkspace,
  generateComponentId,
  processStyle,
  resetHost,
} from "./core/compiler";
import { collectResolvableModuleReferenceSpecifiers } from "./core/dependency-resolution";
import { hydrateMacroTypeDeps } from "./core/macro-type-hydration";
import { parseVueRequest } from "./core/utils";
import { preprocessBlock } from "./core/preprocessor";
import { replaceImportMetaSsr, stripComponents } from "./core/ssr-transforms";

export type {
  VerterPluginOptions,
  VerterFrameworkLanguage,
  HmrStrategy,
  Options,
} from "./core/types";

/** The compiled runtime Main module the bundler ships for a carrier file. */
interface RenderedMain {
  code: string;
  sourceMap?: string;
  lang?: string;
}

interface CompiledStyleArtifact {
  code: string;
  map?: string;
  lang: string;
}

function readCompiledStyleArtifacts(
  host: VerterHost,
  filename: string,
  profile: HostCompileProfile,
): CompiledStyleArtifact[] {
  const artifacts: CompiledStyleArtifact[] = [];
  const indices = host
    .listVirtualFiles(filename)
    .filter((node) => node.kind === "style" && node.index != null)
    .map((node) => node.index as number)
    .sort((a, b) => a - b);
  for (const index of indices) {
    const style = host.getVirtualFile({
      rawId: `${filename}?verter&type=style&index=${index}`,
      compileProfile: profile,
    });
    artifacts[index] = {
      code: style.code,
      map: style.sourceMap ?? undefined,
      lang: style.lang ?? "css",
    };
  }
  return artifacts;
}

/**
 * Render a carrier file's runtime Main module through the render-only
 * bundler lane (`host.compileMany([input], { target: "runtime-render" })`) —
 * the single shared compile substrate. The batch render profile + per-input
 * `componentId` are taken from the SAME `HostCompileProfile` the bundler
 * would have passed to `getVirtualFile({ compileProfile })`, so the Main
 * output is byte-identical to the previous path.
 *
 * A fatal compile surfaces as a thrown error (mirroring `getVirtualFile`'s
 * throw). Non-fatal (warning-severity) diagnostics from the lane's
 * soft-macro contract are forwarded to the bundler via `warn` (preserving
 * code/message/source id); they never become build errors.
 */
function renderMainRuntime(
  host: VerterHost,
  canonicalId: string,
  source: string,
  profile: HostCompileProfile,
  warn?: (w: { message: string; id?: string }) => void,
): RenderedMain {
  const [entry] = host.compileMany(
    [
      {
        canonicalId,
        source,
        componentId: profile.componentId,
      },
    ],
    {
      target: "runtime-render",
      compileProfile: {
        // Required render-profile fields default to the same values the
        // `CompileProfile` default / FFI conversion applies for an absent
        // `HostCompileProfile` field, so a caller that omitted a field
        // renders EXACTLY as `getVirtualFile` would have. Optional fields
        // (`filename`, `comments`, ...) pass through UNTOUCHED — `comments`
        // is a tri-state (absent = compiler default `!isProduction`;
        // collapsing it to `false` would strip comments from dev builds)
        // and `filename` feeds component-name/scope-id/source-map codegen.
        // Every output-affecting field is forwarded so the render output
        // stays byte-identical (omitting one — e.g. sourceMap — would
        // silently drop it from the build).
        filename: profile.filename,
        isProduction: profile.isProduction ?? false,
        ssr: profile.ssr ?? false,
        forceJs: profile.forceJs ?? false,
        forceVapor: profile.forceVapor ?? false,
        sourceMap: profile.sourceMap ?? false,
        comments: profile.comments,
        // `HostCompileProfile.hmrStrategy` is already "none" | "vite" |
        // "webpack"; map it faithfully (default "none").
        hmrStrategy: profile.hmrStrategy ?? "none",
        runtimeModuleName: profile.runtimeModuleName,
        typesModuleName: profile.typesModuleName,
        delimiterOpen: profile.delimiters?.[0],
        delimiterClose: profile.delimiters?.[1],
        customElements: profile.customElements,
      },
    },
  );

  // Fatal errors abort exactly like `getVirtualFile` did (fail-closed).
  if (entry.errors.length > 0) {
    throw new Error(`[verter] ${canonicalId}: ${entry.errors.join("; ")}`);
  }

  // Forward soft (warning-severity) diagnostics as bundler warnings.
  if (warn) {
    for (const d of entry.diagnostics) {
      warn({ message: `[verter] ${d.code}: ${d.message}`, id: canonicalId });
    }
  } else {
    for (const d of entry.diagnostics) {
      console.warn(`[verter] ${d.code}: ${d.message} (${canonicalId})`);
    }
  }

  return {
    code: entry.code,
    sourceMap: entry.sourceMap ?? undefined,
    lang: entry.lang ?? undefined,
  };
}

/** Normalize Windows backslashes to forward slashes for the workspace API. */
function normalizePath(p: string): string {
  return p.replace(/\\/g, "/");
}

async function readTextFileThroughWorkspaceOrDisk(pathname: string): Promise<string | null> {
  const ws = getWorkspace();
  if (ws) {
    return ws.readFile(normalizePath(pathname));
  }

  const fs = await import("node:fs/promises");
  try {
    return await fs.readFile(pathname, "utf8");
  } catch {
    return null;
  }
}

async function fileExistsThroughWorkspaceOrDisk(pathname: string): Promise<boolean> {
  const ws = getWorkspace();
  if (ws) {
    return ws.fileExists(normalizePath(pathname));
  }

  const fs = await import("node:fs/promises");
  try {
    return (await fs.stat(pathname)).isFile();
  } catch {
    return false;
  }
}

type ResolveHook = (
  source: string,
  importer: string,
  options: { skipSelf: true },
) => Promise<unknown> | unknown;

function resolvedIdFromHookResult(result: unknown): string | null {
  if (!result) return null;
  if (typeof result === "string") {
    return result.startsWith("\0") || result.includes("?") ? null : result;
  }
  if (typeof result !== "object") return null;

  const resolved = result as { id?: unknown; external?: unknown };
  if (resolved.external) return null;
  if (typeof resolved.id !== "string") return null;
  if (resolved.id.startsWith("\0") || resolved.id.includes("?")) return null;
  return resolved.id;
}

/**
 * Resolves external sources and type-dependency imports from an upsert result.
 * Shared between `transform()` and `buildStart()` (preCompile).
 */
async function resolveUpsertDependencies(
  host: VerterHost,
  filename: string,
  upsertResult: HostUpdateResult,
  resolveId?: ResolveHook,
): Promise<void> {
  // Resolve external sources (e.g., <style src="./foo.less">, <template src="./t.html">)
  if (upsertResult.externalSourceRequests.length > 0) {
    const path = await import("path");
    for (const req of upsertResult.externalSourceRequests) {
      const resolvedId: string = req.resolvedCanonicalId;
      const specifier: string = req.specifier;
      // Resolve relative to the owner file's directory
      const absPath = path.resolve(path.dirname(filename), specifier);
      const extSource = await readTextFileThroughWorkspaceOrDisk(absPath);
      if (extSource !== null) {
        host.upsert({
          inputId: resolvedId,
          source: extSource,
          fileKind: "non_sfc",
        });
      }
    }
  }

  // Resolve exact and finite-set module references, then feed per-specifier
  // resolution records back to the host for exact cross-file resolution.
  const resolutions: HostDependencyResolution[] = [];
  const dependencySpecifiers = collectResolvableModuleReferenceSpecifiers(
    host,
    upsertResult.moduleReferences ?? [],
  );
  if (dependencySpecifiers.length > 0) {
    const path = await import("path");
    const exts = ["", ".ts", ".tsx", ".js", ".jsx", ".mts", ".mjs", ".d.ts", ".d.mts", ".d.cts"];
    for (const specifier of dependencySpecifiers) {
      // Try bundler resolve hook first (if available)
      if (resolveId) {
        const resolvedId = resolvedIdFromHookResult(
          await resolveId(specifier, filename, { skipSelf: true }),
        );
        if (resolvedId) {
          if (resolvedId.endsWith(".vue")) {
            resolutions.push({ specifier, resolvedCanonicalId: normalizePath(resolvedId) });
            continue;
          }
          const depSource = await readTextFileThroughWorkspaceOrDisk(resolvedId);
          if (depSource !== null) {
            const normalizedResolvedId = normalizePath(resolvedId);
            host.upsert({
              inputId: normalizedResolvedId,
              source: depSource,
              fileKind: "non_sfc",
            });
            resolutions.push({ specifier, resolvedCanonicalId: normalizedResolvedId });
            continue;
          }
          // readFile returned null — fall through for relative specifiers
        }
        // resolveId returned null or read failed — fall through for relative specifiers
      }

      // Filesystem probing fallback for relative specifiers
      if (!specifier.startsWith(".")) continue;

      const absBase = path.resolve(path.dirname(filename), specifier);
      for (const ext of exts) {
        const fullPath = absBase + ext;
        const normalizedFullPath = normalizePath(fullPath);
        if (fullPath.endsWith(".vue") && (await fileExistsThroughWorkspaceOrDisk(fullPath))) {
          resolutions.push({ specifier, resolvedCanonicalId: normalizedFullPath });
          break;
        }
        if (fullPath.endsWith(".vue")) continue;
        const depSource = await readTextFileThroughWorkspaceOrDisk(fullPath);
        if (depSource !== null) {
          host.upsert({
            inputId: normalizedFullPath,
            source: depSource,
            fileKind: "non_sfc",
          });
          resolutions.push({ specifier, resolvedCanonicalId: normalizedFullPath });
          break;
        }
      }
    }
  }
  // Also include external src="..." blocks that were resolved during upsert
  for (const req of upsertResult.externalSourceRequests ?? []) {
    resolutions.push({
      specifier: req.specifier,
      resolvedCanonicalId: req.resolvedCanonicalId,
    });
  }
  host.setImportDependencies(filename, resolutions);
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
    // In Vite mode, skip style preprocessing — Vite's CSS pipeline handles it.
    if (viteConfig && req.blockType === "style") continue;

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

type RuntimeFramework = Exclude<VerterFrameworkLanguage, "auto">;

function frameworkForFilename(
  filename: string,
  selection: VerterFrameworkLanguage,
): RuntimeFramework {
  if (selection !== "auto") return selection;
  if (filename.endsWith(".svelte")) return "sveltejs";
  // An explicit custom `include` historically compiled non-standard suffixes
  // as Vue. Preserve that compatibility while auto-detecting known carriers.
  return "vue";
}

function createFilter(
  include?: string | RegExp | (string | RegExp)[],
  selection: VerterFrameworkLanguage = "auto",
): (filename: string) => boolean {
  if (!include) {
    if (selection === "vue") return (f) => f.endsWith(".vue");
    if (selection === "sveltejs") return (f) => f.endsWith(".svelte");
    return (f) => f.endsWith(".vue") || f.endsWith(".svelte");
  }
  const patterns = Array.isArray(include) ? include : [include];
  return (f) => patterns.some((p) => (typeof p === "string" ? f.endsWith(p) : p.test(f)));
}

/** Detect if a project uses Nuxt by checking for nuxt.config.* or .nuxt/ directory. */
function detectNuxt(root: string): boolean {
  const ws = getWorkspace();
  const normalizedRoot = normalizePath(root);
  const configFiles = ["nuxt.config.ts", "nuxt.config.js", "nuxt.config.mts", "nuxt.config.mjs"];
  for (const f of configFiles) {
    const path = `${normalizedRoot}/${f}`;
    if (ws ? ws.fileExists(path) : existsSync(path)) return true;
  }
  const nuxtDir = `${normalizedRoot}/.nuxt`;
  if (ws ? ws.isDir(nuxtDir) : existsSync(nuxtDir) && statSync(nuxtDir).isDirectory()) return true;
  return false;
}

/** Check if a file is a Nuxt server component (*.server.vue). */
function isServerComponent(filename: string): boolean {
  return filename.endsWith(".server.vue");
}

/** Check if a file is a Nuxt client component (*.client.vue). */
function isClientComponent(filename: string): boolean {
  return filename.endsWith(".client.vue");
}

interface StyleBlockEntry {
  content: string;
  lang: string;
  scoped: boolean;
  module: boolean | string;
}

function createFrameworkFactory(
  pinnedFramework?: RuntimeFramework,
): UnpluginFactory<VerterPluginOptions | undefined> {
  return (options, meta) => {
    const opts = options ?? {};
    const frameworkSelection = pinnedFramework ?? opts.lang ?? "auto";
    let viteConfig: ResolvedConfig | null = null;
    let compiler: any = null;
    const hmrStrategy = getHmrStrategy(meta.framework);
    const filter = createFilter(opts.include, frameworkSelection);
    let isNuxt = false;
    let projectRoot = "";

    // Store compile profiles from transform() so load() can reuse the same profile.
    // This ensures virtual file requests (style, template) use the same componentId
    // and other profile fields as the initial compilation.
    const profileCache = new Map<string, HostCompileProfile>();

    // Cache per-style-block scoped flags from the SFC source.
    // Key: filename, Value: array of booleans (one per style block, in order).
    const styleScopedCache = new Map<string, boolean[]>();

    // Cache compiled scripts for script sub-requests.
    // In Vite mode, the main .vue transform returns a thin module that imports from
    // a script sub-request (?vue&type=script&lang.ts). This lets vite:esbuild and
    // @vitejs/plugin-vue-jsx handle TS stripping and JSX transformation natively,
    // matching @vitejs/plugin-vue's behavior.
    const scriptCache = new Map<string, { code: string; map: any }>();

    // Cache raw style block content for Vite mode.
    // In Vite mode, load() serves raw style source (e.g., SCSS) and lets Vite's
    // CSS pipeline handle preprocessing. transform() then runs compileStyleAsync()
    // for Vue-specific post-processing (scoping + CSS v-bind() rewriting).
    const styleBlockCache = new Map<string, StyleBlockEntry[]>();
    const compiledStyleCache = new Map<string, CompiledStyleArtifact[]>();

    // Build timing instrumentation accumulates per-phase timings across carrier transforms.
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
        if (query.vue || query.verter) {
          return id;
        }
        // Pluggable alias resolution (used by @verter/nuxt for #-prefixed imports)
        if (opts.resolveAlias) {
          const resolved = opts.resolveAlias(id);
          if (resolved) return resolved;
        }
      },

      load(id) {
        if (id === EXPORT_HELPER_ID) {
          return EXPORT_HELPER_CODE;
        }

        const { filename, query } = parseVueRequest(id);
        if (query.verter) {
          if (query.type === "script") {
            const cached = scriptCache.get(filename);
            if (cached) return { code: cached.code, map: cached.map ?? undefined };
          }
          if (query.type === "style") {
            const cached = compiledStyleCache.get(filename)?.[query.index ?? 0];
            if (cached) return { code: cached.code, map: cached.map ?? undefined };
          }
          return;
        }
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

        // Vite mode: return raw style source from cache.
        // Vite's CSS pipeline preprocesses (SCSS/SASS/Less), then our transform()
        // runs compileStyleAsync() for Vue-specific scoping + CSS v-bind() rewriting.
        if (viteConfig && query.type === "style") {
          const styles = styleBlockCache.get(filename);
          const entry = styles?.[query.index ?? 0];
          if (entry) {
            return { code: entry.content };
          }
        }

        const host = loadHost();

        // Reuse the compile profile from transform() to ensure the same componentId
        // and other fields are used. Fall back to a basic profile if not cached.
        const cachedProfile = profileCache.get(filename);
        const compileProfile: HostCompileProfile =
          cachedProfile ??
          (() => {
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
        // Main carrier files for compilation.
        if (filter(filename) && !query.vue && !query.verter) return true;
        // Style virtual files need a transform pass.
        // Vite mode: ALL styles need compileStyleAsync (scoping + CSS v-bind rewriting).
        // Non-Vite: only preprocessed (non-CSS) styles need the scoping pass.
        if (query.vue && query.type === "style" && filter(filename)) {
          if (viteConfig) return true;
          if (query.lang && query.lang !== "css") return true;
        }
        return false;
      },

      async buildStart() {
        if (!opts.preCompile) return;

        const { scanCarrierFiles } = await import("./core/scanner");
        const root = viteConfig?.root ?? process.cwd();
        const files = await scanCarrierFiles(root, filter);

        if (files.size === 0) return;

        const host = loadHost();
        const isProd = viteConfig
          ? viteConfig.command === "build" && !viteConfig.build?.ssr
          : process.env.NODE_ENV === "production";
        const ssr = viteConfig ? Boolean(viteConfig.build?.ssr) : false;
        const componentIdFn = opts.componentId || generateComponentId;

        for (const [filename, source] of files) {
          const carrierFramework = frameworkForFilename(filename, frameworkSelection);
          const componentId = componentIdFn(filename, source, isProd, viteConfig?.root);

          const profile: HostCompileProfile = {
            filename,
            ssr,
            isProduction: isProd,
            componentId,
            hmrStrategy:
              carrierFramework === "sveltejs"
                ? "none"
                : ((isProd ? "none" : hmrStrategy) as HostCompileProfile["hmrStrategy"]),
            sourceMap: true,
            forceJs: !viteConfig || meta.framework !== "vite",
          };

          profileCache.set(filename, profile);

          const upsertResult = host.upsert({
            inputId: filename,
            source,
          });
          await resolveUpsertDependencies(
            host,
            filename,
            upsertResult,
            typeof this?.resolve === "function" ? this.resolve.bind(this) : undefined,
          );

          if (carrierFramework === "vue") {
            // Vue macro type dependencies (package-backed .d.ts files).
            await hydrateMacroTypeDeps(
              host,
              filename,
              typeof this?.resolve === "function" ? this.resolve.bind(this) : undefined,
              getWorkspace() ?? undefined,
            );
          }

          // Preprocess non-native blocks (Pug, CoffeeScript, custom; style skipped in Vite mode)
          await applyPreprocessorRequests(
            host,
            filename,
            upsertResult,
            profile,
            viteConfig,
            opts.customBlocks,
          );

          const main = renderMainRuntime(
            host,
            filename,
            source,
            profile,
            typeof (this as { warn?: unknown })?.warn === "function"
              ? (this as unknown as { warn: (w: { message: string }) => void }).warn.bind(this)
              : undefined,
          );

          if (carrierFramework === "sveltejs") {
            scriptCache.set(filename, {
              code: main.code,
              map: main.sourceMap ?? null,
            });
            compiledStyleCache.set(filename, readCompiledStyleArtifacts(host, filename, profile));
          } else if (viteConfig) {
            scriptCache.set(filename, {
              code: main.code,
              map: main.sourceMap ?? null,
            });
          }
        }

        // Cross-file optimization: analyze render tree for prop constness
        if (opts.crossFileOptimize && isProd && frameworkSelection !== "sveltejs") {
          const resultJson = host.computeCrossFileOptimizations();
          const result = JSON.parse(resultJson);
          // Recompile files whose constness hints changed
          for (const file of result.changedFiles) {
            if (frameworkForFilename(file, frameworkSelection) !== "vue") continue;
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
        const carrierFramework = frameworkForFilename(filename, frameworkSelection);

        // Style virtual module transform:
        // - Vite mode: run compileStyleAsync() for scoping + CSS v-bind() rewriting.
        //   Vite's CSS pipeline has already preprocessed SCSS/SASS/Less before this.
        // - Non-Vite: use Rust processStyle for CSS scoping only.
        if (query.vue && query.type === "style") {
          if (viteConfig && compiler) {
            const profile = profileCache.get(filename);
            const styleIndex = query.index ?? 0;
            const styles = styleBlockCache.get(filename);
            const entry = styles?.[styleIndex];
            const scopedFlags = styleScopedCache.get(filename);
            const isScoped = query.scoped || entry?.scoped || (scopedFlags?.[styleIndex] ?? false);

            const result = await compiler.compileStyleAsync({
              source: code,
              filename,
              id: `data-v-${profile?.componentId ?? ""}`,
              scoped: isScoped,
              isProd: profile?.isProduction ?? false,
            });

            if (result.errors.length) {
              for (const err of result.errors) {
                this.error(typeof err === "string" ? err : err.message);
              }
            }

            return { code: result.code, map: result.map ?? null };
          }

          // Non-Vite: use Rust processStyle for CSS scoping only.
          let css = code;
          const profile = profileCache.get(filename);
          if (profile) {
            const styleIndex = query.index ?? 0;
            const scopedFlags = styleScopedCache.get(filename);
            const isScoped = query.scoped || (scopedFlags?.[styleIndex] ?? false);
            const processed = processStyle(css, {
              scopeId: profile.componentId ?? "",
              scoped: isScoped,
            });
            css = processed.code;
          }
          return { code: css, map: null };
        }

        if (query.verter) return;

        if (carrierFramework === "sveltejs") {
          const host = loadHost();
          const isProd = viteConfig
            ? viteConfig.command === "build" && !viteConfig.build?.ssr
            : process.env.NODE_ENV === "production";
          const ssr = opts.ssr?.enabled ?? (viteConfig ? Boolean(viteConfig.build?.ssr) : false);
          const componentIdFn = opts.componentId || generateComponentId;
          const componentId = componentIdFn(filename, code, isProd, viteConfig?.root);
          const profile: HostCompileProfile = {
            filename,
            ssr,
            isProduction: isProd,
            componentId,
            // Native Svelte HMR codegen is intentionally fail-closed; a bundler
            // file change uses the plugin's full-reload path instead.
            hmrStrategy: "none",
            sourceMap: true,
            forceJs: true,
          };
          profileCache.set(filename, profile);

          const t0 = timing ? performance.now() : 0;
          const upsertResult = host.upsert({ inputId: filename, source: code });
          const t1 = timing ? performance.now() : 0;
          await resolveUpsertDependencies(
            host,
            filename,
            upsertResult,
            typeof this?.resolve === "function" ? this.resolve.bind(this) : undefined,
          );
          await applyPreprocessorRequests(
            host,
            filename,
            upsertResult,
            profile,
            viteConfig,
            opts.customBlocks,
          );
          const t2 = timing ? performance.now() : 0;
          const main = renderMainRuntime(
            host,
            filename,
            code,
            profile,
            typeof (this as { warn?: unknown })?.warn === "function"
              ? (this as unknown as { warn: (w: { message: string }) => void }).warn.bind(this)
              : undefined,
          );
          const t3 = timing ? performance.now() : 0;

          scriptCache.set(filename, {
            code: main.code,
            map: main.sourceMap ?? null,
          });

          const styleArtifacts = readCompiledStyleArtifacts(host, filename, profile);
          compiledStyleCache.set(filename, styleArtifacts);

          if (timing) {
            tUpsertMs += t1 - t0;
            tDepsMs += t2 - t1;
            tCompileMs += t3 - t2;
            tFileCount++;
          }

          const scriptRequest = `${filename}?verter&type=script&lang.${main.lang ?? "js"}`;
          const wrapper = [
            `import Component from ${JSON.stringify(scriptRequest)}`,
            ...styleArtifacts.map(
              (style, index) =>
                `import ${JSON.stringify(`${filename}?verter&type=style&index=${index}&lang.${style.lang}`)}`,
            ),
            `export * from ${JSON.stringify(scriptRequest)}`,
            "export default Component",
          ].join("\n");
          return {
            code: wrapper,
            map: null,
            meta: { vite: { lang: "js" } },
          } as any;
        }

        const host = loadHost();
        const isProd = viteConfig
          ? viteConfig.command === "build" && !viteConfig.build?.ssr
          : process.env.NODE_ENV === "production";
        let ssr = opts.ssr?.enabled ?? (viteConfig ? Boolean(viteConfig.build?.ssr) : false);

        // Nuxt server/client component convention:
        // *.server.vue → always SSR, *.client.vue → never SSR
        if (isNuxt) {
          if (isServerComponent(filename)) ssr = true;
          if (isClientComponent(filename)) ssr = false;
        }

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

        // Extract per-style-block scoped flags from the SFC source.
        // Match <style ... scoped ...> tags in order.
        const scopedFlags: boolean[] = [];
        const styleRe = /<style\b([^>]*)>/gi;
        let styleMatch;
        while ((styleMatch = styleRe.exec(code)) !== null) {
          scopedFlags.push(/\bscoped\b/.test(styleMatch[1]));
        }
        styleScopedCache.set(filename, scopedFlags);

        // Register file in host (handles parsing, caching, change detection)
        const t0 = timing ? performance.now() : 0;
        const upsertResult = host.upsert({
          inputId: filename,
          source: code,
        });
        const t1 = timing ? performance.now() : 0;

        // In Vite mode, populate the style block cache with raw style content.
        // Vite's CSS pipeline will preprocess SCSS/SASS/Less between load() and transform().
        if (viteConfig && compiler) {
          const { descriptor } = compiler.parse(code, { filename });
          const entries: StyleBlockEntry[] = descriptor.styles.map((s: any) => ({
            content: s.content,
            lang: s.lang || "css",
            scoped: s.scoped ?? false,
            module: s.module ?? false,
          }));
          styleBlockCache.set(filename, entries);
        }

        await resolveUpsertDependencies(
          host,
          filename,
          upsertResult,
          typeof this?.resolve === "function" ? this.resolve.bind(this) : undefined,
        );

        // Hydrate macro type dependencies (package-backed .d.ts files)
        await hydrateMacroTypeDeps(
          host,
          filename,
          typeof this?.resolve === "function" ? this.resolve.bind(this) : undefined,
          getWorkspace() ?? undefined,
        );

        // Preprocess non-native blocks (Pug, CoffeeScript, custom; style skipped in Vite mode)
        await applyPreprocessorRequests(
          host,
          filename,
          upsertResult,
          profile,
          viteConfig,
          opts.customBlocks,
        );
        const t2 = timing ? performance.now() : 0;

        // Render the main module through the render-only bundler lane
        // (`compileMany(RuntimeRender)`) — the single shared substrate, no
        // parallel render API. The batch render profile + per-input
        // componentId reproduce EXACTLY the `getVirtualFile({ compileProfile })`
        // values this transform used before, so the Main output is
        // byte-identical. Soft (warning-severity) diagnostics are forwarded as
        // bundler warnings below.
        const main = renderMainRuntime(
          host,
          filename,
          code,
          profile,
          typeof (this as { warn?: unknown })?.warn === "function"
            ? (this as unknown as { warn: (w: { message: string }) => void }).warn.bind(this)
            : undefined,
        );
        const t3 = timing ? performance.now() : 0;

        if (timing) {
          tUpsertMs += t1 - t0;
          tDepsMs += t2 - t1;
          tCompileMs += t3 - t2;
          tFileCount++;
        }

        // Determine the effective language of the compiled output.
        const mainLang: string = main.lang ?? "ts";

        // Apply SSR transforms (import.meta dead-code elimination, component stripping)
        let compiledCode = main.code;
        const ssrOpts = opts.ssr;
        if (ssrOpts?.deadCodeElimination !== false) {
          compiledCode = replaceImportMetaSsr(compiledCode, ssr);
        }
        if (ssr && ssrOpts?.clientOnlyComponents?.length) {
          compiledCode = stripComponents(compiledCode, ssrOpts.clientOnlyComponents);
        }
        if (!ssr && ssrOpts?.serverOnlyComponents?.length) {
          compiledCode = stripComponents(compiledCode, ssrOpts.serverOnlyComponents);
        }

        if (viteConfig) {
          // In Vite mode, emit the compiled output as a script sub-request.
          // This matches @vitejs/plugin-vue's architecture where the main module
          // is a thin wrapper that imports from sub-requests:
          //   - Script: ?vue&type=script&lang.ts  â†’ processed by vite:esbuild (TS)
          //   - Script: ?vue&type=script&lang.jsx â†’ processed by @vitejs/plugin-vue-jsx
          //   - Style:  ?vue&type=style&lang.less â†’ processed by Vite's CSS pipeline
          //
          // This ensures downstream plugins (vue-jsx, external-globals, etc.) receive
          // properly processed JavaScript, not raw TS/JSX.
          scriptCache.set(filename, {
            code: compiledCode,
            map: main.sourceMap ?? null,
          });

          const scriptRequest = `${filename}?vue&type=script&lang.${mainLang}`;
          // Build style imports. Prefer the compiler-parsed cache (accurate lang,
          // scoped, module flags); fall back to a simple regex scan of the raw
          // SFC source when compiler-sfc is absent.
          //
          // CSS modules match @vitejs/plugin-vue:
          //   import styleN from "…?vue&type=style&index=N&lang.module.css"
          //   const cssModules = { "$style": styleN }
          //   export default _export_sfc(_sfc_main, [["__cssModules", cssModules]])
          // Vite's CSS pipeline treats `lang.module.*` as CSS modules and
          // default-exports the class-name map; Vue's instance proxy reads
          // `$style` from `type.__cssModules`.
          const cachedStyles = styleBlockCache.get(filename);
          const styleEntries: Array<{ lang: string; module: boolean | string }> =
            cachedStyles?.map((s) => ({ lang: s.lang, module: s.module })) ??
            (() => {
              const entries: Array<{ lang: string; module: boolean | string }> = [];
              const re = /<style\b([^>]*)>/gi;
              let m;
              while ((m = re.exec(code)) !== null) {
                const attrs = m[1];
                const langMatch = /\blang\s*=\s*["']([^"']+)["']/.exec(attrs);
                let module: boolean | string = false;
                const moduleNamed = /\bmodule\s*=\s*["']([^"']+)["']/.exec(attrs);
                if (moduleNamed) {
                  module = moduleNamed[1];
                } else if (/\bmodule\b/.test(attrs)) {
                  module = true;
                }
                entries.push({ lang: langMatch?.[1] ?? "css", module });
              }
              return entries;
            })();

          const styleLines: string[] = [];
          const cssModulesMap: Record<string, string> = {};
          for (let i = 0; i < styleEntries.length; i++) {
            const entry = styleEntries[i];
            const lang = entry.lang || "css";
            const baseRequest = `${filename}?vue&type=style&index=${i}&lang.${lang}`;
            if (entry.module) {
              // plugin-vue: rewrite `lang.css` → `lang.module.css` so Vite's
              // CSS-modules pipeline returns a class map as the default export.
              const moduleRequest = baseRequest.replace(/\.([A-Za-z0-9]+)$/, ".module.$1");
              const styleVar = `style${i}`;
              const exposedName = typeof entry.module === "string" ? entry.module : "$style";
              styleLines.push(`import ${styleVar} from ${JSON.stringify(moduleRequest)}`);
              cssModulesMap[exposedName] = styleVar;
            } else {
              styleLines.push(`import ${JSON.stringify(baseRequest)}`);
            }
          }

          const hasCssModules = Object.keys(cssModulesMap).length > 0;
          const mainLines = [
            `import _sfc_main from ${JSON.stringify(scriptRequest)}`,
            ...styleLines,
          ];
          if (hasCssModules) {
            mainLines.push(`import _export_sfc from ${JSON.stringify(EXPORT_HELPER_ID)}`);
            const mappingBody = Object.entries(cssModulesMap)
              .map(([key, value]) => `  ${JSON.stringify(key)}: ${value}`)
              .join(",\n");
            mainLines.push(`const cssModules = {\n${mappingBody}\n}`);
            mainLines.push(`export * from ${JSON.stringify(scriptRequest)}`);
            mainLines.push(
              `export default /*@__PURE__*/_export_sfc(_sfc_main, [["__cssModules", cssModules]])`,
            );
          } else {
            mainLines.push(`export * from ${JSON.stringify(scriptRequest)}`);
            mainLines.push(`export default _sfc_main`);
          }
          const mainModule = mainLines.join("\n");

          return {
            code: mainModule,
            map: null,
            meta: { vite: { lang: mainLang } },
          } as any;
        }

        // Non-Vite mode: inline everything (no sub-request support).
        // TS stripping is handled by the host via forceJs: true in the profile.
        return { code: compiledCode, map: null };
      },

      async closeBundle() {
        profileCache.clear();
        scriptCache.clear();
        styleScopedCache.clear();
        styleBlockCache.clear();
        compiledStyleCache.clear();
        resetHost();
      },

      async buildEnd() {
        if (!timing) return;
        const transformTotal = tUpsertMs + tDepsMs + tCompileMs;
        const lines = [
          `[verter] Build timing (${tFileCount} carrier files transformed):`,
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
          styleScopedCache.delete(id);
          styleBlockCache.delete(id);
          compiledStyleCache.delete(id);
        }
      },

      // Vite-specific hooks
      vite: {
        configResolved(resolvedConfig) {
          viteConfig = resolvedConfig;
          projectRoot = resolvedConfig.root;
          isNuxt = detectNuxt(projectRoot);
          // Resolve vue/compiler-sfc from the project root for compileStyleAsync().
          // This handles scoping + CSS v-bind() rewriting after Vite preprocesses styles.
          if (!compiler && frameworkSelection !== "sveltejs") {
            try {
              const { createRequire } = require("node:module");
              const { join } = require("node:path");
              const _require = createRequire(join(resolvedConfig.root, "package.json"));
              compiler = _require("vue/compiler-sfc");
            } catch {
              // compiler-sfc not available — style post-processing will be skipped
            }
          }
        },

        handleHotUpdate({ file, server, modules }) {
          if (!filter(file)) return;

          const host = loadHost();
          host.remove(file);
          profileCache.delete(file);
          scriptCache.delete(file);
          styleScopedCache.delete(file);
          styleBlockCache.delete(file);
          compiledStyleCache.delete(file);

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
}

const autoUnpluginFactory = createFrameworkFactory();
// Preserve the historical default/raw factory contract: existing consumers
// remain Vue-pinned. The additive `Verter` named export is the auto-detecting
// entry for mixed Vue/Svelte projects.
export const unpluginFactory = createFrameworkFactory("vue");
const svelteUnpluginFactory = createFrameworkFactory("sveltejs");

export const Verter = createUnplugin(autoUnpluginFactory);
export const VerterVue = createUnplugin(unpluginFactory);
export const VerterSvelte = createUnplugin(svelteUnpluginFactory);

export default VerterVue;

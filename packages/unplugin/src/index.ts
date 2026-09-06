import type { UnpluginFactory } from "unplugin";
import { createUnplugin } from "unplugin";
import { existsSync, statSync } from "node:fs";
import { createHash } from "node:crypto";
import { relative } from "node:path";
import type { ResolvedConfig } from "vite";
import type {
  VerterPluginOptions,
  VerterFrameworkLanguage,
  HmrStrategy,
  BlockPreprocessor,
} from "./core/types";
import { EXPORT_HELPER_ID, EXPORT_HELPER_CODE } from "./core/constants";
import type {
  BlockContentHashToken,
  HostUpdateResult,
  NativeBlockOverrideEntry,
  HostDependencyResolution,
} from "@verter/native";
import type { VerterHost } from "@verter/native";
import {
  loadHost,
  getWorkspace,
  generateComponentId,
  peekHost,
  transformVueStyle,
  resetHost,
  typedRenderRequest,
  requireRuntimeMain,
  runtimeNodeMatching,
  runtimeStyleArtifacts,
  forwardTypedDiagnostics,
  type VerterRenderProfile,
  type BundlerWarn,
} from "./core/compiler";
import { collectResolvableModuleReferenceSpecifiers } from "./core/dependency-resolution";
import { evictHydratedPath, hydrateMacroTypeDeps } from "./core/macro-type-hydration";
import { parseVueRequest } from "./core/utils";
import { copyCapturedBlockContentEcho, preprocessBlock } from "./core/preprocessor";
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

/**
 * Render a carrier file's runtime Main module through the native host.
 *
 * ONE typed `compileRequest` against the already-registered source — the
 * same demand translated once by `typedRenderRequest`. The typed response
 * carries the style nodes of the same compile, so no second native call
 * ever re-reads them. The published module is exactly what the typed route
 * composed, including the dev-server decoration the request's identity
 * states.
 *
 * A fatal compile surfaces as a thrown error (fail-closed). Non-fatal
 * (warning-severity) diagnostics are forwarded to the bundler via `warn`
 * (preserving code/message/source id); they never become build errors.
 * Error-severity diagnostics on a completed typed response throw.
 * Info-severity is dropped.
 */
function renderMainRuntime(
  host: VerterHost,
  canonicalId: string,
  profile: VerterRenderProfile,
  framework: RuntimeFramework,
  authoredOnlyStyleProcessing: boolean,
  needStyles: boolean,
  warn?: BundlerWarn,
): RenderedMain & { styles: CompiledStyleArtifact[] } {
  const response = executeTypedCompile(
    host,
    canonicalId,
    profile,
    framework,
    authoredOnlyStyleProcessing,
    warn,
  );
  const main = requireRuntimeMain(response, profile.ssr, canonicalId);

  return {
    code: main.code,
    sourceMap: main.sourceMap,
    lang: main.lang ?? undefined,
    // Style artifacts are read only when a consumer exists (the Vite flow
    // serves raw authored style content through its own CSS pipeline and
    // never reads compiled style nodes).
    styles: needStyles ? runtimeStyleArtifacts(response, profile.ssr) : [],
  };
}

/**
 * One typed compile against an already-registered carrier. Admission/decode
 * refusals and error-severity diagnostics fail closed; warning-severity is
 * forwarded. Shared by the main render path and the cross-file recompile.
 */
function executeTypedCompile(
  host: VerterHost,
  canonicalId: string,
  profile: VerterRenderProfile,
  framework: RuntimeFramework,
  authoredOnlyStyleProcessing: boolean,
  warn?: BundlerWarn,
) {
  let response;
  try {
    response = host.compileRequest(
      canonicalId,
      typedRenderRequest(profile, framework, {
        authoredOnlyStyles: authoredOnlyStyleProcessing,
      }),
    );
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    throw new Error(`[verter] ${canonicalId}: ${message}`);
  }
  forwardTypedDiagnostics(canonicalId, response, warn);
  return response;
}

/** Normalize Windows backslashes to forward slashes for the workspace API. */
function normalizePath(p: string): string {
  return p.replace(/\\/g, "/");
}

/**
 * The module id registered on `ssrContext.modules` for Vite SSR asset
 * collection. Vite's ssr-manifest keys are ROOT-RELATIVE — the same
 * `normalizePath(relative(root, filename))` shape `@vitejs/plugin-vue`
 * registers. Without a known root the id is omitted (the host falls back
 * to the canonical id).
 */
function ssrModuleIdFor(ssr: boolean, root: string, filename: string): string | undefined {
  if (!ssr || !root) return undefined;
  return normalizePath(relative(root, filename));
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
  ownerSource: string,
  upsertResult: HostUpdateResult,
  resolveId?: ResolveHook,
): Promise<HostUpdateResult> {
  let refreshedResult = upsertResult;
  let registeredExternalSource = false;

  // Resolve external sources (e.g., <style src="./foo.less">, <template src="./t.html">)
  if (upsertResult.externalSourceRequests.length > 0) {
    const path = await import("path");
    for (const req of upsertResult.externalSourceRequests) {
      const resolvedId: string = req.resolvedCanonicalId;
      const specifier: string = req.specifier;
      // Let the bundler choose the bytes when it can resolve this specifier,
      // while keeping the host-minted canonical ID as the VFS identity.
      const absPath = path.resolve(path.dirname(filename), specifier);
      const hookReadId = resolveId
        ? resolvedIdFromHookResult(await resolveId(specifier, filename, { skipSelf: true }))
        : null;
      let extSource = hookReadId
        ? await readTextFileThroughWorkspaceOrDisk(hookReadId)
        : await readTextFileThroughWorkspaceOrDisk(absPath);
      if (extSource === null && hookReadId && hookReadId !== absPath) {
        extSource = await readTextFileThroughWorkspaceOrDisk(absPath);
      }
      if (extSource !== null) {
        host.upsert({
          inputId: resolvedId,
          source: extSource,
          fileKind: "non_sfc",
        });
        registeredExternalSource = true;
      }
    }
  }

  // External requests are stamped when the owner is upserted. Re-publish the
  // byte-identical owner only after all external bytes are in the host VFS so
  // processed-content requests capture those exact bytes and fresh stamps.
  if (registeredExternalSource) {
    refreshedResult = host.upsert({
      inputId: filename,
      source: ownerSource,
    });
  }

  // Resolve exact and finite-set module references, then feed per-specifier
  // resolution records back to the host for exact cross-file resolution.
  const resolutions: HostDependencyResolution[] = [];
  const dependencySpecifiers = collectResolvableModuleReferenceSpecifiers(
    host,
    refreshedResult.moduleReferences ?? [],
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
  for (const req of refreshedResult.externalSourceRequests ?? []) {
    resolutions.push({
      specifier: req.specifier,
      resolvedCanonicalId: req.resolvedCanonicalId,
    });
  }
  host.setImportDependencies(filename, resolutions);
  return refreshedResult;
}

/**
 * Processes preprocessor requests from an upsert result.
 * For each non-native block (e.g., Pug template, SCSS style), invokes the
 * appropriate preprocessor and sends the results back to the host via
 * `applyBlockOverrides`.
 *
 * No compile profile is named: the host buckets supplied block content
 * under the profile-less bucket, which is exactly the bucket the typed
 * compile route reads — so the override the plugin applies here is visible
 * to the render that follows.
 */
async function applyPreprocessorRequests(
  host: VerterHost,
  filename: string,
  upsertResult: HostUpdateResult,
  viteConfig: unknown | null,
  customBlocks?: Record<string, BlockPreprocessor>,
): Promise<void> {
  if (!upsertResult.preprocessorRequests?.length) return;

  const overrides: NativeBlockOverrideEntry[] = [];
  for (const req of upsertResult.preprocessorRequests) {
    // In Vite mode, skip style preprocessing — Vite's CSS pipeline handles it.
    if (viteConfig && req.contentClass === "style") continue;

    const result = await preprocessBlock(req, filename, viteConfig, customBlocks);
    if (result) {
      overrides.push({
        ...copyCapturedBlockContentEcho(req),
        sourceSpaceToken: req.sourceSpaceToken,
        code: result.code,
        codeHash: hashBlockContent(result.code),
        sourceMap: result.sourceMap,
        sourceMapHash: result.sourceMap ? hashBlockContent(result.sourceMap) : undefined,
        suppliedProvenance: "@verter/unplugin",
      });
    }
  }
  if (overrides.length > 0) {
    host.applyBlockOverrides({
      canonicalId: filename,
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
async function detectNuxt(root: string): Promise<boolean> {
  const ws = getWorkspace();
  const normalizedRoot = normalizePath(root);
  const configFiles = ["nuxt.config.ts", "nuxt.config.js", "nuxt.config.mts", "nuxt.config.mjs"];
  for (const f of configFiles) {
    const path = `${normalizedRoot}/${f}`;
    // Workspace FS is async (libuv thread pool); a non-awaited Promise is
    // always truthy and would spuriously report every project as Nuxt.
    if (ws ? await ws.fileExists(path) : existsSync(path)) return true;
  }
  const nuxtDir = `${normalizedRoot}/.nuxt`;
  if (ws) {
    if (await ws.isDir(nuxtDir)) return true;
  } else if (existsSync(nuxtDir) && statSync(nuxtDir).isDirectory()) {
    return true;
  }
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

/**
 * Publish a native Vue style transform only when every rule was trusted.
 * Soft refusals delete untrusted rules from `code`; publishing that
 * truncated CSS with no signal is forbidden.
 */
function publishTransformedVueStyle(
  code: string,
  options: Parameters<typeof transformVueStyle>[1],
): ReturnType<typeof transformVueStyle> {
  const result = transformVueStyle(code, options);
  if (result.refusals.length > 0) {
    throw new Error(
      `transformVueStyle refused untrusted style rules and will not publish deleted CSS: ${result.refusals.join("; ")}`,
    );
  }
  return result;
}

/**
 * Vite style post-process. Live authority is native `transformVueStyle`
 * (scoping + v-bind). Vite's CSS pipeline has already preprocessed
 * SCSS/SASS/Less before this lane runs.
 */
function applyViteStyleLane(
  code: string,
  filename: string,
  scopeId: string,
  scoped: boolean,
): { code: string; map: unknown } {
  const result = publishTransformedVueStyle(code, {
    scopeId,
    scoped,
    filename,
    sourcemap: true,
  });
  let map: unknown = null;
  if (result.sourceMap) {
    try {
      map = JSON.parse(result.sourceMap);
    } catch {
      map = null;
    }
  }
  return { code: result.code, map };
}

/**
 * Non-Vite style post-process. Live authority is native `transformVueStyle`
 * (CSS scoping + v-bind).
 */
function applyNonViteStyleLane(css: string, scopeId: string, scoped: boolean): string {
  return publishTransformedVueStyle(css, { scopeId, scoped }).code;
}

interface StyleBlockEntry {
  blockToken: string;
  content: string;
  lang: string;
  scoped: boolean;
  module: boolean | string;
}

interface HostStructureRange {
  start: number;
  end: number;
}

interface HostStructureAttribute {
  name?: { normalized: string };
  value?: string;
}

interface HostStructureSection {
  blockToken: string;
  role: { kind: string; scoped?: boolean };
  contentRange: HostStructureRange;
  attributes: HostStructureAttribute[];
}

interface HostDocumentStructure {
  blocks: Array<{ kind: string; section?: HostStructureSection }>;
}

function utf8OffsetToUtf16(source: string, target: number): number | null {
  let bytes = 0;
  let units = 0;
  for (const scalar of source) {
    if (bytes === target) return units;
    const code = scalar.codePointAt(0)!;
    bytes += code <= 0x7f ? 1 : code <= 0x7ff ? 2 : code <= 0xffff ? 3 : 4;
    units += scalar.length;
    if (bytes > target) return null;
  }
  return bytes === target ? units : null;
}

function styleEntriesFromStructure(
  host: VerterHost,
  filename: string,
  source: string,
): StyleBlockEntry[] {
  const encoded = host.getDocumentStructure(filename);
  if (encoded === null) return [];
  const structure = JSON.parse(encoded) as HostDocumentStructure;
  const entries: StyleBlockEntry[] = [];
  for (const block of structure.blocks) {
    const section = block.kind === "section" ? block.section : undefined;
    if (section?.role.kind !== "style") continue;
    const start = utf8OffsetToUtf16(source, section.contentRange.start);
    const end = utf8OffsetToUtf16(source, section.contentRange.end);
    if (start === null || end === null) continue;
    const attribute = (name: string) =>
      section.attributes.find((entry) => entry.name?.normalized === name);
    const module = attribute("module");
    entries.push({
      blockToken: section.blockToken,
      content: source.slice(start, end),
      lang: attribute("lang")?.value ?? "css",
      scoped: section.role.scoped ?? false,
      module: module === undefined ? false : (module.value ?? true),
    });
  }
  return entries;
}

function createFrameworkFactory(
  pinnedFramework?: RuntimeFramework,
): UnpluginFactory<VerterPluginOptions | undefined> {
  return (options, meta) => {
    const opts = options ?? {};
    const frameworkSelection = pinnedFramework ?? opts.lang ?? "auto";
    let viteConfig: ResolvedConfig | null = null;
    const hmrStrategy = getHmrStrategy(meta.framework);
    const filter = createFilter(opts.include, frameworkSelection);
    let isNuxt = false;
    let projectRoot = "";

    // Store render profiles from transform() so load() can reuse the same
    // componentId and other demand fields for virtual file requests.
    const profileCache = new Map<string, VerterRenderProfile>();

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
    // CSS pipeline handle preprocessing. transform() then runs transformVueStyle()
    // for Vue-specific post-processing (scoping + CSS v-bind() rewriting).
    const styleBlockCache = new Map<string, StyleBlockEntry[]>();
    const compiledStyleCache = new Map<string, CompiledStyleArtifact[]>();

    // Build timing instrumentation accumulates timings across carrier transforms.
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
        // runs transformVueStyle() for Vue-specific scoping + CSS v-bind() rewriting.
        if (viteConfig && query.type === "style") {
          const styles = styleBlockCache.get(filename);
          const entry = styles?.[query.index ?? 0];
          if (entry) {
            return { code: entry.content };
          }
        }

        // Style sub-requests of a transformed carrier are served from the
        // same compile transform() already ran — no second native read.
        if (query.type === "style") {
          const compiled = compiledStyleCache.get(filename)?.[query.index ?? 0];
          if (compiled) return { code: compiled.code, map: compiled.map ?? undefined };
        }

        const host = loadHost();

        // Reuse the render profile from transform() to ensure the same componentId
        // and other fields are used. Fall back to a basic profile if not cached.
        const cachedProfile = profileCache.get(filename);
        const renderProfile: VerterRenderProfile =
          cachedProfile ??
          (() => {
            const isProd = viteConfig
              ? viteConfig.command === "build" && !viteConfig.build?.ssr
              : process.env.NODE_ENV === "production";
            const ssr = viteConfig ? Boolean(viteConfig.build?.ssr) : false;
            return {
              isProduction: isProd,
              customElement: false,
              ssr,
              forceJs: false,
              sourceMap: false,
              hmrStrategy: isProd ? "none" : hmrStrategy,
            };
          })();

        try {
          const lt0 = timing ? performance.now() : 0;
          const loadFramework = frameworkForFilename(filename, frameworkSelection);
          // One typed compile and pick the requested node — the same single
          // native route every other read of this carrier uses. The read
          // inherits the fail-closed disposition the profile-bearing virtual
          // read had: an error-severity compile throws (the catch below turns
          // that into "no module") instead of publishing recovery output as
          // if it were a good module. Warnings are swallowed rather than
          // re-forwarded — the carrier's transform already reported them, and
          // a sub-request must not duplicate a build warning.
          const response = executeTypedCompile(
            host,
            filename,
            renderProfile,
            loadFramework,
            // Vite's CSS pipeline owns style preprocessing for Vue carriers;
            // the authored-only cascade keeps authored bytes intact for it.
            Boolean(viteConfig) && loadFramework === "vue",
            () => {},
          );
          const node = runtimeNodeMatching(response, renderProfile.ssr, query);
          if (timing) {
            tLoadMs += performance.now() - lt0;
            tLoadFileCount++;
          }
          if (!node) return undefined;
          return {
            code: node.code,
            map: node.sourceMap ?? undefined,
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
        // Vite mode: ALL styles need transformVueStyle (scoping + CSS v-bind rewriting).
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

          const profile: VerterRenderProfile = {
            filename,
            customElement: false,
            ssr,
            ssrModuleId: ssrModuleIdFor(ssr, projectRoot, filename),
            isProduction: isProd,
            componentId,
            hmrStrategy: carrierFramework === "sveltejs" ? "none" : isProd ? "none" : hmrStrategy,
            sourceMap: true,
            forceJs: !viteConfig || meta.framework !== "vite",
          };

          profileCache.set(filename, profile);

          let upsertResult = host.upsert({
            inputId: filename,
            source,
          });
          upsertResult = await resolveUpsertDependencies(
            host,
            filename,
            source,
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
            viteConfig,
            opts.customBlocks,
          );

          const main = renderMainRuntime(
            host,
            filename,
            profile,
            carrierFramework,
            Boolean(viteConfig) && carrierFramework === "vue",
            carrierFramework === "sveltejs" || !viteConfig,
            typeof (this as { warn?: unknown })?.warn === "function"
              ? (this as unknown as { warn: (w: { message: string }) => void }).warn.bind(this)
              : undefined,
          );

          if (carrierFramework === "sveltejs") {
            scriptCache.set(filename, {
              code: main.code,
              map: main.sourceMap ?? null,
            });
            compiledStyleCache.set(filename, main.styles);
          } else if (viteConfig) {
            scriptCache.set(filename, {
              code: main.code,
              map: main.sourceMap ?? null,
            });
          } else {
            // Non-Vite style sub-requests are served from this same compile.
            compiledStyleCache.set(filename, main.styles);
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
              const response = executeTypedCompile(
                host,
                file,
                cachedProfile,
                "vue",
                Boolean(viteConfig),
                typeof (this as { warn?: unknown })?.warn === "function"
                  ? (this as unknown as { warn: BundlerWarn }).warn.bind(this)
                  : undefined,
              );
              const main = requireRuntimeMain(response, cachedProfile.ssr, file);
              const recompiled = { code: main.code, sourceMap: main.sourceMap ?? null };
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
        // - Vite mode: applyViteStyleLane → transformVueStyle() for scoping
        //   + CSS v-bind() rewriting. Vite's CSS pipeline has already
        //   preprocessed SCSS/SASS/Less before this.
        // - Non-Vite: applyNonViteStyleLane → transformVueStyle for CSS scoping.
        if (query.vue && query.type === "style") {
          if (viteConfig) {
            const profile = profileCache.get(filename);
            const styleIndex = query.index ?? 0;
            const styles = styleBlockCache.get(filename);
            const entry = styles?.[styleIndex];
            const scopedFlags = styleScopedCache.get(filename);
            const isScoped = query.scoped || entry?.scoped || (scopedFlags?.[styleIndex] ?? false);

            return applyViteStyleLane(code, filename, profile?.componentId ?? "", isScoped);
          }

          // Non-Vite: use Rust transformVueStyle for CSS scoping — but not on
          // bytes this plugin's own compile already produced. load() serves a
          // carrier's style sub-request from compiledStyleCache, preprocessed
          // AND scoped, so a second pass over those same bytes would
          // double-scope the moment an id ever carries `&scoped` or
          // styleScopedCache gains a non-Vite entry. Byte identity, not id
          // identity: a bundler passing authored bytes for the same id still
          // gets the scoping pass.
          const served = compiledStyleCache.get(filename)?.[query.index ?? 0];
          if (served && served.code === code) {
            return { code: served.code, map: served.map ?? null };
          }
          let css = code;
          const profile = profileCache.get(filename);
          if (profile) {
            const styleIndex = query.index ?? 0;
            const scopedFlags = styleScopedCache.get(filename);
            const isScoped = query.scoped || (scopedFlags?.[styleIndex] ?? false);
            css = applyNonViteStyleLane(css, profile.componentId ?? "", isScoped);
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
          const profile: VerterRenderProfile = {
            filename,
            customElement: false,
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
          let upsertResult = host.upsert({ inputId: filename, source: code });
          const t1 = timing ? performance.now() : 0;
          upsertResult = await resolveUpsertDependencies(
            host,
            filename,
            code,
            upsertResult,
            typeof this?.resolve === "function" ? this.resolve.bind(this) : undefined,
          );
          await applyPreprocessorRequests(
            host,
            filename,
            upsertResult,
            viteConfig,
            opts.customBlocks,
          );
          const t2 = timing ? performance.now() : 0;
          const main = renderMainRuntime(
            host,
            filename,
            profile,
            "sveltejs",
            false,
            true,
            typeof (this as { warn?: unknown })?.warn === "function"
              ? (this as unknown as { warn: (w: { message: string }) => void }).warn.bind(this)
              : undefined,
          );
          const t3 = timing ? performance.now() : 0;

          scriptCache.set(filename, {
            code: main.code,
            map: main.sourceMap ?? null,
          });

          const styleArtifacts = main.styles;
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

        const profile: VerterRenderProfile = {
          filename,
          customElement: false,
          ssr,
          ssrModuleId: ssrModuleIdFor(ssr, projectRoot, filename),
          isProduction: isProd,
          componentId,
          hmrStrategy: isProd ? "none" : hmrStrategy,
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
        let upsertResult = host.upsert({
          inputId: filename,
          source: code,
        });
        const t1 = timing ? performance.now() : 0;

        // Project authored inline styles from the registered inventory. External
        // content is unavailable on this structure-only surface.
        let styleEntries: StyleBlockEntry[] | null = null;
        if (viteConfig) {
          styleEntries = styleEntriesFromStructure(host, upsertResult.canonicalId, code);
          styleBlockCache.set(filename, styleEntries);
          styleScopedCache.set(
            filename,
            styleEntries.map((entry) => entry.scoped),
          );
        }

        upsertResult = await resolveUpsertDependencies(
          host,
          filename,
          code,
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
          viteConfig,
          opts.customBlocks,
        );
        const t2 = timing ? performance.now() : 0;

        // Render the main module: one typed compile request against the
        // registered source. Soft (warning-severity) diagnostics are
        // forwarded as bundler warnings.
        const main = renderMainRuntime(
          host,
          filename,
          profile,
          "vue",
          Boolean(viteConfig) && carrierFramework === "vue",
          !viteConfig,
          typeof (this as { warn?: unknown })?.warn === "function"
            ? (this as unknown as { warn: (w: { message: string }) => void }).warn.bind(this)
            : undefined,
        );
        const t3 = timing ? performance.now() : 0;

        // Non-Vite style sub-requests are served from this same compile.
        if (!viteConfig) {
          compiledStyleCache.set(filename, main.styles);
        }

        if (timing) {
          tUpsertMs += t1 - t0;
          tDepsMs += t2 - t1;
          tCompileMs += t3 - t2;
          tFileCount++;
        }

        // Determine the effective language of the compiled output.
        const mainLang: string = main.lang ?? "ts";

        // Apply SSR transforms (import.meta dead-code elimination, component
        // stripping). Each rewrite carries the in-flight map along with the
        // code, so the map's generated columns stay aligned with whatever
        // text it currently describes.
        let compiledCode = main.code;
        let compiledMap = main.sourceMap;
        const ssrOpts = opts.ssr;
        if (ssrOpts?.deadCodeElimination !== false) {
          ({ code: compiledCode, map: compiledMap } = replaceImportMetaSsr(
            compiledCode,
            ssr,
            compiledMap,
          ));
        }
        if (ssr && ssrOpts?.clientOnlyComponents?.length) {
          ({ code: compiledCode, map: compiledMap } = stripComponents(
            compiledCode,
            ssrOpts.clientOnlyComponents,
            compiledMap,
          ));
        }
        if (!ssr && ssrOpts?.serverOnlyComponents?.length) {
          ({ code: compiledCode, map: compiledMap } = stripComponents(
            compiledCode,
            ssrOpts.serverOnlyComponents,
            compiledMap,
          ));
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
            map: compiledMap ?? null,
          });

          const scriptRequest = `${filename}?vue&type=script&lang.${mainLang}`;
          // Build style imports from the registered inventory projection.
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
            cachedStyles?.map((s) => ({ lang: s.lang, module: s.module })) ?? [];

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
        //
        // The inline product IS the compiled module, so it carries the map the
        // profile asked the host for — the same map the Vite branch above hands
        // to the script sub-request. Returning `null` here would drop a map the
        // host published and leave a non-Vite consumer with no way back to the
        // authored SFC. Absent a requested map the host publishes none and this
        // is `null`, which is what "no map" means to a bundler.
        return { code: compiledCode, map: compiledMap ?? null };
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
        // A changed dependency file (type .d.ts/.ts, hydrated .vue dep)
        // must re-hydrate on next demand — evict from the per-host
        // hydration memo without lazily creating a host.
        const existing = peekHost();
        if (existing) {
          evictHydratedPath(existing, id);
        }
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
        async configResolved(resolvedConfig) {
          viteConfig = resolvedConfig;
          projectRoot = resolvedConfig.root;
          isNuxt = await detectNuxt(projectRoot);
        },

        handleHotUpdate({ file, server, modules }) {
          {
            // Changed hydrated dependency files re-hydrate on next demand.
            const existing = peekHost();
            if (existing) {
              evictHydratedPath(existing, file);
            }
          }
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
function hashBlockContent(content: string): BlockContentHashToken {
  const digest = createHash("sha256")
    .update("verter.block-content.bytes.v1\0")
    .update(content)
    .digest("hex");
  return `sha256:${digest}` as BlockContentHashToken;
}

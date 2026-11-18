import type { UnpluginFactory } from "unplugin";
import { createUnplugin } from "unplugin";
import { existsSync, statSync } from "node:fs";
import type { ResolvedConfig } from "vite";
import type { VerterPluginOptions, VerterFrameworkLanguage } from "./core/types";
import { EXPORT_HELPER_CODE } from "./core/constants";
import { canonicalizeId } from "@verter/native";
import { graphMode, graphTarget } from "./core/host-session/identity";
import type {
  BundlerGraphKind,
  HookInvocationId,
  HostCanonicalId,
} from "./core/host-session/identity";
import { BundlerHostSession } from "./core/host-session/session";
import { carriersFromInclude } from "./core/host-session/session";
import type { HookGraphContext, SessionBindingInput } from "./core/host-session/session";
import { CarrierPipeline } from "./core/pipeline/carrier-demand";
import type { DemandContext } from "./core/pipeline/carrier-demand";
import {
  executeLoad,
  executeTransform,
  planLoad,
  planTransform,
  transformIncludes,
} from "./core/pipeline/plan";
import type { HookOutput } from "./core/pipeline/plan";

export type {
  VerterPluginOptions,
  VerterFrameworkLanguage,
  HmrStrategy,
  Options,
} from "./core/types";

type RuntimeFramework = Exclude<VerterFrameworkLanguage, "auto">;

/** Detect if a project uses Nuxt by checking for nuxt.config.* or .nuxt/ directory. */
function detectNuxt(root: string): boolean {
  const normalizedRoot = canonicalizeId(root);
  const configFiles = ["nuxt.config.ts", "nuxt.config.js", "nuxt.config.mts", "nuxt.config.mjs"];
  for (const f of configFiles) {
    const path = `${normalizedRoot}/${f}`;
    if (existsSync(path)) return true;
  }
  const nuxtDir = `${normalizedRoot}/.nuxt`;
  return existsSync(nuxtDir) && statSync(nuxtDir).isDirectory();
}

/** What the hook's plugin context lends one demand. */
interface HookContextLike {
  readonly resolve?: unknown;
  readonly warn?: unknown;
  readonly addWatchFile?: unknown;
  readonly environment?: HookGraphContext["environment"];
}

function boundMethod<T>(owner: object | undefined, method: unknown): T | null {
  return typeof method === "function" ? (method.bind(owner) as T) : null;
}

function createFrameworkFactory(
  pinnedFramework?: RuntimeFramework,
): UnpluginFactory<VerterPluginOptions | undefined> {
  return (options, meta) => {
    const opts = options ?? {};
    const frameworkSelection = pinnedFramework ?? opts.lang ?? "auto";
    // A framework entry's `include` means "these files are that framework".
    const pinnedCarriers = pinnedFramework
      ? carriersFromInclude(opts.include, pinnedFramework)
      : null;

    // The plugin instance's one bound session and its pipeline: identity,
    // published modules and native host all live there, so nothing is shared
    // between instances.
    let session: BundlerHostSession | null = null;
    let pipeline: CarrierPipeline | null = null;
    // What `configResolved` stated; a session closed by `closeBundle` is bound
    // again from it by the next hook.
    let lastInput: SessionBindingInput | null = null;

    function inputFromEnvironment(): SessionBindingInput {
      return {
        adapter: meta.framework,
        root: null,
        mode: process.env.NODE_ENV === "production" ? "prod" : "dev",
        defaultTarget: opts.ssr?.enabled ? "ssr" : "client",
        frameworkSelection,
        options: opts,
        carriers: pinnedCarriers,
        hostCssPipeline: false,
        hostStripsTypeScript: false,
        hostConfig: null,
        nuxtFileRoles: false,
      };
    }

    function ensureSession(): { active: BundlerHostSession; pipeline: CarrierPipeline } {
      if (!session || session.state !== "open" || !pipeline) {
        pipeline?.dispose();
        session = BundlerHostSession.bind(lastInput ?? inputFromEnvironment());
        pipeline = new CarrierPipeline(session);
      }
      return { active: session, pipeline };
    }

    function closeSession(): void {
      pipeline?.dispose();
      session?.close();
      pipeline = null;
      session = null;
    }

    /** The graph of one hook call, from the Vite environment the hook runs in when it has one. */
    function graphOf(active: BundlerHostSession, hookThis: unknown): BundlerGraphKind {
      const environment = (hookThis as HookContextLike | undefined)?.environment;
      return active.graphFor({ environment });
    }

    function demandContext(active: BundlerHostSession, hookThis: unknown): DemandContext {
      const self = hookThis as (HookContextLike & object) | undefined;
      return {
        invocation: active.nextInvocation() as HookInvocationId,
        resolve: boundMethod(self, self?.resolve),
        warn: boundMethod(self, self?.warn),
        addWatchFile: boundMethod(self, self?.addWatchFile),
      };
    }

    function hookResult(output: HookOutput | undefined) {
      if (!output) return undefined;
      if (output.lang === undefined) return { code: output.code, map: output.map };
      return { code: output.code, map: output.map, meta: { vite: { lang: output.lang } } } as any;
    }

    // Build instrumentation: VERTER_TIMING=1 prints the pipeline's work counters at buildEnd.
    const timing = process.env.VERTER_TIMING === "1";

    return {
      name: "unplugin-verter",

      resolveId(id) {
        const { active } = ensureSession();
        const identified = active.identify(id, graphOf(active, this));
        if (identified.kind === "generated") return id;
        if (identified.kind === "rejected") {
          throw new Error(`[verter] ${identified.raw}: ${identified.reason}`);
        }
        if (identified.kind === "carrier-module" && identified.request.marker !== undefined) {
          return id;
        }
        // Pluggable alias resolution (used by @verter/nuxt for #-prefixed imports)
        if (opts.resolveAlias) {
          const resolved = opts.resolveAlias(id);
          if (resolved) return resolved;
        }
      },

      load(id) {
        const { active, pipeline } = ensureSession();
        const identified = active.identify(id, graphOf(active, this));
        const plan = planLoad(identified, active.binding);
        if (plan.kind === "skip") return;
        if (plan.kind === "generated") return EXPORT_HELPER_CODE;
        return executeLoad(plan, pipeline, demandContext(active, this)).then((output) =>
          output ? { code: output.code, map: output.map ?? undefined } : undefined,
        ) as any;
      },

      transformInclude(id) {
        const { active } = ensureSession();
        return transformIncludes(active.identify(id, graphOf(active, this)), active.binding);
      },

      async buildStart() {
        if (!opts.preCompile) return;

        const { active, pipeline } = ensureSession();
        const graph = graphOf(active, this);
        const { scanCarrierFiles } = await import("./core/scanner");
        const files = await scanCarrierFiles(
          active.binding.root,
          (file) => active.includesCarrier(file),
          active.workspace(),
        );
        if (files.size === 0) return;

        // Pre-compile is the same demand a transform makes, with the scanned bytes.
        const ctx = demandContext(active, this);
        for (const [filename, source] of files) {
          await pipeline.demand(
            { carrier: canonicalizeId(filename) as HostCanonicalId, rawPath: filename, graph },
            { kind: "bundler", code: source },
            ctx,
          );
        }

        // Cross-file optimization: analyze render tree for prop constness
        const isProduction = graphMode(graph) === "prod" && graphTarget(graph) === "client";
        if (opts.crossFileOptimize && isProduction && frameworkSelection !== "sveltejs") {
          const result = JSON.parse(active.host().computeCrossFileOptimizations());
          // Recompile files whose constness hints changed
          for (const file of result.changedFiles) {
            if (active.frameworkFor(file) !== "vue") continue;
            const cached = active.catalog.record(file as HostCanonicalId, graph);
            const source = cached ? files.get(cached.rawPath) : undefined;
            if (!cached || source === undefined) continue;
            pipeline.invalidate(cached.rawPath);
            await pipeline.demand(
              { carrier: cached.carrier, rawPath: cached.rawPath, graph },
              { kind: "bundler", code: source },
              ctx,
            );
          }
          // Emit diagnostics as warnings
          for (const diag of result.diagnostics) {
            console.warn(`[verter] ${diag.code}: ${diag.message} (${diag.fileId})`);
          }
        }
      },

      async transform(code, id) {
        const { active, pipeline } = ensureSession();
        const identified = active.identify(id, graphOf(active, this));
        const plan = planTransform(identified, active.binding);
        if (plan.kind === "skip") return;
        return hookResult(
          await executeTransform(plan, code, pipeline, active.binding, demandContext(active, this)),
        );
      },

      async closeBundle() {
        closeSession();
      },

      async buildEnd() {
        if (!timing || !pipeline) return;
        const counters = pipeline.counters();
        const lines = [
          `[verter] Build work: hooks=${counters.hookInvocations}  registrations=${counters.registrations}  compiles=${counters.compileRequests}  hydrationReads=${counters.hydrationReads}  routePushes=${counters.routePushes}`,
          `  supersededFlights=${counters.supersededFlights}  staleDiscards=${counters.staleDiscards}  mapCompositions=${counters.mapCompositions}`,
        ];
        const host = session?.peekHost();
        const metrics = (host as any)?.getMetrics?.();
        if (metrics) {
          lines.push(
            `  host metrics: upserts=${metrics.upserts}  compileRequests=${metrics.compileRequests}  cacheHits=${metrics.compileCacheHits}  hitRate=${(metrics.compileCacheHitRate * 100).toFixed(1)}%`,
            `    rustParseTotal=${(metrics.sliceHashTimeUsTotal / 1000).toFixed(0)}ms  rustCompileTotal=${(metrics.compileTimeUsTotal / 1000).toFixed(0)}ms`,
          );
        }
        console.log(lines.join("\n"));
      },

      watchChange(id) {
        // A changed carrier or dependency file (type .d.ts/.ts, hydrated .vue
        // dep) is recompiled or re-hydrated on its next demand.
        ensureSession().pipeline.invalidate(id);
      },

      // Vite-specific hooks
      vite: {
        configResolved(resolvedConfig: ResolvedConfig) {
          lastInput = {
            adapter: meta.framework,
            root: resolvedConfig.root,
            mode: resolvedConfig.command === "build" ? "prod" : "dev",
            defaultTarget:
              (opts.ssr?.enabled ?? Boolean(resolvedConfig.build?.ssr)) ? "ssr" : "client",
            frameworkSelection,
            options: opts,
            carriers: pinnedCarriers,
            hostCssPipeline: true,
            hostStripsTypeScript: meta.framework === "vite",
            hostConfig: resolvedConfig,
            nuxtFileRoles: detectNuxt(resolvedConfig.root),
          };
          // A session bound from an earlier input would keep serving the old configuration.
          closeSession();
        },

        handleHotUpdate({ file, server, modules }) {
          const { active, pipeline } = ensureSession();
          pipeline.invalidate(file);
          if (!active.includesCarrier(file)) return;

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

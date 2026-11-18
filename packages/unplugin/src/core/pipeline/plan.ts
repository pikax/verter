import type { HostVirtualNodeKind } from "@verter/native";
import { transformVueStyle } from "../compiler";
import { EXPORT_HELPER_CODE, EXPORT_HELPER_ID } from "../constants";
import type { CarrierRecord } from "../host-session/catalog";
import type {
  CarrierFramework,
  GeneratedModuleName,
  HostCanonicalId,
} from "../host-session/identity";
import type { ModuleIdentification, SessionBinding } from "../host-session/session";
import { PipelineRefusalError } from "./admission";
import type { CarrierDemandKey, CarrierPipeline, DemandContext } from "./carrier-demand";

/** `bundler-css`: the host's CSS pipeline processes authored bytes. `compiled`: this package serves final CSS. */
export type StyleLane = "bundler-css" | "compiled";

export type LoadPlan =
  | { readonly kind: "skip" }
  | { readonly kind: "generated"; readonly name: GeneratedModuleName }
  | { readonly kind: "carrier-script"; readonly key: CarrierDemandKey; readonly request: string }
  | {
      readonly kind: "style";
      readonly key: CarrierDemandKey;
      readonly request: string;
      readonly index: number;
      readonly lane: StyleLane;
    }
  | {
      readonly kind: "compiled-node";
      readonly key: CarrierDemandKey;
      readonly request: string;
      readonly nodeKind: string;
      readonly index: number;
    };

export type TransformPlan =
  | { readonly kind: "skip" }
  | {
      readonly kind: "carrier-main";
      readonly key: CarrierDemandKey;
      readonly framework: CarrierFramework;
    }
  | {
      readonly kind: "style-scope";
      readonly key: CarrierDemandKey;
      readonly request: string;
      readonly index: number;
      /** The request itself said `scoped`. */
      readonly scopedHint: boolean;
    }
  | {
      readonly kind: "style-compiled";
      readonly key: CarrierDemandKey;
      readonly request: string;
      readonly index: number;
      readonly scopedHint: boolean;
    };

/** What a hook returns to the bundler. */
export interface HookOutput {
  readonly code: string;
  readonly map: unknown | null;
  /** The language the bundler should treat `code` as, when it matters (`ts`, `js`, `jsx`). */
  readonly lang?: string;
}

const SKIP = { kind: "skip" } as const;

export function styleLaneFor(binding: SessionBinding, framework: CarrierFramework): StyleLane {
  return binding.hostCssPipeline && framework === "vue" ? "bundler-css" : "compiled";
}

type CarrierIdentification = Extract<ModuleIdentification, { kind: "carrier-module" }>;

/** The carrier module of an identification; `null` for one that is not this session's. */
function carrierOf(identification: ModuleIdentification): CarrierIdentification | null {
  switch (identification.kind) {
    case "carrier-module":
      return identification;
    case "rejected":
      throw new Error(`[verter] ${identification.raw}: ${identification.reason}`);
    default:
      return null;
  }
}

function demandKeyOf(identification: CarrierIdentification): CarrierDemandKey {
  return {
    carrier: identification.request.canonicalId as HostCanonicalId,
    rawPath: identification.request.rawPath,
    graph: identification.graph,
  };
}

/** Pure. `foreign` gives `skip`. A `rejected` identification throws. */
export function planLoad(identification: ModuleIdentification, binding: SessionBinding): LoadPlan {
  if (identification.kind === "generated") return { kind: "generated", name: identification.name };
  const carrier = carrierOf(identification);
  // The carrier's own file is the bundler's to read.
  if (carrier === null || carrier.request.marker === undefined) return SKIP;
  const key = demandKeyOf(carrier);
  const request = carrier.raw;
  const { kind, index = 0 } = carrier.request.nodeKind;
  switch (kind) {
    case "script":
      return { kind: "carrier-script", key, request };
    case "style":
      return { kind: "style", key, request, index, lane: styleLaneFor(binding, carrier.framework) };
    case "template":
      return { kind: "compiled-node", key, request, nodeKind: "template", index: 0 };
    case "custom":
      return { kind: "compiled-node", key, request, nodeKind: "custom", index };
    default:
      return SKIP;
  }
}

/** Pure. `foreign` gives `skip`. A `rejected` identification throws. */
export function planTransform(
  identification: ModuleIdentification,
  binding: SessionBinding,
): TransformPlan {
  const carrier = carrierOf(identification);
  if (carrier === null) return SKIP;
  const { request, framework } = carrier;
  const key = demandKeyOf(carrier);
  if (request.marker === undefined) return { kind: "carrier-main", key, framework };
  if (request.marker !== "vue" || request.nodeKind.kind !== "style") return SKIP;
  const style = {
    key,
    request: carrier.raw,
    index: request.nodeKind.index ?? 0,
    scopedHint: request.scoped,
  };
  return styleLaneFor(binding, framework) === "bundler-css"
    ? { kind: "style-scope", ...style }
    : { kind: "style-compiled", ...style };
}

/**
 * The bundler-facing transform filter: main requests; style requests on the
 * `bundler-css` lane; on the `compiled` lane only style requests in a language
 * other than `css`, since the compile already scoped plain CSS. A `rejected`
 * identification is not included.
 */
export function transformIncludes(
  identification: ModuleIdentification,
  binding: SessionBinding,
): boolean {
  if (identification.kind !== "carrier-module") return false;
  const plan = planTransform(identification, binding);
  switch (plan.kind) {
    case "skip":
      return false;
    case "carrier-main":
    case "style-scope":
      return true;
    case "style-compiled": {
      const lang = identification.request.lang;
      return lang !== undefined && lang !== "css";
    }
  }
}

/**
 * Serve one planned load. `skip` gives `undefined`. A carrier sub-request is
 * served from the published record, demanding the carrier from disk when no
 * record exists. A request for a block the carrier does not have rejects with
 * `PipelineRefusalError` (`module-not-published`); it never resolves to
 * `undefined`.
 */
export async function executeLoad(
  plan: LoadPlan,
  pipeline: CarrierPipeline,
  ctx: DemandContext,
): Promise<HookOutput | undefined> {
  switch (plan.kind) {
    case "skip":
      return undefined;
    case "generated":
      return { code: EXPORT_HELPER_CODE, map: null };
    case "carrier-script": {
      const record = await pipeline.demand(plan.key, { kind: "disk" }, ctx);
      const main = record.main ?? notPublished(plan.request);
      return { code: main.code, map: main.map };
    }
    case "style": {
      const record = await pipeline.demand(plan.key, { kind: "disk" }, ctx);
      if (plan.lane === "bundler-css") {
        const authored = record.authoredStyles[plan.index] ?? notPublished(plan.request);
        return { code: authored.content, map: null };
      }
      const compiled = record.compiledStyles[plan.index] ?? notPublished(plan.request);
      return { code: compiled.code, map: compiled.map };
    }
    case "compiled-node": {
      const record = await pipeline.demand(plan.key, { kind: "disk" }, ctx);
      const node =
        record.nodes.find(
          (candidate) =>
            candidate.node.kind === plan.nodeKind && (candidate.node.index ?? 0) === plan.index,
        ) ?? notPublished(plan.request);
      return { code: node.code, map: node.sourceMap ?? null };
    }
  }
}

/** Run one planned transform. `skip` gives `undefined`. */
export async function executeTransform(
  plan: TransformPlan,
  code: string,
  pipeline: CarrierPipeline,
  binding: SessionBinding,
  ctx: DemandContext,
): Promise<HookOutput | undefined> {
  switch (plan.kind) {
    case "skip":
      return undefined;
    case "carrier-main": {
      const record = await pipeline.demand(plan.key, { kind: "bundler", code }, ctx);
      return emitMainModule(record, binding, pipeline.moduleIdOf(record.carrier));
    }
    case "style-scope": {
      // The bundler's CSS pipeline has already preprocessed the block; native
      // `transformVueStyle` adds the scoping and CSS v-bind() rewriting.
      const record = await pipeline.demand(plan.key, { kind: "disk" }, ctx);
      const authored = record.authoredStyles[plan.index] ?? notPublished(plan.request);
      return applyViteStyleLane(
        code,
        record.rawPath,
        componentIdOf(record),
        plan.scopedHint || authored.scoped,
      );
    }
    case "style-compiled": {
      let record: CarrierRecord;
      try {
        record = await pipeline.demand(plan.key, { kind: "disk" }, ctx);
      } catch (error) {
        // Style bytes of a carrier that does not exist are not this package's
        // to scope; they pass through as they came.
        if (error instanceof PipelineRefusalError && error.refusal.kind === "carrier-unavailable") {
          return { code, map: null };
        }
        throw error;
      }
      // Not on bytes this package's own compile already produced. load()
      // serves a carrier's style sub-request from the compiled styles,
      // preprocessed AND scoped, so a second pass over those same bytes would
      // double-scope the moment an id ever carries `&scoped`. Byte identity,
      // not id identity: a bundler passing authored bytes for the same id
      // still gets the scoping pass.
      const served = record.compiledStyles[plan.index];
      if (served && served.code === code) return { code: served.code, map: served.map };
      return {
        code: applyNonViteStyleLane(code, componentIdOf(record), plan.scopedHint),
        map: null,
      };
    }
  }
}

/** The id of one of a carrier's sub-modules, in the host's own spelling. */
export type ModuleIdOf = (node: HostVirtualNodeKind, lang?: string) => string;

/**
 * The module the bundler receives for a carrier's main request, in a fixed
 * order: script import, style imports in block order, the CSS-modules map and
 * helper import when any block is a module, the re-export, the default export.
 * Hosts with no sub-request support receive the compiled module itself.
 */
export function emitMainModule(
  record: CarrierRecord,
  binding: SessionBinding,
  moduleId: ModuleIdOf,
): HookOutput {
  const main = record.main;
  if (main === null) {
    throw new Error(`[verter] ${record.rawPath}: the carrier record has no main module`);
  }

  if (record.framework === "sveltejs") {
    const scriptRequest = moduleId({ kind: "script" }, main.lang ?? "js");
    const lines = [
      `import Component from ${JSON.stringify(scriptRequest)}`,
      ...record.compiledStyles.map(
        (style, index) =>
          `import ${JSON.stringify(moduleId({ kind: "style", index }, style.lang))}`,
      ),
      `export * from ${JSON.stringify(scriptRequest)}`,
      "export default Component",
    ];
    return { code: lines.join("\n"), map: null, lang: "js" };
  }

  // No sub-request support: the inline product IS the compiled module, so it
  // carries the map the host published for it.
  if (!binding.hostCssPipeline) return { code: main.code, map: main.map };

  // The sub-request form of `@vitejs/plugin-vue`: a thin wrapper that imports
  // the compiled script and each style as sub-requests, so the bundler's own
  // pipelines (TS, JSX, CSS preprocessors, CSS modules) process them. The
  // wrapper is not derived from the authored SFC, so it carries no map.
  const mainLang = main.lang ?? "ts";
  const scriptRequest = moduleId({ kind: "script" }, mainLang);
  const styleLines: string[] = [];
  const cssModules: Array<[string, string]> = [];
  record.authoredStyles.forEach((style, index) => {
    const lang = style.lang || "css";
    if (style.module) {
      // `lang.module.<ext>` makes the CSS pipeline default-export the class map.
      const request = moduleId({ kind: "style", index }, `module.${lang}`);
      const local = `style${index}`;
      styleLines.push(`import ${local} from ${JSON.stringify(request)}`);
      cssModules.push([typeof style.module === "string" ? style.module : "$style", local]);
    } else {
      const request = moduleId({ kind: "style", index }, lang);
      styleLines.push(`import ${JSON.stringify(request)}`);
    }
  });

  const lines = [`import _sfc_main from ${JSON.stringify(scriptRequest)}`, ...styleLines];
  if (cssModules.length > 0) {
    lines.push(`import _export_sfc from ${JSON.stringify(EXPORT_HELPER_ID)}`);
    const mapping = cssModules
      .map(([name, local]) => `  ${JSON.stringify(name)}: ${local}`)
      .join(",\n");
    lines.push(`const cssModules = {\n${mapping}\n}`);
    lines.push(`export * from ${JSON.stringify(scriptRequest)}`);
    lines.push(
      `export default /*@__PURE__*/_export_sfc(_sfc_main, [["__cssModules", cssModules]])`,
    );
  } else {
    lines.push(`export * from ${JSON.stringify(scriptRequest)}`);
    lines.push(`export default _sfc_main`);
  }
  return { code: lines.join("\n"), map: null, lang: mainLang };
}

function notPublished(request: string): never {
  throw new PipelineRefusalError({ kind: "module-not-published", request });
}

function componentIdOf(record: CarrierRecord): string {
  const componentId = record.profile.componentId;
  if (!componentId) {
    throw new Error(`[verter] ${record.rawPath}: the carrier record has no component id`);
  }
  return componentId;
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

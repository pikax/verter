import { createHash } from "crypto";
import type {
  VerterHost,
  Workspace,
  HostCompileRequest,
  HostCompileResponse,
  HostRuntimeCompiledProduct,
  HostCompileVirtualNode,
} from "@verter/native";

/** The live native style transform: scoping and CSS `v-bind()` rewriting. */
export { transformVueStyle } from "@verter/native";

export function getHash(text: string): string {
  return createHash("sha256").update(text).digest("hex").substring(0, 8);
}

export function generateComponentId(
  filename: string,
  source: string,
  isProd: boolean,
  root?: string,
): string {
  const normalized = filename.replace(/\\/g, "/");
  const normalizedRoot = root?.replace(/\\/g, "/").replace(/\/$/, "");
  // Compute relative path (matching @vitejs/plugin-vue):
  // path.relative(root, filename) with forward slashes
  const relativePath =
    normalizedRoot && normalized.startsWith(normalizedRoot + "/")
      ? normalized.slice(normalizedRoot.length + 1)
      : normalized;
  // Vue: dev = hash(path), prod = hash(path + source)
  return isProd ? getHash(relativePath + source) : getHash(relativePath);
}

/**
 * The bundler render demand in the plugin's own option vocabulary. This is
 * the stable internal shape every render decision is made from; it is
 * translated ONCE at the native call boundary into the typed
 * framework-discriminated request.
 */
export interface VerterRenderProfile {
  filename?: string;
  componentId?: string;
  isProduction: boolean;
  customElement: boolean;
  ssr: boolean;
  ssrModuleId?: string;
  forceJs: boolean;
  hmrStrategy: "none" | "vite" | "webpack";
  sourceMap: boolean;
}

export type RenderFramework = "vue" | "sveltejs";

/**
 * The one translation from the plugin render profile to the typed native
 * host request. Mirrors the session's profile→attempt conversion exactly:
 * `ssr` selects the runtime product kind, identity carries filename /
 * componentId / production / forceJs plus the SSR-manifest key and the
 * dev-server decoration flavour, and the Vue option axes the plugin can
 * state ride the framework arm.
 *
 * `authoredOnlyStyles` states the bundler-owned style cascade on the
 * runtime product: Vite's CSS pipeline preprocesses and scopes, so the
 * compiler publishes authored bytes only. Absent runs the compiler-owned
 * complete authored-to-published cascade.
 */
export function typedRenderRequest(
  profile: VerterRenderProfile,
  framework: RenderFramework,
  options: { authoredOnlyStyles?: boolean } = {},
): HostCompileRequest {
  const identity = {
    isProduction: profile.isProduction,
    forceJs: profile.forceJs,
    hmrStrategy: profile.hmrStrategy,
    ...(profile.filename !== undefined ? { filename: profile.filename } : {}),
    ...(profile.componentId !== undefined ? { componentId: profile.componentId } : {}),
    ...(profile.ssrModuleId !== undefined ? { ssrModuleId: profile.ssrModuleId } : {}),
  };
  const runtimeProduct = {
    kind: profile.ssr ? ("runtimeServer" as const) : ("runtimeClient" as const),
    runtimeSourceMap: profile.sourceMap,
    ...(options.authoredOnlyStyles ? { styleProcessing: "authored-only" as const } : {}),
  };
  if (framework === "sveltejs") {
    return { framework: "svelte", identity, products: [runtimeProduct], options: {} };
  }
  return {
    framework: "vue",
    identity,
    products: [runtimeProduct],
    options: {
      backend: "inferred",
      ssr: profile.ssr,
      isCustomElement: [],
      babelParserPlugins: [],
      scriptCustomElement: profile.customElement,
    },
  };
}

/** The runtime product row of a typed response, for the requested kind. */
export function runtimeProduct(
  response: HostCompileResponse,
  ssr: boolean,
): HostRuntimeCompiledProduct | undefined {
  const kind = ssr ? "runtimeServer" : "runtimeClient";
  const product = response.products.find((row) => row.kind === kind);
  return product && "nodes" in product ? product : undefined;
}

/**
 * The runtime `Main` node of a typed render response — the same module the
 * legacy render lane published, or null when the response carries no Main.
 */
export function runtimeMainNode(
  response: HostCompileResponse,
  ssr: boolean,
): HostCompileVirtualNode | undefined {
  return runtimeProduct(response, ssr)?.nodes.find((node) => node.node.kind === "main");
}

/**
 * Fail-closed Main reader: a completed typed response with no runtime Main
 * is a missing product, not an empty module.
 */
export function requireRuntimeMain(
  response: HostCompileResponse,
  ssr: boolean,
  canonicalId: string,
): HostCompileVirtualNode {
  const main = runtimeMainNode(response, ssr);
  if (!main) {
    throw new Error(
      `[verter] ${canonicalId}: typed compile request published no runtime Main node`,
    );
  }
  return main;
}

/**
 * Virtual-file query against a typed runtime product: `type=script` is the
 * compiled Main module; `type=style&index=N` is that style node.
 */
export function runtimeNodeMatching(
  response: HostCompileResponse,
  ssr: boolean,
  query: { type?: string; index?: number },
): HostCompileVirtualNode | undefined {
  const nodes = runtimeProduct(response, ssr)?.nodes ?? [];
  if (query.type === "style") {
    const index = query.index ?? 0;
    return nodes.find((node) => node.node.kind === "style" && node.node.index === index);
  }
  if (query.type === "script" || query.type === undefined || query.type === "main") {
    return nodes.find((node) => node.node.kind === "main");
  }
  return nodes.find((node) => node.node.kind === query.type);
}

export type BundlerWarn = (warning: { message: string; id?: string }) => void;

/**
 * Typed-lane diagnostic disposition matching the legacy batch lane:
 * error-severity (or `hasErrors`) fails closed; only warning-severity is
 * forwarded as a bundler warning; info is dropped.
 */
export function forwardTypedDiagnostics(
  canonicalId: string,
  response: HostCompileResponse,
  warn?: BundlerWarn,
): void {
  const snapshot = response.diagnostics;
  const diagnostics = snapshot?.diagnostics ?? [];
  const errors = diagnostics.filter((diagnostic) => diagnostic.severity === "error");
  if (errors.length > 0 || snapshot?.hasErrors) {
    const detail =
      errors.length > 0
        ? errors.map((diagnostic) => diagnostic.message).join("; ")
        : "typed compile reported errors";
    throw new Error(`[verter] ${canonicalId}: ${detail}`);
  }
  for (const diagnostic of diagnostics) {
    if (diagnostic.severity !== "warning") continue;
    const message = `[verter] ${diagnostic.code}: ${diagnostic.message}`;
    if (warn) {
      warn({ message, id: canonicalId });
    } else {
      console.warn(`${message} (${canonicalId})`);
    }
  }
}

/**
 * The compiled style artifacts of a typed render response, indexed by style
 * block index — the single-compile replacement for per-style virtual reads.
 */
export function runtimeStyleArtifacts(
  response: HostCompileResponse,
  ssr: boolean,
): Array<{ code: string; map?: string; lang: string }> {
  const nodes = runtimeProduct(response, ssr)?.nodes ?? [];
  const styles = nodes
    .filter((node) => node.node.kind === "style" && node.node.index != null)
    .sort((a, b) => (a.node.index as number) - (b.node.index as number));
  const artifacts: Array<{ code: string; map?: string; lang: string }> = [];
  for (const node of styles) {
    artifacts[node.node.index as number] = {
      code: node.code,
      map: node.sourceMap ?? undefined,
      lang: node.lang ?? "css",
    };
  }
  return artifacts;
}

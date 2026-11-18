import { createHash } from "node:crypto";
import {
  VerterHost as NativeHost,
  Workspace as NativeWorkspace,
  canonicalizeId,
} from "@verter/native";
import type {
  HostCarrierExtension,
  HostIdRejection,
  HostResolvedId,
  VerterHost,
  Workspace,
} from "@verter/native";
import { CLIENT_FRAMEWORKS } from "@verter/language-shared";
import type { VerterFrameworkLanguage, VerterPluginOptions } from "../types";
import { ModuleCatalog } from "./catalog";
import {
  DEFAULT_COMPILE_INTENT,
  GENERATED_MODULE_IDS,
  adapterKindFrom,
  graphKind,
  mintLocalProjectIdentity,
  mintLocalSourceModuleEnvironment,
  mintModuleId,
  mintSessionId,
} from "./identity";
import type {
  BundlerAdapterKind,
  BundlerCompileIntent,
  BundlerGraphKind,
  BundlerGraphMode,
  BundlerGraphTarget,
  BundlerHostSessionId,
  BundlerModuleId,
  BundlerModuleRole,
  CarrierFramework,
  GeneratedModuleName,
  HookInvocationId,
  HostCanonicalId,
  ProjectIdentity,
  SourceModuleEnvironmentIdentity,
} from "./identity";

/** What the adapter states once, when the session is bound. */
export interface SessionBindingInput {
  /** unplugin's `meta.framework`. */
  readonly adapter: string;
  readonly adapterVersion?: string | null;
  /** The host's root. `null`: `process.cwd()` is captured at bind and recorded as such. */
  readonly root: string | null;
  readonly mode: BundlerGraphMode;
  /** The target used when a hook states none. */
  readonly defaultTarget: BundlerGraphTarget;
  readonly frameworkSelection: VerterFrameworkLanguage;
  /** Digested for identity; read only through the fields of `SessionBinding`. */
  readonly options: VerterPluginOptions;
  /**
   * Carrier suffixes beyond the registry's, registered with the host when it is
   * created and added to `options.carriers` (this wins on the same suffix).
   */
  readonly carriers?: Readonly<Record<string, CarrierFramework>> | null;
  /** The host has a CSS pipeline that preprocesses style sub-requests (Vite, Rolldown-Vite). */
  readonly hostCssPipeline: boolean;
  /** The host strips TypeScript from script sub-requests itself (Vite only). */
  readonly hostStripsTypeScript: boolean;
  /** The opaque host configuration handed to `preprocessCSS`; `null` outside Vite. */
  readonly hostConfig: unknown | null;
  /** The project follows Nuxt's `*.server.vue` / `*.client.vue` naming. */
  readonly nuxtFileRoles: boolean;
  readonly compileIntent?: BundlerCompileIntent | null;
  readonly project?: ProjectIdentity | null;
  readonly sourceModuleEnvironment?: SourceModuleEnvironmentIdentity | null;
  readonly workspace?: Workspace | null;
}

/** The frozen result of binding. */
export interface SessionBinding {
  readonly adapter: BundlerAdapterKind;
  readonly adapterVersion: string | null;
  readonly root: string;
  readonly rootSource: "host" | "process-cwd-at-bind";
  readonly mode: BundlerGraphMode;
  readonly defaultTarget: BundlerGraphTarget;
  readonly frameworkSelection: VerterFrameworkLanguage;
  readonly options: VerterPluginOptions;
  /** The extra carrier suffixes the host is created with, dotted. */
  readonly carriers: readonly RegisteredCarrier[];
  readonly optionsDigest: string;
  readonly hostCssPipeline: boolean;
  readonly hostStripsTypeScript: boolean;
  readonly hostConfig: unknown | null;
  readonly nuxtFileRoles: boolean;
  readonly hmrStrategy: "none" | "vite" | "webpack";
  readonly compileIntent: BundlerCompileIntent;
  readonly project: ProjectIdentity;
  readonly sourceModuleEnvironment: SourceModuleEnvironmentIdentity;
}

/** One carrier suffix this session registers with its host. */
export interface RegisteredCarrier {
  /** With its leading dot. */
  readonly suffix: string;
  readonly framework: CarrierFramework;
}

/** What a hook call knows about the graph it runs in. */
export interface HookGraphContext {
  /** Vite's per-environment plugin context, when the host has one. */
  readonly environment?: {
    readonly name: string;
    readonly config?: { readonly consumer?: "client" | "server" };
  } | null;
  /** A per-call SSR flag, when the host passes one. */
  readonly ssrFlag?: boolean | null;
}

/** The host's typed rejection reason for a request id, or `marker-framework-mismatch`. */
export type ModuleRejection = HostIdRejection | "marker-framework-mismatch";

export type ModuleIdentification =
  | {
      readonly kind: "carrier-module";
      /** The id as the bundler gave it. */
      readonly raw: string;
      readonly request: HostResolvedId;
      readonly id: BundlerModuleId;
      readonly framework: CarrierFramework;
      readonly graph: BundlerGraphKind;
    }
  | { readonly kind: "generated"; readonly name: GeneratedModuleName }
  /** Not a module of this session: the path is outside the include filter. */
  | { readonly kind: "foreign" }
  | { readonly kind: "rejected"; readonly raw: string; readonly reason: ModuleRejection };

export type SessionState = "open" | "closed";

export interface SessionInvalidation {
  /** Carriers whose records were retired. */
  readonly retired: readonly string[];
  /** `true` when the path was a registered carrier and was removed from the native host. */
  readonly removedFromHost: boolean;
}

let sessionInstances = 0;

/**
 * One admitted project/tool graph: the binding, the module catalog, the native
 * host handle and the lifecycle. Nothing here is process-global.
 */
export class BundlerHostSession {
  /**
   * Bind a session. Throws for an adapter outside `BundlerAdapterKind`.
   * Mints local project and environment identities when none are supplied.
   */
  static bind(input: SessionBindingInput): BundlerHostSession {
    const adapter = adapterKindFrom(input.adapter);
    const root = input.root ?? process.cwd();
    const adapterVersion = input.adapterVersion ?? null;
    const compileIntent = input.compileIntent ?? DEFAULT_COMPILE_INTENT;
    const project = input.project ?? mintLocalProjectIdentity(root);
    const sourceModuleEnvironment =
      input.sourceModuleEnvironment ??
      mintLocalSourceModuleEnvironment({ adapter, adapterVersion, mode: input.mode });
    const carriers = registeredCarriers({ ...input.options.carriers, ...input.carriers });
    const optionsDigest = digestOptions({
      ...input.options,
      carriers: carriers.map((carrier) => [carrier.suffix, carrier.framework]),
    });
    const binding: SessionBinding = Object.freeze({
      adapter,
      adapterVersion,
      root,
      rootSource: input.root === null || input.root === undefined ? "process-cwd-at-bind" : "host",
      mode: input.mode,
      defaultTarget: input.defaultTarget,
      frameworkSelection: input.frameworkSelection,
      options: input.options,
      carriers,
      optionsDigest,
      hostCssPipeline: input.hostCssPipeline,
      hostStripsTypeScript: input.hostStripsTypeScript,
      hostConfig: input.hostConfig,
      nuxtFileRoles: input.nuxtFileRoles,
      hmrStrategy: hmrStrategyFor(adapter),
      compileIntent,
      project,
      sourceModuleEnvironment,
    } as const);
    sessionInstances += 1;
    const id = mintSessionId({
      adapter,
      adapterVersion,
      root,
      mode: input.mode,
      frameworkSelection: input.frameworkSelection,
      optionsDigest,
      compileIntent,
      project,
      sourceModuleEnvironment,
      instance: sessionInstances,
    });
    return new BundlerHostSession(id, binding, input.workspace ?? null);
  }

  private readonly catalogStore: ModuleCatalog;
  private readonly includes: (rawPath: string, framework: CarrierFramework) => boolean;
  private nativeHost: VerterHost | null = null;
  private currentState: SessionState = "open";
  private currentEpoch = 0;
  private invocations = 0;
  private workspaceHandle: Workspace | null = null;

  private constructor(
    private readonly sessionId: BundlerHostSessionId,
    private readonly sessionBinding: SessionBinding,
    private readonly boundWorkspace: Workspace | null,
  ) {
    this.catalogStore = new ModuleCatalog((record, role) =>
      mintModuleId(this.sessionId, record.graph, record.framework, role, record.carrier),
    );
    this.includes = createIncludeFilter(sessionBinding.options, sessionBinding.frameworkSelection);
  }

  get id(): BundlerHostSessionId {
    return this.sessionId;
  }

  get binding(): SessionBinding {
    return this.sessionBinding;
  }

  get catalog(): ModuleCatalog {
    return this.catalogStore;
  }

  get state(): SessionState {
    return this.currentState;
  }

  /** Starts at 0; `restart()` adds 1. Work begun under an older epoch publishes nothing. */
  get epoch(): number {
    return this.currentEpoch;
  }

  /**
   * The session's own native host, created on first use; the only place this
   * package constructs one. It always has a workspace: the bound one, else one
   * opened at the binding's root. Throws when the session is closed.
   */
  host(): VerterHost {
    if (this.currentState === "closed") {
      throw new Error("[verter] bundler host session is closed");
    }
    if (this.nativeHost) return this.nativeHost;
    this.workspaceHandle ??= this.boundWorkspace ?? new NativeWorkspace([this.sessionBinding.root]);
    this.nativeHost = NativeHost.withWorkspace(
      { devMode: true, carrierExtensions: this.sessionBinding.carriers.map(hostCarrierExtension) },
      this.workspaceHandle,
    );
    return this.nativeHost;
  }

  /** The already-created host, or `null`. Never creates one. */
  peekHost(): VerterHost | null {
    return this.nativeHost;
  }

  /** The workspace the host reads through: the bound one, or the one opened at the root. */
  workspace(): Workspace {
    this.host();
    return this.workspaceHandle as Workspace;
  }

  /**
   * The graph of one hook call. Target, in order: the hook's environment
   * consumer; the per-call SSR flag; the bound default. Mode: the binding's.
   */
  graphFor(hook: HookGraphContext): BundlerGraphKind {
    const consumer = hook.environment?.config?.consumer;
    let target: BundlerGraphTarget;
    if (consumer !== undefined) {
      target = consumer === "server" ? "ssr" : "client";
    } else if (hook.ssrFlag !== null && hook.ssrFlag !== undefined) {
      target = hook.ssrFlag ? "ssr" : "client";
    } else {
      target = this.sessionBinding.defaultTarget;
    }
    return graphKind(target, this.sessionBinding.mode);
  }

  /**
   * `true` when the host reads the path (no query) as a carrier, a registry
   * carrier or one registered by this session, and the bound include filter admits it.
   */
  includesCarrier(rawPath: string): boolean {
    if (!this.host().isCarrierSourcePath(rawPath)) return false;
    const framework = carrierFrameworkOf(rawPath, this.sessionBinding.carriers);
    return framework !== null && this.includes(rawPath, framework);
  }

  /** The framework of an included carrier: the bound selection, else the registration or registry row whose suffix matches the path. */
  frameworkFor(rawPath: string): CarrierFramework {
    const selection = this.sessionBinding.frameworkSelection;
    if (selection !== "auto") return selection;
    const framework = carrierFrameworkOf(rawPath, this.sessionBinding.carriers);
    if (framework === null) {
      throw new Error(`[verter] '${rawPath}' is not a carrier source path`);
    }
    return framework;
  }

  /** Admit one request id: the host parses it; this only checks inclusion and the marker's framework. */
  identify(rawId: string, graph: BundlerGraphKind): ModuleIdentification {
    for (const [name, fixedId] of Object.entries(GENERATED_MODULE_IDS)) {
      if (rawId === fixedId) return { kind: "generated", name: name as GeneratedModuleName };
    }
    const request = this.host().resolve(rawId);
    if ("rejected" in request) {
      // A virtual-file suffix the grammar does not know is not this package's id.
      if (request.rejected === "unknown-virtual-suffix") return { kind: "foreign" };
      return { kind: "rejected", raw: rawId, reason: request.rejected };
    }
    if (!this.includesCarrier(request.rawPath)) return { kind: "foreign" };
    const framework = this.frameworkFor(request.rawPath);
    if (
      (request.marker === "vue" && framework === "sveltejs") ||
      (request.marker === "verter" && framework === "vue")
    ) {
      return { kind: "rejected", raw: rawId, reason: "marker-framework-mismatch" };
    }
    return {
      kind: "carrier-module",
      raw: rawId,
      request,
      id: mintModuleId(
        this.sessionId,
        graph,
        framework,
        roleOf(request),
        request.canonicalId as HostCanonicalId,
      ),
      framework,
      graph,
    };
  }

  nextInvocation(): HookInvocationId {
    this.invocations += 1;
    return this.invocations as HookInvocationId;
  }

  /**
   * A file changed or was removed. Retires the carrier's records and the
   * records of carriers whose closure contains it; removes a carrier from the
   * native host. Never creates a host.
   */
  invalidate(rawPath: string): SessionInvalidation {
    const host = this.nativeHost;
    const parsed = host?.resolve(rawPath);
    const resolved = parsed && !("rejected" in parsed) ? parsed : null;
    // Without a host nothing was upserted: the catalog key is the host's spelling all the same.
    const carrier = (resolved?.canonicalId ?? canonicalizeId(rawPath)) as HostCanonicalId;
    const retired = new Set<string>();
    if (this.catalogStore.retire(carrier) > 0) retired.add(carrier);
    if (resolved) {
      for (const dependent of this.catalogStore.retireDependents(carrier)) retired.add(dependent);
    }
    const removedFromHost =
      host !== null && this.includesCarrier(rawPath) && host.remove(carrier) != null;
    return { retired: [...retired], removedFromHost };
  }

  /** Configuration changed: epoch + 1, catalog cleared, host closed. The session stays open. */
  restart(): void {
    if (this.currentState === "closed") return;
    this.currentEpoch += 1;
    this.catalogStore.clear();
    this.closeHost();
  }

  /** Clear the catalog and close the native host (once). Idempotent. */
  close(): void {
    if (this.currentState === "closed") return;
    this.catalogStore.clear();
    this.closeHost();
    this.currentState = "closed";
  }

  private closeHost(): void {
    const host = this.nativeHost;
    this.nativeHost = null;
    host?.close();
  }
}

function hmrStrategyFor(adapter: BundlerAdapterKind): SessionBinding["hmrStrategy"] {
  switch (adapter) {
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

/**
 * The carrier framework of a path: the session's registrations first (the
 * longest suffix wins), then the registry manifest row whose carrier
 * extensions match. `null` for a path none claims.
 */
function carrierFrameworkOf(
  rawPath: string,
  registered: readonly RegisteredCarrier[],
): CarrierFramework | null {
  let best: RegisteredCarrier | null = null;
  for (const candidate of registered) {
    if (
      rawPath.endsWith(candidate.suffix) &&
      (best === null || candidate.suffix.length > best.suffix.length)
    ) {
      best = candidate;
    }
  }
  if (best) return best.framework;
  const row = CLIENT_FRAMEWORKS.find((manifestRow) =>
    manifestRow.carrierExtensions.some((extension) => rawPath.endsWith(extension)),
  );
  return row ? (CARRIER_FRAMEWORK_OF_ROW[row.frameworkId] ?? null) : null;
}

/** The plugin's framework name for each manifest row: the one place the option vocabulary meets the registry's. */
const CARRIER_FRAMEWORK_OF_ROW: Readonly<Record<string, CarrierFramework>> = {
  vue: "vue",
  svelte: "sveltejs",
};

function hostCarrierExtension(carrier: RegisteredCarrier): HostCarrierExtension {
  const row = Object.entries(CARRIER_FRAMEWORK_OF_ROW).find(
    ([, name]) => name === carrier.framework,
  );
  return { suffix: carrier.suffix, framework: row ? row[0] : carrier.framework };
}

function registeredCarriers(
  carriers: Readonly<Record<string, CarrierFramework>> | undefined,
): RegisteredCarrier[] {
  return Object.entries(carriers ?? {}).map(([suffix, framework]) => ({
    suffix: suffix.startsWith(".") ? suffix : `.${suffix}`,
    framework,
  }));
}

/** The module role a host-parsed request names. */
function roleOf(request: HostResolvedId): BundlerModuleRole {
  const { kind, index } = request.nodeKind;
  switch (kind) {
    case "style":
      return { kind, index: index ?? 0 };
    case "custom":
      return { kind, index: index ?? 0, blockType: request.blockType ?? "" };
    default:
      return { kind };
  }
}

function createIncludeFilter(
  options: Pick<VerterPluginOptions, "include" | "exclude">,
  selection: VerterFrameworkLanguage,
): (rawPath: string, framework: CarrierFramework) => boolean {
  const { include, exclude } = options;
  const excluded = exclude ? matchesAny(exclude) : () => false;
  if (!include) {
    return (rawPath, framework) =>
      (selection === "auto" || selection === framework) && !excluded(rawPath);
  }
  const included = matchesAny(include);
  return (rawPath) => included(rawPath) && !excluded(rawPath);
}

function matchesAny(
  patterns: NonNullable<VerterPluginOptions["include"]>,
): (value: string) => boolean {
  const list = Array.isArray(patterns) ? patterns : [patterns];
  return (value) =>
    list.some((pattern) =>
      typeof pattern === "string" ? value.endsWith(pattern) : testPattern(pattern, value),
    );
}

/**
 * The carrier suffixes a framework entry's `include` registers: its `include`
 * means "these files are `framework`". A suffix comes from a string pattern
 * (`.md`, matched as a path suffix like the filter) or from a regular expression that is a literal dotted
 * suffix anchored at the end (`/\.md$/`). A pattern of another shape only
 * narrows. A suffix the registry already owns is not registered again.
 */
export function carriersFromInclude(
  include: VerterPluginOptions["include"],
  framework: CarrierFramework,
): Record<string, CarrierFramework> {
  const carriers: Record<string, CarrierFramework> = {};
  if (!include) return carriers;
  for (const pattern of Array.isArray(include) ? include : [include]) {
    for (const suffix of suffixesOfPattern(pattern)) {
      if (!isRegistrySuffix(suffix)) carriers[suffix] = framework;
    }
  }
  return carriers;
}

/** The dotted suffixes a pattern names: `.md`, a regular expression `\.md$` or `\.(vue|md)$`. Other shapes name none. */
function suffixesOfPattern(pattern: string | RegExp): string[] {
  if (typeof pattern === "string") {
    const literal = /^(\.[A-Za-z0-9]+(?:\.[A-Za-z0-9]+)*)$/.exec(pattern);
    return literal ? [literal[1]] : [];
  }
  const anchored =
    /^\\\.(?:\(([A-Za-z0-9]+(?:\|[A-Za-z0-9]+)*)\)|([A-Za-z0-9]+(?:\\\.[A-Za-z0-9]+)*))\$$/.exec(
      pattern.source,
    );
  if (!anchored) return [];
  if (anchored[1] !== undefined) return anchored[1].split("|").map((name) => "." + name);
  return ["." + anchored[2].replace(/\\\./g, ".")];
}

function isRegistrySuffix(suffix: string): boolean {
  return CLIENT_FRAMEWORKS.some((row) => row.carrierExtensions.includes(suffix));
}

/** A global or sticky RegExp keeps `lastIndex` between calls; a filter must not. */
function testPattern(pattern: RegExp, value: string): boolean {
  pattern.lastIndex = 0;
  return pattern.test(value);
}

/** sha256 over a stable text of the options: keys sorted, functions and RegExps by description. */
function digestOptions(options: unknown): string {
  return createHash("sha256").update(stableText(options, [])).digest("hex");
}

function stableText(value: unknown, ancestors: readonly object[]): string {
  if (typeof value === "function") return JSON.stringify("[function]");
  if (value === null || typeof value === "string" || typeof value === "boolean") {
    return JSON.stringify(value);
  }
  if (typeof value === "number")
    return JSON.stringify(Number.isFinite(value) ? value : String(value));
  if (value instanceof RegExp) return JSON.stringify({ regexp: [value.source, value.flags] });
  if (typeof value !== "object") {
    throw new Error(`[verter] cannot digest a plugin option of type ${typeof value}`);
  }
  if (ancestors.includes(value)) throw new Error("[verter] cannot digest a circular plugin option");
  if (value instanceof Map || value instanceof Set || value instanceof Date) {
    throw new Error(`[verter] cannot digest a plugin option of type ${value.constructor.name}`);
  }
  const path = [...ancestors, value];
  if (Array.isArray(value)) {
    return `[${value.map((item) => (item === undefined ? "null" : stableText(item, path))).join(",")}]`;
  }
  const record = value as Record<string, unknown>;
  const parts = Object.keys(record)
    .filter((key) => record[key] !== undefined)
    .sort()
    .map((key) => `${JSON.stringify(key)}:${stableText(record[key], path)}`);
  return `{${parts.join(",")}}`;
}

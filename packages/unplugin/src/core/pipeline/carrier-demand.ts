import { relative } from "node:path";
import { canonicalizeId } from "@verter/native";
import type {
  HostCompileResponse,
  HostDependencyResolution,
  HostMissingInput,
  HostUpdateResult,
  VerterHost,
  Workspace,
} from "@verter/native";
import {
  forwardTypedDiagnostics,
  generateComponentId,
  typedRenderRequest,
  type BundlerWarn,
  type VerterRenderProfile,
} from "../compiler";
import {
  resolveThroughBundler,
  type BundlerResolution,
  type ResolveHook,
} from "../bundler-resolve";
import { collectResolvableModuleReferenceSpecifiers } from "../dependency-resolution";
import type { AuthoredStyleBlock, CarrierRecord, PublishedMain } from "../host-session/catalog";
import {
  BUNDLER_GRAPH_KINDS,
  encodeFramed,
  graphMode,
  graphTarget,
} from "../host-session/identity";
import type {
  BundlerGraphKind,
  BundlerSourceRevision,
  CarrierFramework,
  HookInvocationId,
  HostCanonicalId,
} from "../host-session/identity";
import type { BundlerHostSession, SessionBinding } from "../host-session/session";
import { replaceImportMetaSsr, stripComponents, type SsrRewriteResult } from "../ssr-transforms";
import {
  admitCompile,
  fillableInputs,
  missingInputKey,
  PipelineRefusalError,
  refusalFromCompileError,
} from "./admission";
import { typeTargetOf } from "../macro-type-hydration";
import { createPipelineCounters, type PipelineCounters } from "./counters";
import { applyProcessedContent } from "./processed-content";
import { styleLaneFor, type ModuleIdOf } from "./plan";
import { DependencyRouteTable } from "./route-table";
import { TypeDependencyHydrator, typeFileAccess, type TypeFileAccess } from "./type-hydration";

export interface CarrierDemandKey {
  readonly carrier: HostCanonicalId;
  /** The path spelling of the request that raised the demand. */
  readonly rawPath: string;
  readonly graph: BundlerGraphKind;
}

/** `bundler`: the bytes a transform hook received. `disk`: read the carrier; used when a sub-request arrives first. */
export type CarrierSource =
  | { readonly kind: "bundler"; readonly code: string }
  | { readonly kind: "disk" };

/** What a hook lends the pipeline for one call. */
export interface DemandContext {
  readonly invocation: HookInvocationId;
  readonly resolve: ResolveHook | null;
  readonly warn: BundlerWarn | null;
  readonly addWatchFile: ((path: string) => void) | null;
}

/** Nuxt's `<name>.server.<ext>` / `<name>.client.<ext>` component naming. */
const NUXT_SERVER_FILE = /\.server\.[^./]+$/;
const NUXT_CLIENT_FILE = /\.client\.[^./]+$/;

/**
 * The render profile of one carrier in one graph. Pure.
 * `isProduction` follows the graph's mode and `ssr` the graph's target,
 * independently; the component id does not depend on the target.
 * `*.server.vue` / `*.client.vue` override `ssr` when the binding says the
 * project follows that naming.
 */
export function profileFor(
  binding: SessionBinding,
  key: CarrierDemandKey,
  source: string,
  framework: CarrierFramework,
): VerterRenderProfile {
  const filename = key.rawPath;
  const target = graphTarget(key.graph);
  const isProduction = graphMode(key.graph) === "prod";
  const root = binding.rootSource === "host" ? binding.root : undefined;
  const componentIdFn = binding.options.componentId || generateComponentId;
  // From the canonical path, not the request's spelling: whichever request
  // raised the demand, the carrier gets one component id (and scope id).
  const componentId = componentIdFn(
    key.carrier,
    source,
    isProduction,
    root === undefined ? undefined : canonicalizeId(root),
  );

  if (framework === "sveltejs") {
    return {
      filename,
      customElement: false,
      ssr: target === "ssr",
      isProduction,
      componentId,
      // Native Svelte HMR codegen is intentionally fail-closed; a bundler
      // file change uses the plugin's full-reload path instead.
      hmrStrategy: "none",
      sourceMap: true,
      forceJs: true,
    };
  }

  let ssr = target === "ssr";
  // Nuxt server/client component convention:
  // *.server.vue → always SSR, *.client.vue → never SSR
  if (binding.nuxtFileRoles) {
    if (NUXT_SERVER_FILE.test(filename)) ssr = true;
    if (NUXT_CLIENT_FILE.test(filename)) ssr = false;
  }
  return {
    filename,
    customElement: false,
    ssr,
    ssrModuleId: ssrModuleIdFor(ssr, root ?? "", filename),
    isProduction,
    componentId,
    hmrStrategy: isProduction ? "none" : binding.hmrStrategy,
    sourceMap: true,
    // Only Vite itself strips TS via vite:esbuild on script sub-requests.
    // Rolldown/tsdown sets a Vite config but lacks vite:esbuild, so Verter
    // must strip TS. Other bundlers (webpack, rspack) also need stripping.
    forceJs: !binding.hostStripsTypeScript,
  };
}

/** One running compile of one key. `revision` is `null` while a disk read is pending. */
interface Flight {
  revision: BundlerSourceRevision | null;
  promise: Promise<CarrierRecord>;
}

/** The objects tied to one native host instance; replaced when the session restarts its host. */
interface HostState {
  readonly host: VerterHost;
  readonly routes: DependencyRouteTable;
  readonly hydrator: TypeDependencyHydrator;
  readonly files: TypeFileAccess;
}

/** A flight that lost its key to a newer one resolves with the newer outcome. */
class FlightReplaced {
  constructor(readonly next: Promise<CarrierRecord>) {}
}

/**
 * The only route by which a carrier is compiled: register, resolve, hydrate,
 * hand over processed content, compile once, publish once.
 */
export class CarrierPipeline {
  private readonly flights = new Map<string, Flight>();
  private readonly workCounters: PipelineCounters = createPipelineCounters();
  private hostState: HostState | null = null;
  private lastInvocation = 0;

  constructor(private readonly session: BundlerHostSession) {}

  /**
   * The published record for `key`, computing it when needed.
   *
   * - `bundler` source: a record of the same revision and epoch is returned
   *   without work; a flight of the same revision is joined; otherwise a new
   *   flight starts and supersedes any older one.
   * - `disk` source: any record of the current epoch is returned; any flight
   *   is joined; otherwise the carrier is read and a flight starts.
   *
   * A compile that fails for missing inputs rejects with a `need-inputs`
   * refusal.
   *
   * Nothing is published before the compile has been admitted under the
   * session's current epoch. Rejects with `PipelineRefusalError` or the
   * compile's own error; every joiner of a flight sees the same outcome.
   */
  demand(key: CarrierDemandKey, source: CarrierSource, ctx: DemandContext): Promise<CarrierRecord> {
    if (ctx.invocation > this.lastInvocation) {
      this.lastInvocation = ctx.invocation;
      this.workCounters.hookInvocations += 1;
    }
    const k = flightKey(key);
    const record = this.session.catalog.record(key.carrier, key.graph);
    const flight = this.flights.get(k);
    const epoch = this.session.epoch;

    if (source.kind === "bundler") {
      // The host's answer to the registration names the revision. The upsert is
      // synchronous, so it precedes every await of this demand.
      const upsert = this.currentHostState().host.upsert({
        inputId: key.rawPath,
        source: source.code,
      });
      this.workCounters.registrations += 1;
      const revision = upsert.contentHash as BundlerSourceRevision;
      if (record && record.revision === revision && record.epoch === epoch) {
        return Promise.resolve(record);
      }
      if (flight && flight.revision === revision) return flight.promise;
      if (flight) this.workCounters.supersededFlights += 1;
      return this.start(key, k, revision, source, ctx, upsert);
    }

    // Whatever the bundler last gave wins over the disk.
    if (record && record.epoch === epoch) return Promise.resolve(record);
    if (flight) return flight.promise;
    return this.start(key, k, null, source, ctx, null);
  }

  /** `true` while a flight for `key` is running. */
  inFlight(key: CarrierDemandKey): boolean {
    return this.flights.has(flightKey(key));
  }

  /** A file changed or was removed: forwards to the session and the hydrator, and drops the key's flight. */
  invalidate(rawPath: string): void {
    const state = this.hostState;
    // The hydrator asks the host for its spelling of the path, so it goes
    // before the session removes the carrier from the host. Never creates a host.
    if (state && state.host === this.session.peekHost()) state.hydrator.invalidate(rawPath);
    this.session.invalidate(rawPath);
    const carrier = canonicalizeId(rawPath);
    for (const graph of BUNDLER_GRAPH_KINDS) this.flights.delete(encodeFramed([carrier, graph]));
  }

  /** Writes the ids of a carrier's sub-modules in the host's own spelling. */
  moduleIdOf(carrier: HostCanonicalId): ModuleIdOf {
    const { host } = this.currentHostState();
    return (node, lang) => host.renderBundlerId(carrier, node, lang);
  }

  counters(): Readonly<PipelineCounters> {
    return this.workCounters;
  }

  /** Drop flights, hydration state and routes. Called before the session closes. */
  dispose(): void {
    this.flights.clear();
    this.hostState?.hydrator.clear();
    this.hostState?.routes.clear();
    this.hostState = null;
  }

  /** Register the flight under its key before any work, so every later demand finds it. */
  private start(
    key: CarrierDemandKey,
    k: string,
    revision: BundlerSourceRevision | null,
    source: CarrierSource,
    ctx: DemandContext,
    registered: HostUpdateResult | null,
  ): Promise<CarrierRecord> {
    const flight = { revision } as Flight;
    this.flights.set(k, flight);
    flight.promise = this.fly(key, k, flight, source, ctx, registered);
    return flight.promise;
  }

  private async fly(
    key: CarrierDemandKey,
    k: string,
    flight: Flight,
    source: CarrierSource,
    ctx: DemandContext,
    registered: HostUpdateResult | null,
  ): Promise<CarrierRecord> {
    const epoch = this.session.epoch;
    try {
      let code: string;
      let upsert: HostUpdateResult;
      if (source.kind === "bundler" && registered) {
        code = source.code;
        upsert = registered;
      } else {
        const read = await readText(this.session.workspace(), key.rawPath);
        this.ensureCurrent(key, k, flight, epoch);
        if (read === null) {
          throw new PipelineRefusalError({ kind: "carrier-unavailable", carrier: key.rawPath });
        }
        code = read;
        upsert = this.currentHostState().host.upsert({ inputId: key.rawPath, source: code });
        this.workCounters.registrations += 1;
        flight.revision = upsert.contentHash as BundlerSourceRevision;
      }
      const record = await this.compile(key, k, flight, code, upsert, epoch, ctx);
      this.ensureCurrent(key, k, flight, epoch);
      this.session.catalog.publish(record);
      return record;
    } catch (error) {
      // A newer flight's outcome wins over this flight's own stale refusal:
      // the refusal is the host noticing the supersession.
      let outcome: FlightReplaced | PipelineRefusalError | unknown;
      if (error instanceof FlightReplaced) {
        outcome = error;
      } else {
        const overtaken = this.overtaken(key, k, flight, epoch);
        outcome =
          isStaleRefusal(error) && !(overtaken instanceof FlightReplaced) ? error : overtaken;
      }
      if (outcome instanceof FlightReplaced) return outcome.next;
      if (outcome) {
        if (this.session.epoch !== epoch) this.workCounters.staleDiscards += 1;
        throw outcome;
      }
      throw error;
    } finally {
      if (this.flights.get(k) === flight) this.flights.delete(k);
    }
  }

  /** Every step from registration to the admitted record. Publishes nothing. */
  private async compile(
    key: CarrierDemandKey,
    k: string,
    flight: Flight,
    code: string,
    registered: HostUpdateResult,
    epoch: number,
    ctx: DemandContext,
  ): Promise<CarrierRecord> {
    const revision = registered.contentHash as BundlerSourceRevision;
    const binding = this.session.binding;
    const counters = this.workCounters;
    const { host, routes, hydrator, files } = this.currentHostState();
    const framework = this.session.frameworkFor(key.rawPath);
    const lane = styleLaneFor(binding, framework);
    const profile = profileFor(binding, key, code, framework);

    let upsert = registered;
    const owner = upsert.canonicalId as HostCanonicalId;
    hydrator.noteCarrier(owner, revision);

    // Authored inline styles from the registered inventory, for the bundler's
    // CSS pipeline. External content is unavailable on this structure-only surface.
    const authoredStyles =
      lane === "bundler-css" ? styleEntriesFromStructure(host, upsert.canonicalId, code) : [];

    upsert = await resolveModuleReferences(
      host,
      this.session.workspace(),
      key.rawPath,
      code,
      upsert,
      ctx.resolve,
      routes,
      owner,
      counters,
      () => this.overtaken(key, k, flight, epoch) === null,
    );
    this.ensureCurrent(key, k, flight, epoch);

    let closure: ReadonlySet<HostCanonicalId> = new Set();
    if (framework === "vue") {
      closure = await hydrator.hydrateCarrier(owner, ctx.resolve);
      this.ensureCurrent(key, k, flight, epoch);
    }

    // Preprocess non-native blocks (Pug, CoffeeScript, custom; style skipped
    // when the bundler has a CSS pipeline).
    const processed = await applyProcessedContent(
      host,
      key.rawPath,
      upsert,
      binding,
      ctx,
      counters,
    );
    // A file an external stage read is part of what the record was built from,
    // so a change to it retires the record like a change to a hydrated type file.
    closure = new Set([
      ...closure,
      // The files of external `src` blocks are read into the compile too.
      ...upsert.externalSourceRequests.map(
        (request) => request.resolvedCanonicalId as HostCanonicalId,
      ),
      ...processed.flatMap((envelope) =>
        envelope.dependencies.map((dependency) => canonicalizeId(dependency) as HostCanonicalId),
      ),
    ]);
    this.ensureCurrent(key, k, flight, epoch);

    // The compile may name inputs it needs: supply what can be supplied and
    // compile again, until it succeeds or asks for nothing new.
    const supplied = new Set<string>();
    const hydrated = new Set<HostCanonicalId>();
    let response: HostCompileResponse;
    for (;;) {
      counters.compileRequests += 1;
      try {
        response = executeTypedCompile(
          host,
          key.rawPath,
          profile,
          framework,
          lane === "bundler-css",
          ctx.warn ?? undefined,
        );
        break;
      } catch (error) {
        if (!(error instanceof PipelineRefusalError) || error.refusal.kind !== "need-inputs") {
          throw error;
        }
        const fillable = fillableInputs(error.refusal.inputs, supplied);
        if (fillable === "terminal") throw compileFailure(key.rawPath, error.cause);
        for (const input of fillable) supplied.add(missingInputKey(input));
        const supply = await this.supplyInputs(fillable, hydrator, routes, files, ctx.resolve);
        if (supply === null) throw compileFailure(key.rawPath, error.cause);
        for (const id of supply) hydrated.add(id);
        this.ensureCurrent(key, k, flight, epoch);
        counters.inputRounds += 1;
      }
    }
    if (hydrated.size > 0) closure = new Set([...closure, ...hydrated]);
    const admitted = admitCompile(response, profile.ssr);
    const main =
      framework === "vue" ? this.rewriteForTarget(admitted.main, profile.ssr) : admitted.main;

    return {
      carrier: key.carrier,
      rawPath: key.rawPath,
      graph: key.graph,
      framework,
      revision,
      epoch,
      profile,
      main,
      authoredStyles,
      compiledStyles: admitted.styles,
      nodes: admitted.nodes,
      closure,
    };
  }

  /**
   * Supply the inputs one compile round named: hydrate a named file, and
   * resolve, hydrate and route a named route under its importer. Returns the
   * host ids of every file hydrated, or `null` when an input cannot be supplied.
   */
  private async supplyInputs(
    inputs: readonly HostMissingInput[],
    hydrator: TypeDependencyHydrator,
    routes: DependencyRouteTable,
    files: TypeFileAccess,
    resolve: ResolveHook | null,
  ): Promise<ReadonlySet<HostCanonicalId> | null> {
    const hydrated = new Set<HostCanonicalId>();
    for (const input of inputs) {
      if (input.kind === "file") {
        const file = await hydrator.ensureFile(input.canonicalId, resolve);
        if (!file.registered || file.id === null) return null;
        hydrated.add(file.id);
      } else if (input.kind === "route") {
        const target = await typeTargetOf(input.importer, input.specifier, resolve, files);
        if (target === null) return null;
        for (const id of await hydrator.closureOf([target], resolve)) hydrated.add(id);
        routes.contribute(
          input.importer as HostCanonicalId,
          [{ specifier: input.specifier, resolvedCanonicalId: target }],
          "declaration",
        );
      }
    }
    return hydrated;
  }

  /**
   * SSR transforms (import.meta dead-code elimination, component stripping).
   * Each rewrite carries the in-flight map along with the code, so the map's
   * generated columns stay aligned with whatever text it currently describes.
   */
  private rewriteForTarget(main: PublishedMain, ssr: boolean): PublishedMain {
    const ssrOptions = this.session.binding.options.ssr;
    let current: SsrRewriteResult = { code: main.code, map: main.map ?? undefined };
    const apply = (next: SsrRewriteResult): void => {
      if (next.code !== current.code) this.workCounters.mapCompositions += 1;
      current = next;
    };
    if (ssrOptions?.deadCodeElimination !== false) {
      apply(replaceImportMetaSsr(current.code, ssr, current.map));
    }
    if (ssr && ssrOptions?.clientOnlyComponents?.length) {
      apply(stripComponents(current.code, ssrOptions.clientOnlyComponents, current.map));
    }
    if (!ssr && ssrOptions?.serverOnlyComponents?.length) {
      apply(stripComponents(current.code, ssrOptions.serverOnlyComponents, current.map));
    }
    return { code: current.code, map: current.map ?? null, lang: main.lang };
  }

  /** Throws when the flight no longer owns its key or its epoch has passed. */
  private ensureCurrent(key: CarrierDemandKey, k: string, flight: Flight, epoch: number): void {
    const outcome = this.overtaken(key, k, flight, epoch);
    if (outcome) throw outcome;
  }

  /**
   * `null` while the flight is current. Otherwise what its joiners get: the
   * newer flight's outcome, the record that flight published, or `stale`.
   */
  private overtaken(
    key: CarrierDemandKey,
    k: string,
    flight: Flight,
    epoch: number,
  ): FlightReplaced | PipelineRefusalError | null {
    if (this.session.epoch !== epoch) {
      return new PipelineRefusalError({
        kind: "stale",
        message: `${key.rawPath} was compiled under session epoch ${epoch}, the session is at ${this.session.epoch}`,
      });
    }
    const current = this.flights.get(k);
    if (current === flight) return null;
    if (current) return new FlightReplaced(current.promise);
    const record = this.session.catalog.record(key.carrier, key.graph);
    if (record && record.epoch === epoch) return new FlightReplaced(Promise.resolve(record));
    return new PipelineRefusalError({
      kind: "stale",
      message: `${key.rawPath} changed before its compile finished`,
    });
  }

  /** The route table and hydrator of the session's current native host. */
  private currentHostState(): HostState {
    const host = this.session.host();
    if (this.hostState?.host === host) return this.hostState;
    const routes = new DependencyRouteTable(host, this.workCounters);
    const files = typeFileAccess(this.session.workspace());
    const hydrator = new TypeDependencyHydrator(host, files, routes, this.workCounters);
    this.hostState = { host, routes, hydrator, files };
    return this.hostState;
  }
}

function flightKey(key: CarrierDemandKey): string {
  return encodeFramed([key.carrier, key.graph]);
}

function isStaleRefusal(error: unknown): error is PipelineRefusalError {
  return error instanceof PipelineRefusalError && error.refusal.kind === "stale";
}

/** A native compile failure with the carrier named; it keeps its `kind` and travels as the `cause`. */
function compileFailure(canonicalId: string, error: unknown): Error {
  const message = error instanceof Error ? error.message : String(error);
  const kind =
    error !== null && typeof error === "object" ? (error as { kind?: unknown }).kind : undefined;
  return Object.assign(new Error(`[verter] ${canonicalId}: ${message}`), {
    cause: error,
    ...(kind !== undefined ? { kind } : {}),
  });
}

/**
 * One typed compile against an already-registered carrier. Admission/decode
 * refusals and error-severity diagnostics fail closed; warning-severity is
 * forwarded.
 *
 * A thrown native failure keeps its `kind` and travels as the `cause`; an
 * incomplete-dependencies failure becomes a `need-inputs` refusal.
 */
function executeTypedCompile(
  host: VerterHost,
  canonicalId: string,
  profile: VerterRenderProfile,
  framework: CarrierFramework,
  authoredOnlyStyleProcessing: boolean,
  warn?: BundlerWarn,
): HostCompileResponse {
  let response: HostCompileResponse;
  try {
    response = host.compileRequest(
      canonicalId,
      typedRenderRequest(profile, framework, {
        authoredOnlyStyles: authoredOnlyStyleProcessing,
      }),
    );
  } catch (error) {
    const refusal = refusalFromCompileError(error);
    if (refusal) throw new PipelineRefusalError(refusal, error);
    throw compileFailure(canonicalId, error);
  }
  forwardTypedDiagnostics(canonicalId, response, warn);
  return response;
}

/**
 * The module id registered on `ssrContext.modules` for Vite SSR asset
 * collection. Vite's ssr-manifest keys are ROOT-RELATIVE, the same shape
 * `@vitejs/plugin-vue` registers. Without a known root the id is omitted (the
 * host falls back to the canonical id).
 */
function ssrModuleIdFor(ssr: boolean, root: string, filename: string): string | undefined {
  if (!ssr || !root) return undefined;
  return canonicalizeId(relative(root, filename));
}

function readText(ws: Workspace, pathname: string): Promise<string | null> {
  return ws.readFile(canonicalizeId(pathname));
}

/**
 * Resolves external sources and module references from an upsert result and
 * contributes the carrier's module routes while the demand is still current.
 * Returns the refreshed upsert result.
 */
async function resolveModuleReferences(
  host: VerterHost,
  workspace: Workspace,
  filename: string,
  ownerSource: string,
  upsertResult: HostUpdateResult,
  resolveId: ResolveHook | null,
  routes: DependencyRouteTable,
  owner: HostCanonicalId,
  counters: PipelineCounters,
  current: () => boolean,
): Promise<HostUpdateResult> {
  let refreshedResult = upsertResult;
  let registeredExternalSource = false;

  // Resolve external sources (e.g., <style src="./foo.less">, <template src="./t.html">)
  for (const req of upsertResult.externalSourceRequests) {
    const resolvedId: string = req.resolvedCanonicalId;
    // Let the bundler choose the bytes when it can resolve this specifier,
    // while keeping the host-minted canonical ID as the VFS identity.
    const hookResolution = resolveId
      ? await resolveThroughBundler(resolveId, req.specifier, filename)
      : null;
    const hookReadId = hookResolution?.id ?? null;
    let extSource = await readText(workspace, hookReadId ?? resolvedId);
    if (extSource === null && hookReadId !== null && hookReadId !== resolvedId) {
      extSource = await readText(workspace, resolvedId);
    }
    if (extSource === null && hookResolution?.failure) throw hookResolution.failure.error;
    if (extSource !== null) {
      host.upsert({ inputId: resolvedId, source: extSource });
      registeredExternalSource = true;
    }
  }

  // External requests are stamped when the owner is upserted. Re-publish the
  // byte-identical owner only after all external bytes are in the host VFS so
  // processed-content requests capture those exact bytes and fresh stamps.
  if (registeredExternalSource && current()) {
    refreshedResult = host.upsert({
      inputId: filename,
      source: ownerSource,
    });
    counters.registrations += 1;
  }

  // Resolve exact and finite-set module references, then feed per-specifier
  // resolution records back to the host for exact cross-file resolution.
  const resolutions: HostDependencyResolution[] = [];
  const dependencySpecifiers = collectResolvableModuleReferenceSpecifiers(
    host,
    refreshedResult.moduleReferences ?? [],
  );
  for (const specifier of dependencySpecifiers) {
    const relative = specifier.startsWith(".");
    // The bundler's resolver goes first, so its aliases apply.
    const resolution = resolveId
      ? await resolveThroughBundler(resolveId, specifier, filename)
      : null;
    const failure = resolution?.failure ?? null;
    let target = resolution?.id ?? null;
    if (target === null) {
      // A relative specifier is resolved by the workspace. A bare specifier
      // whose bundler resolution threw may be a package sub-path with no
      // runtime target (exported only under the `types` condition): its
      // declaration file is the dependency. Any other bare specifier is
      // hydrated from the carrier's macro types, not here.
      target = relative
        ? await workspace.resolveImport(filename, specifier)
        : failure
          ? await workspace.resolveImport(filename, specifier, undefined, "type")
          : null;
    }
    const id = target === null ? null : canonicalizeId(target);
    // A carrier is the bundler's to supply; any other dependency is read.
    if (id !== null && !host.isCarrierSourcePath(id)) {
      const depSource = await readText(workspace, id);
      if (depSource === null) {
        if (failure) throw failure.error;
        continue;
      }
      host.upsert({ inputId: id, source: depSource });
    }
    if (id === null) {
      if (failure) throw failure.error;
      continue;
    }
    resolutions.push({ specifier, resolvedCanonicalId: id });
  }
  // Also include external src="..." blocks that were resolved during upsert
  for (const req of refreshedResult.externalSourceRequests ?? []) {
    resolutions.push({
      specifier: req.specifier,
      resolvedCanonicalId: req.resolvedCanonicalId,
    });
  }
  // A superseded demand must not leave its routes in the newer revision's table.
  if (current()) routes.contribute(owner, resolutions, "module", { always: true });
  return refreshedResult;
}

interface HostStructureRange {
  start: number;
  end: number;
  /** Versioned encoding tag of `start`/`end`. */
  encoding: string;
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

/** The host answers every range in the unit the request asked for; anything else is refused. */
function expectUtf16(range: HostStructureRange): void {
  if (range.encoding !== "utf16.v1") {
    throw new Error(
      `document structure range declares encoding '${String(range.encoding)}'; expected 'utf16.v1'`,
    );
  }
}

function styleEntriesFromStructure(
  host: VerterHost,
  filename: string,
  source: string,
): AuthoredStyleBlock[] {
  const encoded = host.getDocumentStructure(filename, "utf16.v1");
  if (encoded === null) return [];
  const structure = JSON.parse(encoded) as HostDocumentStructure;
  const entries: AuthoredStyleBlock[] = [];
  for (const block of structure.blocks) {
    const section = block.kind === "section" ? block.section : undefined;
    if (section?.role.kind !== "style") continue;
    if (section.contentRange.start > section.contentRange.end) {
      throw new Error(
        `document structure content range ${section.contentRange.start}..${section.contentRange.end} is inverted`,
      );
    }
    expectUtf16(section.contentRange);
    const { start, end } = section.contentRange;
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

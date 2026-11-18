import type { HostDependencyResolution, VerterHost, Workspace } from "@verter/native";
import type { ResolveHook } from "../bundler-resolve";
import type { BundlerSourceRevision, HostCanonicalId } from "../host-session/identity";
import { typeTargetOf, type FileAnalysisSnapshot } from "../macro-type-hydration";
import type { PipelineCounters } from "./counters";
import type { DependencyRouteTable } from "./route-table";

/** What hydration asks of the session's workspace. */
export interface TypeFileAccess {
  readFile(path: string): Promise<string | null>;
  /** The file declaring the types `specifier` names from `importer`; `null` when none does. */
  resolveImport(importer: string, specifier: string): Promise<string | null>;
  /** Whether the file belongs to an installed package. */
  isPackageBacked(canonicalId: string): boolean;
}

/** One dependency file after its own registration and routes are complete. */
export interface HydratedFile {
  /** The host's id for the file; `null` when the file could not be read and the host does not track it. */
  readonly id: HostCanonicalId | null;
  /** `false` when the file could not be read; it then has no children and is not remembered. */
  readonly registered: boolean;
  /** Resolved paths of the file's type imports, re-exports and macro type sources. */
  readonly children: readonly string[];
}

function parseAnalysis(json: string | null): FileAnalysisSnapshot | null {
  if (!json) return null;
  try {
    return JSON.parse(json) as FileAnalysisSnapshot;
  } catch {
    return null;
  }
}

function unregistered(id: HostCanonicalId | null): Promise<HydratedFile> {
  return Promise.resolve({ id, registered: false, children: [] });
}

/**
 * Loads the declaration files a carrier's macro types need into the native
 * host, with their import routes.
 *
 * A file counts as hydrated only when its promise has resolved. A carrier's
 * hydration resolves only after every file of its closure has. A failed file
 * is forgotten, so the next demand retries it.
 *
 * Every method accepts any spelling of a path. Internally files are keyed by
 * the host's id, so two spellings of one file share one entry.
 */
export class TypeDependencyHydrator {
  /** Host id of a file to the promise of its registration and routes. Never a "started" mark. */
  private readonly files = new Map<HostCanonicalId, Promise<HydratedFile>>();
  private readonly completed = new Set<HostCanonicalId>();
  /** Carriers registered in the host with bytes the bundler supplied, by revision. */
  private readonly carriers = new Map<HostCanonicalId, BundlerSourceRevision>();

  constructor(
    private readonly host: VerterHost,
    private readonly access: TypeFileAccess,
    private readonly routes: DependencyRouteTable,
    private readonly counters: PipelineCounters,
  ) {}

  /**
   * The carrier was registered in the host with bytes the bundler supplied.
   * A later `ensureFile` for the same path neither reads nor upserts it.
   * A revision different from the one noted before invalidates the path first.
   */
  noteCarrier(carrier: HostCanonicalId, revision: BundlerSourceRevision): void {
    const noted = this.carriers.get(carrier);
    if (noted !== undefined && noted !== revision) this.forget(carrier);
    this.carriers.set(carrier, revision);
  }

  /**
   * Register one dependency file and its own routes. Single-flight per file.
   * Never waits for another file. Rejects when a resolver fails; the file is
   * then forgotten. An unreadable file resolves `{ registered: false }` and is
   * not remembered, so a later call reads again.
   *
   * The memo is keyed by the id the host gives the path, which it knows
   * without the file's bytes, so the completion promise is stored before the
   * first `await`; the read, the upsert and the walk happen inside it.
   */
  ensureFile(path: string, resolve: ResolveHook | null): Promise<HydratedFile> {
    const id = this.keyOf(path);
    if (id === null) return unregistered(null);
    const held = this.files.get(id);
    if (held) return held;

    const load = this.load(id, resolve);
    this.files.set(id, load);
    load.then(
      (file) => {
        if (this.files.get(id) !== load) return;
        if (file.registered) this.completed.add(id);
        else this.files.delete(id);
      },
      () => {
        if (this.files.get(id) === load) this.files.delete(id);
      },
    );
    return load;
  }

  /** Read and register the file unless the bundler supplied it, then resolve its routes. */
  private async load(id: HostCanonicalId, resolve: ResolveHook | null): Promise<HydratedFile> {
    if (!this.carriers.has(id)) {
      const source = await this.access.readFile(id);
      this.counters.hydrationReads++;
      if (source === null) return { id: this.hostIdOf(id), registered: false, children: [] };
      try {
        // The host classifies the file from its path: an SFC is analysed as
        // an SFC, everything else as plain source.
        this.host.upsert({ inputId: id, source });
      } catch {
        return { id: this.hostIdOf(id), registered: false, children: [] };
      }
    }
    return this.walk(id, resolve);
  }

  /** The file's own routes, contributed once its sources are resolved. */
  private async walk(id: HostCanonicalId, resolve: ResolveHook | null): Promise<HydratedFile> {
    const analysis = parseAnalysis(this.host.getAnalysis(id));
    const children: string[] = [];
    const routes: HostDependencyResolution[] = [];
    if (analysis) {
      const sources = new Set<string>([
        ...(analysis.imports ?? []).map((entry) => entry.source),
        ...(analysis.exportSignatures ?? [])
          .filter((signature) => signature.reexportSource)
          .map((signature) => signature.reexportSource as string),
        ...(analysis.macroTypeDeps ?? []).map((dep) => dep.importSource),
      ]);
      for (const source of sources) {
        const target = await typeTargetOf(id, source, resolve, this.access);
        if (target === null) continue;
        // A walk never enters a package from a bare import: package
        // declarations are hydrated once, from the carrier's own specifiers.
        if (!source.startsWith(".") && this.access.isPackageBacked(target)) continue;
        if (!children.includes(target)) children.push(target);
        routes.push({ specifier: source, resolvedCanonicalId: target });
      }
    }
    this.routes.contribute(id, routes, "declaration");
    return { id, registered: true, children };
  }

  /**
   * Every file reachable from `roots`, each awaited. Safe on cyclic graphs:
   * the walk keeps its own visited set.
   */
  async closureOf(
    roots: readonly string[],
    resolve: ResolveHook | null,
  ): Promise<ReadonlySet<HostCanonicalId>> {
    const seen = new Set(roots);
    const queue = [...seen];
    const closure = new Set<HostCanonicalId>();
    while (queue.length > 0) {
      const file = await this.ensureFile(queue.shift() as string, resolve);
      if (!file.registered || file.id === null || closure.has(file.id)) continue;
      closure.add(file.id);
      for (const child of file.children) {
        if (seen.has(child)) continue;
        seen.add(child);
        queue.push(child);
      }
    }
    return closure;
  }

  /**
   * Hydrate what the carrier's macro types demand and contribute the carrier's
   * declaration routes. Resolves with the closure; empty when the carrier has
   * no macro type dependencies (no file is read).
   */
  async hydrateCarrier(
    carrier: HostCanonicalId,
    resolve: ResolveHook | null,
  ): Promise<ReadonlySet<HostCanonicalId>> {
    const analysis = parseAnalysis(this.host.getAnalysis(carrier));
    // Hydration exists for macro type resolution: a carrier whose analysis
    // demands no macro type deps loads nothing. The analysis already lists
    // local declaration heritage as a dependency, so this is the complete
    // demand set; widening it to every import would crawl the whole graph.
    const specifiers = [...new Set((analysis?.macroTypeDeps ?? []).map((dep) => dep.importSource))];
    if (specifiers.length === 0) return new Set();

    const targets: string[] = [];
    const routes: HostDependencyResolution[] = [];
    for (const specifier of specifiers) {
      const target = await typeTargetOf(carrier, specifier, resolve, this.access);
      if (target === null) continue;
      targets.push(target);
      routes.push({ specifier, resolvedCanonicalId: target });
    }
    this.routes.contribute(carrier, routes, "declaration");
    return this.closureOf(targets, resolve);
  }

  /**
   * A file changed or was removed: forget its completion and its routes, and
   * drop the copy this hydrator registered so the next read sees the new bytes.
   * A carrier the bundler supplied is the session's to remove. Returns `true`
   * when the path was known.
   */
  invalidate(path: string): boolean {
    const id = this.hostIdOf(path);
    if (id === null) return false;
    if (!this.carriers.has(id) && this.files.has(id)) this.host.remove(id);
    return this.forget(id);
  }

  private forget(id: HostCanonicalId): boolean {
    const known = this.files.has(id) || this.carriers.has(id);
    this.files.delete(id);
    this.completed.delete(id);
    this.carriers.delete(id);
    this.routes.forget(id);
    return known;
  }

  /** The host's spelling of a path, whether or not the host holds the file yet. */
  private keyOf(path: string): HostCanonicalId | null {
    const resolved = this.host.resolve(path);
    return "rejected" in resolved ? null : (resolved.canonicalId as HostCanonicalId);
  }

  /** The host's id for a path, or `null` when the host does not track it. No read, no upsert. */
  hostIdOf(path: string): HostCanonicalId | null {
    const resolved = this.host.resolve(path);
    if ("rejected" in resolved || !resolved.existsInHost) return null;
    return resolved.canonicalId as HostCanonicalId;
  }

  /** Files whose hydration has completed. */
  hydratedPaths(): ReadonlySet<HostCanonicalId> {
    return this.completed;
  }

  clear(): void {
    this.files.clear();
    this.completed.clear();
    this.carriers.clear();
    this.routes.clear();
  }
}

/** File access over the session's workspace. */
export function typeFileAccess(workspace: Workspace): TypeFileAccess {
  return {
    readFile: (path) => workspace.readFile(path),
    resolveImport: (importer, specifier) =>
      workspace.resolveImport(importer, specifier, undefined, "type"),
    isPackageBacked: (canonicalId) => workspace.isPackageBacked(canonicalId),
  };
}

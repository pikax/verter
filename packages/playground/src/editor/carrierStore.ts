/**
 * In-memory `CarrierStoreReader` backing for the WASM in-context
 * LanguageService. The browser worker feeds it the WASM host's three
 * generated surfaces per carrier source —
 *
 *   - IDE carrier         `Comp.vue.tsx`        ← `getIde(id, profile)`
 *   - declaration carrier `Comp.d.vue.ts`       ← `getPublicApi(id, "declaration")`
 *   - API carrier         `Comp.vue.verter.ts`  ← `getPublicApi(id)`
 *
 * — and this store serves them through the shared CORE reader contract, so
 * the host/mapper orchestration never forks from the Node tsserver-plugin
 * instantiation. Browser-safe: no fs, no manifest file, no Node builtins.
 */
import {
  normalizePath,
  scriptKindForCarrier,
  toDeclarationCarrierFileName,
  toIdeCarrierFileName,
  toVueVirtualFileName,
  type CarrierStoreReader,
  type Manifest,
  type ManifestRole,
  type ManifestScriptKind,
  type OwnedSource,
  type ProjectEntry,
  type ReadyFile,
} from "@verter/language-shared";

/** One generated surface's content (+ optional V3 source map JSON). */
export interface CarrierSurfaceContent {
  code: string;
  sourceMap?: string | null;
}

/** The three surfaces a carrier source publishes (absent = not produced). */
export interface CarrierSurfaces {
  ide?: CarrierSurfaceContent | null;
  decl?: CarrierSurfaceContent | null;
  api?: CarrierSurfaceContent | null;
}

/** The provider paths one upsert published, by surface. */
export interface PublishedCarrierPaths {
  ide?: string;
  decl?: string;
  api?: string;
}

interface StoredCarrier {
  sourceUri: string;
  providerPath: string;
  role: ManifestRole;
  scriptKind: ManifestScriptKind;
  code: string;
  mapJson: string | null;
  version: number;
}

/**
 * The string-valued `ts.ScriptKind` facade: the CORE policy computes the
 * manifest `script_kind` STRING directly from the descriptor-derived rules —
 * no `typescript` import in this browser-safe module.
 */
const MANIFEST_SCRIPT_KIND_FACADE = {
  ScriptKind: {
    TS: "TS" as ManifestScriptKind,
    TSX: "TSX" as ManifestScriptKind,
    JS: "JS" as ManifestScriptKind,
    JSX: "JSX" as ManifestScriptKind,
  },
};

/** Cheap stable content hash (identity only — the store is in-memory). */
function contentHash(text: string): string {
  let hash = 5381;
  for (let i = 0; i < text.length; i += 1) {
    hash = ((hash << 5) + hash + text.charCodeAt(i)) | 0;
  }
  return `mem-${(hash >>> 0).toString(16)}-${text.length}`;
}

/**
 * The `Map`-backed in-memory carrier store. `upsertSource` publishes ALL of a
 * source's surfaces in ONE synchronous call (atomic from every reader's
 * perspective), bumps the store epoch once, and bumps each carrier's version
 * MONOTONICALLY (version counters survive remove/re-add so a host
 * `getScriptVersion` never moves backwards). `removeSource` retires every
 * carrier of a source.
 */
export class InMemoryCarrierStore implements CarrierStoreReader {
  /** providerPath → stored carrier. */
  private readonly carriers = new Map<string, StoredCarrier>();
  /** sourceUri → provider paths it published. */
  private readonly bySource = new Map<string, string[]>();
  /** providerPath → monotonic version seed (never deleted). */
  private readonly versionSeeds = new Map<string, number>();
  /** providerPath → last successfully served blob. */
  private readonly lastGood = new Map<string, string>();
  private epoch = 0;
  private readonly projectUri: string;

  constructor(projectUri = "playground://project") {
    this.projectUri = projectUri;
  }

  /**
   * Atomically publish/update every surface of `sourcePath`. Surfaces absent
   * from `surfaces` are RETIRED (the WASM host no longer produces them).
   */
  upsertSource(sourcePath: string, surfaces: CarrierSurfaces): PublishedCarrierPaths {
    const source = normalizePath(sourcePath);
    const published: PublishedCarrierPaths = {};
    const next: StoredCarrier[] = [];

    const ide = surfaces.ide ?? null;
    const idePath = toIdeCarrierFileName(source);
    if (ide && idePath) {
      next.push(this.storedCarrier(source, idePath, "CarrierIde", ide));
      published.ide = idePath;
    }
    const decl = surfaces.decl ?? null;
    const declPath = toDeclarationCarrierFileName(source);
    if (decl && declPath) {
      next.push(this.storedCarrier(source, declPath, "CarrierApi", decl));
      published.decl = declPath;
    }
    const api = surfaces.api ?? null;
    // Only a recognised component carrier projects a distinct API virtual
    // file (`toVueVirtualFileName` would append a bare `.ts` to anything).
    const apiPath = idePath === null ? null : toVueVirtualFileName(source, "public");
    if (api && apiPath) {
      next.push(this.storedCarrier(source, apiPath, "CarrierApi", api));
      published.api = apiPath;
    }

    // Retire carriers the new publish no longer produces, then swap the
    // source's carrier set in one pass.
    const previous = this.bySource.get(source) ?? [];
    const nextPaths = next.map((c) => c.providerPath);
    for (const stale of previous) {
      if (!nextPaths.includes(stale)) {
        this.carriers.delete(stale);
      }
    }
    for (const carrier of next) {
      this.carriers.set(carrier.providerPath, carrier);
    }
    this.bySource.set(source, nextPaths);
    this.epoch += 1;
    return published;
  }

  /** Remove every carrier `sourcePath` published (delete / rename cleanup). */
  removeSource(sourcePath: string): void {
    const source = normalizePath(sourcePath);
    const paths = this.bySource.get(source);
    if (paths === undefined) return;
    for (const providerPath of paths) {
      this.carriers.delete(providerPath);
    }
    this.bySource.delete(source);
    this.epoch += 1;
  }

  /** The provider paths currently published for a source. */
  carrierPathsFor(sourcePath: string): string[] {
    return [...(this.bySource.get(normalizePath(sourcePath)) ?? [])];
  }

  private storedCarrier(
    sourceUri: string,
    providerPath: string,
    role: ManifestRole,
    content: CarrierSurfaceContent,
  ): StoredCarrier {
    const normalized = normalizePath(providerPath);
    const version = (this.versionSeeds.get(normalized) ?? 0) + 1;
    this.versionSeeds.set(normalized, version);
    const scriptKind =
      scriptKindForCarrier(normalized, MANIFEST_SCRIPT_KIND_FACADE) ?? ("TS" as ManifestScriptKind);
    return {
      sourceUri,
      providerPath: normalized,
      role,
      scriptKind,
      code: content.code,
      mapJson:
        typeof content.sourceMap === "string" && content.sourceMap.length > 2
          ? content.sourceMap
          : null,
      version,
    };
  }

  // ── CarrierStoreReader ──

  isAvailable(): boolean {
    return true;
  }

  readManifest(): Manifest | undefined {
    const project: ProjectEntry = { owned_sources: [], ready_files: {} };
    for (const carrier of this.carriers.values()) {
      project.owned_sources.push(this.ownedSourceRow(carrier));
      project.ready_files[carrier.providerPath] = this.readyFileRow(carrier);
    }
    return {
      epoch: this.epoch,
      host_version: "in-memory",
      projects: { [this.projectUri]: project },
    };
  }

  currentEpoch(): number | undefined {
    return this.epoch;
  }

  ownedSources(projectUri?: string): OwnedSource[] {
    if (projectUri !== undefined && projectUri !== this.projectUri) {
      return [];
    }
    return [...this.carriers.values()].map((carrier) => this.ownedSourceRow(carrier));
  }

  readyFile(providerPath: string): ReadyFile | undefined {
    const carrier = this.carriers.get(normalizePath(providerPath));
    return carrier === undefined ? undefined : this.readyFileRow(carrier);
  }

  readyIdeCompanions(): string[] {
    return [...this.carriers.values()]
      .filter((carrier) => carrier.role === "CarrierIde")
      .map((carrier) => carrier.providerPath);
  }

  readyFileForSource(sourcePath: string): ReadyFile | undefined {
    const companion = this.companionForSource(sourcePath);
    return companion === undefined ? undefined : this.readyFile(companion);
  }

  companionForSource(sourcePath: string): string | undefined {
    const companion = toIdeCarrierFileName(normalizePath(sourcePath));
    return companion === null ? undefined : companion;
  }

  ownedSourceFor(providerOrSourcePath: string): OwnedSource | undefined {
    const normalized = normalizePath(providerOrSourcePath);
    const direct = this.carriers.get(normalized);
    if (direct !== undefined) {
      return this.ownedSourceRow(direct);
    }
    const companion = this.companionForSource(normalized);
    if (companion !== undefined) {
      const viaSource = this.carriers.get(companion);
      if (viaSource !== undefined) {
        return this.ownedSourceRow(viaSource);
      }
    }
    return undefined;
  }

  readBlobSync(blobRel: string, providerPath?: string): string | undefined {
    // In-memory blobs are keyed by provider path directly (`blob_rel` IS the
    // provider path).
    const carrier = this.carriers.get(normalizePath(blobRel));
    if (carrier === undefined) {
      return providerPath !== undefined ? undefined : undefined;
    }
    if (providerPath !== undefined) {
      this.lastGood.set(normalizePath(providerPath), carrier.code);
    }
    return carrier.code;
  }

  readMapSync(mapRel: string): unknown | undefined {
    const carrier = this.carriers.get(normalizePath(mapRel));
    if (carrier === undefined || carrier.mapJson === null) {
      return undefined;
    }
    try {
      return JSON.parse(carrier.mapJson) as unknown;
    } catch {
      return undefined;
    }
  }

  lastGoodBlobFor(providerPath: string): string | undefined {
    return this.lastGood.get(normalizePath(providerPath));
  }

  private ownedSourceRow(carrier: StoredCarrier): OwnedSource {
    return {
      source_uri: carrier.sourceUri,
      provider_uri: carrier.providerPath,
      role: carrier.role,
      script_kind: carrier.scriptKind,
    };
  }

  private readyFileRow(carrier: StoredCarrier): ReadyFile {
    return {
      content_hash: contentHash(carrier.code),
      version: carrier.version,
      script_kind: carrier.scriptKind,
      role: carrier.role,
      map_hash: carrier.mapJson === null ? "" : contentHash(carrier.mapJson),
      // In-memory: the blob/map "relative path" is the provider path itself.
      blob_rel: carrier.providerPath,
      ...(carrier.mapJson === null ? {} : { map_rel: carrier.providerPath }),
    };
  }

  /** The raw map JSON for a provider path (mapper construction). */
  mapJsonFor(providerPath: string): string | null {
    return this.carriers.get(normalizePath(providerPath))?.mapJson ?? null;
  }
}

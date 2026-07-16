// The carrier-store reader CONTRACT. This module is BROWSER-SAFE and pure: it
// defines the manifest value types the Rust `verter_lsp` publishes and the
// reader interface every host consumes — it contains NO Node APIs and no
// disk/manifest/epoch implementation details. The Node tsserver plugin
// implements it with a synchronous Node-fs disk adapter; a browser/WASM
// in-context host implements it over in-memory published snapshots. Both
// instantiations share the SAME interface so the remap/mapper orchestration
// never forks per host.

/** The carrier role as serialized in the manifest. */
export type ManifestRole = "CarrierIde" | "CarrierApi" | "Shadow" | "Real";

/** The TypeScript script kind as serialized in the manifest. */
export type ManifestScriptKind = "TSX" | "TS" | "JSX" | "JS";

/**
 * One entry in a project's `owned_sources`: the full project-owned carrier set,
 * known the moment ownership resolves (BEFORE any content is published). A
 * source is advertised to the TS host only once it appears in `ready_files`.
 */
export interface OwnedSource {
  source_uri: string;
  provider_uri: string;
  role: ManifestRole;
  script_kind: ManifestScriptKind;
}

/**
 * One entry in a project's `ready_files`: a `provider_uri` whose content blob
 * write has SUCCEEDED. Carries the content-addressed blob/map relative paths so
 * the reader reads the exact bytes the carrier's offsets/maps were produced
 * against.
 */
export interface ReadyFile {
  content_hash: string;
  version: number;
  script_kind: ManifestScriptKind;
  role: ManifestRole;
  map_hash: string;
  /** `blobs/blake3-<content_hash_hex>.<ext>` — relative to the store dir. */
  blob_rel: string;
  /** `maps/blake3-<map_hash_hex>.json` — relative to the store dir; absent when no map. */
  map_rel?: string;
}

/** One project's manifest entry: its owned carrier set plus the ready subset. */
export interface ProjectEntry {
  owned_sources: OwnedSource[];
  /** Keyed by `provider_uri`. */
  ready_files: Record<string, ReadyFile>;
}

/** The atomic manifest. `epoch` is monotonic across every publish to this store. */
export interface Manifest {
  epoch: number;
  host_version: string;
  /** Keyed by `project_uri` (the owning tsconfig URI). */
  projects: Record<string, ProjectEntry>;
}

/**
 * The synchronous carrier-store reader every host consumes. The Rust
 * `verter_lsp` is the sole carrier authority — it compiles every framework
 * carrier (`.vue`/`.svelte`) and publishes the result as content-addressed
 * blobs + maps under an atomically-swapped manifest keyed by a monotonic
 * `epoch`. An implementation reads that published state; it never compiles a
 * carrier itself.
 *
 * Contract notes an implementation must honor:
 *
 * - **Fail closed, never throw into a host hook.** A missing/torn manifest, an
 *   unavailable store, or a missing blob yields `undefined`/empty results.
 * - **Scoping.** A project-scoped implementation consults only its own
 *   project's manifest entry; an unscoped one spans every project.
 * - **`readBlobSync` last-good retention.** A successfully-read blob for a
 *   `providerPath` is retained so `lastGoodBlobFor` can serve it across a
 *   transient not-ready window (mid-publish).
 *
 * How an implementation SOURCES the data (disk + Node fs, in-memory
 * snapshots, a WASM bridge) is entirely its own concern — no storage detail
 * leaks into this interface.
 */
export interface CarrierStoreReader {
  /**
   * Canonicalize a path under the owning host filesystem's identity policy.
   * Readers that omit this optional capability retain exact normalized-path
   * comparisons; Node editor readers provide it so Windows drive/case variants
   * still address the same published source without weakening case-sensitive
   * hosts.
   */
  canonicalPath?(fileName: string): string;

  /** Whether the store backing this reader is configured/available at all. */
  isAvailable(): boolean;

  /**
   * The current manifest, or `undefined` when the store is unavailable or the
   * manifest is missing/torn (fail closed — never throws).
   */
  readManifest(): Manifest | undefined;

  /** The current published epoch, or `undefined` when the store is unavailable. */
  currentEpoch(): number | undefined;

  /**
   * The owned-source set. With an explicit `projectUri`, that one project's
   * owned set (regardless of the reader's scope). Without one, the reader's
   * scoped project set (every project for an unscoped reader). Empty when the
   * store is unavailable or the manifest names no owned sources.
   */
  ownedSources(projectUri?: string): OwnedSource[];

  /**
   * The `ReadyFile` entry for a provider path (the carrier companion path,
   * e.g. `.../Comp.vue.tsx`), searched within the reader's scoped project set,
   * or `undefined` when that project has not published the companion's content
   * yet.
   */
  readyFile(providerPath: string): ReadyFile | undefined;

  /**
   * The IDE-companion provider paths of every READY `CarrierIde` carrier in
   * the reader's scoped project set, intersected with that project's
   * owned-source set (a ready entry is advertised only when the project also
   * OWNS that companion).
   */
  readyIdeCompanions(): string[];

  /**
   * The `ReadyFile` for the IDE companion that backs a carrier SOURCE path
   * (`Comp.vue` → its `Comp.vue.tsx` companion's ready entry), or `undefined`
   * when the path is not a carrier source or its companion is not yet
   * published.
   */
  readyFileForSource(sourcePath: string): ReadyFile | undefined;

  /**
   * The IDE companion provider path that backs a carrier SOURCE path.
   * `undefined` for a non-carrier path.
   */
  companionForSource(sourcePath: string): string | undefined;

  /**
   * The `OwnedSource` entry for a path that may be a provider companion path
   * OR a source path — searched within the reader's scoped project set.
   */
  ownedSourceFor(providerOrSourcePath: string): OwnedSource | undefined;

  /**
   * Read a content blob synchronously by its store-relative path. Returns
   * `undefined` when the store is unavailable or the blob is missing. A
   * successfully-read blob for `providerPath` is retained as last-good.
   */
  readBlobSync(blobRel: string, providerPath?: string): string | undefined;

  /**
   * Read the sourcemap JSON for a ready file (for navigation remapping).
   * Returns `undefined` when the store is unavailable, the carrier carries no
   * map, or the map blob is missing/unparseable.
   */
  readMapSync(mapRel: string): unknown | undefined;

  /** The last-good blob previously served for `providerPath`, if any. */
  lastGoodBlobFor(providerPath: string): string | undefined;
}

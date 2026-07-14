import fs from "node:fs";
import path from "node:path";
import {
  carrierSourceToCompanion,
  normalizePath,
  type CarrierStoreReader,
  type Manifest,
  type OwnedSource,
  type ProjectEntry,
  type ReadyFile,
} from "@verter/language-shared";

/**
 * The NODE ADAPTER of the shared [`CarrierStoreReader`] interface: a
 * synchronous `node:fs` reader over the Rust-published on-disk
 * content-addressed carrier-snapshot store + atomic manifest. The interface
 * (and the manifest value types) live in `@verter/language-shared`; this is
 * the SOLE module in the plugin that touches the store filesystem — every
 * host hook reads carriers through it.
 *
 * The Rust `verter_lsp` is the sole carrier authority — it compiles every
 * framework carrier (`.vue`/`.svelte`) and publishes the result to this store
 * (content-addressed blobs + maps under an atomically-swapped `manifest.json`
 * keyed by a monotonic `epoch`). The plugin runs inside the user's tsserver (a
 * SEPARATE process with NO shared memory), so it reads the store synchronously
 * and never compiles a carrier itself.
 *
 * ## Store layout (mirrors the Rust publish store)
 *
 * The per-workspace store dir holds `blobs/`, `maps/`, and `manifest.json`. The
 * plugin does NOT recompute the dir — the Rust LSP passes the RESOLVED dir in
 * the plugin config (`carrierStoreDir`) with a `VERTER_CARRIER_STORE_DIR`
 * environment fallback. When neither is set the store is UNAVAILABLE: the
 * reader serves nothing for carriers (fail closed) and the host hooks fall
 * through to real disk for everything else.
 *
 * ## Manifest schema (the Rust serde shape)
 *
 * ```jsonc
 * {
 *   "epoch": 7,
 *   "host_version": "…",
 *   "projects": {
 *     "<project_uri>": {
 *       "owned_sources": [
 *         { "source_uri": "…", "provider_uri": "…",
 *           "role": "CarrierIde", "script_kind": "TSX" }
 *       ],
 *       "ready_files": {
 *         "<provider_uri>": {
 *           "content_hash": "<hex>", "version": 3,
 *           "script_kind": "TSX", "role": "CarrierIde",
 *           "map_hash": "<hex>",
 *           "blob_rel": "blobs/blake3-….tsx",
 *           "map_rel": "maps/blake3-….json"   // absent when no map
 *         }
 *       }
 *     }
 *   }
 * }
 * ```
 *
 * The `role` / `script_kind` JSON values are the Rust `ManifestRole` /
 * `ManifestScriptKind` serde renames (`CarrierIde`/`CarrierApi`/`Shadow`/`Real`;
 * `TSX`/`TS`/`JSX`/`JS`).
 *
 * ## Read consistency
 *
 * The manifest is read once and CACHED, keyed by the manifest file's mtime + size.
 * A re-read only happens when the file changes (a sync `fs.statSync` is the cheap
 * change check on every accessor). A missing or torn manifest is tolerated — every
 * accessor returns `undefined`/empty rather than throwing into a tsserver hook.
 * The Rust two-phase publish guarantees every `ready_files` entry the cached
 * manifest names has its blob present on disk, so a blob read for a ready file
 * never observes a half-written file.
 */

/** The plugin-config / environment keys carrying the resolved store dir. */
export const CARRIER_STORE_DIR_CONFIG_KEY = "carrierStoreDir";
export const CARRIER_STORE_DIR_ENV_KEY = "VERTER_CARRIER_STORE_DIR";

/**
 * Resolve the store dir from the plugin config (preferred) or the environment
 * fallback. Returns `undefined` when neither is set — the store is unavailable
 * and the reader serves nothing for carriers.
 */
export function resolveCarrierStoreDir(
  config: { readonly [CARRIER_STORE_DIR_CONFIG_KEY]?: unknown } | undefined,
): string | undefined {
  const fromConfig = config?.[CARRIER_STORE_DIR_CONFIG_KEY];
  if (typeof fromConfig === "string" && fromConfig.length > 0) {
    return fromConfig;
  }
  const fromEnv = process.env[CARRIER_STORE_DIR_ENV_KEY];
  if (typeof fromEnv === "string" && fromEnv.length > 0) {
    return fromEnv;
  }
  return undefined;
}

/**
 * The plugin-config / environment keys gating companion→source RESPONSE
 * remapping by surface. The plugin serves TWO surfaces:
 *
 * 1. **VS Code DIRECT surface** — VS Code's own TS server loads this plugin and
 *    a plain `.ts` talks to the plugin DIRECTLY, with no Verter LSP in the
 *    response path. The plugin is then the SOLE response mapper, so it MUST map
 *    carrier-companion responses back to `.vue`/`.svelte` source. This is the
 *    DEFAULT (`responseRemap` ENABLED).
 * 2. **verter_lsp-internal backend** — verter_lsp spawns its OWN tsserver with
 *    this plugin and queries it; the Rust `verter_lsp` merge layer is the SOLE
 *    response mapper (it owns the authoritative `ProviderPositionMapper`, strict
 *    offset mapping, preamble-import re-anchor, current-vs-foreign classification,
 *    fail-closed). The plugin pre-mapping there would DOUBLE-MAP, so verter_lsp
 *    DISABLES `responseRemap` and the plugin returns RAW companion responses.
 */
export const RESPONSE_REMAP_CONFIG_KEY = "responseRemap";
export const RESPONSE_REMAP_ENV_KEY = "VERTER_PLUGIN_RESPONSE_REMAP";

/**
 * Whether the plugin should map carrier-companion responses back to source. The
 * plugin config (`responseRemap`) is preferred, then the `VERTER_PLUGIN_RESPONSE_REMAP`
 * environment fallback (the SAME channel `carrierStoreDir`/`VERTER_CARRIER_STORE_DIR`
 * uses), then the DEFAULT `true` (the VS Code direct surface, where the plugin is the
 * only mapper). A value of `false` / `"0"` / `"false"` (case-insensitive) DISABLES the
 * remap — the verter_lsp-internal backend sets this so the Rust merge layer is the sole
 * mapper and there is no double-mapping. Any other value keeps the default.
 */
export function resolveResponseRemap(
  config: { readonly [RESPONSE_REMAP_CONFIG_KEY]?: unknown } | undefined,
): boolean {
  const fromConfig = config?.[RESPONSE_REMAP_CONFIG_KEY];
  if (typeof fromConfig === "boolean") {
    return fromConfig;
  }
  const fromEnv = process.env[RESPONSE_REMAP_ENV_KEY];
  if (typeof fromEnv === "string") {
    const normalized = fromEnv.trim().toLowerCase();
    if (normalized === "0" || normalized === "false") {
      return false;
    }
    if (normalized === "1" || normalized === "true") {
      return true;
    }
  }
  // Default ENABLED: the VS Code direct surface, where the plugin is the sole
  // companion→source response mapper.
  return true;
}

/** The change-detection key for the cached manifest: file mtime + size. */
interface ManifestStat {
  mtimeMs: number;
  size: number;
}

/**
 * The synchronous DISK store reader — the node adapter implementing the shared
 * [`CarrierStoreReader`] interface. Constructed once per plugin `create` with
 * the resolved store dir; an `undefined` dir means the store is unavailable and
 * every accessor is a no-op (fail closed).
 *
 * The reader is PROJECT-SCOPED. The plugin's `create(info)` is per configured
 * project, so a reader is bound to that project's identity (`projectKey` — the
 * configured project's `getProjectName()`, the manifest `projects` key). Every
 * carrier lookup (`readyFile` / `ownedSourceFor` / `readyIdeCompanions`) then
 * consults ONLY that project's manifest entry, never another tsconfig's. In a
 * multi-tsconfig workspace this prevents serving / advertising a carrier
 * compiled under one tsconfig's options (`paths`/`types`/`lib`) to a different
 * tsconfig. An UNSCOPED reader (`projectKey` omitted) consults every project —
 * used only by call sites that legitimately span projects.
 */
export class DiskCarrierStoreReader implements CarrierStoreReader {
  private readonly storeDir: string | undefined;
  private readonly manifestPath: string | undefined;
  /**
   * The normalized project identity this reader is scoped to (the configured
   * project's `getProjectName()` — the manifest `projects` key, forward-slash
   * normalized). `undefined` ⇒ the reader spans every project in the manifest.
   */
  private readonly projectKey: string | undefined;
  private cachedManifest: Manifest | undefined;
  private cachedStat: ManifestStat | undefined;
  /**
   * Last-good ready blobs by `provider_uri`. A previously-served ready blob is
   * retained so a transient not-ready window (mid-publish) returns the last-good
   * content rather than blocking or a negative — the C10 sticky-`TS2307` defense.
   */
  private readonly lastGoodBlob = new Map<string, string>();

  constructor(storeDir: string | undefined, projectKey?: string) {
    this.storeDir = storeDir;
    this.manifestPath = storeDir === undefined ? undefined : path.join(storeDir, "manifest.json");
    this.projectKey = projectKey === undefined ? undefined : normalizePath(projectKey);
  }

  /** Whether the store dir is configured at all. */
  isAvailable(): boolean {
    return this.storeDir !== undefined;
  }

  /**
   * The manifest project entries this reader may consult. A PROJECT-SCOPED
   * reader returns only its own project's entry (matched on the normalized
   * project URI, with a case-insensitive fallback so a Windows drive-letter or
   * NTFS/APFS case difference between the Rust-written tsconfig path and
   * tsserver's `getProjectName()` still resolves — distinct projects on a
   * case-sensitive FS are disambiguated by the exact-match first pass). An
   * unscoped reader returns every entry.
   */
  private scopedProjectEntries(manifest: Manifest): ProjectEntry[] {
    if (this.projectKey === undefined) {
      return Object.values(manifest.projects);
    }
    const exact = manifest.projects[this.projectKey];
    if (exact !== undefined) {
      return [exact];
    }
    const wantNormalized = this.projectKey;
    const wantFolded = wantNormalized.toLowerCase();
    for (const [key, entry] of Object.entries(manifest.projects)) {
      const keyNormalized = normalizePath(key);
      if (keyNormalized === wantNormalized || keyNormalized.toLowerCase() === wantFolded) {
        return [entry];
      }
    }
    return [];
  }

  /**
   * Read the manifest, re-parsing only when the on-disk file's mtime/size has
   * changed since the cached read. A missing/torn manifest yields `undefined`.
   */
  readManifest(): Manifest | undefined {
    if (this.manifestPath === undefined) {
      return undefined;
    }

    let stat: fs.Stats;
    try {
      stat = fs.statSync(this.manifestPath);
    } catch {
      // No manifest yet (store not warmed) — drop any stale cache.
      this.cachedManifest = undefined;
      this.cachedStat = undefined;
      return undefined;
    }

    const mtimeMs = stat.mtimeMs;
    const size = stat.size;
    if (
      this.cachedManifest !== undefined &&
      this.cachedStat !== undefined &&
      this.cachedStat.mtimeMs === mtimeMs &&
      this.cachedStat.size === size
    ) {
      return this.cachedManifest;
    }

    let raw: string;
    try {
      raw = fs.readFileSync(this.manifestPath, "utf8");
    } catch {
      return this.cachedManifest;
    }

    let parsed: Manifest;
    try {
      parsed = JSON.parse(raw) as Manifest;
    } catch {
      // A torn write (the atomic swap should prevent this, but tolerate it):
      // keep the last good manifest rather than throwing into a tsserver hook.
      return this.cachedManifest;
    }

    if (
      parsed === null ||
      typeof parsed !== "object" ||
      typeof parsed.epoch !== "number" ||
      parsed.projects === null ||
      typeof parsed.projects !== "object"
    ) {
      return this.cachedManifest;
    }

    this.cachedManifest = parsed;
    this.cachedStat = { mtimeMs, size };
    return parsed;
  }

  /** The current published epoch, or `undefined` when the store is unavailable. */
  currentEpoch(): number | undefined {
    return this.readManifest()?.epoch;
  }

  /**
   * The owned-source set. With an explicit `projectUri`, that one project's
   * owned set (regardless of the reader's scope). Without one, the reader's
   * SCOPED project set (every project for an unscoped reader). Empty when the
   * store is unavailable or the manifest names no owned sources.
   */
  ownedSources(projectUri?: string): OwnedSource[] {
    const manifest = this.readManifest();
    if (!manifest) {
      return [];
    }
    if (projectUri !== undefined) {
      return manifest.projects[projectUri]?.owned_sources ?? [];
    }
    const all: OwnedSource[] = [];
    for (const project of this.scopedProjectEntries(manifest)) {
      all.push(...project.owned_sources);
    }
    return all;
  }

  /**
   * The `ReadyFile` entry for a provider path (the carrier companion path, e.g.
   * `…/Comp.vue.tsx`), searched within the reader's SCOPED project set, or
   * `undefined` when that project has not published the companion's content yet.
   * A project-scoped reader never reads another tsconfig's `ready_files`, so a
   * host hook can never serve a companion compiled under a foreign project's
   * options.
   */
  readyFile(providerPath: string): ReadyFile | undefined {
    const manifest = this.readManifest();
    if (!manifest) {
      return undefined;
    }
    const key = normalizePath(providerPath);
    for (const project of this.scopedProjectEntries(manifest)) {
      const entry = project.ready_files[key];
      if (entry) {
        return entry;
      }
    }
    return undefined;
  }

  /**
   * The IDE-companion provider paths of every READY `CarrierIde` carrier in the
   * reader's SCOPED project set, intersected with that project's owned-source
   * set (a ready entry is advertised only when the project also OWNS that
   * companion). This is the `getExternalFiles` membership set: a project-scoped
   * reader returns ONLY its own project's carriers, never leaking a sibling
   * tsconfig's companions into this project's Program.
   */
  readyIdeCompanions(): string[] {
    const manifest = this.readManifest();
    if (!manifest) {
      return [];
    }
    const out = new Set<string>();
    for (const project of this.scopedProjectEntries(manifest)) {
      const ownedProviders = new Set(
        project.owned_sources.map((o) => normalizePath(o.provider_uri)),
      );
      for (const [providerUri, ready] of Object.entries(project.ready_files)) {
        if (ready.role === "CarrierIde" && ownedProviders.has(normalizePath(providerUri))) {
          out.add(providerUri);
        }
      }
    }
    return [...out];
  }

  /**
   * The `ReadyFile` for the IDE companion that backs a carrier SOURCE path
   * (`Comp.vue` → its `Comp.vue.tsx` companion's ready entry), or `undefined`
   * when the path is not a carrier source or its companion is not yet published.
   *
   * `getExternalFiles` advertises the SOURCE path to tsserver (so the carrier is
   * a configured-project member under `extraFileExtensions`); tsserver then asks
   * the host for the SOURCE path's snapshot/kind/version. This maps that source
   * query to the IDE companion's ready blob so the source path is served the
   * generated TSX carrier content — the membership-identity reconciliation.
   */
  readyFileForSource(sourcePath: string): ReadyFile | undefined {
    const companion = carrierSourceToCompanion(sourcePath);
    if (companion === null) {
      return undefined;
    }
    return this.readyFile(companion);
  }

  /**
   * The IDE companion provider path that backs a carrier SOURCE path, when that
   * companion's content is published (ready). `undefined` for a non-carrier path
   * or an unready companion. The cold-read path uses the returned companion to
   * resolve a known-but-not-yet-ready source.
   */
  companionForSource(sourcePath: string): string | undefined {
    const companion = carrierSourceToCompanion(sourcePath);
    return companion === null ? undefined : companion;
  }

  /**
   * The `OwnedSource` entry for a path that may be a provider companion path OR
   * a source path — searched within the reader's SCOPED project set. Lets a host
   * hook answer `fileExists`/`getScriptKind` for a KNOWN-but-maybe-not-ready
   * companion (this project owns it, but its content is not yet published)
   * without consulting a foreign tsconfig's owned set.
   */
  ownedSourceFor(providerOrSourcePath: string): OwnedSource | undefined {
    const manifest = this.readManifest();
    if (!manifest) {
      return undefined;
    }
    const key = normalizePath(providerOrSourcePath);
    for (const project of this.scopedProjectEntries(manifest)) {
      for (const owned of project.owned_sources) {
        if (normalizePath(owned.provider_uri) === key || normalizePath(owned.source_uri) === key) {
          return owned;
        }
      }
    }
    return undefined;
  }

  /**
   * Read a content blob synchronously from `<store-dir>/<blob_rel>`. Returns
   * `undefined` when the store is unavailable or the blob is missing. A
   * successfully-read blob for `providerPath` is retained as last-good.
   */
  readBlobSync(blobRel: string, providerPath?: string): string | undefined {
    if (this.storeDir === undefined) {
      return undefined;
    }
    let content: string;
    try {
      content = fs.readFileSync(path.join(this.storeDir, blobRel), "utf8");
    } catch {
      return undefined;
    }
    if (providerPath !== undefined) {
      this.lastGoodBlob.set(normalizePath(providerPath), content);
    }
    return content;
  }

  /**
   * Read the sourcemap JSON for a ready file (for navigation remapping).
   * Returns `undefined` when the store is unavailable, the carrier carries no
   * map, or the map blob is missing/unparseable.
   */
  readMapSync(mapRel: string): unknown | undefined {
    if (this.storeDir === undefined) {
      return undefined;
    }
    let raw: string;
    try {
      raw = fs.readFileSync(path.join(this.storeDir, mapRel), "utf8");
    } catch {
      return undefined;
    }
    try {
      return JSON.parse(raw);
    } catch {
      return undefined;
    }
  }

  /** The last-good blob previously served for `providerPath`, if any. */
  lastGoodBlobFor(providerPath: string): string | undefined {
    return this.lastGoodBlob.get(normalizePath(providerPath));
  }
}

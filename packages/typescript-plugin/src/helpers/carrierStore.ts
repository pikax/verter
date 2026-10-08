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
import type { CarrierImportCompletionSnapshot } from "./pathCompletion";

import {
  CARRIER_STORE_HEAD_FILE,
  carrierJournalFile,
  carrierSnapshotFile,
  decodeCarrierJournalLine,
  parseCarrierStoreHead,
  type CarrierJournalOp,
  type CarrierStoreHead,
} from "./carrierJournal";

/**
 * The NODE ADAPTER of the shared [`CarrierStoreReader`] interface: a
 * synchronous `node:fs` reader over the Rust-published on-disk
 * content-addressed carrier-snapshot store and its incremental manifest. The
 * interface (and the manifest value types) live in `@verter/language-shared`;
 * this is the SOLE module in the plugin that touches the store filesystem —
 * every host hook reads carriers through it.
 *
 * The Rust `verter_lsp` is the sole carrier authority — it compiles every
 * framework carrier (`.vue`/`.svelte`) and publishes the result to this store
 * (content-addressed blobs + maps, advertised by journal records under a
 * monotonic `epoch`). The plugin runs inside the user's tsserver (a SEPARATE
 * process with NO shared memory), so it reads the store synchronously and never
 * compiles a carrier itself.
 *
 * ## Store layout (mirrors the Rust publish store)
 *
 * The per-workspace store dir holds `blobs/`, `maps/`, and the incremental
 * manifest described in `./carrierJournal`: `head.json` naming a generation,
 * that generation's compacted base `snapshot-<g>.json`, and its append-only
 * `journal-<g>.log`. The plugin does NOT recompute the dir — the Rust LSP
 * passes the RESOLVED dir in the plugin config (`carrierStoreDir`) with a
 * `VERTER_CARRIER_STORE_DIR` environment fallback. When neither is set the
 * store is UNAVAILABLE: the reader serves nothing for carriers (fail closed) and
 * the host hooks fall through to real disk for everything else.
 *
 * ## Manifest schema (the Rust serde shape)
 *
 * The base snapshot (and `readManifest()`'s materialized view) is:
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
 * ## Read consistency and cost
 *
 * The reader folds the published state ONCE and then follows it: every accessor
 * reads the small head and, while it names the folded generation, applies only
 * the journal records appended since the last read (the journal only grows, so
 * no publication can hide behind an unchanged stat). A compaction changes the
 * generation and costs one base load. A missing or torn store is tolerated —
 * every accessor returns `undefined`/empty (or the last good fold) rather than
 * throwing into a tsserver hook. The Rust two-phase publish guarantees every
 * `ready_files` entry a committed record names has its blob present on disk, so
 * a blob read for a ready file never observes a half-written file.
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

/** One project's folded rows plus its canonical-path indexes, all maintained per op. */
interface FoldedProject {
  /** Source → its owned rows, in owned order (a re-put source moves to the end). */
  owned: Map<string, OwnedSource[]>;
  /** Provider URI → ready entry. */
  ready: Map<string, ReadyFile>;
  /** Canonical provider path → ready entry. */
  readyByCanonical: Map<string, ReadyFile>;
  /** Canonical source OR provider path → owned rows naming it, in owned order. */
  ownedByCanonical: Map<string, OwnedSource[]>;
}

/** A folded generation at a journal byte offset. */
interface FoldedStore {
  generation: number;
  instance: string;
  /** Bytes of the journal already applied (always a line end). */
  offset: number;
  epoch: number;
  hostVersion: string;
  projects: Map<string, FoldedProject>;
}

function emptyProject(): FoldedProject {
  return {
    owned: new Map(),
    ready: new Map(),
    readyByCanonical: new Map(),
    ownedByCanonical: new Map(),
  };
}

function isNotFound(error: unknown): boolean {
  return (error as { code?: unknown } | null)?.code === "ENOENT";
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
 * carrier lookup (`readyFile` / `ownedSourceFor` / ready membership) then
 * consults ONLY that project's manifest entry, never another tsconfig's. In a
 * multi-tsconfig workspace this prevents serving / advertising a carrier
 * compiled under one tsconfig's options (`paths`/`types`/`lib`) to a different
 * tsconfig. An UNSCOPED reader (`projectKey` omitted) consults every project —
 * used only by call sites that legitimately span projects.
 */
export class DiskCarrierStoreReader implements CarrierStoreReader {
  private readonly storeDir: string | undefined;
  /**
   * The normalized project identity this reader is scoped to (the configured
   * project's `getProjectName()` — the manifest `projects` key, forward-slash
   * normalized). `undefined` ⇒ the reader spans every project in the manifest.
   */
  private readonly projectKey: string | undefined;
  private readonly useCaseSensitiveFileNames: boolean;
  /** The folded published state; `undefined` before anything was published. */
  private folded: FoldedStore | undefined;
  /** The materialized `readManifest()` view of `folded`, dropped on any change. */
  private materialized: Manifest | undefined;
  /**
   * Last-good ready blobs by `provider_uri`. A previously-served ready blob is
   * retained so a transient not-ready window (mid-publish) returns the last-good
   * content rather than blocking or a negative — the C10 sticky-`TS2307` defense.
   */
  private readonly lastGoodBlob = new Map<string, string>();
  /** Parsed maps are immutable content-addressed artifacts. */
  private readonly parsedMaps = new Map<string, unknown>();

  constructor(storeDir: string | undefined, projectKey?: string, useCaseSensitiveFileNames = true) {
    this.storeDir = storeDir;
    this.projectKey = projectKey === undefined ? undefined : normalizePath(projectKey);
    this.useCaseSensitiveFileNames = useCaseSensitiveFileNames;
  }

  canonicalPath(fileName: string): string {
    const normalized = normalizePath(fileName);
    return this.useCaseSensitiveFileNames ? normalized : normalized.toLowerCase();
  }

  /** Whether the store dir is configured at all. */
  isAvailable(): boolean {
    return this.storeDir !== undefined;
  }

  // ── folding ──────────────────────────────────────────────────────────────

  private indexOwnedRow(project: FoldedProject, row: OwnedSource): void {
    const keys = new Set([
      this.canonicalPath(row.source_uri),
      this.canonicalPath(row.provider_uri),
    ]);
    for (const key of keys) {
      const rows = project.ownedByCanonical.get(key);
      if (rows === undefined) project.ownedByCanonical.set(key, [row]);
      else rows.push(row);
    }
  }

  private unindexOwnedRow(project: FoldedProject, row: OwnedSource): void {
    const keys = new Set([
      this.canonicalPath(row.source_uri),
      this.canonicalPath(row.provider_uri),
    ]);
    for (const key of keys) {
      const rows = project.ownedByCanonical.get(key);
      if (rows === undefined) continue;
      const remaining = rows.filter((candidate) => candidate !== row);
      if (remaining.length === 0) project.ownedByCanonical.delete(key);
      else project.ownedByCanonical.set(key, remaining);
    }
  }

  private putOwned(project: FoldedProject, sourceUri: string, rows: OwnedSource[]): void {
    this.deleteOwned(project, sourceUri);
    project.owned.set(sourceUri, rows);
    for (const row of rows) this.indexOwnedRow(project, row);
  }

  private deleteOwned(project: FoldedProject, sourceUri: string): void {
    const prior = project.owned.get(sourceUri);
    if (prior === undefined) return;
    project.owned.delete(sourceUri);
    for (const row of prior) this.unindexOwnedRow(project, row);
  }

  private putReady(project: FoldedProject, providerUri: string, file: ReadyFile): void {
    project.ready.set(providerUri, file);
    project.readyByCanonical.set(this.canonicalPath(providerUri), file);
  }

  private deleteReady(project: FoldedProject, providerUri: string): void {
    const prior = project.ready.get(providerUri);
    if (prior === undefined) return;
    project.ready.delete(providerUri);
    const key = this.canonicalPath(providerUri);
    if (project.readyByCanonical.get(key) === prior) project.readyByCanonical.delete(key);
  }

  private applyOp(store: FoldedStore, op: CarrierJournalOp): void {
    let project = store.projects.get(op.project);
    if (project === undefined) {
      if (op.op === "owned_del" || op.op === "ready_del") return;
      project = emptyProject();
      store.projects.set(op.project, project);
    }
    switch (op.op) {
      case "project_put":
        return;
      case "owned_clear":
        project.owned.clear();
        project.ownedByCanonical.clear();
        return;
      case "owned_put":
        this.putOwned(project, op.source_uri, op.rows);
        return;
      case "owned_del":
        this.deleteOwned(project, op.source_uri);
        return;
      case "ready_put":
        this.putReady(project, op.provider_uri, op.file);
        return;
      case "ready_del":
        this.deleteReady(project, op.provider_uri);
        return;
    }
  }

  /** Fold a base snapshot; owned rows group per source in first-appearance order. */
  private foldBase(head: CarrierStoreHead, manifest: Manifest): FoldedStore {
    const store: FoldedStore = {
      generation: head.generation,
      instance: head.instance,
      offset: 0,
      epoch: manifest.epoch,
      hostVersion: manifest.host_version,
      projects: new Map(),
    };
    for (const [projectUri, entry] of Object.entries(manifest.projects)) {
      const project = emptyProject();
      const grouped = new Map<string, OwnedSource[]>();
      for (const row of entry.owned_sources ?? []) {
        const rows = grouped.get(row.source_uri);
        if (rows === undefined) grouped.set(row.source_uri, [row]);
        else rows.push(row);
      }
      for (const [sourceUri, rows] of grouped) this.putOwned(project, sourceUri, rows);
      for (const [providerUri, ready] of Object.entries(entry.ready_files ?? {})) {
        this.putReady(project, providerUri, ready);
      }
      store.projects.set(projectUri, project);
    }
    return store;
  }

  /**
   * Apply every complete, verifying record appended past `store.offset`, reading
   * ONLY those bytes. Returns `"reload"` when the journal is gone or shrank below
   * the consumed offset; otherwise stops (fail closed, offset unchanged) at the
   * first line that does not verify — a torn tail still being appended, or
   * corruption a later compaction replaces.
   */
  private tail(dir: string, store: FoldedStore): "ok" | "reload" {
    let fd: number;
    try {
      fd = fs.openSync(path.join(dir, carrierJournalFile(store.generation)), "r");
    } catch {
      return "reload";
    }
    try {
      const size = fs.fstatSync(fd).size;
      if (size < store.offset) return "reload";
      if (size === store.offset) return "ok";
      const bytes = Buffer.allocUnsafe(size - store.offset);
      let read = 0;
      while (read < bytes.length) {
        const n = fs.readSync(fd, bytes, read, bytes.length - read, store.offset + read);
        if (n === 0) break;
        read += n;
      }
      let at = 0;
      while (at < read) {
        const newline = bytes.indexOf(0x0a, at);
        if (newline < 0 || newline >= read) break;
        const record = decodeCarrierJournalLine(bytes, at, newline);
        if (record === undefined || record.epoch <= store.epoch) break;
        for (const op of record.ops) this.applyOp(store, op);
        store.epoch = record.epoch;
        store.offset += newline + 1 - at;
        this.materialized = undefined;
        at = newline + 1;
      }
      return "ok";
    } catch {
      return "ok";
    } finally {
      fs.closeSync(fd);
    }
  }

  /** Load generation `head.generation` from scratch: its base, then its journal. */
  private load(dir: string, head: CarrierStoreHead): FoldedStore | undefined {
    let parsed: Manifest;
    try {
      parsed = JSON.parse(
        fs.readFileSync(path.join(dir, carrierSnapshotFile(head.generation)), "utf8"),
      ) as Manifest;
    } catch {
      return undefined;
    }
    if (
      parsed === null ||
      typeof parsed !== "object" ||
      typeof parsed.epoch !== "number" ||
      parsed.projects === null ||
      typeof parsed.projects !== "object"
    ) {
      return undefined;
    }
    const store = this.foldBase(head, parsed);
    return this.tail(dir, store) === "ok" ? store : undefined;
  }

  /**
   * Bring the folded state up to the published state: read the small head; when
   * it still names the folded generation, apply only the journal bytes appended
   * since the last read; reload a base only when a compaction (or a re-created
   * store) changed the generation. Every accessor reads through here, so each
   * host-hook call costs one head read plus the new records — never a whole
   * manifest. A missing head yields `undefined`; an unreadable head, base or
   * journal keeps the last good fold (never throws into a tsserver hook).
   */
  private readFolded(): FoldedStore | undefined {
    const dir = this.storeDir;
    if (dir === undefined) return undefined;
    for (let attempt = 0; attempt < 3; attempt++) {
      let raw: string;
      try {
        raw = fs.readFileSync(path.join(dir, CARRIER_STORE_HEAD_FILE), "utf8");
      } catch (error) {
        if (isNotFound(error)) {
          // No store yet (not warmed) — drop any stale fold.
          this.folded = undefined;
          this.materialized = undefined;
        }
        return this.folded;
      }
      const head = parseCarrierStoreHead(raw);
      if (head === undefined) return this.folded;
      const current = this.folded;
      if (
        current !== undefined &&
        current.generation === head.generation &&
        current.instance === head.instance &&
        this.tail(dir, current) === "ok"
      ) {
        return current;
      }
      const loaded = this.load(dir, head);
      if (loaded !== undefined) {
        this.folded = loaded;
        this.materialized = undefined;
        return loaded;
      }
      // The generation was retired between the head read and the base open (a
      // concurrent compaction): re-read the head.
    }
    return this.folded;
  }

  /**
   * The folded projects this reader may consult. A PROJECT-SCOPED reader returns
   * only its own project (matched on the normalized project URI, with a
   * case-insensitive fallback so a Windows drive-letter or NTFS/APFS case
   * difference between the Rust-written tsconfig path and tsserver's
   * `getProjectName()` still resolves — distinct projects on a case-sensitive FS
   * are disambiguated by the exact-match first pass). An unscoped reader returns
   * every project.
   */
  private scopedProjects(store: FoldedStore): FoldedProject[] {
    if (this.projectKey === undefined) {
      return [...store.projects.values()];
    }
    const exact = store.projects.get(this.projectKey);
    if (exact !== undefined) {
      return [exact];
    }
    const wantNormalized = this.projectKey;
    const wantFolded = wantNormalized.toLowerCase();
    for (const [key, project] of store.projects) {
      const keyNormalized = normalizePath(key);
      if (keyNormalized === wantNormalized || keyNormalized.toLowerCase() === wantFolded) {
        return [project];
      }
    }
    return [];
  }

  private static ownedRows(project: FoldedProject): OwnedSource[] {
    const rows: OwnedSource[] = [];
    for (const sourceRows of project.owned.values()) rows.push(...sourceRows);
    return rows;
  }

  /** Whether an owned row of `project` names the provider at canonical `key`. */
  private ownsProvider(project: FoldedProject, key: string): boolean {
    return (
      project.ownedByCanonical
        .get(key)
        ?.some((owned) => this.canonicalPath(owned.provider_uri) === key) === true
    );
  }

  // ── the reader contract ──────────────────────────────────────────────────

  /**
   * The published state materialized as a `Manifest` (cached until the next
   * applied record), or `undefined` when nothing is published or the store is
   * unavailable. Host hooks read through the indexed fold instead; this view
   * serves callers that need the whole manifest value.
   */
  readManifest(): Manifest | undefined {
    const store = this.readFolded();
    if (store === undefined) return undefined;
    if (this.materialized !== undefined) return this.materialized;
    const projects: Record<string, ProjectEntry> = {};
    for (const [projectUri, project] of store.projects) {
      projects[projectUri] = {
        owned_sources: DiskCarrierStoreReader.ownedRows(project),
        ready_files: Object.fromEntries(project.ready),
      };
    }
    this.materialized = { epoch: store.epoch, host_version: store.hostVersion, projects };
    return this.materialized;
  }

  /** The current published epoch, or `undefined` when the store is unavailable. */
  currentEpoch(): number | undefined {
    return this.readFolded()?.epoch;
  }

  /**
   * The owned-source set. With an explicit `projectUri`, that one project's
   * owned set (regardless of the reader's scope). Without one, the reader's
   * SCOPED project set (every project for an unscoped reader). Empty when the
   * store is unavailable or the manifest names no owned sources.
   */
  ownedSources(projectUri?: string): OwnedSource[] {
    const store = this.readFolded();
    if (!store) {
      return [];
    }
    if (projectUri !== undefined) {
      const project = store.projects.get(projectUri);
      return project === undefined ? [] : DiskCarrierStoreReader.ownedRows(project);
    }
    const all: OwnedSource[] = [];
    for (const project of this.scopedProjects(store)) {
      all.push(...DiskCarrierStoreReader.ownedRows(project));
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
    const store = this.readFolded();
    if (!store) {
      return undefined;
    }
    const normalized = normalizePath(providerPath);
    const key = this.canonicalPath(normalized);
    for (const project of this.scopedProjects(store)) {
      const entry = project.ready.get(normalized) ?? project.readyByCanonical.get(key);
      if (entry) {
        return entry;
      }
    }
    return undefined;
  }

  /** Ready companion identities for non-editor consumers of the shared reader contract. */
  readyIdeCompanions(): string[] {
    const store = this.readFolded();
    if (!store) {
      return [];
    }
    const out = new Set<string>();
    for (const project of this.scopedProjects(store)) {
      for (const [providerUri, ready] of project.ready) {
        if (
          ready.role === "CarrierIde" &&
          this.ownsProvider(project, this.canonicalPath(providerUri))
        ) {
          out.add(providerUri);
        }
      }
    }
    return [...out];
  }

  /**
   * Content identities for every ready companion in this reader's project
   * scope. The plugin keeps its own last-invalidated snapshot of this map so a
   * publication refresh reloads only ScriptInfos whose generated bytes changed;
   * membership-only additions/removals remain the configured-project root
   * reloader's responsibility.
   */
  readyFileVersions(): Map<string, string> {
    const store = this.readFolded();
    const out = new Map<string, string>();
    if (!store) {
      return out;
    }
    for (const project of this.scopedProjects(store)) {
      for (const [providerUri, ready] of project.ready) {
        const provider = normalizePath(providerUri);
        if (this.ownsProvider(project, this.canonicalPath(provider))) {
          out.set(provider, `${ready.version}:${ready.content_hash}`);
        }
      }
    }
    return out;
  }

  /**
   * Source identities of every READY `CarrierIde` in the reader's scoped project
   * set. A source is advertised only when its owned provider has a ready carrier
   * blob. Keeping the opened source identity in the Program lets the host hooks
   * substitute generated content without creating a second document identity.
   */
  readyIdeSources(): string[] {
    const store = this.readFolded();
    if (!store) {
      return [];
    }
    const out = new Set<string>();
    for (const project of this.scopedProjects(store)) {
      for (const rows of project.owned.values()) {
        for (const owned of rows) {
          if (
            owned.role === "CarrierIde" &&
            project.readyByCanonical.get(this.canonicalPath(owned.provider_uri))?.role ===
              "CarrierIde"
          ) {
            out.add(owned.source_uri);
          }
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
    const companion = this.companionForSource(sourcePath);
    if (companion === undefined) {
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
    const source = normalizePath(sourcePath);
    const sourceKey = this.canonicalPath(source);
    const store = this.readFolded();
    if (store) {
      for (const project of this.scopedProjects(store)) {
        const ownedIde = project.ownedByCanonical
          .get(sourceKey)
          ?.find(
            (owned) =>
              owned.role === "CarrierIde" && this.canonicalPath(owned.source_uri) === sourceKey,
          );
        if (ownedIde) {
          return normalizePath(ownedIde.provider_uri);
        }
      }
    }
    const companion = carrierSourceToCompanion(source);
    return companion === null ? undefined : companion;
  }

  /**
   * ONE-read snapshot for a single import-path completion request: the reader's
   * SCOPED owned-source rows plus the canonical provider-path set of ready
   * `CarrierApi` surfaces, both taken from the SAME folded state. Completion
   * runs per-candidate policy + readiness checks on the keystroke path; this
   * snapshot bounds the whole request at exactly one store read regardless of
   * how many carriers the directory holds.
   */
  importCompletionSnapshot(): CarrierImportCompletionSnapshot {
    const store = this.readFolded();
    if (!store) {
      return { ownedSources: [], readyApiProviders: new Set() };
    }
    const ownedSources: OwnedSource[] = [];
    const readyApiProviders = new Set<string>();
    for (const project of this.scopedProjects(store)) {
      ownedSources.push(...DiskCarrierStoreReader.ownedRows(project));
      for (const [providerUri, ready] of project.ready) {
        if (ready.role === "CarrierApi") {
          readyApiProviders.add(this.canonicalPath(providerUri));
        }
      }
    }
    return { ownedSources, readyApiProviders };
  }

  /**
   * The `OwnedSource` entry for a path that may be a provider companion path OR
   * a source path — searched within the reader's SCOPED project set. Lets a host
   * hook answer `fileExists`/`getScriptKind` for a KNOWN-but-maybe-not-ready
   * companion (this project owns it, but its content is not yet published)
   * without consulting a foreign tsconfig's owned set.
   */
  ownedSourceFor(providerOrSourcePath: string): OwnedSource | undefined {
    const store = this.readFolded();
    if (!store) {
      return undefined;
    }
    const key = this.canonicalPath(providerOrSourcePath);
    for (const project of this.scopedProjects(store)) {
      const owned = project.ownedByCanonical.get(key)?.[0];
      if (owned !== undefined) return owned;
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
      this.lastGoodBlob.set(this.canonicalPath(providerPath), content);
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
    const cached = this.parsedMaps.get(mapRel);
    if (cached !== undefined) return cached;
    let raw: string;
    try {
      raw = fs.readFileSync(path.join(this.storeDir, mapRel), "utf8");
    } catch {
      return undefined;
    }
    try {
      const parsed: unknown = JSON.parse(raw);
      if (parsed !== undefined) this.parsedMaps.set(mapRel, parsed);
      return parsed;
    } catch {
      return undefined;
    }
  }

  /** The last-good blob previously served for `providerPath`, if any. */
  lastGoodBlobFor(providerPath: string): string | undefined {
    return this.lastGoodBlob.get(this.canonicalPath(providerPath));
  }
}

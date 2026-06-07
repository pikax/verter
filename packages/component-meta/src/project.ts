/**
 * First-class public API for component metadata extraction.
 *
 * ```ts
 * import { openComponentMetaSession, shutdownMetaRuntime } from "@verter/component-meta"
 *
 * const session = await openComponentMetaSession({ root: "./my-app", tsconfig: "./tsconfig.json" })
 * const meta = await session.getComponentMeta("./src/Button.vue")
 * session.close() // optional, resources are pooled
 *
 * // In tests or app shutdown:
 * shutdownMetaRuntime()
 * ```
 */

import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { resolve } from "node:path";
import { mapComponentMeta } from "./compat/checker.js";
import type { CheckerWorkspace } from "./compat/checker.js";
import { projectDeclaredOnlyNativeResult } from "./compat/native-projection.js";
import type { MetaCheckerOptions, VolarComponentMeta } from "./compat/types.js";
import {
  nativeComponentMetaToComponentMeta,
  nativeTypeRegistryToMap,
} from "./native-component-meta.js";
import type { NativeComponentMetaResult } from "./native-component-meta.js";
import {
  createMetaRuntime,
  computeEngineKey,
  extractPathAliases,
  getMetaRuntime,
  normalizePath,
  parseTsconfig,
  resolvePath,
  shutdownMetaRuntime as _shutdownMetaRuntime,
  stableHash,
  stableSelectiveConfigHash,
} from "./runtime/index.js";
import type {
  BootstrapFn,
  EngineKeyInput,
  MetaRuntimeImpl,
  NativeMetaProject,
  ProjectSession,
} from "./runtime/index.js";

export type ComponentMetaSessionConfig =
  | {
      root: string;
      tsconfig: string;
      config?: never;
      backend?: "napi" | "wasm";
    }
  | {
      root: string;
      config: Record<string, unknown>;
      tsconfig?: never;
      backend?: "napi" | "wasm";
    };

export class ComponentMetaSession {
  private readonly _session: ProjectSession;
  private readonly _root: string;
  private readonly _options: MetaCheckerOptions;
  private readonly _workspace: CheckerWorkspace | undefined;
  private readonly _runtime: MetaRuntimeImpl;
  private readonly _ownsRuntime: boolean;
  private readonly _touchedFiles = new Set<string>();
  private readonly _baseFiles = new Set<string>();
  private readonly _deletedFiles = new Set<string>();
  private _closed = false;

  /** @internal */
  constructor(
    session: ProjectSession,
    root: string,
    options?: MetaCheckerOptions,
    workspace?: CheckerWorkspace,
    runtime?: MetaRuntimeImpl,
    ownsRuntime = false,
  ) {
    this._session = session;
    this._root = root;
    this._options = options ?? {};
    this._workspace = workspace;
    this._runtime = runtime ?? getMetaRuntime();
    this._ownsRuntime = ownsRuntime;
  }

  private ensureOpen(): void {
    if (this._closed) throw new Error("ComponentMetaSession is closed");
    if (this._session.closed || this._session.engine.state !== "active") {
      throw new Error("ComponentMetaSession session was invalidated");
    }
  }

  updateFile(filePath: string, source: string): void {
    this.ensureOpen();
    const abs = resolvePath(this._root, filePath);
    this._touchedFiles.add(abs);
    this._baseFiles.delete(abs);
    this._deletedFiles.delete(abs);
    this._session.upsert(abs, source);
  }

  deleteFile(filePath: string): void {
    this.ensureOpen();
    const abs = resolvePath(this._root, filePath);
    this._touchedFiles.add(abs);
    this._baseFiles.delete(abs);
    this._deletedFiles.add(abs);
    this._session.delete(abs);
  }

  async reload(): Promise<void> {
    this.ensureOpen();
    if (!this._workspace) return;

    for (const fileId of this._touchedFiles) {
      const content = await this._workspace.readFile(fileId);
      this.ensureOpen();
      if (content !== null) {
        this._deletedFiles.delete(fileId);
        this._session.upsert(fileId, content);
      } else {
        this._deletedFiles.add(fileId);
        this._session.delete(fileId);
      }
    }

    for (const fileId of Array.from(this._baseFiles)) {
      const loaded = this._session.refreshBaseFile(fileId);
      this.ensureOpen();
      if (!loaded) {
        this._baseFiles.delete(fileId);
      }
    }
  }

  clearCaches(): void {
    this.ensureOpen();
    this._session.engine.clearCaches();
  }

  private ensureNativeMetaFile(filePath: string): string | undefined {
    const abs = resolvePath(this._root, filePath);

    if (this._deletedFiles.has(abs)) {
      return undefined;
    }

    if (!this._session.hasFile(abs) && this._workspace && this._session.ensureBaseFile(abs)) {
      this._baseFiles.add(abs);
    }

    if (this._session.getEffectiveSource(abs) === undefined) {
      return undefined;
    }

    return abs;
  }

  private loadCompatNativeMeta(filePath: string): NativeComponentMetaResult | undefined {
    const abs = this.ensureNativeMetaFile(filePath);
    if (!abs) {
      return undefined;
    }

    const fullNativeMeta = this._session.getComponentMeta(abs) as NativeComponentMetaResult | null;
    return projectDeclaredOnlyNativeResult(fullNativeMeta) ?? undefined;
  }

  private loadResolvedNativeMeta(filePath: string): NativeComponentMetaResult | undefined {
    const abs = this.ensureNativeMetaFile(filePath);
    if (!abs) {
      return undefined;
    }

    const getResolvedComponentMeta = (
      this._session as {
        getResolvedComponentMeta?: ProjectSession["getResolvedComponentMeta"];
      }
    ).getResolvedComponentMeta;
    if (typeof getResolvedComponentMeta !== "function") {
      throw new Error("Resolved component-meta query is unavailable on the active native session");
    }

    const nativeMeta = getResolvedComponentMeta.call(this._session, abs);
    if (!nativeMeta) {
      return undefined;
    }

    return nativeMeta as NativeComponentMetaResult;
  }

  async getComponentMeta(filePath: string): Promise<VolarComponentMeta> {
    this.ensureOpen();
    const nativeMeta = this.loadCompatNativeMeta(filePath);
    if (!nativeMeta) {
      return { type: 0, props: [], events: [], slots: [], exposed: [] };
    }

    return mapComponentMeta(
      nativeComponentMetaToComponentMeta(nativeMeta),
      this._options,
      nativeTypeRegistryToMap(nativeMeta),
    );
  }

  /**
   * Batch surface for {@link getComponentMeta}. All `filePaths` resolve
   * under one shared overlay view and a single scheduler dispatch on
   * the native side; host-owned admission caches
   * (`MaterializeStructureDb`, `ComponentMetaResultDb`,
   * `SemanticGraphStore`) are shared across the batch.
   *
   * Returns one slot per input in input order — a fully-projected
   * `VolarComponentMeta` for successful slots, the empty-meta default
   * for missing canonicals / per-id failures.
   */
  async getComponentMetaBatch(filePaths: string[]): Promise<VolarComponentMeta[]> {
    this.ensureOpen();
    // Resolve each input to its canonical absolute path; preserve
    // input order positionally.
    const canonicalIds: string[] = filePaths.map((p) => {
      const abs = this.ensureNativeMetaFile(p);
      // ensureNativeMetaFile returns undefined when the file cannot
      // be located. Pass the input through verbatim — the native
      // batch will surface a missing-canonical slot.
      return abs ?? p;
    });
    const sessionWithBatch = this._session as {
      getComponentMetaBatch?: (canonicalIds: string[]) => Array<unknown | null>;
    };
    const getComponentMetaBatch = sessionWithBatch.getComponentMetaBatch;
    if (typeof getComponentMetaBatch !== "function") {
      // Fallback: per-id loop. Stays positional in input order and
      // matches the batch semantics observable from JS, at the cost
      // of N scheduler dispatches. Preserves backward compatibility
      // for native bindings that have not yet exposed the batch
      // surface.
      const results: VolarComponentMeta[] = [];
      for (const p of filePaths) {
        results.push(await this.getComponentMeta(p));
      }
      return results;
    }
    const raw = getComponentMetaBatch.call(this._session, canonicalIds);
    return raw.map((nativeMeta) => {
      if (!nativeMeta) {
        return { type: 0, props: [], events: [], slots: [], exposed: [] };
      }
      const declaredOnly = projectDeclaredOnlyNativeResult(nativeMeta as NativeComponentMetaResult);
      if (!declaredOnly) {
        return { type: 0, props: [], events: [], slots: [], exposed: [] };
      }
      return mapComponentMeta(
        nativeComponentMetaToComponentMeta(declaredOnly),
        this._options,
        nativeTypeRegistryToMap(declaredOnly),
      );
    });
  }

  /**
   * Selective surface. Returns the
   * `verter.v1.ComponentMetaSurface` proto bytes — eager scalars
   * combined with `NamedTypeHandle` for every type-bearing field.
   * Consumers walk one layer at a time via {@link
   * getComponentMetaTypeExpansion}.
   *
   * Throws when the canonical does not resolve to a component, or
   * when the bridge surfaced a typed error envelope.
   */
  async getComponentMetaSurface(filePath: string): Promise<Buffer> {
    this.ensureOpen();
    const abs = resolve(filePath);
    const surface = this._session.getComponentMetaSurface(abs);
    if (!surface) throw new Error(`no surface for ${filePath}`);
    return surface;
  }

  /**
   * Selective surface. Resolves a
   * `verter.v1.TypeHandle` to a one-layer
   * `verter.v1.TypeExpansion`. Caller pre-encodes the handle via the
   * proto module. Returned bytes carry an error envelope (first byte
   * `0xFF` -> `TypeHandleError`) if the handle is stale; otherwise
   * the bytes decode as `verter.v1.TypeExpansion`.
   */
  async getComponentMetaTypeExpansion(handleBuf: Buffer, depth?: number): Promise<Buffer> {
    this.ensureOpen();
    return this._session.getComponentMetaTypeExpansion(handleBuf, depth);
  }

  async getNativeComponentMeta(filePath: string): Promise<NativeComponentMetaResult | undefined> {
    this.ensureOpen();
    return this.loadResolvedNativeMeta(filePath);
  }

  async getExportNames(_filePath: string): Promise<string[]> {
    this.ensureOpen();
    return ["default"];
  }

  close(): void {
    if (this._closed) return;
    this._closed = true;
    this._touchedFiles.clear();
    this._baseFiles.clear();
    this._deletedFiles.clear();
    this._runtime.closeSession(this._session);
    if (this._ownsRuntime) {
      this._runtime.shutdownNow();
    }
  }
}

function loadNative(): any {
  const _require = typeof require === "function" ? require : createRequire(import.meta.url);
  return _require("@verter/native");
}

function hashTsconfigConfig(tsconfigPath: string): string {
  try {
    const raw = readFileSync(tsconfigPath, "utf8");
    const stripped = raw.replace(/\/\/.*$/gm, "").replace(/\/\*[\s\S]*?\*\//g, "");
    return stableSelectiveConfigHash(JSON.parse(stripped));
  } catch {
    return stableHash({ tsconfigPath: resolvePath(tsconfigPath) });
  }
}

function buildEngineKeyInput(
  options: ComponentMetaSessionConfig,
  checkerOptions?: MetaCheckerOptions,
): EngineKeyInput {
  const root = resolvePath(options.root);
  const tsconfigPath = options.tsconfig ? resolvePath(options.tsconfig) : undefined;

  return {
    backend: options.backend ?? "napi",
    root,
    configKind: options.tsconfig ? "tsconfig" : "inline",
    tsconfigPath,
    configHash: options.config
      ? stableSelectiveConfigHash(options.config)
      : hashTsconfigConfig(resolvePath(options.tsconfig)),
    nativeFlags: {
      analysisLevel: "full",
      auditEnabled: checkerOptions?.logging?.audit ?? false,
    },
  };
}

async function openComponentMetaSessionInternal(
  options: ComponentMetaSessionConfig,
  checkerOptions?: MetaCheckerOptions,
): Promise<ComponentMetaSession> {
  const runtime =
    checkerOptions?.runtimeMode === "dedicated" ? createMetaRuntime() : getMetaRuntime();
  const ownsRuntime = checkerOptions?.runtimeMode === "dedicated";
  const root = resolvePath(options.root);
  const native = loadNative();
  const workspace: CheckerWorkspace = new native.Workspace([root]);
  const parsedConfig = options.tsconfig
    ? await parseTsconfig(resolvePath(options.tsconfig), workspace)
    : null;
  const input = buildEngineKeyInput(options, checkerOptions);

  const bootstrap: BootstrapFn = async () => {
    const config = {
      devMode: false,
      analysisLevel: "full",
      auditEnabled: checkerOptions?.logging?.audit ?? false,
    };
    const nativeProject: NativeMetaProject = native.MetaProject.withWorkspace(config, workspace);

    if (options.tsconfig) {
      if (parsedConfig) {
        const aliases = extractPathAliases(parsedConfig.config, normalizePath(root));
        workspace.configureProjects([aliases]);
      }
    } else if (options.config) {
      const aliases = extractPathAliases(options.config, normalizePath(root));
      workspace.configureProjects([aliases]);
    }

    // Selective loading: no eager preload. Files are loaded on-demand
    // in getComponentMeta() when the file is first requested.
    return { nativeProject, baseFileIds: [] };
  };

  const engine = await runtime.getOrCreateEngine(input, bootstrap);
  const session = runtime.openSession(engine);
  return new ComponentMetaSession(session, root, checkerOptions, workspace, runtime, ownsRuntime);
}

/**
 * Open a component-meta session.
 */
export async function openComponentMetaSession(
  config: ComponentMetaSessionConfig,
  checkerOptions?: MetaCheckerOptions,
): Promise<ComponentMetaSession> {
  return openComponentMetaSessionInternal(config, checkerOptions);
}

export function evictComponentMetaSession(
  options: ComponentMetaSessionConfig,
  checkerOptions?: MetaCheckerOptions,
): void {
  const key = computeEngineKey(buildEngineKeyInput(options, checkerOptions));
  getMetaRuntime().evictEngine(key);
}

export function shutdownMetaRuntime(): void {
  _shutdownMetaRuntime();
}

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
import { mapComponentMeta } from "./compat/checker.js";
import type { CheckerWorkspace } from "./compat/checker.js";
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

export type TypeExpansionBackend = "verter" | "tsserver" | "tsgo" | "auto";

export type ComponentMetaSessionConfig =
  | {
      root: string;
      tsconfig: string;
      config?: never;
      backend?: "napi" | "wasm";
      typeExpansionBackend?: TypeExpansionBackend;
    }
  | {
      root: string;
      config: Record<string, unknown>;
      tsconfig?: never;
      backend?: "napi" | "wasm";
      typeExpansionBackend?: TypeExpansionBackend;
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

    const nativeMeta =
      typeof this._session.getDeclaredComponentMeta === "function"
        ? this._session.getDeclaredComponentMeta(abs)
        : this._session.getComponentMeta(abs);
    if (!nativeMeta) {
      return undefined;
    }

    return nativeMeta as NativeComponentMetaResult;
  }

  private loadResolvedNativeMeta(filePath: string): NativeComponentMetaResult | undefined {
    const abs = this.ensureNativeMetaFile(filePath);
    if (!abs) {
      return undefined;
    }

    const session = this._session as ProjectSession & {
      getResolvedComponentMeta?: (canonicalId: string) => unknown | null;
    };
    const nativeMeta = session.getResolvedComponentMeta
      ? session.getResolvedComponentMeta(abs)
      : this._session.getComponentMeta(abs);
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

function buildEngineKeyInput(options: ComponentMetaSessionConfig): EngineKeyInput {
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
    nativeFlags: { analysisLevel: "full" },
    typeExpansionBackend: options.typeExpansionBackend ?? "verter",
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
  const input = buildEngineKeyInput(options);

  const bootstrap: BootstrapFn = async () => {
    const config = {
      devMode: false,
      analysisLevel: "full",
      typeExpansionBackend: options.typeExpansionBackend ?? "verter",
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
 * Open a component-meta session with explicit backend selection.
 *
 * This is the preferred API over `openMetaProject()`. It supports
 * `typeExpansionBackend` for choosing between Verter, tsserver, TSGO, or auto.
 */
export async function openComponentMetaSession(
  config: ComponentMetaSessionConfig,
  checkerOptions?: MetaCheckerOptions,
): Promise<ComponentMetaSession> {
  const normalizedConfig: ComponentMetaSessionConfig = config.tsconfig
    ? {
        root: config.root,
        tsconfig: config.tsconfig,
        backend: config.backend,
        typeExpansionBackend: config.typeExpansionBackend,
      }
    : {
        root: config.root,
        config: config.config ?? {},
        backend: config.backend,
        typeExpansionBackend: config.typeExpansionBackend,
      };
  return openComponentMetaSessionInternal(normalizedConfig, checkerOptions);
}

export function evictComponentMetaSession(options: ComponentMetaSessionConfig): void {
  const key = computeEngineKey(buildEngineKeyInput(options));
  getMetaRuntime().evictEngine(key);
}

export function shutdownMetaRuntime(): void {
  _shutdownMetaRuntime();
}

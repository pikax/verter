/**
 * First-class public API for component metadata extraction.
 *
 * ```ts
 * import { openMetaProject, shutdownMetaRuntime } from "@verter/component-meta"
 *
 * const project = await openMetaProject({ root: "./my-app", tsconfig: "./tsconfig.json" })
 * const meta = await project.getComponentMeta("./src/Button.vue")
 * project.close() // optional, resources are pooled
 *
 * // In tests or app shutdown:
 * shutdownMetaRuntime()
 * ```
 */

import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, resolve } from "node:path";
import { extractComponentMeta, buildTypeRegistry } from "./extractor.js";
import { mapComponentMeta } from "./compat/checker.js";
import type { CheckerWorkspace } from "./compat/checker.js";
import type { MetaCheckerOptions, VolarComponentMeta } from "./compat/types.js";
import {
  computeEngineKey,
  discoverVueFiles,
  extractPathAliases,
  getMetaRuntime,
  normalizePath,
  parseTsconfig,
  shutdownMetaRuntime as _shutdownMetaRuntime,
  stableHash,
} from "./runtime/index.js";
import type {
  BootstrapFn,
  EngineKeyInput,
  MetaRuntimeImpl,
  NativeMetaProject,
  ProjectSession,
} from "./runtime/index.js";

export type MetaProjectConfig =
  | { root: string; tsconfig: string; config?: never; backend?: "napi" | "wasm" }
  | { root: string; config: Record<string, unknown>; tsconfig?: never; backend?: "napi" | "wasm" };

export class MetaProject {
  private readonly _session: ProjectSession;
  private readonly _root: string;
  private readonly _options: MetaCheckerOptions;
  private readonly _workspace: CheckerWorkspace | undefined;
  private readonly _runtime: MetaRuntimeImpl;
  private readonly _touchedFiles = new Set<string>();
  private readonly _deletedFiles = new Set<string>();
  private _closed = false;

  /** @internal */
  constructor(
    session: ProjectSession,
    root: string,
    options?: MetaCheckerOptions,
    workspace?: CheckerWorkspace,
    runtime?: MetaRuntimeImpl,
  ) {
    this._session = session;
    this._root = root;
    this._options = options ?? {};
    this._workspace = workspace;
    this._runtime = runtime ?? getMetaRuntime();
  }

  private ensureOpen(): void {
    if (this._closed) throw new Error("MetaProject is closed");
    if (this._session.closed || this._session.engine.state !== "active") {
      throw new Error("MetaProject session was invalidated");
    }
  }

  updateFile(filePath: string, source: string): void {
    this.ensureOpen();
    const abs = normalizePath(resolve(this._root, filePath));
    this._touchedFiles.add(abs);
    this._deletedFiles.delete(abs);
    this._session.upsert(abs, source);
  }

  deleteFile(filePath: string): void {
    this.ensureOpen();
    const abs = normalizePath(resolve(this._root, filePath));
    this._touchedFiles.add(abs);
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
  }

  clearCaches(): void {
    this.ensureOpen();
    this._session.engine.clearCaches();
  }

  async getComponentMeta(filePath: string): Promise<VolarComponentMeta> {
    this.ensureOpen();
    const abs = normalizePath(resolve(this._root, filePath));

    if (this._deletedFiles.has(abs)) {
      return { type: 0, props: [], events: [], slots: [], exposed: [] };
    }

    if (!this._session.hasFile(abs) && this._workspace) {
      const content = await this._workspace.readFile(abs);
      if (content !== null) {
        this._touchedFiles.add(abs);
        this._session.upsert(abs, content);
      }
    }

    const source = this._session.getEffectiveSource(abs);
    if (source === undefined) {
      return { type: 0, props: [], events: [], slots: [], exposed: [] };
    }

    const rawSnapshot = this._session.getAnalysis(abs);
    const typeRegistry = rawSnapshot ? buildTypeRegistry(rawSnapshot) : undefined;

    if (typeRegistry) {
      const importedJson = this._session.resolveImportedTypes(abs);
      if (importedJson) {
        try {
          for (const rlt of JSON.parse(importedJson) as Array<{ name: string; expanded: string }>) {
            if (!typeRegistry.has(rlt.name)) {
              typeRegistry.set(rlt.name, rlt.expanded);
            }
          }
        } catch {
          // Ignore parse errors from corrupted imported-type payloads.
        }
      }
    }

    const sessionAdapter = {
      upsert() {},
      getAnalysis: (id: string) => this._session.getAnalysis(id),
      resolveImportedTypes: (id: string) => this._session.resolveImportedTypes(id),
    };

    const meta = extractComponentMeta(sessionAdapter, abs, abs);
    if (!meta) {
      return { type: 0, props: [], events: [], slots: [], exposed: [] };
    }

    return mapComponentMeta(meta, this._options, typeRegistry);
  }

  async getExportNames(_filePath: string): Promise<string[]> {
    this.ensureOpen();
    return ["default"];
  }

  close(): void {
    if (this._closed) return;
    this._closed = true;
    this._touchedFiles.clear();
    this._deletedFiles.clear();
    this._runtime.closeSession(this._session);
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
    return stableHash(JSON.parse(stripped));
  } catch {
    return stableHash({ tsconfigPath: normalizePath(resolve(tsconfigPath)) });
  }
}

function buildEngineKeyInput(options: MetaProjectConfig): EngineKeyInput {
  const root = normalizePath(resolve(options.root));
  const tsconfigPath = options.tsconfig ? normalizePath(resolve(options.tsconfig)) : undefined;

  return {
    backend: options.backend ?? "napi",
    root,
    configKind: options.tsconfig ? "tsconfig" : "inline",
    tsconfigPath,
    configHash: options.config
      ? stableHash(options.config)
      : hashTsconfigConfig(resolve(options.tsconfig)),
    nativeFlags: { analysisLevel: "full", deepMacroResolutionType: true },
  };
}

export async function openMetaProject(
  options: MetaProjectConfig,
  checkerOptions?: MetaCheckerOptions,
): Promise<MetaProject> {
  const runtime = getMetaRuntime();
  const root = resolve(options.root);
  const native = loadNative();
  const workspace: CheckerWorkspace = new native.Workspace([normalizePath(root)]);
  const parsedConfig = options.tsconfig
    ? await parseTsconfig(resolve(options.tsconfig), workspace)
    : null;
  const input = buildEngineKeyInput(options);

  const bootstrap: BootstrapFn = async () => {
    const config = {
      devMode: false,
      analysisLevel: "full",
      deepMacroResolutionType: true,
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

    const tsconfigDir = options.tsconfig ? dirname(resolve(options.tsconfig)) : root;
    const include = options.config ? (options.config as { include?: string[] }).include : undefined;
    const vueFiles = await discoverVueFiles(tsconfigDir, workspace, include);
    for (const filePath of vueFiles) {
      const content = await workspace.readFile(normalizePath(filePath));
      if (content !== null) {
        nativeProject.upsertBase(filePath, content);
      }
    }

    return { nativeProject, baseFileIds: vueFiles };
  };

  const engine = await runtime.getOrCreateEngine(input, bootstrap);
  const session = runtime.openSession(engine);
  return new MetaProject(session, root, checkerOptions, workspace, runtime);
}

export function evictMetaProject(options: MetaProjectConfig): void {
  const key = computeEngineKey(buildEngineKeyInput(options));
  getMetaRuntime().evictEngine(key);
}

export function shutdownMetaRuntime(): void {
  _shutdownMetaRuntime();
}

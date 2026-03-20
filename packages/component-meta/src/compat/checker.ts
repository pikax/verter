/**
 * Volar-compatible ComponentMetaChecker — drop-in replacement for vue-component-meta.
 *
 * Usage:
 * ```ts
 * import { createChecker } from '@verter/component-meta/compat'
 * const checker = await createChecker('./tsconfig.json')
 * const meta = await checker.getComponentMeta('./src/MyButton.vue')
 * ```
 */

import { resolve, dirname } from "node:path";
import { createRequire } from "node:module";
import { buildTypeRegistry, snapshotToMeta } from "../extractor.js";
import { parseType } from "../resolver.js";
import type { TypeDescriptor } from "../type-ir.js";
import type { VerterHostAdapter } from "../host-adapter.js";
import type { ComponentMeta, PropMeta, EventMeta, SlotMeta, ExposedMeta } from "../types.js";
import type { PropertyMeta, VolarComponentMeta, MetaCheckerOptions } from "./types.js";
import { typeDescriptorToSchema, typeDescriptorToString } from "./schema.js";
import {
  getMetaRuntime,
  computeEngineKey,
  normalizePath as runtimeNormalizePath,
  stableHash,
  getWorkspaceIdentity,
  parseTsconfig,
  extractPathAliases,
  discoverVueFiles as runtimeDiscoverVueFiles,
} from "../runtime/index.js";
import type {
  EngineKeyInput,
  NativeMetaProject,
  BootstrapFn,
  ProjectSession,
  MetaRuntimeImpl,
} from "../runtime/index.js";

/**
 * Minimal workspace interface used by the checker.
 * Matches the Workspace class from @verter/native.
 */
export interface CheckerWorkspace {
  readFile(path: string): Promise<string | null>;
  fileExists(path: string): Promise<boolean>;
  isDir(path: string): Promise<boolean>;
  readDir(dir: string): Promise<Array<{ path: string; isDir: boolean }>>;
  walk(root: string, excludeDirs: string[], extensions?: string[]): Promise<string[]>;
  configureProjects(
    projects: Array<{
      root: string;
      workspaceRoot: string;
      compilerOptions?: {
        baseUrl?: string;
        paths?: Array<{ pattern: string; targets: string[] }>;
      };
    }>,
  ): void;
}

/**
 * Create a workspace from @verter/native for the given root directory.
 */
function createWorkspace(rootDir: string): CheckerWorkspace {
  const _require = typeof require === "function" ? require : createRequire(import.meta.url);
  const native = _require("@verter/native");
  return new native.Workspace([normalizePath(rootDir)]);
}

/**
 * Read a file using workspace. Workspace is required.
 */
async function readFileSafe(absPath: string, ws: CheckerWorkspace): Promise<string | null> {
  return (await ws.readFile(normalizePath(absPath))) ?? null;
}

/**
 * Check if file exists using workspace. Workspace is required.
 */
async function fileExistsSafe(absPath: string, ws: CheckerWorkspace): Promise<boolean> {
  return ws.fileExists(normalizePath(absPath));
}

function normalizePath(p: string): string {
  return p.replace(/\\/g, "/");
}

/**
 * Map a Verter PropMeta to Volar PropertyMeta.
 */
export function mapPropMeta(
  prop: PropMeta,
  options?: MetaCheckerOptions,
  typeRegistry?: Map<string, TypeDescriptor>,
): PropertyMeta {
  return {
    name: prop.name,
    description: prop.description ?? "",
    type: normalizeTypeString(prop.rawType ?? typeDescriptorToString(prop.type)),
    required: prop.required,
    global: false,
    default: evaluateDefault(prop.default),
    tags: (prop.tags ?? []).map((t) => ({
      name: t.name,
      ...(t.text != null && { text: t.text }),
    })),
    schema: typeDescriptorToSchema(prop.type, options, typeRegistry),
  };
}

/**
 * Normalize Array<T> syntax to T[] for consistency with Volar output.
 */
function normalizeTypeString(type: string): string {
  const match = type.match(/^Array<(.+)>$/);
  if (match) return `${match[1]}[]`;
  return type;
}

/**
 * Evaluate common default value patterns to match Volar's behavior.
 * Volar evaluates simple defaults; verter stores the raw source text.
 */
function evaluateDefault(val: string | undefined): string | undefined {
  if (val === undefined) return undefined;
  // Arrow function returning empty object: () => ({})
  if (/^\(\)\s*=>\s*\(\s*\{\s*\}\s*\)$/.test(val)) return "{}";
  // Arrow function returning empty array: () => []
  if (/^\(\)\s*=>\s*\[\s*\]$/.test(val)) return "[]";
  // Arrow function returning array literal: () => ['a', 'b']
  const arrowArrMatch = val.match(/^\(\)\s*=>\s*(\[.*\])$/);
  if (arrowArrMatch) return arrowArrMatch[1];
  return val;
}

/**
 * Map a Verter EventMeta to Volar PropertyMeta.
 */
export function mapEventMeta(event: EventMeta, options?: MetaCheckerOptions): PropertyMeta {
  return {
    name: event.name,
    description: event.description ?? "",
    type: event.rawSignature ?? typeDescriptorToString(event.payload),
    required: false,
    global: false,
    tags: (event.tags ?? []).map((t) => ({
      name: t.name,
      ...(t.text != null && { text: t.text }),
    })),
    schema: typeDescriptorToSchema(event.payload, options),
  };
}

/**
 * Map a Verter SlotMeta to Volar PropertyMeta.
 */
export function mapSlotMeta(slot: SlotMeta, options?: MetaCheckerOptions): PropertyMeta {
  const type =
    slot.bindings.length > 0
      ? `{ ${slot.bindings.map((b) => `${b.name}: ${b.rawType ?? typeDescriptorToString(b.type)}`).join("; ")} }`
      : "{}";
  return {
    name: slot.name,
    description: slot.description ?? "",
    type,
    required: slot.isRequired ?? false,
    global: false,
    tags: (slot.tags ?? []).map((t) => ({
      name: t.name,
      ...(t.text != null && { text: t.text }),
    })),
    schema: type,
  };
}

/**
 * Map a Verter ExposedMeta to Volar PropertyMeta.
 */
export function mapExposedMeta(exposed: ExposedMeta, options?: MetaCheckerOptions): PropertyMeta {
  return {
    name: exposed.name,
    description: exposed.description ?? "",
    type: typeDescriptorToString(exposed.type),
    required: false,
    global: false,
    tags: [],
    schema: typeDescriptorToSchema(exposed.type, options),
  };
}

/**
 * Map full Verter ComponentMeta to Volar VolarComponentMeta shape.
 */
export function mapComponentMeta(
  meta: ComponentMeta,
  options?: MetaCheckerOptions,
  typeRegistry?: Map<string, TypeDescriptor>,
): VolarComponentMeta {
  return {
    type: 0,
    props: meta.props.map((p) => mapPropMeta(p, options, typeRegistry)),
    events: meta.events.map((e) => mapEventMeta(e, options)),
    slots: meta.slots.map((s) => mapSlotMeta(s, options)),
    exposed: meta.exposed.map((e) => mapExposedMeta(e, options)),
    _verter: meta,
  };
}

/**
 * Volar-compatible checker class.
 *
 * Provides `getComponentMeta()`, `getExportNames()`, `updateFile()`, etc.
 */
export class ComponentMetaChecker {
  private adapter: VerterHostAdapter;
  private options: MetaCheckerOptions;
  private trackedFiles: Map<string, string> = new Map();
  private deletedFiles = new Set<string>();
  private projectRoot: string;
  private workspace: CheckerWorkspace | undefined;
  private disposed = false;
  /** Runtime session backing this checker. */
  private _session: ProjectSession | null = null;
  private _runtime: MetaRuntimeImpl | null = null;

  constructor(
    adapter: VerterHostAdapter,
    projectRoot: string,
    options?: MetaCheckerOptions,
    session?: ProjectSession,
    workspace?: CheckerWorkspace,
    runtime?: MetaRuntimeImpl,
  ) {
    this.adapter = adapter;
    this.projectRoot = projectRoot;
    this.options = options ?? {};
    this.workspace = workspace;
    this._session = session ?? null;
    this._runtime = runtime ?? null;
  }

  /**
   * Get component metadata in Volar-compatible shape.
   */
  async getComponentMeta(filePath: string, _exportName?: string): Promise<VolarComponentMeta> {
    this.ensureActive();
    const absPath = resolve(this.projectRoot, filePath);
    await this.ensureFile(absPath);
    // getAnalysis now enriches macros with cross-file type resolution
    // (prop_fields, emit_fields, slot_fields, resolved_local_types)
    // when deepMacroResolutionType is enabled on the host config.
    const rawSnapshot = this.adapter.getAnalysis(absPath);
    const typeRegistry = rawSnapshot ? buildTypeRegistry(rawSnapshot) : undefined;
    if (typeRegistry) {
      // Safety net: enrich typeRegistry with resolved imported types from the host.
      // No-op when Rust enrichment already populated resolved_local_types (which
      // buildTypeRegistry picks up), but catches edge cases where enrichment
      // fails or the host was configured without deepMacroResolutionType.
      const importedJson = this.adapter.resolveImportedTypes?.(absPath);
      if (importedJson) {
        try {
          for (const rlt of JSON.parse(importedJson) as Array<{
            name: string;
            expanded: string;
          }>) {
            if (!typeRegistry.has(rlt.name)) {
              typeRegistry.set(rlt.name, parseType(rlt.expanded));
            }
          }
        } catch {
          // Ignore parse errors
        }
      }
      // Legacy: extract locally-defined interfaces/types from SFC content via
      // regex for schema expansion of PropType<T> references in Options API.
      // The native evaluator handles type-based macros; this is retained only
      // as a fallback for runtime-style props. Can be removed once native
      // evaluation covers all Options API prop type annotations.
      const sfcContent = this.trackedFiles.get(absPath);
      if (sfcContent) {
        extractLocalInterfaces(sfcContent, typeRegistry);
      }
    }
    // Use native lightweight type evaluation (session or adapter)
    let evaluatedTypes = null;
    try {
      if (this._session) {
        evaluatedTypes = this._session.evaluateTypes(absPath);
      } else if (this.adapter.evaluateTypes) {
        const json = this.adapter.evaluateTypes(absPath);
        evaluatedTypes = json ? JSON.parse(json) : null;
      }
    } catch {
      // Graceful fallback — evaluation is optional
    }

    const meta = rawSnapshot
      ? snapshotToMeta(
          rawSnapshot,
          absPath,
          evaluatedTypes as import("../type-expr-bridge.js").NativeEvaluatedTypes | null,
        )
      : null;
    if (!meta) {
      return {
        type: 0,
        props: [],
        events: [],
        slots: [],
        exposed: [],
      };
    }
    return mapComponentMeta(meta, this.options, typeRegistry);
  }

  /**
   * Get export names from a file.
   * For Vue SFCs, this typically returns `["default"]`.
   */
  async getExportNames(_filePath: string): Promise<string[]> {
    this.ensureActive();
    // Vue SFCs always have a default export
    return ["default"];
  }

  /**
   * Update (or create) a file in the host.
   */
  updateFile(filePath: string, content: string): void {
    this.ensureActive();
    const absPath = resolve(this.projectRoot, filePath);
    this.deletedFiles.delete(absPath);
    this.trackedFiles.set(absPath, content);
    this.adapter.upsert({ inputId: absPath, source: content });
  }

  /**
   * Delete a file from the host (upsert empty string).
   */
  deleteFile(filePath: string): void {
    this.ensureActive();
    const absPath = resolve(this.projectRoot, filePath);
    this.trackedFiles.delete(absPath);
    this.deletedFiles.add(absPath);
    if (this._session) {
      this._session.delete(absPath);
      return;
    }
    if (this.adapter.remove) {
      this.adapter.remove(absPath);
      return;
    }
    this.adapter.upsert({ inputId: absPath, source: "" });
  }

  /**
   * Reload all tracked files from disk.
   */
  async reload(): Promise<void> {
    this.ensureActive();
    if (!this.workspace) return;
    for (const absPath of new Set([...this.trackedFiles.keys(), ...this.deletedFiles])) {
      const content = await readFileSafe(absPath, this.workspace);
      this.ensureActive();
      if (content !== null) {
        this.deletedFiles.delete(absPath);
        this.trackedFiles.set(absPath, content);
        this.adapter.upsert({ inputId: absPath, source: content });
      } else {
        this.trackedFiles.delete(absPath);
        this.deletedFiles.add(absPath);
        if (this._session) {
          this._session.delete(absPath);
        } else {
          this.adapter.remove?.(absPath);
        }
      }
    }
  }

  /**
   * Clear all cached files and re-read from disk.
   * Alias for `reload()`.
   */
  async clearCache(): Promise<void> {
    this.ensureActive();
    await this.reload();
  }

  /**
   * Release the session and clear tracked in-memory state.
   * Optional — resources are pooled and will be reclaimed automatically.
   *
   * After close, further checker operations throw.
   */
  close(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.trackedFiles.clear();
    this.deletedFiles.clear();
    if (this._session) {
      if (this._runtime) {
        this._runtime.closeSession(this._session);
      } else {
        this._session.close();
      }
      this._session = null;
    }
    this.adapter.close?.();
  }

  /**
   * Alias for `close()`. Kept for backward compatibility.
   */
  dispose(): void {
    this.close();
  }

  /** @internal */
  rememberTrackedFile(absPath: string, content: string): void {
    this.deletedFiles.delete(absPath);
    this.trackedFiles.set(absPath, content);
  }

  /**
   * Not supported — Verter does not expose a TypeScript Program.
   * @throws Always throws.
   */
  getProgram(): never {
    this.ensureActive();
    throw new Error(
      "getProgram() is not supported by Verter. Verter does not use a TypeScript Program.",
    );
  }

  private async ensureFile(absPath: string): Promise<void> {
    this.ensureActive();
    if (this.deletedFiles.has(absPath)) {
      return;
    }
    if (!this.trackedFiles.has(absPath)) {
      // Try session source first, then fall back to workspace
      if (this._session) {
        const src = this._session.getEffectiveSource(absPath);
        if (src !== undefined) {
          this.trackedFiles.set(absPath, src);
          return;
        }
      }
      if (this.workspace) {
        const content = await readFileSafe(absPath, this.workspace);
        this.ensureActive();
        if (content !== null) {
          this.trackedFiles.set(absPath, content);
          this.adapter.upsert({ inputId: absPath, source: content });
        }
      }
    }
  }

  private ensureActive(): void {
    if (this.disposed) {
      throw new Error("ComponentMetaChecker has been disposed.");
    }
    if (this._session && (this._session.closed || this._session.engine.state !== "active")) {
      throw new Error("ComponentMetaChecker is closed.");
    }
  }
}

/**
 * Extract locally-defined interface/type declarations from SFC content
 * and add them to the type registry.
 *
 * Does NOT overwrite existing registry entries.
 */
function extractLocalInterfaces(sfcContent: string, registry: Map<string, TypeDescriptor>): void {
  const scriptBlocks = sfcContent.matchAll(/<script[^>]*>([\s\S]*?)<\/script>/g);
  for (const match of scriptBlocks) {
    const script = match[1];
    // Match interface declarations
    const interfacePattern = /\binterface\s+(\w+)(?:\s+extends\s+[^{]+)?\s*\{/g;
    let ifMatch;
    while ((ifMatch = interfacePattern.exec(script)) !== null) {
      const name = ifMatch[1];
      if (registry.has(name)) continue;
      const startIdx = ifMatch.index + ifMatch[0].length - 1;
      let depth = 1;
      let i = startIdx + 1;
      while (i < script.length && depth > 0) {
        if (script[i] === "{") depth++;
        else if (script[i] === "}") depth--;
        i++;
      }
      if (depth === 0) {
        registry.set(name, parseType(script.slice(startIdx, i)));
      }
    }
    // Match type alias declarations
    const typePattern = /\btype\s+(\w+)(?:<[^>]*>)?\s*=\s*/g;
    let typeMatch;
    while ((typeMatch = typePattern.exec(script)) !== null) {
      const name = typeMatch[1];
      if (registry.has(name)) continue;
      const startIdx = typeMatch.index + typeMatch[0].length;
      let depth = 0;
      let i = startIdx;
      while (i < script.length) {
        const ch = script[i];
        if (ch === "{" || ch === "(" || ch === "<") depth++;
        else if (ch === "}" || ch === ")" || ch === ">") depth--;
        else if (depth === 0 && (ch === "\n" || ch === ";")) break;
        i++;
      }
      const value = script.slice(startIdx, i).trim();
      if (value) {
        registry.set(name, parseType(value));
      }
    }
  }
}

/**
 * Create a Volar-compatible checker from a tsconfig.json path.
 *
 * @param tsconfigPath Path to tsconfig.json
 * @param options      Checker options
 */
export async function createChecker(
  workspace: CheckerWorkspace,
  tsconfigPath: string,
  options?: MetaCheckerOptions,
): Promise<ComponentMetaChecker> {
  const absPath = resolve(tsconfigPath);
  const projectRoot = dirname(absPath);
  const parsed = await parseTsconfig(absPath, workspace);

  // Build engine key for pooling
  const wsIdentity = getWorkspaceIdentity(workspace);
  const input: EngineKeyInput = {
    backend: "napi",
    root: runtimeNormalizePath(projectRoot),
    configKind: "tsconfig",
    tsconfigPath: runtimeNormalizePath(absPath),
    configHash: stableHash(parsed?.config ?? { tsconfigPath: runtimeNormalizePath(absPath) }),
    workspaceIdentity: wsIdentity,
    nativeFlags: { analysisLevel: "full", deepMacroResolutionType: true },
  };

  const runtime = getMetaRuntime();

  const bootstrap: BootstrapFn = async () => {
    const _require = typeof require === "function" ? require : createRequire(import.meta.url);
    const native = _require("@verter/native");
    const config = { devMode: false, analysisLevel: "full", deepMacroResolutionType: true };
    const nativeProject: NativeMetaProject = native.MetaProject.withWorkspace(config, workspace);

    // Configure project resolver
    if (parsed) {
      const aliases = extractPathAliases(parsed.config, runtimeNormalizePath(projectRoot));
      workspace.configureProjects([aliases]);
    }

    // Discover and bulk-load .vue files
    const vueFiles = await runtimeDiscoverVueFiles(dirname(absPath), workspace);
    for (const filePath of vueFiles) {
      const content = await readFileSafe(filePath, workspace);
      if (content !== null) {
        nativeProject.upsertBase(filePath, content);
      }
    }

    return { nativeProject, baseFileIds: vueFiles };
  };

  const engine = await runtime.getOrCreateEngine(input, bootstrap);
  const session = runtime.openSession(engine);

  // Create session-backed adapter
  const adapter: VerterHostAdapter = {
    upsert(request) {
      session.upsert(request.inputId, request.source);
    },
    remove(canonicalOrAlias) {
      session.delete(canonicalOrAlias);
    },
    getAnalysis(canonicalOrAlias) {
      return session.getAnalysis(canonicalOrAlias);
    },
    resolveImportedTypes(canonicalOrAlias) {
      return session.resolveImportedTypes(canonicalOrAlias);
    },
    configureProjects(projects) {
      workspace.configureProjects(projects);
    },
  };

  const checker = new ComponentMetaChecker(
    adapter,
    projectRoot,
    options,
    session,
    workspace,
    runtime,
  );

  // Pre-track discovered files
  const baseIds = engine.nativeProject.baseFileIds();
  for (const filePath of baseIds) {
    const content = session.getEffectiveSource(filePath);
    if (content !== undefined) {
      checker.rememberTrackedFile(filePath, content);
    }
  }

  return checker;
}

/**
 * Create a Volar-compatible checker from an inline tsconfig JSON object.
 *
 * Creates a workspace internally from `@verter/native`.
 *
 * @param projectRoot Root directory for the project
 * @param configJson  tsconfig-like configuration object
 * @param options     Checker options
 */
export async function createCheckerByJson(
  projectRoot: string,
  configJson: object,
  options?: MetaCheckerOptions,
): Promise<ComponentMetaChecker> {
  const absRoot = resolve(projectRoot);
  const config = configJson as Record<string, unknown>;
  const workspace = createWorkspace(absRoot);

  const input: EngineKeyInput = {
    backend: "napi",
    root: runtimeNormalizePath(absRoot),
    configKind: "inline",
    configHash: stableHash(config),
    nativeFlags: { analysisLevel: "full", deepMacroResolutionType: true },
  };

  const runtime = getMetaRuntime();

  const bootstrap: BootstrapFn = async () => {
    const _require = typeof require === "function" ? require : createRequire(import.meta.url);
    const native = _require("@verter/native");
    const hostConfig = { devMode: false, analysisLevel: "full", deepMacroResolutionType: true };
    const nativeProject: NativeMetaProject = native.MetaProject.withWorkspace(
      hostConfig,
      workspace,
    );

    // Configure project resolver
    const aliases = extractPathAliases(config, runtimeNormalizePath(absRoot));
    workspace.configureProjects([aliases]);

    // Discover .vue files
    const include = config.include as string[] | undefined;
    const vueFiles = await runtimeDiscoverVueFiles(absRoot, workspace, include);
    for (const filePath of vueFiles) {
      const content = await readFileSafe(filePath, workspace);
      if (content !== null) {
        nativeProject.upsertBase(filePath, content);
      }
    }

    return { nativeProject, baseFileIds: vueFiles };
  };

  const engine = await runtime.getOrCreateEngine(input, bootstrap);
  const session = runtime.openSession(engine);

  const adapter: VerterHostAdapter = {
    upsert(request) {
      session.upsert(request.inputId, request.source);
    },
    remove(canonicalOrAlias) {
      session.delete(canonicalOrAlias);
    },
    getAnalysis(canonicalOrAlias) {
      return session.getAnalysis(canonicalOrAlias);
    },
    resolveImportedTypes(canonicalOrAlias) {
      return session.resolveImportedTypes(canonicalOrAlias);
    },
    configureProjects(projects) {
      workspace.configureProjects(projects);
    },
  };

  const checker = new ComponentMetaChecker(adapter, absRoot, options, session, workspace, runtime);

  const baseIds = engine.nativeProject.baseFileIds();
  for (const filePath of baseIds) {
    const content = session.getEffectiveSource(filePath);
    if (content !== undefined) {
      checker.rememberTrackedFile(filePath, content);
    }
  }

  return checker;
}

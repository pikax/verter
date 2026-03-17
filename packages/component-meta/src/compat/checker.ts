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
import { createNapiAdapter } from "../host-adapter.js";
import { extractComponentMeta, buildTypeRegistry } from "../extractor.js";
import { parseType } from "../resolver.js";
import type { VerterHostAdapter } from "../host-adapter.js";
import type { ComponentMeta, PropMeta, EventMeta, SlotMeta, ExposedMeta } from "../types.js";
import type { PropertyMeta, VolarComponentMeta, MetaCheckerOptions } from "./types.js";
import { typeDescriptorToSchema, typeDescriptorToString } from "./schema.js";

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
  typeRegistry?: Map<string, string>,
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
  typeRegistry?: Map<string, string>,
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
  private projectRoot: string;
  private workspace: CheckerWorkspace;

  constructor(
    workspace: CheckerWorkspace,
    adapter: VerterHostAdapter,
    projectRoot: string,
    options?: MetaCheckerOptions,
  ) {
    this.adapter = adapter;
    this.projectRoot = projectRoot;
    this.options = options ?? {};
    this.workspace = workspace;
  }

  /**
   * Get component metadata in Volar-compatible shape.
   */
  async getComponentMeta(filePath: string, _exportName?: string): Promise<VolarComponentMeta> {
    const absPath = resolve(this.projectRoot, filePath);
    await this.ensureFile(absPath);
    const rawSnapshot = this.adapter.getAnalysis(absPath);
    // Ensure dependency .ts files are in the host for cross-file type resolution
    await this.ensureTypeDependencies(absPath, rawSnapshot);
    // Build type registry and enrich with resolved imported types
    const typeRegistry = rawSnapshot ? buildTypeRegistry(rawSnapshot) : undefined;
    if (typeRegistry) {
      const importedJson = this.adapter.resolveImportedTypes?.(absPath);
      if (importedJson) {
        try {
          for (const rlt of JSON.parse(importedJson) as Array<{
            name: string;
            expanded: string;
          }>) {
            if (!typeRegistry.has(rlt.name)) {
              typeRegistry.set(rlt.name, rlt.expanded);
            }
          }
        } catch {
          // Ignore parse errors
        }
      }
      // Extract locally-defined interfaces/types from SFC content
      // for runtime-style defineProps that reference local types
      const sfcContent = this.trackedFiles.get(absPath);
      if (sfcContent) {
        extractLocalInterfaces(sfcContent, typeRegistry);
      }
    }
    const meta = extractComponentMeta(this.adapter, absPath, absPath);
    if (!meta) {
      return {
        type: 0,
        props: [],
        events: [],
        slots: [],
        exposed: [],
      };
    }
    // Fill in props from resolved imported types when extraction couldn't resolve them
    if (meta.props.length === 0 && typeRegistry && typeRegistry.size > 0) {
      const snapshot = rawSnapshot as {
        macros?: Array<{ typeReferences?: string[]; propFields?: unknown[] }>;
      } | null;
      const unresolvedRefs =
        snapshot?.macros
          ?.filter((m) => (!m.propFields || m.propFields.length === 0) && m.typeReferences?.length)
          ?.flatMap((m) => m.typeReferences ?? []) ?? [];
      for (const ref of unresolvedRefs) {
        const expanded = typeRegistry.get(ref);
        if (!expanded) continue;
        // Parse "{ label: string; size?: number }" into props
        const parsed = parseType(expanded);
        if (parsed.kind === "object") {
          for (const prop of parsed.properties) {
            meta.props.push({
              name: prop.name,
              type: prop.type,
              required: !prop.optional,
              hasDefault: false,
              rawType: expanded.includes(prop.name)
                ? prop.type.kind === "unknown"
                  ? prop.type.rawType
                  : undefined
                : undefined,
            });
          }
        }
      }
    }
    return mapComponentMeta(meta, this.options, typeRegistry);
  }

  /**
   * Get export names from a file.
   * For Vue SFCs, this typically returns `["default"]`.
   */
  async getExportNames(_filePath: string): Promise<string[]> {
    // Vue SFCs always have a default export
    return ["default"];
  }

  /**
   * Update (or create) a file in the host.
   */
  updateFile(filePath: string, content: string): void {
    const absPath = resolve(this.projectRoot, filePath);
    this.trackedFiles.set(absPath, content);
    this.adapter.upsert({ inputId: absPath, source: content });
  }

  /**
   * Delete a file from the host (upsert empty string).
   */
  deleteFile(filePath: string): void {
    const absPath = resolve(this.projectRoot, filePath);
    this.trackedFiles.delete(absPath);
    this.adapter.upsert({ inputId: absPath, source: "" });
  }

  /**
   * Reload all tracked files from disk.
   */
  async reload(): Promise<void> {
    for (const [absPath] of this.trackedFiles) {
      const content = await readFileSafe(absPath, this.workspace);
      if (content !== null) {
        this.trackedFiles.set(absPath, content);
        this.adapter.upsert({ inputId: absPath, source: content });
      } else {
        this.trackedFiles.delete(absPath);
      }
    }
  }

  /**
   * Clear all cached files and re-read from disk.
   * Alias for `reload()`.
   */
  async clearCache(): Promise<void> {
    await this.reload();
  }

  /**
   * Not supported — Verter does not expose a TypeScript Program.
   * @throws Always throws.
   */
  getProgram(): never {
    throw new Error(
      "getProgram() is not supported by Verter. Verter does not use a TypeScript Program.",
    );
  }

  private async ensureFile(absPath: string): Promise<void> {
    if (!this.trackedFiles.has(absPath)) {
      const content = await readFileSafe(absPath, this.workspace);
      if (content !== null) {
        this.trackedFiles.set(absPath, content);
        this.adapter.upsert({ inputId: absPath, source: content });
      }
    }
  }

  /**
   * Ensure dependency `.ts` files for cross-file type resolution are in the host.
   */
  private async ensureTypeDependencies(absPath: string, rawSnapshot: unknown): Promise<void> {
    const snapshot = rawSnapshot as { macroTypeDeps?: Array<{ importSource: string }> } | null;
    if (!snapshot?.macroTypeDeps) return;
    for (const dep of snapshot.macroTypeDeps) {
      if (!dep.importSource.startsWith(".")) continue;
      const resolved = await this.resolveDepPath(absPath, dep.importSource);
      if (resolved && !this.trackedFiles.has(resolved)) {
        const content = await readFileSafe(resolved, this.workspace);
        if (content !== null) {
          this.trackedFiles.set(resolved, content);
          this.adapter.upsert({ inputId: resolved, source: content, fileKind: "non_sfc" });
        }
      }
    }
  }

  /**
   * Resolve a relative import specifier to an absolute file path,
   * trying common TypeScript extensions.
   */
  private async resolveDepPath(fromPath: string, specifier: string): Promise<string | null> {
    const base = resolve(dirname(fromPath), specifier);
    for (const ext of [".ts", ".tsx", "/index.ts", ".d.ts"]) {
      const candidate = base + ext;
      if (await fileExistsSafe(candidate, this.workspace)) return candidate;
    }
    // Try exact path (might already have extension)
    if (await fileExistsSafe(base, this.workspace)) return base;
    return null;
  }
}

/**
 * Extract locally-defined interface/type declarations from SFC content
 * and add them to the type registry.
 *
 * Handles simple interface definitions like:
 * ```ts
 * interface Foo { name: string; age: number }
 * ```
 *
 * Does NOT overwrite existing registry entries.
 */
function extractLocalInterfaces(sfcContent: string, registry: Map<string, string>): void {
  // Extract script content (both <script setup> and <script>)
  const scriptBlocks = sfcContent.matchAll(/<script[^>]*>([\s\S]*?)<\/script>/g);
  for (const match of scriptBlocks) {
    const script = match[1];
    // Match interface declarations: interface Name { ... }
    // Use a balanced brace matching approach
    const interfacePattern = /\binterface\s+(\w+)(?:\s+extends\s+[^{]+)?\s*\{/g;
    let ifMatch;
    while ((ifMatch = interfacePattern.exec(script)) !== null) {
      const name = ifMatch[1];
      if (registry.has(name)) continue;
      // Find the matching closing brace
      const startIdx = ifMatch.index + ifMatch[0].length - 1; // position of opening {
      let depth = 1;
      let i = startIdx + 1;
      while (i < script.length && depth > 0) {
        if (script[i] === "{") depth++;
        else if (script[i] === "}") depth--;
        i++;
      }
      if (depth === 0) {
        const body = script.slice(startIdx, i); // includes { and }
        registry.set(name, body);
      }
    }

    // Match type alias declarations: type Name = ...
    const typePattern = /\btype\s+(\w+)(?:<[^>]*>)?\s*=\s*/g;
    let typeMatch;
    while ((typeMatch = typePattern.exec(script)) !== null) {
      const name = typeMatch[1];
      if (registry.has(name)) continue;
      // Extract the type value until the next unbalanced semicolon or newline
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
        registry.set(name, value);
      }
    }
  }
}

/**
 * Parse tsconfig.json and discover .vue files.
 */
async function discoverVueFiles(tsconfigPath: string, ws: CheckerWorkspace): Promise<string[]> {
  const absPath = resolve(tsconfigPath);
  const dir = dirname(absPath);

  try {
    const raw = await readFileSafe(absPath, ws);
    if (!raw) return [];
    // Strip JSON comments (// and /* */) for tsconfig.json compat
    const stripped = raw.replace(/\/\/.*$/gm, "").replace(/\/\*[\s\S]*?\*\//g, "");
    const config = JSON.parse(stripped);
    const include: string[] = config.include ?? ["**/*.vue"];

    // Simple glob expansion for .vue files
    const files: string[] = [];
    for (const pattern of include) {
      if (pattern.includes("*.vue") || pattern === "**/*") {
        // Walk the directory for .vue files (lazy — we don't need exhaustive discovery)
        await collectVueFiles(dir, files, ws);
        break;
      }
    }

    // Also include explicit files
    if (config.files) {
      for (const f of config.files) {
        const fp = resolve(dir, f);
        if (fp.endsWith(".vue")) files.push(fp);
      }
    }

    return [...new Set(files)];
  } catch {
    return [];
  }
}

async function collectVueFiles(
  dir: string,
  files: string[],
  ws: CheckerWorkspace,
  depth = 0,
): Promise<void> {
  if (depth > 10) return; // Prevent infinite recursion
  try {
    const entries = await ws.readDir(normalizePath(dir));
    for (const entry of entries) {
      const name = entry.path.split("/").pop() ?? "";
      if (name.startsWith(".") || name === "node_modules") continue;
      const full = entry.path;
      if (entry.isDir) {
        await collectVueFiles(full, files, ws, depth + 1);
      } else if (name.endsWith(".vue")) {
        files.push(full);
      }
    }
  } catch {
    // Directory not readable
  }
}

/**
 * Parse tsconfig compilerOptions and configure the adapter's project resolver.
 */
async function configureProjectFromTsconfig(
  ws: CheckerWorkspace,
  adapter: VerterHostAdapter,
  tsconfigPath: string,
  projectRoot: string,
): Promise<void> {
  const raw = await readFileSafe(tsconfigPath, ws);
  if (!raw) return;
  try {
    const stripped = raw.replace(/\/\/.*$/gm, "").replace(/\/\*[\s\S]*?\*\//g, "");
    const config = JSON.parse(stripped) as Record<string, unknown>;
    configureProjectFromConfigJson(ws, adapter, projectRoot, config);
  } catch {
    // tsconfig not readable or invalid — skip project configuration
  }
}

/**
 * Configure the adapter's project resolver from an inline config JSON object.
 */
function configureProjectFromConfigJson(
  ws: CheckerWorkspace,
  adapter: VerterHostAdapter,
  projectRoot: string,
  config: Record<string, unknown>,
): void {
  const compilerOptions = (config.compilerOptions ?? {}) as Record<string, unknown>;
  const rawPaths = (compilerOptions.paths ?? {}) as Record<string, string[]>;

  const paths: { pattern: string; targets: string[] }[] = Object.entries(rawPaths).map(
    ([pattern, targets]) => ({ pattern, targets }),
  );

  const project = {
    root: projectRoot,
    workspaceRoot: projectRoot,
    compilerOptions: {
      baseUrl: (compilerOptions.baseUrl as string) ?? undefined,
      paths: paths.length > 0 ? paths : undefined,
    },
  };

  ws.configureProjects([project]);
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
  const adapter = createNapiAdapter(workspace);

  // Configure project resolver with tsconfig paths so aliased imports resolve
  await configureProjectFromTsconfig(workspace, adapter, absPath, projectRoot);

  // Discover and bulk-upsert .vue files
  const vueFiles = await discoverVueFiles(absPath, workspace);
  for (const filePath of vueFiles) {
    const content = await readFileSafe(filePath, workspace);
    if (content !== null) {
      adapter.upsert({ inputId: filePath, source: content });
    }
  }

  const checker = new ComponentMetaChecker(workspace, adapter, projectRoot, options);

  // Track discovered files
  for (const filePath of vueFiles) {
    const content = await readFileSafe(filePath, workspace);
    if (content !== null) {
      (checker as any).trackedFiles.set(filePath, content);
    }
  }

  return checker;
}

/**
 * Resolve files from tsconfig-style include patterns.
 * Handles specific file paths and `dir/**\/*` glob patterns.
 */
async function resolveIncludePatterns(
  rootDir: string,
  include: string[],
  ws: CheckerWorkspace,
): Promise<string[]> {
  const files: string[] = [];

  for (const pattern of include) {
    const absPattern = resolve(rootDir, pattern);

    // Check if it's a specific file path (has a file extension)
    if (/\.\w+$/.test(pattern) && !pattern.includes("*")) {
      if (await ws.fileExists(normalizePath(absPattern))) {
        files.push(absPattern);
      }
      continue;
    }

    // Handle glob patterns like "dir/**/*" — walk the directory part
    const globIndex = pattern.indexOf("*");
    if (globIndex !== -1) {
      const dirPart = pattern.substring(0, globIndex).replace(/[/\\]+$/, "");
      const absDir = resolve(rootDir, dirPart);
      if ((await ws.fileExists(normalizePath(absDir))) && (await ws.isDir(normalizePath(absDir)))) {
        await collectVueFiles(absDir, files, ws);
      }
      continue;
    }

    // Plain directory path — walk it
    if (
      (await ws.fileExists(normalizePath(absPattern))) &&
      (await ws.isDir(normalizePath(absPattern)))
    ) {
      await collectVueFiles(absPattern, files, ws);
    }
  }

  return [...new Set(files)];
}

/**
 * Create a Volar-compatible checker from an inline tsconfig JSON object.
 *
 * @param projectRoot Root directory for the project
 * @param configJson  tsconfig-like configuration object
 * @param options     Checker options
 */
export async function createCheckerByJson(
  workspace: CheckerWorkspace,
  projectRoot: string,
  configJson: object,
  options?: MetaCheckerOptions,
): Promise<ComponentMetaChecker> {
  const absRoot = resolve(projectRoot);
  const adapter = createNapiAdapter(workspace);
  const config = configJson as Record<string, unknown>;

  // Configure project resolver with inline compilerOptions
  configureProjectFromConfigJson(workspace, adapter, absRoot, config);

  // Resolve files from include patterns if available, otherwise walk project root
  let vueFiles: string[];
  const include = config.include as string[] | undefined;
  if (include && include.length > 0) {
    vueFiles = await resolveIncludePatterns(absRoot, include, workspace);
  } else {
    vueFiles = [];
    await collectVueFiles(absRoot, vueFiles, workspace);
  }

  for (const filePath of vueFiles) {
    const content = await readFileSafe(filePath, workspace);
    if (content !== null) {
      adapter.upsert({ inputId: filePath, source: content });
    }
  }

  const checker = new ComponentMetaChecker(workspace, adapter, absRoot, options);
  for (const filePath of vueFiles) {
    const content = await readFileSafe(filePath, workspace);
    if (content !== null) {
      (checker as any).trackedFiles.set(filePath, content);
    }
  }

  return checker;
}

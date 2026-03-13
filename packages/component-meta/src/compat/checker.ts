/**
 * Volar-compatible ComponentMetaChecker — drop-in replacement for vue-component-meta.
 *
 * Usage:
 * ```ts
 * import { createChecker } from '@verter/component-meta/compat'
 * const checker = createChecker('./tsconfig.json')
 * const meta = checker.getComponentMeta('./src/MyButton.vue')
 * ```
 */

import { readFileSync } from "node:fs";
import { resolve, dirname, basename, extname } from "node:path";
import { createNapiAdapter } from "../host-adapter.js";
import { extractComponentMeta, buildTypeRegistry } from "../extractor.js";
import type { VerterHostAdapter } from "../host-adapter.js";
import type { ComponentMeta, PropMeta, EventMeta, SlotMeta, ExposedMeta } from "../types.js";
import type { PropertyMeta, VolarComponentMeta, MetaCheckerOptions } from "./types.js";
import { typeDescriptorToSchema, typeDescriptorToString } from "./schema.js";

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
    type: prop.rawType ?? typeDescriptorToString(prop.type),
    required: prop.required,
    global: false,
    default: prop.default,
    tags: (prop.tags ?? []).map((t) => ({
      name: t.name,
      ...(t.text != null && { text: t.text }),
    })),
    schema: typeDescriptorToSchema(prop.type, options, typeRegistry),
  };
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

  constructor(adapter: VerterHostAdapter, projectRoot: string, options?: MetaCheckerOptions) {
    this.adapter = adapter;
    this.projectRoot = projectRoot;
    this.options = options ?? {};
  }

  /**
   * Get component metadata in Volar-compatible shape.
   */
  getComponentMeta(filePath: string, _exportName?: string): VolarComponentMeta {
    const absPath = resolve(this.projectRoot, filePath);
    this.ensureFile(absPath);
    // Build type registry from raw snapshot for ref resolution
    const rawSnapshot = this.adapter.getAnalysis(absPath);
    const typeRegistry = rawSnapshot ? buildTypeRegistry(rawSnapshot) : undefined;
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
    return mapComponentMeta(meta, this.options, typeRegistry);
  }

  /**
   * Get export names from a file.
   * For Vue SFCs, this typically returns `["default"]`.
   */
  getExportNames(_filePath: string): string[] {
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
  reload(): void {
    for (const [absPath] of this.trackedFiles) {
      try {
        const content = readFileSync(absPath, "utf-8");
        this.trackedFiles.set(absPath, content);
        this.adapter.upsert({ inputId: absPath, source: content });
      } catch {
        // File may have been deleted
        this.trackedFiles.delete(absPath);
      }
    }
  }

  /**
   * Clear all cached files and re-read from disk.
   * Alias for `reload()`.
   */
  clearCache(): void {
    this.reload();
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

  private ensureFile(absPath: string): void {
    if (!this.trackedFiles.has(absPath)) {
      try {
        const content = readFileSync(absPath, "utf-8");
        this.trackedFiles.set(absPath, content);
        this.adapter.upsert({ inputId: absPath, source: content });
      } catch {
        // File doesn't exist — will return empty meta
      }
    }
  }
}

/**
 * Parse tsconfig.json and discover .vue files.
 */
function discoverVueFiles(tsconfigPath: string): string[] {
  const absPath = resolve(tsconfigPath);
  const dir = dirname(absPath);

  try {
    const raw = readFileSync(absPath, "utf-8");
    // Strip JSON comments (// and /* */) for tsconfig.json compat
    const stripped = raw.replace(/\/\/.*$/gm, "").replace(/\/\*[\s\S]*?\*\//g, "");
    const config = JSON.parse(stripped);
    const include: string[] = config.include ?? ["**/*.vue"];

    // Simple glob expansion for .vue files
    const files: string[] = [];
    for (const pattern of include) {
      if (pattern.includes("*.vue") || pattern === "**/*") {
        // Walk the directory for .vue files (lazy — we don't need exhaustive discovery)
        collectVueFiles(dir, files);
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

function collectVueFiles(dir: string, files: string[], depth = 0): void {
  if (depth > 10) return; // Prevent infinite recursion
  try {
    const { readdirSync } = require("node:fs") as typeof import("node:fs");
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      if (entry.name.startsWith(".") || entry.name === "node_modules") continue;
      const full = resolve(dir, entry.name);
      if (entry.isDirectory()) {
        collectVueFiles(full, files, depth + 1);
      } else if (entry.isFile() && entry.name.endsWith(".vue")) {
        files.push(full);
      }
    }
  } catch {
    // Directory not readable
  }
}

/**
 * Create a Volar-compatible checker from a tsconfig.json path.
 *
 * @param tsconfigPath Path to tsconfig.json
 * @param options      Checker options
 */
export function createChecker(
  tsconfigPath: string,
  options?: MetaCheckerOptions,
): ComponentMetaChecker {
  const absPath = resolve(tsconfigPath);
  const projectRoot = dirname(absPath);
  const adapter = createNapiAdapter();

  // Discover and bulk-upsert .vue files
  const vueFiles = discoverVueFiles(absPath);
  for (const filePath of vueFiles) {
    try {
      const content = readFileSync(filePath, "utf-8");
      adapter.upsert({ inputId: filePath, source: content });
    } catch {
      // Skip unreadable files
    }
  }

  const checker = new ComponentMetaChecker(adapter, projectRoot, options);

  // Track discovered files
  for (const filePath of vueFiles) {
    try {
      const content = readFileSync(filePath, "utf-8");
      (checker as any).trackedFiles.set(filePath, content);
    } catch {
      // Skip
    }
  }

  return checker;
}

/**
 * Create a Volar-compatible checker from an inline tsconfig JSON object.
 *
 * @param projectRoot Root directory for the project
 * @param configJson  tsconfig-like configuration object
 * @param options     Checker options
 */
export function createCheckerByJson(
  projectRoot: string,
  _configJson: object,
  options?: MetaCheckerOptions,
): ComponentMetaChecker {
  const absRoot = resolve(projectRoot);
  const adapter = createNapiAdapter();

  // Discover .vue files under project root
  const vueFiles: string[] = [];
  collectVueFiles(absRoot, vueFiles);
  for (const filePath of vueFiles) {
    try {
      const content = readFileSync(filePath, "utf-8");
      adapter.upsert({ inputId: filePath, source: content });
    } catch {
      // Skip
    }
  }

  const checker = new ComponentMetaChecker(adapter, absRoot, options);
  for (const filePath of vueFiles) {
    try {
      const content = readFileSync(filePath, "utf-8");
      (checker as any).trackedFiles.set(filePath, content);
    } catch {
      // Skip
    }
  }

  return checker;
}

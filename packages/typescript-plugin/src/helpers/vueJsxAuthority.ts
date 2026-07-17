import { createHash } from "node:crypto";
import {
  existsSync,
  mkdirSync,
  readFileSync,
  realpathSync,
  renameSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

const VUE_JSX_PRAGMA = "/** @jsxImportSource vue */\n";

interface ResolvedVuePackage {
  readonly root: string;
  readonly jsxRuntimeTypes: string;
  readonly packageJson: Buffer;
  readonly runtimeTypes: Buffer;
  readonly version: string;
}

export interface PreparedVueJsxCarrier {
  readonly content: string;
  readonly adapterPath: string;
  readonly adapterContent: string;
}

function exportedTypes(value: unknown): string | undefined {
  if (typeof value === "string") return value;
  if (Array.isArray(value)) {
    for (const entry of value) {
      const target = exportedTypes(entry);
      if (target !== undefined) return target;
    }
    return undefined;
  }
  if (value === null || typeof value !== "object") return undefined;
  const conditions = value as Record<string, unknown>;
  if (typeof conditions.types === "string") return conditions.types;
  return exportedTypes(conditions.import) ?? exportedTypes(conditions.default);
}

function declarationTarget(packageRoot: string, target: string): string | undefined {
  const relative = target.startsWith("./") ? target.slice(2) : target;
  if (path.isAbsolute(relative) || relative.split(/[\\/]/u).some((segment) => segment === "..")) {
    return undefined;
  }
  const candidate = path.join(packageRoot, relative);
  return existsSync(candidate) ? realpathSync(candidate) : undefined;
}

function resolveVuePackage(candidate: string): ResolvedVuePackage | undefined {
  try {
    const packageJsonPath = path.join(candidate, "package.json");
    const packageJson = readFileSync(packageJsonPath);
    const manifest = JSON.parse(packageJson.toString("utf8")) as Record<string, unknown>;
    if (manifest.name !== "vue" || typeof manifest.version !== "string") return undefined;
    const exports = manifest.exports;
    if (exports === null || typeof exports !== "object") return undefined;
    const target = exportedTypes((exports as Record<string, unknown>)["./jsx-runtime"]);
    if (target === undefined) return undefined;
    const root = realpathSync(candidate);
    const jsxRuntimeTypes = declarationTarget(root, target);
    if (jsxRuntimeTypes === undefined) return undefined;
    return {
      root,
      jsxRuntimeTypes,
      packageJson,
      runtimeTypes: readFileSync(jsxRuntimeTypes),
      version: manifest.version,
    };
  } catch {
    return undefined;
  }
}

function nearestVuePackage(providerPath: string): ResolvedVuePackage | undefined {
  let directory = path.dirname(path.resolve(providerPath));
  for (;;) {
    const candidate = path.join(directory, "node_modules", "vue");
    if (existsSync(path.join(candidate, "package.json"))) {
      return resolveVuePackage(candidate);
    }
    const parent = path.dirname(directory);
    if (parent === directory) return undefined;
    directory = parent;
  }
}

function modulePath(value: string): string {
  let normalized = path.resolve(value);
  if (normalized.startsWith("\\\\?\\UNC\\")) {
    normalized = `\\\\${normalized.slice("\\\\?\\UNC\\".length)}`;
  } else if (normalized.startsWith("\\\\?\\")) {
    normalized = normalized.slice("\\\\?\\".length);
  }
  return normalized.replace(/\\/gu, "/");
}

function hashFields(fields: readonly (string | Buffer)[]): string {
  const hash = createHash("sha256");
  for (const field of fields) {
    const bytes = typeof field === "string" ? Buffer.from(field) : field;
    const length = Buffer.allocUnsafe(8);
    length.writeBigUInt64LE(BigInt(bytes.length));
    hash.update(length);
    hash.update(bytes);
  }
  return hash.digest("hex").slice(0, 24);
}

function extensionlessDeclarationSpecifier(fileName: string): string {
  for (const extension of [".ts", ".mts", ".cts"]) {
    if (fileName.endsWith(extension)) {
      return modulePath(fileName.slice(0, -extension.length));
    }
  }
  return modulePath(fileName);
}

function classicAdapter(packageInfo: ResolvedVuePackage): string {
  const runtime = JSON.stringify(extensionlessDeclarationSpecifier(packageInfo.jsxRuntimeTypes));
  return `import type { JSX as __VerterAutomaticJSX } from ${runtime};
export function h(...args: unknown[]): __VerterAutomaticJSX.Element;
export const Fragment: unique symbol;
export namespace JSX {
  type Element = __VerterAutomaticJSX.Element;
  type ElementClass = __VerterAutomaticJSX.ElementClass;
  type ElementAttributesProperty = __VerterAutomaticJSX.ElementAttributesProperty;
  interface ElementChildrenAttribute {}
  type IntrinsicElements = __VerterAutomaticJSX.IntrinsicElements;
  type IntrinsicAttributes = __VerterAutomaticJSX.IntrinsicAttributes;
}
`;
}

function writeImmutable(fileName: string, content: string): boolean {
  try {
    if (readFileSync(fileName, "utf8") === content) return true;
    return false;
  } catch {
    // The content-addressed target has not been materialized yet.
  }

  const directory = path.dirname(fileName);
  mkdirSync(directory, { recursive: true });
  const temporary = path.join(
    directory,
    `.${path.basename(fileName)}.${process.pid}.${createHash("sha256")
      .update(`${Date.now()}\0${Math.random()}`)
      .digest("hex")
      .slice(0, 16)}.tmp`,
  );
  try {
    writeFileSync(temporary, content, { encoding: "utf8", flag: "wx" });
    try {
      renameSync(temporary, fileName);
      return true;
    } catch {
      return readFileSync(fileName, "utf8") === content;
    }
  } catch {
    try {
      return readFileSync(fileName, "utf8") === content;
    } catch {
      return false;
    }
  } finally {
    rmSync(temporary, { force: true });
  }
}

function collisionFreeBinding(assetKey: string, content: string): string {
  for (let nonce = 0; ; nonce += 1) {
    const suffix = nonce === 0 ? assetKey : hashFields([assetKey, String(nonce)]);
    const candidate = `__verter_vue_jsx_${suffix}`;
    if (!content.includes(candidate)) return candidate;
  }
}

/**
 * Specialize one compiler-owned Vue IDE carrier for the editor tsserver.
 *
 * TypeScript's classic JSX lookup is local to the imported factory namespace,
 * so project-wide React declarations cannot contribute an incompatible
 * `ElementChildrenAttribute`. The adapter aliases only the nearest installed
 * Vue package's official `vue/jsx-runtime` declarations. Its explicit empty
 * local `ElementChildrenAttribute` prevents a fallback to React's unrelated
 * `children` prop convention; Vue JSX children represent slots. Replacing
 * exactly one generated line with one generated line preserves every authored
 * source-map coordinate. A missing or invalid Vue install fails closed by
 * returning `undefined`; callers must retain the original compiler bytes.
 */
export function prepareVueJsxCarrier(
  providerPath: string,
  content: string,
): PreparedVueJsxCarrier | undefined {
  if (!content.startsWith(VUE_JSX_PRAGMA)) return undefined;
  const packageInfo = nearestVuePackage(providerPath);
  if (packageInfo === undefined) return undefined;

  const ownerKey = hashFields([
    packageInfo.version,
    modulePath(packageInfo.root),
    modulePath(packageInfo.jsxRuntimeTypes),
    packageInfo.packageJson,
    packageInfo.runtimeTypes,
  ]);
  const adapterContent = classicAdapter(packageInfo);
  // The adapter is immutable, so its path must change when either the owning
  // Vue package OR Verter's generated adapter schema changes. An owner-only key
  // leaves stale bytes at the same path after an in-place plugin upgrade and
  // correctly makes writeImmutable fail closed, stranding every carrier.
  const assetKey = hashFields([ownerKey, adapterContent]);
  const adapterPath = path.join(
    tmpdir(),
    "verter-host",
    `vue-jsx-tsserver-${assetKey}`,
    "classic.d.ts",
  );
  if (!writeImmutable(adapterPath, adapterContent)) return undefined;

  const binding = collisionFreeBinding(assetKey, content);
  const adapterSpecifier = JSON.stringify(
    modulePath(adapterPath.slice(0, -path.extname(adapterPath).length)),
  );
  const intro = `/** @jsxRuntime classic */ /** @jsx ${binding}.h */ /** @jsxFrag ${binding}.Fragment */ import * as ${binding} from ${adapterSpecifier};\n`;
  return {
    content: `${intro}${content.slice(VUE_JSX_PRAGMA.length)}`,
    adapterPath,
    adapterContent,
  };
}

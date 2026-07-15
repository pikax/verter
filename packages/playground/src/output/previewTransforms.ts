/**
 * Transform helpers for the preview iframe.
 * Converts ES module imports/exports into window.__modules__ assignments
 * so that compiled Vue SFC output can run in a sandboxed iframe without
 * native ES module support.
 */

/** Transform 'as' to ':' for destructuring (import uses 'as', destructuring uses ':') */
export const SVELTE_RUNTIME_FLAGS = ["legacy", "async", "tracing"] as const;
export type SvelteRuntimeFlag = (typeof SVELTE_RUNTIME_FLAGS)[number];

const SVELTE_RUNTIME_FLAG_IMPORT_RE =
  /import\s+['"]svelte\/internal\/flags\/(legacy|async|tracing)['"]\s*;?/g;

/** Collect side-effect runtime flags that must be evaluated before a Svelte component mounts. */
export function collectSvelteRuntimeFlags(code: string): SvelteRuntimeFlag[] {
  const seen = new Set<SvelteRuntimeFlag>();
  for (const match of code.matchAll(SVELTE_RUNTIME_FLAG_IMPORT_RE)) {
    seen.add(match[1] as SvelteRuntimeFlag);
  }
  return [...seen];
}

export function transformImportList(imports: string): string {
  return imports.replace(/(\w+)\s+as\s+(\w+)/g, "$1: $2");
}

/** Transform compiled code to work in preview iframe */
export function transformForPreview(code: string, moduleName: string): string {
  let transformed = code;

  // The iframe loads these framework-owned runtime modules through its import
  // map before evaluating compiled carriers. Preserve the compiler's namespace
  // shape without leaving an ESM import inside the classic-script evaluator.
  transformed = transformed.replace(
    /import\s+\*\s+as\s+([$_A-Za-z][$_\w]*)\s+from\s+['"]svelte\/internal\/client['"]\s*;?/g,
    (_, name) => `const ${name} = window.SvelteInternalClient`,
  );
  transformed = transformed.replace(
    /import\s+['"]svelte\/internal\/disclose-version['"]\s*;?/g,
    "",
  );
  // Runtime flags are preloaded by the iframe before evaluating any compiled
  // module. Leaving these static imports in a classic-script evaluator would be
  // a syntax error; simply dropping them without preloading would break legacy,
  // async, or tracing semantics.
  transformed = transformed.replace(SVELTE_RUNTIME_FLAG_IMPORT_RE, "");

  // Transform: import { x, y as z } from 'vue' -> const { x, y: z } = window.Vue
  transformed = transformed.replace(
    /import\s+\{([^}]+)\}\s+from\s+['"]vue['"]/g,
    (_, imports) => `const {${transformImportList(imports)}} = window.Vue`,
  );

  // Transform: import x from 'vue' -> const x = window.Vue
  transformed = transformed.replace(
    /import\s+(\w+)\s+from\s+['"]vue['"]/g,
    (_, name) => `const ${name} = window.Vue`,
  );

  // Transform: import { x } from './File.vue' -> const { x } = window.__modules__['./File.js']
  transformed = transformed.replace(
    /import\s+\{([^}]+)\}\s+from\s+['"]\.\/([^'"]+)['"]/g,
    (_, imports, path) => {
      const modulePath = "./" + path.replace(/\.(vue|svelte|ts)$/, ".js");
      return `const {${transformImportList(imports)}} = window.__modules__["${modulePath}"]`;
    },
  );

  // Transform: import X from './File.vue' -> const X = window.__modules__['./File.js'].default
  transformed = transformed.replace(
    /import\s+(\w+)\s+from\s+['"]\.\/([^'"]+)['"]/g,
    (_, name, path) => {
      const modulePath = "./" + path.replace(/\.(vue|svelte|ts)$/, ".js");
      return `const ${name} = window.__modules__["${modulePath}"].default`;
    },
  );

  // Transform: export default X -> window.__modules__['moduleName'].default = X
  transformed = transformed.replace(
    /export\s+default\s+/g,
    `window.__modules__["${moduleName}"].default = `,
  );

  // Transform: export function X -> window.__modules__['moduleName'].X = function X
  transformed = transformed.replace(
    /export\s+function\s+(\w+)/g,
    (_, name) => `window.__modules__["${moduleName}"].${name} = function ${name}`,
  );

  // Note: standalone `function render(...)` is NOT transformed here.
  // The mergeRenderIntoComponent step in compiler.ts attaches render to the component
  // via `__sfc__.render = render`, so the function declaration must remain as-is.

  // Transform: export const/let/var X = -> window.__modules__['moduleName'].X =
  transformed = transformed.replace(
    /export\s+(const|let|var)\s+(\w+)\s*=/g,
    (_, _keyword, name) => `window.__modules__["${moduleName}"].${name} =`,
  );

  // Transform: export { x, y } -> Object.assign(window.__modules__['moduleName'], { x, y })
  transformed = transformed.replace(/export\s+\{([^}]+)\}/g, (_, exports) => {
    const items = exports
      .split(",")
      .map((e: string) => {
        const parts = e.trim().split(/\s+as\s+/);
        const name = parts[0];
        const alias = parts[1] || name;
        return `${alias}: ${name}`;
      })
      .join(", ");
    return `Object.assign(window.__modules__["${moduleName}"], { ${items} })`;
  });

  return transformed;
}

/**
 * Extract local module paths (./X.js) that appear as `window.__modules__["./X.js"]`
 * in already-transformed preview code.
 */
export function extractLocalImports(code: string): string[] {
  const seen = new Set<string>();
  const re = /window\.__modules__\["(\.\/[^"]+)"\]/g;
  let match;
  while ((match = re.exec(code)) !== null) {
    seen.add(match[1]);
  }
  return [...seen];
}

/**
 * Topological sort of filenames by dependency order (Kahn's algorithm).
 * Dependencies evaluate before dependents so that `window.__modules__` is populated.
 *
 * @param files - Map of filename → transformed compiled JS
 * @param mainFile - The entry point filename (goes last)
 * @returns Ordered list of filenames with non-empty JS
 */
export function orderScriptsByDependency(
  files: Record<string, string>,
  mainFile: string,
): string[] {
  // Build filename → module name mapping and reverse
  const filenameToModule = new Map<string, string>();
  const moduleToFilename = new Map<string, string>();
  const nonEmpty: string[] = [];

  for (const [filename, code] of Object.entries(files)) {
    if (!code) continue;
    nonEmpty.push(filename);
    const moduleName = "./" + filename.replace(/\.(vue|svelte|ts)$/, ".js");
    filenameToModule.set(filename, moduleName);
    moduleToFilename.set(moduleName, filename);
  }

  if (nonEmpty.length <= 1) return nonEmpty;

  // Build adjacency: edges[A] = [B] means A depends on B (B must come first)
  const deps = new Map<string, Set<string>>();
  const inDegree = new Map<string, number>();

  for (const f of nonEmpty) {
    deps.set(f, new Set());
    inDegree.set(f, 0);
  }

  for (const filename of nonEmpty) {
    const imports = extractLocalImports(files[filename]);
    for (const imp of imports) {
      const depFilename = moduleToFilename.get(imp);
      if (depFilename && depFilename !== filename) {
        deps.get(filename)!.add(depFilename);
      }
    }
  }

  // Compute in-degrees
  for (const [filename, depSet] of deps) {
    for (const dep of depSet) {
      inDegree.set(dep, (inDegree.get(dep) ?? 0) + 1);
    }
  }

  // Kahn's: start with nodes that have no dependents (in-degree 0 = leaves)
  // We want leaves first, so nodes with in-degree 0 from the *reverse* graph.
  // Actually, we want: if A depends on B, B comes first.
  // In-degree counts how many files depend ON this file. High in-degree = needed early.
  // Kahn's on forward graph (A→B means A depends on B):
  //   Process nodes with in-degree 0 in the REVERSE graph = nodes nobody depends on = leaf consumers.
  // Easier: reverse the perspective. Build graph where edge B→A means "B must come before A".
  // Then Kahn's on that graph gives the right order.

  // Rebuild: edge from dep → filename (dep must come before filename)
  const mustComeBefore = new Map<string, Set<string>>();
  const reverseDegree = new Map<string, number>();

  for (const f of nonEmpty) {
    mustComeBefore.set(f, new Set());
    reverseDegree.set(f, 0);
  }

  for (const [filename, depSet] of deps) {
    for (const dep of depSet) {
      mustComeBefore.get(dep)!.add(filename);
      reverseDegree.set(filename, (reverseDegree.get(filename) ?? 0) + 1);
    }
  }

  const queue: string[] = [];
  for (const [f, deg] of reverseDegree) {
    if (deg === 0) queue.push(f);
  }

  const result: string[] = [];
  while (queue.length > 0) {
    const node = queue.shift()!;
    result.push(node);
    for (const dependent of mustComeBefore.get(node) ?? []) {
      const newDeg = reverseDegree.get(dependent)! - 1;
      reverseDegree.set(dependent, newDeg);
      if (newDeg === 0) queue.push(dependent);
    }
  }

  // Cycle fallback: if not all nodes were visited, add remaining (non-main first)
  if (result.length < nonEmpty.length) {
    const inResult = new Set(result);
    const remaining = nonEmpty.filter((f) => !inResult.has(f));
    // Put non-main remaining files before main
    const nonMain = remaining.filter((f) => f !== mainFile);
    const main = remaining.filter((f) => f === mainFile);
    result.push(...nonMain, ...main);
  }

  return result;
}

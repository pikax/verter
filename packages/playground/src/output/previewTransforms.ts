/**
 * Transform helpers for the preview iframe.
 * Converts ES module imports/exports into window.__modules__ assignments
 * so that compiled Vue SFC output can run in a sandboxed iframe without
 * native ES module support.
 */

/** Transform 'as' to ':' for destructuring (import uses 'as', destructuring uses ':') */
export function transformImportList(imports: string): string {
  return imports.replace(/(\w+)\s+as\s+(\w+)/g, "$1: $2");
}

/** Transform compiled code to work in preview iframe */
export function transformForPreview(code: string, moduleName: string): string {
  let transformed = code;

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
      const modulePath = "./" + path.replace(/\.(vue|ts)$/, ".js");
      return `const {${transformImportList(imports)}} = window.__modules__["${modulePath}"]`;
    },
  );

  // Transform: import X from './File.vue' -> const X = window.__modules__['./File.js'].default
  transformed = transformed.replace(
    /import\s+(\w+)\s+from\s+['"]\.\/([^'"]+)['"]/g,
    (_, name, path) => {
      const modulePath = "./" + path.replace(/\.(vue|ts)$/, ".js");
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

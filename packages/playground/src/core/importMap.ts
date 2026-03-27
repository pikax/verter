export interface ImportMap {
  imports: Record<string, string>;
  scopes?: Record<string, Record<string, string>>;
}

export function getDefaultImportMap(vueVersion = "3.5.26"): ImportMap {
  return {
    imports: {
      vue: `https://cdn.jsdelivr.net/npm/vue@${vueVersion}/dist/vue.esm-browser.js`,
      "vue/server-renderer": `https://cdn.jsdelivr.net/npm/@vue/server-renderer@${vueVersion}/dist/server-renderer.esm-browser.js`,
    },
  };
}

export function mergeImportMap(a: ImportMap, b: ImportMap): ImportMap {
  return {
    imports: { ...a.imports, ...b.imports },
    scopes: { ...a.scopes, ...b.scopes },
  };
}

const vueVersionRe = /cdn\.jsdelivr\.net\/npm\/vue@([^/]+)\//;

export function extractVueVersion(importMap: ImportMap): string | undefined {
  const vueUrl = importMap.imports?.vue;
  if (!vueUrl) return undefined;
  const match = vueUrl.match(vueVersionRe);
  return match?.[1];
}

export function isDefaultImport(key: string, value: string, vueVersion?: string): boolean {
  if (!vueVersion) return false;
  const defaults = getDefaultImportMap(vueVersion);
  return defaults.imports[key] === value;
}

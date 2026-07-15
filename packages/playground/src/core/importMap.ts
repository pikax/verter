export interface ImportMap {
  imports: Record<string, string>;
  scopes?: Record<string, Record<string, string>>;
}

export const SVELTE_RUNTIME_VERSION = "5.56.3";

export function getDefaultImportMap(vueVersion = "3.5.26"): ImportMap {
  const svelteBase = `https://cdn.jsdelivr.net/npm/svelte@${SVELTE_RUNTIME_VERSION}`;
  return {
    imports: {
      vue: `https://cdn.jsdelivr.net/npm/vue@${vueVersion}/dist/vue.esm-browser.js`,
      "vue/server-renderer": `https://cdn.jsdelivr.net/npm/@vue/server-renderer@${vueVersion}/dist/server-renderer.esm-browser.js`,
      svelte: `${svelteBase}/src/index-client.js`,
      "svelte/internal/client": `${svelteBase}/src/internal/client/index.js`,
      "svelte/internal/server": `${svelteBase}/src/internal/server/index.js`,
      "svelte/internal/disclose-version": `${svelteBase}/src/internal/disclose-version.js`,
      "svelte/internal/flags/legacy": `${svelteBase}/src/internal/flags/legacy.js`,
      "svelte/internal/flags/async": `${svelteBase}/src/internal/flags/async.js`,
      "svelte/internal/flags/tracing": `${svelteBase}/src/internal/flags/tracing.js`,
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

export interface ImportMap {
  imports: Record<string, string>
  scopes?: Record<string, Record<string, string>>
}

export function getDefaultImportMap(vueVersion = '3.5.26'): ImportMap {
  return {
    imports: {
      vue: `https://cdn.jsdelivr.net/npm/vue@${vueVersion}/dist/vue.esm-browser.js`,
      'vue/server-renderer': `https://cdn.jsdelivr.net/npm/@vue/server-renderer@${vueVersion}/dist/server-renderer.esm-browser.js`,
    },
  }
}

export function mergeImportMap(a: ImportMap, b: ImportMap): ImportMap {
  return {
    imports: { ...a.imports, ...b.imports },
    scopes: { ...a.scopes, ...b.scopes },
  }
}

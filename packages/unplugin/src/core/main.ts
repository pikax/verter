import type { ViteCodegenResult } from "@verter/native";
import type { HmrStrategy } from "./types";
import { basename } from "path";
import { EXPORT_HELPER_ID } from "./constants";

/**
 * Extract component name from filename and sanitize to a valid JS identifier.
 * e.g., "SplitPane.vue" → "SplitPane", "app-bar-first.vue" → "app_bar_first",
 * "404.vue" → "_404"
 */
function extractComponentName(filename: string): string {
  let name = basename(filename).replace(/\.vue$/, "");
  name = name.replace(/[^a-zA-Z0-9_$]/g, "_");
  if (/^[0-9]/.test(name)) {
    name = "_" + name;
  }
  return name;
}

export interface MainModuleOptions {
  filename: string;
  scopeId: string;
  ssr: boolean;
  isProd: boolean;
  hmr: HmrStrategy;
}

/**
 * Generate the main module output by assembling split blocks.
 */
export function generateMainModule(result: ViteCodegenResult, options: MainModuleOptions): string {
  const { filename, scopeId, ssr, isProd, hmr } = options;

  const lines: string[] = [];

  // 1. Import styles as virtual modules
  result.styles.forEach((style, index) => {
    const query = new URLSearchParams();
    query.set("vue", "");
    query.set("type", "style");
    query.set("index", String(index));
    if (style.lang) query.set("lang", style.lang);
    if (style.scoped) query.set("scoped", "true");
    if (style.is_module) query.set("module", "true");

    const lang = style.lang || "css";
    lines.push(`import "${filename}?${query.toString()}&lang.${lang}"`);
  });

  if (result.styles.length > 0) {
    lines.push("");
  }

  // 2. Add script code (component definition)
  let hasDefaultExport = false;
  if (result.script) {
    let scriptCode = result.script.code;

    hasDefaultExport = scriptCode.includes("export default");
    if (hasDefaultExport) {
      // Replace the LAST "export default " occurrence — the real export statement.
      // Earlier occurrences may be inside comments or string literals (e.g. Preview.vue).
      const marker = "export default ";
      const lastIdx = scriptCode.lastIndexOf(marker);
      if (lastIdx !== -1) {
        scriptCode =
          scriptCode.substring(0, lastIdx) +
          "const _sfc_main = " +
          scriptCode.substring(lastIdx + marker.length);
      }
    }

    lines.push(scriptCode);
    lines.push("");
  }

  // 3. Add template code (render function)
  if (result.template) {
    lines.push(result.template.code);
    lines.push("");
  }

  // 4. Attach render function to component
  // The render function may come from a separate template block, or be included
  // directly in the script code by the Rust compiler (non-inline template mode).
  const hasRenderFunction = result.template || result.script?.code.includes("function render(");
  if (hasRenderFunction && hasDefaultExport) {
    lines.push("_sfc_main.render = render");
  }

  // 5. Apply metadata and export
  const hasScoped = result.styles.some((s) => s.scoped);
  const metadataProps: string[] = [];

  if (hasScoped) {
    metadataProps.push(`["__scopeId", "data-v-${scopeId}"]`);
  }
  if (!isProd && hasDefaultExport) {
    metadataProps.push(`["__file", ${JSON.stringify(filename)}]`);
  }

  // CSS Modules: inject __cssModules for useCssModule() runtime support
  const moduleStyles = result.styles.filter(
    (s) => s.is_module && s.module_classes.length > 0,
  );
  if (moduleStyles.length > 0) {
    lines.push(`const __cssModules = {}`);
    for (const style of moduleStyles) {
      const moduleName = style.module_name || "$style";
      const classesObj = style.module_classes
        .map(([orig, hashed]) => `"${orig}":"${hashed}"`)
        .join(",");
      lines.push(`__cssModules["${moduleName}"] = {${classesObj}}`);
    }
    metadataProps.push(`["__cssModules", __cssModules]`);
  }

  if (metadataProps.length > 0 && hasDefaultExport) {
    lines.push(`import _export_sfc from "${EXPORT_HELPER_ID}"`);
    const componentName = extractComponentName(filename);
    lines.push(
      `const ${componentName} = /* @__PURE__ */ _export_sfc(_sfc_main, [${metadataProps.join(", ")}])`,
    );
  }

  // 6. HMR setup (development only)
  if (!isProd && !ssr && hasDefaultExport) {
    if (hmr === "vite") {
      lines.push("");
      lines.push(`/* Hot Module Replacement */`);
      lines.push(`if (import.meta.hot) {`);
      lines.push(`  _sfc_main.__hmrId = "${scopeId}"`);
      lines.push(`  const __VUE_HMR_RUNTIME__ = globalThis.__VUE_HMR_RUNTIME__`);
      lines.push(`  if (__VUE_HMR_RUNTIME__) {`);
      lines.push(`    if (!__VUE_HMR_RUNTIME__.createRecord("${scopeId}", _sfc_main)) {`);
      lines.push(`      __VUE_HMR_RUNTIME__.reload("${scopeId}", _sfc_main)`);
      lines.push(`    }`);
      lines.push(`  }`);
      lines.push(`  import.meta.hot.accept((mod) => {`);
      lines.push(`    if (!mod) return`);
      lines.push(`    const { default: updated, _rerender_only } = mod`);
      lines.push(`    if (_rerender_only) {`);
      lines.push(`      __VUE_HMR_RUNTIME__?.rerender("${scopeId}", updated.render)`);
      lines.push(`    } else {`);
      lines.push(`      __VUE_HMR_RUNTIME__?.reload("${scopeId}", updated)`);
      lines.push(`    }`);
      lines.push(`  })`);
      lines.push(`}`);
    } else if (hmr === "webpack") {
      lines.push("");
      lines.push(`/* Hot Module Replacement */`);
      lines.push(`if (module.hot) {`);
      lines.push(`  _sfc_main.__hmrId = "${scopeId}"`);
      lines.push(`  const __VUE_HMR_RUNTIME__ = globalThis.__VUE_HMR_RUNTIME__`);
      lines.push(`  if (__VUE_HMR_RUNTIME__) {`);
      lines.push(`    if (!__VUE_HMR_RUNTIME__.createRecord("${scopeId}", _sfc_main)) {`);
      lines.push(`      __VUE_HMR_RUNTIME__.reload("${scopeId}", _sfc_main)`);
      lines.push(`    }`);
      lines.push(`  }`);
      lines.push(`  module.hot.accept((err) => {`);
      lines.push(`    if (err) {`);
      lines.push(`      __VUE_HMR_RUNTIME__?.reload("${scopeId}", _sfc_main)`);
      lines.push(`    }`);
      lines.push(`  })`);
      lines.push(`}`);
    }
    // hmr === "none" — skip HMR code entirely
  }

  // 7. Export the component
  if (hasDefaultExport) {
    lines.push("");
    if (metadataProps.length > 0) {
      const componentName = extractComponentName(filename);
      lines.push(`export default ${componentName}`);
    } else {
      lines.push(`export default _sfc_main`);
    }
  }

  return lines.join("\n");
}

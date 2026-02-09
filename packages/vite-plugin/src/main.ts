import type { ViteCodegenResult } from "@verter/native";
import { basename } from "path";

const EXPORT_HELPER_ID = "\0plugin-vue:export-helper";

/**
 * Extract component name from filename and sanitize to a valid JS identifier.
 * e.g., "SplitPane.vue" → "SplitPane", "app-bar-first.vue" → "app_bar_first",
 * "404.vue" → "_404"
 */
function extractComponentName(filename: string): string {
  let name = basename(filename).replace(/\.vue$/, "");
  // Replace any character that isn't a valid JS identifier char with underscore
  name = name.replace(/[^a-zA-Z0-9_$]/g, "_");
  // Prefix with underscore if starts with a digit
  if (/^[0-9]/.test(name)) {
    name = "_" + name;
  }
  return name;
}

/**
 * Options for main module generation
 */
export interface MainModuleOptions {
  filename: string;
  scopeId: string;
  ssr: boolean;
  isProd: boolean;
}

/**
 * Generate the main module output by assembling split blocks.
 *
 * This generates the compiled Vue component with:
 * 1. Style virtual module imports (so Vite processes CSS)
 * 2. Script code (component definition with `export default` → `const _sfc_main =`)
 * 3. Template code (render function)
 * 4. `_sfc_main.render = render` attachment
 * 5. Metadata (__scopeId, __file, __hmrId)
 * 6. HMR setup in development
 * 7. Export _sfc_main
 */
export function generateMainModule(result: ViteCodegenResult, options: MainModuleOptions): string {
  const { filename, scopeId, ssr, isProd } = options;

  const lines: string[] = [];

  // 1. Import styles as virtual modules (Vite processes these through CSS pipeline)
  result.styles.forEach((style, index) => {
    const query = new URLSearchParams();
    query.set("vue", "");
    query.set("type", "style");
    query.set("index", String(index));
    if (style.lang) query.set("lang", style.lang);
    if (style.scoped) query.set("scoped", "true");
    if (style.isModule) query.set("module", "true");

    // Append &lang.css (or &lang.scss etc.) so Vite routes this through the CSS pipeline
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

    // Replace "export default" with a variable assignment so we can add HMR/render
    hasDefaultExport = scriptCode.includes("export default");
    if (hasDefaultExport) {
      scriptCode = scriptCode.replace(/export default\s+/, "const _sfc_main = ");
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
  if (result.template && hasDefaultExport) {
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

  // Use _export_sfc helper when there are metadata props to apply
  // This matches @vitejs/plugin-vue's behavior
  if (metadataProps.length > 0 && hasDefaultExport) {
    lines.push(`import _export_sfc from "${EXPORT_HELPER_ID}"`);
    const componentName = extractComponentName(filename);
    lines.push(
      `const ${componentName} = /* @__PURE__ */ _export_sfc(_sfc_main, [${metadataProps.join(", ")}])`,
    );
  }

  // 6. HMR setup (development only)
  if (!isProd && !ssr && hasDefaultExport) {
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
  }

  // 7. Export the component
  if (hasDefaultExport) {
    lines.push("");
    if (metadataProps.length > 0) {
      // Export with named component (matches @vitejs/plugin-vue behavior)
      const componentName = extractComponentName(filename);
      lines.push(`export default ${componentName}`);
    } else {
      lines.push(`export default _sfc_main`);
    }
  }

  return lines.join("\n");
}

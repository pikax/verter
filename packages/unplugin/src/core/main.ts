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

// napi-rs converts snake_case to camelCase at runtime, but TS types use snake_case.
// These helpers access the correct field regardless of naming convention.
function styleIsModule(s: any): boolean {
  return s.isModule ?? s.is_module ?? false;
}
function styleModuleClasses(s: any): [string, string][] {
  return s.moduleClasses ?? s.module_classes ?? [];
}
function styleModuleName(s: any): string | undefined {
  return s.moduleName ?? s.module_name;
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
    if (styleIsModule(style)) query.set("module", "true");

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

    // Hoist all import statements to the top.
    // The Rust compiler may emit vapor template imports after `export default`,
    // which is invalid ESM. Collect and hoist them.
    const importLines: string[] = [];
    scriptCode = scriptCode.replace(
      /^import\s+\{[^}]+\}\s+from\s+['"][^'"]+['"];?\s*$/gm,
      (match) => {
        importLines.push(match.trim());
        return "";
      },
    );
    if (importLines.length > 0) {
      lines.push(...importLines);
      lines.push("");
    }

    hasDefaultExport = scriptCode.includes("export default");
    if (hasDefaultExport) {
      scriptCode = scriptCode.replace(/export default\s+/, "const _sfc_main = ");
    }

    // Strip `export` from `export function render(...)` — vapor puts an exported
    // render function after the component definition, but we attach it via
    // `_sfc_main.render = render` instead.
    scriptCode = scriptCode.replace(/^export function render\b/m, "function render");

    // Vapor: the Rust compiler may place the template code (template constants,
    // delegateEvents, render function) INSIDE the setup() closure.  We must
    // extract it so the render function is at module scope, and ensure setup()
    // returns the bindings that the render function accesses via _ctx.
    let vaporTemplateBlock = "";
    const templateStart = scriptCode.search(/\nconst t\d+ = _template\(/);
    if (templateStart !== -1 && /\bfunction render\b/.test(scriptCode)) {
      // Find the end of the render function by brace-matching from `function render(`
      const renderIdx = scriptCode.indexOf("function render(", templateStart);
      if (renderIdx !== -1) {
        const openBrace = scriptCode.indexOf("{", renderIdx);
        if (openBrace !== -1) {
          let depth = 1;
          let i = openBrace + 1;
          while (i < scriptCode.length && depth > 0) {
            if (scriptCode[i] === "{") depth++;
            else if (scriptCode[i] === "}") depth--;
            i++;
          }
          // i now points right after the closing } of render
          vaporTemplateBlock = scriptCode.slice(templateStart, i);
          scriptCode = scriptCode.slice(0, templateStart) + scriptCode.slice(i);

          // In production mode the Rust compiler omits `return __returned__`,
          // so setup() variables become dead code.  Collect _ctx.xxx references
          // from the render function and synthesise a return statement.
          if (!scriptCode.includes("return __returned__")) {
            const ctxRefs = new Set<string>();
            vaporTemplateBlock.replace(/_ctx\.(\w+)/g, (_, name: string) => {
              ctxRefs.add(name);
              return "";
            });
            if (ctxRefs.size > 0) {
              const returnObj = Array.from(ctxRefs).join(", ");
              // Insert before the final }});  that closes setup + defineComponent
              scriptCode = scriptCode.replace(
                /(\}\}\);?\s*)$/,
                `return { ${returnObj} }\n$1`,
              );
            }
          }
        }
      }
    }

    lines.push(scriptCode);
    lines.push("");

    if (vaporTemplateBlock) {
      lines.push(vaporTemplateBlock);
      lines.push("");
    }
  }

  // 3. Add template code (render function)
  // The Rust compiler may put the render function in result.template or inline it
  // in result.script.code. Detect either case.
  const hasRenderInScript = result.script ? /\bfunction render\b/.test(result.script.code) : false;
  if (result.template) {
    lines.push(result.template.code);
    lines.push("");
  }

  // 4. Attach render function to component
  if ((result.template || hasRenderInScript) && hasDefaultExport) {
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
    (s) => styleIsModule(s) && styleModuleClasses(s).length > 0,
  );
  if (moduleStyles.length > 0) {
    lines.push(`const __cssModules = {}`);
    for (const style of moduleStyles) {
      const moduleName = styleModuleName(style) || "$style";
      const classesObj = styleModuleClasses(style)
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

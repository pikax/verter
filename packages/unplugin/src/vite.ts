import type { Plugin } from "vite";
import { createRequire } from "node:module";
import { join } from "node:path";
import unplugin from "./index";
import type { VerterPluginOptions } from "./core/types";
export type { Options, VerterPluginOptions } from "./core/types";
export { parseVueRequest } from "./core/utils";

/**
 * Vite plugin factory that returns verter's main plugin.
 *
 * The plugin is named `vite:vue` (matching `@vitejs/plugin-vue`) so that
 * downstream plugins that discover the Vue plugin by name work correctly:
 * - `unplugin-vue-macros` wraps it and intercepts hooks
 * - `unplugin-vue-i18n` finds it via `config.plugins.find(p => p.name === 'vite:vue')`
 * - `unplugin-vue-short-vmodel` checks for the Vue plugin by name
 *
 * Returns a **single** Plugin object so callers that expect a single
 * plugin (e.g., `unplugin-vue-macros`) work correctly. Previous versions
 * returned `[mainPlugin, compatPlugin]` which broke VueMacros.
 */
export default function verterVitePlugin(
  options?: VerterPluginOptions,
): Plugin {
  const mainPlugin = unplugin.vite(options) as Plugin;

  // Rename to match @vitejs/plugin-vue — we are the drop-in replacement
  mainPlugin.name = "vite:vue";

  // Build the compat API object that downstream plugins look for.
  // VueMacros checks `api.version` to validate the Vue plugin;
  // @vitejs/plugin-vue exposes its version here.
  const compatApi = {
    version: "3.5.0",
    options: {
      isProduction: false,
      root: process.cwd(),
      template: options?.template ?? {},
      compiler: null as any,
    },
  };

  // Expose the compat API on the plugin
  (mainPlugin as any).api = compatApi;

  // Chain into the existing configResolved hook
  const origConfigResolved = (mainPlugin as any).configResolved;
  (mainPlugin as any).configResolved = function (config: any) {
    origConfigResolved?.call(this, config);

    compatApi.options.isProduction = config.command === "build";
    compatApi.options.root = config.root;

    if (!compatApi.options.compiler) {
      try {
        const _require = createRequire(join(config.root, "package.json"));
        compatApi.options.compiler = _require("@vue/compiler-sfc");
      } catch {
        // compiler-sfc not available — plugins that need it will see null
      }
    }
  };

  return mainPlugin;
}

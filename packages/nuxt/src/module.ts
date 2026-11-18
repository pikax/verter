import { defineNuxtModule } from "@nuxt/kit";
import verterVitePlugin from "@verter/unplugin/vite";
import type { VerterPluginOptions } from "@verter/unplugin/vite";
import { installVerterPlugin } from "./install";
import { loadNuxtPathAliases, resolveNuxtAlias } from "./aliases";

export type { VerterPluginOptions };

export interface VerterNuxtOptions extends VerterPluginOptions {
  /** Also remove vite:vue-jsx plugin. @default false */
  replaceJsx?: boolean;
}

// Augment Nuxt schema for autocomplete in nuxt.config.ts
declare module "@nuxt/schema" {
  interface NuxtConfig {
    verter?: VerterNuxtOptions;
  }
  interface NuxtOptions {
    verter?: VerterNuxtOptions;
  }
}

export default defineNuxtModule<VerterNuxtOptions>({
  meta: {
    name: "@verter/nuxt",
    configKey: "verter",
    compatibility: { nuxt: ">=3.0.0" },
  },
  setup(options, nuxt) {
    // Forward Nuxt's vite.vue template options if user didn't set verter.template
    const nuxtVueOpts = nuxt.options.vite?.vue;
    if (nuxtVueOpts?.template && !options.template) {
      options.template = nuxtVueOpts.template;
    }

    // Load Nuxt path aliases from .nuxt/tsconfig.json for #-prefixed import resolution.
    // Falls back to hardcoded core aliases if .nuxt/tsconfig.json doesn't exist yet
    // (i.e., user hasn't run `nuxt prepare`).
    const rootDir = nuxt.options.rootDir;
    const nuxtAliases = loadNuxtPathAliases(rootDir);

    // Use vite:configResolved — Nuxt adds vite:vue AFTER vite:extendConfig
    // but BEFORE vite:configResolved (proven in integration tests)
    nuxt.hook("vite:configResolved", (config) => {
      // Replace Nuxt's built-in vite:vue (and optionally vite:vue-jsx) in place:
      // the config may be a shallow copy, so the array itself must be mutated.
      const { replaceJsx: _, ...verterOpts } = options;
      verterOpts.resolveAlias = (id: string) => resolveNuxtAlias(id, nuxtAliases, rootDir);
      const plugins = config.plugins;
      if (!plugins) {
        throw new Error(
          "[verter] vite:configResolved received a config without a plugins array; the stock vue plugin cannot be replaced.",
        );
      }
      installVerterPlugin(plugins as unknown[], () => verterVitePlugin(verterOpts), {
        replaceJsx: !!options.replaceJsx,
      });
    });
  },
});

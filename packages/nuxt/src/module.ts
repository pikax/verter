import { defineNuxtModule } from "@nuxt/kit";
import verterVitePlugin from "@verter/unplugin/vite";
import type { VerterPluginOptions } from "@verter/unplugin/vite";
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
      // Remove Nuxt's built-in vite:vue (and optionally vite:vue-jsx)
      config.plugins = (config.plugins || []).filter((p) => {
        if (p && typeof p === "object" && "name" in p) {
          if (p.name === "vite:vue") return false;
          if (options.replaceJsx && p.name === "vite:vue-jsx") return false;
        }
        return true;
      });

      // Add Verter plugin with Nuxt alias resolver
      const { replaceJsx: _, ...verterOpts } = options;
      verterOpts.resolveAlias = (id: string) => resolveNuxtAlias(id, nuxtAliases, rootDir);
      config.plugins.push(verterVitePlugin(verterOpts));
    });
  },
});

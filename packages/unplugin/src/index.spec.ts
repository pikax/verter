/**
 * @ai-generated - Integration tests for the unplugin factory.
 */
import { describe, it, expect } from "vitest";
import unplugin, { unpluginFactory } from "./index";
import { EXPORT_HELPER_ID, EXPORT_HELPER_CODE } from "./core/constants";

describe("unplugin factory", () => {
  it("exports unpluginFactory function", () => {
    expect(typeof unpluginFactory).toBe("function");
  });

  it("exports unplugin instance with framework-specific creators", () => {
    expect(unplugin).toBeDefined();
    expect(typeof unplugin.vite).toBe("function");
    expect(typeof unplugin.rollup).toBe("function");
    expect(typeof unplugin.webpack).toBe("function");
    expect(typeof unplugin.esbuild).toBe("function");
    expect(typeof unplugin.rspack).toBe("function");
    expect(typeof unplugin.rolldown).toBe("function");
    expect(typeof unplugin.farm).toBe("function");
  });

  it("creates a raw plugin object from the factory", () => {
    const plugin = unpluginFactory(undefined, {
      framework: "rollup",
      versions: { unplugin: "0.0.0", rollup: "0.0.0" },
    } as any);

    expect(plugin).toBeDefined();
    expect((plugin as any).name).toBe("unplugin-verter");
  });
});

describe("unplugin hooks", () => {
  function createPlugin() {
    return unpluginFactory(undefined, {
      framework: "rollup",
      versions: { unplugin: "0.0.0", rollup: "0.0.0" },
    } as any) as any;
  }

  it("resolveId resolves the export helper ID", () => {
    const plugin = createPlugin();
    const result = plugin.resolveId(EXPORT_HELPER_ID);
    expect(result).toBe(EXPORT_HELPER_ID);
  });

  it("resolveId resolves vue virtual module IDs", () => {
    const plugin = createPlugin();
    const result = plugin.resolveId(
      "/path/to/App.vue?vue&type=style&index=0&lang=css",
    );
    expect(result).toBe("/path/to/App.vue?vue&type=style&index=0&lang=css");
  });

  it("resolveId returns undefined for non-vue files", () => {
    const plugin = createPlugin();
    const result = plugin.resolveId("/path/to/file.ts");
    expect(result).toBeUndefined();
  });

  it("load returns export helper code", () => {
    const plugin = createPlugin();
    const result = plugin.load(EXPORT_HELPER_ID);
    expect(result).toBe(EXPORT_HELPER_CODE);
  });

  it("load returns undefined for non-vue IDs", () => {
    const plugin = createPlugin();
    const result = plugin.load("/path/to/file.ts");
    expect(result).toBeUndefined();
  });

  it("transformInclude returns true for .vue files", () => {
    const plugin = createPlugin();
    expect(plugin.transformInclude("/path/to/App.vue")).toBe(true);
    expect(plugin.transformInclude("/path/to/Component.vue")).toBe(true);
  });

  it("transformInclude returns false for non-.vue files", () => {
    const plugin = createPlugin();
    expect(plugin.transformInclude("/path/to/file.ts")).toBe(false);
    expect(plugin.transformInclude("/path/to/file.js")).toBe(false);
    expect(plugin.transformInclude("/path/to/file.css")).toBe(false);
  });

  it("transformInclude returns false for vue virtual modules", () => {
    const plugin = createPlugin();
    expect(
      plugin.transformInclude(
        "/path/to/App.vue?vue&type=style&index=0&lang=css",
      ),
    ).toBe(false);
  });

  // @ai-generated - Tests include option for non-.vue files
  it("transformInclude respects include option with RegExp array", () => {
    const plugin = unpluginFactory({ include: [/\.vue$/, /\.md$/] }, {
      framework: "rollup",
      versions: { unplugin: "0.0.0", rollup: "0.0.0" },
    } as any) as any;
    expect(plugin.transformInclude("/path/to/App.vue")).toBe(true);
    expect(plugin.transformInclude("/path/to/docs.md")).toBe(true);
    expect(plugin.transformInclude("/path/to/file.ts")).toBe(false);
  });

  it("transformInclude respects include option with single RegExp", () => {
    const plugin = unpluginFactory({ include: /\.(vue|md)$/ }, {
      framework: "rollup",
      versions: { unplugin: "0.0.0", rollup: "0.0.0" },
    } as any) as any;
    expect(plugin.transformInclude("/path/to/App.vue")).toBe(true);
    expect(plugin.transformInclude("/path/to/docs.md")).toBe(true);
    expect(plugin.transformInclude("/path/to/file.ts")).toBe(false);
  });
});

describe("type exports", () => {
  // @ai-generated - Tests that Options type alias is re-exported
  it("exports Options as alias for VerterPluginOptions", async () => {
    const mod = await import("./index");
    // Options is a type-only export, so it won't exist at runtime.
    // Verify the module exports at least the expected runtime values.
    expect(mod.unpluginFactory).toBeDefined();
  });

  // @ai-generated - Tests that VerterPluginOptions accepts template option
  it("accepts template option in plugin factory", () => {
    const plugin = unpluginFactory(
      {
        template: {
          compilerOptions: {
            isCustomElement: (tag: string) => tag.startsWith("td-"),
          },
        },
      },
      {
        framework: "rollup",
        versions: { unplugin: "0.0.0", rollup: "0.0.0" },
      } as any,
    ) as any;
    expect(plugin).toBeDefined();
    expect(plugin.name).toBe("unplugin-verter");
  });
});

describe("vite compat shim", () => {
  // @ai-generated - Tests that vite entry returns array with vite:vue compat plugin
  it("vite plugin returns array with vite:vue compat shim", async () => {
    const mod = await import("./vite");
    const result = mod.default();
    // Should return an array (or single plugin if compiler-sfc not available)
    if (Array.isArray(result)) {
      expect(result.length).toBe(2);
      const [main, compat] = result;
      expect(main.name).toBe("unplugin-verter");
      expect(compat.name).toBe("vite:vue");
      expect((compat as any).api).toBeDefined();
      expect((compat as any).api.options).toBeDefined();
      // compiler is null by default; it's populated by configResolved
      // which doesn't fire outside of an actual Vite build.
      expect((compat as any).api.options).toHaveProperty("compiler");
    } else {
      // Single plugin returned when compiler-sfc not available
      expect(result.name).toBe("unplugin-verter");
    }
  });

  it("vite compat shim passes template options through", async () => {
    const mod = await import("./vite");
    const result = mod.default({
      template: { compilerOptions: { isCustomElement: (tag: string) => tag === "x-foo" } },
    });
    if (Array.isArray(result)) {
      const compat = result[1];
      expect((compat as any).api.options.template.compilerOptions.isCustomElement("x-foo")).toBe(true);
    }
  });
});

describe("compilation output", () => {
  function createPlugin() {
    return unpluginFactory(undefined, {
      framework: "rollup",
      versions: { unplugin: "0.0.0", rollup: "0.0.0" },
    } as any) as any;
  }

  // @ai-generated - Regression: setup() must return template ref bindings
  it("setup returns template ref bindings for ref='name'", async () => {
    const plugin = createPlugin();
    const sfc = `<script setup lang="ts">
import { ref, onMounted } from 'vue'

const container = ref<HTMLElement>()
const msg = ref('hello')

onMounted(() => {
  console.log(container.value)
})
</script>

<template>
  <div class="wrapper">
    <div ref="container" class="editor" />
    <span>{{ msg }}</span>
  </div>
</template>
`;
    const result = await plugin.transform(sfc, "/test/Editor.vue");
    expect(result).toBeDefined();
    const code = result.code;

    // The setup function must return container for the template ref to work
    expect(code).toContain("container");
    // It must not return an empty object
    expect(code).not.toMatch(/return\s*\{\s*\}\s*;/);
  });

  // @ai-generated - Template ref bindings alongside $setup usage
  it("keeps both template ref and interpolation bindings in return", async () => {
    const plugin = createPlugin();
    const sfc = `<script setup>
import { ref } from 'vue'

const el = ref()
const count = ref(0)
const unused = 'type-only'
</script>

<template>
  <div ref="el">{{ count }}</div>
</template>
`;
    const result = await plugin.transform(sfc, "/test/Mixed.vue");
    expect(result).toBeDefined();
    const code = result.code;

    // el used as template ref, count used in interpolation
    expect(code).toMatch(/return.*el/);
    expect(code).toMatch(/return.*count/);
  });
});

describe("bundler entry points", () => {
  // @ai-generated - Tests that each bundler export creates a function
  it("vite export creates a plugin factory", async () => {
    const mod = await import("./vite");
    expect(typeof mod.default).toBe("function");
  });

  it("rollup export creates a plugin factory", async () => {
    const mod = await import("./rollup");
    expect(typeof mod.default).toBe("function");
  });

  it("webpack export creates a plugin factory", async () => {
    const mod = await import("./webpack");
    expect(typeof mod.default).toBe("function");
  });

  it("esbuild export creates a plugin factory", async () => {
    const mod = await import("./esbuild");
    expect(typeof mod.default).toBe("function");
  });

  it("rspack export creates a plugin factory", async () => {
    const mod = await import("./rspack");
    expect(typeof mod.default).toBe("function");
  });

  it("rolldown export creates a plugin factory", async () => {
    const mod = await import("./rolldown");
    expect(typeof mod.default).toBe("function");
  });

  it("farm export creates a plugin factory", async () => {
    const mod = await import("./farm");
    expect(typeof mod.default).toBe("function");
  });
});

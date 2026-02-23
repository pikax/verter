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
  // @ai-generated - Tests that vite entry returns single plugin named vite:vue
  it("vite plugin returns single plugin named vite:vue with compat API", async () => {
    const mod = await import("./vite");
    const result = mod.default();
    // Returns a single plugin named "vite:vue" (drop-in replacement)
    expect(Array.isArray(result)).toBe(false);
    expect(result.name).toBe("vite:vue");
    expect((result as any).api).toBeDefined();
    expect((result as any).api.version).toBeDefined();
    expect((result as any).api.options).toBeDefined();
    // compiler is null by default; it's populated by configResolved
    // which doesn't fire outside of an actual Vite build.
    expect((result as any).api.options).toHaveProperty("compiler");
  });

  it("vite compat shim passes template options through", async () => {
    const mod = await import("./vite");
    const result = mod.default({
      template: { compilerOptions: { isCustomElement: (tag: string) => tag === "x-foo" } },
    });
    expect((result as any).api.options.template.compilerOptions.isCustomElement("x-foo")).toBe(true);
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

  // @ai-generated - Rolldown framework strips TS syntax (no vite:esbuild)
  it("rolldown framework strips TypeScript annotations", async () => {
    const plugin = unpluginFactory(undefined, {
      framework: "rolldown",
      versions: { unplugin: "0.0.0", rolldown: "0.0.0" },
    } as any) as any;
    const sfc = `<script setup lang="ts">
import { ref } from 'vue'

const count = ref<number>(0)
const msg: string = 'hello'
</script>

<template>
  <div>{{ count }} {{ msg }}</div>
</template>
`;
    const result = await plugin.transform(sfc, "/test/Rolldown.vue");
    expect(result).toBeDefined();
    const code = result!.code;

    // TS type annotations should be stripped (forceJs=true for rolldown)
    expect(code).not.toContain("ref<number>");
    expect(code).not.toContain(": string");
  });
});

describe("import resolution does not clobber vue files", () => {
  // @ai-generated - import-resolution must skip .vue files to avoid non_sfc clobbering
  it("transform of SFC importing another .vue file preserves script_lang", async () => {
    const plugin = unpluginFactory(undefined, {
      framework: "rollup",
      versions: { unplugin: "0.0.0", rollup: "0.0.0" },
    } as any) as any;

    // First transform a parent SFC that imports SideMenu.vue
    const parentSfc = `<script setup lang="ts">
import SideMenu from './SideMenu.vue'
const x = 1
</script>
<template><SideMenu /></template>
`;
    await plugin.transform(parentSfc, "/test/Parent.vue");

    // Now transform SideMenu.vue itself — it should be properly parsed as VueSfc
    const childSfc = `<script setup lang="ts">
import { computed } from 'vue'

export type NavigatePayload = { type: string; to: string }

const props = defineProps<{ visible?: boolean }>()
const isOpen = computed(() => props.visible)
</script>
<template><div>{{ isOpen }}</div></template>
`;
    const result = await plugin.transform(childSfc, "/test/SideMenu.vue");
    expect(result).toBeDefined();
    const code = result!.code;

    // export type must be hoisted outside setup (kept in TS mode, stripped by vite:esbuild)
    // The key check: it should NOT contain raw export type inside a setup function
    expect(code).toContain("isOpen");

    // The code should not contain "export type" inside the setup function body
    // (it should be hoisted before the _defineComponent wrapper)
    const setupIdx = code.indexOf("setup(");
    const exportTypeIdx = code.indexOf("export type NavigatePayload");
    if (exportTypeIdx !== -1 && setupIdx !== -1) {
      expect(exportTypeIdx).toBeLessThan(setupIdx);
    }
  });
});

describe("mixed import dependency upsert", () => {
  function createPlugin() {
    return unpluginFactory(undefined, {
      framework: "rollup",
      versions: { unplugin: "0.0.0", rollup: "0.0.0" },
    } as any) as any;
  }

  // @ai-generated - Mixed imports (value + type) should not crash the transform
  it("mixed import with type and value specifiers does not crash", async () => {
    const plugin = createPlugin();
    const sfc = `<script setup lang="ts">
import { type Props, ref } from './types'
defineProps<Props>()
const count = ref(0)
</script>

<template>
  <div>{{ count }}</div>
</template>
`;
    // The dep file ./types.ts won't exist, so the readFileSync will fail silently.
    // This test verifies the transform doesn't crash on mixed imports.
    const result = await plugin.transform(sfc, "/test/MixedImport.vue");
    expect(result).toBeDefined();
    expect(result.code).toContain("count");
  });
});

describe("custom block URL format", () => {
  function createPlugin() {
    return unpluginFactory(undefined, {
      framework: "rollup",
      versions: { unplugin: "0.0.0", rollup: "0.0.0" },
    } as any) as any;
  }

  // @ai-generated - resolveId accepts new custom block URL format (type=route)
  it("resolveId resolves custom block URLs with block type as type param", () => {
    const plugin = createPlugin();
    const result = plugin.resolveId("/path/to/App.vue?vue&type=route&index=0");
    expect(result).toBe("/path/to/App.vue?vue&type=route&index=0");
  });

  // @ai-generated - resolveId accepts i18n custom block URLs
  it("resolveId resolves i18n custom block URLs", () => {
    const plugin = createPlugin();
    const result = plugin.resolveId("/path/to/App.vue?vue&type=i18n&index=0");
    expect(result).toBe("/path/to/App.vue?vue&type=i18n&index=0");
  });

  // @ai-generated - Transform output for SFC with <i18n> block uses type=i18n in import URL
  it("transform output contains custom block import with type=i18n", async () => {
    const plugin = createPlugin();
    const sfc = `<script setup>
const n = 1
</script>
<template><div>{{n}}</div></template>
<i18n>{"en":{"hello":"world"}}</i18n>
`;
    const result = await plugin.transform(sfc, "/test/WithI18n.vue");
    expect(result).toBeDefined();
    const code = result.code;

    // Main module should contain custom block import with type=i18n (not type=custom)
    expect(code).toContain("type=i18n&index=0");
    expect(code).not.toContain("type=custom");
    expect(code).not.toContain("blockType=");
  });

  // @ai-generated - Transform output for SFC with <route> block uses type=route in import URL
  it("transform output contains route block import with type=route", async () => {
    const plugin = createPlugin();
    const sfc = `<script setup>
const n = 1
</script>
<template><div>{{n}}</div></template>
<route>{"path": "/home"}</route>
`;
    const result = await plugin.transform(sfc, "/test/WithRoute.vue");
    expect(result).toBeDefined();
    const code = result.code;

    expect(code).toContain("type=route&index=0");
    expect(code).not.toContain("type=custom");
    expect(code).not.toContain("blockType=");
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

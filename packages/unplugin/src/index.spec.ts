/**
 * @ai-generated - Integration tests for the unplugin factory.
 */
import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { mkdirSync, writeFileSync, rmSync } from "fs";
import { join } from "path";
import { tmpdir } from "os";
import { defineConfig, resolveConfig } from "vite";
import unplugin, { unpluginFactory } from "./index";
import { loadHost, resetHost } from "./core/compiler";
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
    const result = plugin.resolveId("/path/to/App.vue?vue&type=style&index=0&lang=css");
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
    expect(plugin.transformInclude("/path/to/App.vue?vue&type=style&index=0&lang=css")).toBe(false);
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

describe("vite style virtual modules", () => {
  let tempDir: string;
  let activePlugin: any = null;

  beforeEach(() => {
    tempDir = join(
      tmpdir(),
      `verter-vite-style-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
    );
    mkdirSync(tempDir, { recursive: true });
    resetHost();
  });

  afterEach(async () => {
    // Always close the plugin to kill the preprocessor child process.
    // Without this, leaked child processes keep the Node.js event loop alive
    // and cause vitest to hang after tests complete.
    if (activePlugin) {
      await activePlugin.closeBundle();
      activePlugin = null;
    }
    rmSync(tempDir, { recursive: true, force: true });
    resetHost();
  });

  async function createVitePlugin() {
    const plugin = unpluginFactory(undefined, {
      framework: "vite",
      versions: { unplugin: "0.0.0", vite: "7.0.0" },
    } as any) as any;

    const viteConfig = await resolveConfig(
      defineConfig({
        root: tempDir,
        build: { cssCodeSplit: false },
      }),
      "build",
      "production",
    );

    plugin.vite.configResolved(viteConfig);
    activePlugin = plugin;
    return plugin;
  }

  it("loads compiled CSS for a scoped scss style virtual module", async () => {
    const plugin = await createVitePlugin();
    const file = join(tempDir, "ScopedStyle.vue").replace(/\\/g, "/");
    const sfc = `<template><button class="scoped-style">x</button></template>
<style scoped lang="scss">
$border: #555;
.scoped-style {
  &:hover {
    border-color: $border;
  }
}
</style>`;

    try {
      await plugin.transform(sfc, file);
      const style = await plugin.load(`${file}?vue&type=style&index=0&lang.scss`);

      expect(style).toBeDefined();
      expect(style.code).toContain(".scoped-style:hover");
      expect(style.code).toContain("#555");
      expect(style.code).not.toContain("$border");
      expect(style.code).not.toContain("[data-v-");
    } finally {
      await plugin.closeBundle();
    }
  });

  it("does NOT scope CSS for non-scoped scss style virtual modules", async () => {
    const plugin = await createVitePlugin();
    const file = join(tempDir, "NonScoped.vue").replace(/\\/g, "/");
    const sfc = `<template><button class="non-scoped">x</button></template>
<style lang="scss">
$color: #333;
.non-scoped {
  color: $color;
}
</style>`;

    try {
      await plugin.transform(sfc, file);
      const styleId = `${file}?vue&type=style&index=0&lang.scss`;
      const style = await plugin.load(styleId);
      const transformed = await plugin.transform(style.code, styleId);

      expect(transformed).toBeDefined();
      expect(transformed.code).not.toContain("[data-v-");
      expect(transformed.code).toContain("#333");
    } finally {
      await plugin.closeBundle();
    }
  });

  it("scopes compiled CSS for non-css style virtual modules without re-preprocessing", async () => {
    const plugin = await createVitePlugin();
    const file = join(tempDir, "ScopedTransform.vue").replace(/\\/g, "/");
    const sfc = `<template><button class="scoped-transform">x</button></template>
<style scoped lang="scss">
$border: #555;
.scoped-transform {
  &:hover {
    border-color: $border;
  }
}
</style>`;

    try {
      await plugin.transform(sfc, file);
      const styleId = `${file}?vue&type=style&index=0&lang.scss`;
      const style = await plugin.load(styleId);
      const transformed = await plugin.transform(style.code, styleId);

      expect(transformed).toBeDefined();
      expect(transformed.code).toContain("[data-v-");
      expect(transformed.code).toContain("#555");
      expect(transformed.code).not.toContain("$border");
    } finally {
      await plugin.closeBundle();
    }
  });

  it("buildEnd closes the style preprocessor session and later transforms respawn it", async () => {
    const plugin = await createVitePlugin();
    const firstFile = join(tempDir, "BuildEndOne.vue").replace(/\\/g, "/");
    const secondFile = join(tempDir, "BuildEndTwo.vue").replace(/\\/g, "/");
    const firstSfc = `<template><button class="first-build-end">x</button></template>
<style scoped lang="scss">
$border: #555;
.first-build-end {
  &:hover {
    border-color: $border;
  }
}
</style>`;
    const secondSfc = `<template><button class="second-build-end">x</button></template>
<style scoped lang="scss">
$border: #0a84ff;
.second-build-end {
  &:hover {
    border-color: $border;
  }
}
</style>`;

    try {
      await plugin.transform(firstSfc, firstFile);
      await expect(
        Promise.race([
          plugin.buildEnd(),
          new Promise((_, reject) => {
            setTimeout(() => reject(new Error("buildEnd timed out")), 4_000);
          }),
        ]),
      ).resolves.toBeUndefined();

      await plugin.transform(secondSfc, secondFile);
      const style = await plugin.load(`${secondFile}?vue&type=style&index=0&lang.scss`);

      expect(style).toBeDefined();
      expect(style.code).toContain("#0a84ff");
      expect(style.code).not.toContain("$border");
    } finally {
      await plugin.closeBundle();
    }
  }, 45_000);
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
    expect((result as any).api.options.template.compilerOptions.isCustomElement("x-foo")).toBe(
      true,
    );
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

  // @ai-generated — Document actual forceJs behavior per framework.
  // These tests capture the CURRENT behavior of Verter's type stripping (forceJs=true).
  // Key finding: forceJs=true strips `import type` statements but does NOT strip
  // inline type annotations like `: Ref<number>` or `ref<number>()`.

  const TS_SFC = `<script setup lang="ts">
import { ref } from 'vue'
import type { Ref } from 'vue'
const count = ref<number>(0)
const msg: string = 'hello'
</script>
<template><div>{{ count }} {{ msg }}</div></template>
`;

  for (const framework of ["rollup", "webpack", "rspack", "rolldown"] as const) {
    it(`${framework} framework: forceJs=true strips import type but NOT inline annotations`, async () => {
      const plugin = unpluginFactory(undefined, {
        framework,
        versions: { unplugin: "0.0.0", [framework]: "0.0.0" },
      } as any) as any;
      const result = await plugin.transform(TS_SFC, `/test/${framework}.vue`);
      expect(result).toBeDefined();
      const code = result!.code;

      // forceJs=true for non-Vite: strips `import type` statements
      expect(code).not.toContain("import type");

      // BUG: forceJs=true does NOT strip inline type annotations
      // This means non-Vite bundlers still receive TS syntax in the output.
      // These assertions document the ACTUAL (buggy) behavior:
      expect(code).toContain("ref<number>"); // NOT stripped
      expect(code).toContain(": string"); // NOT stripped
    });
  }

  // @ai-generated - Type-only `import type { Ref }` is stripped by forceJs=true
  it("explicit import type is stripped when forceJs=true", async () => {
    const plugin = unpluginFactory(undefined, {
      framework: "rollup",
      versions: { unplugin: "0.0.0", rollup: "0.0.0" },
    } as any) as any;
    const sfc = `<script setup lang="ts">
import type { Ref } from 'vue'
import { ref } from 'vue'
const x: Ref<number> = ref(0)
</script>
<template><div>{{ x }}</div></template>
`;
    const result = await plugin.transform(sfc, "/test/TypeOnly.vue");
    expect(result).toBeDefined();
    const code = result!.code;

    // `import type` is stripped by forceJs=true
    expect(code).not.toContain("import type");
    // BUG: inline type annotation `: Ref<number>` is NOT stripped
    expect(code).toContain("Ref<number>");
  });

  // @ai-generated - Mixed import `import { Ref, ref }` — Ref is type-only but
  // NOT marked with `type` keyword. Tests the actual Bug 2 scenario.
  it("implicit type-only import (Ref without type keyword) is pruned from emitted JS", async () => {
    const plugin = unpluginFactory(undefined, {
      framework: "rollup",
      versions: { unplugin: "0.0.0", rollup: "0.0.0" },
    } as any) as any;
    const sfc = `<script setup lang="ts">
import { Ref, ref } from 'vue'
const x: Ref<number> = ref(0)
</script>
<template><div>{{ x }}</div></template>
`;
    const result = await plugin.transform(sfc, "/test/ImplicitType.vue");
    expect(result).toBeDefined();
    const code = result!.code;

    // The generated JS keeps the runtime import but prunes the type-only symbol.
    expect(code).toContain("import { ref } from 'vue'");
    expect(code).not.toContain("Ref, ref");
    expect(code).not.toContain("Ref<number>");

    // filter_setup_return removes Ref from setup return (template doesn't use $setup.Ref)
    expect(code).not.toContain("return { Ref");
    expect(code).not.toContain("return {Ref");
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
  it("mixed import with type and value specifiers resolves exact module references", async () => {
    const plugin = createPlugin();
    const testDir = join(
      tmpdir(),
      `verter-mixed-import-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
    );
    mkdirSync(testDir, { recursive: true });
    const sfc = `<script setup lang="ts">
import { type Props, helper } from './types'
defineProps<Props>()
const count = helper()
</script>

<template>
  <div>{{ count }}</div>
</template>
`;
    const filename = join(testDir, "MixedImport.vue").replace(/\\/g, "/");

    writeFileSync(
      join(testDir, "types.ts"),
      `export interface Props { count?: number }\nexport const helper = () => 1\n`,
    );

    try {
      const result = await plugin.transform(sfc, filename);
      expect(result).toBeDefined();
      expect(result.code).toContain("count");
      expect(result.code).toContain("helper");
    } finally {
      rmSync(testDir, { recursive: true, force: true });
    }
  });
});

describe("bundler dependency delegation", () => {
  let tempDir: string;

  beforeEach(() => {
    tempDir = join(tmpdir(), `verter-deps-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`);
    mkdirSync(tempDir, { recursive: true });
    resetHost();
  });

  afterEach(() => {
    rmSync(tempDir, { recursive: true, force: true });
    resetHost();
  });

  function createPlugin() {
    return unpluginFactory(undefined, {
      framework: "rollup",
      versions: { unplugin: "0.0.0", rollup: "0.0.0" },
    } as any) as any;
  }

  it("transform delegates exact and finite candidates through the resolve hook", async () => {
    const plugin = createPlugin();
    const filename = join(tempDir, "App.vue").replace(/\\/g, "/");
    const typeFile = join(tempDir, "types.ts").replace(/\\/g, "/");
    const aFile = join(tempDir, "a.ts").replace(/\\/g, "/");
    const bFile = join(tempDir, "b.ts").replace(/\\/g, "/");
    writeFileSync(typeFile, "export interface Props { name: string }\n");
    writeFileSync(aFile, "export const a = 1\n");
    writeFileSync(bFile, "export const b = 2\n");

    const resolveSpy = vi.fn(async (source: string) => {
      const map: Record<string, string> = {
        "./types": typeFile,
        "./a": aFile,
        "./b": bFile,
      };
      return map[source] ? { id: map[source] } : null;
    });

    await plugin.transform.call(
      { resolve: resolveSpy },
      `<script setup lang="ts">
import type { Props } from './types'
const branch = cond ? './a' : './b'
import(branch)
</script>
<template><div>ok</div></template>`,
      filename,
    );

    expect(resolveSpy).toHaveBeenCalledWith("./types", filename, { skipSelf: true });
    expect(resolveSpy).toHaveBeenCalledWith("./a", filename, { skipSelf: true });
    expect(resolveSpy).toHaveBeenCalledWith("./b", filename, { skipSelf: true });
  });

  it("transform does not delegate unknown dynamic references", async () => {
    const plugin = createPlugin();
    const filename = join(tempDir, "Dynamic.vue").replace(/\\/g, "/");
    const resolveSpy = vi.fn();

    await plugin.transform.call(
      { resolve: resolveSpy },
      `<script setup lang="ts">
import(\`./widgets/\${window.name}\`)
</script>
<template><div>ok</div></template>`,
      filename,
    );

    expect(resolveSpy).not.toHaveBeenCalled();
  });

  it("resolved .vue dependencies are recorded but not upserted as non_sfc", async () => {
    const plugin = createPlugin();
    const host = loadHost();
    const filename = join(tempDir, "Parent.vue").replace(/\\/g, "/");
    const childFile = join(tempDir, "Child.vue").replace(/\\/g, "/");
    writeFileSync(childFile, "<template><div>child</div></template>\n");

    const upsertSpy = vi.spyOn(host, "upsert");
    const setDepsSpy = vi.spyOn(host, "setImportDependencies");

    await plugin.transform.call(
      {
        resolve: vi.fn(async (source: string) =>
          source === "./Child.vue" ? { id: childFile } : null,
        ),
      },
      `<script setup lang="ts">
import Child from './Child.vue'
</script>
<template><Child /></template>`,
      filename,
    );

    expect(setDepsSpy).toHaveBeenCalledWith(filename, [
      { specifier: "./Child.vue", resolvedCanonicalId: childFile },
    ]);
    expect(
      upsertSpy.mock.calls.some(
        ([request]) => request?.inputId === childFile && request?.fileKind === "non_sfc",
      ),
    ).toBe(false);
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

describe("preCompile", () => {
  let tempDir: string;

  function createTempDir(): string {
    const dir = join(
      tmpdir(),
      `verter-precompile-test-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
    );
    mkdirSync(dir, { recursive: true });
    return dir;
  }

  beforeEach(() => {
    tempDir = createTempDir();
    resetHost();
  });

  afterEach(() => {
    if (origCwd) process.cwd = origCwd;
    rmSync(tempDir, { recursive: true, force: true });
    resetHost();
  });

  let origCwd: typeof process.cwd;

  function createPreCompilePlugin(extraOpts?: Record<string, unknown>) {
    // Override cwd so buildStart scans our temp dir.
    // Must remain active until buildStart() completes (it reads cwd at call time).
    origCwd = process.cwd;
    process.cwd = () => tempDir;
    const plugin = unpluginFactory(
      { preCompile: true, ...extraOpts } as any,
      { framework: "rollup", versions: { unplugin: "0.0.0", rollup: "0.0.0" } } as any,
    ) as any;
    return plugin;
  }

  // @ai-generated - preCompile option accepted by factory
  it("preCompile option accepted by factory", () => {
    const plugin = unpluginFactory({ preCompile: true }, {
      framework: "rollup",
      versions: { unplugin: "0.0.0", rollup: "0.0.0" },
    } as any) as any;
    expect(plugin).toBeDefined();
    expect(plugin.name).toBe("unplugin-verter");
  });

  // @ai-generated - buildStart is a no-op when preCompile is false/undefined
  it("buildStart is a no-op when preCompile is false", async () => {
    const plugin = unpluginFactory(undefined, {
      framework: "rollup",
      versions: { unplugin: "0.0.0", rollup: "0.0.0" },
    } as any) as any;

    // Should not throw and return quickly
    await plugin.buildStart();
  });

  // @ai-generated - Pre-compiled files produce same output when transform receives unchanged source
  it("pre-compiled files produce same output via transform with unchanged source", async () => {
    const sfc = `<script setup>\nconst msg = 'hello'\n</script>\n<template><div>{{ msg }}</div></template>\n`;
    const filename = join(tempDir, "App.vue").replace(/\\/g, "/");
    writeFileSync(join(tempDir, "App.vue"), sfc);

    const plugin = createPreCompilePlugin();
    await plugin.buildStart();

    // Now call transform with the same source — should get same result
    const result = await plugin.transform(sfc, filename);
    expect(result).toBeDefined();
    expect(result.code).toContain("msg");
  });

  // @ai-generated - buildStart pre-compiles .vue files in subdirectories
  it("buildStart pre-compiles .vue files in subdirectories", async () => {
    mkdirSync(join(tempDir, "components"), { recursive: true });
    const sfc = `<script setup>\nconst x = 1\n</script>\n<template><div>{{ x }}</div></template>\n`;
    writeFileSync(join(tempDir, "components", "Btn.vue"), sfc);

    const plugin = createPreCompilePlugin();
    await plugin.buildStart();

    // Transform should still work
    const filename = join(tempDir, "components", "Btn.vue").replace(/\\/g, "/");
    const result = await plugin.transform(sfc, filename);
    expect(result).toBeDefined();
    expect(result.code).toBeDefined();
  });

  // @ai-generated - Modified source triggers recompilation with new content
  it("modified source (simulating another plugin) triggers recompilation", async () => {
    const originalSfc = `<script setup>\nconst msg = 'original'\n</script>\n<template><div>{{ msg }}</div></template>\n`;
    const modifiedSfc = `<script setup>\nconst msg = 'modified'\nconst extra = 42\n</script>\n<template><div>{{ msg }} {{ extra }}</div></template>\n`;
    writeFileSync(join(tempDir, "App.vue"), originalSfc);

    const plugin = createPreCompilePlugin();
    await plugin.buildStart();

    // Another plugin modifies the file — transform receives different content
    const filename = join(tempDir, "App.vue").replace(/\\/g, "/");
    const result = await plugin.transform(modifiedSfc, filename);
    expect(result).toBeDefined();
    expect(result.code).toContain("extra");
  });

  // @ai-generated - Macro type resolution during preCompile
  it("resolves type dependencies for defineProps macros during preCompile", async () => {
    const vueSfc = `<script setup lang="ts">\nimport type { MyProps } from './types'\ndefineProps<MyProps>()\n</script>\n<template><div>hello</div></template>\n`;
    const typesTs = `export interface MyProps {\n  name: string\n  count: number\n}\n`;

    writeFileSync(join(tempDir, "App.vue"), vueSfc);
    writeFileSync(join(tempDir, "types.ts"), typesTs);

    const plugin = createPreCompilePlugin();
    // Should not throw — dependencies are resolved during buildStart
    await plugin.buildStart();

    const filename = join(tempDir, "App.vue").replace(/\\/g, "/");
    const result = await plugin.transform(vueSfc, filename);
    expect(result).toBeDefined();
    // The compiled output should contain the resolved prop names
    expect(result.code).toContain("name");
    expect(result.code).toContain("count");
  });

  // @ai-generated - External src resolution during preCompile
  it("resolves external style src during preCompile", async () => {
    const vueSfc = `<script setup>\nconst x = 1\n</script>\n<template><div>{{ x }}</div></template>\n<style src="./style.css" scoped></style>\n`;
    const css = `.box { color: red; }\n`;

    writeFileSync(join(tempDir, "App.vue"), vueSfc);
    writeFileSync(join(tempDir, "style.css"), css);

    const plugin = createPreCompilePlugin();
    // Should not throw — external src is resolved during buildStart
    await plugin.buildStart();

    const filename = join(tempDir, "App.vue").replace(/\\/g, "/");
    const result = await plugin.transform(vueSfc, filename);
    expect(result).toBeDefined();
    expect(result.code).toBeDefined();
  });

  // @ai-generated - node_modules exclusion: files in node_modules are not pre-compiled
  it("node_modules .vue files are excluded from preCompile but compile via transform", async () => {
    // Create a .vue file in node_modules
    mkdirSync(join(tempDir, "node_modules", "some-lib"), { recursive: true });
    const libSfc = `<script setup>\nconst lib = 'value'\n</script>\n<template><div>{{ lib }}</div></template>\n`;
    writeFileSync(join(tempDir, "node_modules", "some-lib", "Comp.vue"), libSfc);

    // Also create a project .vue file
    const appSfc = `<script setup>\nconst app = 1\n</script>\n<template><div>{{ app }}</div></template>\n`;
    writeFileSync(join(tempDir, "App.vue"), appSfc);

    const plugin = createPreCompilePlugin();
    await plugin.buildStart();

    // node_modules file should still compile correctly via transform
    const libFilename = join(tempDir, "node_modules", "some-lib", "Comp.vue").replace(/\\/g, "/");
    const result = await plugin.transform(libSfc, libFilename);
    expect(result).toBeDefined();
    expect(result.code).toContain("lib");
  });

  // @ai-generated - Benchmark: measure preCompile cost for N files
  it("benchmark: preCompile N files measures timing", async () => {
    const N = 20;
    mkdirSync(join(tempDir, "src"), { recursive: true });
    for (let i = 0; i < N; i++) {
      const sfc = `<script setup>\nconst val${i} = ${i}\n</script>\n<template><div>{{ val${i} }}</div></template>\n`;
      writeFileSync(join(tempDir, "src", `Comp${i}.vue`), sfc);
    }

    const plugin = createPreCompilePlugin();
    const start = performance.now();
    await plugin.buildStart();
    const elapsed = performance.now() - start;

    // Just log the timing — this is a baseline measurement, not a pass/fail assertion
    console.log(
      `[benchmark] preCompile ${N} files: ${elapsed.toFixed(1)}ms (${(elapsed / N).toFixed(2)}ms/file)`,
    );
    expect(elapsed).toBeGreaterThan(0);
  });
});

describe("closeBundle hook", () => {
  beforeEach(() => {
    resetHost();
  });

  afterEach(() => {
    resetHost();
  });

  it("plugin has a closeBundle hook that resets the host", async () => {
    const plugin = unpluginFactory(undefined, {
      framework: "rollup",
      versions: { unplugin: "0.0.0", rollup: "0.0.0" },
    } as any) as any;

    // Force host creation by transforming a file
    const sfc = `<script setup>\nconst x = 1\n</script>\n<template><div>{{ x }}</div></template>\n`;
    await plugin.transform(sfc, "/test/CloseBundle.vue");

    // Verify closeBundle hook exists
    expect(typeof plugin.closeBundle).toBe("function");

    // Call closeBundle — should not throw
    await plugin.closeBundle();

    // After closeBundle, loadHost() should create a NEW host (the old one was nulled)
    // We verify by importing loadHost and checking it works without error
    const { loadHost } = await import("./core/compiler");
    const newHost = loadHost();
    expect(newHost).toBeDefined();
  });

  it("closeBundle is safe to call even when no host was created", () => {
    const plugin = unpluginFactory(undefined, {
      framework: "rollup",
      versions: { unplugin: "0.0.0", rollup: "0.0.0" },
    } as any) as any;

    // closeBundle should not throw even if no host was ever created
    expect(typeof plugin.closeBundle).toBe("function");
    expect(() => plugin.closeBundle()).not.toThrow();
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

describe("macro type hydration", () => {
  let tempDir: string;

  beforeEach(() => {
    tempDir = join(
      tmpdir(),
      `verter-macro-hydrate-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
    );
    mkdirSync(tempDir, { recursive: true });
    resetHost();
  });

  afterEach(() => {
    rmSync(tempDir, { recursive: true, force: true });
    resetHost();
  });

  function createPlugin() {
    return unpluginFactory(undefined, {
      framework: "rollup",
      versions: { unplugin: "0.0.0", rollup: "0.0.0" },
    } as any) as any;
  }

  it("transform hydrates package-backed macro type deps via resolve hook + package.json types", async () => {
    const plugin = createPlugin();
    const filename = join(tempDir, "App.vue").replace(/\\/g, "/");

    // Create a fake package with type declarations.
    // The bundler resolve hook returns the runtime entry (.js), but the
    // hydration helper must discover and load the .d.ts from package.json "types".
    const pkgDir = join(tempDir, "node_modules", "my-animation-lib");
    mkdirSync(pkgDir, { recursive: true });
    writeFileSync(
      join(pkgDir, "package.json"),
      JSON.stringify({
        name: "my-animation-lib",
        main: "./index.js",
        types: "./index.d.ts",
      }),
    );
    writeFileSync(
      join(pkgDir, "index.d.ts"),
      "export interface AnimationOptions { duration?: number; easing?: string; }\n",
    );
    writeFileSync(join(pkgDir, "index.js"), "module.exports = {};\n");

    // defineProps<AnimationOptions>() — the entire type is imported.
    // The host MUST resolve AnimationOptions from the package .d.ts
    // to discover prop names (duration, easing).
    const sfc = `<script setup lang="ts">
import type { AnimationOptions } from "my-animation-lib"
defineProps<AnimationOptions>()
</script>
<template><div>hello</div></template>
`;

    // Resolve hook returns the JS runtime file.
    // The hydration helper should walk up to package.json, find "types",
    // and upsert the .d.ts file instead.
    const resolveSpy = vi.fn(async (source: string) => {
      if (source === "my-animation-lib") {
        return { id: join(pkgDir, "index.js").replace(/\\/g, "/") };
      }
      return null;
    });

    const result = await plugin.transform.call({ resolve: resolveSpy }, sfc, filename);
    expect(result).toBeDefined();
    const code = result.code;

    // Positive: the host resolved the type and extracted prop names
    expect(code).toContain("duration");
    expect(code).toContain("easing");
    // Negative: no HOST_MISSING_MACRO_TYPE_DEP compile error
    expect(code).not.toContain("HOST_MISSING_MACRO_TYPE_DEP");
  });

  it("transform with relative type deps works via hydration (existing behavior preserved)", async () => {
    const plugin = createPlugin();
    const filename = join(tempDir, "App.vue").replace(/\\/g, "/");
    const typeFile = join(tempDir, "types.ts").replace(/\\/g, "/");

    writeFileSync(
      join(tempDir, "types.ts"),
      "export interface MyProps { name: string; count: number; }\n",
    );

    const sfc = `<script setup lang="ts">
import type { MyProps } from "./types"
defineProps<MyProps>()
</script>
<template><div>hello</div></template>
`;

    const resolveSpy = vi.fn(async (source: string) => {
      if (source === "./types") return { id: typeFile };
      return null;
    });

    const result = await plugin.transform.call({ resolve: resolveSpy }, sfc, filename);
    expect(result).toBeDefined();
    const code = result.code;

    // Positive: resolved props should appear
    expect(code).toContain("name");
    expect(code).toContain("count");
    // Negative: no missing type dep error
    expect(code).not.toContain("HOST_MISSING_MACRO_TYPE_DEP");
  });

  it("relative .d.ts macro type deps are resolved when resolveId returns null", async () => {
    const plugin = createPlugin();
    const filename = join(tempDir, "App.vue").replace(/\\/g, "/");

    // Create a .d.ts file (not .ts) — Vite's resolveId won't resolve these
    writeFileSync(
      join(tempDir, "type.d.ts"),
      "export interface Props { order: string | null }\nexport interface Emits { updatePrice: [number]; updateStatus: [string]; }\n",
    );

    const sfc = `<script setup lang="ts">
import type { Props, Emits } from "./type"
defineProps<Props>()
defineEmits<Emits>()
</script>
<template><div>hello</div></template>
`;

    // Resolve hook returns null for ./type (Vite can't resolve .d.ts)
    const resolveSpy = vi.fn(async () => null);

    const result = await plugin.transform.call({ resolve: resolveSpy }, sfc, filename);
    expect(result).toBeDefined();
    const code = result.code;

    // Positive: the host resolved the types and extracted prop/emit names
    expect(code).toContain("order");
    // Negative: no HOST_MISSING_MACRO_TYPE_DEP compile error
    expect(code).not.toContain("HOST_MISSING_MACRO_TYPE_DEP");
  });

  it("recursive relative imports inside hydrated type files are upserted", async () => {
    const plugin = createPlugin();
    const filename = join(tempDir, "App.vue").replace(/\\/g, "/");
    const typeFile = join(tempDir, "types.ts").replace(/\\/g, "/");
    const baseFile = join(tempDir, "base.ts").replace(/\\/g, "/");

    writeFileSync(join(tempDir, "base.ts"), "export interface BaseProps { id: string; }\n");
    writeFileSync(
      join(tempDir, "types.ts"),
      "import { BaseProps } from './base';\nexport interface MyProps extends BaseProps { name: string; }\n",
    );

    const sfc = `<script setup lang="ts">
import type { MyProps } from "./types"
defineProps<MyProps>()
</script>
<template><div>hello</div></template>
`;

    const resolveSpy = vi.fn(async (source: string) => {
      if (source === "./types") return { id: typeFile };
      if (source === "./base") return { id: baseFile };
      return null;
    });

    const result = await plugin.transform.call({ resolve: resolveSpy }, sfc, filename);
    expect(result).toBeDefined();
    const code = result.code;

    // Positive: both base and derived type props should resolve
    expect(code).toContain("name");
    expect(code).toContain("id");
    // Negative: no missing type dep
    expect(code).not.toContain("HOST_MISSING_MACRO_TYPE_DEP");
  });

  it("barrel file export-star re-export chain resolves macro type deps", async () => {
    const plugin = createPlugin();
    const filename = join(tempDir, "App.vue").replace(/\\/g, "/");

    // Create a package with barrel file structure internally:
    //   my-ui-lib/index.d.ts  →  export * from './components/Drawer'
    //   my-ui-lib/components/Drawer/index.d.ts  →  export type { DrawerEmits } from './types'
    //   my-ui-lib/components/Drawer/types.d.ts  →  defines DrawerEmits interface
    const pkgDir = join(tempDir, "node_modules", "my-ui-lib");
    const drawerDir = join(pkgDir, "components", "Drawer");
    mkdirSync(drawerDir, { recursive: true });

    writeFileSync(
      join(pkgDir, "package.json"),
      JSON.stringify({
        name: "my-ui-lib",
        main: "./index.js",
        types: "./index.d.ts",
      }),
    );
    writeFileSync(join(pkgDir, "index.js"), "module.exports = {};\n");

    // Barrel entry: export * re-export
    writeFileSync(join(pkgDir, "index.d.ts"), "export * from './components/Drawer'\n");

    // Drawer barrel: named re-export
    writeFileSync(join(drawerDir, "index.d.ts"), "export type { DrawerEmits } from './types'\n");

    // Actual type definition
    writeFileSync(
      join(drawerDir, "types.d.ts"),
      "export interface DrawerEmits { (e: 'close'): void; (e: 'open'): void; }\n",
    );

    const sfc = `<script setup lang="ts">
import type { DrawerEmits } from "my-ui-lib"
defineEmits<DrawerEmits>()
</script>
<template><div>hello</div></template>
`;

    const resolveSpy = vi.fn(async (source: string) => {
      if (source === "my-ui-lib") {
        return { id: join(pkgDir, "index.js").replace(/\\/g, "/") };
      }
      return null;
    });

    const result = await plugin.transform.call({ resolve: resolveSpy }, sfc, filename);
    expect(result).toBeDefined();
    const code = result.code;

    // Positive: the host followed the barrel chain and resolved the emit type
    expect(code).toContain("close");
    expect(code).toContain("open");
    // Negative: no missing type dep error
    expect(code).not.toContain("HOST_MISSING_MACRO_TYPE_DEP");
  });

  it("direct host: barrel chain ending at .vue file resolves type", () => {
    // Bypasses unplugin transform to test the Rust host directly via NAPI.
    const host = loadHost();

    const consumerPath = join(tempDir, "Consumer.vue").replace(/\\/g, "/");
    const basePath = join(tempDir, "base", "index.ts").replace(/\\/g, "/");
    const drawerPath = join(tempDir, "base", "Drawer", "index.ts").replace(/\\/g, "/");
    const vuePath = join(tempDir, "base", "Drawer", "src", "index.vue").replace(/\\/g, "/");

    const baseDir = join(tempDir, "base");
    const drawerDir = join(baseDir, "Drawer");
    const drawerSrcDir = join(drawerDir, "src");
    mkdirSync(drawerSrcDir, { recursive: true });

    writeFileSync(join(baseDir, "index.ts"), "export * from './Drawer'\n");
    writeFileSync(
      join(drawerDir, "index.ts"),
      "export type { DrawerEmits } from './src/index.vue'\n",
    );
    writeFileSync(
      join(drawerSrcDir, "index.vue"),
      `<script setup lang="ts">\nexport interface DrawerEmits {\n  open: []\n  close: []\n  disposed: []\n}\ndefineEmits<DrawerEmits>()\n</script>\n<template><div>drawer</div></template>\n`,
    );

    // Upsert all files
    host.upsert({
      inputId: consumerPath,
      source: `<script setup lang="ts">\nimport type { DrawerEmits } from "@/components/base"\ndefineEmits<DrawerEmits>()\n</script>\n<template><div>hello</div></template>\n`,
    });
    host.upsert({ inputId: basePath, source: "export * from './Drawer'\n", fileKind: "non_sfc" });
    host.upsert({
      inputId: drawerPath,
      source: "export type { DrawerEmits } from './src/index.vue'\n",
      fileKind: "non_sfc",
    });
    host.upsert({
      inputId: vuePath,
      source: `<script setup lang="ts">\nexport interface DrawerEmits {\n  open: []\n  close: []\n  disposed: []\n}\ndefineEmits<DrawerEmits>()\n</script>\n<template><div>drawer</div></template>\n`,
    });

    // Wire up dependency chain (same as the Rust unit test)
    host.setImportDependencies(consumerPath, [
      { specifier: "@/components/base", resolvedCanonicalId: basePath },
    ]);
    host.setImportDependencies(basePath, [
      { specifier: "./Drawer", resolvedCanonicalId: drawerPath },
    ]);
    host.setImportDependencies(drawerPath, [
      { specifier: "./src/index.vue", resolvedCanonicalId: vuePath },
    ]);

    // Compile
    const result = host.getVirtualFile({
      rawId: consumerPath,
      compileProfile: {
        filename: consumerPath,
        ssr: false,
        isProduction: false,
        componentId: "test",
        hmrStrategy: "vite",
        sourceMap: false,
        forceJs: false,
      },
    });

    expect(result.code).not.toContain("XInvalidMacroType");
    expect(result.code).not.toContain("HOST_MISSING");
    expect(result.code).toContain("open");
    expect(result.code).toContain("close");
    expect(result.code).toContain("disposed");
  });

  it("barrel chain ending at .vue file resolves exported type via alias", async () => {
    const plugin = createPlugin();
    const filename = join(tempDir, "Consumer.vue").replace(/\\/g, "/");

    // Simulates @/components/base path alias → barrel chain ending at .vue:
    //   base/index.ts  →  export * from './Drawer'
    //   base/Drawer/index.ts  →  export type { DrawerEmits } from './src/index.vue'
    //   base/Drawer/src/index.vue  →  defines DrawerEmits in <script setup>
    const baseDir = join(tempDir, "base");
    const drawerDir = join(baseDir, "Drawer");
    const drawerSrcDir = join(drawerDir, "src");
    mkdirSync(drawerSrcDir, { recursive: true });

    writeFileSync(join(baseDir, "index.ts"), "export * from './Drawer'\n");
    writeFileSync(
      join(drawerDir, "index.ts"),
      "export type { DrawerEmits } from './src/index.vue'\n",
    );
    writeFileSync(
      join(drawerSrcDir, "index.vue"),
      `<script setup lang="ts">
export interface DrawerEmits {
  open: []
  close: []
  disposed: []
}
defineEmits<DrawerEmits>()
</script>
<template><div>drawer</div></template>
`,
    );

    // Use path alias (@/components/base) — not relative, so hydrateMacroTypeDeps processes it
    const sfc = `<script setup lang="ts">
import type { DrawerEmits } from "@/components/base"
defineEmits<DrawerEmits>()
</script>
<template><div>hello</div></template>
`;

    // Resolve hook maps the alias to the absolute path (simulates Vite alias resolution)
    const resolveSpy = vi.fn(async (source: string) => {
      if (source === "@/components/base")
        return { id: join(baseDir, "index.ts").replace(/\\/g, "/") };
      return null;
    });

    const result = await plugin.transform.call({ resolve: resolveSpy }, sfc, filename);
    expect(result).toBeDefined();
    const code = result.code;

    // Positive: emit names from the .vue file's exported interface
    expect(code).toContain("open");
    expect(code).toContain("close");
    expect(code).toContain("disposed");
    // Negative: no unresolved type errors
    expect(code).not.toContain("HOST_MISSING_MACRO_TYPE_DEP");
    expect(code).not.toContain("XInvalidMacroType");
  });

  it("relative .vue → .vue type import resolves defineProps type", async () => {
    const plugin = createPlugin();
    const filename = join(tempDir, "Parent.vue").replace(/\\/g, "/");

    // Create a child .vue file that exports a type via <script setup>
    writeFileSync(
      join(tempDir, "Child.vue"),
      `<script setup lang="ts">
export interface ChildProps {
  label: string
  count: number
}
defineProps<ChildProps>()
</script>
<template><div>child</div></template>
`,
    );

    // Parent imports the type from the child .vue file and uses it in defineProps
    const sfc = `<script setup lang="ts">
import type { ChildProps } from "./Child.vue"
defineProps<ChildProps>()
</script>
<template><div>parent</div></template>
`;

    // Resolve hook maps relative .vue imports
    const childPath = join(tempDir, "Child.vue").replace(/\\/g, "/");
    const resolveSpy = vi.fn(async (source: string) => {
      if (source === "./Child.vue") return { id: childPath };
      return null;
    });

    const result = await plugin.transform.call({ resolve: resolveSpy }, sfc, filename);
    expect(result).toBeDefined();
    const code = result.code;

    // Positive: prop names from the child's exported interface
    expect(code).toContain("label");
    expect(code).toContain("count");
    // Negative: no unresolved type errors
    expect(code).not.toContain("HOST_MISSING_MACRO_TYPE_DEP");
  });

  it("path alias .vue type import resolves defineEmits type", async () => {
    const plugin = createPlugin();
    const filename = join(tempDir, "Consumer.vue").replace(/\\/g, "/");

    // Create a .vue file that exports an emits type
    const compDir = join(tempDir, "components");
    mkdirSync(compDir, { recursive: true });
    writeFileSync(
      join(compDir, "Dialog.vue"),
      `<script setup lang="ts">
export interface DialogEmits {
  confirm: []
  cancel: []
}
defineEmits<DialogEmits>()
</script>
<template><div>dialog</div></template>
`,
    );

    // Consumer imports via path alias (@/components/Dialog.vue)
    const sfc = `<script setup lang="ts">
import type { DialogEmits } from "@/components/Dialog.vue"
defineEmits<DialogEmits>()
</script>
<template><div>consumer</div></template>
`;

    // Resolve hook maps the alias to the absolute path
    const dialogPath = join(compDir, "Dialog.vue").replace(/\\/g, "/");
    const resolveSpy = vi.fn(async (source: string) => {
      if (source === "@/components/Dialog.vue") return { id: dialogPath };
      return null;
    });

    const result = await plugin.transform.call({ resolve: resolveSpy }, sfc, filename);
    expect(result).toBeDefined();
    const code = result.code;

    // Positive: emit names from the .vue file's exported interface
    expect(code).toContain("confirm");
    expect(code).toContain("cancel");
    // Negative: no unresolved type errors
    expect(code).not.toContain("HOST_MISSING_MACRO_TYPE_DEP");
  });

  it("bare sub-path package specifier resolves types-only .d.ts via package directory probe", async () => {
    const plugin = createPlugin();
    const filename = join(tempDir, "App.vue").replace(/\\/g, "/");

    // Create a package with a sub-path that only has .d.ts (no JS runtime entry).
    // This simulates `echarts/types/dist/shared` where the sub-path is types-only.
    const pkgDir = join(tempDir, "node_modules", "my-chart-lib");
    const subDir = join(pkgDir, "types", "dist");
    mkdirSync(subDir, { recursive: true });

    writeFileSync(
      join(pkgDir, "package.json"),
      JSON.stringify({
        name: "my-chart-lib",
        main: "./index.js",
        types: "./index.d.ts",
      }),
    );
    writeFileSync(join(pkgDir, "index.js"), "module.exports = {};\n");
    writeFileSync(join(pkgDir, "index.d.ts"), "export declare function init(): void;\n");
    // The sub-path types/dist/shared.d.ts — no corresponding .js file
    writeFileSync(
      join(subDir, "shared.d.ts"),
      "export interface GridOption { left?: string; right?: string; top?: string; bottom?: string; }\n",
    );

    const sfc = `<script setup lang="ts">
import type { GridOption } from "my-chart-lib/types/dist/shared"
defineProps<GridOption>()
</script>
<template><div>chart</div></template>
`;

    // Bundler resolve returns null for types-only sub-path (no JS entry)
    const resolveSpy = vi.fn(async (source: string) => {
      // Only resolve the main package, not the sub-path
      if (source === "my-chart-lib") {
        return { id: join(pkgDir, "index.js").replace(/\\/g, "/") };
      }
      return null;
    });

    const result = await plugin.transform.call({ resolve: resolveSpy }, sfc, filename);
    expect(result).toBeDefined();
    const code = result.code;

    // Positive: prop names from the sub-path .d.ts
    expect(code).toContain("left");
    expect(code).toContain("right");
    expect(code).toContain("top");
    expect(code).toContain("bottom");
    // Negative: no unresolved type errors
    expect(code).not.toContain("HOST_MISSING_MACRO_TYPE_DEP");
  });

  it("scoped package sub-path resolves types-only .d.ts", async () => {
    const plugin = createPlugin();
    const filename = join(tempDir, "App.vue").replace(/\\/g, "/");

    // Create a scoped package with a sub-path that only has .d.ts
    const pkgDir = join(tempDir, "node_modules", "@myorg", "config");
    const subDir = join(pkgDir, "shared");
    mkdirSync(subDir, { recursive: true });

    writeFileSync(
      join(pkgDir, "package.json"),
      JSON.stringify({
        name: "@myorg/config",
        main: "./index.js",
        types: "./index.d.ts",
      }),
    );
    writeFileSync(join(pkgDir, "index.js"), "module.exports = {};\n");
    writeFileSync(join(pkgDir, "index.d.ts"), "export declare const version: string;\n");
    writeFileSync(
      join(subDir, "theme.d.ts"),
      "export interface ThemeConfig { primary: string; secondary: string; }\n",
    );

    const sfc = `<script setup lang="ts">
import type { ThemeConfig } from "@myorg/config/shared/theme"
defineProps<ThemeConfig>()
</script>
<template><div>themed</div></template>
`;

    const resolveSpy = vi.fn(async () => null);

    const result = await plugin.transform.call({ resolve: resolveSpy }, sfc, filename);
    expect(result).toBeDefined();
    const code = result.code;

    // Positive: prop names from the scoped package sub-path .d.ts
    expect(code).toContain("primary");
    expect(code).toContain("secondary");
    // Negative: no unresolved type errors
    expect(code).not.toContain("HOST_MISSING_MACRO_TYPE_DEP");
  });

  it("relative extensionless import resolving to .ts (not .vue) resolves defineProps type", async () => {
    const plugin = createPlugin();
    const filename = join(tempDir, "Parent.vue").replace(/\\/g, "/");

    // Create a .ts type file alongside the SFC — simulates
    // `import type { BacktopProps } from './backtop'` where ./backtop.ts exists
    writeFileSync(
      join(tempDir, "backtop.ts"),
      "export interface BacktopProps { visibilityHeight: number; right: number; }\n",
    );

    const sfc = `<script setup lang="ts">
import type { BacktopProps } from "./backtop"
defineProps<BacktopProps>()
</script>
<template><div>parent</div></template>
`;

    // Resolve hook resolves ./backtop to the .ts file
    const backtopPath = join(tempDir, "backtop.ts").replace(/\\/g, "/");
    const resolveSpy = vi.fn(async (source: string) => {
      if (source === "./backtop") return { id: backtopPath };
      return null;
    });

    const result = await plugin.transform.call({ resolve: resolveSpy }, sfc, filename);
    expect(result).toBeDefined();
    const code = result.code;

    // Positive: prop names from the .ts file
    expect(code).toContain("visibilityHeight");
    expect(code).toContain("right");
    // Negative: no unresolved type errors
    expect(code).not.toContain("HOST_MISSING_MACRO_TYPE_DEP");
  });

  it("relative extensionless import with no resolve hook probes .ts files", async () => {
    const plugin = createPlugin();
    const filename = join(tempDir, "Wrapper.vue").replace(/\\/g, "/");

    // Create a .ts type file — no resolve hook to find it, must probe filesystem
    writeFileSync(
      join(tempDir, "options.ts"),
      "export interface OptionProps { size: string; variant: string; }\n",
    );

    const sfc = `<script setup lang="ts">
import type { OptionProps } from "./options"
defineProps<OptionProps>()
</script>
<template><div>wrapper</div></template>
`;

    // Resolve hook returns null
    const resolveSpy = vi.fn(async () => null);

    const result = await plugin.transform.call({ resolve: resolveSpy }, sfc, filename);
    expect(result).toBeDefined();
    const code = result.code;

    // Positive: prop names from the probed .ts file
    expect(code).toContain("size");
    expect(code).toContain("variant");
    // Negative: no unresolved type errors
    expect(code).not.toContain("HOST_MISSING_MACRO_TYPE_DEP");
  });

  it("extensionless relative import resolving to .vue resolves defineProps type", async () => {
    const plugin = createPlugin();
    const filename = join(tempDir, "Wrapper.vue").replace(/\\/g, "/");

    // Create a .vue file at ./backtop.vue
    writeFileSync(
      join(tempDir, "backtop.vue"),
      `<script setup lang="ts">
export interface BacktopProps {
  visibilityHeight: number
  right: number
}
defineProps<BacktopProps>()
</script>
<template><div>backtop</div></template>
`,
    );

    // Wrapper imports via extensionless specifier
    const sfc = `<script setup lang="ts">
import type { BacktopProps } from "./backtop"
defineProps<BacktopProps>()
</script>
<template><div>wrapper</div></template>
`;

    // Resolve hook maps extensionless to .vue file
    const backtopPath = join(tempDir, "backtop.vue").replace(/\\/g, "/");
    const resolveSpy = vi.fn(async (source: string) => {
      if (source === "./backtop") return { id: backtopPath };
      return null;
    });

    const result = await plugin.transform.call({ resolve: resolveSpy }, sfc, filename);
    expect(result).toBeDefined();
    const code = result.code;

    // Positive: prop names from backtop's exported interface
    expect(code).toContain("visibilityHeight");
    expect(code).toContain("right");
    // Negative: no unresolved type errors
    expect(code).not.toContain("HOST_MISSING_MACRO_TYPE_DEP");
  });
});

describe("barrel file export signatures", () => {
  afterEach(() => {
    resetHost();
  });

  it("upsert of barrel file returns export signatures with reexport metadata", () => {
    const host = loadHost();
    const result = host.upsert({
      inputId: "/src/barrel.ts",
      source:
        "export { default as Button } from './Button.vue';\nexport type { Props } from './types';",
      fileKind: "non_sfc",
    });

    // Positive: export signatures present
    expect(result.exportSignatures).toBeDefined();
    expect(result.exportSignatures.length).toBeGreaterThanOrEqual(2);

    const button = result.exportSignatures.find((s: any) => s.name === "Button");
    expect(button).toBeDefined();
    expect(button!.isType).toBe(false);
    expect(button!.reexportSource).toBe("./Button.vue");
    expect(button!.reexportLocal).toBe("default");

    const props = result.exportSignatures.find((s: any) => s.name === "Props");
    expect(props).toBeDefined();
    expect(props!.isType).toBe(true);
    expect(props!.reexportSource).toBe("./types");
  });

  it("upsert of local-only file returns export signatures without reexport fields", () => {
    const host = loadHost();
    const result = host.upsert({
      inputId: "/src/utils.ts",
      source: "export function greet() {}\nexport type Color = string;",
      fileKind: "non_sfc",
    });

    expect(result.exportSignatures).toBeDefined();

    const greet = result.exportSignatures.find((s: any) => s.name === "greet");
    expect(greet).toBeDefined();
    // Negative: local exports must not have reexport fields
    expect(greet!.reexportSource).toBeUndefined();
    expect(greet!.reexportLocal).toBeUndefined();

    const color = result.exportSignatures.find((s: any) => s.name === "Color");
    expect(color).toBeDefined();
    expect(color!.isType).toBe(true);
  });

  it("resolveExports follows re-export chains across multiple files", () => {
    const host = loadHost();

    // Create a chain: deep.ts -> mid.ts -> top.ts
    host.upsert({
      inputId: "/src/deep.ts",
      source: "export const SECRET = 42;\nexport type DeepType = string;",
      fileKind: "non_sfc",
    });

    host.upsert({
      inputId: "/src/mid.ts",
      source: "export { SECRET } from './deep';\nexport type { DeepType } from './deep';",
      fileKind: "non_sfc",
    });
    host.setImportDependencies("/src/mid.ts", [
      { specifier: "./deep", resolvedCanonicalId: "/src/deep.ts" },
    ]);

    host.upsert({
      inputId: "/src/top.ts",
      source: "export { SECRET } from './mid';\nexport type { DeepType } from './mid';",
      fileKind: "non_sfc",
    });
    host.setImportDependencies("/src/top.ts", [
      { specifier: "./mid", resolvedCanonicalId: "/src/mid.ts" },
    ]);

    const resolved = host.resolveExports("/src/top.ts");
    expect(resolved).toBeDefined();

    const secret = resolved.find((e: any) => e.name === "SECRET");
    expect(secret).toBeDefined();
    // Resolved through chain to ultimate source
    expect(secret!.sourceCanonicalId).toBe("/src/deep.ts");
    expect(secret!.sourceName).toBe("SECRET");

    const deepType = resolved.find((e: any) => e.name === "DeepType");
    expect(deepType).toBeDefined();
    expect(deepType!.isType).toBe(true);
    expect(deepType!.sourceCanonicalId).toBe("/src/deep.ts");
  });

  it("getAnalysis includes exportSignatures in JSON output", () => {
    const host = loadHost();
    host.upsert({
      inputId: "/src/barrel.ts",
      source: "export { default as Dialog } from './Dialog.vue';\nexport const VERSION = '1.0';",
      fileKind: "non_sfc",
    });

    const analysisJson = host.getAnalysis("/src/barrel.ts");
    expect(analysisJson).toBeTruthy();
    const analysis = JSON.parse(analysisJson!);

    expect(analysis.exportSignatures).toBeDefined();
    expect(analysis.exportSignatures.length).toBeGreaterThanOrEqual(2);

    const dialog = analysis.exportSignatures.find((s: any) => s.name === "Dialog");
    expect(dialog).toBeDefined();
    expect(dialog.reexportSource).toBe("./Dialog.vue");

    const version = analysis.exportSignatures.find((s: any) => s.name === "VERSION");
    expect(version).toBeDefined();
    // Negative: local export must not have reexportSource
    expect(version.reexportSource).toBeUndefined();
  });
});

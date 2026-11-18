/**
 * Plugin hooks serve their modules from the instance's own bound session:
 * canonical keys, one request grammar, and no state shared between instances.
 */
import { afterAll, afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { mkdirSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { defineConfig, resolveConfig } from "vite";
import { Verter, VerterVue, unpluginFactory } from "../index";
import { ModuleCatalog } from "../core/host-session/catalog";

const SFC_WITH_STYLE = `<script setup lang="ts">
const msg = "hello"
</script>
<template><div class="a">{{ msg }}</div></template>
<style>.a{color:red}</style>`;

const SFC_WITH_ONE_STYLE_V1 = `<template><div class="v1">one</div></template>
<style>.only-in-first-revision{color:blue}</style>`;

const SFC_WITHOUT_STYLE_V2 = `<template><div class="v2">two</div></template>`;

let tempDir: string;
const plugins: Array<{ closeBundle: () => Promise<void> }> = [];

beforeAll(() => {
  tempDir = join(
    tmpdir(),
    `verter-session-hooks-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
  );
  mkdirSync(tempDir, { recursive: true });
});

afterAll(() => {
  rmSync(tempDir, { recursive: true, force: true });
});

afterEach(async () => {
  for (const plugin of plugins.splice(0)) await plugin.closeBundle();
});

async function vitePlugin(): Promise<any> {
  const plugin = unpluginFactory(undefined, {
    framework: "vite",
    versions: { unplugin: "0.0.0", vite: "7.0.0" },
  } as any) as any;
  const config = await resolveConfig(
    defineConfig({ root: tempDir, build: { cssCodeSplit: false } }),
    "build",
    "production",
  );
  await plugin.vite.configResolved(config);
  plugins.push(plugin);
  return plugin;
}

function rollupPlugin(): any {
  const plugin = unpluginFactory(undefined, {
    framework: "rollup",
    versions: { unplugin: "0.0.0", rollup: "0.0.0" },
  } as any) as any;
  plugins.push(plugin);
  return plugin;
}

const hookContext = () => ({ warn: vi.fn() });

describe("plugin hooks through the host session", () => {
  it.runIf(process.platform === "win32")(
    "a style sub-request spelled with a lowercase drive letter is served from the same entry",
    async () => {
      const plugin = await vitePlugin();
      const file = join(tempDir, "App.vue").replace(/\\/g, "/");
      const upper = file.charAt(0).toUpperCase() + file.slice(1);
      const lower = file.charAt(0).toLowerCase() + file.slice(1);

      const publish = vi.spyOn(ModuleCatalog.prototype, "publish");
      await plugin.transform.call(hookContext(), SFC_WITH_STYLE, upper);
      expect(publish).toHaveBeenCalled();
      for (const [record] of publish.mock.calls) expect(record.carrier).toBe(lower);
      const style = await plugin.load.call(
        hookContext(),
        `${lower}?vue&type=style&index=0&inline&used&lang.css`,
      );

      expect(style).toBeDefined();
      expect(style.code).toContain(".a{color:red}");
      expect(style.code).not.toContain("<script");
    },
  );

  it("a style request with the inline flag gets the same bytes as the linked request", async () => {
    const plugin = await vitePlugin();
    const file = join(tempDir, "Inline.vue").replace(/\\/g, "/");

    await plugin.transform.call(hookContext(), SFC_WITH_STYLE, file);
    const linked = await plugin.load.call(hookContext(), `${file}?vue&type=style&index=0&lang.css`);
    const inline = await plugin.load.call(
      hookContext(),
      `${file}?vue&type=style&index=0&inline&lang.css`,
    );

    expect(linked.code).toContain(".a{color:red}");
    expect(inline.code).toBe(linked.code);
  });

  it("a marked request for a file outside the include filter is not claimed", () => {
    const plugin = rollupPlugin();

    const claimed = plugin.resolveId.call(
      hookContext(),
      "/a/readme.md?vue&type=style&index=0&lang.css",
    );

    expect(claimed).toBeUndefined();
  });

  it("a malformed marked request is refused", () => {
    const plugin = rollupPlugin();

    expect(() => plugin.resolveId.call(hookContext(), "/a/App.vue?vue")).toThrow(
      /marker-without-type/,
    );
  });

  it("two plugin instances do not share modules", async () => {
    const first = await vitePlugin();
    const second = await vitePlugin();
    const file = join(tempDir, "Shared.vue").replace(/\\/g, "/");

    await first.transform.call(hookContext(), SFC_WITH_STYLE, file);

    const served = await first.load.call(hookContext(), `${file}?vue&type=script&lang.ts`);
    expect(served?.code).toBeDefined();
    // The second instance has no record of its own and the carrier is not on disk.
    await expect(
      second.load.call(hookContext(), `${file}?vue&type=script&lang.ts`),
    ).rejects.toMatchObject({ refusal: { kind: "carrier-unavailable" } });
  });

  it("closing one instance leaves the other serving", async () => {
    const first = await vitePlugin();
    const second = await vitePlugin();
    const file = join(tempDir, "Survivor.vue").replace(/\\/g, "/");

    await first.transform.call(hookContext(), SFC_WITH_STYLE, file);
    await second.transform.call(hookContext(), SFC_WITH_STYLE, file);
    await first.closeBundle();

    const served = await second.load.call(hookContext(), `${file}?vue&type=script&lang.ts`);
    expect(served?.code).toContain("hello");
  });

  it("a page-macro request is compiled as the main module", () => {
    const plugin = rollupPlugin();

    expect(plugin.transformInclude.call(hookContext(), "/a/pages/index.vue?macro=true")).toBe(true);
  });

  it("an edited carrier never serves a slot of its previous revision", async () => {
    const plugin = await vitePlugin();
    const file = join(tempDir, "Edited.vue").replace(/\\/g, "/");
    const styleRequest = `${file}?vue&type=style&index=0&lang.css`;

    await plugin.transform.call(hookContext(), SFC_WITH_ONE_STYLE_V1, file);
    const first = await plugin.load.call(hookContext(), styleRequest);
    expect(first.code).toContain("only-in-first-revision");

    await plugin.transform.call(hookContext(), SFC_WITHOUT_STYLE_V2, file);

    // The new revision has no style block: the request is refused by name.
    await expect(plugin.load.call(hookContext(), styleRequest)).rejects.toMatchObject({
      refusal: { kind: "module-not-published", request: styleRequest },
    });
  });

  describe("carriers beyond the language registry", () => {
    const MARKDOWN_COMPONENT = '<template><div class="md">{{ 1 + 1 }}</div></template>';
    const markdownId = () => join(tempDir, "Page.md").replace(/\\/g, "/");

    it("a suffix registered through `carriers` compiles as Vue", async () => {
      const plugin = Verter.rollup({ carriers: { ".md": "vue" } }) as any;
      plugins.push(plugin);
      const result = await plugin.transform.call(hookContext(), MARKDOWN_COMPONENT, markdownId());
      expect(result?.code).toContain("md");
    });

    it("the Vue entry reads its `include` as the files that are Vue", async () => {
      const plugin = VerterVue.rollup({ include: /\.md$/ }) as any;
      plugins.push(plugin);
      const result = await plugin.transform.call(hookContext(), MARKDOWN_COMPONENT, markdownId());
      expect(result?.code).toContain("md");
    });

    it("the Vue entry's `exclude` leaves a matching file out", async () => {
      const plugin = VerterVue.rollup({ include: /\.md$/, exclude: /Page\.md$/ }) as any;
      plugins.push(plugin);
      expect(
        await plugin.transform.call(hookContext(), MARKDOWN_COMPONENT, markdownId()),
      ).toBeFalsy();
    });

    it("the generic entry with only an `include` refuses a suffix the registry does not know", async () => {
      const plugin = Verter.rollup({ include: /\.md$/ }) as any;
      plugins.push(plugin);
      expect(
        await plugin.transform.call(hookContext(), MARKDOWN_COMPONENT, markdownId()),
      ).toBeFalsy();
    });
  });
});

/**
 * What a component is, across the graphs one project builds: one scope id for
 * the client and the server build, no hot-update code in a production server
 * module, and one plugin instance serving two Vite environments without mixing
 * their modules.
 */
import { afterAll, afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { build, defineConfig, resolveConfig } from "vite";
import { VerterVue, unpluginFactory } from "../index";

const COMPONENT = `<script setup lang="ts">
const label = "scoped"
</script>
<template><p class="tag">{{ label }}</p></template>
<style scoped>.tag { color: red; }</style>`;

let root: string;
const plugins: Array<{ closeBundle: () => Promise<void> }> = [];

beforeAll(() => {
  root = mkdtempSync(join(tmpdir(), "verter-graph-identity-")).replace(/\\/g, "/");
  mkdirSync(root, { recursive: true });
  writeFileSync(join(root, "App.vue"), COMPONENT);
  writeFileSync(join(root, "main.ts"), "import App from './App.vue';\nexport default App;\n");
});

afterAll(() => {
  rmSync(root, { recursive: true, force: true });
});

afterEach(async () => {
  for (const plugin of plugins.splice(0)) await plugin.closeBundle();
});

const scopeIdPattern = /data-v-[0-9a-f]+/;

async function buildGraph(ssr: boolean): Promise<string> {
  const result = (await build({
    root,
    configFile: false,
    logLevel: "silent",
    plugins: [VerterVue.vite()] as never,
    build: {
      write: false,
      minify: false,
      ssr: ssr ? join(root, "main.ts") : false,
      lib: ssr ? undefined : { entry: join(root, "main.ts"), formats: ["es"], fileName: "out" },
      rollupOptions: {
        external: (id: string) => id === "vue" || id.startsWith("vue/") || id.startsWith("@vue/"),
      },
    },
  })) as any;
  const outputs = Array.isArray(result) ? result : [result];
  return outputs
    .flatMap((output: any) => output.output)
    .map((chunk: any) => (chunk.type === "chunk" ? chunk.code : String(chunk.source)))
    .join("\n");
}

describe("component identity across graphs", () => {
  it("a component has one scope id in the client and the server build", async () => {
    const client = await buildGraph(false);
    const server = await buildGraph(true);

    const clientId = scopeIdPattern.exec(client)?.[0];
    const serverId = scopeIdPattern.exec(server)?.[0];
    expect(clientId).toBeDefined();
    expect(serverId).toBe(clientId);
  });

  it("a production server module carries no hot-update code", async () => {
    const server = await buildGraph(true);

    expect(server).not.toContain("import.meta.hot");
    expect(server).not.toContain("__hmrId");
  });
});

describe("one plugin serving two environments", () => {
  it("keeps the client and the server module of a carrier apart", async () => {
    const plugin = unpluginFactory(undefined, {
      framework: "vite",
      versions: { unplugin: "0.0.0", vite: "7.0.0" },
    } as never) as any;
    const config = await resolveConfig(
      defineConfig({ root, build: { cssCodeSplit: false } }),
      "build",
      "production",
    );
    await plugin.vite.configResolved(config);
    plugins.push(plugin);
    const file = join(root, "App.vue").replace(/\\/g, "/");
    const client = {
      warn: vi.fn(),
      environment: { name: "client", config: { consumer: "client" } },
    };
    const server = { warn: vi.fn(), environment: { name: "ssr", config: { consumer: "server" } } };
    const script = `${file}?vue&type=script&lang.ts`;

    await plugin.transform.call(client, COMPONENT, file);
    await plugin.transform.call(server, COMPONENT, file);
    const clientModule = await plugin.load.call(client, script);
    const serverModule = await plugin.load.call(server, script);

    expect(serverModule.code).toContain("ssrRender");
    expect(clientModule.code).not.toContain("ssrRender");
  });
});

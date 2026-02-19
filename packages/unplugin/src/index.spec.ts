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

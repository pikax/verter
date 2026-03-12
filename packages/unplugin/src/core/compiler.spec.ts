/**
 * @ai-generated - Tests for host-based compiler module.
 * Tests the VerterHost integration replacing the old compileForVite direct calls.
 *
 * Note: NAPI-RS converts snake_case Rust fields to camelCase at JS runtime,
 * so we use camelCase for all object properties despite the TS types showing snake_case.
 */
import { describe, it, expect, beforeEach } from "vitest";
import { loadHost, resetHost, generateComponentId, getHash } from "./compiler";

describe("loadHost", () => {
  beforeEach(() => {
    resetHost();
  });

  it("returns a VerterHost instance with expected methods", () => {
    const host = loadHost();
    expect(host).toBeDefined();
    expect(typeof host.upsert).toBe("function");
    expect(typeof host.resolve).toBe("function");
    expect(typeof host.getVirtualFile).toBe("function");
    expect(typeof host.applyBlockOverrides).toBe("function");
    expect(typeof host.listVirtualFiles).toBe("function");
    expect(typeof host.remove).toBe("function");
  });

  it("returns the same instance on subsequent calls", () => {
    const host1 = loadHost();
    const host2 = loadHost();
    expect(host1).toBe(host2);
  });

  it("returns a fresh instance after resetHost", () => {
    const host1 = loadHost();
    resetHost();
    const host2 = loadHost();
    expect(host1).not.toBe(host2);
  });
});

describe("generateComponentId", () => {
  // @ai-generated - Match Vue's @vitejs/plugin-vue behavior:
  // Dev:  hash(relativePath)
  // Prod: hash(relativePath + source)

  it("dev mode hashes only the relative path (matches Vue)", () => {
    const id = generateComponentId("/project/src/App.vue", "source", false, "/project");
    expect(id).toBe(getHash("src/App.vue"));
  });

  it("prod mode hashes relative path + source (matches Vue)", () => {
    const id = generateComponentId("/project/src/App.vue", "source", true, "/project");
    expect(id).toBe(getHash("src/App.vue" + "source"));
  });

  it("normalizes Windows backslashes in relative path", () => {
    const unix = generateComponentId("/project/src/App.vue", "s", false, "/project");
    const win = generateComponentId("\\project\\src\\App.vue", "s", false, "\\project");
    expect(unix).toBe(win);
  });

  it("falls back to full normalized path when root is not provided", () => {
    const id = generateComponentId("/path/to/App.vue", "source", false);
    expect(id).toBe(getHash("/path/to/App.vue"));
  });

  it("falls back to full normalized path when root is not provided (prod)", () => {
    const id = generateComponentId("/path/to/App.vue", "source", true);
    expect(id).toBe(getHash("/path/to/App.vue" + "source"));
  });
});

describe("host: upsert + getVirtualFile", () => {
  beforeEach(() => {
    resetHost();
  });

  it("compiles a simple SFC and returns main module code", () => {
    const host = loadHost();
    const sfc = [
      "<script setup>",
      "const msg = 'hello'",
      "</script>",
      "<template><div>{{ msg }}</div></template>",
    ].join("\n");

    host.upsert({
      inputId: "/test/App.vue",
      source: sfc,
      compileProfile: {
        filename: "/test/App.vue",
        isProduction: false,
        hmrStrategy: "vite",
        sourceMap: true,
      },
    } as any);

    const main = host.getVirtualFile({
      rawId: "/test/App.vue",
      compileProfile: {
        filename: "/test/App.vue",
        isProduction: false,
        hmrStrategy: "vite",
        sourceMap: true,
      },
    } as any);

    expect(main.code).toContain("_sfc_main");
    expect(main.code).toContain("export default");
  });

  it("main module imports styles as virtual modules", () => {
    const host = loadHost();
    const sfc = [
      "<script setup>const x = 1</script>",
      "<template><div>{{ x }}</div></template>",
      "<style scoped>.red { color: red }</style>",
    ].join("\n");

    const profile = { filename: "/test/App.vue" };

    host.upsert({
      inputId: "/test/App.vue",
      source: sfc,
      compileProfile: profile,
    } as any);

    const main = host.getVirtualFile({
      rawId: "/test/App.vue",
      compileProfile: profile,
    } as any);

    expect(main.code).toContain("import");
    expect(main.code).toContain("type=style");
  });

  it("returns style virtual file content", () => {
    const host = loadHost();
    const sfc = [
      "<script setup>const x = 1</script>",
      "<template><div>{{ x }}</div></template>",
      "<style>.red { color: red }</style>",
    ].join("\n");

    const profile = { filename: "/test/App.vue" };

    host.upsert({
      inputId: "/test/App.vue",
      source: sfc,
      compileProfile: profile,
    } as any);

    const style = host.getVirtualFile({
      rawId: "/test/App.vue?vue&type=style&index=0",
      compileProfile: profile,
    } as any);

    expect(style.code).toContain("color");
    expect(style.code).toContain("red");
  });

  it("returns scoped style with scope attribute", () => {
    const host = loadHost();
    const sfc = [
      "<script setup>const x = 1</script>",
      "<template><div>{{ x }}</div></template>",
      "<style scoped>.red { color: red }</style>",
    ].join("\n");

    const profile = { filename: "/test/App.vue", componentId: "abc123" };

    host.upsert({
      inputId: "/test/App.vue",
      source: sfc,
      compileProfile: profile,
    } as any);

    const style = host.getVirtualFile({
      rawId: "/test/App.vue?vue&type=style&index=0",
      compileProfile: profile,
    } as any);

    // Scoped styles should contain the scope attribute selector
    expect(style.code).toContain("[data-v-");
  });

  // @ai-generated - Verifies caching: same file upserted twice, second reports no change
  it("caching: unchanged re-upsert reports no change", () => {
    const host = loadHost();
    const sfc = "<script setup>const x = 1</script>\n<template><div>{{ x }}</div></template>";
    const profile = { filename: "/test/App.vue" };

    const first = host.upsert({
      inputId: "/test/App.vue",
      source: sfc,
      compileProfile: profile,
    } as any);
    expect(first.changed).toBe(true);

    const second = host.upsert({
      inputId: "/test/App.vue",
      source: sfc,
      compileProfile: profile,
    } as any);
    expect(second.changed).toBe(false);
  });

  // @ai-generated - Verifies compile cache: getVirtualFile returns same code on repeated calls
  it("getVirtualFile returns identical code on repeated calls (compile cache)", () => {
    const host = loadHost();
    const sfc = "<script setup>const x = 1</script>\n<template><div>{{ x }}</div></template>";
    const profile = { filename: "/test/App.vue", isProduction: false };

    host.upsert({
      inputId: "/test/App.vue",
      source: sfc,
      compileProfile: profile,
    } as any);

    const first = host.getVirtualFile({ rawId: "/test/App.vue", compileProfile: profile } as any);
    const second = host.getVirtualFile({ rawId: "/test/App.vue", compileProfile: profile } as any);

    expect(first.code).toBe(second.code);
  });

  // @ai-generated - Production vs dev produces different output
  it("different compile profiles produce different output", () => {
    const host = loadHost();
    const sfc = "<script setup>const x = 1</script>\n<template><div>{{ x }}</div></template>";

    host.upsert({
      inputId: "/test/App.vue",
      source: sfc,
      compileProfile: { filename: "/test/App.vue", isProduction: false },
    } as any);

    const dev = host.getVirtualFile({
      rawId: "/test/App.vue",
      compileProfile: { filename: "/test/App.vue", isProduction: false },
    } as any);

    const prod = host.getVirtualFile({
      rawId: "/test/App.vue",
      compileProfile: { filename: "/test/App.vue", isProduction: true },
    } as any);

    expect(dev.code).not.toBe(prod.code);
  });

  it("custom block virtual files are served", () => {
    const host = loadHost();
    const sfc = [
      "<script setup>const x = 1</script>",
      "<template><div>{{ x }}</div></template>",
      '<i18n lang="json">{"en":{"hello":"Hello"}}</i18n>',
    ].join("\n");

    const profile = { filename: "/test/App.vue" };

    host.upsert({
      inputId: "/test/App.vue",
      source: sfc,
      compileProfile: profile,
    } as any);

    const custom = host.getVirtualFile({
      rawId: "/test/App.vue?vue&type=i18n&index=0",
      compileProfile: profile,
    } as any);

    expect(custom.code).toContain("hello");
  });
});

describe("host: resolve", () => {
  beforeEach(() => {
    resetHost();
  });

  it("resolves a style virtual module ID", () => {
    const host = loadHost();
    const resolved = host.resolve("/test/App.vue?vue&type=style&index=0");
    expect(resolved).not.toBeNull();
    expect(resolved!.nodeKind.kind).toBe("style");
    expect(resolved!.nodeKind.index).toBe(0);
  });

  it("resolves a template virtual module ID", () => {
    const host = loadHost();
    const resolved = host.resolve("/test/App.vue?vue&type=template");
    expect(resolved).not.toBeNull();
    expect(resolved!.nodeKind.kind).toBe("template");
  });

  it("resolves a script virtual module ID", () => {
    const host = loadHost();
    const resolved = host.resolve("/test/App.vue?vue&type=script");
    expect(resolved).not.toBeNull();
    expect(resolved!.nodeKind.kind).toBe("script");
  });

  it("resolves a plain .vue file as main", () => {
    const host = loadHost();
    const resolved = host.resolve("/test/App.vue");
    expect(resolved).not.toBeNull();
    expect(resolved!.nodeKind.kind).toBe("main");
  });

  it("resolves with existsInHost=false for unregistered files", () => {
    const host = loadHost();
    const resolved = host.resolve("/test/Unknown.vue");
    expect(resolved).not.toBeNull();
    expect(resolved!.existsInHost).toBe(false);
  });

  it("resolves with existsInHost=true after upsert", () => {
    const host = loadHost();
    host.upsert({
      inputId: "/test/App.vue",
      source: "<template><div>hi</div></template>",
      compileProfile: { filename: "/test/App.vue" },
    } as any);

    const resolved = host.resolve("/test/App.vue");
    expect(resolved).not.toBeNull();
    expect(resolved!.existsInHost).toBe(true);
  });
});

describe("host: remove", () => {
  beforeEach(() => {
    resetHost();
  });

  it("removes a registered file", () => {
    const host = loadHost();
    host.upsert({
      inputId: "/test/App.vue",
      source: "<template><div>hi</div></template>",
      compileProfile: { filename: "/test/App.vue" },
    } as any);

    const result = host.remove("/test/App.vue");
    expect(result).not.toBeNull();
    expect(result!.canonicalId).toBe("/test/App.vue");
  });

  it("getVirtualFile throws after removal", () => {
    const host = loadHost();
    host.upsert({
      inputId: "/test/App.vue",
      source: "<template><div>hi</div></template>",
      compileProfile: { filename: "/test/App.vue" },
    } as any);

    host.remove("/test/App.vue");

    expect(() => {
      host.getVirtualFile({
        rawId: "/test/App.vue",
        compileProfile: { filename: "/test/App.vue" },
      } as any);
    }).toThrow();
  });

  it("returns null for unregistered file", () => {
    const host = loadHost();
    const result = host.remove("/test/Unknown.vue");
    expect(result).toBeNull();
  });
});

describe("host: listVirtualFiles", () => {
  beforeEach(() => {
    resetHost();
  });

  it("lists all virtual nodes for a file with script, template, and style", () => {
    const host = loadHost();
    const sfc = [
      "<script setup>const x = 1</script>",
      "<template><div>{{ x }}</div></template>",
      "<style>.red { color: red }</style>",
    ].join("\n");

    host.upsert({
      inputId: "/test/App.vue",
      source: sfc,
      compileProfile: { filename: "/test/App.vue" },
    } as any);

    const nodes = host.listVirtualFiles("/test/App.vue");
    const kinds = nodes.map((n: any) => n.kind);

    expect(kinds).toContain("main");
    expect(kinds).toContain("script");
    expect(kinds).toContain("template");
    expect(kinds).toContain("style");
  });
});

describe("host: applyBlockOverrides", () => {
  beforeEach(() => {
    resetHost();
  });

  // @ai-generated - Block overrides replace style output without re-upsert
  it("overrides style content without re-upsert", () => {
    const host = loadHost();
    const sfc = [
      "<script setup>const x = 1</script>",
      "<template><div>{{ x }}</div></template>",
      "<style>.a { color: red }</style>",
    ].join("\n");

    const profile = { filename: "/test/App.vue" };

    host.upsert({
      inputId: "/test/App.vue",
      source: sfc,
      compileProfile: profile,
    } as any);

    const before = host.getVirtualFile({
      rawId: "/test/App.vue?vue&type=style&index=0",
      compileProfile: profile,
    } as any);

    host.applyBlockOverrides({
      canonicalId: "/test/App.vue",
      compileProfile: profile,
      overrides: [{ blockType: "style", index: 0, code: ".a { color: green }" }],
    } as any);

    const after = host.getVirtualFile({
      rawId: "/test/App.vue?vue&type=style&index=0",
      compileProfile: profile,
    } as any);

    expect(before.code).not.toBe(after.code);
    expect(after.code).toContain("green");
  });
});

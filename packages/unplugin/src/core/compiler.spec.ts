/**
 * @ai-generated - Tests for host-based compiler module.
 * Tests the VerterHost integration replacing the old compileForVite direct calls.
 *
 * Note: NAPI-RS converts snake_case Rust fields to camelCase at JS runtime,
 * so we use camelCase for all object properties despite the TS types showing snake_case.
 */
import { describe, it, expect, beforeEach, vi } from "vitest";
import { createHash } from "node:crypto";
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

describe("style wrappers", () => {
  const SOURCE = ".foo { color: red }";
  // Minimal surgical edit: the scoped-selector rewrite inserts the scope
  // attribute right after the compound, touching NOTHING else — an exact
  // byte pin, not a substring search, so a stray reformat/normalization
  // (e.g. a re-introduced full AST reprint) fails this test.
  const EXPECTED = ".foo[data-v-abc123] { color: red }";

  it("does not export processStyle", async () => {
    const mod = await import("./compiler");
    expect(typeof (mod as { processStyle?: unknown }).processStyle).toBe("undefined");
  });

  it("exports transformVueStyle as the live native wrapper", async () => {
    const mod = await import("./compiler");
    expect(typeof mod.transformVueStyle).toBe("function");
  });

  it("transformVueStyle scopes with a surgical selector edit", async () => {
    const mod = await import("./compiler");
    const result = mod.transformVueStyle(SOURCE, {
      scopeId: "abc123",
      scoped: true,
    });
    expect(result.code).toBe(EXPECTED);
  });

  it("leaves bytes a Vue-owned transform does not touch identical to the authored input", async () => {
    const mod = await import("./compiler");
    const result = mod.transformVueStyle(SOURCE, {
      scopeId: "abc123",
      scoped: false,
    });
    expect(result.code).toBe(SOURCE);
  });

  it("transformVueStyle reports an empty refusals list on an ordinary successful transform", async () => {
    const mod = await import("./compiler");
    const result = mod.transformVueStyle(SOURCE, {
      scopeId: "abc123",
      scoped: true,
    });
    expect(result.refusals).toEqual([]);
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
      '<i18n>{"en":{"hello":"Hello"}}</i18n>',
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

  // @ai-generated - A sealed, stamped result replaces one processed block without re-upsert.
  it("admits one stamped result and rejects correlation replay", () => {
    const host = loadHost();
    const authoredPreprocessorSyntax = "$authored-color: red";
    const sfc = [
      "<script setup>const x = 1</script>",
      "<template><div>{{ x }}</div></template>",
      `<style lang="postcss">${authoredPreprocessorSyntax}; .authored-only { color: $authored-color }</style>`,
    ].join("\n");

    const profile = { filename: "/test/App.vue" };
    const styleRequest = {
      rawId: "/test/App.vue?vue&type=style&index=0",
      compileProfile: profile,
    };

    const update = host.upsert({
      inputId: "/test/App.vue",
      source: sfc,
      compileProfile: profile,
    } as any);

    const request = update.preprocessorRequests[0];
    expect(request.availability).toBe("processedContentRequired");
    expect(request.content).toContain(authoredPreprocessorSyntax);
    expect(() => host.getVirtualFile(styleRequest as any)).toThrow(/ProcessedContentRequired/);

    const code = ".supplied-only { color: green }";

    host.applyBlockOverrides({
      canonicalId: "/test/App.vue",
      compileProfile: profile,
      overrides: [
        {
          correlationToken: request.correlationToken,
          blockToken: request.blockToken,
          ownerRevision: request.ownerRevision,
          artifactToken: request.artifactToken,
          expectedLanguage: request.expectedLanguage,
          priorBasisToken: request.priorBasisToken,
          basisToken: request.basisToken,
          sourceSpaceToken: request.sourceSpaceToken,
          code,
          codeHash: `sha256:${createHash("sha256")
            .update("verter.block-content.bytes.v1\0")
            .update(code)
            .digest("hex")}`,
          suppliedProvenance: "compiler.spec",
        },
      ],
    } as any);

    const compiledStyle = host.getVirtualFile(styleRequest as any);
    expect(compiledStyle.code).toContain(".supplied-only");
    expect(compiledStyle.code).toContain("color: green");
    expect(compiledStyle.code).not.toContain(authoredPreprocessorSyntax);
    expect(compiledStyle.code).not.toContain(".authored-only");

    expect(() =>
      host.applyBlockOverrides({
        canonicalId: "/test/App.vue",
        compileProfile: profile,
        overrides: [
          {
            correlationToken: request.correlationToken,
            blockToken: request.blockToken,
            ownerRevision: request.ownerRevision,
            artifactToken: request.artifactToken,
            expectedLanguage: request.expectedLanguage,
            priorBasisToken: request.priorBasisToken,
            basisToken: request.basisToken,
            sourceSpaceToken: request.sourceSpaceToken,
            code,
            codeHash: `sha256:${createHash("sha256")
              .update("verter.block-content.bytes.v1\0")
              .update(code)
              .digest("hex")}`,
          },
        ],
      } as any),
    ).toThrow(/correlation.*terminal/i);
  });
});

describe("typed render request construction", () => {
  const base = {
    filename: "/test/App.vue",
    componentId: "abc123",
    isProduction: false,
    customElement: false,
    ssr: false,
    forceJs: false,
    hmrStrategy: "vite" as const,
    sourceMap: true,
  };

  it("builds one Vue client runtime product with identity and option axes", async () => {
    const { typedRenderRequest } = await import("./compiler");
    const request = typedRenderRequest(base, "vue");
    expect(request.framework).toBe("vue");
    expect(request.products).toEqual([{ kind: "runtimeClient", runtimeSourceMap: true }]);
    expect(request.identity).toMatchObject({
      isProduction: false,
      forceJs: false,
      filename: "/test/App.vue",
      componentId: "abc123",
    });
    expect(request.options).toMatchObject({
      backend: "inferred",
      ssr: false,
      scriptCustomElement: false,
      isCustomElement: [],
      babelParserPlugins: [],
    });
  });

  it("selects the runtimeServer product for SSR demands", async () => {
    const { typedRenderRequest } = await import("./compiler");
    const request = typedRenderRequest({ ...base, ssr: true }, "vue");
    expect(request.products).toEqual([{ kind: "runtimeServer", runtimeSourceMap: true }]);
    expect(request.options).toMatchObject({ ssr: true });
  });

  it("builds the neutral svelte request shape", async () => {
    const { typedRenderRequest } = await import("./compiler");
    const request = typedRenderRequest(base, "sveltejs");
    expect(request.framework).toBe("svelte");
    expect(request.products).toEqual([{ kind: "runtimeClient", runtimeSourceMap: true }]);
    expect(request.options).toEqual({});
  });
});

describe("typed render request assembly axes", () => {
  const profile = {
    filename: "/test/App.vue",
    componentId: "abc123",
    isProduction: true,
    customElement: false,
    ssr: true,
    ssrModuleId: "src/App.vue",
    forceJs: false,
    hmrStrategy: "none" as const,
    sourceMap: false,
  };

  it("carries the SSR-manifest key and dev-server flavour on the identity", async () => {
    const { typedRenderRequest } = await import("./compiler");
    const request = typedRenderRequest(profile, "vue");
    expect(request.identity).toMatchObject({
      ssrModuleId: "src/App.vue",
      hmrStrategy: "none",
    });

    const without = typedRenderRequest({ ...profile, ssrModuleId: undefined }, "vue");
    expect(without.identity.ssrModuleId).toBeUndefined();

    const dev = typedRenderRequest({ ...profile, isProduction: false, hmrStrategy: "vite" }, "vue");
    expect(dev.identity.hmrStrategy).toBe("vite");
  });

  it("states the authored-only style cascade only when the bundler owns styles", async () => {
    const { typedRenderRequest } = await import("./compiler");
    const authoredOnly = typedRenderRequest(profile, "vue", { authoredOnlyStyles: true });
    expect(authoredOnly.products).toEqual([
      { kind: "runtimeServer", runtimeSourceMap: false, styleProcessing: "authored-only" },
    ]);

    const complete = typedRenderRequest(profile, "vue");
    expect(complete.products).toEqual([{ kind: "runtimeServer", runtimeSourceMap: false }]);
  });
});

describe("typed response readers", () => {
  beforeEach(() => {
    resetHost();
  });

  it("one typed compile publishes the Main node and indexed style artifacts", async () => {
    const { typedRenderRequest, runtimeMainNode, runtimeStyleArtifacts } =
      await import("./compiler");
    const host = loadHost();
    host.upsert({
      inputId: "/test/TypedReader.vue",
      source: [
        "<script setup>const x = 1</script>",
        "<template><div>{{ x }}</div></template>",
        "<style>.a { color: red }</style>",
        "<style>.b { color: blue }</style>",
      ].join("\n"),
    });

    const response = host.compileRequest(
      "/test/TypedReader.vue",
      typedRenderRequest(
        {
          filename: "/test/TypedReader.vue",
          isProduction: true,
          customElement: false,
          ssr: false,
          forceJs: false,
          hmrStrategy: "none",
          sourceMap: false,
        },
        "vue",
      ),
    );

    const main = runtimeMainNode(response, false);
    expect(main).toBeDefined();
    expect(main?.code).toContain("_sfc_main");
    expect(main?.code).toContain("export default");

    const styles = runtimeStyleArtifacts(response, false);
    expect(styles).toHaveLength(2);
    expect(styles[0].code).toContain("red");
    expect(styles[1].code).toContain("blue");
    expect(styles[0].lang).toBe("css");
  });
});

describe("typed compile diagnostic disposition", () => {
  it("throws on error-severity diagnostics", async () => {
    const { forwardTypedDiagnostics } = await import("./compiler");
    expect(() =>
      forwardTypedDiagnostics("/test/App.vue", {
        canonicalId: "/test/App.vue",
        diagnostics: {
          hasErrors: true,
          diagnostics: [
            { severity: "error", code: "E1", message: "typed boom", spanStart: 0, spanEnd: 0 },
          ],
        },
        products: [],
      }),
    ).toThrow(/typed boom/);
  });

  it("throws when hasErrors is set even without an error item", async () => {
    const { forwardTypedDiagnostics } = await import("./compiler");
    expect(() =>
      forwardTypedDiagnostics("/test/App.vue", {
        canonicalId: "/test/App.vue",
        diagnostics: { hasErrors: true, diagnostics: [] },
        products: [],
      }),
    ).toThrow(/typed compile reported errors/);
  });

  it("forwards only warning-severity diagnostics", async () => {
    const { forwardTypedDiagnostics } = await import("./compiler");
    const warn = vi.fn();
    forwardTypedDiagnostics(
      "/test/App.vue",
      {
        canonicalId: "/test/App.vue",
        diagnostics: {
          hasErrors: false,
          diagnostics: [
            { severity: "warning", code: "W1", message: "soft", spanStart: 0, spanEnd: 0 },
            { severity: "info", code: "I1", message: "chatter", spanStart: 0, spanEnd: 0 },
          ],
        },
        products: [],
      },
      warn,
    );
    expect(warn).toHaveBeenCalledTimes(1);
    expect(warn.mock.calls[0][0].message).toContain("W1");
    expect(warn.mock.calls[0][0].message).toContain("soft");
    expect(JSON.stringify(warn.mock.calls)).not.toContain("chatter");
  });
});

describe("typed compile Main reader", () => {
  it("throws when the typed response published no runtime Main node", async () => {
    const { requireRuntimeMain } = await import("./compiler");
    expect(() =>
      requireRuntimeMain(
        {
          canonicalId: "/test/App.vue",
          diagnostics: { hasErrors: false, diagnostics: [] },
          products: [{ kind: "runtimeClient", nodes: [] }],
        },
        false,
        "/test/App.vue",
      ),
    ).toThrow(/published no runtime Main node/);
  });
});

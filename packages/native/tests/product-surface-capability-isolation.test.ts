/**
 * Public-route isolation: projection, metadata and lint must answer for a
 * file whose runtime route cannot emit a construct. Runtime compilation of
 * that construct still fails with its existing typed reason.
 *
 * Native `ensureIdeCompiled` / `getIde` take no compile shape and start from
 * the host-default bundler profile; leftover runtime target bits must not
 * demand the runtime emitter.
 */

import { describe, expect, it } from "vitest";

const native = require("../index.js") as typeof import("../index");

type Cell = {
  label: string;
  id: string;
  source: string;
  runtimeCode: string;
  rest: string;
};

const CELLS: Cell[] = [
  {
    label: "nav",
    id: "IsoNav.svelte",
    source: "<script>let count = $state(0);</script>\n<nav>home</nav>\n<p>{count}</p>\n",
    runtimeCode: "svelte-runtime-unsupported-element",
    rest: "count",
  },
  {
    label: "bind-this",
    id: "IsoBindThis.svelte",
    source:
      "<script>let refs = []; let count = $state(0);</script>\n<div bind:this={refs[0]}></div>\n<p>{count}</p>\n",
    runtimeCode: "svelte-runtime-unsupported-binding",
    rest: "count",
  },
];

const CONTROL = {
  id: "IsoControl.svelte",
  source: "<script>let count = $state(0);</script>\n<div>{count}</div>\n",
};

describe("product-surface capability isolation", () => {
  for (const cell of CELLS) {
    it(`${cell.label}: projection, metadata and lint answer; runtime still refuses`, () => {
      const host = new native.VerterHost({ analysisLevel: "full" });
      host.upsert({
        inputId: cell.id,
        source: cell.source,
        fileKind: "svelte",
      });

      expect(() =>
        host.getVirtualFile({
          canonicalId: cell.id,
          nodeKind: { kind: "main" },
        }),
      ).toThrow(new RegExp(cell.runtimeCode));

      expect(host.ensureIdeCompiled(cell.id)).toBe(true);
      const ide = host.getIde(cell.id);
      expect(ide).not.toBeNull();
      expect(ide!.code).toContain("@verter/svelte-jsx");
      expect(ide!.code).toContain(cell.rest);

      const publicApi = host.getPublicApi(cell.id, "public");
      expect(publicApi.error).toBeNull();
      expect(publicApi.value?.code).toBeTruthy();

      const analysis = host.getAnalysis(cell.id);
      expect(analysis).not.toBeNull();
      expect(analysis as string).toContain("count");

      const lint = host.lint(cell.id, null);
      expect(Array.isArray(lint)).toBe(true);
    });
  }

  it("control file without unsupported constructs is unchanged", () => {
    const host = new native.VerterHost({ analysisLevel: "full" });
    host.upsert({
      inputId: CONTROL.id,
      source: CONTROL.source,
      fileKind: "svelte",
    });

    const main = host.getVirtualFile({
      canonicalId: CONTROL.id,
      nodeKind: { kind: "main" },
    });
    expect(main?.code).toContain("svelte/internal/client");

    expect(host.ensureIdeCompiled(CONTROL.id)).toBe(true);
    const ide = host.getIde(CONTROL.id);
    expect(ide).not.toBeNull();
    expect(ide!.code).toContain("count");

    const publicApi = host.getPublicApi(CONTROL.id, "public");
    expect(publicApi.error).toBeNull();
    expect(publicApi.value?.code).toBeTruthy();
    expect(host.lint(CONTROL.id, null)).toEqual(expect.any(Array));
  });

  it("vue shares the admission site: default-profile projection still answers for <nav>", () => {
    const host = new native.VerterHost({ analysisLevel: "full" });
    host.upsert({
      inputId: "IsoNav.vue",
      source:
        "<script setup>\nconst count = 1\n</script>\n<template><nav>{{ count }}</nav><p>{{ count }}</p></template>\n",
      fileKind: "vue",
    });

    const main = host.getVirtualFile({
      canonicalId: "IsoNav.vue",
      nodeKind: { kind: "main" },
    });
    expect(main?.code).toBeTruthy();

    expect(host.ensureIdeCompiled("IsoNav.vue")).toBe(true);
    const ide = host.getIde("IsoNav.vue");
    expect(ide).not.toBeNull();
    expect(ide!.code).toContain("count");

    const publicApi = host.getPublicApi("IsoNav.vue", "public");
    expect(publicApi.error).toBeNull();
    expect(host.lint("IsoNav.vue", null)).toEqual(expect.any(Array));
  });
});

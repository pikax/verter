import {
  existsSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
  writeFileSync,
  mkdirSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import { requireAnchor, type AnchorMap } from "../src/anchors.js";
import type {
  MaterializeResult,
  MaterializeWireRequest,
} from "../src/baseline/materializeClient.js";
import { VENDORED_VUE_VERSION } from "../src/vendorManifest.js";
import {
  createMaterializedWorkspace,
  disposeMaterializedWorkspace,
} from "../src/materializedWorkspace.js";

const tmps: string[] = [];
afterEach(() => {
  for (const d of tmps.splice(0)) rmSync(d, { recursive: true, force: true });
});

function fixtureDir(files: Record<string, string>): string {
  const dir = mkdtempSync(join(tmpdir(), "dx-fixture-"));
  tmps.push(dir);
  for (const [rel, content] of Object.entries(files)) {
    const abs = join(dir, rel);
    mkdirSync(join(abs, ".."), { recursive: true });
    writeFileSync(abs, content);
  }
  return dir;
}

/** A capturing fake C materialize runner — returns a canned report. */
function fakeMaterialize(sink: {
  req?: MaterializeWireRequest;
}): (req: MaterializeWireRequest) => Promise<MaterializeResult> {
  return async (req) => {
    sink.req = req;
    return {
      ideArtifacts: [
        {
          sourceVue: `${req.workspaceRoot}/A.vue`,
          generatedPath: `${req.workspaceRoot}/A.vue.tsx`,
          sourceMapPresent: true,
          sourceMap: "C-AUTHORITATIVE-MAP",
        },
      ],
      publicApiTwins: [],
      verterTypesDts: null,
      mapAbsent: [],
      sourceMapIdentities: {},
      compileErrors: [],
      tsconfigPath: `${req.workspaceRoot}/tsconfig.json`,
      synthesizedTsconfig: true,
      supportRewrites: [],
      vueVersionWarnings: [],
    };
  };
}

describe("createMaterializedWorkspace", () => {
  it("copies + strips fixtures, merges anchors, and assembles the immutable DTO", async () => {
    const dir = fixtureDir({
      "A.vue":
        '<script setup lang="ts">\n// @dx-anchor decl\nconst x = 1\n</script>\n<template><!-- @dx-anchor use -->{{ x }}</template>\n',
      "barrel.ts": "export { default as A } from './A.vue'\n",
    });
    const sink: { req?: MaterializeWireRequest } = {};
    const ws = await createMaterializedWorkspace({
      fixtureDir: dir,
      repoRoot: "D:/wt/dx-harness",
      typeProvider: "tsgo",
      materialize: fakeMaterialize(sink),
    });
    tmps.push(ws.root);

    // The stripped source is on disk with NO anchor comments left.
    const onDisk = readFileSync(join(ws.root, "A.vue"), "utf-8");
    expect(onDisk).not.toContain("@dx-anchor");
    expect(onDisk).toContain("const x = 1");

    // Source files are relative and cover both the .vue and the support file.
    expect([...ws.sourceFiles].sort()).toEqual(["A.vue", "barrel.ts"]);

    // Anchors merged with their owning file + recomputed positions. The script
    // line-comment anchor occupied its own line (now empty) → column 0; the
    // template HTML-comment anchor lands where `{{ x }}` begins, after "<template>".
    // `MaterializedWorkspace` publishes `anchorMap` as a frozen `ReadonlyMap`; the
    // `requireAnchor` probe helper narrows to a mutable `AnchorMap`, so build a
    // mutable probe view over the same (still-frozen) `Anchor` entries to look up.
    const anchors: AnchorMap = new Map(ws.anchorMap);
    expect(requireAnchor(anchors, "decl").file).toBe("A.vue");
    expect(requireAnchor(anchors, "use").file).toBe("A.vue");
    expect(requireAnchor(anchors, "decl")).toMatchObject({ line: 1, character: 0 });
    expect(requireAnchor(anchors, "use")).toMatchObject({ line: 4, character: 10 });

    // Every anchorMap entry carries explicit column-encoding metadata so a raw-LSP
    // / extension consumer reads the unit (UTF-16) off the DTO, not a doc comment.
    expect(ws.anchorMap.size).toBeGreaterThan(0);
    for (const [, anchor] of ws.anchorMap) {
      expect(anchor.encoding).toBe("utf-16");
      expect(anchor.encoding).not.toBeUndefined();
    }

    // Tool roots + vendor manifest computed once.
    expect(ws.toolRoots.repoRoot).toBe("d:/wt/dx-harness");
    expect(ws.vendor.expectedVueVersion).toBe(VENDORED_VUE_VERSION);
    expect(ws.vendor.manifest.files.length).toBeGreaterThan(0);

    // The materialize request carried the computed inputs.
    expect(sink.req?.workspaceRoot).toBe(ws.root);
    expect(sink.req?.entries).toEqual([`${ws.root}/A.vue`]);
    expect(sink.req?.vendorNodeModules).toBe(ws.vendor.shimsDir);
    expect(sink.req?.expectedVueVersion).toBe(VENDORED_VUE_VERSION);

    // The workspace settings file was written under the root.
    expect(existsSync(ws.workspaceSettings.settingsPath)).toBe(true);

    disposeMaterializedWorkspace(ws);
  });

  it("defaults the materialize request to strict vue-version sync; a caller can opt out", async () => {
    const dir = fixtureDir({ "A.vue": "<template><div/></template>\n" });
    const strictSink: { req?: MaterializeWireRequest } = {};
    const wsStrict = await createMaterializedWorkspace({
      fixtureDir: dir,
      repoRoot: "/repo",
      materialize: fakeMaterialize(strictSink),
    });
    tmps.push(wsStrict.root);
    // Default: strict vendored-Vue sync — the B↔C contract hard-fails on drift.
    expect(strictSink.req?.strictVueVersion).toBe(true);
    disposeMaterializedWorkspace(wsStrict);

    const laxSink: { req?: MaterializeWireRequest } = {};
    const wsLax = await createMaterializedWorkspace({
      fixtureDir: dir,
      repoRoot: "/repo",
      strictVueVersion: false,
      materialize: fakeMaterialize(laxSink),
    });
    tmps.push(wsLax.root);
    expect(laxSink.req?.strictVueVersion).toBe(false);
    disposeMaterializedWorkspace(wsLax);
  });

  it("reads C's already-shifted source map as authoritative (never recomputes it)", async () => {
    const dir = fixtureDir({ "A.vue": "<template><div/></template>\n" });
    const sink: { req?: MaterializeWireRequest } = {};
    const ws = await createMaterializedWorkspace({
      fixtureDir: dir,
      repoRoot: "/repo",
      materialize: fakeMaterialize(sink),
    });
    tmps.push(ws.root);
    // The DTO carries C's map verbatim — B does not re-shift or recompute it.
    expect(ws.materializeReport.ideArtifacts[0].sourceMap).toBe("C-AUTHORITATIVE-MAP");
    disposeMaterializedWorkspace(ws);
  });

  it("throws on a duplicate anchor name across fixture files", async () => {
    const dir = fixtureDir({
      "A.vue": "<template><!-- @dx-anchor same -->{{ a }}</template>\n",
      "B.vue": "<template><!-- @dx-anchor same -->{{ b }}</template>\n",
    });
    await expect(
      createMaterializedWorkspace({
        fixtureDir: dir,
        repoRoot: "/repo",
        materialize: fakeMaterialize({}),
      }),
    ).rejects.toThrow(/same/);
  });

  it("produces a frozen DTO", async () => {
    const dir = fixtureDir({ "A.vue": "<template><div/></template>\n" });
    const ws = await createMaterializedWorkspace({
      fixtureDir: dir,
      repoRoot: "/repo",
      materialize: fakeMaterialize({}),
    });
    tmps.push(ws.root);
    expect(Object.isFrozen(ws)).toBe(true);
    expect(() => {
      // @ts-expect-error — the DTO is immutable.
      ws.root = "/elsewhere";
    }).toThrow();
    disposeMaterializedWorkspace(ws);
  });

  it("deep-freezes the immutable scaffold — nested fields cannot be mutated", async () => {
    const dir = fixtureDir({
      "A.vue": "<template><!-- @dx-anchor a -->{{ x }}</template>\n",
    });
    const ws = await createMaterializedWorkspace({
      fixtureDir: dir,
      repoRoot: "/repo",
      typeProvider: "tsgo",
      materialize: fakeMaterialize({}),
    });
    tmps.push(ws.root);

    // The scaffold is frozen all the way down, not just at the top-level DTO.
    expect(Object.isFrozen(ws.toolRoots)).toBe(true);
    expect(Object.isFrozen(ws.workspaceSettings)).toBe(true);
    expect(Object.isFrozen(ws.workspaceSettings.settings)).toBe(true);
    expect(Object.isFrozen(ws.workspaceSettings.env)).toBe(true);
    expect(Object.isFrozen(ws.vendor)).toBe(true);
    expect(Object.isFrozen(ws.vendor.manifest)).toBe(true);
    expect(Object.isFrozen(ws.vendor.manifest.files)).toBe(true);
    expect(Object.isFrozen(ws.materializeReport)).toBe(true);
    expect(Object.isFrozen(ws.materializeReport.ideArtifacts)).toBe(true);
    expect(Object.isFrozen(ws.sourceFiles)).toBe(true);

    // Mutating a NESTED field throws (ES-module strict mode), not just the top.
    expect(() => {
      (ws.toolRoots as { repoRoot: string }).repoRoot = "x";
    }).toThrow();
    expect(() => {
      (ws.workspaceSettings.settings as Record<string, unknown>)["verter.typeProvider"] =
        "tsserver";
    }).toThrow();
    expect(() => {
      (ws.workspaceSettings.env as Record<string, string>).EXTRA = "y";
    }).toThrow();
    expect(() => {
      (ws.vendor.manifest as { vueVersion: string }).vueVersion = "9.9.9";
    }).toThrow();
    expect(() => {
      ws.materializeReport.ideArtifacts.push({} as never);
    }).toThrow();

    // The anchorMap is genuinely read-only: entries cannot be mutated, and the
    // map cannot gain or lose entries.
    // Same mutable probe view as above — entries stay the frozen `Anchor`s, so the
    // immutability assertions below still hold on the looked-up entry.
    const anchors: AnchorMap = new Map(ws.anchorMap);
    const a = requireAnchor(anchors, "a");
    expect(Object.isFrozen(a)).toBe(true);
    expect(() => {
      (a as { line: number }).line = 99;
    }).toThrow();
    // The encoding metadata is part of the frozen DTO too — it cannot be mutated.
    expect(() => {
      (a as { encoding: string }).encoding = "utf-8";
    }).toThrow();
    expect(() => (ws.anchorMap as Map<string, typeof a>).set("b", a)).toThrow();
    expect(() => (ws.anchorMap as Map<string, typeof a>).delete("a")).toThrow();
    expect(() => (ws.anchorMap as Map<string, typeof a>).clear()).toThrow();
    // The entry and lookups still read back correctly — incl. the column encoding.
    expect(requireAnchor(anchors, "a")).toEqual({
      file: "A.vue",
      line: 0,
      character: 10,
      encoding: "utf-16",
    });

    disposeMaterializedWorkspace(ws);
  });

  it("exposes a named tsconfigSet reflecting a synthesized fallback from C's report", async () => {
    const dir = fixtureDir({ "A.vue": "<template><div/></template>\n" });
    // fakeMaterialize reports synthesizedTsconfig:true + tsconfigPath=<root>/tsconfig.json.
    const ws = await createMaterializedWorkspace({
      fixtureDir: dir,
      repoRoot: "/repo",
      materialize: fakeMaterialize({}),
    });
    tmps.push(ws.root);

    // The named handoff exists — downstream never has to scrape the C report shape.
    expect(ws.tsconfigSet).not.toBeUndefined();
    expect(ws.tsconfigSet.tsconfigPath).toBe(`${ws.root}/tsconfig.json`);
    expect(ws.tsconfigSet.synthesized).toBe(true);
    expect(ws.tsconfigSet.synthesizedConfigPath).toBe(`${ws.root}/tsconfig.json`);
    // A synthesized fallback means no copied/resolved project config.
    expect(ws.tsconfigSet.projectConfigPath).toBeNull();
    // It is derived from C's report, not a re-computation.
    expect(ws.tsconfigSet.tsconfigPath).toBe(ws.materializeReport.tsconfigPath);
    expect(ws.tsconfigSet.synthesized).toBe(ws.materializeReport.synthesizedTsconfig);

    // It rides the deep-freeze.
    expect(Object.isFrozen(ws.tsconfigSet)).toBe(true);
    expect(() => {
      (ws.tsconfigSet as { synthesized: boolean }).synthesized = false;
    }).toThrow();

    disposeMaterializedWorkspace(ws);
  });

  it("reflects a copied/resolved project config in tsconfigSet (not synthesized)", async () => {
    const dir = fixtureDir({ "A.vue": "<template><div/></template>\n" });
    const ws = await createMaterializedWorkspace({
      fixtureDir: dir,
      repoRoot: "/repo",
      materialize: async (req) => ({
        ideArtifacts: [],
        publicApiTwins: [],
        verterTypesDts: null,
        mapAbsent: [],
        sourceMapIdentities: {},
        compileErrors: [],
        tsconfigPath: `${req.workspaceRoot}/tsconfig.app.json`,
        synthesizedTsconfig: false,
        supportRewrites: [],
        vueVersionWarnings: [],
      }),
    });
    tmps.push(ws.root);

    expect(ws.tsconfigSet.synthesized).toBe(false);
    expect(ws.tsconfigSet.tsconfigPath).toBe(`${ws.root}/tsconfig.app.json`);
    // A real copied/resolved config is surfaced; there is no synthesized fallback.
    expect(ws.tsconfigSet.projectConfigPath).toBe(`${ws.root}/tsconfig.app.json`);
    expect(ws.tsconfigSet.synthesizedConfigPath).toBeNull();

    disposeMaterializedWorkspace(ws);
  });

  it("removes the temp root when materialization fails (no temp-dir leak)", async () => {
    const parent = mkdtempSync(join(tmpdir(), "dx-ws-parent-"));
    tmps.push(parent);
    const dir = fixtureDir({ "A.vue": "<template><div/></template>\n" });
    await expect(
      createMaterializedWorkspace({
        fixtureDir: dir,
        repoRoot: "/repo",
        tmpRootParent: parent,
        materialize: async () => {
          throw new Error("boom-materialize");
        },
      }),
    ).rejects.toThrow(/boom-materialize/);
    // The temp workspace root created under `parent` was cleaned up on failure —
    // a successful return is the only path that keeps the temp root.
    expect(readdirSync(parent)).toEqual([]);
  });

  it("removes the temp root when fixture copy fails (duplicate anchor)", async () => {
    const parent = mkdtempSync(join(tmpdir(), "dx-ws-parent-"));
    tmps.push(parent);
    const dir = fixtureDir({
      "A.vue": "<template><!-- @dx-anchor dup -->{{ a }}</template>\n",
      "B.vue": "<template><!-- @dx-anchor dup -->{{ b }}</template>\n",
    });
    await expect(
      createMaterializedWorkspace({
        fixtureDir: dir,
        repoRoot: "/repo",
        tmpRootParent: parent,
        materialize: fakeMaterialize({}),
      }),
    ).rejects.toThrow(/dup/);
    expect(readdirSync(parent)).toEqual([]);
  });

  it("keeps the temp root on a successful create until dispose", async () => {
    const parent = mkdtempSync(join(tmpdir(), "dx-ws-parent-"));
    tmps.push(parent);
    const dir = fixtureDir({ "A.vue": "<template><div/></template>\n" });
    const ws = await createMaterializedWorkspace({
      fixtureDir: dir,
      repoRoot: "/repo",
      tmpRootParent: parent,
      materialize: fakeMaterialize({}),
    });
    // Success keeps the temp root: it exists and is the single child of `parent`.
    expect(existsSync(ws.root)).toBe(true);
    expect(readdirSync(parent).length).toBe(1);
    disposeMaterializedWorkspace(ws);
    expect(existsSync(ws.root)).toBe(false);
  });

  it("disposeMaterializedWorkspace removes the temp root", async () => {
    const dir = fixtureDir({ "A.vue": "<template><div/></template>\n" });
    const ws = await createMaterializedWorkspace({
      fixtureDir: dir,
      repoRoot: "/repo",
      materialize: fakeMaterialize({}),
    });
    expect(existsSync(ws.root)).toBe(true);
    disposeMaterializedWorkspace(ws);
    expect(existsSync(ws.root)).toBe(false);
  });
});

import {
  existsSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
  writeFileSync,
  mkdirSync,
  symlinkSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import { requireAnchor, type AnchorMap } from "../src/anchors.js";
import { joinCanonical } from "../src/paths.js";
import type {
  MaterializeResult,
  MaterializeWireRequest,
} from "../src/baseline/materializeClient.js";
import { VENDORED_VUE_VERSION } from "../src/vendorManifest.js";
import {
  createMaterializedWorkspace,
  disposeMaterializedWorkspace,
  pruneBaselineGeneratedArtifacts,
  type MaterializedWorkspace,
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

describe("pruneBaselineGeneratedArtifacts", () => {
  /**
   * A report naming an IDE entry, a public-API twin, an already-absent artifact,
   * and one OUTSIDE the workspace root — the four cases the prune must separate.
   */
  function reportWithArtifacts(root: string, escaping: boolean): MaterializeResult {
    return {
      ideArtifacts: [
        {
          sourceVue: `${root}/A.vue`,
          generatedPath: `${root}/A.vue.tsx`,
          sourceMapPresent: true,
        },
        {
          sourceVue: `${root}/Gone.vue`,
          generatedPath: `${root}/Gone.vue.tsx`,
          sourceMapPresent: false,
        },
        ...(escaping
          ? [
              {
                sourceVue: `${root}/A.vue`,
                generatedPath: "/elsewhere/Escaped.vue.tsx",
                sourceMapPresent: false,
              },
            ]
          : []),
      ],
      publicApiTwins: [
        {
          sourceVue: `${root}/A.vue`,
          generatedPath: `${root}/A.vue.ts`,
          sourceMapPresent: false,
        },
      ],
      verterTypesDts: null,
      mapAbsent: [],
      sourceMapIdentities: {},
      compileErrors: [],
      tsconfigPath: `${root}/tsconfig.json`,
      synthesizedTsconfig: true,
      supportRewrites: [],
      vueVersionWarnings: [],
    };
  }

  it("removes every in-root generated companion and reports exactly what it removed", async () => {
    const dir = fixtureDir({ "A.vue": "<template><div/></template>\n" });
    const ws = await createMaterializedWorkspace({
      fixtureDir: dir,
      repoRoot: "/repo",
      materialize: async (req) => reportWithArtifacts(req.workspaceRoot, false),
    });
    tmps.push(ws.root);

    // The fake runner reports artifacts it did not write; the on-disk companions
    // the real baseline emits are staged here so the prune has real files to
    // remove and the assertions can distinguish "removed" from "never existed".
    // `tsconfig.json` stands in for C's emitted project config.
    writeFileSync(join(ws.root, "A.vue.tsx"), "// ide entry\n");
    writeFileSync(join(ws.root, "A.vue.ts"), "// api twin\n");
    writeFileSync(join(ws.root, "tsconfig.json"), "{}\n");

    const removed = pruneBaselineGeneratedArtifacts(ws);

    // Both on-disk companions are gone, and both are named in the return value.
    expect(existsSync(join(ws.root, "A.vue.tsx"))).toBe(false);
    expect(existsSync(join(ws.root, "A.vue.ts"))).toBe(false);
    // Compare canonical paths so the assertion holds on Windows, where `join`
    // yields backslashes and the prune reports canonical forward-slash ids.
    expect([...removed].sort()).toEqual(
      [joinCanonical(ws.root, "A.vue.tsx"), joinCanonical(ws.root, "A.vue.ts")].sort(),
    );

    // An artifact the report names but disk does not hold is NOT reported as
    // removed — the return value is evidence of real deletions, not of intent.
    expect(removed).not.toContain(joinCanonical(ws.root, "Gone.vue.tsx"));

    // The authored carrier and the project config survive: the prune removes the
    // baseline's generated layer, never the workspace itself.
    expect(existsSync(join(ws.root, "A.vue"))).toBe(true);
    expect(existsSync(join(ws.root, "tsconfig.json"))).toBe(true);
  });

  it("refuses a `..`-escaping artifact path that a prefix check alone would admit", async () => {
    const dir = fixtureDir({ "A.vue": "<template><div/></template>\n" });
    const ws = await createMaterializedWorkspace({
      fixtureDir: dir,
      repoRoot: "/repo",
      materialize: async (req) => ({
        ...reportWithArtifacts(req.workspaceRoot, false),
        // Lexically INSIDE the root (it starts with `${root}/`), but `..` walks it
        // back out. `canonicalizePath` normalises separators without resolving dot
        // segments, so a `startsWith` containment check admits this — and `rmSync`
        // then resolves it and deletes a file outside the temp workspace.
        publicApiTwins: [
          {
            sourceVue: `${req.workspaceRoot}/A.vue`,
            generatedPath: `${req.workspaceRoot}/../escaped-victim.vue.ts`,
            sourceMapPresent: false,
          },
        ],
      }),
    });
    tmps.push(ws.root);

    // A real file at the escaped location: if containment is lexical, the prune
    // deletes it. Staged as a sibling of the temp root, exactly where the `..`
    // lands, so the assertion observes a real deletion rather than a path string.
    const victim = join(ws.root, "..", "escaped-victim.vue.ts");
    writeFileSync(victim, "// a file the harness must never delete\n");
    try {
      expect(() => pruneBaselineGeneratedArtifacts(ws)).toThrow(/outside the workspace root/);
      expect(existsSync(victim), "a `..`-escaping artifact path must never be deleted").toBe(true);
    } finally {
      rmSync(victim, { force: true });
    }
  });

  it("refuses a path that escapes through an intermediate directory SYMLINK", async () => {
    const dir = fixtureDir({ "A.vue": "<template><div/></template>\n" });
    // The victim lives entirely outside the workspace; the workspace merely holds a
    // symlink pointing at its directory. This is the shape pnpm produces all over
    // this repo (`node_modules/.pnpm/node_modules/@verter/typescript-plugin` →
    // `packages/typescript-plugin`), so it is a real layout, not a contrived one.
    const outside = mkdtempSync(join(tmpdir(), "dx-outside-"));
    tmps.push(outside);
    writeFileSync(join(outside, "victim.vue.ts"), "// must never be deleted\n");

    const ws = await createMaterializedWorkspace({
      fixtureDir: dir,
      repoRoot: "/repo",
      materialize: async (req) => ({
        ...reportWithArtifacts(req.workspaceRoot, false),
        publicApiTwins: [
          {
            sourceVue: `${req.workspaceRoot}/A.vue`,
            // Lexically under the root even after `path.resolve` — no `..` at all —
            // but `link` is a symlink OUT of the workspace, so the real file is not.
            generatedPath: `${req.workspaceRoot}/link/victim.vue.ts`,
            sourceMapPresent: false,
          },
        ],
      }),
    });
    tmps.push(ws.root);
    symlinkSync(outside, join(ws.root, "link"), "dir");

    try {
      expect(() => pruneBaselineGeneratedArtifacts(ws)).toThrow(/outside the workspace root/);
      expect(
        existsSync(join(outside, "victim.vue.ts")),
        "a path escaping through a symlinked directory must never be deleted",
      ).toBe(true);
    } finally {
      rmSync(join(ws.root, "link"), { force: true });
    }
  });

  it("accepts a workspace root reached by a symlinked spelling", async () => {
    // The other direction: realpathing must not turn a legitimate prune into a
    // false refusal when the caller's root spelling differs from the real path
    // (macOS `/tmp` → `/private/tmp` is exactly this).
    const dir = fixtureDir({ "A.vue": "<template><div/></template>\n" });
    const ws = await createMaterializedWorkspace({
      fixtureDir: dir,
      repoRoot: "/repo",
      materialize: async (req) => reportWithArtifacts(req.workspaceRoot, false),
    });
    tmps.push(ws.root);
    writeFileSync(join(ws.root, "A.vue.tsx"), "// ide entry\n");
    writeFileSync(join(ws.root, "A.vue.ts"), "// api twin\n");

    // Reach the SAME workspace through a symlinked alias and report artifacts by
    // that spelling. Both sides realpath to one physical path, so this is contained.
    const aliasParent = mkdtempSync(join(tmpdir(), "dx-alias-"));
    tmps.push(aliasParent);
    const alias = join(aliasParent, "ws");
    symlinkSync(ws.root, alias, "dir");
    const aliased = {
      ...ws,
      materializeReport: {
        ...ws.materializeReport,
        ideArtifacts: [
          {
            sourceVue: `${alias}/A.vue`,
            generatedPath: `${alias}/A.vue.tsx`,
            sourceMapPresent: true,
          },
        ],
        publicApiTwins: [
          {
            sourceVue: `${alias}/A.vue`,
            generatedPath: `${alias}/A.vue.ts`,
            sourceMapPresent: false,
          },
        ],
      },
    } as MaterializedWorkspace;

    expect(pruneBaselineGeneratedArtifacts(aliased)).toHaveLength(2);
    expect(existsSync(join(ws.root, "A.vue.tsx"))).toBe(false);
    expect(existsSync(join(ws.root, "A.vue.ts"))).toBe(false);
  });

  it("unlinks a symlinked artifact without deleting the file it points at", async () => {
    const dir = fixtureDir({ "A.vue": "<template><div/></template>\n" });
    const ws = await createMaterializedWorkspace({
      fixtureDir: dir,
      repoRoot: "/repo",
      materialize: async (req) => ({
        ...reportWithArtifacts(req.workspaceRoot, false),
        publicApiTwins: [],
      }),
    });
    tmps.push(ws.root);

    // The reported artifact is a symlink to the AUTHORED source. Resolving the final
    // segment before deleting would unlink `A.vue` — destroying the fixture rather
    // than the generated layer. `rmSync` unlinks the link itself, so the target must
    // survive.
    symlinkSync(join(ws.root, "A.vue"), join(ws.root, "A.vue.tsx"), "file");
    expect(existsSync(join(ws.root, "A.vue"))).toBe(true);

    const removed = pruneBaselineGeneratedArtifacts(ws);

    expect(removed).toHaveLength(1);
    expect(existsSync(join(ws.root, "A.vue.tsx")), "the symlink itself is removed").toBe(false);
    expect(
      existsSync(join(ws.root, "A.vue")),
      "the authored file the symlink pointed at must survive",
    ).toBe(true);
  });

  it("accepts a NESTED not-yet-written artifact under a symlink-spelled root", async () => {
    // `reportWithArtifacts` names `Gone.vue.tsx`, which C never wrote. Put it under a
    // directory that does not exist either, and reach the root through a symlinked
    // spelling — the macOS `/tmp` → `/private/tmp` shape. Realpathing only the
    // immediate parent leaves the unresolved ancestors comparing unequal and refuses
    // a perfectly contained path.
    const dir = fixtureDir({ "A.vue": "<template><div/></template>\n" });
    const ws = await createMaterializedWorkspace({
      fixtureDir: dir,
      repoRoot: "/repo",
      materialize: async (req) => ({
        ...reportWithArtifacts(req.workspaceRoot, false),
        ideArtifacts: [
          {
            sourceVue: `${req.workspaceRoot}/A.vue`,
            generatedPath: `${req.workspaceRoot}/never/created/deeply/Gone.vue.tsx`,
            sourceMapPresent: false,
          },
        ],
        publicApiTwins: [],
      }),
    });
    tmps.push(ws.root);

    const aliasParent = mkdtempSync(join(tmpdir(), "dx-alias2-"));
    tmps.push(aliasParent);
    const alias = join(aliasParent, "ws");
    symlinkSync(ws.root, alias, "dir");
    const aliased = {
      ...ws,
      root: alias,
      materializeReport: {
        ...ws.materializeReport,
        ideArtifacts: [
          {
            sourceVue: `${alias}/A.vue`,
            generatedPath: `${alias}/never/created/deeply/Gone.vue.tsx`,
            sourceMapPresent: false,
          },
        ],
      },
    } as MaterializedWorkspace;

    // Contained, simply absent: no throw, and nothing reported as removed.
    expect(pruneBaselineGeneratedArtifacts(aliased)).toEqual([]);
  });

  it("refuses an artifact path outside the workspace root", async () => {
    const dir = fixtureDir({ "A.vue": "<template><div/></template>\n" });
    const ws = await createMaterializedWorkspace({
      fixtureDir: dir,
      repoRoot: "/repo",
      materialize: async (req) => reportWithArtifacts(req.workspaceRoot, true),
    });
    tmps.push(ws.root);

    // `/elsewhere/Escaped.vue.tsx` is named by the report but lives outside the
    // temp root. A prune that deletes it would delete arbitrary user files, so it
    // must be rejected loudly rather than skipped in silence.
    expect(() => pruneBaselineGeneratedArtifacts(ws)).toThrow(/outside the workspace root/);
  });
});

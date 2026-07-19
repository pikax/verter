import { describe, it, expect, afterEach } from "vitest";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { VerterHost } from "@verter/native";

import {
  CarrierPublicApiProjectionFailure,
  runBatchTypecheck,
  commonAncestorDir,
  isInsideDir,
  type CarrierSource,
  type CarrierCodegenHost,
  type CarrierPublicApiProjectionError,
} from "./mirrorHost";
import {
  createSingleProjectFixture,
  createReferenceFixture,
  snapshotTree,
  diffTrees,
  type Fixture,
} from "./testFixtures";

// The driver consumes the real `@verter/native` carrier-codegen authority. A
// fresh host per call keeps tests isolated.
function host(): InstanceType<typeof VerterHost> {
  return new VerterHost();
}

const fixtures: Fixture[] = [];
afterEach(() => {
  for (const f of fixtures) f.cleanup();
  fixtures.length = 0;
});
function track<T extends Fixture>(f: T): T {
  fixtures.push(f);
  return f;
}

/** Read a fixture source into a `CarrierSource` (leaf IDE carrier). */
function ideCarrier(root: string, rel: string, framework: "vue" | "svelte"): CarrierSource {
  const sourcePath = path.join(root, rel);
  return {
    sourcePath,
    source: fs.readFileSync(sourcePath, "utf8"),
    framework,
    ownership: "Owned",
    role: "ide",
  };
}

describe("commonAncestorDir (mirror base is a DIRECTORY)", () => {
  it("a SINGLETON file input yields that file's DIRECTORY, not the file path", () => {
    // The bug: a single file path returned the FILE as the ancestor, so the
    // mirror re-rooted relative to a file and the generated tsconfig could land
    // outside the mirror. The directory is the correct base. (`commonAncestorDir`
    // runs `path.resolve` on each input, so the expectation is the resolved
    // dirname — drive-agnostic across Windows / POSIX test hosts.)
    const singleton = path.join(os.tmpdir(), "proj", "tsconfig.json");
    expect(commonAncestorDir([singleton])).toBe(
      path.dirname(path.resolve(singleton)).replace(/\\/g, "/"),
    );
  });

  it("multiple files in the same dir collapse to that dir", () => {
    expect(commonAncestorDir(["d:/ws/src/A.vue", "d:/ws/src/B.vue", "d:/ws/tsconfig.json"])).toBe(
      "d:/ws",
    );
  });

  it("files in sibling subtrees yield the shared parent dir", () => {
    expect(commonAncestorDir(["d:/ws/a/src/A.vue", "d:/ws/b/src/B.vue"])).toBe("d:/ws");
  });
});

describe("isInsideDir (segment-aware containment, not startsWith)", () => {
  it("a path inside the dir (or the dir itself) is inside", () => {
    expect(isInsideDir("d:/mirror", "d:/mirror")).toBe(true);
    expect(isInsideDir("d:/mirror", "d:/mirror/sub/file.tsx")).toBe(true);
  });

  it("a SIBLING sharing the name prefix is NOT inside (the startsWith bug)", () => {
    // `d:/mirror2` shares the textual prefix `d:/mirror` but is a SIBLING — a raw
    // startsWith would wrongly accept it; the segment-aware check rejects it.
    expect(isInsideDir("d:/mirror", "d:/mirror2/file.tsx")).toBe(false);
    expect(isInsideDir("d:/mirror", "d:/mirror-extra")).toBe(false);
  });

  it("an outside / ancestor path is NOT inside", () => {
    expect(isInsideDir("d:/mirror", "d:/other/file.tsx")).toBe(false);
    expect(isInsideDir("d:/mirror/sub", "d:/mirror")).toBe(false);
  });
});

describe("runBatchTypecheck — single project (noEmit diagnostics-only)", () => {
  it("records a projection failure per source and continues later siblings", () => {
    const fx = track(
      createSingleProjectFixture([
        {
          rel: "src/Ordinal.vue",
          content: `<script setup lang="ts">\nconst value = 1\n</script>`,
        },
        {
          rel: "src/Later.vue",
          content: `<script setup lang="ts">\nconst value = 2\n</script>`,
        },
      ]),
    );
    const native = host();
    const projectionError: CarrierPublicApiProjectionError = {
      code: "tsc-generation",
      detailCode: "invalid-authored-member-ordinal",
      subject: { kind: "macro", syntaxIndex: 9 },
      declarationShapeReason: null,
      memberOrdinal: 3,
      outcomeKind: null,
      outcomeReason: null,
      outcomeDiagnostic: null,
    };
    const failingHost: CarrierCodegenHost = {
      upsert: native.upsert.bind(native),
      ensureIdeCompiled: native.ensureIdeCompiled.bind(native),
      getIde: native.getIde.bind(native),
      getPublicApi: (canonicalId, mode) =>
        canonicalId === "Ordinal.vue"
          ? { value: null, error: projectionError }
          : native.getPublicApi(canonicalId, mode),
      close: native.close.bind(native),
    };

    try {
      const ordinal = path.join(fx.root, "src/Ordinal.vue").replace(/\\/g, "/");
      const later = path.join(fx.root, "src/Later.vue").replace(/\\/g, "/");
      const result = runBatchTypecheck({
        tsconfigPath: fx.tsconfigPath,
        carrierSources: [
          ideCarrier(fx.root, "src/Ordinal.vue", "vue"),
          ideCarrier(fx.root, "src/Later.vue", "vue"),
        ],
        host: failingHost,
      });

      expect(result.sourceOutcomes.get(ordinal)).toEqual({
        kind: "projectionFailure",
        error: projectionError,
      });
      expect(result.materializedCarriers.has(ordinal)).toBe(false);
      expect(result.sourceOutcomes.get(later)).toMatchObject({ kind: "materialized" });
      expect(result.materializedCarriers.has(later)).toBe(true);
    } finally {
      failingHost.close?.();
    }
  });

  it("preserves all unavailable-outcome arms on the thrown error", () => {
    const cases: CarrierPublicApiProjectionError[] = [
      {
        code: "tsc-generation",
        detailCode: "unavailable-outcome",
        subject: { kind: "macro", syntaxIndex: 0 },
        declarationShapeReason: null,
        memberOrdinal: null,
        outcomeKind: "partial",
        outcomeReason: "incomplete-traversal",
        outcomeDiagnostic: "partial detail",
      },
      {
        code: "tsc-generation",
        detailCode: "unavailable-outcome",
        subject: { kind: "macro", syntaxIndex: 1 },
        declarationShapeReason: null,
        memberOrdinal: null,
        outcomeKind: "unresolved",
        outcomeReason: "ambiguous-reference",
        outcomeDiagnostic: "unresolved detail",
      },
      {
        code: "tsc-generation",
        detailCode: "unavailable-outcome",
        subject: { kind: "macro", syntaxIndex: 2 },
        declarationShapeReason: null,
        memberOrdinal: null,
        outcomeKind: "unsupported",
        outcomeReason: "semantic-construct",
        outcomeDiagnostic: "unsupported detail",
      },
      {
        code: "tsc-generation",
        detailCode: "unavailable-outcome",
        subject: { kind: "macro", syntaxIndex: 3 },
        declarationShapeReason: null,
        memberOrdinal: null,
        outcomeKind: "invalid",
        outcomeReason: "non-object-root",
        outcomeDiagnostic: "invalid detail",
      },
      {
        code: "tsc-generation",
        detailCode: "unavailable-outcome",
        subject: {
          kind: "scriptSetupAttrs",
          sourceRange: { start: 31, end: 37 },
        },
        declarationShapeReason: null,
        memberOrdinal: null,
        outcomeKind: "invalid",
        outcomeReason: "malformed-or-recovered-type-syntax",
        outcomeDiagnostic: null,
      },
    ];

    for (const error of cases) {
      expect(new CarrierPublicApiProjectionFailure(error)).toMatchObject(error);
    }
  });

  it("reports a carrier type error mapped back to the .vue source position", () => {
    const fx = track(
      createSingleProjectFixture([
        {
          rel: "src/Bad.vue",
          content: `<script setup lang="ts">
const count: number = "not a number"
</script>
<template><div>{{ count }}</div></template>`,
        },
      ]),
    );

    const result = runBatchTypecheck({
      tsconfigPath: fx.tsconfigPath,
      carrierSources: [ideCarrier(fx.root, "src/Bad.vue", "vue")],
      host: host(),
    });

    expect(result.buildMode).toBe(false);
    // The assignment type error (TS2322) must surface, mapped back to the .vue.
    const ts2322 = result.diagnostics.filter((d) => d.code === 2322);
    expect(ts2322.length).toBeGreaterThan(0);
    const mapped = ts2322.find((d) => d.mappedFromCarrier);
    expect(mapped).toBeDefined();
    expect(mapped!.fileName).toBe(path.join(fx.root, "src/Bad.vue").replace(/\\/g, "/"));
    // The error sits on source line 2 (the `const count` line) — offset must land
    // inside the source, not in generated helper preamble.
    const src = fs.readFileSync(path.join(fx.root, "src/Bad.vue"), "utf8");
    expect(mapped!.start).toBeGreaterThan(0);
    expect(mapped!.start).toBeLessThan(src.length);
    // The mapped offset falls on the second line.
    const lineOfOffset = src.slice(0, mapped!.start).split("\n").length;
    expect(lineOfOffset).toBe(2);
  });

  it("reports NO type error for a well-typed carrier", () => {
    const fx = track(
      createSingleProjectFixture([
        {
          rel: "src/Good.vue",
          content: `<script setup lang="ts">
const count: number = 42
</script>
<template><div>{{ count }}</div></template>`,
        },
      ]),
    );

    const result = runBatchTypecheck({
      tsconfigPath: fx.tsconfigPath,
      carrierSources: [ideCarrier(fx.root, "src/Good.vue", "vue")],
      host: host(),
    });

    // No 2322 assignment error for the well-typed component.
    expect(result.diagnostics.filter((d) => d.code === 2322)).toHaveLength(0);
  });

  it("EXCLUDES a NoProject/Ambiguous source from the batch (no carrier materialised)", () => {
    const fx = track(
      createSingleProjectFixture([
        {
          rel: "src/Owned.vue",
          content: `<script setup lang="ts">
const x: number = 1
</script>
<template><div>{{ x }}</div></template>`,
        },
        {
          rel: "src/Orphan.vue",
          content: `<script setup lang="ts">
const y: number = "wrong"
</script>
<template><div>{{ y }}</div></template>`,
        },
      ]),
    );

    const orphanPath = path.join(fx.root, "src/Orphan.vue");
    const result = runBatchTypecheck({
      tsconfigPath: fx.tsconfigPath,
      carrierSources: [
        ideCarrier(fx.root, "src/Owned.vue", "vue"),
        {
          sourcePath: orphanPath,
          source: fs.readFileSync(orphanPath, "utf8"),
          framework: "vue",
          ownership: "NoProject", // excluded
          role: "ide",
        },
      ],
      host: host(),
    });

    // The orphan's carrier is never materialised.
    expect(result.materializedCarriers.has(orphanPath.replace(/\\/g, "/"))).toBe(false);
    // No diagnostic maps back to the excluded orphan.
    expect(result.diagnostics.some((d) => d.fileName === orphanPath.replace(/\\/g, "/"))).toBe(
      false,
    );
  });

  it("suppresses generated-only spans (every emitted diagnostic maps to a real source point)", () => {
    const fx = track(
      createSingleProjectFixture([
        {
          rel: "src/Bad.vue",
          content: `<script setup lang="ts">
const count: number = "not a number"
</script>
<template><div>{{ count }}</div></template>`,
        },
      ]),
    );

    const result = runBatchTypecheck({
      tsconfigPath: fx.tsconfigPath,
      carrierSources: [ideCarrier(fx.root, "src/Bad.vue", "vue")],
      host: host(),
    });

    // Every carrier-mapped diagnostic has a concrete source span (no generated-only
    // leak): a mapped diagnostic with a defined start must point into the source.
    for (const d of result.diagnostics) {
      if (d.mappedFromCarrier && d.start !== undefined) {
        const src = fs.readFileSync(d.fileName!, "utf8");
        expect(d.start).toBeLessThanOrEqual(src.length);
      }
    }
  });
});

// ── The CRITICAL zero-working-tree-writes guard ──────────────────────────────
// The mirror-host batch materialises every carrier + emit ONLY under the mirror
// root; the user's checkout is read-only as far as the batch is concerned.
describe("tsc_batch_writes_no_working_tree_files (CRITICAL)", () => {
  it("materializes carriers + emit ONLY under the mirror root; the user tree is byte-unchanged", () => {
    const fx = track(
      createSingleProjectFixture([
        {
          rel: "src/Bad.vue",
          content: `<script setup lang="ts">
const count: number = "not a number"
</script>
<template><div>{{ count }}</div></template>`,
        },
      ]),
    );

    const before = snapshotTree(fx.root);

    const explicitMirror = fs.mkdtempSync(path.join(os.tmpdir(), "verter-explicit-mirror-"));
    try {
      const result = runBatchTypecheck({
        tsconfigPath: fx.tsconfigPath,
        carrierSources: [ideCarrier(fx.root, "src/Bad.vue", "vue")],
        mirrorRoot: explicitMirror,
        host: host(),
      });

      // 1) The user's working tree is byte-IDENTICAL after the run.
      const after = snapshotTree(fx.root);
      const diff = diffTrees(before, after);
      expect(diff.added).toEqual([]);
      expect(diff.modified).toEqual([]);
      expect(diff.removed).toEqual([]);

      // 2) The carrier(s) exist under the MIRROR root, not the source tree.
      expect(result.materializedCarriers.size).toBeGreaterThan(0);
      for (const carrierPath of result.materializedCarriers.values()) {
        const normMirror = path.resolve(explicitMirror).replace(/\\/g, "/");
        expect(path.resolve(carrierPath).replace(/\\/g, "/").startsWith(normMirror)).toBe(true);
        expect(fs.existsSync(carrierPath)).toBe(true);
        // And NOT under the source tree.
        const normRoot = path.resolve(fx.root).replace(/\\/g, "/");
        expect(path.resolve(carrierPath).replace(/\\/g, "/").startsWith(normRoot)).toBe(false);
      }
    } finally {
      fs.rmSync(explicitMirror, { recursive: true, force: true });
    }
  });

  it("build-mode emit (.d.ts/.tsbuildinfo) lands under the mirror, never the user tree", () => {
    const fx = track(
      createReferenceFixture({
        libSources: [
          {
            rel: "src/Button.vue",
            content: `<script setup lang="ts">
defineProps<{ label: string }>()
</script>
<template><button>{{ label }}</button></template>`,
          },
        ],
        appSources: [
          {
            rel: "src/App.vue",
            content: `<script setup lang="ts">
import Button from "@lib/Button.vue"
</script>
<template><Button label="ok" /></template>`,
          },
        ],
        appPaths: { "@lib/*": ["../lib/src/*"] },
      }),
    );

    const before = snapshotTree(fx.root);

    const explicitMirror = fs.mkdtempSync(path.join(os.tmpdir(), "verter-explicit-mirror-ref-"));
    try {
      const libPath = path.join(fx.root, "packages/lib/src/Button.vue");
      const appPath = path.join(fx.root, "packages/app/src/App.vue");
      const result = runBatchTypecheck({
        tsconfigPath: fx.appTsconfigPath,
        carrierSources: [
          // Leaf (under check) gets the IDE carrier.
          {
            sourcePath: appPath,
            source: fs.readFileSync(appPath, "utf8"),
            framework: "vue",
            ownership: "Owned",
            role: "ide",
          },
          // Referenced project's source gets the declaration carrier (.verter.ts),
          // grouped under the LIB's own tsconfig (its own mirror subtree + config).
          {
            sourcePath: libPath,
            source: fs.readFileSync(libPath, "utf8"),
            framework: "vue",
            ownership: "Owned",
            role: "api",
            projectTsconfigPath: fx.libTsconfigPath,
          },
        ],
        mirrorRoot: explicitMirror,
        host: host(),
      });

      expect(result.buildMode).toBe(true);

      // The user tree is byte-identical — no `.d.ts`, no `.tsbuildinfo`, no `dist/`.
      const after = snapshotTree(fx.root);
      const diff = diffTrees(before, after);
      expect(diff.added).toEqual([]);
      expect(diff.modified).toEqual([]);
      expect(diff.removed).toEqual([]);

      // No `dist/` directory was created in the user's lib project.
      expect(fs.existsSync(path.join(fx.root, "packages/lib/dist"))).toBe(false);
    } finally {
      fs.rmSync(explicitMirror, { recursive: true, force: true });
    }
  });

  // The driver ENFORCES the boundary: a caller-supplied mirrorRoot that points
  // AT the source tree is REJECTED (throws) BEFORE any carrier is materialised —
  // the guard lives in the driver, not in a cooperative caller. (This replaces
  // the prior `[mutation]` test, which proved a source-tree mirror would dirty
  // the tree; the driver now refuses that input outright.)
  it("REJECTS a mirrorRoot pointing at the source tree (boundary enforced in the driver)", () => {
    const fx = track(
      createSingleProjectFixture([
        {
          rel: "src/Bad.vue",
          content: `<script setup lang="ts">
const count: number = "x"
</script>
<template><div>{{ count }}</div></template>`,
        },
      ]),
    );

    const before = snapshotTree(fx.root);

    expect(() =>
      runBatchTypecheck({
        tsconfigPath: fx.tsconfigPath,
        carrierSources: [ideCarrier(fx.root, "src/Bad.vue", "vue")],
        mirrorRoot: fx.root, // INSIDE the user tree → rejected
        host: host(),
      }),
    ).toThrow(/mirror root inside the user tree/i);

    // The working tree is byte-IDENTICAL — nothing was materialised before the
    // rejection (the discriminating proof: a non-enforcing driver would have
    // written `src/Bad.vue.tsx`).
    const after = snapshotTree(fx.root);
    const diff = diffTrees(before, after);
    expect(diff.added).toEqual([]);
    expect(diff.modified).toEqual([]);
    expect(diff.removed).toEqual([]);
  });

  it("REJECTS a mirrorRoot in a SUBDIRECTORY of the source tree", () => {
    const fx = track(
      createSingleProjectFixture([
        {
          rel: "src/Bad.vue",
          content: `<script setup lang="ts">
const x: number = 1
</script>
<template><div>{{ x }}</div></template>`,
        },
      ]),
    );
    const before = snapshotTree(fx.root);
    expect(() =>
      runBatchTypecheck({
        tsconfigPath: fx.tsconfigPath,
        carrierSources: [ideCarrier(fx.root, "src/Bad.vue", "vue")],
        mirrorRoot: path.join(fx.root, ".verter-mirror"), // inside the user tree
        host: host(),
      }),
    ).toThrow(/mirror root inside the user tree/i);
    const diff = diffTrees(before, snapshotTree(fx.root));
    expect(diff.added.concat(diff.modified, diff.removed)).toEqual([]);
  });

  it("REJECTS a mirrorRoot that is an ANCESTOR of the source tree", () => {
    const fx = track(
      createSingleProjectFixture([
        {
          rel: "src/X.vue",
          content: `<script setup lang="ts">
const x: number = 1
</script>
<template><div>{{ x }}</div></template>`,
        },
      ]),
    );
    // The fixture root's PARENT contains the user tree, so a mirror there would
    // enclose the checkout — equally forbidden.
    expect(() =>
      runBatchTypecheck({
        tsconfigPath: fx.tsconfigPath,
        carrierSources: [ideCarrier(fx.root, "src/X.vue", "vue")],
        mirrorRoot: path.dirname(fx.root),
        host: host(),
      }),
    ).toThrow(/mirror root inside the user tree/i);
  });

  it("the DEFAULT mirror (no mirrorRoot supplied) is a fresh temp dir, NEVER the user tree", () => {
    const fx = track(
      createSingleProjectFixture([
        {
          rel: "src/Good.vue",
          content: `<script setup lang="ts">
const x: number = 1
</script>
<template><div>{{ x }}</div></template>`,
        },
      ]),
    );
    const before = snapshotTree(fx.root);
    const result = runBatchTypecheck({
      tsconfigPath: fx.tsconfigPath,
      carrierSources: [ideCarrier(fx.root, "src/Good.vue", "vue")],
      // mirrorRoot omitted → auto mkdtemp under os.tmpdir().
      host: host(),
    });
    // The auto mirror is under the OS temp dir and OUTSIDE the user tree.
    const tmp = path.resolve(os.tmpdir()).replace(/\\/g, "/");
    expect(path.resolve(result.mirrorRoot).replace(/\\/g, "/").startsWith(tmp)).toBe(true);
    expect(
      path
        .resolve(result.mirrorRoot)
        .replace(/\\/g, "/")
        .startsWith(path.resolve(fx.root).replace(/\\/g, "/")),
    ).toBe(false);
    // The user tree is byte-unchanged by the default-mirror run.
    const diff = diffTrees(before, snapshotTree(fx.root));
    expect(diff.added.concat(diff.modified, diff.removed)).toEqual([]);
  });
});

/**
 * Demand-driven contract of `hydrateMacroTypeDeps`.
 *
 * Uses a MOCK host (no native binding): the contract under test is which
 * files the hydrator reads/upserts/walks, not the host's type resolution.
 *
 * Pins:
 * - the macro-demand gate (no macroTypeDeps -> zero hydration work);
 * - the per-host memo (a dependency file is read/walked once across
 *   transforms, not once per importing SFC);
 * - watcher eviction re-opening a single entry;
 * - the closure never walking bare npm packages (node_modules-resolved
 *   imports inside hydrated type files stay untouched);
 * - alias-resolved project-local deps hydrating like relative ones.
 */
import { describe, it, expect } from "vitest";
import type { VerterHost, Workspace } from "@verter/native";
import { evictHydratedPath, hydrateMacroTypeDeps } from "./macro-type-hydration";

interface MockFile {
  source: string;
  analysis?: object;
}

function makeWorld(files: Record<string, MockFile>) {
  const reads: string[] = [];
  const upserts: string[] = [];
  const analyses = new Map<string, object>();
  for (const [path, file] of Object.entries(files)) {
    if (file.analysis) analyses.set(path, file.analysis);
  }

  const host = {
    getAnalysis(path: string): string | null {
      const analysis = analyses.get(path);
      return analysis ? JSON.stringify(analysis) : null;
    },
    upsert(entry: { inputId: string; source: string; fileKind?: string }): void {
      upserts.push(entry.inputId);
    },
    setImportDependencies(): void {},
  } as unknown as VerterHost;

  const ws = {
    fileExists: (path: string) => Object.prototype.hasOwnProperty.call(files, path),
    readFile: (path: string) => {
      reads.push(path);
      return files[path]?.source ?? null;
    },
  } as unknown as Workspace;

  return { host, ws, reads, upserts };
}

const ENTRY = "/proj/src/App.vue";

describe("hydrateMacroTypeDeps demand-driven contract", () => {
  it("hydrates NOTHING when the analysis demands no macro type deps", async () => {
    const { host, ws, reads, upserts } = makeWorld({
      [ENTRY]: {
        source: "<template/>",
        analysis: {
          // Imports exist but nothing demands macro type resolution.
          imports: [{ source: "./helpers" }, { source: "lodash" }],
          exportSignatures: [{ name: "x", isType: false, reexportSource: "./barrel" }],
          macroTypeDeps: [],
        },
      },
      "/proj/src/helpers.ts": { source: "export const h = 1;" },
    });

    await hydrateMacroTypeDeps(host, ENTRY, undefined, ws);

    expect(upserts).toEqual([]);
    expect(reads).toEqual([]);
  });

  it("hydrates a resolved macro type dep and walks its relative closure once", async () => {
    const resolveId = async (source: string) =>
      source === "./types"
        ? "/proj/src/types.d.ts"
        : source === "./more"
          ? "/proj/src/more.d.ts"
          : null;
    const world = {
      [ENTRY]: {
        source: "<template/>",
        analysis: {
          macroTypeDeps: [{ typeName: "Props", importSource: "./types", macroKind: "defineProps" }],
        },
      },
      "/proj/src/types.d.ts": {
        source: "import type { More } from './more';\nexport interface Props { m: More }",
        analysis: { imports: [{ source: "./more" }] },
      },
      "/proj/src/more.d.ts": { source: "export interface More { x: number }", analysis: {} },
    };
    const { host, ws, reads, upserts } = makeWorld(world);

    await hydrateMacroTypeDeps(host, ENTRY, resolveId, ws);

    expect(upserts).toContain("/proj/src/types.d.ts");
    expect(upserts).toContain("/proj/src/more.d.ts");

    // Second transform of another importer: the per-host memo means the dep
    // files are NOT re-read or re-upserted.
    const readsBefore = reads.length;
    const upsertsBefore = upserts.length;
    await hydrateMacroTypeDeps(host, ENTRY, resolveId, ws);
    expect(upserts.length).toBe(upsertsBefore);
    expect(reads.filter((r) => r === "/proj/src/more.d.ts").length).toBe(1);
    expect(reads.length).toBeLessThanOrEqual(readsBefore + 1); // entry re-probe only

    // Watcher eviction re-opens exactly the evicted entry.
    evictHydratedPath(host, "/proj/src/more.d.ts");
    await hydrateMacroTypeDeps(host, ENTRY, resolveId, ws);
    expect(reads.filter((r) => r === "/proj/src/more.d.ts").length).toBe(2);
  });

  it("closure never walks bare npm package imports (node_modules stays cold)", async () => {
    const resolveId = async (source: string) =>
      source === "./types"
        ? "/proj/src/types.d.ts"
        : source === "some-pkg"
          ? "/proj/node_modules/some-pkg/dist/index.d.ts"
          : null;
    const { host, ws, upserts } = makeWorld({
      [ENTRY]: {
        source: "<template/>",
        analysis: {
          macroTypeDeps: [{ typeName: "Props", importSource: "./types", macroKind: "defineProps" }],
        },
      },
      "/proj/src/types.d.ts": {
        source: "import type { P } from 'some-pkg';\nexport interface Props { p: P }",
        analysis: { imports: [{ source: "some-pkg" }] },
      },
      "/proj/node_modules/some-pkg/dist/index.d.ts": { source: "export interface P {}" },
    });

    await hydrateMacroTypeDeps(host, ENTRY, resolveId, ws);

    expect(upserts).toContain("/proj/src/types.d.ts");
    expect(upserts).not.toContain("/proj/node_modules/some-pkg/dist/index.d.ts");
  });

  it("alias-resolved project-local deps hydrate like relative ones", async () => {
    const resolveId = async (source: string) =>
      source === "./types"
        ? "/proj/src/types.d.ts"
        : source === "@/base"
          ? "/proj/src/base.ts"
          : null;
    const { host, ws, upserts } = makeWorld({
      [ENTRY]: {
        source: "<template/>",
        analysis: {
          macroTypeDeps: [{ typeName: "Props", importSource: "./types", macroKind: "defineProps" }],
        },
      },
      "/proj/src/types.d.ts": {
        source: "import type { B } from '@/base';\nexport interface Props extends B {}",
        analysis: { imports: [{ source: "@/base" }] },
      },
      "/proj/src/base.ts": { source: "export interface B { b: string }", analysis: {} },
    });

    await hydrateMacroTypeDeps(host, ENTRY, resolveId, ws);

    expect(upserts).toContain("/proj/src/base.ts");
  });

  it("intermediate .vue macro type deps upsert as SFC and their closure walks", async () => {
    const resolveId = async (source: string) =>
      source === "./Base.vue"
        ? "/proj/src/Base.vue"
        : source === "@/Primitive"
          ? "/proj/src/Primitive.ts"
          : null;
    const { host, ws, upserts } = makeWorld({
      [ENTRY]: {
        source: "<template/>",
        analysis: {
          macroTypeDeps: [
            { typeName: "BaseProps", importSource: "./Base.vue", macroKind: "defineProps" },
          ],
        },
      },
      "/proj/src/Base.vue": {
        source: "<script setup lang='ts'>import type { P } from '@/Primitive'</script>",
        analysis: { imports: [{ source: "@/Primitive" }] },
      },
      "/proj/src/Primitive.ts": { source: "export interface P { as?: string }", analysis: {} },
    });

    await hydrateMacroTypeDeps(host, ENTRY, resolveId, ws);

    expect(upserts).toContain("/proj/src/Base.vue");
    // The reka/radix heritage chain: the .vue dep's @/ alias import loads.
    expect(upserts).toContain("/proj/src/Primitive.ts");
  });
});

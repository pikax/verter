/**
 * Demand-driven contract of `TypeDependencyHydrator`.
 *
 * Uses a MOCK host (no native binding): the contract under test is which
 * files the hydrator reads/upserts/walks, not the host's type resolution.
 *
 * Pins:
 * - the macro-demand gate (no macroTypeDeps -> zero hydration work);
 * - the per-host memo (a dependency file is read/walked once across
 *   carriers, not once per importing SFC);
 * - watcher invalidation re-opening a single entry;
 * - the closure never walking bare npm packages (node_modules-resolved
 *   imports inside hydrated type files stay untouched);
 * - alias-resolved project-local deps hydrating like relative ones.
 */
import { describe, it, expect } from "vitest";
import type { VerterHost } from "@verter/native";
import type { HostCanonicalId } from "./host-session/identity";
import { createPipelineCounters } from "./pipeline/counters";
import { DependencyRouteTable } from "./pipeline/route-table";
import { TypeDependencyHydrator } from "./pipeline/type-hydration";

interface MockFile {
  source: string;
  analysis?: object;
}

function makeWorld(files: Record<string, MockFile>) {
  const reads: string[] = [];
  const upserts: string[] = [];
  const kinds = new Map<string, string | undefined>();
  const tracked = new Set<string>();
  const analyses = new Map<string, object>();
  for (const [path, file] of Object.entries(files)) {
    if (file.analysis) analyses.set(path, file.analysis);
  }

  const host = {
    getAnalysis(path: string): string | null {
      const analysis = analyses.get(path);
      return analysis ? JSON.stringify(analysis) : null;
    },
    resolve(path: string) {
      return { canonicalId: path, existsInHost: tracked.has(path) };
    },
    upsert(entry: { inputId: string; source: string; fileKind?: string }) {
      upserts.push(entry.inputId);
      kinds.set(entry.inputId, entry.fileKind);
      tracked.add(entry.inputId);
      return { canonicalId: entry.inputId };
    },
    remove(path: string): void {
      tracked.delete(path);
    },
    setImportDependencies(): void {},
  } as unknown as VerterHost;

  const has = (path: string) => Object.prototype.hasOwnProperty.call(files, path);
  // The workspace resolver answers a file that exists: the path the bundler named,
  // or a relative specifier read from the importer's directory.
  const access = {
    readFile: async (path: string) => {
      reads.push(path);
      return files[path]?.source ?? null;
    },
    resolveImport: async (importer: string, specifier: string) => {
      if (specifier.startsWith("/")) return has(specifier) ? specifier : null;
      const joined = new URL(specifier, `file://${importer}`).pathname;
      return [joined, `${joined}.ts`, `${joined}.d.ts`].find(has) ?? null;
    },
    isPackageBacked: (canonicalId: string) => canonicalId.includes("/node_modules/"),
  };
  const counters = createPipelineCounters();
  const hydrator = new TypeDependencyHydrator(
    host,
    access,
    new DependencyRouteTable(host, counters),
    counters,
  );

  return { host, hydrator, reads, upserts, kinds };
}

const ENTRY = "/proj/src/App.vue";

describe("TypeDependencyHydrator demand-driven contract", () => {
  it("hydrates NOTHING when the analysis demands no macro type deps", async () => {
    const { hydrator, reads, upserts } = makeWorld({
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

    await hydrator.hydrateCarrier(ENTRY as HostCanonicalId, null);

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
    const { hydrator, reads, upserts } = makeWorld(world);

    await hydrator.hydrateCarrier(ENTRY as HostCanonicalId, resolveId);

    expect(upserts).toContain("/proj/src/types.d.ts");
    expect(upserts).toContain("/proj/src/more.d.ts");

    // Second transform of another importer: the per-host memo means the dep
    // files are NOT re-read or re-upserted.
    const readsBefore = reads.length;
    const upsertsBefore = upserts.length;
    await hydrator.hydrateCarrier(ENTRY as HostCanonicalId, resolveId);
    expect(upserts.length).toBe(upsertsBefore);
    expect(reads.filter((r) => r === "/proj/src/more.d.ts").length).toBe(1);
    expect(reads.length).toBe(readsBefore);

    // Watcher invalidation re-opens exactly the invalidated file: it is read
    // again, its importer is not.
    expect(hydrator.invalidate("/proj/src/more.d.ts")).toBe(true);
    await hydrator.hydrateCarrier(ENTRY as HostCanonicalId, resolveId);
    expect(reads.filter((r) => r === "/proj/src/more.d.ts").length).toBe(2);
    expect(reads.filter((r) => r === "/proj/src/types.d.ts").length).toBe(1);
  });

  it("closure never walks bare npm package imports (node_modules stays cold)", async () => {
    const resolveId = async (source: string) =>
      source === "./types"
        ? "/proj/src/types.d.ts"
        : source === "some-pkg"
          ? "/proj/node_modules/some-pkg/dist/index.d.ts"
          : null;
    const { hydrator, upserts } = makeWorld({
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

    await hydrator.hydrateCarrier(ENTRY as HostCanonicalId, resolveId);

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
    const { hydrator, upserts } = makeWorld({
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

    await hydrator.hydrateCarrier(ENTRY as HostCanonicalId, resolveId);

    expect(upserts).toContain("/proj/src/base.ts");
  });

  it("intermediate .vue macro type deps are registered and their closure walks", async () => {
    const resolveId = async (source: string) =>
      source === "./Base.vue"
        ? "/proj/src/Base.vue"
        : source === "@/Primitive"
          ? "/proj/src/Primitive.ts"
          : null;
    const { hydrator, upserts, kinds } = makeWorld({
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

    await hydrator.hydrateCarrier(ENTRY as HostCanonicalId, resolveId);

    expect(upserts).toContain("/proj/src/Base.vue");
    // The host classifies each file from its path: no request states a kind.
    expect([...kinds.values()].every((kind) => kind === undefined)).toBe(true);
    // The reka/radix heritage chain: the .vue dep's @/ alias import loads.
    expect(upserts).toContain("/proj/src/Primitive.ts");
  });

  // Regression guard: a path-alias specifier (`@/…`) resolves PER
  // PACKAGE. Two packages that both import `@/Primitive` resolve it — through
  // the importer-aware resolve hook — to two DIFFERENT files, and both must
  // hydrate. An importer-agnostic cache keyed on the bare specifier would let
  // package B warm-hit package A's file and silently drop B's own heritage.
  it("resolves the same @/ alias per-package (no importer-agnostic warm hit)", async () => {
    // Importer-aware hook: `@/Primitive` and `./types` resolve relative to the
    // importing package, exactly like a per-package tsconfig `paths` alias.
    const resolveId = async (source: string, importer: string) => {
      const pkg = importer.includes("/pkgA/") ? "pkgA" : "pkgB";
      if (source === "./types") return `/${pkg}/src/types.d.ts`;
      if (source === "@/Primitive") return `/${pkg}/src/Primitive.ts`;
      return null;
    };

    const { hydrator, upserts } = makeWorld({
      "/pkgA/src/App.vue": {
        source: "<template/>",
        analysis: {
          macroTypeDeps: [{ typeName: "Props", importSource: "./types", macroKind: "defineProps" }],
        },
      },
      "/pkgA/src/types.d.ts": {
        source: "import type { P } from '@/Primitive';\nexport interface Props extends P {}",
        analysis: { imports: [{ source: "@/Primitive" }] },
      },
      "/pkgA/src/Primitive.ts": { source: "export interface P { a: string }", analysis: {} },
      "/pkgB/src/App.vue": {
        source: "<template/>",
        analysis: {
          macroTypeDeps: [{ typeName: "Props", importSource: "./types", macroKind: "defineProps" }],
        },
      },
      "/pkgB/src/types.d.ts": {
        source: "import type { P } from '@/Primitive';\nexport interface Props extends P {}",
        analysis: { imports: [{ source: "@/Primitive" }] },
      },
      "/pkgB/src/Primitive.ts": { source: "export interface P { b: number }", analysis: {} },
    });

    // Hydrate package A first, then package B, against the SAME host memo.
    await hydrator.hydrateCarrier("/pkgA/src/App.vue" as HostCanonicalId, resolveId);
    await hydrator.hydrateCarrier("/pkgB/src/App.vue" as HostCanonicalId, resolveId);

    // Each package's OWN Primitive must be hydrated — not A's for both.
    expect(upserts).toContain("/pkgA/src/Primitive.ts");
    expect(upserts).toContain("/pkgB/src/Primitive.ts");
  });

  // Regression guard: the vue closure walk must follow PLAIN value
  // imports, not only `import type` ones. An intermediate `.vue` whose
  // heritage base arrives via a plain `import { Base }` (no `type` keyword,
  // verbatimModuleSyntax off) and which declares no own defineProps still
  // contributes `Base` to a downstream `defineProps<X>()`. Filtering the
  // closure to type-only imports would drop `base.ts` and under-hydrate.
  it("walks plain (non-type) heritage imports inside an intermediate .vue closure", async () => {
    const resolveId = async (source: string) =>
      source === "./Base.vue"
        ? "/proj/src/Base.vue"
        : source === "./base"
          ? "/proj/src/base.ts"
          : null;
    const { hydrator, upserts } = makeWorld({
      [ENTRY]: {
        source: "<template/>",
        analysis: {
          macroTypeDeps: [{ typeName: "X", importSource: "./Base.vue", macroKind: "defineProps" }],
        },
      },
      "/proj/src/Base.vue": {
        // Plain value import (no `type` keyword) feeding an exported heritage
        // interface; the .vue itself has NO defineProps macro of its own.
        source:
          "<script setup lang='ts'>import { Base } from './base'\nexport interface X extends Base {}</script>",
        analysis: { imports: [{ source: "./base" }] },
      },
      "/proj/src/base.ts": { source: "export class Base { a = 1 }", analysis: {} },
    });

    await hydrator.hydrateCarrier(ENTRY as HostCanonicalId, resolveId);

    expect(upserts).toContain("/proj/src/Base.vue");
    // The plain-import heritage base must be hydrated for downstream props.
    expect(upserts).toContain("/proj/src/base.ts");
  });
});

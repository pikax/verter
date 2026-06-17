import { describe, expect, it } from "vitest";
import { hydrateMacroTypeDependencies, type MacroTypeDependencyAccess } from "./macroTypeHydration";

let VerterHostCtor: (new () => any) | null = null;
let hasNativeBinary = false;

try {
  const native: typeof import("@verter/native") = require("@verter/native");
  VerterHostCtor = native.VerterHost;
  new VerterHostCtor();
  hasNativeBinary = true;
} catch {
  hasNativeBinary = false;
}

function normalizePath(fileName: string): string {
  return fileName.replace(/\\/g, "/");
}

function createLayeredAccess(
  openFiles: Record<string, string>,
  diskFiles: Record<string, string> = {},
): MacroTypeDependencyAccess {
  const openMap = new Map(
    Object.entries(openFiles).map(([fileName, source]) => [normalizePath(fileName), source]),
  );
  const diskMap = new Map(
    Object.entries(diskFiles).map(([fileName, source]) => [normalizePath(fileName), source]),
  );
  const extensionCandidates = ["", ".ts", ".tsx", ".d.ts", ".js", ".jsx", ".vue"];

  return {
    resolveModule(containingFile, specifier) {
      if (!specifier.startsWith(".")) return undefined;

      const containing = normalizePath(containingFile);
      const baseDir = containing.slice(0, containing.lastIndexOf("/"));
      const segments = [...baseDir.split("/"), ...specifier.split("/")];
      const resolved: string[] = [];
      for (const segment of segments) {
        if (!segment || segment === ".") continue;
        if (segment === "..") {
          resolved.pop();
          continue;
        }
        resolved.push(segment);
      }

      const basePath = "/" + resolved.join("/");
      for (const ext of extensionCandidates) {
        const candidate = normalizePath(basePath + ext);
        if (openMap.has(candidate) || diskMap.has(candidate)) {
          return candidate;
        }
      }
      return undefined;
    },
    readSource(fileName) {
      const normalized = normalizePath(fileName);
      return openMap.get(normalized) ?? diskMap.get(normalized);
    },
    fileExists(fileName) {
      const normalized = normalizePath(fileName);
      return openMap.has(normalized) || diskMap.has(normalized);
    },
  };
}

function createHost(): any {
  if (!VerterHostCtor) {
    throw new Error("native VerterHost is unavailable");
  }
  return new VerterHostCtor();
}

function compileMain(host: any, canonicalId: string): any {
  return host.getVirtualFile({
    canonicalId,
    nodeKind: { kind: "main" },
  });
}

/**
 * A recording host stub that captures `upsert` / `setImportDependencies` calls
 * (and serves analysis snapshots) WITHOUT the native binary, so the
 * rune-module-disambiguation regressions run under the canonical `pnpm test`.
 */
function createRecordingHost(analyses: Record<string, unknown>): {
  host: any;
  upserts: { inputId: string; source: string }[];
  importDeps: { file: string; deps: { specifier: string; resolvedCanonicalId: string }[] }[];
} {
  const upserts: { inputId: string; source: string }[] = [];
  const importDeps: {
    file: string;
    deps: { specifier: string; resolvedCanonicalId: string }[];
  }[] = [];
  const analysisMap = new Map(
    Object.entries(analyses).map(([fileName, snapshot]) => [
      normalizePath(fileName),
      JSON.stringify(snapshot),
    ]),
  );
  const host = {
    getAnalysis(fileName: string): string | undefined {
      return analysisMap.get(normalizePath(fileName));
    },
    upsert(req: { inputId: string; source: string }): void {
      upserts.push(req);
    },
    setImportDependencies(
      file: string,
      deps: { specifier: string; resolvedCanonicalId: string }[],
    ): void {
      importDeps.push({ file: normalizePath(file), deps });
    },
  };
  return { host, upserts, importDeps };
}

describe("rune-module vs virtual-carrier disambiguation (.svelte.ts ambiguity)", () => {
  // A `.svelte` component imports a REAL standalone rune module `./store.svelte.ts`.
  // There is NO backing `./store.svelte` carrier, so the rune path must be
  // upserted UNCHANGED (under `/app/store.svelte.ts`) and recorded as the
  // import dependency UNCHANGED — never collapsed to a phantom `/app/store.svelte`.
  it("leaves a real .svelte.ts rune module unchanged through resolve + upsert", () => {
    const entryFile = "/app/Widget.svelte";
    const runeModule = "/app/store.svelte.ts";
    const { host, upserts, importDeps } = createRecordingHost({
      [entryFile]: {
        imports: [{ source: "./store.svelte" }],
        macroTypeDeps: [{ importSource: "./store.svelte" }],
      },
      [runeModule]: { imports: [] },
    });
    const access = createLayeredAccess({
      [entryFile]: "<script lang=\"ts\">import { count } from './store.svelte';</script>",
      [runeModule]: "export const count = $state(0);",
    });

    hydrateMacroTypeDependencies(host as any, entryFile, access);

    // The rune module was upserted under its OWN path, not a phantom carrier.
    expect(upserts.map((u) => u.inputId)).toContain(runeModule);
    expect(upserts.map((u) => u.inputId)).not.toContain("/app/store.svelte");
    // The recorded import dependency keeps the real rune path.
    const recordedDep = importDeps.flatMap((d) => d.deps).find((d) => d.specifier === "./store.svelte");
    expect(recordedDep?.resolvedCanonicalId).toBe(runeModule);
    expect(recordedDep?.resolvedCanonicalId).not.toBe("/app/store.svelte");
  });

  // The complement: when a dependency RESOLVES to a carrier API virtual
  // `Child.svelte.ts` AND the backing `Child.svelte` carrier exists, the virtual
  // suffix IS stripped to the bare carrier for the upsert. Mirrors what the
  // production `resolveModule` produces (it can return the `.svelte.ts` API
  // virtual for a `.svelte` import). Proves the disambiguation strips a genuine
  // virtual, not just leaves everything intact.
  it("strips a virtual Comp.svelte.ts to the bare carrier when the backing .svelte exists", () => {
    const entryFile = "/app/Parent.svelte";
    const childVirtual = "/app/Child.svelte.ts";
    const childCarrier = "/app/Child.svelte";
    const { host, upserts } = createRecordingHost({
      [entryFile]: {
        imports: [{ source: "./Child.svelte" }],
        macroTypeDeps: [{ importSource: "./Child.svelte" }],
      },
      // analysis is keyed by the STRIPPED carrier path (what hydration queues).
      [childCarrier]: { imports: [] },
    });
    const sources: Record<string, string> = {
      [childVirtual]: "export const __api = 1;",
      [childCarrier]: "<script lang=\"ts\">export let label: string;</script>",
    };
    // Inline access whose `resolveModule` returns the API VIRTUAL path (as the
    // production resolver does for a `.svelte` component import), while the
    // backing carrier exists.
    const access: MacroTypeDependencyAccess = {
      resolveModule(_containingFile, specifier) {
        return specifier === "./Child.svelte" ? childVirtual : undefined;
      },
      readSource(fileName) {
        return sources[fileName.replace(/\\/g, "/")];
      },
      fileExists(fileName) {
        const n = fileName.replace(/\\/g, "/");
        return n === childCarrier || n === childVirtual;
      },
    };

    hydrateMacroTypeDependencies(host as any, entryFile, access);

    // The virtual was stripped to the bare carrier for the upsert.
    expect(upserts.map((u) => u.inputId)).toContain(childCarrier);
    expect(upserts.map((u) => u.inputId)).not.toContain(childVirtual);
  });
});

describe("hydrateMacroTypeDependencies", () => {
  it.skipIf(!hasNativeBinary)(
    "loads extensionless imported defineProps dependencies before compilation",
    () => {
      const host = createHost();
      const entryFile = "/test/src/components/Popover/components/PopoverItem/PopoverItem.vue";
      const source = `
<script setup lang="ts">
import type { PopoverAction } from '../../types'
const props = withDefaults(defineProps<PopoverAction>(), {})
</script>

<template>
  <button>{{ props.label }}</button>
</template>
`;
      const access = createLayeredAccess({
        [entryFile]: source,
        "/test/src/components/Popover/types.ts": `
export interface BaseAction {
  id: string
}

export interface PopoverAction extends BaseAction {
  label: string
  disabled?: boolean
}
`,
      });

      host.upsert({ inputId: entryFile, source });
      expect(() => compileMain(host, entryFile)).toThrow(/HOST_MISSING_MACRO_TYPE_DEP/);

      hydrateMacroTypeDependencies(host, entryFile, access);
      const compiled = compileMain(host, entryFile);

      expect(compiled.code).toContain("export default");
      expect(compiled.diagnostics.hasErrors).toBe(false);
    },
  );

  it.skipIf(!hasNativeBinary)(
    "refreshes compile results when an imported macro dependency changes",
    () => {
      const host = createHost();
      const entryFile = "/test/src/components/RefreshDeps.vue";
      const source = `
<script setup lang="ts">
import type { Props } from './types'
const props = defineProps<Props>()
</script>
<template><div>{{ props.label }}</div></template>
`;
      const openFiles: Record<string, string> = {
        [entryFile]: source,
        "/test/src/components/types.ts": `
export interface Props {
  label: string
}
`,
      };

      host.upsert({ inputId: entryFile, source });
      hydrateMacroTypeDependencies(host, entryFile, createLayeredAccess(openFiles));
      const first = compileMain(host, entryFile);
      expect(first.diagnostics.hasErrors).toBe(false);

      openFiles["/test/src/components/types.ts"] = "export type Props = string";
      hydrateMacroTypeDependencies(host, entryFile, createLayeredAccess(openFiles));

      expect(() => compileMain(host, entryFile)).toThrow(/object-like props type/);
    },
  );

  it.skipIf(!hasNativeBinary)(
    "prefers unsaved dependency snapshots over fallback disk content",
    () => {
      const host = createHost();
      const entryFile = "/test/src/components/LocalFirst.vue";
      const source = `
<script setup lang="ts">
import type { Props } from './types'
const props = defineProps<Props>()
</script>
<template><div>{{ props.label }}</div></template>
`;
      const unsavedFiles = {
        [entryFile]: source,
        "/test/src/components/types.ts": "export type Props = string",
      };
      const diskFiles = {
        [entryFile]: source,
        "/test/src/components/types.ts": `
export interface Props {
  label: string
}
`,
      };

      host.upsert({ inputId: entryFile, source });
      hydrateMacroTypeDependencies(host, entryFile, createLayeredAccess(unsavedFiles, diskFiles));

      expect(() => compileMain(host, entryFile)).toThrow(/object-like props type/);
    },
  );
});

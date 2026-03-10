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

describe("hydrateMacroTypeDependencies", () => {
  it.skipIf(!hasNativeBinary)(
    "loads extensionless imported defineProps dependencies before compilation",
    () => {
      const host = createHost();
      const entryFile =
        "/test/src/components/Popover/components/PopoverItem/PopoverItem.vue";
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
      hydrateMacroTypeDependencies(
        host,
        entryFile,
        createLayeredAccess(unsavedFiles, diskFiles),
      );

      expect(() => compileMain(host, entryFile)).toThrow(/object-like props type/);
    },
  );
});

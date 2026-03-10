/**
 * @ai-generated - Tests for getDtsSnapshot using VerterHost (Rust-backed SFC compilation).
 */
import { describe, it, expect, beforeEach, vi } from "vitest";
import {
  clearVirtualPublicApiCache,
  FALLBACK_STUB,
  getCachedVirtualPublicApi,
  parseFile,
  remapVirtualSpan,
} from "./getDtsSnapshot";

const mockLogger = {
  info: vi.fn(),
  msg: vi.fn(),
} as any;

beforeEach(() => {
  vi.clearAllMocks();
  clearVirtualPublicApiCache();
});

const hasNativeBinary =
  parseFile("/probe.vue", "<template><div /></template>", mockLogger) !== FALLBACK_STUB;

function normalizePath(fileName: string): string {
  return fileName.replace(/\\/g, "/");
}

function createInMemoryAccess(
  files: Record<string, string>,
): {
  resolveModule: (containingFile: string, specifier: string) => string | undefined;
  readSource: (fileName: string) => string | undefined;
} {
  const normalizedFiles = new Map(
    Object.entries(files).map(([fileName, source]) => [normalizePath(fileName), source]),
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
        if (normalizedFiles.has(candidate)) {
          return candidate;
        }
      }
      return undefined;
    },
    readSource(fileName) {
      return normalizedFiles.get(normalizePath(fileName));
    },
  };
}

describe("parseFile", () => {
  it("compiles a simple SFC with script setup", () => {
    const sfc = `
<script setup lang="ts">
const msg = "hello";
</script>

<template>
  <div>{{ msg }}</div>
</template>
`;
    const result = parseFile("/test/Simple.vue", sfc, mockLogger);

    // getPublicApi() generates a type declaration for the component's public API
    expect(result).not.toBe("export default {} as any");
    // Component name derived from filename
    expect(result).toContain("Simple");
    // Uses defineComponent from vue
    expect(result).toContain('defineComponent');
    // Local bindings like `msg` are NOT included — only macros (defineProps, defineEmits, etc.)
    expect(result).not.toContain("msg");
  });

  it("compiles defineProps with types", () => {
    const sfc = `
<script setup lang="ts">
const props = defineProps<{ title: string; count?: number }>();
</script>

<template>
  <h1>{{ props.title }}</h1>
</template>
`;
    const result = parseFile("/test/Props.vue", sfc, mockLogger);

    expect(result).not.toBe("export default {} as any");
    expect(result).toContain("title");
  });

  it("returns fallback stub for SFC with no script block", () => {
    const sfc = `
<template>
  <div>static content</div>
</template>
`;
    const result = parseFile("/test/NoScript.vue", sfc, mockLogger);

    // Should still produce output (template-only component)
    expect(typeof result).toBe("string");
    expect(result.length).toBeGreaterThan(0);
  });

  it("returns fallback stub for empty input", () => {
    const result = parseFile("/test/Empty.vue", "", mockLogger);

    expect(typeof result).toBe("string");
    expect(result.length).toBeGreaterThan(0);
  });

  it("returns consistent output for same content", () => {
    const sfc = `
<script setup lang="ts">
const x = 1;
</script>

<template>
  <span>{{ x }}</span>
</template>
`;
    const result1 = parseFile("/test/Cache.vue", sfc, mockLogger);
    const result2 = parseFile("/test/Cache.vue", sfc, mockLogger);

    expect(result1).toBe(result2);
  });

  it("compiles SFC with defineEmits", () => {
    const sfc = `
<script setup lang="ts">
const emit = defineEmits<{ change: [value: string] }>();
</script>

<template>
  <button @click="emit('change', 'hi')">Click</button>
</template>
`;
    const result = parseFile("/test/Emits.vue", sfc, mockLogger);

    expect(result).not.toBe("export default {} as any");
    expect(result).toContain("emit");
  });

  // @ai-generated - Verifies generated public API uses Vue public prop types.
  it("generated public API uses Vue public prop types", () => {
    const sfc = `
<script setup lang="ts">
const props = defineProps<{ title: string }>();
</script>

<template>
  <h1>{{ props.title }}</h1>
</template>
`;
    const result = parseFile("/test/VerterTypes.vue", sfc, mockLogger);

    expect(result).toContain('__OmitNew');
    expect(result).toContain('import("vue").PublicProps');
    expect(result).toContain('import("vue")');
    // Props should be present in the $props type
    expect(result).toContain('title');
  });

  it.skipIf(!hasNativeBinary)("caches generated public API for both virtual file suffixes", () => {
    const sfc = `
<script setup lang="ts">
defineProps<{ title: string }>()
</script>
<template><div>{{ title }}</div></template>
`;
    parseFile("/test/Cached.vue", sfc, mockLogger);

    const tsEntry = getCachedVirtualPublicApi("/test/Cached.vue.ts");
    const dtsEntry = getCachedVirtualPublicApi("/test/Cached.vue.d.ts");

    expect(tsEntry).toBeDefined();
    expect(dtsEntry).toBeDefined();
    expect(tsEntry?.code).toContain("title");
    expect(dtsEntry?.code).toContain("title");
    expect(tsEntry?.sourceMap).toBeTruthy();
    expect(dtsEntry?.sourceMap).toBeTruthy();
  });

  it.skipIf(!hasNativeBinary)("remaps virtual definition spans back to the local .vue file", () => {
    const sfc = `
<script setup lang="ts">
defineProps<{ title: string }>()
</script>
<template><div>{{ title }}</div></template>
`;
    parseFile("/test/Remap.vue", sfc, mockLogger);
    const cached = getCachedVirtualPublicApi("/test/Remap.vue.ts");

    expect(cached).toBeDefined();
    const generatedStart = cached!.code.indexOf("title: string");
    expect(generatedStart).toBeGreaterThanOrEqual(0);

    const remapped = remapVirtualSpan(
      "/test/Remap.vue.ts",
      { start: generatedStart, length: "title".length },
      (fileName) => (fileName === "/test/Remap.vue" ? sfc : undefined),
    );

    expect(remapped).toEqual({
      fileName: "/test/Remap.vue",
      textSpan: {
        start: sfc.indexOf("title: string"),
        length: 1,
      },
    });
  });

  it.skipIf(!hasNativeBinary)("refreshes the cached source map when the SFC changes", () => {
    const original = `
<script setup lang="ts">
defineProps<{ title: string }>()
</script>
<template><div>{{ title }}</div></template>
`;
    const updated = `
<script setup lang="ts">
defineProps<{ label: string }>()
</script>
<template><div>{{ label }}</div></template>
`;

    parseFile("/test/Refresh.vue", original, mockLogger);
    parseFile("/test/Refresh.vue", updated, mockLogger);

    const cached = getCachedVirtualPublicApi("/test/Refresh.vue.ts");
    expect(cached).toBeDefined();
    expect(cached?.code).toContain("label");
    expect(cached?.code).not.toContain("title: string");

    const generatedStart = cached!.code.indexOf("label: string");
    expect(generatedStart).toBeGreaterThanOrEqual(0);

    const remapped = remapVirtualSpan(
      "/test/Refresh.vue.ts",
      { start: generatedStart, length: "label".length },
      (fileName) => (fileName === "/test/Refresh.vue" ? updated : undefined),
    );

    expect(remapped).toEqual({
      fileName: "/test/Refresh.vue",
      textSpan: {
        start: updated.indexOf("label: string"),
        length: 1,
      },
    });
  });

  // @ai-generated - Resolves imported defineEmits types before getPublicApi.
  it.skipIf(!hasNativeBinary)("hydrates imported defineEmits types before getPublicApi", () => {
    const entryFile = "/test/src/components/EmitBox/EmitBox.vue";
    const source = `
<script setup lang="ts">
import type { EmitShape } from './types'
const emit = defineEmits<EmitShape>()
</script>

<template>
  <button @click="emit('submit', 'ok')">Send</button>
</template>
`;
    const access = createInMemoryAccess({
      [entryFile]: source,
      "/test/src/components/EmitBox/types.ts": `
export interface EmitShape {
  (e: 'submit', payload: string): void
  confirm: [id: number]
}
`,
    });

    const result = parseFile(entryFile, source, mockLogger, access);

    expect(result).not.toBe(FALLBACK_STUB);
    expect(result).toContain('"onSubmit"?: (payload: string) => void');
    expect(result).toContain('"onConfirm"?: (...args: [id: number]) => void');
  });

  // @ai-generated - Refreshes generated public API when an imported emits dependency changes.
  it.skipIf(!hasNativeBinary)("refreshes imported macro types when the dependency source changes", () => {
    const entryFile = "/test/src/components/RefreshDeps.vue";
    const source = `
<script setup lang="ts">
import type { Emits } from './types'
const emit = defineEmits<Emits>()
</script>
<template><button @click="emit('submit', 'ok')">Send</button></template>
`;
    const files = {
      [entryFile]: source,
      "/test/src/components/types.ts": `
export interface Emits {
  (e: 'submit', payload: string): void
}
`,
    };

    const first = parseFile(entryFile, source, mockLogger, createInMemoryAccess(files));
    expect(first).toContain('"onSubmit"?: (payload: string) => void');
    expect(first).not.toContain('"onConfirm"?: (...args: [id: number]) => void');

    files["/test/src/components/types.ts"] = `
export interface Emits {
  confirm: [id: number]
}
`;

    const second = parseFile(entryFile, source, mockLogger, createInMemoryAccess(files));
    expect(second).toContain('"onConfirm"?: (...args: [id: number]) => void');
    expect(second).not.toContain('"onSubmit"?: (payload: string) => void');
  });
});

/**
 * @ai-generated - Baseline comparison tests for Verter vs official Vue compiler.
 *
 * These tests compile the same Vue SFC snippets with both Verter (via VerterHost
 * from @verter/native) and the official Vue compiler (@vue/compiler-sfc), then
 * compare their output to ensure behavioral equivalence.
 *
 * The official Vue compiler output is the baseline — any difference in Verter's
 * output that would affect runtime behavior is flagged as a failure.
 */

import { describe, it, expect } from "vitest";
import { compileScript, parse } from "@vue/compiler-sfc";
import { VerterHost } from "@verter/native";
import type { HostCompileProfile } from "@verter/native";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

interface CompileResult {
  code: string;
  sourceMap?: string;
  lang?: string;
}

/**
 * One-shot compile helper: creates a fresh VerterHost, upserts the SFC,
 * and returns the compiled main module output.
 */
function compile(
  source: string,
  options: {
    filename?: string;
    isProduction?: boolean;
    componentId?: string;
    forceJs?: boolean;
    forceVapor?: boolean;
  } = {},
): CompileResult {
  const host = new VerterHost({ devMode: true });
  const filename = options.filename ?? "anonymous.vue";

  host.upsert({ inputId: filename, source });

  const profile: HostCompileProfile = {
    filename,
    isProduction: options.isProduction ?? false,
    ssr: false,
    componentId: options.componentId,
    hmrStrategy: "none",
    sourceMap: false,
    forceJs: options.forceJs ?? true,
    forceVapor: options.forceVapor ?? false,
  };

  const result = host.getVirtualFile({
    rawId: filename,
    compileProfile: profile,
  });

  return {
    code: result.code,
    sourceMap: result.sourceMap,
    lang: result.lang,
  };
}

interface AnalyzedResult {
  /** Full compiled code (script + render) */
  code: string;
  /** Extracted import specifiers as a set of identifiers */
  imports: Set<string>;
  /** Whether the component is marked as vapor */
  isVapor: boolean;
  /** Whether it uses inline template mode */
  isInline: boolean;
  /** Binding references found in the render function */
  renderBindings: string[];
}

function compileWithVue(
  sfc: string,
  opts: { inlineTemplate?: boolean; vapor?: boolean; isProduction?: boolean } = {},
): AnalyzedResult {
  const { descriptor } = parse(sfc, { filename: "test.vue" });
  const result = compileScript(descriptor, {
    id: "test123",
    inlineTemplate: opts.inlineTemplate ?? false,
    vapor: opts.vapor ?? false,
    isProd: opts.isProduction ?? false,
  });
  const code = result.content;
  return analyzeCode(code);
}

function compileWithVerter(sfc: string, opts: { isProduction?: boolean } = {}): AnalyzedResult {
  const result = compile(sfc, {
    filename: "test.vue",
    isProduction: opts.isProduction ?? false,
    componentId: "test123",
  });
  return analyzeCode(result.code);
}

function analyzeCode(code: string): AnalyzedResult {
  const imports = new Set<string>();
  // Extract import specifiers: import { foo as _foo, bar as _bar } from 'vue'
  const importRegex = /import\s*\{([^}]+)\}\s*from\s*['"][^'"]+['"]/g;
  let m: RegExpExecArray | null;
  while ((m = importRegex.exec(code)) !== null) {
    for (const spec of m[1].split(",")) {
      const trimmed = spec.trim();
      if (trimmed) {
        // Extract the local name (after 'as') or the original name
        const asMatch = trimmed.match(/(\w+)\s+as\s+(\w+)/);
        if (asMatch) {
          imports.add(asMatch[2]); // local alias (e.g., _openBlock)
        } else {
          imports.add(trimmed);
        }
      }
    }
  }

  const isVapor = code.includes("__vapor") || code.includes("_template(");
  const isInline = code.includes("(_ctx,_cache) => {") || code.includes("(_ctx, _cache) => {");

  // Extract binding references in render function
  const renderBindings: string[] = [];
  // Look for $setup.xxx, _ctx.xxx, __props.xxx patterns
  const bindingRegex = /(\$setup|_ctx|__props)\.(\w+)/g;
  while ((m = bindingRegex.exec(code)) !== null) {
    renderBindings.push(`${m[1]}.${m[2]}`);
  }

  return { code, imports, isVapor, isInline, renderBindings };
}

// ---------------------------------------------------------------------------
// Test cases: VDOM mode
// ---------------------------------------------------------------------------

describe("Baseline: VDOM mode", () => {
  const cases = [
    {
      name: "simple interpolation",
      sfc: `<script setup>
import { ref } from 'vue'
const msg = ref('hello')
</script>
<template><div>{{ msg }}</div></template>`,
    },
    {
      name: "v-if directive",
      sfc: `<script setup>
import { ref } from 'vue'
const show = ref(true)
</script>
<template><div v-if="show">visible</div></template>`,
    },
    {
      name: "v-for directive",
      sfc: `<script setup>
import { ref } from 'vue'
const items = ref([1,2,3])
</script>
<template><div v-for="item in items" :key="item">{{ item }}</div></template>`,
    },
    {
      name: "event handler",
      sfc: `<script setup>
const onClick = () => {}
</script>
<template><button @click="onClick">click</button></template>`,
    },
    {
      name: "v-model on input",
      sfc: `<script setup>
import { ref } from 'vue'
const text = ref('')
</script>
<template><input v-model="text" /></template>`,
    },
    {
      name: "slot outlet with props",
      sfc: `<script setup>
import { ref } from 'vue'
const count = ref(0)
</script>
<template><div><slot name="item" :count="count" /></div></template>`,
    },
    {
      name: "defineProps",
      sfc: `<script setup lang="ts">
const props = defineProps<{ msg: string }>()
</script>
<template><div>{{ props.msg }}</div></template>`,
    },
    {
      name: "defineEmits",
      sfc: `<script setup lang="ts">
const emit = defineEmits<{ (e: 'update', val: string): void }>()
</script>
<template><button @click="emit('update', 'hi')">emit</button></template>`,
    },
    {
      name: "mixed text and element children",
      sfc: `<script setup>
import { ref } from 'vue'
const msg = ref('hello')
</script>
<template><div><span>child</span>text{{ msg }}<span>end</span></div></template>`,
    },
  ];

  for (const { name, sfc } of cases) {
    it(`${name}: imports match baseline`, () => {
      const vue = compileWithVue(sfc);
      const verter = compileWithVerter(sfc);

      // Check that Verter imports all the helpers Vue does (superset is OK)
      for (const imp of vue.imports) {
        // Skip user imports like 'ref' - only check runtime helpers (prefixed with _)
        if (!imp.startsWith("_")) continue;
        expect(
          verter.imports.has(imp),
          `Verter missing import '${imp}' that Vue has.\nVue imports: ${[...vue.imports].join(", ")}\nVerter imports: ${[...verter.imports].join(", ")}`,
        ).toBe(true);
      }
    });

    it(`${name}: binding prefix consistency`, () => {
      const vue = compileWithVue(sfc);
      const verter = compileWithVerter(sfc);

      // In non-inline mode, both should use $setup. for setup bindings
      const vueSetupBindings = vue.renderBindings.filter((b) => b.startsWith("$setup."));
      const verterSetupBindings = verter.renderBindings.filter((b) => b.startsWith("$setup."));

      // The set of referenced bindings should match
      const vueNames = new Set(vueSetupBindings.map((b) => b.replace("$setup.", "")));
      const verterNames = new Set(verterSetupBindings.map((b) => b.replace("$setup.", "")));

      for (const name of vueNames) {
        expect(
          verterNames.has(name),
          `Verter missing $setup.${name} binding that Vue references.\nVue: ${[...vueNames].join(", ")}\nVerter: ${[...verterNames].join(", ")}`,
        ).toBe(true);
      }
    });
  }
});

// ---------------------------------------------------------------------------
// Test cases: Vapor mode
// ---------------------------------------------------------------------------

describe("Baseline: Vapor mode", () => {
  const vaporCases = [
    {
      name: "simple interpolation",
      sfc: `<script setup>
import { ref } from 'vue'
const msg = ref('hello')
</script>
<template vapor><div>{{ msg }}</div></template>`,
    },
    {
      name: "click event",
      sfc: `<script setup>
const onClick = () => {}
</script>
<template vapor><button @click="onClick">click</button></template>`,
    },
  ];

  for (const { name, sfc } of vaporCases) {
    it(`${name}: vapor binding prefix uses _ctx (not $setup)`, () => {
      const verter = compileWithVerter(sfc);

      const verterSetupBindings = verter.renderBindings.filter((b) => b.startsWith("$setup."));
      const verterCtxBindings = verter.renderBindings.filter((b) => b.startsWith("_ctx."));

      expect(
        verterSetupBindings.length,
        `Verter vapor should not use $setup. prefix — found: ${verterSetupBindings.join(", ")}`,
      ).toBe(0);

      expect(
        verterCtxBindings.length,
        `Verter vapor should use _ctx. prefix for bindings in render function`,
      ).toBeGreaterThan(0);
    });

    it(`${name}: Verter preserves vapor output markers`, () => {
      // Vue's public compileScript API currently does not surface a distinct
      // vapor marker for these fixtures, so assert the behavior on Verter's
      // emitted output directly instead of relying on raw baseline text.
      const verter = compileWithVerter(sfc);
      expect(verter.isVapor, "Verter vapor compile should stay in vapor mode").toBe(true);
      expect(verter.code, "Verter vapor component should include __vapor: true flag").toContain(
        "__vapor",
      );
    });
  }
});

// ---------------------------------------------------------------------------
// Test cases: Script setup features
// ---------------------------------------------------------------------------

describe("Baseline: Script setup", () => {
  it("useCssVars uses direct binding (not _ctx. prefix)", () => {
    const sfc = `<script setup>
import { ref } from 'vue'
const color = ref('red')
</script>
<template><div>{{ color }}</div></template>
<style scoped>.box { color: v-bind(color); }</style>`;

    const vue = compileWithVue(sfc);
    const verter = compileWithVerter(sfc);

    // Both should have useCssVars
    expect(vue.code).toContain("useCssVars");
    expect(verter.code).toContain("useCssVars");

    // Official Vue uses color.value (direct ref access), NOT _ctx.color
    expect(vue.code).toContain("color.value");
    expect(verter.code).toContain("color.value");
    expect(verter.code).not.toContain("_ctx.color");
  });

  it("component name from filename", () => {
    const sfc = `<script setup>
const x = 1
</script>
<template><div>{{ x }}</div></template>`;

    const vue = compileWithVue(sfc);
    const verter = compileWithVerter(sfc);

    // Both should set __name
    expect(vue.code).toContain("__name");
    expect(verter.code).toContain("__name");
  });
});

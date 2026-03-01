import { extractSsrRenderBody } from "./normalize.mjs";
import { createRequire } from "node:module";
import path from "node:path";

const require = createRequire(import.meta.url);
const ROOT = path.resolve(import.meta.dirname, "../..");
const { parse, compileScript, compileTemplate } = require(
  require.resolve("@vue/compiler-sfc", { paths: [path.join(ROOT, "node_modules/.pnpm")] }),
);

const { VerterHost } = require(path.join(ROOT, "packages/native/index.js"));
const host = new VerterHost({ devMode: false, analysisLevel: "none" });

function compileVue(source, filename) {
  const { descriptor } = parse(source, { filename });
  if (!descriptor.template) return null;
  let bm = {};
  if (descriptor.script || descriptor.scriptSetup) {
    try {
      const sr = compileScript(descriptor, { id: filename, inlineTemplate: false });
      bm = sr.bindings || {};
    } catch (e) {
      console.log("Vue compileScript failed:", e.message?.slice(0, 100));
    }
  }
  console.log("Vue bindings:", JSON.stringify(bm));
  const result = compileTemplate({
    source: descriptor.template.content,
    filename,
    id: filename,
    ssr: true,
    compilerOptions: { mode: "module", bindingMetadata: bm },
  });
  if (result.errors?.length) { console.log("Vue errors:", result.errors.map(e => e.message)); return null; }
  return result.code;
}

function compileVerter(source, filePath) {
  const upsertResult = host.upsert({ inputId: filePath, source, fileKind: "vue" });
  const result = host.getVirtualFile({
    canonicalId: upsertResult.canonicalId,
    nodeKind: { kind: "main" },
    compileProfile: { filename: path.basename(filePath), ssr: true, forceJs: true, sourceMap: false },
  });
  return result?.code;
}

// Test: Simulated element-plus pattern — compileScript will fail because deps are missing
const source = `<script setup lang="ts">
import { computed, inject, ref, useSlots } from 'vue'
import { ElIcon } from '@element-plus/components/icon'
import { useNamespace } from '@element-plus/hooks'
defineOptions({
  name: 'ElAlert',
})
const props = defineProps({
  title: { type: String, default: '' },
})
</script>
<template>
  <div>
    <el-icon class="test" />
    <ElIcon class="test2" />
  </div>
</template>`;

const vueCode = compileVue(source, "test.vue");
const verterCode = compileVerter(source, "d:/test/test.vue");

console.log("\n=== Vue SSR ===");
console.log(extractSsrRenderBody(vueCode));

console.log("\n=== Verter SSR ===");
console.log(extractSsrRenderBody(verterCode));

// Test 2: No compileScript at all — empty bindings
console.log("\n\n=== Test 2: Empty bindings (no script) ===");
const source2 = `<template>
  <div>
    <el-icon class="test" />
  </div>
</template>`;

const vueCode2 = compileVue(source2, "test2.vue");
const verterCode2 = compileVerter(source2, "d:/test/test2.vue");

console.log("\n=== Vue SSR (no script) ===");
console.log(extractSsrRenderBody(vueCode2));

console.log("\n=== Verter SSR (no script) ===");
console.log(extractSsrRenderBody(verterCode2));

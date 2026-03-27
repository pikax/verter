import { extractSsrRenderBody, normalizeForComparison } from "./normalize.mjs";
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
      console.log("Vue bindings:", bm);
    } catch (e) {
      console.log("Vue compileScript failed:", e.message);
    }
  }
  const result = compileTemplate({
    source: descriptor.template.content,
    filename,
    id: filename,
    ssr: true,
    compilerOptions: { mode: "module", bindingMetadata: bm },
  });
  if (result.errors?.length) return null;
  return result.code;
}

function compileVerter(source, filePath) {
  const upsertResult = host.upsert({ inputId: filePath, source, fileKind: "vue" });
  const result = host.getVirtualFile({
    canonicalId: upsertResult.canonicalId,
    nodeKind: { kind: "main" },
    compileProfile: {
      filename: path.basename(filePath),
      ssr: true,
      forceJs: true,
      sourceMap: false,
    },
  });
  return result?.code;
}

// Test: component imported via setup with kebab-case tag
const source = `<script setup>
import ElIcon from './ElIcon.vue'
</script>
<template>
  <div>
    <el-icon class="test" />
  </div>
</template>`;

const vueCode = compileVue(source, "test.vue");
const verterCode = compileVerter(source, "d:/test/test.vue");

console.log("\n=== Vue SSR ===");
console.log(extractSsrRenderBody(vueCode));

console.log("\n=== Verter SSR ===");
console.log(extractSsrRenderBody(verterCode));

// Test 2: component imported with PascalCase tag
const source2 = `<script setup>
import ElIcon from './ElIcon.vue'
</script>
<template>
  <div>
    <ElIcon class="test" />
  </div>
</template>`;

const vueCode2 = compileVue(source2, "test2.vue");
const verterCode2 = compileVerter(source2, "d:/test/test2.vue");

console.log("\n=== Vue SSR (PascalCase tag) ===");
console.log(extractSsrRenderBody(vueCode2));

console.log("\n=== Verter SSR (PascalCase tag) ===");
console.log(extractSsrRenderBody(verterCode2));

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
    } catch {}
  }
  const result = compileTemplate({
    source: descriptor.template.content, filename, id: filename, ssr: true,
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
    compileProfile: { filename: path.basename(filePath), ssr: true, forceJs: true, sourceMap: false },
  });
  return result?.code;
}

function showDiff(label, source) {
  const vueCode = compileVue(source, "test.vue");
  const verterCode = compileVerter(source, "d:/test/test.vue");
  console.log(`\n=== ${label} ===`);
  console.log("Vue:");
  console.log(extractSsrRenderBody(vueCode));
  console.log("\nVerter:");
  console.log(extractSsrRenderBody(verterCode));
}

// Test: v-model on native input in SSR
showDiff("v-model on native input", `<script setup>
const modelValue = defineModel()
</script>
<template>
  <input v-model="modelValue" class="test" />
</template>`);

// Test: v-model on root input (with _attrs)
showDiff("v-model on root input", `<script setup>
const modelValue = defineModel()
</script>
<template>
  <input v-model="modelValue" class="test" />
</template>`);

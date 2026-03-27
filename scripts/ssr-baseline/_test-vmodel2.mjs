import { extractSsrRenderBody, normalizeForComparison } from "./normalize.mjs";
import { createRequire } from "node:module";
import path from "node:path";
import fs from "node:fs";

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
  return compileTemplate({
    source: descriptor.template.content,
    filename,
    id: filename,
    ssr: true,
    compilerOptions: { mode: "module", bindingMetadata: bm },
  });
}

function compileVerter(source, filePath) {
  const upsertResult = host.upsert({ inputId: filePath, source, fileKind: "vue" });
  return host.getVirtualFile({
    canonicalId: upsertResult.canonicalId,
    nodeKind: { kind: "main" },
    compileProfile: {
      filename: path.basename(filePath),
      ssr: true,
      forceJs: true,
      sourceMap: false,
    },
  });
}

// v-model + explicit @update:model-value handler
const source = `<script setup>
import Comp from './Comp.vue'
const val = ref('')
function onInput(v) { console.log(v) }
</script>
<template>
  <div>
    <Comp v-model="val" @update:model-value="onInput" />
  </div>
</template>`;

const vueResult = compileVue(source, "test.vue");
const verterResult = compileVerter(source, "d:/test/test.vue");

console.log("=== Vue SSR ===");
console.log(extractSsrRenderBody(vueResult.code));

console.log("\n=== Verter SSR ===");
console.log(extractSsrRenderBody(verterResult.code));

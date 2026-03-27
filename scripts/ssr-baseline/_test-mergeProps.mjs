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

function showDiff(label, source) {
  const filename = "test.vue";
  const filePath = "d:/test/" + filename;
  const vueCode = compileVue(source, filename);
  const verterCode = compileVerter(source, filePath);

  const vueBody = extractSsrRenderBody(vueCode);
  const verterBody = extractSsrRenderBody(verterCode);

  console.log(`\n=== ${label} ===`);
  console.log("Vue raw:");
  console.log(vueBody);
  console.log("\nVerter raw:");
  console.log(verterBody);
}

// Test: Root element is a component that receives _attrs
const source1 = `<script setup>
import { MyComp } from './comp'
</script>
<template>
  <MyComp class="mb-2" @click="handler">
    <span>hello</span>
  </MyComp>
</template>`;
showDiff("Root component with class + event", source1);

// Test: Root element is a div with attrs passthrough
const source2 = `<template>
  <div class="container" :style="style">
    <span>hello</span>
  </div>
</template>`;
showDiff("Root div with class + style", source2);

// Test: The specific pattern from the report: _mergeProps with _ctx.$attrs
const source3 = `<script setup>
import { MyComp } from './comp'
</script>
<template>
  <MyComp class="mb-2" inputAlignRight modelValue="x" />
</template>`;
showDiff("Component with multiple props", source3);

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
  console.log("Vue:");
  console.log(vueBody);
  console.log("\nVerter:");
  console.log(verterBody);

  const vueNorm = normalizeForComparison(vueBody);
  const verterNorm = normalizeForComparison(verterBody);
  console.log("\nMatch:", vueNorm === verterNorm);
  if (vueNorm !== verterNorm) {
    let diffAt = 0;
    for (let i = 0; i < Math.min(vueNorm.length, verterNorm.length); i++) {
      if (vueNorm[i] !== verterNorm[i]) {
        diffAt = i;
        break;
      }
    }
    console.log("First diff at:", diffAt);
    console.log("Vue:    " + vueNorm.substring(Math.max(0, diffAt - 20), diffAt + 60));
    console.log("Verter: " + verterNorm.substring(Math.max(0, diffAt - 20), diffAt + 60));
  }
}

// Test 1: v-bind="$attrs" on a nested component
showDiff(
  "v-bind=$attrs on component",
  `<script setup>
import Comp from './Comp.vue'
</script>
<template>
  <div>
    <Comp v-bind="$attrs" class="mb-2" :title="msg" />
  </div>
</template>`,
);

// Test 2: v-bind="$attrs" on a native element
showDiff(
  "v-bind=$attrs on element",
  `<template>
  <div>
    <input v-bind="$attrs" class="input" />
  </div>
</template>`,
);

// Test 3: v-bind="$attrs" on root element component
showDiff(
  "v-bind=$attrs on root component",
  `<script setup>
import Comp from './Comp.vue'
</script>
<template>
  <Comp v-bind="$attrs" class="mb-2" :title="msg" />
</template>`,
);

// Test 4: v-bind="obj" spread (not $attrs)
showDiff(
  "v-bind=obj spread",
  `<script setup>
import Comp from './Comp.vue'
const obj = { a: 1, b: 2 }
</script>
<template>
  <div>
    <Comp v-bind="obj" class="mb-2" />
  </div>
</template>`,
);

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
  const response = host.compileRequest(upsertResult.canonicalId, {
    framework: "vue",
    identity: {
      filename: path.basename(filePath),
      isProduction: false,
      forceJs: true,
    },
    products: [{ kind: "runtimeServer", runtimeSourceMap: false }],
    options: {
      backend: "inferred",
      ssr: true,
      isCustomElement: [],
      babelParserPlugins: [],
    },
  });
  const runtime = response.products.find((p) => p.kind === "runtimeServer");
  return runtime?.nodes.find((n) => n.node.kind === "main")?.code;
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
}

// Test 1: v-model on a component
showDiff(
  "v-model on component",
  `<script setup>
import Comp from './Comp.vue'
const val = ref('')
</script>
<template>
  <div>
    <Comp v-model="val" class="mb-2" />
  </div>
</template>`,
);

// Test 2: v-model with custom prop name
showDiff(
  "v-model with custom prop name",
  `<script setup>
import Comp from './Comp.vue'
const val = ref('')
</script>
<template>
  <div>
    <Comp v-model:title="val" class="mb-2" />
  </div>
</template>`,
);

// Test 3: Multiple v-models
showDiff(
  "Multiple v-models",
  `<script setup>
import Comp from './Comp.vue'
const a = ref('')
const b = ref('')
</script>
<template>
  <div>
    <Comp v-model="a" v-model:title="b" class="mb-2" />
  </div>
</template>`,
);

// Test 4: v-model with modifiers
showDiff(
  "v-model with modifiers",
  `<script setup>
import Comp from './Comp.vue'
const val = ref('')
</script>
<template>
  <div>
    <Comp v-model.trim.lazy="val" class="mb-2" />
  </div>
</template>`,
);

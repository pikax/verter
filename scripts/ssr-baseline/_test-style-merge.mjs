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

// Test 1: v-show + :style on same element
showDiff("v-show + :style", `<template>
  <div>
    <span v-show="visible" :style="customStyle">text</span>
  </div>
</template>`);

// Test 2: v-show + static style on same element
showDiff("v-show + static style", `<template>
  <div>
    <span v-show="visible" style="color: red">text</span>
  </div>
</template>`);

// Test 3: v-show + :style array + static style
showDiff("v-show + :style + static style", `<template>
  <div>
    <span v-show="visible" :style="[customStyle, anotherStyle]" style="color: red">text</span>
  </div>
</template>`);

// Test 4: element-plus badge pattern — :style + conditional v-show
showDiff("badge pattern", `<script setup>
const hidden = ref(false)
const content = ref(5)
const isDot = ref(false)
const style = ref({})
const badgeClass = ref('')
</script>
<template>
  <div>
    <sup v-show="!hidden && (content || isDot)" :style="style" :class="['badge', badgeClass]">text</sup>
  </div>
</template>`);

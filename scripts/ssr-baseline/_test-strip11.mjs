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

// Look at a simple component with just default slot (single slot, no named)
// that should be trivially matching
const source1 = `<script setup>
import { MyComp } from './comp'
</script>
<template>
  <MyComp title="hello">
    <span>{{ msg }}</span>
  </MyComp>
</template>`;

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
    compileProfile: { filename: path.basename(filePath), ssr: true, forceJs: true, sourceMap: false },
  });
  return result?.code;
}

function compare(label, source) {
  const filename = "test.vue";
  const filePath = "d:/test/" + filename;
  const vueCode = compileVue(source, filename);
  const verterCode = compileVerter(source, filePath);

  const vueBody = extractSsrRenderBody(vueCode);
  const verterBody = extractSsrRenderBody(verterCode);

  const vueNorm = normalizeForComparison(vueBody);
  const verterNorm = normalizeForComparison(verterBody);

  if (vueNorm === verterNorm) {
    console.log(`[MATCH] ${label}`);
    return;
  }

  console.log(`\n[MISMATCH] ${label}`);
  // Find first diff
  let diffAt = 0;
  for (let i = 0; i < Math.max(vueNorm.length, verterNorm.length); i++) {
    if (vueNorm[i] !== verterNorm[i]) { diffAt = i; break; }
  }
  console.log("First diff at char", diffAt);
  console.log("Vue:    ..." + vueNorm.substring(Math.max(0, diffAt - 40), diffAt + 60) + "...");
  console.log("Verter: ..." + verterNorm.substring(Math.max(0, diffAt - 40), diffAt + 60) + "...");

  // Show raw bodies (first 500 chars)
  console.log("\n--- Vue raw (first 400) ---");
  console.log(vueBody?.substring(0, 400));
  console.log("\n--- Verter raw (first 400) ---");
  console.log(verterBody?.substring(0, 400));
}

// Test 1: Simple single default slot
compare("Simple default slot", source1);

// Test 2: Named slots
const source2 = `<script setup>
import { MyComp } from './comp'
</script>
<template>
  <MyComp>
    <template #header>
      <h1>Header</h1>
    </template>
    <template #default>
      <p>{{ msg }}</p>
    </template>
  </MyComp>
</template>`;
compare("Named slots (header + default)", source2);

// Test 3: Nested components with slots
const source3 = `<script setup>
import { Outer, Inner } from './comp'
</script>
<template>
  <Outer title="x">
    <Inner spacing="sm">
      <span>{{ msg }}</span>
    </Inner>
  </Outer>
</template>`;
compare("Nested components with slots", source3);

// Test 4: Named + default mixed
const source4 = `<script setup>
import { Card } from './comp'
</script>
<template>
  <Card title="x">
    <template #title>
      <h1>Title</h1>
    </template>
    <p>Default content</p>
  </Card>
</template>`;
compare("Named + implicit default mixed", source4);

// Test 5: Deeply nested with multiple slots
const source5 = `<script setup>
import { Outer, Middle, Inner } from './comp'
</script>
<template>
  <Outer>
    <Middle>
      <template #header>
        <h1>Header</h1>
      </template>
      <Inner>
        <span>{{ msg }}</span>
      </Inner>
    </Middle>
  </Outer>
</template>`;
compare("Deeply nested (3 levels)", source5);

import { extractSsrRenderBody, normalizeForComparison } from "./normalize.mjs";
import { createRequire } from "node:module";
import path from "node:path";
import fs from "node:fs";

const require = createRequire(import.meta.url);
const ROOT = path.resolve(import.meta.dirname, "../..");

const { VerterHost } = require(path.join(ROOT, "packages/native/index.js"));
const host = new VerterHost({ devMode: false, analysisLevel: "none" });

// Minimal reproduction: component with v-bind="$attrs" and other props
const source = `<script setup>
import Comp from './Comp.vue'
const rules = [() => true]
</script>
<template>
  <div class="wrapper">
    <Comp
      :modelValue="modelValue"
      v-bind="$attrs"
      class="mb-2"
      type="number"
      :rules="rules"
      inputAlignRight
      @input="val => emit('update:modelValue', val)"
    >
      <template #header>
        <span>Header</span>
      </template>
      <template #footer>
        <span>Footer</span>
      </template>
    </Comp>
  </div>
</template>`;

const filePath = "d:/test/test.vue";
const upsertResult = host.upsert({ inputId: filePath, source, fileKind: "vue" });
const result = host.getVirtualFile({
  canonicalId: upsertResult.canonicalId,
  nodeKind: { kind: "main" },
  compileProfile: { filename: "test.vue", ssr: true, forceJs: true, sourceMap: false },
});

console.log("Verter SSR output:");
console.log(result.code);

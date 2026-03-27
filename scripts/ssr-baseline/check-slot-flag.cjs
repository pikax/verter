const { compileTemplate, parse } = require("@vue/compiler-sfc");

const source = `<template>
  <Outer v-slot="{ state }">
    <Inner>text</Inner>
  </Outer>
</template>`;

const { descriptor } = parse(source);
const { code } = compileTemplate({
  source: descriptor.template.content,
  filename: "test.vue",
  id: "test",
  ssr: true,
});

console.log("=== SSR Output ===");
console.log(code);

// Also check non-SSR (VDOM) output
const { code: vdomCode } = compileTemplate({
  source: descriptor.template.content,
  filename: "test.vue",
  id: "test",
  ssr: false,
});

console.log("\n=== VDOM Output ===");
console.log(vdomCode);

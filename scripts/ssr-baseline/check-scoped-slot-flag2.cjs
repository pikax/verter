const { compileTemplate, parse } = require('@vue/compiler-sfc');

// Check VDOM output for scoped slot nesting
const source = `<template>
  <a-list :grid="{ gutter: 16, column: 4 }" :data-source="data">
    <template #renderItem="{ item }">
      <a-list-item>
        <a-card :title="item.title">Card content</a-card>
      </a-list-item>
    </template>
  </a-list>
</template>
<script setup>
const data = [{ title: 'Title 1' }]
</script>`;

const { descriptor } = parse(source);

// SSR output (has VDOM fallback)
const { code: ssrCode } = compileTemplate({
  source: descriptor.template.content,
  filename: 'test.vue',
  id: 'test',
  ssr: true,
});

console.log('=== SSR Output ===');
console.log(ssrCode);

// VDOM output (for reference)
const { code: vdomCode } = compileTemplate({
  source: descriptor.template.content,
  filename: 'test.vue',
  id: 'test',
  ssr: false,
});

console.log('\n=== VDOM Output ===');
console.log(vdomCode);

// Find all slot flags in both
const ssrFlags = [...ssrCode.matchAll(/_:\s*(\d+)\s*(\/\*\s*(\w+)\s*\*\/)?/g)];
const vdomFlags = [...vdomCode.matchAll(/_:\s*(\d+)\s*(\/\*\s*(\w+)\s*\*\/)?/g)];
console.log('\nSSR slot flags:', ssrFlags.map(m => m[1]+'/'+m[3]).join(', '));
console.log('VDOM slot flags:', vdomFlags.map(m => m[1]+'/'+m[3]).join(', '));

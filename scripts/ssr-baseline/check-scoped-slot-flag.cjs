const { compileTemplate, parse } = require('@vue/compiler-sfc');

// Test 1: Named scoped slot — does Vue mark it DYNAMIC?
const test1 = `<template>
  <Comp>
    <template #default="{ item }">
      <div>{{ item }}</div>
    </template>
  </Comp>
</template>`;

// Test 2: Named scoped slot without params — should be STABLE
const test2 = `<template>
  <Comp>
    <template #default>
      <div>Static content</div>
    </template>
  </Comp>
</template>`;

// Test 3: Scoped slot with v-for in content
const test3 = `<template>
  <Comp>
    <template #renderItem="{ item }">
      <div>{{ item.title }}</div>
    </template>
  </Comp>
</template>`;

for (const [name, source] of [['scoped', test1], ['no-params', test2], ['scoped-v-for-name', test3]]) {
  const { descriptor } = parse(source);
  const { code } = compileTemplate({
    source: descriptor.template.content,
    filename: 'test.vue',
    id: 'test',
    ssr: true,
  });

  // Find slot flags
  const flags = [...code.matchAll(/_:\s*(\d+)\s*(\/\*\s*(\w+)\s*\*\/)?/g)];
  console.log(`${name}: ${flags.map(m => m[1] + '/' + (m[3]||'?')).join(', ')}`);

  // Check for DYNAMIC_SLOTS
  console.log(`  DYNAMIC_SLOTS: ${code.includes('DYNAMIC_SLOTS')}`);
}

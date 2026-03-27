const { compileTemplate, parse } = require("@vue/compiler-sfc");

// TransitionGroup with no tag prop (default "span")
const s1 = `<template>
<TransitionGroup name="fade">
  <div v-for="item in items" :key="item.id">{{ item.name }}</div>
</TransitionGroup>
</template>`;
const { descriptor: d1 } = parse(s1);
const { code: c1 } = compileTemplate({
  source: d1.template.content,
  filename: "test.vue",
  id: "test",
  ssr: true,
});
console.log("=== TransitionGroup (no tag, root) ===");
console.log(c1);

// TransitionGroup nested (not root)
const s2 = `<template>
<div>
  <TransitionGroup name="fade" tag="ol" class="my-list">
    <li v-for="item in items" :key="item.id">{{ item.name }}</li>
  </TransitionGroup>
</div>
</template>`;
const { descriptor: d2 } = parse(s2);
const { code: c2 } = compileTemplate({
  source: d2.template.content,
  filename: "test.vue",
  id: "test",
  ssr: true,
});
console.log('\n=== TransitionGroup (tag="ol", nested) ===');
console.log(c2);

// TransitionGroup with dynamic tag
const s3 = `<template>
<TransitionGroup name="list" :tag="tagName">
  <div v-for="item in items" :key="item.id">{{ item.name }}</div>
</TransitionGroup>
</template>`;
const { descriptor: d3 } = parse(s3);
const { code: c3 } = compileTemplate({
  source: d3.template.content,
  filename: "test.vue",
  id: "test",
  ssr: true,
});
console.log("\n=== TransitionGroup (dynamic tag) ===");
console.log(c3);

// Transition with slot (default slot with v-if)
const s4 = `<template>
<Transition name="fade" mode="out-in">
  <div v-if="show" key="a" class="box">A</div>
  <div v-else key="b" class="box">B</div>
</Transition>
</template>`;
const { descriptor: d4 } = parse(s4);
const { code: c4 } = compileTemplate({
  source: d4.template.content,
  filename: "test.vue",
  id: "test",
  ssr: true,
});
console.log("\n=== Transition with v-if/v-else (root) ===");
console.log(c4);

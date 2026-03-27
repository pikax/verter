const { compileTemplate, parse } = require("@vue/compiler-sfc");

// Check TransitionGroup SSR output
const source1 = `<template>
<TransitionGroup name="list" tag="ul">
  <li v-for="item in items" :key="item.id">{{ item.name }}</li>
</TransitionGroup>
</template>
<script setup>
const items = []
</script>`;

const { descriptor: d1 } = parse(source1);
const { code: ssr1 } = compileTemplate({
  source: d1.template.content,
  filename: "test.vue",
  id: "test",
  ssr: true,
});
console.log("=== TransitionGroup SSR ===");
console.log(ssr1);

// Check Transition SSR output
const source2 = `<template>
<button @click="show = !show">Toggle</button>
<Transition>
  <div v-if="show" class="box">Content</div>
</Transition>
</template>
<script setup>
const show = ref(true)
</script>`;

const { descriptor: d2 } = parse(source2);
const { code: ssr2 } = compileTemplate({
  source: d2.template.content,
  filename: "test.vue",
  id: "test",
  ssr: true,
});
console.log("\n=== Transition SSR ===");
console.log(ssr2);

// Check KeepAlive SSR output
const source3 = `<template>
<KeepAlive>
  <component :is="currentTab" />
</KeepAlive>
</template>
<script setup>
const currentTab = ref('TabA')
</script>`;

const { descriptor: d3 } = parse(source3);
const { code: ssr3 } = compileTemplate({
  source: d3.template.content,
  filename: "test.vue",
  id: "test",
  ssr: true,
});
console.log("\n=== KeepAlive SSR ===");
console.log(ssr3);

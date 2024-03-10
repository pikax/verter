<script lang="ts" setup>
const Supa = "HELLO";
const Items = [
  { name: "1", children: [{ value: "1" }, { value: "2" }] },
  { name: "2", children: [{ value: "1" }, { value: "2" }] },
];

defineExpose({ Items, Supa });
</script>
<template>
  <div v-for="item in Items" :key="item.name">
    <p>{{ Supa }}</p>
    <div v-for="child in item.children" :key="`${item.name}.${child.value}`">
      <p>{{ Supa }}: {{ item.name }} {{ child.value }}</p>
    </div>
  </div>
</template>

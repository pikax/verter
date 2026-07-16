<script setup lang="ts">
import IdeSurfaceChild from "../ide/IdeSurfaceChild.vue";
import SlotTypedHost from "../slots/SlotTypedHost.vue";
import StrictLeaf from "../strict/StrictLeaf.vue";

const label = "multi";
const count = 1;
function onPick(v: string) {
  void v;
}
</script>

<template>
  <StrictLeaf :label="label" />
  <IdeSurfaceChild :label="label" :count="count" @pick="onPick">
    <template #header="{ title }">{{ title }}</template>
  </IdeSurfaceChild>
  <SlotTypedHost>
    <template #header="{ title, count: c }">{{ title }}{{ c }}</template>
    <template #default="{ body }">{{ body }}</template>
  </SlotTypedHost>
</template>

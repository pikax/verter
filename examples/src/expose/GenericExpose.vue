<script setup lang="ts" generic="T extends Record<string, unknown>">
import { ref } from "vue";

// Generic component with defineExpose
const items = ref<T[]>([]);

function addItem(item: T): void {
  items.value.push(item);
}

function getItems(): T[] {
  return items.value;
}

function findItem(predicate: (item: T) => boolean): T | undefined {
  return items.value.find(predicate);
}

defineExpose({
  items,
  addItem,
  getItems,
  findItem,
});
</script>

<template>
  <div>
    <slot :items="items" :add="addItem" />
  </div>
</template>

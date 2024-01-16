<script setup lang="ts" generic="T">
import { computed } from "vue";

const props = defineProps<{
  items: T[];
  getKey: (item: T) => string | number;
  getLabel: (item: T) => string;
}>();

const orderedItems = computed<T[]>(() =>
  props.items.sort((item1, item2) =>
    props.getLabel(item1).localeCompare(props.getLabel(item2))
  )
);

function getItemAtIndex(index: number): T | undefined {
  if (index < 0 || index >= orderedItems.value.length) {
    return undefined;
  }
  return orderedItems.value[index];
}

defineExpose({ getItemAtIndex });
</script>

<template>
  <ol>
    <li v-for="item in orderedItems" :key="getKey(item)">
      {{ getLabel(item) }}
    </li>
  </ol>
</template>

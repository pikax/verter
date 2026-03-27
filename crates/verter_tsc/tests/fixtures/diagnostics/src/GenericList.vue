<script setup lang="ts">
const props = defineProps<{
  items: { id: number }[];
  label?: string;
}>();

const emit = defineEmits<{
  select: [item: { id: number }];
}>();

function onSelect(item: { id: number }) {
  emit("select", item);
}
</script>
<template>
  <ul>
    <li v-for="item in props.items" :key="item.id" @click="onSelect(item)">
      {{ item.id }} {{ label }}
    </li>
  </ul>
</template>

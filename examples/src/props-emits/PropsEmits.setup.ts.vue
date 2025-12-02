<script setup lang="ts">
// defineProps - type-based syntax
const props = defineProps<{
  name: string;
  count: number;
  active?: boolean;
}>();

// defineEmits - type-based syntax
const emit = defineEmits<{
  change: [value: string];
  update: [id: number];
}>();

function handleClick() {
  emit("change", props.name);
  emit("update", props.count);
}
</script>

<template>
  <div>
    <p>Name: {{ props.name }}</p>
    <p>Count: {{ props.count }}</p>
    <p>Active: {{ props.active }}</p>
    <button @click="handleClick">Emit</button>
  </div>
</template>

<script setup lang="ts">
const emit = defineEmits<{
  pick: [value: string];
  close: [];
}>();

function fire() {
  emit("pick", "item");
}
</script>

<template>
  <button type="button" @click="fire">emit</button>
</template>

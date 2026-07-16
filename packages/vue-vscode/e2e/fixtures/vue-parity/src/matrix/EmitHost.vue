<script setup lang="ts">
const emit = defineEmits<{
  pick: [value: string];
  close: [];
}>();
function fire() {
  emit("pick", "x");
  emit("close");
}
</script>

<template>
  <button type="button" @click="fire">emit</button>
</template>

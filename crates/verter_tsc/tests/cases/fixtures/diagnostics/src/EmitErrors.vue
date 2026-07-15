<script setup lang="ts">
const emit = defineEmits<{
  submit: [payload: string];
  count: [n: number];
}>();

// PINNED: line 8, TS2345 — number not assignable to string
emit("submit", 42);
// TS2345 — object not assignable to string
emit("submit", { wrong: true });
// TS2345 — string not assignable to number
emit("count", "not-a-number");
</script>
<template>
  <div />
</template>

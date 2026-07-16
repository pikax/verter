<script setup lang="ts">
/**
 * Typed child surface for negative typecheck fixtures.
 * Props and emits are intentionally strict.
 */
const props = defineProps<{
  count: number;
  title: string;
  enabled?: boolean;
}>();

const emit = defineEmits<{
  pick: [value: string];
  change: [next: number];
  close: [];
}>();

function fire() {
  emit("pick", props.title);
  emit("change", props.count);
  emit("close");
}
</script>

<template>
  <button type="button" @click="fire">{{ title }}:{{ count }}:{{ enabled }}</button>
</template>

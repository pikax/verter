<script setup lang="ts" generic="T extends string | number">
/**
 * Multi-prop linkage: value, format, and change payload all share T.
 */
defineProps<{
  value: T;
  format: (v: T) => string;
  disabled?: boolean;
}>();

const emit = defineEmits<{
  change: [next: T];
}>();

function onInput(raw: string) {
  // call-site still types emit with T
  void raw;
  void emit;
}
</script>

<template>
  <div>
    <span>{{ format(value) }}</span>
    <button type="button" :disabled="disabled" @click="onInput(String(value))">touch</button>
  </div>
</template>

<script setup lang="ts">
/**
 * Declares ONLY `label`. Native root <button> so class/style/data-* fall through
 * correctly — but invented prop names must still be unknown-prop (strict-first).
 */
defineProps<{
  label: string;
}>();
</script>

<template>
  <button type="button">{{ label }}</button>
</template>

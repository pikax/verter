<script setup lang="ts">
// Globally-registered component (see global-components.d.ts) with a CLEAN
// typed prop + event surface (no quoted/hyphenated names). It is NEVER
// imported by a consumer: the tag must type through the synthesized
// GlobalComponents fallback const in every script arm.
defineProps<{
  count?: number;
  onPing?: (payload: { pingCode: string; pingCount: number }) => void;
}>();
</script>

<template>
  <div>global count comp</div>
</template>

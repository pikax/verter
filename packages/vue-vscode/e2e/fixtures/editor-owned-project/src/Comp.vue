<script setup lang="ts">
import type { LabelProps } from "./props";

const { label, count } = defineProps<LabelProps>();

// Deliberate TS2322. The acceptance suite requires this diagnostic to map back
// to this real source line through the exact configured editor project.
const wrong: number = label;
</script>

<template>
  <div>{{ label }} / {{ count }} / {{ wrong }}</div>
</template>

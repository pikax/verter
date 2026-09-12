<script setup lang="ts">
// Fixed workload fixture. Small warm-path carrier: one imported props
// type, one imported value, one emit surface. Frozen bytes.
import type { Labelled, PropsBase } from "../shared/props-base";
import { defaultTheme } from "../shared/theme";

const props = defineProps<PropsBase<Labelled>>();
const emit = defineEmits<{ select: [row: Labelled] }>();

const accent = defaultTheme.accent;
</script>

<template>
  <ul :class="props.density" :style="{ color: accent }">
    <li v-for="row in props.rows" :key="row.id" @click="emit('select', row)">
      {{ row.label }}
    </li>
  </ul>
</template>

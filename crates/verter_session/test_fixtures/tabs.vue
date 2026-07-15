<script setup lang="ts">
import type { TabItem } from './tabs_types';
import { activeIndexFor } from './tabs_helper';

const props = defineProps<{ items: TabItem[]; modelValue?: number }>();
defineEmits<{ 'update:modelValue': [value: number] }>();

const active = activeIndexFor(props.items, props.modelValue);
</script>

<template>
  <div class="tabs">
    <button v-for="(item, i) in items" :key="i" :class="{ active: i === active }">
      {{ item.label }}
    </button>
  </div>
</template>

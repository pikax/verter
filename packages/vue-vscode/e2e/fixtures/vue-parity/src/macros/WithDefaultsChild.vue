<script setup lang="ts">
export interface DefaultsProps {
  title: string;
  size?: number;
}

withDefaults(defineProps<DefaultsProps>(), {
  size: 16,
});
</script>

<template>
  <span>{{ title }}:{{ size }}</span>
</template>

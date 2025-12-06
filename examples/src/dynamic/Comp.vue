<script setup lang="ts">
const props = defineProps<{
  as?: string;
}>();

defineSlots<{
  default: (args: {}) => any[];
}>();
</script>

<template>
  <component :is="as || 'div'">
    <slot />
  </component>
  <component is="div"> </component>
</template>

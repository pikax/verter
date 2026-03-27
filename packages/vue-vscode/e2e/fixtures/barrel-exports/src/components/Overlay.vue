<script setup lang="ts">
defineProps<{
  zIndex?: number;
  duration?: number | string;
  show?: boolean;
  lockScroll?: boolean;
}>();
</script>
<template>
  <div v-if="show" class="overlay">
    <slot />
  </div>
</template>

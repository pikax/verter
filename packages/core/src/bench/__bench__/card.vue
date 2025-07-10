<script setup lang="ts">
defineProps<{
  noPadding?: boolean;
}>();
</script>
<template>
  <div class="rounded shadow-md" :class="{ 'px-8 pt-6 pb-8': !noPadding }">
    <slot />
  </div>
</template>

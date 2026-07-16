<script setup lang="ts">
/**
 * Native-root leaf. Declares only `label`.
 * class/style/data-*/aria-*/native listeners should fall through to <button>.
 */
defineProps<{ label: string }>();
</script>

<template>
  <button type="button">{{ label }}</button>
</template>

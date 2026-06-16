<template>
  <header class="drawer-header">
    <slot name="title">{{ title }}</slot>
  </header>
</template>

<script setup lang="ts">
defineProps<{ title: string }>();
</script>

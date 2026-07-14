<script setup lang="ts">
defineProps<{ heading: string; padded: boolean }>();
</script>
<template>
  <section :class="{ padded }">{{ heading }}</section>
</template>

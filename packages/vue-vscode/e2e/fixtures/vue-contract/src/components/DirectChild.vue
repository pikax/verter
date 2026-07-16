<script setup lang="ts">
defineProps<{ contractProp: string }>();
</script>

<template>
  <strong>{{ contractProp }}</strong>
</template>

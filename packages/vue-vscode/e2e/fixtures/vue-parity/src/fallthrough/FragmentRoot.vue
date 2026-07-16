<script setup lang="ts">
defineProps<{ msg: string }>();
</script>

<template>
  <span>{{ msg }}</span>
  <span>extra</span>
</template>

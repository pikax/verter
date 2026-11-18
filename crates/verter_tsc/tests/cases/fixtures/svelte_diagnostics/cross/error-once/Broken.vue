<script setup lang="ts">
defineProps<{ label: string }>();
const wrong: number = "broken";
</script>

<template>
  <p>{{ label }} {{ wrong }}</p>
</template>

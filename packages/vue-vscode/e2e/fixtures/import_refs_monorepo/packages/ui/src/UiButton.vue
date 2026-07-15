<script setup lang="ts">
defineProps<{ uiButtonOnly: string }>();
</script>
<template>
  <button>{{ uiButtonOnly }}</button>
</template>

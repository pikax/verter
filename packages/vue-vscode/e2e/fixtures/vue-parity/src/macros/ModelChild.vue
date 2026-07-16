<script setup lang="ts">
const open = defineModel<boolean>("open", { required: true });
</script>

<template>
  <div v-if="open">open</div>
</template>

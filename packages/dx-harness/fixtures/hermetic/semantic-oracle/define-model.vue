<template>
  <input type="checkbox" :checked="isOpen" />
</template>

<script setup lang="ts">
const open = defineModel<boolean>();

const isOpen = open.value; // @dx-anchor model.value
</script>

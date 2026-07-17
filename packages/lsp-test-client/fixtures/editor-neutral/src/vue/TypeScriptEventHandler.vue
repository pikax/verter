<script setup lang="ts">
function myClick(e) {
  return e.pointerId;
}
</script>

<template><button @click="myClick">Click</button></template>

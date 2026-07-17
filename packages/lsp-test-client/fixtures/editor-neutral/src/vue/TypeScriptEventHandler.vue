<script setup lang="ts">
function myClick(e) {
  return e.pointerId;
}
</script>

<template><button @pointerdown="myClick">Click</button></template>

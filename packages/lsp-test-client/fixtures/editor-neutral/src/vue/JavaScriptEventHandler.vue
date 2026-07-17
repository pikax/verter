<script setup>
// @ts-expect-error unannotated JavaScript named handlers intentionally require authored JSDoc
function myClick(e) {
  return e.pointerId;
}
</script>

<template><button @pointerdown="myClick">Click</button></template>

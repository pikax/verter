<script setup>
/** @param {PointerEvent} e */
function myClick(e) {
  e.pointerId;
  return e.__verterMissingPointerMember;
}
</script>

<template><button @pointerdown="myClick">Click</button></template>

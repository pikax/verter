<script setup>
/** @param {PointerEvent} e */
function myClick(e) {
  e.pointerId;
  // @ts-expect-error authored PointerEvent has no such member; lax policy suppresses diagnostics
  return e.__verterMissingPointerMember;
}
</script>

<template><button @pointerdown="myClick">Click</button></template>

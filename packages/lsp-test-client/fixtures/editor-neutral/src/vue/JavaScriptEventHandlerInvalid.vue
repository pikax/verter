<script setup>
/** @param {PointerEvent} e */
function myClick(e) {
  e.pointerId;
  // @ts-expect-error PointerEvent has no such member; this must not degrade to any
  return e.__verterMissingPointerMember;
}
</script>

<template><button @pointerdown="myClick">Click</button></template>

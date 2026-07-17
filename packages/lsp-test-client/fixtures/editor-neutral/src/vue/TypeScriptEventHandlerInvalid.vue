<script setup lang="ts">
function myClick(e) {
  // @ts-expect-error inferred PointerEvent has no such member; any would leave this unused
  return e.__verterMissingPointerMember;
}
</script>

<template><button @pointerdown="myClick">Click</button></template>

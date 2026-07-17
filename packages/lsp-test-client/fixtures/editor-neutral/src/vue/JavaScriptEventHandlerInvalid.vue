<script setup>
function myClick(e) {
  return e.__verterMissingPointerMember;
}
</script>

<template><button @click="myClick">Click</button></template>

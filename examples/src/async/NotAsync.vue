<script setup lang="ts">
async function bindGesture(el?: HTMLElement | null) {
  if (!el) {
    return;
  }
  await waitUntilNotBusy();
}
</script>

<template></template>

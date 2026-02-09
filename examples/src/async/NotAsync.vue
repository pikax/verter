<script setup lang="ts">
declare function waitUntilNotBusy(): Promise<void>;
async function bindGesture(el?: HTMLElement | null) {
  if (!el) {
    return;
  }
  await waitUntilNotBusy();
}

function foo() {}
</script>

<template></template>

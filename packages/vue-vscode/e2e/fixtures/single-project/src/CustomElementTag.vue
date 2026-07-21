<script setup lang="ts">
// Web-component regression control: `x-status-badge` is neither imported, nor
// registered in GlobalComponents, nor a native element — it is a custom-element
// tag. It must stay FAIL-OPEN (typed through JSX.IntrinsicElements, `any` under
// Vue's index signature): NO diagnostic on its line, never a false TS2604.
// The kebab-ONLY registered global (`global-count-comp`, no Pascal occurrence
// in this file) must still type through the fail-open kebab fallback const.
const ceReady = "ready";
</script>

<template>
  <div>
    <span>{{ ceReady }}</span>
    <x-status-badge level="3"></x-status-badge>
    <global-count-comp :count="7" />
  </div>
</template>

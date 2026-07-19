<script setup lang="ts">
// Negative control: a component tag that is neither imported, nor registered in
// GlobalComponents, nor a native element. The tag must FAIL CLOSED — a typed
// `unknown` const producing a real JSX diagnostic — never a silent `any`.
const anchorReady = "ready";
</script>

<template>
  <div>
    <span>{{ anchorReady }}</span>
    <TotallyUnknownComp :mystery="1" />
  </div>
</template>

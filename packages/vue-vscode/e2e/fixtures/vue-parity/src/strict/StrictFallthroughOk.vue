<script setup lang="ts">
import StrictLeaf from "./StrictLeaf.vue";
</script>

<template>
  <!-- class/style/data-/aria are accepted via native-root fallthrough (not Volar-loose) -->
  <StrictLeaf
    label="ok"
    class="strict-class"
    style="color: blue"
    data-testid="strict-leaf"
    aria-label="strict"
  />
</template>

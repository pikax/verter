<script setup lang="ts">
const trulyUnused = 1;
const templateOnly = 2;
const cssOnly = "rebeccapurple";
</script>

<template>
  <div class="binding-contract" :data-template-only="templateOnly" />
</template>

<style scoped>
.binding-contract {
  color: v-bind(cssOnly);
}
</style>

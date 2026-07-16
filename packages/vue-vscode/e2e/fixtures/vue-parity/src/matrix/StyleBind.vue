<script setup lang="ts">
const accent = "navy";
const pad = "8px";
</script>

<template>
  <div class="box">styled</div>
</template>

<style scoped>
.box {
  color: v-bind(accent);
  padding: v-bind(pad);
}
</style>

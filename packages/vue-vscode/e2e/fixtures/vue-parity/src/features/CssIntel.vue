<script setup lang="ts">
const chipWidth = 12;
</script>

<template>
  <div class="chip-live ghost-none">chip</div>
</template>

<style scoped>
.chip-live {
  width: v-bind(chipWidth);
  color: #336699;
}
</style>

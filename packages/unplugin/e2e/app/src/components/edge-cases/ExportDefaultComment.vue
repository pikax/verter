<script setup lang="ts">
import { ref } from 'vue'

// Regression: this comment contains "export default" which previously caused
// duplicate export default statements when combined with scoped styles.
// Transform: export default X -> window.__modules__['moduleName'].default = X
const count = ref(0)
const label = 'export default in comment'
</script>

<template>
  <div class="export-comment-box">
    <span data-testid="export-comment-label">{{ label }}</span>
    <span data-testid="export-comment-count">{{ count }}</span>
    <button data-testid="export-comment-btn" @click="count++">+</button>
  </div>
</template>

<style scoped>
.export-comment-box {
  padding: 8px;
  border: 1px solid #ccc;
}
</style>

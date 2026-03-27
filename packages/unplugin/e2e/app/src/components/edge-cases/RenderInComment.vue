<script setup lang="ts">
import { ref } from "vue";

// Regression: this comment contains "function render(...)" which
// should not cause a false positive render attachment in production builds.
// Note: standalone `function render(...)` is NOT transformed here.
const count = ref(0);
const label = "function render in comment";
</script>

<template>
  <div class="render-comment-box">
    <span data-testid="render-comment-label">{{ label }}</span>
    <span data-testid="render-comment-count">{{ count }}</span>
    <button data-testid="render-comment-btn" @click="count++">+</button>
  </div>
</template>

<style scoped>
.render-comment-box {
  padding: 8px;
  border: 1px solid #ccc;
}
</style>
